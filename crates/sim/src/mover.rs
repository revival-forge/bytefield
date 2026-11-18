//! A vehicle in the world: its flight physics (`craft`), the parts that touch things
//! (`contact`), and its bounding radius, stepped the way the server's frame does it:
//! 1. `update_parts` (`objectManager` → `handleUpdate`): inputs, bundles, engines, wings, floats
//! 2. `integrate` (`physicsNodeManager` → the physics node's physics update)
//! 3. `collide`: obstacles and terrain, impulses, friction,
//!    springs; the driven wheels' friction loads the engine back (`feedbackLoop`)
//!
//! Each phase runs for every object before the next one starts ([`crate::Sim::step`]). The
//! collision itself is two phases ([`collide_all`]): every object checks (the other movers too),
//! then every object solves.

use serde::Serialize;

use crate::contact::{self, Contactor, Drive, Ground, Materials, collision_pass};
use crate::craft::{Craft, Kind};
use crate::obstacle::{Obstacles, pick_lod, segment_on};
use crate::rigid::GRAVITY_Y;
use crate::vecmath::V3;

/// What the collision pass of a vehicle runs against.
pub struct Surroundings<'a> {
    pub ground: &'a dyn Ground,
    pub obstacles: Obstacles<'a>,
    pub materials: &'a Materials,
}

#[derive(Debug, Clone, Serialize)]
pub struct Vehicle {
    pub template: String,
    pub craft: Craft,
    pub contacts: Vec<Contactor>,
    /// `getBoundingRadius` (the broad phase and the obstacles' collision LOD)
    pub radius: f32,
    /// a soldier's player, input and animation state (`soldier::Movement`)
    pub movement: Option<crate::soldier::Movement>,
    /// its place in `Sim.vehicles` (who another one collided with)
    pub id: usize,
    /// `hasArmor`: hit points, critical damage, the wreck
    pub armor: Option<crate::damage::Armor>,
    /// gone from the world (`destroyObject`: the wreck's time ran out); kept in `Sim.vehicles`
    /// so indices stay, until [`crate::Sim::spawn_vehicle`] takes the place for another
    pub removed: bool,
    /// the world time it was first seen `removed` (at the end of a step): its place is free [`crate::REUSE_AFTER`] s later
    #[serde(skip)]
    pub gone_since: Option<f64>,
    /// which of the server's objects it is (a count of the server's `spawn_vehicle`; 0: one a copy made itself): a place
    /// taken for another changes it, and the copy builds the object again
    #[serde(skip)]
    pub born: u32,
    /// how many times [`crate::Sim::spawn_vehicle`] took its place for another, the generation of the AI's handles;
    /// wraps at 256
    #[serde(skip)]
    pub place_gen: u8,
    /// a soldier's team (its player's): the control points count it
    pub team: u32,
    /// a vehicle's seat and driver, or the vehicle a soldier is in
    pub entry: crate::entry::Entry,
    /// its weapons (`FireArms` parts: the part, its state) and the shots they fired since the
    /// sim last took them
    pub weapons: Vec<(usize, crate::firearms::FireArms)>,
    pub shots: Vec<(String, crate::projectile::Shot, usize)>,
    /// its parts solved in this frame's checks already ([`check_all`])
    #[serde(skip)]
    pub solved: bool,
    /// no mobile physics (`hasMobilePhysics` 0: a fixed gun): it is not integrated nor collides;
    /// the level's obstacles already hold its geometry (`World::obstacles`)
    #[serde(default)]
    pub fixed: bool,
    /// its physics off (the physical object's disable physics: a bot's `Medium` move slides it by
    /// hand): not integrated, and it collides with nothing
    #[serde(default)]
    pub physics_off: bool,
}

impl Vehicle {
    /// The parts under a moving one (a front wheel under its steering bundle) where its angles
    /// put them now, in the root's frame.
    pub fn place_contacts(&mut self) {
        if self.contacts.iter().all(|c| c.anchor.is_none()) {
            return;
        }
        let frames = self.craft.frames_in_root();
        for c in self.contacts.iter_mut() {
            for (i, (a, rest)) in c.chain_anchor.iter().enumerate() {
                let Some((k, ar)) = a else { continue };
                let Some(f) = frames.get(*k) else { continue };
                let now = crate::craft::mul(ar, &f.child_rows);
                c.chain[i].1 = if now == *rest { *rest } else { now };
            }
            // a spring's chain: each link's parent axes as its moving part turns them now (as built while unturned)
            if let Some(sp) = c.spring.as_mut() {
                for (i, (a, rest)) in sp.chain_anchor.iter().enumerate() {
                    let Some((k, ar)) = a else { continue };
                    let Some(f) = frames.get(*k) else { continue };
                    let now = crate::craft::mul(ar, &f.child_rows);
                    sp.chain[i].1 = if now == *rest { *rest } else { now };
                }
            }
            let Some(a) = c.anchor else { continue };
            let Some(f) = frames.get(a.part) else { continue };
            let mut rows = crate::craft::mul(&a.rows, &f.child_rows);
            // (a spring stays in its parent's axes, without its own turn: as built)
            if let Some(t) = c.spring.as_ref().and_then(|s| s.turn) {
                rows = crate::craft::mul(&crate::craft::transpose(&t), &rows);
            }
            // (unturned: where it was built, summed root first as then)
            if rows == a.rest.1 {
                (c.pos, c.rows) = a.rest;
                continue;
            }
            let q = crate::craft::to_parent(a.pos, &f.child_rows);
            c.pos = [f.pos[0] + q[0], f.pos[1] + q[1], f.pos[2] + q[2]];
            c.rows = rows;
        }
    }

    /// The first engine, as the wheels see it.
    pub fn drive(&self) -> Option<Drive> {
        self.craft.parts.iter().find_map(|p| match &p.kind {
            Kind::Engine(e) => Some(Drive {
                ratio: e.current_ratio(),
                rpm: e.rpm,
                differential: e.steer,
                gear_timer: e.gear_timer,
                disengaged: e.braking,
                tracked: e.engine_type & 4 != 0,
            }),
            _ => None,
        })
    }

    /// In the physics: not removed, and not a soldier inside a vehicle (`disablePhysicsOnSoldier`).
    pub fn in_world(&self) -> bool {
        !self.removed && self.entry.inside.is_none()
    }

    /// Turn every engine on or off (an engine runs only with a driver).
    pub fn set_engines_on(&mut self, on: bool) {
        for p in self.craft.parts.iter_mut() {
            if let Kind::Engine(e) = &mut p.kind {
                e.on = on;
            }
        }
    }

    pub fn update_parts(&mut self, dt: f32, ground: &dyn Ground) {
        self.craft.update_parts(dt, &|x, z| Some(ground.height_and_normal(x, z).0));
        self.place_contacts();
        // a soldier's input: the speed the ground under it should
        // have, and a push of 0.75 of it
        if let Some(m) = self.movement.as_mut()
            && let Some(c) = self.contacts.iter_mut().find(|c| c.soldier)
        {
            let g = crate::rigid::GRAVITY_Y;
            let p = self.craft.body.pos;
            m.height_over_terrain = p[1] - ground.height_and_normal(p[0], p[2]).0;
            if m.has_player && m.driven {
                // the frame updates come first, then
                // `simulatePlayerUpdate`: the triggers, the input, then `performHandleUpdate`
                m.run_frames();
                m.triggers(&self.craft.body, g);
                c.response.walk = m.handle_player_input(&mut self.craft.body, &mut c.response, ground, dt);
                m.handle_update(&mut self.craft.body, &mut c.response, ground, dt);
                m.weapon_update(&self.craft.body, g, dt);
            } else {
                m.handle_update(&mut self.craft.body, &mut c.response, ground, dt);
                m.weapon_frame(&self.craft.body, g, dt);
                if m.driven {
                    c.response.walk = m.handle_player_input(&mut self.craft.body, &mut c.response, ground, dt);
                }
            }
        }
        if let Some(d) = self.movement.as_mut().and_then(|m| m.set_drag.take()) {
            self.craft.body.drag = d;
        }
        for c in self.contacts.iter_mut().filter(|c| c.soldier) {
            c.response.surface_speed = c.response.walk;
        }
    }

    pub fn integrate(&mut self, dt: f32) {
        if self.fixed || self.physics_off {
            return;
        }
        // soldiers never count down to sleep
        let keep = self.is_soldier().then_some(self.craft.body.sleepiness);
        // nor turn from a push off their centre (an explosion's): the server's soldier never has
        // a rotational velocity (dmg_soldier_grenade); the soldier's update handling sets its
        // matrix (upright, not done for a dead one: its heading turns with the death)
        if keep.is_some() {
            self.craft.body.rotacc = [0.0; 3];
            self.craft.body.rotvel = [0.0; 3];
        }
        self.craft.body.step(dt);
        if let Some(s) = keep {
            self.craft.body.sleepiness = s;
        }
    }

    /// What the wheels see of their engines now: the first one's state, and with several (DC's Ural tanker hangs one in
    /// the other, each with its wheels) the nearest above each wheel, as `addFriction` finds it.
    fn drive_wheels(&mut self) -> Option<Drive> {
        let of = |e: &crate::craft::Engine| Drive { ratio: e.current_ratio(), rpm: e.rpm, differential: e.steer, gear_timer: e.gear_timer, disengaged: e.braking, tracked: e.engine_type & 4 != 0 };
        let parts = &self.craft.parts;
        if parts.iter().filter(|p| matches!(p.kind, Kind::Engine(_))).count() > 1 {
            for c in &mut self.contacts {
                let mut at = c.anchor.as_ref().map(|a| a.part);
                c.drive = None;
                while let Some(k) = at {
                    if let Kind::Engine(e) = &parts[k].kind {
                        c.drive = Some((k, of(e)));
                        break;
                    }
                    at = parts[k].parent;
                }
            }
        }
        self.drive()
    }

    /// The collision pass, with the first engine driving the wheels and taking their feedback.
    pub fn collide(&mut self, env: &Surroundings, dt: f32) {
        let drive = self.drive_wheels();
        let fb = collision_pass(&mut self.contacts, &mut self.craft.body, env.ground, env.obstacles, self.radius, env.materials, GRAVITY_Y, dt, drive.as_ref());
        self.feed_engine(fb);
    }

    /// The driven wheels' friction back to the first engine (`feedbackLoop`).
    pub(crate) fn feed_engine(&mut self, fb: Vec<contact::Feedback>) {
        for (f, axis, engine) in fb {
            let e = match engine {
                Some(k) => self.craft.parts.get_mut(k),
                None => self.craft.parts.iter_mut().find(|p| matches!(p.kind, Kind::Engine(_))),
            };
            if let Some(crate::craft::Part { kind: Kind::Engine(e), .. }) = e {
                e.feedback(f, axis);
            }
        }
    }

    /// The second phase of the collision: solve, friction, springs, the engine's feedback.
    pub fn solve(&mut self, dt: f32) {
        let drive = self.drive_wheels();
        let fb = contact::solve_pass(&mut self.contacts, &mut self.craft.body, GRAVITY_Y, dt, drive.as_ref());
        self.feed_engine(fb);
    }

    /// One whole frame for this vehicle alone (parts, integration, collision).
    pub fn step(&mut self, env: &Surroundings, dt: f32) {
        self.update_parts(dt, env.ground);
        self.integrate(dt);
        self.collide(env, dt);
    }
}

/// The response physics' object vs objects check, between two movers' parts:
/// parts whose bounding radii are both under this are not tested against each other.
pub const MIN_PART_RADIUS: f32 = 0.45;
/// The reach of the pair test grows with each root's |v|² · dt · this.
pub const REACH_SPEED: f32 = 0.25;
/// A root whose bounding radius is at most this times the other's: only its vertices are
/// tested, against the other's faces, with the whole impulse; otherwise both ways, half each.
pub const SMALL_SHARE: f32 = 0.25;
/// The response physics' object vs object check: share of the impulse of the vertices' object,
/// m_other / (m_other + m_own); above `SHARE_ALL` it takes it all, below `SHARE_NONE` none
/// (and then, an engine quirk, the other gets +1 instead of −1: it is pushed the wrong way).
pub const SHARE_ALL: f32 = 0.95;
pub const SHARE_NONE: f32 = 0.05;
/// |relative speed|² above this calls both objects' `handleCollision` (damage).
pub const HIT_SPEED2: f32 = 0.1;

fn len2(v: V3) -> f32 {
    v[2] * v[2] + v[1] * v[1] + v[0] * v[0]
}

/// Mutable access to two different elements.
fn pair_mut<T>(v: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert!(i != j);
    if i < j {
        let (a, b) = v.split_at_mut(j);
        (&mut a[i], &mut b[0])
    } else {
        let (a, b) = v.split_at_mut(i);
        (&mut b[0], &mut a[j])
    }
}

/// The shares of the impulse (the vertices' object, the faces' object), from the two roots'
/// masses, × `frac` (what the manager gives this direction: 1 or 0.5).
pub fn shares(m_own: f32, m_other: f32, frac: f32) -> (f32, f32) {
    let f = m_other as f64 / (m_other as f64 + m_own as f64);
    let (a, b) = if f > SHARE_ALL as f64 {
        (1.0, 0.0)
    } else if f < SHARE_NONE as f64 {
        (0.0, 1.0)
    } else {
        (f as f32, -((1.0 - f) as f32))
    };
    (a * frac, b * frac)
}

/// The response physics' object vs object check: the vertices of part `x` of
/// `p` (LOD 0, as against the terrain) against the faces of part `y` of `q` (collision LOD 1 if
/// p's bounding radius is under 4 m), in q's frame as it is now. The segment runs from p's root
/// set back by one step of the *relative* speed to each vertex. A hit gives both parts an
/// impulse with the same normal (the face's), each its share of the relative velocity at the
/// point and of the depth: p's along the normal, q's against it.
pub fn check_vertices_on_faces(p: &mut Vehicle, x: usize, q: &mut Vehicle, y: usize, materials: &Materials, dt: f32, frac: f32) {
    let c = &p.contacts[x];
    let mut n = c.vertices.len();
    if n == 0 {
        return;
    }
    if n < 4 {
        n = 1;
    }
    let target = &q.contacts[y];
    let Some(mesh) = pick_lod(&target.faces, contact::faces_lod(p.radius)) else { return };
    let (bp, bq) = (&p.craft.body, &q.craft.body);
    // a fixed vehicle's faces take no impulse (the original's `StaticResponsePhysics`): the mover's vertices take it all,
    // as against the level's obstacles; a fixed vehicle's own vertices (an active part of one) are not tested
    if p.fixed {
        return;
    }
    let (sp, sq) = if q.fixed { (frac, 0.0) } else { shares(bp.mass, bq.mass, frac) };
    let vrel = [bp.vel[0] - bq.vel[0], bp.vel[1] - bq.vel[1], bp.vel[2] - bq.vel[2]];
    let step = [vrel[0] * dt, vrel[1] * dt, vrel[2] * dt];
    let (rows, pos) = c.frame(bp);
    let off = [pos[0] - bp.pos[0] + step[0], pos[1] - bp.pos[1] + step[1], pos[2] - bp.pos[2] + step[2]];
    let start = [pos[0] - off[0], pos[1] - off[1], pos[2] - off[2]];
    let (qrows, qpos) = target.frame(bq);
    let mut hits = Vec::new();
    for i in 0..n {
        let v = c.vertices[i];
        let w = [
            rows[2][0] * v[2] + rows[0][0] * v[0] + rows[1][0] * v[1],
            rows[0][1] * v[0] + rows[1][1] * v[1] + rows[2][1] * v[2],
            v[2] * rows[2][2] + rows[0][2] * v[0] + v[1] * rows[1][2],
        ];
        let dir = [w[0] + off[0], w[1] + off[1], w[2] + off[2]];
        let Some(mut hit) = segment_on(mesh, &qrows, qpos, target.scale, start, dir, false) else { continue };
        let (va, vb) = (bp.velocity_at(hit.point), bq.velocity_at(hit.point));
        let vr = [va[0] - vb[0], va[1] - vb[1], va[2] - vb[2]];
        // the soldier's `handleCollision` gets the face's normal, the impulse its own
        let face = hit.normal;
        if c.soldier && i != 0 {
            crate::soldier::object_normal(&mut hit, start, pos);
        }
        // |vr|² > HIT_SPEED2: both `handleCollision`s; for vehicles they return true (damage
        // is not modelled), so the impulses always follow
        hits.push((i, hit, vr, face));
    }
    let (pname, qname) = (p.template.clone(), q.template.clone());
    let (pid, qid) = (p.id, q.id);
    // a soldier against the vehicle it left less than 2 s ago: its `handleCollision` returns 1
    // before anything: the impulse, but no ground and no crash of its own
    let (p_quiet, q_quiet) = (p.entry.exited.is_some_and(|e| e.0 == qid), q.entry.exited.is_some_and(|e| e.0 == pid));
    for (i, hit, vr, face) in hits {
        if p.contacts[x].soldier && len2(vr) > HIT_SPEED2 && !p_quiet {
            crate::soldier::handle_collision(&mut p.contacts[x].response, face, false);
        }
        let m = (p.contacts[x].materials.get(i).copied().unwrap_or(0), hit.material);
        // both `handleCollision`s (damage): q's with the velocity the other way and the
        // materials swapped
        if len2(vr) > HIT_SPEED2 {
            use crate::damage::Other;
            if !p_quiet {
                p.contacts[x].hits.push(contact::Hit { other: Other::Part(qid, y), v: vr, n: face, materials: m, height: pos[1], barrier: None });
            }
            if !q_quiet {
                q.contacts[y].hits.push(contact::Hit { other: Other::Part(pid, x), v: [-vr[0], -vr[1], -vr[2]], n: face, materials: (m.1, m.0), height: qpos[1], barrier: None });
            }
        }
        let mats = (materials.get(m.0), materials.get(m.1));
        if sp != 0.0 {
            let ra = contact::root_adjust(&p.contacts);
            let c = &mut p.contacts[x];
            let r = [hit.point[0] - pos[0], hit.point[1] - pos[1], hit.point[2] - pos[2]];
            let v = [sp * vr[0], sp * vr[1], sp * vr[2]];
            let depth = hit.depth * sp;
            c.response.impulse_on(r, v, hit.normal, depth, mats, c.grip, if c.is_root { None } else { ra });
            c.last_impulses.push(contact::Impulse { r, v, n: hit.normal, depth, materials: m, with: Some(qname.clone()) });
        }
        if sq != 0.0 {
            let ra = contact::root_adjust(&q.contacts);
            let c = &mut q.contacts[y];
            let r = [hit.point[0] - qpos[0], hit.point[1] - qpos[1], hit.point[2] - qpos[2]];
            let v = [sq * vr[0], sq * vr[1], sq * vr[2]];
            let depth = hit.depth * sq;
            c.response.impulse_on(r, v, hit.normal, depth, mats, c.grip, if c.is_root { None } else { ra });
            c.last_impulses.push(contact::Impulse { r, v, n: hit.normal, depth, materials: m, with: Some(pname.clone()) });
        }
    }
}

/// `checkObjectVsObjects` for part `ka` of `a` against every part of the mover `b`: the pairs
/// with a root on one side, near enough (bounding radii plus the speeds), and not two tiny meshes
/// (≤ 3 vertices each). Which side's vertices go against the other's faces, and with what
/// share, depends on the roots' bounding radii.
pub fn check_part_vs_mover(a: &mut Vehicle, ka: usize, b: &mut Vehicle, materials: &Materials, dt: f32) {
    let (va2, vb2) = (len2(a.craft.body.vel), len2(b.craft.body.vel));
    let (_, pa) = a.contacts[ka].frame(&a.craft.body);
    for kb in 0..b.contacts.len() {
        let (ca, cb) = (&a.contacts[ka], &b.contacts[kb]);
        if !(ca.is_root || cb.is_root) {
            continue;
        }
        if ca.radius < MIN_PART_RADIUS && cb.radius < MIN_PART_RADIUS {
            continue;
        }
        let (_, pb) = cb.frame(&b.craft.body);
        let d = [pa[0] - pb[0], pa[1] - pb[1], pa[2] - pb[2]];
        let reach = ca.radius + va2 * dt * REACH_SPEED + cb.radius + vb2 * dt * REACH_SPEED;
        if len2(d) > reach * reach {
            continue;
        }
        if ca.vertices.len() <= 3 && cb.vertices.len() <= 3 {
            continue;
        }
        // soldiers: two of them push apart; otherwise the soldier's vertices, all of it,
        // but not against a projectile (the response physics' object vs object check returns when the
        // soldier's other is a `ProjectileTemplate`'s: GC's shield or a grenade on the ground; the
        // obstacle the shield carries is no projectile)
        match (ca.soldier, cb.soldier) {
            (true, true) => {
                crate::soldier::push_apart(&mut a.craft.body, &mut b.craft.body);
                continue;
            }
            (true, false) | (false, true) if ca.projectile || cb.projectile => continue,
            (true, false) => {
                check_vertices_on_faces(a, ka, b, kb, materials, dt, 1.0);
                continue;
            }
            (false, true) => {
                check_vertices_on_faces(b, kb, a, ka, materials, dt, 1.0);
                continue;
            }
            _ => {}
        }
        let (ra, rb) = (a.radius, b.radius);
        if rb <= ra {
            if rb <= ra * SMALL_SHARE {
                check_vertices_on_faces(b, kb, a, ka, materials, dt, 1.0);
            } else {
                check_vertices_on_faces(b, kb, a, ka, materials, dt, 0.5);
                check_vertices_on_faces(a, ka, b, kb, materials, dt, 0.5);
            }
        } else if ra <= rb * SMALL_SHARE {
            check_vertices_on_faces(a, ka, b, kb, materials, dt, 1.0);
        } else {
            check_vertices_on_faces(a, ka, b, kb, materials, dt, 0.5);
            check_vertices_on_faces(b, kb, a, ka, materials, dt, 0.5);
        }
    }
}

/// Whether two movers can touch this frame at all (their roots' spheres, grown by the speeds:
/// a superset of every part pair's test).
pub(crate) fn may_touch(a: &Vehicle, b: &Vehicle, dt: f32) -> bool {
    let (pa, pb) = (a.craft.body.pos, b.craft.body.pos);
    let d = [pa[0] - pb[0], pa[1] - pb[1], pa[2] - pb[2]];
    let reach = a.radius + b.radius + (len2(a.craft.body.vel) + len2(b.craft.body.vel)) * dt * REACH_SPEED;
    len2(d) <= reach * reach
}

/// The response physics' update for every mover: first each awake one, part by part, against
/// the level's obstacles, the other movers and the terrain; then all of them (asleep too) solve
/// and add their friction. A pair of parts is tested once a frame, by the first of the two
/// objects to be checked; a sleeping object is not checked itself, but the others check it.
pub fn collide_all(vs: &mut [Vehicle], env: &Surroundings, dt: f32) {
    check_all(vs, env, dt);
    solve_all(vs, dt);
}

/// The first phase of [`collide_all`]: every object checks (its `handleCollision`s happen here:
/// the damage of crashes, comes between this and [`solve_all`]).
pub fn check_all(vs: &mut [Vehicle], env: &Surroundings, dt: f32) {
    for v in vs.iter_mut() {
        contact::begin_pass(&mut v.contacts);
    }
    for i in 0..vs.len() {
        if vs[i].craft.body.is_sleeping() || !vs[i].in_world() || vs[i].fixed || vs[i].physics_off {
            continue;
        }
        let near = {
            let v = &mut vs[i];
            v.craft.body.underwater = 0.0;
            contact::near_obstacles(&v.craft.body, env.obstacles, v.radius, dt)
        };
        // a fixed vehicle (no mobile physics: a fixed gun, DC's Patriot) never checks, so every mover checks against it:
        // its faces are there to be hit
        let others: Vec<usize> = (0..vs.len()).filter(|&j| j != i && vs[j].in_world() && !vs[j].physics_off && (j > i || vs[j].fixed || vs[j].craft.body.is_sleeping()) && may_touch(&vs[i], &vs[j], dt)).collect();
        let drive = vs[i].drive_wheels();
        // a vehicle with a driver goes through the response physics' update:
        // each response solves (and adds its friction) as soon as it has checked, so the next
        // part tests from where this one put the body; the others, `update(dt, null)`: every
        // check, then every solve (in `solve_all`)
        let inline = vs[i].entry.driver.is_some();
        vs[i].solved = inline;
        // and the two go over the parts in a different order: the manager's list (the order of `World::contactors`), or
        // its own tree, the template's
        let mut order: Vec<usize> = (0..vs[i].contacts.len()).collect();
        if inline {
            order.sort_by_key(|&k| vs[i].contacts[k].walked);
        }
        for k in order {
            {
                let v = &mut vs[i];
                contact::check_part_obstacles(&mut v.contacts, k, &v.craft.body, &near, v.radius, env.materials, dt);
            }
            for &j in &others {
                let (a, b) = pair_mut(vs, i, j);
                check_part_vs_mover(a, k, b, env.materials, dt);
            }
            let v = &mut vs[i];
            contact::check_part_terrain(&mut v.contacts, k, &mut v.craft.body, env.ground, env.materials);
            if inline {
                let fb = contact::solve_part(&mut v.contacts[k], &mut v.craft.body, GRAVITY_Y, drive.as_ref());
                v.feed_engine(fb.into_iter().collect());
            }
        }
    }
}

/// The second phase of [`collide_all`]: every object solves.
pub fn solve_all(vs: &mut [Vehicle], dt: f32) {
    for v in vs.iter_mut().filter(|v| v.in_world() && !v.physics_off) {
        if std::mem::take(&mut v.solved) {
            contact::spring_pass(&mut v.contacts, &mut v.craft.body, GRAVITY_Y, dt);
        } else {
            v.solve(dt);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use bytefield_formats::sm::{BspFace, BspNode};

    use super::*;
    use crate::contact::{Response, grip};
    use crate::obstacle::FaceMesh;
    use crate::rigid::RigidBody;
    use crate::vehicle::Component;

    struct Far;
    impl Ground for Far {
        fn height_and_normal(&self, _: f32, _: f32) -> (f32, V3) {
            (-100.0, [0.0, 1.0, 0.0])
        }
        fn material(&self, _: f32, _: f32) -> u32 {
            0
        }
        fn water_level(&self) -> f32 {
            -200.0
        }
    }

    /// A weightless box (half sizes `h`) of `mass` kg, its own collision mesh: 8 corners, 12
    /// faces with outward normals in one BSP leaf.
    fn boxcar(name: &str, h: V3, mass: f32, pos: V3, vel: V3) -> Vehicle {
        let mut verts = Vec::new();
        for x in [-h[0], h[0]] {
            for y in [-h[1], h[1]] {
                for z in [-h[2], h[2]] {
                    verts.push([x, y, z]);
                }
            }
        }
        let idx = |c: [usize; 3]| (c[0] * 4 + c[1] * 2 + c[2]) as u32;
        let mut faces = Vec::new();
        for axis in 0..3 {
            for side in 0..2usize {
                let mut n = [0.0f32; 3];
                n[axis] = if side == 1 { 1.0 } else { -1.0 };
                let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                let corner = |i: usize, j: usize| {
                    let mut c = [0usize; 3];
                    c[axis] = side;
                    c[a] = i;
                    c[b] = j;
                    idx(c)
                };
                faces.push(BspFace { normal: n, v: [corner(0, 0), corner(1, 0), corner(1, 1)], material: 0 });
                faces.push(BspFace { normal: n, v: [corner(0, 0), corner(1, 1), corner(0, 1)], material: 0 });
            }
        }
        let leaf = BspNode { point: [0.0; 3], normal: [0.0, 1.0, 0.0], faces: (0..faces.len() as u32).collect(), front: None, back: None };
        let bbox = [[-h[0], -h[1], -h[2]], h];
        let mesh = FaceMesh { vertices: verts.clone(), faces, nodes: vec![leaf], bounds: bbox };
        let radius = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt();
        let model = Component { name: name.into(), kind: "PlayerControlObject".into(), position: [0.0; 3], rotation: [0.0; 3], params: BTreeMap::new(), children: Vec::new() };
        let mut body = RigidBody::new(pos, mass, 0.0, Some(bbox));
        body.gravity_modifier = 0.0;
        body.vel = vel;
        body.acc = [0.0; 3];
        let contact = Contactor {
            walked: 0,
            name: name.into(),
            materials: vec![0; verts.len()],
            vertices: verts,
            bbox: Some(bbox),
            pos: [0.0; 3],
            rows: crate::craft::IDENTITY,
            grip: grip::CONTACT,
            is_root: true,
            soldier: false,
            projectile: false,
            spring: None,
            response: Response::default(),
            last_impulses: Vec::new(),
                    hits: Vec::new(),
            faces: vec![Arc::new(mesh)],
            scale: [1.0; 3],
            radius,
            anchor: None,
            drive: None,
            forward: [0.0, 0.0, 1.0],
            side: (0.0, [1.0, 0.0, 0.0]),
            engine_above: false,
            chain: Vec::new(),
            chain_anchor: Vec::new(),
        };
        Vehicle { template: name.into(), craft: Craft::new(&model, body), contacts: vec![contact], radius, movement: None, id: 0, armor: None, removed: false, gone_since: None, born: 0, place_gen: 0, team: 0, entry: Default::default(), weapons: Vec::new(), shots: Vec::new(), solved: false, fixed: false, physics_off: false }
    }

    /// `steps` frames of the movers in `vs`; the impulses they got.
    fn run(vs: &mut [Vehicle], steps: usize) -> usize {
        let mats = Materials::default();
        let env = Surroundings { ground: &Far, obstacles: Obstacles::default(), materials: &mats };
        let mut hits = 0;
        for _ in 0..steps {
            for v in vs.iter_mut() {
                v.update_parts(1.0 / 30.0, &Far);
            }
            for v in vs.iter_mut() {
                v.integrate(1.0 / 30.0);
            }
            collide_all(vs, &env, 1.0 / 30.0);
            hits += vs.iter().map(|v| v.contacts[0].last_impulses.len()).sum::<usize>();
        }
        hits
    }

    #[test]
    fn shares_follow_the_masses() {
        assert_eq!(shares(1000.0, 1000.0, 1.0), (0.5, -0.5));
        assert_eq!(shares(1000.0, 1000.0, 0.5), (0.25, -0.25));
        let (a, b) = shares(1000.0, 3000.0, 1.0);
        assert!((a - 0.75).abs() < 1e-6 && (b + 0.25).abs() < 1e-6);
        assert_eq!(shares(1.0, 100.0, 1.0), (1.0, 0.0));
        // the engine's quirk: the much heavier faces' object gets +1
        assert_eq!(shares(100.0, 1.0, 1.0), (0.0, 1.0));
    }

    #[test]
    fn two_equal_boxes_head_on_bounce_back_symmetrically() {
        let h = [2.0, 1.0, 2.0];
        let mut vs = vec![boxcar("a", h, 1000.0, [-2.6, 0.0, 0.0], [5.0, 0.0, 0.0]), boxcar("b", h, 1000.0, [2.6, 0.0, 0.0], [-5.0, 0.0, 0.0])];
        let hits = run(&mut vs, 30);
        assert!(hits > 0);
        let (a, b) = (&vs[0].craft.body, &vs[1].craft.body);
        // mirror images of each other
        assert!((a.pos[0] + b.pos[0]).abs() < 1e-3, "{:?} {:?}", a.pos, b.pos);
        assert!((a.vel[0] + b.vel[0]).abs() < 1e-3, "{:?} {:?}", a.vel, b.vel);
        // momentum along x conserved (it was 0), and they no longer approach
        assert!((a.vel[0] * a.mass + b.vel[0] * b.mass).abs() < 1.0);
        assert!(a.vel[0] <= 1e-3 && b.vel[0] >= -1e-3, "{:?} {:?}", a.vel, b.vel);
        // and they do not go through each other
        assert!(b.pos[0] - a.pos[0] > 3.0, "{:?} {:?}", a.pos, b.pos);
    }

    #[test]
    fn the_lighter_one_takes_more() {
        let h = [2.0, 1.0, 2.0];
        let mut vs = vec![boxcar("heavy", h, 3000.0, [-2.6, 0.0, 0.0], [5.0, 0.0, 0.0]), boxcar("light", h, 1000.0, [2.6, 0.0, 0.0], [0.0, 0.0, 0.0])];
        let p0 = 3000.0 * 5.0;
        run(&mut vs, 30);
        let (a, b) = (&vs[0].craft.body, &vs[1].craft.body);
        let p = a.vel[0] * a.mass + b.vel[0] * b.mass;
        assert!((p - p0).abs() < 0.05 * p0, "momentum {p} vs {p0}");
        // the light one's speed changes three times as much as the heavy one's
        let (da, db) = (5.0 - a.vel[0], b.vel[0]);
        assert!(da > 0.5 && (db - 3.0 * da).abs() < 0.05 * db, "{:?} {:?}", a.vel, b.vel);
    }

    #[test]
    fn a_sleeping_one_is_still_hit() {
        let h = [2.0, 1.0, 2.0];
        let mut vs = vec![boxcar("rest", h, 1000.0, [2.6, 0.0, 0.0], [0.0; 3]), boxcar("moving", h, 1000.0, [-2.6, 0.0, 0.0], [5.0, 0.0, 0.0])];
        vs[0].craft.body.sleepiness = 0;
        run(&mut vs, 30);
        assert!(vs[0].craft.body.vel[0] > 1.0, "{:?}", vs[0].craft.body.vel);
    }
}
