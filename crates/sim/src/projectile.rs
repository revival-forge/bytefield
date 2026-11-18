//! Projectiles, as the original game moves them: a `Projectile` object with a
//! `PointPhysicsNode` (shells, bullets, rockets) fired by a `FireArms`, its line against the
//! level's objects, the movers and the terrain (`PointResponsePhysics`), and what the hit does
//! to it; or, for grenades, bombs and torpedoes
//! (`setHasPointPhysics 0`), a rigid body that bounces and turns where it lands
//! (The response physics' solve impulse, the `CID_ProjectileTemplate` case).
//!
//! The frame:
//! 1. `handleUpdate`: the time to live (`timeToLive`, then `hasOnTimeEffect`: detonate or vanish)
//! 2. the point physics node's physics update: drag, 4 substeps, gravity for the next frame
//! 3. the response physics' update, point objects first: the step's segment against the
//!    objects (`checkObjectVsObject`) and the terrain and water (`checkVsTerrain`); the hit met
//!    first wins; then `solveImpulse` hands it to `handleCollision`.

use crate::vecmath::{sub, dot};
use std::sync::Arc;

use serde::Serialize;

use crate::contact::{self, Ground, Materials};
use crate::mover::Vehicle;
use crate::obstacle::{self, Obstacle, Obstacles};
use crate::rigid::RigidBody;
use crate::{Sim, World};
use bytefield_world::scene::children;

use crate::vecmath::V3;

/// Faces of this material let projectiles through (`isSolidMaterial`; the material slot of
/// `getDistanceToGeometry` holding 0 skips them).
pub const NON_SOLID_MATERIAL: u32 = 90;
/// The material `checkVsTerrain` gives a crossing of the water surface.
pub const WATER_MATERIAL: u32 = 1;
/// The vehicle id of a rigid projectile's body: a mover it touches (`checkObjectVsObject`) gets
/// a hit from no vehicle, and its `handleCollision` with a projectile is no crash.
pub const PROJECTILE_ID: usize = usize::MAX;

/// Underwater drag factor of the point node (as the rigid one).
pub const UNDERWATER_DRAG: f32 = 25.0;
/// The terrain's distance to geometry lookup takes a hit only if the segment goes into the
/// terrain faster than this (n · dir < −1e-5).
pub const TERRAIN_MIN_APPROACH: f64 = -1e-5;
/// How much `solveImpulse` turns a rigid projectile per unit of the normal along its axes, in
/// degrees (pitch about its x axis by n·z, roll about its z axis by −n·x).
pub const IMPACT_TURN: f32 = 10.0;

fn len2(a: V3) -> f32 {
    dot(a, a)
}

/// `RandomDistribution` (`CRD_NONE/a/b/flag`, `CRD_UNIFORM`…). The sim is deterministic: it
/// takes `CRD_NONE`'s value, the middle of `CRD_UNIFORM`, the mean of the others.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Distribution {
    /// 0 none, 1 uniform, 2 exponential, 3 normal
    pub kind: u8,
    pub a: f32,
    pub b: f32,
}

impl Distribution {
    pub fn parse(s: &str) -> Option<Distribution> {
        let mut it = s.split('/');
        let kind = match it.next()?.trim().to_ascii_uppercase().as_str() {
            "CRD_NONE" => 0,
            "CRD_UNIFORM" => 1,
            "CRD_EXPONENTIAL" => 2,
            "CRD_NORMAL" => 3,
            _ => return None,
        };
        let a = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
        let b = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
        Some(Distribution { kind, a, b })
    }

    /// The value the sim uses.
    pub fn value(&self) -> f32 {
        match self.kind {
            1 => (self.a + self.b) * 0.5,
            _ => self.a,
        }
    }
}

/// What the sim needs of a `ProjectileTemplate` (the engine's defaults
/// and the template's constructor).
#[derive(Debug, Clone, Serialize)]
pub struct ProjectileTemplate {
    pub name: String,
    pub mass: f32,
    /// `drag` (the constructor sets 0)
    pub drag: f32,
    pub gravity_modifier: f32,
    /// `getBoundingRadius` of the object: the drag's cross-section is π r²
    pub radius: f32,
    /// `material`: replaces the projectile's own material in `handleCollision` (−1: none)
    pub material: Option<u32>,
    /// `timeToLive` (default `CRD_UNIFORM/3/0`)
    pub time_to_live: Distribution,
    /// when the time runs out: detonate (else it just vanishes)
    pub has_on_time_effect: bool,
    /// it ends where it hits (detonating)
    pub has_collision_effect: bool,
    /// it ends at its first collision (default 1)
    pub die_after_coll: bool,
    pub die_at_object_hit: bool,
    /// water stops it (default 1); else it goes on under the surface
    pub detonate_on_water: bool,
    pub is_sticky: bool,
    /// `explodeNearEnemyDistance` (proximity fuse, [`crate::explosives::fuse`]; < 0: none)
    pub explode_near_enemy: f32,
    /// `ProximityFusePrimer`: seconds before the fuse is armed (default −1)
    pub proximity_fuse_primer: f32,
    /// a `PointPhysicsNode` (the default) or a rigid body (`setHasPointPhysics 0`)
    pub point_physics: bool,
    /// an `Engine` among its children (DC's `DefenderTOW`): a point projectile's engine pushes it too
    pub engine: bool,
    /// what it does to what it hits
    pub damage: crate::damage::ProjectileDamage,
}

/// What the sim needs of a `FireArmsTemplate`.
#[derive(Debug, Clone, Serialize)]
pub struct Weapon {
    pub name: String,
    /// `projectileTemplate`
    pub projectile: String,
    /// `projectileTemplate` names a template that does not exist (DC_Final's `RocketBlast` -> `Blank_Projectile`, the Sea
    /// Sparrow's launch flash): the original's fire returns at its null projectile once the delay is set, before
    /// `fireBarrel` (the round), the heat, the deviation and the effects. A pack, with none, is not one.
    pub projectile_missing: bool,
    /// `velocity`, m/s along the barrel's z axis (default 200)
    pub velocity: f32,
    /// `projectilePosition`, in the barrel's frame
    pub position: V3,
    /// `addRootSpeed` (default 1): the shot gets the root's speed, plus 2 frames of gravity up
    pub add_root_speed: bool,
    /// `rotationalSpeed`, in the barrel's frame
    pub rotational_speed: V3,
    /// the magazines: `magSize` (30), `numOfMag` (3; −1 endless), `magType` (0;
    /// 1: no magazines), `reloadTime` (1 s), `roundOfFire` (10 a second), `fireDelay` (0),
    /// `fireOnce`, `autoReload`, `autoFire`, `allowReloadOnEmptyClipOnly`
    pub mag_size: i32,
    pub num_of_mag: i32,
    pub mag_type: i32,
    pub reload_time: f32,
    pub round_of_fire: f32,
    pub fire_delay: f32,
    pub fire_once: bool,
    pub auto_reload: bool,
    pub auto_fire: bool,
    pub reload_empty_only: bool,
    /// `changeWeaponWhenNoAmmo`: out of rounds, the soldier takes another one up;
    /// `cantSelectWhenNoAmmo`: out of rounds, it cannot be taken up (the grenades,
    /// charges and mines); `timeToNotAllowChange` (−1): how long before the next shot
    /// is ready the soldier may change weapon or get in (the bolt-action rifles: 0.25)
    pub change_when_empty: bool,
    pub cant_select_empty: bool,
    pub time_to_not_allow_change: f32,
    /// `destroyVehicleWhenNoAmmo`: its last round destroys its PlayerControlObject
    /// (the Wasserfall's warhead); `disableWhenFired`: its dummy projectiles hide from the
    /// shot to the end of the reload (`fireFinished`, `ReloadFinished`; only seen)
    pub destroy_vehicle_when_empty: bool,
    pub disable_when_fired: bool,
    /// `setFireDev max add decrease`, `setMinDev` (degrees)
    pub fire_dev: [f32; 3],
    pub min_dev: f32,
    /// `heatAddWhenFire`, `coolDownPerSec`, `timeDelayOnOverHeat`
    pub heat_add: f32,
    pub cool_down: f32,
    pub overheat_delay: f32,
    /// `velocityDependentOnHeat` (the grenades): the heat is the throw's charge (the
    /// shot's speed × it), not a heating
    pub heat_velocity: bool,
    /// `hideDuringFireTime`
    pub hide_time: f32,
    /// a `HandFireArms`'s own deviation
    pub hand: Option<crate::firearms::HandDeviation>,
    /// `setHasRecoilForce 1`
    pub recoil: Option<crate::firearms::Recoil>,
    /// `itemIndex`: its slot in a soldier's inventory (`c_PIWeapon<n>` raises slot n)
    pub item_index: i32,
    /// `setInputFire`: the key it fires on, `c_PIFire` (8, message 6) or
    /// `c_PIAltFire` (23, message 7)
    pub input_fire: usize,
    /// `fireingForce`: each shot pushes its root back along the barrel (an acceleration at the
    /// weapon's position, the root woken)
    pub firing_force: f32,
    /// `setTracerTemplate <projectile> <distribution>`: after that many shots, one of these
    pub tracer: Option<(String, crate::firearms::Distribution4)>,
    /// `fireInCameraDof`: it fires from its vehicle's camera (position and axes)
    pub fire_in_camera: bool,
    /// `addFireArmsPosition <pos> <rot>`: its barrels, in its frame; a shot leaves from
    /// the active one, then the next; or all at once (see `asynchrony_fire`)
    pub barrels: Vec<V3>,
    /// the second vector of each `addFireArmsPosition`: the barrel's turn (yaw, pitch,
    /// roll, degrees), so each pellet of a shotgun leaves a little apart
    pub barrel_rots: Vec<V3>,
    /// `setAsynchronyFire`: one barrel per shot, in turn; without it a shot fires them all
    pub asynchrony_fire: bool,
    /// `setBlastAmmoCount`: all the barrels fire even with fewer rounds in the magazine
    pub blast_ammo_count: bool,
    /// the zoom: `zoomFov` (−1: none; zoomed, the mouse turns × it),
    /// `altFireOnce` (one toggle per press), `unZoomBetweenFireTime` (> 0: the
    /// zoom goes when the fire key is let go)
    pub zoom_fov: f32,
    pub alt_fire_once: bool,
    pub unzoom_between_fire: f32,
    /// `ejectClipTime`: a human's empty clip is thrown out (message 8) for that long
    pub eject_clip_time: f32,
    /// `ammoType` (0): what a supply depot's `addAmmoType` of that type refills
    pub ammo_type: i32,
}

/// Where and how fast a projectile leaves the barrel.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Shot {
    pub pos: V3,
    pub rows: [V3; 3],
    pub vel: V3,
    pub rotvel: V3,
}

impl Weapon {
    /// The weapon's fire barrel (no barrel list, no deviation): the projectile takes
    /// the barrel's axes `rows` (z: the direction) at `pos` moved by `projectilePosition`; its
    /// speed is `velocity` along z, plus, with `addRootSpeed` and a root that has a physics node,
    /// the root's speed `root_vel` and 2/30 s of gravity upwards (`gravity`: −14.73).
    pub fn fire(&self, rows: [V3; 3], pos: V3, root_vel: Option<V3>, gravity: f32) -> Shot {
        self.fire_charged(rows, pos, root_vel, gravity, None)
    }

    /// The same with `velocityDependentOnHeat`: the speed along z × the charge.
    pub fn fire_charged(&self, rows: [V3; 3], pos: V3, root_vel: Option<V3>, gravity: f32, charge: Option<f32>) -> Shot {
        self.fire_from(rows, pos, self.position, root_vel, gravity, charge)
    }

    /// The same from `p` in the barrel's axes (`projectilePosition`, plus a barrel's own).
    pub fn fire_from(&self, rows: [V3; 3], pos: V3, p: V3, root_vel: Option<V3>, gravity: f32, charge: Option<f32>) -> Shot {
        let (x, y, z) = (rows[0], rows[1], rows[2]);
        let at = [
            z[0] * p[2] + pos[0] + y[0] * p[1] + x[0] * p[0],
            z[1] * p[2] + pos[1] + y[1] * p[1] + x[1] * p[0],
            pos[2] + p[2] * z[2] + p[1] * y[2] + x[2] * p[0],
        ];
        let v = self.velocity;
        let mut vel = [z[0] * v, z[1] * v, z[2] * v];
        if let Some(h) = charge {
            vel = vel.map(|c| c * h);
        }
        if self.add_root_speed
            && let Some(rv) = root_vel
        {
            vel = [rv[0] + vel[0], rv[1] + vel[1], rv[2] + vel[2]];
            vel[1] = (vel[1] as f64 - (2.0 / contact::FPS as f64) * gravity as f64) as f32;
        }
        let r = self.rotational_speed;
        let rotvel = [
            x[0] * r[0] + y[0] * r[1] + z[0] * r[2],
            x[1] * r[0] + y[1] * r[1] + z[1] * r[2],
            r[0] * x[2] + r[1] * y[2] + r[2] * z[2],
        ];
        Shot { pos: at, rows, vel, rotvel }
    }
}

/// A hit, as `solveImpulse` hands it to `handleCollision`.
#[derive(Debug, Clone, Serialize)]
pub struct Impact {
    /// the frame (`Sim::steps`) it happened in
    pub step: u64,
    pub point: V3,
    pub normal: V3,
    /// the surface's material (the face's, the terrain's, or 1 for water)
    pub material: u32,
    /// the object hit: an obstacle's or a vehicle's template; None for terrain and water
    pub object: Option<String>,
    /// the vehicle hit, by index in `Sim::vehicles`
    pub vehicle: Option<usize>,
    pub water: bool,
    /// the projectile's speed relative to what it hit
    pub vel: V3,
    /// whether the projectile ended there
    pub ended: bool,
    /// a soldier's collision bone it hit (its skeleton's index)
    pub bone: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum End {
    /// it hit something and ended there; `detonated`: `hasCollisionEffect`
    Hit { detonated: bool },
    /// stuck to what it hit (`isSticky`)
    Stuck,
    /// its time ran out; `detonated`: `hasOnTimeEffect`
    Expired { detonated: bool },
    /// The projectile's detonate: its proximity fuse or the detonator
    Detonated,
    /// gone without going off: its pool gave its slot to a new shot, or its kit was destroyed
    Removed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Projectile {
    #[serde(skip)]
    pub template: Arc<ProjectileTemplate>,
    pub pos: V3,
    pub rows: [V3; 3],
    pub vel: V3,
    /// acceleration accumulator (gravity of the last frame)
    pub acc: V3,
    /// depth under water this frame (`setUnderWater`: only the frame it crosses the surface)
    pub underwater: f32,
    /// seconds since it was fired (summed in f64: in f32, 90 frames of 1/30 make 2.9999986 and
    /// a 3 s grenade went off a frame after the server's)
    pub age: f64,
    /// its time to live
    pub ttl: f32,
    pub end: Option<End>,
    pub impacts: Vec<Impact>,
    /// rigid projectiles: their body and the parts that touch things
    pub rigid: Option<Box<Vehicle>>,
    /// a point projectile with an engine: its parts, whose pushes go to the point node (the body's own integration is not
    /// used; the physics engine's physics update adds to the root's accumulator, here the point node's)
    pub parts: Option<Box<Vehicle>>,
    /// where it was fired: the damage falls off with the distance from here
    pub start: V3,
    /// the vehicle of the player that fired it (its id): never hit (a soldier's shots)
    pub owner: Option<usize>,
    /// the seat of the owner's weapon that fired it (0 for a soldier's)
    #[serde(skip)]
    pub seat: usize,
    /// a scout camera: the projectile is the camera, the mark is here
    pub scout: Option<ScoutMark>,
    /// when it was seen ended (`Sim::step_ordered`): its place waits [`SPENT_REUSE_AFTER`] before another takes it
    #[serde(skip)]
    pub gone_since: Option<f64>,
}

/// Seconds a spent projectile's place in `Sim.projectiles` waits before [`Sim::add_projectile`] gives it to another (ours,
/// past what reads an index the frame it is made, `fired` and the client's sounds and effects; a scout view
/// checks its marks each frame)
pub const SPENT_REUSE_AFTER: f64 = 10.0;

/// What `placeScoutCamera` writes in the binoculars' projectile before the game's place scout camera
/// lists it: the point the ray met and the scout's player and team.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScoutMark {
    pub target: V3,
    pub player: usize,
    pub team: u32,
}

impl Projectile {
    pub fn new(template: Arc<ProjectileTemplate>, shot: &Shot) -> Projectile {
        let ttl = template.time_to_live.value();
        Projectile { template, pos: shot.pos, rows: shot.rows, vel: shot.vel, acc: [0.0; 3], underwater: 0.0, age: 0.0, ttl, end: None, impacts: Vec::new(), rigid: None, parts: None, start: shot.pos, owner: None, seat: 0, scout: None, gone_since: None }
    }

    pub fn alive(&self) -> bool {
        self.end.is_none()
    }

    /// `handleUpdate` and the timer: the time to live runs out at the start of a frame.
    pub fn update_timer(&mut self, dt: f32) {
        if self.end.is_some() {
            return;
        }
        if self.age >= self.ttl as f64 {
            self.end = Some(End::Expired { detonated: self.template.has_on_time_effect });
            return;
        }
        self.age += dt as f64;
    }

    /// The point physics node's physics update: linear drag a −= π r² `drag`/m · (k·v − wind)
    /// with k from the depth under water (×25 fully under), then 4 substeps of dt/4 (velocity,
    /// then position), empty the accumulator and add gravity × `gravityModifier` for the next
    /// frame. No clamp. In `f32`.
    pub fn integrate(&mut self, dt: f32, gravity: f32, wind: V3) {
        let t = &self.template;
        if t.drag > 0.0 {
            let r = t.radius;
            let f = if r > 0.0 { (self.underwater / r).min(1.0) } else { 0.0 };
            let k = (f as f64 * UNDERWATER_DRAG as f64 + (1.0 - f as f64)) as f32;
            let inv_m = 1.0 / t.mass;
            let pi = std::f32::consts::PI as f64;
            for i in 0..3 {
                let w = (self.vel[i] * k) as f64 - wind[i] as f64;
                self.acc[i] = (self.acc[i] as f64 - inv_m as f64 * w * pi * r as f64 * r as f64 * t.drag as f64) as f32;
            }
        }
        // summing the four v·h at the end is not enough: the server's positions come out
        // exact (shells at 100 m/s, x = 1800..3100) only with the position rounded to f32 at every
        // substep: v·h rounded, then added
        let h = 0.25f32 * dt;
        for _ in 0..4 {
            for i in 0..3 {
                self.vel[i] = (self.acc[i] as f64 * h as f64 + self.vel[i] as f64) as f32;
                self.pos[i] += self.vel[i] * h;
            }
        }
        self.acc = [0.0, (gravity as f64 * t.gravity_modifier as f64) as f32, 0.0];
    }

    /// The point response physics' object vs object check and `checkVsTerrain`: this frame's segment
    /// (back one step of the speed relative to the other object) against the obstacles' and
    /// the vehicles' faces (collision LOD 1, material 90 let through), the terrain and the water
    /// surface. The hit farthest from the end of the segment (met first) wins.
    pub fn collide(&mut self, terrain: &dyn Terrain, obstacles: Obstacles<'_>, vehicles: &[Vehicle], dt: f32) -> Option<Impact> {
        let pos = self.pos;
        let mut best: Option<(f32, Impact)> = None;
        let take = |d2: f32, imp: Impact, best: &mut Option<(f32, Impact)>| {
            if best.as_ref().is_none_or(|b| b.0 < d2) {
                *best = Some((d2, imp));
            }
        };
        // the level's objects (static)
        let dir = [self.vel[0] * dt, self.vel[1] * dt, self.vel[2] * dt];
        let start = sub(pos, dir);
        for o in obstacles.iter().filter(|o| !o.projectiles_pass) {
            if !segment_near_sphere(start, dir, o.center, o.radius + 1.0) {
                continue;
            }
            let Some(mesh) = obstacle::pick_lod(&o.lods, 1) else { continue };
            if let Some(h) = obstacle::segment_on_skipping(mesh, &o.rows, o.pos, o.scale, start, dir, false, Some(NON_SOLID_MATERIAL)) {
                let d2 = len2(sub(h.point, pos));
                take(d2, Impact { step: 0, point: h.point, normal: h.normal, material: h.material, object: Some(o.name.clone()), vehicle: None, water: false, vel: self.vel, ended: false, bone: None }, &mut best);
            }
        }
        // the movers: every part with faces, the segment relative to the root's speed
        for (vi, v) in vehicles.iter().enumerate().filter(|(i, v)| v.in_world() && self.owner != Some(*i)) {
            let body = &v.craft.body;
            let rel = sub(self.vel, body.vel);
            let dir = [rel[0] * dt, rel[1] * dt, rel[2] * dt];
            let start = sub(pos, dir);
            if !segment_near_sphere(start, dir, body.pos, v.radius + 1.0) {
                continue;
            }
            for (h, soldier) in mover_segment(v, start, dir) {
                let d2 = len2(sub(h.point, pos));
                let vo = if soldier { body.vel } else { body.velocity_at(h.point) };
                take(d2, Impact { step: 0, point: h.point, normal: h.normal, material: h.material, object: Some(v.template.clone()), vehicle: Some(vi), water: false, vel: sub(self.vel, vo), ended: false, bone: h.bone }, &mut best);
            }
        }
        // the terrain, then the water surface if the terrain was not hit above it
        let dir = [self.vel[0] * dt, self.vel[1] * dt, self.vel[2] * dt];
        let start = sub(pos, dir);
        let water = terrain.water_level();
        self.underwater = 0.0;
        let mut above_water = false;
        if let Some((p, n)) = terrain.segment(start, dir) {
            let d2 = len2(sub(p, pos));
            take(d2, Impact { step: 0, point: p, normal: n, material: terrain.material(p[0], p[2]), object: None, vehicle: None, water: false, vel: self.vel, ended: false, bone: None }, &mut best);
            above_water = water < p[1];
        }
        if !above_water && pos[1] <= water && water <= start[1] {
            let below = pos[1] - water;
            let f = 1.0 - below / (pos[1] - start[1]);
            let p = [f * dir[0] + start[0], f * dir[1] + start[1], f * dir[2] + start[2]];
            let d2 = len2(sub(p, pos));
            if best.as_ref().is_none_or(|b| b.0 < d2) {
                self.underwater = -below;
                take(d2, Impact { step: 0, point: p, normal: [0.0, 1.0, 0.0], material: WATER_MATERIAL, object: None, vehicle: None, water: true, vel: self.vel, ended: false, bone: None }, &mut best);
            }
        }
        best.map(|b| b.1)
    }

    /// What a line of sight meets first: start → start + dir
    /// against what [`Projectile::collide`] tests (the objects, the movers but `skip`, the terrain); the point.
    pub fn sight(terrain: &dyn Terrain, obstacles: Obstacles<'_>, vehicles: &[Vehicle], start: V3, dir: V3, skip: Option<usize>) -> Option<V3> {
        let mut best: Option<(f32, V3)> = None;
        let mut take = |p: V3| {
            let d2 = len2(sub(p, start));
            if best.is_none_or(|b| d2 < b.0) {
                best = Some((d2, p));
            }
        };
        for o in obstacles.iter().filter(|o| !o.projectiles_pass && segment_near_sphere(start, dir, o.center, o.radius + 1.0)) {
            let Some(mesh) = obstacle::pick_lod(&o.lods, 1) else { continue };
            if let Some(h) = obstacle::segment_on_skipping(mesh, &o.rows, o.pos, o.scale, start, dir, false, Some(NON_SOLID_MATERIAL)) {
                take(h.point);
            }
        }
        for (_, v) in vehicles.iter().enumerate().filter(|(i, v)| v.in_world() && skip != Some(*i) && segment_near_sphere(start, dir, v.craft.body.pos, v.radius + 1.0)) {
            for (h, _) in mover_segment(v, start, dir) {
                take(h.point);
            }
        }
        if let Some((p, _)) = terrain.segment(start, dir) {
            take(p);
        }
        best.map(|b| b.1)
    }

    /// The projectile's collision handling: what a hit does to it. Water lets it through unless
    /// `detonateOnWaterCollision`; on the terrain it ends with `dieAfterColl` or
    /// `hasCollisionEffect`; on an object it sticks (`isSticky`), or ends with
    /// `dieAtObjectHit`, `dieAfterColl` or `hasCollisionEffect`.
    pub fn handle_collision(&mut self, mut hit: Impact) {
        if self.end.is_some() {
            return;
        }
        let t = &self.template;
        if hit.object.is_none() && hit.material == WATER_MATERIAL && hit.water && !t.detonate_on_water {
            return;
        }
        let end = if hit.object.is_none() {
            (t.die_after_coll || t.has_collision_effect).then_some(End::Hit { detonated: t.has_collision_effect })
        } else if t.is_sticky {
            Some(End::Stuck)
        } else {
            (t.die_at_object_hit || t.die_after_coll || t.has_collision_effect).then_some(End::Hit { detonated: t.has_collision_effect })
        };
        hit.ended = end.is_some();
        if let Some(e) = end {
            self.end = Some(e);
            if e == End::Stuck {
                self.pos = hit.point;
                self.vel = [0.0; 3];
            }
        }
        self.impacts.push(hit);
    }

    /// One whole frame of a point projectile alone.
    pub fn step(&mut self, terrain: &dyn Terrain, obstacles: Obstacles<'_>, vehicles: &[Vehicle], step: u64, dt: f32, gravity: f32) {
        self.update_timer(dt);
        if self.end.is_some() {
            return;
        }
        if let Some(v) = self.rigid.as_deref_mut() {
            v.integrate(dt);
            self.pos = v.craft.body.pos;
            self.rows = v.craft.body.rows;
            self.vel = v.craft.body.vel;
            return;
        }
        self.integrate(dt, gravity, [0.0; 3]);
        if let Some(mut hit) = self.collide(terrain, obstacles, vehicles, dt) {
            hit.step = step;
            self.handle_collision(hit);
        }
    }
}

/// Where the segment meets a mover's faces (point, normal, material; `true`: a soldier's
/// collision skeleton, its one hit), else every part's faces that it crosses.
pub(crate) fn mover_segment(v: &Vehicle, start: V3, dir: V3) -> Vec<(MoverHit, bool)> {
    let body = &v.craft.body;
    if let Some(sk) = v.movement.as_ref().and_then(|m| m.skeleton.as_ref()) {
        return sk.segment(&body.rows, body.pos, start, dir).map(|h| (MoverHit { point: h.point, normal: h.normal, material: h.material, bone: Some(h.bone) }, true)).into_iter().collect();
    }
    v.contacts
        .iter()
        .filter_map(|c| {
            let mesh = obstacle::pick_lod(&c.faces, 1)?;
            let (rows, at) = c.frame(body);
            let h = obstacle::segment_on_skipping(mesh, &rows, at, c.scale, start, dir, false, Some(NON_SOLID_MATERIAL))?;
            Some((MoverHit { point: h.point, normal: h.normal, material: h.material, bone: None }, false))
        })
        .collect()
}


pub(crate) struct MoverHit {
    pub point: V3,
    pub normal: V3,
    pub material: u32,
    pub bone: Option<usize>,
}

pub(crate) fn segment_near_sphere(start: V3, dir: V3, c: V3, r: f32) -> bool {
    let l2 = len2(dir);
    let t = if l2 > 0.0 { (dot(sub(c, start), dir) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let p = [start[0] + dir[0] * t, start[1] + dir[1] * t, start[2] + dir[2] * t];
    len2(sub(c, p)) <= r * r
}

/// The terrain as a projectile sees it: a segment test, the material and the water level.
pub trait Terrain {
    /// The terrain's distance to geometry lookup: the first point where start → start + dir goes
    /// into the terrain (from above), and the terrain's normal there; only if the segment's end
    /// is under that plane.
    fn segment(&self, start: V3, dir: V3) -> Option<(V3, V3)>;
    fn material(&self, x: f32, z: f32) -> u32;
    fn water_level(&self) -> f32;
}

impl Terrain for bytefield_world::Level {
    fn segment(&self, start: V3, dir: V3) -> Option<(V3, V3)> {
        terrain_segment(self, start, dir)
    }
    fn material(&self, x: f32, z: f32) -> u32 {
        self.material_at(x as f64, z as f64)
    }
    fn water_level(&self) -> f32 {
        self.terrain.water_level as f32
    }
}

/// The height map's intersect ray + `intersectRayWithCell` + the terrain's distance to geometry lookup:
/// the cells under the segment, in order; in each, its two triangles (the diagonal in a
/// chequerboard), a plane hit from above (n · dir < 0, t > 0) inside the triangle;
/// the first cell with one decides. Kept if n · dir < −1e-5 with the terrain's normal there and
/// the segment's end is under the plane.
pub fn terrain_segment(level: &bytefield_world::Level, start: V3, dir: V3) -> Option<(V3, V3)> {
    let n = level.heightmap.size;
    if n < 2 {
        return None;
    }
    let cell = (level.terrain.world_size / n as f64) as f32;
    let h = |c: usize, r: usize| level.terrain.height_m(level.heightmap.at(c, r)) as f32;
    let corner = |c: usize, r: usize| [c as f32 * cell, h(c, r), r as f32 * cell];
    let test_cell = |c: i64, r: i64| -> Option<V3> {
        if c < 0 || r < 0 || c > n as i64 - 2 || r > n as i64 - 2 {
            return None;
        }
        let (cu, ru) = (c as usize, r as usize);
        let (fc, fr) = (c as f32, r as f32);
        let local = |p: V3| ((p[0] / cell) - fc, (p[2] / cell) - fr);
        let inside = |fx: f32, fz: f32| (0.0..=1.0).contains(&fx) && (0.0..=1.0).contains(&fz);
        if (c + r) % 2 == 0 {
            if let Some(p) = ray_plane(start, dir, corner(cu, ru), corner(cu, ru + 1), corner(cu + 1, ru + 1)) {
                let (fx, fz) = local(p);
                if inside(fx, fz) && fx <= fz {
                    return Some(p);
                }
            }
            let p = ray_plane(start, dir, corner(cu, ru), corner(cu + 1, ru + 1), corner(cu + 1, ru))?;
            let (fx, fz) = local(p);
            (inside(fx, fz) && fx >= fz).then_some(p)
        } else {
            if let Some(p) = ray_plane(start, dir, corner(cu, ru), corner(cu, ru + 1), corner(cu + 1, ru)) {
                let (fx, fz) = local(p);
                if inside(fx, fz) && fx <= 1.0 - fz {
                    return Some(p);
                }
            }
            let p = ray_plane(start, dir, corner(cu + 1, ru), corner(cu, ru + 1), corner(cu + 1, ru + 1))?;
            let (fx, fz) = local(p);
            (inside(fx, fz) && fx >= 1.0 - fz).then_some(p)
        }
    };
    let hit = walk_cells(start[0] / cell, start[2] / cell, dir[0] / cell, dir[2] / cell, test_cell)?;
    let (_, nn) = level.height_and_normal(hit[0] as f64, hit[2] as f64, bytefield_formats::image::Orientation::IDENTITY)?;
    let nrm = [nn[0] as f32, nn[1] as f32, nn[2] as f32];
    let end = [start[0] + dir[0], start[1] + dir[1], start[2] + dir[2]];
    let depth = dot(sub(end, hit), nrm);
    let approach = dot(nrm, dir);
    ((approach as f64) < TERRAIN_MIN_APPROACH && depth <= 0.0).then_some((hit, nrm))
}

/// The cells (c, r) a segment's projection crosses, from its start, until `test` finds a hit.
fn walk_cells(u0: f32, v0: f32, du: f32, dv: f32, mut test: impl FnMut(i64, i64) -> Option<V3>) -> Option<V3> {
    let (mut c, mut r) = (u0.floor() as i64, v0.floor() as i64);
    let (c1, r1) = ((u0 + du).floor() as i64, (v0 + dv).floor() as i64);
    let (sc, sr) = (if du > 0.0 { 1 } else { -1 }, if dv > 0.0 { 1 } else { -1 });
    let next = |x: f32, d: f32, i: i64| -> f32 {
        if d == 0.0 {
            f32::INFINITY
        } else {
            let edge = if d > 0.0 { (i + 1) as f32 } else { i as f32 };
            (edge - x) / d
        }
    };
    let (mut tu, mut tv) = (next(u0, du, c), next(v0, dv, r));
    let (eu, ev) = (if du != 0.0 { 1.0 / du.abs() } else { f32::INFINITY }, if dv != 0.0 { 1.0 / dv.abs() } else { f32::INFINITY });
    let max = (c1 - c).abs() + (r1 - r).abs() + 2;
    for _ in 0..=max {
        if let Some(p) = test(c, r) {
            return Some(p);
        }
        if (c, r) == (c1, r1) || (tu > 1.0 && tv > 1.0) {
            break;
        }
        if tu < tv {
            c += sc;
            tu += eu;
        } else {
            r += sr;
            tv += ev;
        }
    }
    None
}

/// The height map's intersect ray with plane: the plane through a, b, c, normal (b − a) × (c − a)
/// (up for the terrain's triangles), hit from its front side ahead of the start.
fn ray_plane(start: V3, dir: V3, a: V3, b: V3, c: V3) -> Option<V3> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let l = len2(n);
    if (l - 1.0).abs() >= f32::EPSILON {
        if l.abs() >= f32::EPSILON {
            let k = 1.0 / l.sqrt();
            n = [n[0] * k, n[1] * k, n[2] * k];
        } else {
            n = [0.0; 3];
        }
    }
    let dn = dot(n, dir);
    if dn >= 0.0 {
        return None;
    }
    let t = -(dot(n, start) - dot(n, a)) / dn;
    (t > 0.0).then(|| [dir[0] * t + start[0], dir[1] * t + start[1], t * dir[2] + start[2]])
}

/// The response physics' solve impulse of a rigid projectile's part: like any other part (the root
/// moves by the position correction; the speed correction × fps × (1 + elasticity) / 2 is an
/// acceleration), but at the centre, and the projectile turns where it lands: pitch about its
/// x axis by 10°·clamp(n·z), then roll about its z axis by −10°·clamp(n·x) (n: the mean normal).
pub fn solve_projectile_part(c: &mut contact::Contactor, body: &mut RigidBody) {
    let r = &mut c.response;
    if c.spring.is_none() && r.pos_adjust != [0.0; 3] {
        for i in 0..3 {
            body.pos[i] += r.pos_adjust[i];
        }
        let n = r.normal;
        let pitch = dot(n, body.rows[2]).clamp(-1.0, 1.0) * IMPACT_TURN;
        let x = body.rows[0];
        turn(&mut body.rows, x, pitch);
        let roll = -dot(n, body.rows[0]).clamp(-1.0, 1.0) * IMPACT_TURN;
        let z = body.rows[2];
        turn(&mut body.rows, z, roll);
        r.point = [0.0; 3];
        let e = r.elasticity + 1.0;
        let k = contact::FPS * e * 0.5;
        let a = [r.vel_adjust[0] * k, r.vel_adjust[1] * k, r.vel_adjust[2] * k];
        r.pos_adjust = [0.0; 3];
        r.root_adjust = [0.0; 3];
        r.vel_adjust = [0.0; 3];
        let (_, at) = c.frame(body);
        body.add_acc_at(at, a);
    } else {
        c.solve(body);
    }
}

/// `rotateAboutLine(M, axis, degrees)`: every row turned about the (unit) world axis.
fn turn(rows: &mut [V3; 3], axis: V3, degrees: f32) {
    let l = len2(axis).sqrt();
    if l == 0.0 {
        return;
    }
    let [x, y, z] = [axis[0] / l, axis[1] / l, axis[2] / l];
    let (s, c) = degrees.to_radians().sin_cos();
    let t = 1.0 - c;
    let m = [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ];
    for r in rows.iter_mut() {
        let v = *r;
        *r = [dot(m[0], v), dot(m[1], v), dot(m[2], v)];
    }
}

/// The collision pass of a rigid projectile (the response physics' update, rigid list):
/// its parts against the obstacles, the movers (`checkObjectVsObjects`: a grenade's vertices
/// against a tank's faces) and the terrain, then `solveImpulse` with the projectile's turn and
/// `addFriction`. Returns the hits (the objects told: `handleCollision`).
pub fn collide_rigid(v: &mut Vehicle, ground: &dyn Ground, obstacles: Obstacles<'_>, movers: &mut [Vehicle], skip: &[usize], materials: &Materials, gravity: f32, dt: f32) -> Vec<(contact::Impulse, Option<usize>)> {
    if v.craft.body.is_sleeping() {
        return Vec::new();
    }
    contact::begin_pass(&mut v.contacts);
    v.craft.body.underwater = 0.0;
    let near = contact::near_obstacles(&v.craft.body, obstacles, v.radius, dt);
    let others: Vec<usize> = (0..movers.len()).filter(|&j| !skip.contains(&j) && movers[j].in_world() && crate::mover::may_touch(v, &movers[j], dt)).collect();
    // TODO: the point path leaves out the projectile's owner (`self.owner`), this one never did: does a bomb run into the plane
    // that drops it in the original?
    // (part, impulse, mover): which of the sim's vehicles each impulse is from, for the hit's damage
    let mut tags = Vec::new();
    for k in 0..v.contacts.len() {
        contact::check_part_obstacles(&mut v.contacts, k, &v.craft.body, &near, v.radius, materials, dt);
        for &j in &others {
            let n = v.contacts[k].last_impulses.len();
            crate::mover::check_part_vs_mover(v, k, &mut movers[j], materials, dt);
            let m = v.contacts[k].last_impulses.len();
            tags.extend((n..m).map(|i| (k, i, j)));
        }
        contact::check_part_terrain(&mut v.contacts, k, &mut v.craft.body, ground, materials);
    }
    let mut hits = Vec::new();
    for (k, c) in v.contacts.iter_mut().enumerate() {
        hits.extend(c.last_impulses.iter().enumerate().map(|(i, h)| (h.clone(), tags.iter().find(|t| t.0 == k && t.1 == i).map(|t| t.2))));
        solve_projectile_part(c, &mut v.craft.body);
        c.friction(&mut v.craft.body, gravity, None);
    }
    hits
}

impl World {
    /// A `ProjectileTemplate` (`ObjectTemplate.create Projectile`) as the sim uses it.
    pub fn projectile_template(&self, name: &str) -> Option<ProjectileTemplate> {
        let t = self.db.object_template(name)?;
        if !t.kind.eq_ignore_ascii_case("projectile") {
            return None;
        }
        let first = |n: &str| t.get(n).and_then(|a| a.first()).map(|v| v.trim().to_string());
        let num = |n: &str, d: f32| first(n).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        let flag = |n: &str, d: bool| first(n).map_or(d, |v| v != "0");
        let material = first("material").and_then(|v| v.parse::<i64>().ok()).filter(|&m| m >= 0).map(|m| m as u32);
        Some(ProjectileTemplate {
            name: t.name.clone(),
            mass: num("mass", 1.0),
            drag: num("drag", 0.0),
            gravity_modifier: num("gravitymodifier", 1.0),
            radius: self.bounding_radius(name),
            material,
            time_to_live: first("timetolive").and_then(|v| Distribution::parse(&v)).unwrap_or(Distribution { kind: 1, a: 3.0, b: 0.0 }),
            has_on_time_effect: flag("hasontimeeffect", false),
            has_collision_effect: flag("hascollisioneffect", false),
            die_after_coll: flag("dieaftercoll", true),
            die_at_object_hit: flag("dieatobjecthit", false),
            detonate_on_water: flag("detonateonwatercollision", true),
            is_sticky: flag("issticky", false),
            explode_near_enemy: num("explodenearenemydistance", -1.0),
            proximity_fuse_primer: num("proximityfuseprimer", -1.0),
            point_physics: flag("haspointphysics", true),
            engine: children(t).iter().any(|(c, _)| self.db.object_template(c).is_some_and(|c| c.kind.eq_ignore_ascii_case("engine"))),
            damage: crate::damage::ProjectileDamage {
                material: material.unwrap_or(0),
                damage_type: first("damagetype").and_then(|v| v.parse().ok()).unwrap_or(0),
                radius: num("radius", 10.0),
                material2: first("material2").and_then(|v| v.parse::<i64>().ok()).filter(|&m| m >= 0).map_or(u32::MAX, |m| m as u32),
                force: num("forceonexplosion", 150.0),
                ymod: num("ymodonexplosion", 1.0),
                min_damage: num("mindamage", 1.0),
                dist_start_lose: num("disttostartlosedamage", 0.0),
                dist_min: num("disttomindamage", 0.0),
            },
        })
    }

    /// A `FireArms` (or `HandFireArms`) template's firing data.
    pub fn weapon(&self, name: &str) -> Option<Weapon> {
        let t = self.db.object_template(name)?;
        if !t.kind.eq_ignore_ascii_case("firearms") && !t.kind.eq_ignore_ascii_case("handfirearms") {
            return None;
        }
        let first = |n: &str| t.get(n).and_then(|a| a.first()).map(|v| v.trim().to_string());
        let vec3 = |n: &str| first(n).and_then(|v| bytefield_world::parse_vec(&v)).filter(|v| v.len() == 3).map(|v| [v[0] as f32, v[1] as f32, v[2] as f32]).unwrap_or([0.0; 3]);
        let num = |n: &str, d: f32| first(n).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        let flag = |n: &str| first(n).is_some_and(|v| v != "0");
        // a command's arguments as numbers (missing ones 0)
        fn args<const N: usize>(a: Option<&[String]>) -> [f32; N] {
            let mut out = [0.0; N];
            for (o, v) in out.iter_mut().zip(a.into_iter().flatten()) {
                *o = v.trim().parse().unwrap_or(0.0);
            }
            out
        }
        let dist = |n: &str| first(n).and_then(|v| crate::firearms::Distribution4::parse(&v)).unwrap_or(crate::firearms::Distribution4 { kind: 0, a: 0.0, b: 0.0, symmetric: false });
        Some(Weapon {
            name: t.name.clone(),
            // (the medic's and the engineer's packs have none: they fire nothing)
            projectile: first("projectiletemplate").unwrap_or_default(),
            projectile_missing: first("projectiletemplate").is_some_and(|p| !p.is_empty() && self.db.object_template(&p).is_none()),
            velocity: first("velocity").and_then(|v| v.parse().ok()).unwrap_or(200.0),
            // a second argument (GC's TieBombRack, `-0.7/-2.2/1.2 0/0/-20`) leaves the original's at 0: it drops the
            // bomb from the weapon's origin. TODO: whether every one-argument setter given more is ignored likewise;
            // How: a synthetic weapon with `projectilePosition` and another setter given a second argument, in the original
            position: if t.get("projectileposition").is_some_and(|a| a.len() > 1) { [0.0; 3] } else { vec3("projectileposition") },
            add_root_speed: first("addrootspeed").is_none_or(|v| v != "0"),
            rotational_speed: vec3("rotationalspeed"),
            mag_size: first("magsize").and_then(|v| v.parse().ok()).unwrap_or(30),
            num_of_mag: first("numofmag").and_then(|v| v.parse().ok()).unwrap_or(3),
            mag_type: first("magtype").and_then(|v| v.parse().ok()).unwrap_or(0),
            reload_time: num("reloadtime", 1.0),
            round_of_fire: num("roundoffire", 10.0),
            fire_delay: num("firedelay", 0.0),
            fire_once: flag("fireonce"),
            auto_reload: flag("autoreload"),
            auto_fire: flag("autofire"),
            reload_empty_only: flag("allowreloadonemptycliponly"),
            change_when_empty: flag("changeweaponwhennoammo"),
            cant_select_empty: flag("cantselectwhennoammo"),
            destroy_vehicle_when_empty: flag("destroyvehiclewhennoammo"),
            disable_when_fired: flag("disablewhenfired"),
            time_to_not_allow_change: num("timetonotallowchange", -1.0),
            fire_dev: args(t.get("setfiredev")),
            min_dev: num("setmindev", 0.0),
            heat_add: num("heataddwhenfire", 0.0),
            // (per update: the template keeps `coolDownPerSec` / `g_simulationFps`, as its `makeScript` writes it back ×
            // the fps; before, the whole value went each update, 30 times too fast: DC's M249, dc_fire_m249_prone)
            cool_down: num("cooldownpersec", 0.0) / crate::soldier::SIM_FPS,
            overheat_delay: num("timedelayonoverheat", 0.0),
            heat_velocity: flag("velocitydependentonheat"),
            hide_time: num("hideduringfiretime", 0.0),
            hand: t.kind.eq_ignore_ascii_case("handfirearms").then(|| crate::firearms::HandDeviation {
                turn: args(t.get("setturndev")),
                speed: args(t.get("setspeeddev")),
                misc: args(t.get("setmiscdev")),
                dev_mod: t.get("setdevmod").map_or([1.0; 3], |_| args(t.get("setdevmod"))),
            }),
            recoil: flag("sethasrecoilforce").then(|| crate::firearms::Recoil {
                up: dist("setrecoilforceup"),
                left_right: dist("setrecoilforceleftright"),
                go_back: first("setgobackonrecoil").is_none_or(|v| v != "0"),
            }),
            item_index: first("itemindex").and_then(|v| v.parse().ok()).unwrap_or(0),
            firing_force: num("fireingforce", 0.0),
            tracer: t.get("settracertemplate").and_then(|a| {
                let n = crate::firearms::Distribution4::parse(a.get(1)?)?;
                Some((a.first()?.trim().to_string(), n))
            }),
            fire_in_camera: flag("fireincameradof"),
            barrels: t
                .all("addfirearmsposition")
                .iter()
                .filter_map(|a| bytefield_world::parse_vec(a.first()?).filter(|v| v.len() == 3).map(|v| [v[0] as f32, v[1] as f32, v[2] as f32]))
                .collect(),
            barrel_rots: t
                .all("addfirearmsposition")
                .iter()
                .filter(|a| a.first().and_then(|v| bytefield_world::parse_vec(v)).is_some_and(|v| v.len() == 3))
                .map(|a| a.get(1).and_then(|v| bytefield_world::parse_vec(v)).filter(|v| v.len() == 3).map_or([0.0; 3], |v| [v[0] as f32, v[1] as f32, v[2] as f32]))
                .collect(),
            asynchrony_fire: flag("setasynchronyfire") || flag("asynchronyfire"),
            blast_ammo_count: flag("setblastammocount") || flag("blastammocount"),
            input_fire: if first("setinputfire").is_some_and(|v| v.eq_ignore_ascii_case("c_PIAltFire")) { 23 } else { 8 },
            zoom_fov: num("zoomfov", -1.0),
            alt_fire_once: flag("altfireonce"),
            unzoom_between_fire: num("unzoombetweenfiretime", 0.0),
            eject_clip_time: num("ejectcliptime", 0.0),
            ammo_type: first("ammotype").and_then(|v| v.parse().ok()).unwrap_or(0),
        })
    }

    /// The weapons of a `Kit` (its `addTemplate`s that are fire arms), each with its slot.
    pub fn kit_weapons(&self, kit: &str) -> Vec<Weapon> {
        let Some(t) = self.db.object_template(kit) else { return Vec::new() };
        t.all("addtemplate").into_iter().filter_map(|a| self.weapon(a.first()?)).collect()
    }

    /// A projectile of `template` leaving `shot`. A rigid one (`setHasPointPhysics 0`) gets
    /// its rigid body and contact parts.
    pub fn projectile(&self, template: &str, shot: &Shot) -> Option<Projectile> {
        let t = Arc::new(self.projectile_template(template)?);
        let mut p = Projectile::new(t.clone(), shot);
        if !t.point_physics
            && let Some(mut v) = self.vehicle(template)
        {
            v.craft.body.pos = shot.pos;
            v.craft.body.rows = shot.rows;
            v.craft.body.vel = shot.vel;
            v.craft.body.rotvel = shot.rotvel;
            v.craft.body.gravity_modifier = t.gravity_modifier;
            // (it flies whatever its template says: GC's T-3B rocket and bombs have no `hasMobilePhysics`)
            v.fixed = false;
            // not one of the sim's vehicles: what it hits does not take it for one
            v.id = PROJECTILE_ID;
            // the `Obstacle`s in its tree are run into by themselves ([`Sim::carried`]), not parts
            // of its body: the original's `ShieldProjectile` stays put while a soldier crosses its
            // `ShieldBlock` (a vehicle's, as the podracers' energy binder, stay parts: unchecked)
            let carried = self.carried_obstacles(template);
            v.contacts.retain(|c| !carried.iter().any(|o| o.name.eq_ignore_ascii_case(&c.name)));
            // a fired rocket's engine runs
            v.set_engines_on(true);
            p.rigid = Some(Box::new(v));
        } else if t.point_physics
            && t.engine
            && let Some(mut v) = self.vehicle(template)
        {
            v.fixed = false;
            v.id = PROJECTILE_ID;
            v.contacts.clear();
            v.set_engines_on(true);
            p.parts = Some(Box::new(v));
        }
        Some(p)
    }
}

impl Sim<'_> {
    /// Add a projectile of `template` leaving `shot`; its index in `projectiles`.
    pub fn add_projectile(&mut self, template: &str, shot: &Shot) -> Option<usize> {
        let p = self.world.projectile(template, shot)?;
        // a spent one's place, as `spawn_vehicle` (they stayed until the round ended, 12816 in 10 min of 64 clients
        // in Berlin, a few MB and a pass over the dead each step)
        let k = match self.spent_place() {
            Some(k) => {
                // what hung from the old one goes: its depots, and its spawners follow nothing (an index no projectile has)
                self.depots.retain(|d| !(d.on_projectile && d.on.is_some_and(|(j, _)| j == k)));
                for s in self.spawners.iter_mut().filter(|s| s.parent == crate::create::Parent::Projectile(k)) {
                    s.parent = crate::create::Parent::Projectile(usize::MAX);
                }
                self.projectiles[k] = p;
                k
            }
            None => {
                self.projectiles.push(p);
                self.projectiles.len() - 1
            }
        };
        // the spawners and depots in its tree (`createObject` makes its children): what it
        // deploys where it is (DC's mortar and PKM, GC's E-Web, made in the spawners' next
        // frame), what it supplies while it lasts (GC's repair droids)
        self.add_tree(template, crate::create::Parent::Projectile(k), shot.pos, shot.rows);
        let carried = self.world.carried_obstacles(template);
        if !carried.is_empty() {
            self.carried_from.push((k, carried));
            self.place_carried();
        }
        Some(k)
    }

    /// The oldest spent projectile's place, ended [`SPENT_REUSE_AFTER`] s ago or more, that no explosives' pool names (its
    /// slots: a mine of it ended is replaced in its place). In `Sim::spent`, in the order they ended.
    fn spent_place(&mut self) -> Option<usize> {
        let &k = self.spent.front()?;
        if self.projectiles.get(k).is_none_or(|q| q.alive() || q.gone_since.is_none_or(|t| self.clock.time - t < SPENT_REUSE_AFTER)) {
            // (a stale entry, from before a `clear_world`: dropped)
            if self.projectiles.get(k).is_none_or(|q| q.alive()) {
                self.spent.pop_front();
            }
            return None;
        }
        self.spent.pop_front();
        if self.pools.iter().any(|p| p.slots.contains(&k)) {
            // (named: it waits at the back, as one ended now would)
            self.spent.push_back(k);
            return None;
        }
        Some(k)
    }

    /// What can be run into now: the level's obstacles and those the projectiles carry.
    pub fn obstacles(&self) -> Obstacles<'_> {
        Obstacles { fixed: self.world.obstacles(), carried: &self.carried }
    }

    /// The obstacles in the trees of the projectiles flying, placed where their projectile is
    /// (`carried`); those of a projectile that ended go with it.
    pub(crate) fn place_carried(&mut self) {
        let projectiles = &self.projectiles;
        self.carried_from.retain(|(k, _)| projectiles[*k].alive());
        self.carried.clear();
        for (k, locals) in &self.carried_from {
            let p = &projectiles[*k];
            let (r, at) = (p.rows, p.pos);
            let point = |l: V3| [0, 1, 2].map(|j| at[j] + l[0] * r[0][j] + l[1] * r[1][j] + l[2] * r[2][j]);
            for o in locals.iter() {
                let mut o = o.clone();
                o.rows = o.rows.map(|row| [0, 1, 2].map(|j| row[0] * r[0][j] + row[1] * r[1][j] + row[2] * r[2][j]));
                o.pos = point(o.pos);
                o.center = point(o.center);
                o.carrier = Some(*k);
                self.carried.push(o);
            }
        }
    }

    /// `weapon` fires from the barrel axes `rows` at `pos`; `root_vel`: the speed of the object
    /// carrying it (None: no root physics node). The projectile's index.
    pub fn fire(&mut self, weapon: &str, rows: [V3; 3], pos: V3, root_vel: Option<V3>) -> Option<usize> {
        let w = self.world.weapon(weapon)?;
        let shot = w.fire(rows, pos, root_vel, crate::rigid::GRAVITY_Y);
        self.add_projectile(&w.projectile, &shot)
    }

    /// The same from barrel `barrel` (`addFireArmsPosition`: its position and turn: the Hydra fired from barrel 0 in
    /// the original left 0.99 m lower than ours from the rack's origin). A barrel the weapon lacks (−1 included) fires from
    /// `projectilePosition`. With the deviation of a new `FireArms` (`setMinDev`) at input index 0, as the reference harness fires
    /// it (DC's mortar, `setMinDev 1`, left at +0.424 m/s in x and −0.559 in y, the numbers of index 0 and barrel −1;
    /// without them the shot flew 15 cm high and missed the Humvee's hood).
    pub fn fire_barrel(&mut self, weapon: &str, rows: [V3; 3], pos: V3, root_vel: Option<V3>, barrel: i32) -> Option<usize> {
        let w = self.world.weapon(weapon)?;
        let barrel = if usize::try_from(barrel).is_ok_and(|i| i < w.barrels.len()) { barrel } else { -1 };
        let shot = crate::firearms::FireArms::new(Arc::new(w.clone())).barrel_shot(rows, pos, root_vel, crate::rigid::GRAVITY_Y, 0, barrel);
        // what a new weapon fires first: its tracer if the count is within the barrels ([`FireArms::next_projectile`])
        // TODO: the count's seed is 0 here; the server's weapon draws it from its own, so DC's ZPU-4 fires its tracer first here and
        // ZPU4_Projectile there (0.14 m apart). How: the seed from the reference run's `fire` event
        let first = w.tracer.as_ref().filter(|t| crate::firearms::tracer_count(&t.1, w.barrels.len(), &mut 0) <= w.barrels.len() as i32).map_or(&w.projectile, |t| &t.0);
        self.add_projectile(first, &shot)
    }

    /// The projectiles' frame: the parts of the rigid ones, timers, integration, then (point objects first, as
    /// the response physics' update does) their collision, then the rigid ones'.
    pub fn step_projectiles(&mut self, dt: f32, parts_last: bool) {
        if self.projectiles.is_empty() {
            return;
        }
        let world = self.world;
        let level = &world.level;
        // (taken while `self` is borrowed below; placed again at the end, where the projectiles went)
        let carried = std::mem::take(&mut self.carried);
        let obstacles = Obstacles { fixed: world.obstacles(), carried: &carried };
        let materials = world.materials();
        let step = self.clock.steps;
        let g = crate::rigid::GRAVITY_Y;
        // a rigid projectile's parts first, as a vehicle's (a torpedo's engine, wings, floats);
        // `parts_last`: after its collision instead (`Sim::step_ordered`)
        let parts = |ps: &mut [Projectile]| {
            for p in ps.iter_mut().filter(|p| p.alive()) {
                if let Some(v) = p.rigid.as_deref_mut() {
                    v.update_parts(dt, level);
                } else if let Some(v) = p.parts.as_deref_mut() {
                    let b = &mut v.craft.body;
                    (b.pos, b.rows, b.vel, b.acc) = (p.pos, p.rows, p.vel, [0.0; 3]);
                    v.update_parts(dt, level);
                    let a = std::mem::take(&mut v.craft.body.acc);
                    v.craft.body.rotacc = [0.0; 3];
                    p.acc = [0, 1, 2].map(|i| p.acc[i] + a[i]);
                }
            }
        };
        if !parts_last {
            parts(&mut self.projectiles);
        }
        let mut events = Vec::new();
        let water = level.terrain.water_level as f32;
        for (k, p) in self.projectiles.iter_mut().enumerate() {
            let (n, alive) = (p.impacts.len(), p.alive());
            // `handleUpdate`'s proximity fuse, before the timer and the physics
            if alive && crate::explosives::fuse(p, &self.vehicles, p.pos[1] < water) {
                p.end = Some(End::Detonated);
                events.push((k, None));
                continue;
            }
            p.step(level, obstacles, &self.vehicles, step, dt, g);
            for h in n..p.impacts.len() {
                events.push((k, Some(h)));
            }
            if alive && matches!(p.end, Some(End::Expired { detonated: true })) {
                events.push((k, None));
            }
        }
        // what they did: hits and explosions
        for (k, h) in events {
            let p = self.projectiles[k].clone();
            match h {
                Some(h) => self.projectile_hit(&p, &p.impacts[h]),
                None => self.projectile_detonated(&p),
            }
        }
        // the rigid ones (grenades, bombs): their contact pass, and each hit goes through
        // `handleCollision` as a point projectile's (a bomb's `hasCollisionEffect` ends it on
        // the ground and it explodes; a grenade bounces)
        let mut ended = Vec::new();
        // what each one's own spawners made: GC's landing craft drops a `Speederbike_Deploy` whose spawner makes the
        // speeder bike where it is at once; the original's falls through it to the ground
        let made: Vec<(usize, usize)> = self
            .spawners
            .iter()
            .filter_map(|s| match s.parent {
                crate::create::Parent::Projectile(p) => Some((p, s)),
                _ => None,
            })
            .flat_map(|(p, s)| s.slots.iter().flatten().chain(s.last.iter()).filter_map(move |m| match m {
                crate::create::Made::Vehicle(j) => Some((p, *j)),
                _ => None,
            }))
            .collect();
        let movers = &mut self.vehicles;
        for (k, p) in self.projectiles.iter_mut().enumerate().filter(|(_, p)| p.alive()) {
            let Some(v) = p.rigid.as_deref_mut() else { continue };
            // (the hits' points from where it was tested, before the collision moved it)
            let at = v.craft.body.pos;
            // not against what it carries itself
            let others: Vec<Obstacle>;
            let obstacles = if carried.iter().any(|o| o.carrier == Some(k)) {
                others = carried.iter().filter(|o| o.carrier != Some(k)).cloned().collect();
                Obstacles { fixed: obstacles.fixed, carried: &others }
            } else {
                obstacles
            };
            let skip: Vec<usize> = made.iter().filter(|m| m.0 == k).map(|m| m.1).collect();
            let hits = collide_rigid(v, level, obstacles, movers, &skip, materials, g, dt);
            p.pos = v.craft.body.pos;
            p.rows = v.craft.body.rows;
            p.vel = v.craft.body.vel;
            for (h, vehicle) in hits {
                let point = [at[0] + h.r[0], at[1] + h.r[1], at[2] + h.r[2]];
                let was = p.end.is_some();
                p.handle_collision(Impact { step, point, normal: h.n, material: h.materials.1, object: h.with.clone(), vehicle, water: false, vel: h.v, ended: false, bone: None });
                if !was && p.end.is_some() {
                    ended.push((k, p.impacts.len() - 1));
                }
            }
        }
        for (k, h) in ended {
            let p = self.projectiles[k].clone();
            self.projectile_hit(&p, &p.impacts[h]);
        }
        if parts_last {
            parts(&mut self.projectiles);
        }
        self.place_carried();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat(f32);
    impl Terrain for Flat {
        fn segment(&self, start: V3, dir: V3) -> Option<(V3, V3)> {
            let end = start[1] + dir[1];
            if start[1] >= self.0 && end <= self.0 && dir[1] < 0.0 {
                let t = (start[1] - self.0) / -dir[1];
                Some(([start[0] + dir[0] * t, self.0, start[2] + dir[2] * t], [0.0, 1.0, 0.0]))
            } else {
                None
            }
        }
        fn material(&self, _: f32, _: f32) -> u32 {
            5
        }
        fn water_level(&self) -> f32 {
            -100.0
        }
    }

    fn shell() -> Arc<ProjectileTemplate> {
        Arc::new(ProjectileTemplate {
            name: "Shell".into(),
            mass: 1.0,
            drag: 0.0,
            gravity_modifier: 1.0,
            radius: 0.5,
            material: None,
            time_to_live: Distribution { kind: 0, a: 10.0, b: 0.0 },
            has_on_time_effect: false,
            has_collision_effect: true,
            die_after_coll: true,
            die_at_object_hit: false,
            detonate_on_water: true,
            is_sticky: false,
            explode_near_enemy: -1.0,
            proximity_fuse_primer: -1.0,
            point_physics: true,
            engine: false,
            damage: Default::default(),
        })
    }

    #[test]
    fn distributions() {
        assert_eq!(Distribution::parse("CRD_NONE/10/0/0").unwrap().value(), 10.0);
        assert_eq!(Distribution::parse("CRD_UNIFORM/2/4").unwrap().value(), 3.0);
    }

    #[test]
    fn a_shot_falls_from_its_second_frame_and_lands_on_flat_ground() {
        let w = Weapon {
            name: "Gun".into(),
            projectile: "Shell".into(),
            projectile_missing: false,
            velocity: 100.0,
            position: [0.0; 3],
            add_root_speed: true,
            rotational_speed: [0.0; 3],
            mag_size: 30,
            num_of_mag: 3,
            mag_type: 0,
            reload_time: 1.0,
            round_of_fire: 10.0,
            fire_delay: 0.0,
            fire_once: false,
            auto_reload: false,
            auto_fire: false,
            reload_empty_only: false,
            destroy_vehicle_when_empty: false,
            disable_when_fired: false,
            change_when_empty: false,
            cant_select_empty: false,
            time_to_not_allow_change: -1.0,
            fire_dev: [0.0; 3],
            min_dev: 0.0,
            heat_add: 0.0,
            cool_down: 0.0,
            overheat_delay: 0.0,
            heat_velocity: false,
            hide_time: 0.0,
            hand: None,
            recoil: None,
            item_index: 0,
            input_fire: 8,
            firing_force: 0.0,
            tracer: None,
            fire_in_camera: false,
            barrels: Vec::new(),
            barrel_rots: Vec::new(),
            asynchrony_fire: false,
            blast_ammo_count: false,
            zoom_fov: -1.0,
            alt_fire_once: false,
            unzoom_between_fire: 0.0,
            eject_clip_time: 0.0,
            ammo_type: 0,
        };
        let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let shot = w.fire(rows, [0.0, 2.0, 0.0], Some([0.0; 3]), crate::rigid::GRAVITY_Y);
        assert_eq!(shot.vel[2], 100.0);
        assert!((shot.vel[1] - 0.982).abs() < 1e-5, "{:?}", shot.vel);
        let mut p = Projectile::new(shell(), &shot);
        let dt = 1.0 / contact::FPS;
        p.step(&Flat(0.0), Obstacles::default(), &[], 1, dt, crate::rigid::GRAVITY_Y);
        assert!((p.pos[1] - (2.0 + 0.982 * dt)).abs() < 1e-5, "no gravity in the first frame: {:?}", p.pos);
        let mut n = 1;
        while p.alive() && n < 300 {
            n += 1;
            p.step(&Flat(0.0), Obstacles::default(), &[], n, dt, crate::rigid::GRAVITY_Y);
        }
        assert_eq!(p.end, Some(End::Hit { detonated: true }));
        let hit = p.impacts.last().unwrap();
        assert_eq!(hit.material, 5);
        assert!(hit.point[1].abs() < 1e-5 && hit.point[2] > 40.0, "{:?}", hit.point);
    }

    #[test]
    fn its_time_runs_out() {
        let shot = Shot { pos: [0.0, 1000.0, 0.0], rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], vel: [0.0, 50.0, 0.0], rotvel: [0.0; 3] };
        let mut p = Projectile::new(shell(), &shot);
        let dt = 1.0 / contact::FPS;
        let mut n = 0;
        while p.alive() {
            n += 1;
            p.step(&Flat(0.0), Obstacles::default(), &[], n, dt, crate::rigid::GRAVITY_Y);
        }
        assert_eq!(p.end, Some(End::Expired { detonated: false }));
        assert!((299..=302).contains(&n), "{n}");
    }

    #[test]
    fn drag_slows_it_down() {
        let mut t = (*shell()).clone();
        t.drag = 0.1;
        let shot = Shot { pos: [0.0, 1000.0, 0.0], rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], vel: [100.0, 0.0, 0.0], rotvel: [0.0; 3] };
        let mut p = Projectile::new(Arc::new(t), &shot);
        p.integrate(1.0 / 30.0, crate::rigid::GRAVITY_Y, [0.0; 3]);
        p.integrate(1.0 / 30.0, crate::rigid::GRAVITY_Y, [0.0; 3]);
        // a = −π 0.25 · 0.1 · 100 ≈ −7.85 m/s², each frame
        assert!((p.vel[0] - (100.0 - 2.0 * 7.853_982 / 30.0)).abs() < 2e-3, "{:?}", p.vel);
    }

    #[test]
    fn walks_the_cells_in_order() {
        let mut seen = Vec::new();
        walk_cells(0.5, 0.5, 2.0, 1.0, |c, r| {
            seen.push((c, r));
            None
        });
        assert_eq!(seen.first(), Some(&(0, 0)));
        assert_eq!(seen.last(), Some(&(2, 1)));
        assert!(seen.windows(2).all(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs() == 1));
    }

    #[test]
    fn a_rigid_projectile_turns_where_it_lands() {
        let mut rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        // nose (z) down a little: n·z > 0 on flat ground, it pitches about x
        turn(&mut rows, [1.0, 0.0, 0.0], 30.0);
        let before = rows;
        let n = [0.0f32, 1.0, 0.0];
        let pitch = dot(n, rows[2]).clamp(-1.0, 1.0) * IMPACT_TURN;
        let x = rows[0];
        turn(&mut rows, x, pitch);
        assert!(dot(rows[0], before[0]) > 0.9999);
        assert!(dot(rows[2], before[2]) < 1.0);
    }
}
