//! Damage and destruction, read from the original game:
//! `Armor` (hit points, critical damage, the wreck), the server's projectile collision handling
//! (a projectile's hit), `handleExplosion`/`handleExplosionOnObject` (radius, damage, push),
//! `handleCollisionLandOrWater` and `handleCollisionObjectVsObject` (crashes) and
//! the object's update handling (the wreck's time to live).
//!
//! The chain for every source: `giveDamage` → `calcDamage` (friendly fire: nothing without
//! players) → `_giveDamage` (nothing on a destroyed armor) → `handleDamage` (negative: heal) →
//! the armor's damage → [`Armor::status`] (critical, destroyed: the wreck explodes).
//! Explosions push at once but their damage waits for the next frame (`gameStatusPlaying` swaps
//! two lists).
//!
//! Friendly fire ([`ServerRules`], [`crate::Sim::friendly_ratio`]): `calcDamage`, for an attacker of team 1 or
//! 2 hitting an object whose root is a player-controlled object of its own team, scales the damage by
//! `soldierFFRatio` (the root a soldier) or `vehicleFFRatio`, and an explosion's by `…OnSplash`
//!. `giveDamage` asks for the first, `handleExplosionOnObject` for the second (and scales a
//! soldier's push as its damage); the explosions' damage of the next frame goes to `_giveDamage`, without the first.
//! The constructor leaves 0.3, 0.8, 1, 1, but the game's `Settings/ServerSettings.con` (and the original's) set
//! `serverSoldierFriendlyFire` & co. to 100 (the setup's world init divides by 100): ratios of 1, our default.
//! The kick-back (`_giveDamage`, [`crate::Sim::kickback`]): a player whose damage reaches a teammate's object with
//! another player of the team in it takes it back × `kickBack` or, in an explosion, `kickBackOnSplash`,
//! on its soldier and through `giveDamage` without an attacker; 0 in `ServerSettings.con`, and CREATE GAME
//! writes 0 there.
//! The sources with an attacker: a projectile's hit, an explosion and a crash against an object with a player (its
//! driver, or the one who left it under 2 s before: `handleCollisionObjectVsObject`, [`crate::Sim::rammer`]; none
//! when the hit one is of its team and goes faster along the crash than it); the
//! ground, the water, the fire of a critical armor and being upside down have none.

use crate::vecmath::{sub, dot};
use bytefield_con::templates::Template;
use serde::Serialize;

use crate::contact::{self, Materials};
use crate::create::{Made, Parent};

use crate::vecmath::V3;

/// The vector normalize: unchanged within ε of unit length, 0 within ε of 0.
pub fn normalize(v: V3) -> V3 {
    let l = dot(v, v);
    if (l - 1.0).abs() < f32::EPSILON {
        return v;
    }
    if l.abs() < f32::EPSILON {
        return [0.0; 3];
    }
    let k = 1.0 / l.sqrt();
    [v[0] * k, v[1] * k, v[2] * k]
}

/// Below this the hit points are 0 (the armor's damage, `setHitPoints`) and the armor is destroyed
/// (`status`).
pub const DEAD_HP: f32 = 0.001;
/// What a destroyed object leaves of its springs' strength and damping (the spring's message handling, a float 0.25).
pub const WRECK_SPRING: f32 = 0.25;
/// A projectile's hit gives no damage at or under this (`handleCollisionForProjectile`).
pub const MIN_HIT_DAMAGE: f32 = 0.01;
/// The critical damage, the upside-down and the wreck run once per this many seconds.
pub const TICK: f32 = 1.0;
/// A crash's attacker may have left the object this long before (`handleCollisionObjectVsObject`).
pub const RAMMER_SECONDS: f64 = 2.0;
/// `holdObject` lets go once the held object's engine turns faster than this.
pub const HOLD_RPM: f32 = 0.1;
/// `createObjectOnAllClients`: a spawner's land or air vehicle waits while a human's soldier is closer (squared)
pub const HUMAN_NEAR: f32 = 2.0;
/// A face of this material destroys whatever touches it (`handleCollision`, 1e10): the plane under a building's floor
pub const KILL_MATERIAL: u32 = 99;
/// The armor's add collision object: an object it collided with is ignored for this long.
pub const COLLISION_MEMORY: f32 = 1.0;
/// The armor's update: upside down is the body's up axis under this (against the terrain's normal
/// on the ground; its y in the air over the ground)
pub const UPSIDE_DOWN: f32 = 0.3;
/// An explosion pushes a soldier only within this share of its radius (`expRadiusCutOnSoldier`),
/// and up by this × (1 − distance / radius) instead of the direction's y
/// (`expUpForeceModifierOnSoldier`): `GameServer`'s defaults (nothing in the scripts sets them).
pub const EXP_RADIUS_CUT_ON_SOLDIER: f32 = 0.5;
pub const EXP_UP_FORCE_ON_SOLDIER: f32 = 5.0;
/// An explosion pushes a soldier under water this much (× its push).
pub const EXP_UNDER_WATER_ON_SOLDIER: f32 = 0.1;
/// `checkForHitOnSoldier`: points around a soldier (offsets from its position, world axes) that
/// an explosion must see, by pose (standing, crouching, lying), and what the count of seen ones
/// is divided by (a standing soldier, with the crouching points over 18, takes half at most).
pub const SOLDIER_BLAST_POINTS: [&[V3]; 3] = [
    &[[0.0, -0.1, 0.0], [0.0, -0.3, 0.0], [0.0, -0.7, 0.0], [-0.2, -0.1, 0.0], [-0.2, -0.3, 0.0], [-0.2, -0.7, 0.0], [0.2, -0.1, 0.0], [0.2, -0.3, 0.0], [0.2, -0.7, 0.0]],
    &[[0.0, -0.1, 0.0], [0.0, -0.3, 0.0], [0.0, -0.7, 0.0], [-0.2, -0.1, 0.0], [-0.2, -0.3, 0.0], [-0.2, -0.7, 0.0], [0.2, -0.1, 0.0], [0.2, -0.3, 0.0], [0.2, -0.7, 0.0]],
    &[[0.0, -0.7, 0.0], [-0.5, -0.7, 0.0], [0.5, -0.7, 0.0]],
];
pub const SOLDIER_BLAST_DIVISOR: [f32; 3] = [18.0, 9.0, 3.0];
/// A soldier's crash counts only the speed above this (m/s; slower: nothing).
pub const SOLDIER_SAFE_SPEED: f32 = 8.0;
/// A soldier's crash counts whole (no angle) from this height fallen (m) and this speed over
/// the safe one (m/s); from `SOLDIER_FULL_SPEED_FROM` the angle starts to count less.
pub const SOLDIER_FULL_HEIGHT: f32 = 2.0;
pub const SOLDIER_FULL_SPEED_FROM: f32 = 10.0;
pub const SOLDIER_FULL_SPEED: f32 = 30.0;

/// The armor part of a `SimpleObjectTemplate` (`hasArmor 1`); engine defaults from its
/// constructor.
#[derive(Debug, Clone, Serialize)]
pub struct ArmorTemplate {
    pub hit_points: f32,
    pub max_hit_points: f32,
    /// `material`: the object's own (explosions read it; −1: none)
    pub material: i64,
    /// `damageMod`: what it gives when it rams something
    pub damage_mod: f32,
    /// `speedMod` (0.05): crash damage per (m/s)²
    pub speed_mod: f32,
    /// `angleMod` (0): the share of a hit's damage that does not depend on the angle
    pub angle_mod: f32,
    /// `criticalDamage`: at or under this many hit points it burns
    pub critical_damage: f32,
    /// `hpLostWhileCriticalDamage` (1) per second while it burns
    pub hp_lost_critical: f32,
    /// `hpLostWhileUpSideDown` (1) per second
    pub hp_lost_upside_down: f32,
    /// the wreck's explosion: `explosionRadius` (15), `explosionDamage` (10), `explosionMaterial`
    /// (204), `explosionForce` (150)
    pub explosion_radius: f32,
    pub explosion_damage: f32,
    pub explosion_material: u32,
    pub explosion_force: f32,
    /// what an explosion does to it: × `explosionForceMod` (1), at most `explosionForceMax` (300)
    pub explosion_force_mod: f32,
    pub explosion_force_max: f32,
    /// `timeToLiveAfterDeath` (10): the wreck stays this long
    pub time_to_live_after_death: f32,
    /// `fadeAtTimeToLiveAfterDeath` (1) and `timeToStartFadeAfterDeath` (8): the wreck fades
    /// out from that many seconds after its death to the end of its time (`wreck_alpha`)
    pub fade_at_time_to_live_after_death: bool,
    pub time_to_start_fade_after_death: f32,
    /// `damageFromWater`: a crash into the water (material 1) gives damage, and so does staying
    /// in it: `hpLostWhileDamageFromWater` (1) a second after `waterDamageDelay` (0) s
    pub damage_from_water: bool,
    pub water_damage_delay: f32,
    pub hp_lost_water: f32,
    /// `damageFromDeepWater` (0), `deepWaterDamageDelay` (1), `hpLostWhileDamageFromDeepWater`
    /// (1); in the water is its position more than `deepWaterLevel` (0) under the surface
    pub damage_from_deep_water: bool,
    pub deep_water_damage_delay: f32,
    pub hp_lost_deep_water: f32,
    pub deep_water_level: f32,
    /// `resetWhenRemoved`: instead of going, the wreck is whole again
    pub reset_when_removed: bool,
    /// `stayAsDestroyed`: the wreck never goes
    pub stay_as_destroyed: bool,
}

impl ArmorTemplate {
    /// From an object template, if it has armor (`hasArmor 1`).
    pub fn from_template(t: &Template) -> Option<ArmorTemplate> {
        let first = |n: &str| t.get(n).and_then(|a| a.first()).map(|v| v.trim().to_string());
        let num = |n: &str, d: f32| first(n).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        let flag = |n: &str| first(n).is_some_and(|v| v != "0");
        if !flag("hasarmor") {
            return None;
        }
        // The simple object template's setting of sink in to land after death speed does not read its argument: it puts
        // the wreck's 10, 8 and 1 back, so what came before it is lost (the
        // scrap and rubble of `MOVE_FILES`, 3 s in the scripts, stay 10)
        let at = |n: &str| t.props.iter().rposition(|p| p.name == n || p.name.strip_prefix("set") == Some(n));
        let sink = at("sinkintolandafterdeathspeed");
        let wreck = |n: &str| first(n).filter(|_| sink.is_none_or(|s| at(n).is_some_and(|i| i > s)));
        Some(ArmorTemplate {
            hit_points: num("hitpoints", 10.0),
            max_hit_points: num("maxhitpoints", 10.0),
            material: first("material").and_then(|v| v.parse().ok()).unwrap_or(-1),
            damage_mod: num("damagemod", 1.0),
            speed_mod: num("speedmod", 0.05),
            angle_mod: num("anglemod", 0.0),
            critical_damage: num("criticaldamage", 0.0),
            hp_lost_critical: num("hplostwhilecriticaldamage", 1.0),
            hp_lost_upside_down: num("hplostwhileupsidedown", 1.0),
            explosion_radius: num("explosionradius", 15.0),
            explosion_damage: num("explosiondamage", 10.0),
            explosion_material: first("explosionmaterial").and_then(|v| v.parse().ok()).unwrap_or(204),
            explosion_force: num("explosionforce", 150.0),
            explosion_force_mod: num("explosionforcemod", 1.0),
            explosion_force_max: num("explosionforcemax", 300.0),
            time_to_live_after_death: wreck("timetoliveafterdeath").and_then(|v| v.parse().ok()).unwrap_or(10.0),
            fade_at_time_to_live_after_death: wreck("fadeattimetoliveafterdeath").is_none_or(|v| v != "0"),
            time_to_start_fade_after_death: wreck("timetostartfadeafterdeath").and_then(|v| v.parse().ok()).unwrap_or(8.0),
            damage_from_water: flag("damagefromwater"),
            water_damage_delay: num("waterdamagedelay", 0.0),
            hp_lost_water: num("hplostwhiledamagefromwater", 1.0),
            damage_from_deep_water: flag("damagefromdeepwater"),
            deep_water_damage_delay: num("deepwaterdamagedelay", 1.0),
            hp_lost_deep_water: num("hplostwhiledamagefromdeepwater", 1.0),
            deep_water_level: num("deepwaterlevel", 0.0),
            reset_when_removed: flag("resetwhenremoved"),
            stay_as_destroyed: flag("stayasdestroyed"),
        })
    }
}

/// Who an object collided with (`Armor`'s list of the last second's collisions): the terrain
/// (or water), a level object, a part of a vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Other {
    Terrain,
    Obstacle(usize),
    Part(usize, usize),
}

/// An `Armor` and the destruction state of its object.
#[derive(Debug, Clone, Serialize)]
pub struct Armor {
    pub template: ArmorTemplate,
    /// the hit points and their maximum
    pub hp: f32,
    pub max_hp: f32,
    /// destroyed; burning (critical) or destroyed
    pub destroyed: bool,
    pub critical: bool,
    /// seconds since the last tick (the critical damage and upside down)
    pub accum: f32,
    /// the objects of the last second's collisions, each with its delay after the
    /// one before it (they leave in order)
    pub recent: Vec<(Other, f32)>,
    /// it collided since the last update
    pub collided: bool,
    /// its height at its last collision (`getLastCollisionHeight`), raised by
    /// the armor's update to its height in every frame without one (the top of a fall); −1 at first
    pub last_collision_height: f32,
    /// the object's flag that it is destroyed and told so (the wreck's clock may run)
    pub dead: bool,
    /// the object's flag that the wreck's clock has started (the first update or collision after)
    pub death_seen: bool,
    /// the object's wreck time left
    pub death_timer: f32,
    /// seconds until the water (the deep water) does damage again
    pub water_timer: f32,
    pub deep_water_timer: f32,
    /// who gave the damage that destroyed it (a vehicle: a projectile's owner), for `killPlayer`
    pub killed_by: Option<usize>,
    /// in the water this update (under its surface by more than `deepWaterLevel`, or a
    /// floater on it)
    pub in_water: bool,
    /// once destroyed: the threshold of the effect it plays (`playEffect`: −1 in the
    /// water, else 0)
    pub wreck_effect: Option<i32>,
}

/// What happened to an armor (for the bench's log).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Status {
    Damaged,
    Critical,
    Repaired,
    Destroyed,
}

impl Armor {
    pub fn new(template: ArmorTemplate) -> Armor {
        // `setArmorComponent`: setMaxHitPoints, then setHitPoints (which raises the max if needed)
        let max_hp = template.max_hit_points.max(template.hit_points);
        let hp = if template.hit_points <= DEAD_HP { 0.0 } else { template.hit_points };
        let (ttl, wt, dwt) = (template.time_to_live_after_death, template.water_damage_delay, template.deep_water_damage_delay);
        Armor { template, hp, max_hp, destroyed: false, critical: false, accum: 0.0, recent: Vec::new(), collided: false, last_collision_height: -1.0, dead: false, death_seen: false, death_timer: ttl, water_timer: wt, deep_water_timer: dwt, killed_by: None, in_water: false, wreck_effect: None }
    }

    /// The armor's status for the new hit points: crossing `criticalDamage` either way, or destruction.
    fn status(&mut self, new: f32) -> Option<Status> {
        let (h, c) = (self.hp, self.template.critical_damage);
        if new > DEAD_HP {
            if (h < c && h > DEAD_HP) || c < new {
                if (c < h && h > DEAD_HP) || new <= c {
                    return None;
                }
                self.destroyed = false;
                self.critical = false;
                return Some(Status::Repaired);
            }
            self.destroyed = false;
            self.critical = true;
            return Some(Status::Critical);
        }
        self.destroyed = true;
        self.critical = true;
        // `handleMessage(0x15)`: the object knows, and its wreck's clock may start
        self.dead = true;
        Some(Status::Destroyed)
    }

    /// The object's damage handling → the armor's damage (or `heal` for a negative amount);
    /// `_giveDamage` does nothing to a destroyed armor. What it did to the armor.
    pub fn give(&mut self, amount: f32) -> Option<Status> {
        if self.destroyed {
            return None;
        }
        let new = if amount <= 0.0 { (self.hp - amount).min(self.max_hp) } else { self.hp - amount };
        let new = if new <= DEAD_HP { 0.0 } else { new };
        let s = self.status(new);
        self.hp = new;
        Some(s.unwrap_or(Status::Damaged))
    }

    /// The armor's setting of hit points (a wreck that `resetWhenRemoved` is whole again).
    pub fn set_hit_points(&mut self, hp: f32) {
        let hp = if hp <= DEAD_HP { 0.0 } else { hp };
        self.status(hp);
        self.hp = hp;
        self.max_hp = self.max_hp.max(hp);
    }

    /// The object's collision handling with an armor: it collided (the armor's collision), and was
    /// this other object not in the last second's list, it now is: true when the damage is to
    /// be worked out (`bf::game` → `handleCollision`).
    pub fn collide_with(&mut self, other: Other, height: f32) -> bool {
        self.collided = true;
        let fresh = self.add_col_object(other);
        if self.destroyed {
            self.death_seen = true;
        }
        self.last_collision_height = height;
        fresh
    }

    /// `isInColList`, and if not, `addColObject`: its delay is what is left of a second after
    /// the ones before it. True when it was not in the list.
    pub fn add_col_object(&mut self, other: Other) -> bool {
        if self.recent.iter().any(|(o, _)| *o == other) {
            return false;
        }
        let ahead: f32 = self.recent.iter().map(|(_, t)| t).sum();
        let t = COLLISION_MEMORY - ahead;
        if self.recent.len() == 15 {
            self.recent.remove(0);
        }
        self.recent.push((other, if t > 0.0 { t } else { COLLISION_MEMORY }));
        true
    }

    /// The armor's update of an object at height `y`: the damage to give itself
    /// (`giveDamage`) for burning and for lying upside down (not a soldier's), once a second;
    /// `upside_down` is asked only then. Then the collision list runs down, and without a
    /// collision this frame the last collision's height rises to `y`.
    pub fn tick(&mut self, dt: f32, y: f32, soldier: bool, upside_down: impl FnOnce(&Armor) -> bool) -> (Option<f32>, Option<f32>) {
        self.tick_in(dt, y, f32::NEG_INFINITY, soldier, upside_down).0
    }

    /// [`Armor::tick`] with the water's surface over the object (`getWaterLevel`): also the
    /// water's damage (in it: its position more than `deepWaterLevel` under the surface; each
    /// timer counts down there, gives its damage at 0 and starts again from 1 s; out of it, back
    /// to its delay).
    pub fn tick_in(&mut self, dt: f32, y: f32, water: f32, soldier: bool, upside_down: impl FnOnce(&Armor) -> bool) -> ((Option<f32>, Option<f32>), (Option<f32>, Option<f32>)) {
        let mut out = (None, None);
        let mut wet = (None, None);
        self.in_water = water > y - self.template.deep_water_level;
        if !self.destroyed {
            let t = &self.template;
            let inside = water > y - t.deep_water_level;
            if inside && t.damage_from_water {
                self.water_timer -= dt;
                if self.water_timer < 0.0 {
                    wet.0 = Some(t.hp_lost_water);
                    self.water_timer = 1.0;
                }
            } else {
                self.water_timer = t.water_damage_delay;
            }
            if inside && t.damage_from_deep_water {
                self.deep_water_timer -= dt;
                if self.deep_water_timer < 0.0 {
                    wet.1 = Some(t.hp_lost_deep_water);
                    self.deep_water_timer = 1.0;
                }
            } else {
                self.deep_water_timer = t.deep_water_damage_delay;
            }
        }
        if !self.destroyed {
            self.accum += dt;
            if self.accum >= TICK {
                if !soldier && self.hp <= self.template.critical_damage && self.template.hp_lost_critical != 0.0 {
                    out.0 = Some(self.accum * self.template.hp_lost_critical);
                }
                if !soldier && self.template.hp_lost_upside_down > 0.01 && upside_down(self) {
                    out.1 = Some(self.accum * self.template.hp_lost_upside_down);
                }
                self.accum = 0.0;
            }
        }
        // the collision list: the delays run out in order
        let mut left = dt;
        while let Some(first) = self.recent.first_mut() {
            first.1 -= left;
            if first.1 >= 0.0 {
                break;
            }
            left = -first.1;
            self.recent.remove(0);
        }
        if !self.collided && self.last_collision_height < y {
            self.last_collision_height = y;
        }
        (out, wet)
    }

    /// The end of update: the collided flag goes back to 0.
    pub fn end_update(&mut self) {
        self.collided = false;
    }

    /// The object's update handling, the wreck's part: true when the wreck goes now. A wreck that
    /// `resetWhenRemoved` is whole again instead.
    pub fn wreck_update(&mut self, dt: f32) -> bool {
        if !self.dead || self.template.stay_as_destroyed {
            return false;
        }
        if !self.death_seen {
            self.death_seen = true;
            return false;
        }
        if self.death_timer >= 0.0 {
            self.death_timer -= dt;
            if self.death_timer > 0.0 {
                return false;
            }
            if self.template.reset_when_removed {
                self.set_hit_points(self.max_hp);
                self.death_seen = false;
                self.death_timer = self.template.time_to_live_after_death;
                self.dead = false;
                return false;
            }
        }
        true
    }

    /// The object's update handling, the fade: once the wreck's time left is under
    /// `timeToLiveAfterDeath − timeToStartFadeAfterDeath` (2 s), its alpha (0–255) is that time
    /// over those seconds; none before (or when it does not fade), and at 0 or less it keeps the
    /// last one given (the caller's). The server sets it on the geometry of the destroyed LOD
    /// (`DistCompareLodSelector2` told to compare), or on its own with no LOD and no parent.
    pub fn wreck_alpha(&self) -> Option<u8> {
        let t = &self.template;
        if !self.dead || !self.death_seen || t.stay_as_destroyed || !t.fade_at_time_to_live_after_death || self.death_timer < 0.0 {
            return None;
        }
        let span = t.time_to_live_after_death - t.time_to_start_fade_after_death;
        if self.death_timer >= span {
            return None;
        }
        let f = self.death_timer / span;
        (f > 0.0).then(|| (f.min(1.0) * 255.0).round() as u8)
    }
}

/// The armor's upside-down test (with the collision of this frame or asleep): low over
/// the terrain (its y − 2 × bounding radius under the ground's height there) and its up axis
/// (`rows[1]`) under 0.3: against the terrain's normal if its last collision was on the ground
/// (under the ground + 0.1), else its y.
pub fn is_upside_down(a: &Armor, asleep: bool, pos: V3, up: V3, radius: f32, ground: (f32, V3)) -> bool {
    if !(a.collided || asleep) {
        return false;
    }
    let (h, n) = ground;
    if pos[1] - (radius + radius) >= h {
        return false;
    }
    if a.last_collision_height >= h + 0.1 { up[1] < UPSIDE_DOWN } else { dot(up, n) < UPSIDE_DOWN }
}

/// `sin(clamp(cos · π/2))` blended with `angleMod`: a hit square on (|v̂ · n̂| = 1) counts whole.
pub fn angle_factor(angle_mod: f32, v: V3, n: V3) -> f32 {
    let c = dot(normalize(v), normalize(n)).abs();
    let x = (c * std::f32::consts::FRAC_PI_2).clamp(-1000.0, 1000.0);
    angle_mod + (1.0 - angle_mod) * x.sin()
}

/// A projectile's damage data (`ProjectileTemplate`): its own `material` decides the hit's
/// damage (`materialDamage` × the damage table), `material2` the explosion's.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProjectileDamage {
    /// `material` (−1: none, then 0)
    pub material: u32,
    /// `damageType`: 1 explodes where it hits (with `hasCollisionEffect`); 1 and 4 when it
    /// detonates
    pub damage_type: i32,
    /// `radius` (10), `material2`, `forceOnExplosion` (150), `YModOnExplosion` (1)
    pub radius: f32,
    pub material2: u32,
    pub force: f32,
    pub ymod: f32,
    /// `minDamage` (1), `distToStartLoseDamage` (0), `distToMinDamage` (0): the damage falls
    /// to `minDamage` × itself between those distances from where it was fired
    pub min_damage: f32,
    pub dist_start_lose: f32,
    pub dist_min: f32,
}

impl Default for ProjectileDamage {
    /// The `ProjectileTemplate` constructor's.
    fn default() -> ProjectileDamage {
        ProjectileDamage { material: 0, damage_type: 0, radius: 10.0, material2: u32::MAX, force: 150.0, ymod: 1.0, min_damage: 1.0, dist_start_lose: 0.0, dist_min: 0.0 }
    }
}

impl ProjectileDamage {
    /// The projectile's damage lookup.
    pub fn damage(&self, materials: &Materials, start: V3, point: V3) -> f32 {
        let d = materials.damage(self.material);
        if self.min_damage < 1.0 && self.dist_start_lose > 0.0 {
            let r = sub(point, start);
            let dist = dot(r, r).sqrt();
            if self.dist_start_lose < dist {
                let f = if dist <= self.dist_min {
                    (1.0 - self.min_damage) * ((self.dist_min - dist) / (self.dist_min - self.dist_start_lose)) + self.min_damage
                } else {
                    self.min_damage
                };
                return d * f;
            }
        }
        d
    }
}

/// `handleCollisionForProjectile`, the hit on an armored object that is not destroyed: the
/// angle factor of its armor × the damage table (projectile's material, the face's) ×
/// `getDamage`. None at or under 0.01.
#[allow(clippy::too_many_arguments)]
pub fn hit_damage(armor: &Armor, p: &ProjectileDamage, materials: &Materials, start: V3, point: V3, v: V3, n: V3, face_material: u32) -> Option<f32> {
    let a = angle_factor(armor.template.angle_mod, v, n);
    let d = a * materials.damage_mod(p.material, face_material) * p.damage(materials, start, point);
    (d > MIN_HIT_DAMAGE).then_some(d)
}

/// The server's rules a round plays by that the briefing shows (`ServerSettings.con`): friendly fire
/// (`calcDamage`, [`crate::Sim::friendly_ratio`]) and the nose camera.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize)]
pub struct ServerRules {
    /// `serverSoldierFriendlyFire`/100: a teammate's soldier hit
    pub soldier_ff: f32,
    /// `serverVehicleFriendlyFire`/100: a teammate's vehicle hit
    pub vehicle_ff: f32,
    /// `serverSoldierFriendlyFireOnSplash`/100: in an explosion
    pub soldier_ff_splash: f32,
    /// `serverVehicleFriendlyFireOnSplash`/100
    pub vehicle_ff_splash: f32,
    /// `serverKickBack`/100: the share of a damage to a teammate's object (with another player in it)
    /// that its attacker's soldier takes back (`_giveDamage`)
    pub kickback: f32,
    /// `serverKickBackOnSplash`/100: in an explosion
    pub kickback_splash: f32,
    /// `serverAllowNoseCam`: F9 inside a vehicle may hide its cockpit (the client's `nose` flag)
    pub nose_cam: bool,
}

impl Default for ServerRules {
    /// The game's `ServerSettings.con`: 100 %, no kick-back and the nose camera allowed.
    fn default() -> Self {
        ServerRules { soldier_ff: 1.0, vehicle_ff: 1.0, soldier_ff_splash: 1.0, vehicle_ff_splash: 1.0, kickback: 0.0, kickback_splash: 0.0, nose_cam: true }
    }
}

impl ServerRules {
    /// CREATE GAME's FRIENDLY FIRE slider (0–100 %): it shows the soldiers' and, on starting, writes it to both
    /// `serverSoldierFriendlyFire` and `serverVehicleFriendlyFire`, not to the splash ones. The setters
    /// clamp to [0, 1].
    pub fn with_friendly_fire(self, percent: f32) -> Self {
        let r = (percent / 100.0).clamp(0.0, 1.0);
        ServerRules { soldier_ff: r, vehicle_ff: r, ..self }
    }

    /// `serverKickBack` and `serverKickBackOnSplash` both at `percent` (CREATE GAME has no control for them and
    /// writes 0), clamped to [0, 1].
    pub fn with_kickback(self, percent: f32) -> Self {
        let r = (percent / 100.0).clamp(0.0, 1.0);
        ServerRules { kickback: r, kickback_splash: r, ..self }
    }

    /// The briefing's FRIENDLY FIRE: the soldiers' (the slider's; with the menu both are the same).
    pub fn friendly_fire_percent(&self) -> f32 {
        self.soldier_ff * 100.0
    }

    /// Over these, what a `ServerSettings.con` sets (the names without case, with or without `set`, as the
    /// console takes them; the percentages clamped to [0, 1] as the setters do). A round started without the menu
    /// (the command line, a dedicated server) plays by the file; CREATE GAME then writes its own.
    pub fn with_con(self, text: &str) -> Self {
        let mut r = self;
        for (name, v) in server_settings(text) {
            let pc = || v.parse::<f32>().ok().map(|p| (p / 100.0).clamp(0.0, 1.0));
            let slot = match name.as_str() {
                "serversoldierfriendlyfire" => &mut r.soldier_ff,
                "servervehiclefriendlyfire" => &mut r.vehicle_ff,
                "serversoldierfriendlyfireonsplash" => &mut r.soldier_ff_splash,
                "servervehiclefriendlyfireonsplash" => &mut r.vehicle_ff_splash,
                "serverkickback" => &mut r.kickback,
                "serverkickbackonsplash" => &mut r.kickback_splash,
                "serverallownosecam" => {
                    if let Ok(n) = v.parse::<f32>() {
                        r.nose_cam = n != 0.0;
                    }
                    continue;
                }
                _ => continue,
            };
            if let Some(p) = pc() {
                *slot = p;
            }
        }
        r
    }
}

/// The `Settings/ServerSettings.con` a round of `mod_name` reads: its mod's, else the game's (the mods ship none).
pub fn server_settings_file(game: &std::path::Path, mod_name: &str) -> Option<String> {
    [mod_name, "bf1942"].iter().find_map(|m| std::fs::read(game.join("Mods").join(m).join("Settings/ServerSettings.con")).ok()).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// A `ServerSettings.con`'s `game.<name> <value>` lines: the name in lower case without `set`, the value without quotes.
pub fn server_settings(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.trim().splitn(2, char::is_whitespace);
            let key = it.next()?.to_ascii_lowercase();
            let name = key.strip_prefix("game.")?;
            let name = name.strip_prefix("set").unwrap_or(name);
            Some((name.to_string(), it.next().unwrap_or("").trim().trim_matches('"').to_string()))
        })
        .collect()
}

#[cfg(test)]
mod server_settings_tests {
    use super::*;

    #[test]
    fn server_settings_con_sets_the_rules() {
        let text = "game.serverName \"BfServer\"\ngame.serverSoldierFriendlyFire 50\ngame.ServerKickBack 250\ngame.setServerVehicleFriendlyFireOnSplash 0\ngame.ServerAllowNoseCam 0\ngame.serverGameStartDelay 20\n";
        let r = ServerRules::default().with_con(text);
        assert_eq!((r.soldier_ff, r.vehicle_ff, r.vehicle_ff_splash, r.kickback, r.kickback_splash, r.nose_cam), (0.5, 1.0, 0.0, 1.0, 0.0, false));
        assert!(server_settings(text).contains(&("servergamestartdelay".to_string(), "20".to_string())));
    }
}

/// An explosion: what `handleExplosionOnObject` needs.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Explosion {
    pub center: V3,
    pub radius: f32,
    pub material: u32,
    pub damage: f32,
    pub force: f32,
    /// × the height difference before the distance (`YModOnExplosion`)
    pub ymod: f32,
    /// who set it off (the vehicle whose projectile it was, or that destroyed the wreck)
    pub by: Option<usize>,
}

/// What an explosion does to one object: its damage (given the next frame) and the push, an
/// acceleration at a point, when it has a physics node.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Blast {
    pub damage: f32,
    pub push: Option<(V3, V3)>,
}

/// `handleExplosionOnObject` for an object with armor at `pos` (its matrix's position), turned by
/// `rows`, with bounding radius `radius`, of material `material`. Inside the radius the damage
/// falls linearly to 0 at the edge; the push (`explosionForceMod` × force / radius, at most
/// `explosionForceMax`, no fall-off) goes along the body axis that points most towards the
/// object from the centre plus that direction, at the point of the bounding sphere nearest the
/// centre. `ground_normal` is the terrain's at the centre (the direction when the centre is the
/// object's position). A soldier (`soldier`: the share of its points the explosion sees, and
/// whether it is under water) takes that share of the damage and of the push (a tenth under
/// water), only within half the radius, and up by 5 × (1 − distance / radius).
#[allow(clippy::too_many_arguments)]
pub fn blast(e: &Explosion, armor: &Armor, materials: &Materials, pos: V3, rows: &[V3; 3], radius: f32, mobile: bool, ground_normal: V3, soldier: Option<(f32, bool)>) -> Option<Blast> {
    let seen = soldier.map_or(1.0, |s| s.0);
    if seen <= 0.0 {
        return None;
    }
    let m = if armor.template.material < 0 { u32::MAX } else { armor.template.material as u32 };
    let modi = materials.damage_mod(e.material, m);
    let d = sub(pos, e.center);
    let d = [d[0], e.ymod * d[1], d[2]];
    let dist = dot(d, d).sqrt();
    if dist >= e.radius {
        return None;
    }
    let inv = 1.0 / e.radius;
    let f = ((e.radius - dist) * inv).clamp(0.0, 1.0);
    let damage = f * e.damage * modi * seen;
    if damage <= 0.0 {
        return None;
    }
    let mut push = None;
    if mobile {
        let mut force = (armor.template.explosion_force_mod as f64 * e.force as f64 * inv as f64 * seen as f64) as f32;
        if soldier.is_some_and(|s| s.1) {
            force *= EXP_UNDER_WATER_ON_SOLDIER;
        }
        if armor.template.explosion_force_max < force {
            force = armor.template.explosion_force_max;
        }
        let dir = if 0.001 <= dist {
            let (fy, fx, fz) = (dot(rows[1], d), dot(rows[0], d), dot(rows[2], d));
            let (axis, s) = if fy.abs() <= fx.abs() || fy.abs() <= fz.abs() {
                if fz.abs() <= fx.abs() { (rows[0], fx) } else { (rows[2], fz) }
            } else {
                (rows[1], fy)
            };
            let axis = if s < 0.0 { [-axis[0], -axis[1], -axis[2]] } else { axis };
            let (a, mut dn) = (normalize(axis), normalize(d));
            if soldier.is_some() {
                if dist / e.radius <= EXP_RADIUS_CUT_ON_SOLDIER {
                    dn[1] = (1.0 - dist / e.radius) * EXP_UP_FORCE_ON_SOLDIER;
                } else {
                    force = 0.0;
                }
            }
            [dn[0] + a[0], dn[1] + a[1], dn[2] + a[2]]
        } else {
            ground_normal
        };
        let dir = normalize(dir);
        let acc = [force * dir[0], force * dir[1], force * dir[2]];
        let r = sub(e.center, pos);
        let r2 = dot(r, r);
        let point = if r2 > radius * radius {
            let k = radius / r2.sqrt();
            [r[0] * k + pos[0], r[1] * k + pos[1], r[2] * k + pos[2]]
        } else {
            e.center
        };
        push = Some((point, acc));
    }
    Some(Blast { damage, push })
}

/// A soldier's part of a crash: the height of its last collision (the top of a fall), its own
/// height now and its kit's damping (`getDamageDampingFromActiveKitParts`: the least `damping`
/// of its active kit parts, 1 without any).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Fall {
    pub last_height: f32,
    pub y: f32,
    pub damping: f32,
}

/// The soldier's factors of a crash at speed `s` (already less the safe 8 m/s) whose angle
/// factor is `f`: from 1 m fallen (the last collision's height − its own − 1) the angle counts
/// less (not at all from 2 m) and the damage grows with the square of the height × the kit's
/// damping (at least 1); from 10 m/s over the safe speed the angle counts less, from 30 not at
/// all. (f, g): the damage is f × … × g².
fn soldier_factors(mut f: f32, s: f32, fall: &Fall) -> (f32, f32) {
    let h = fall.last_height - fall.y - 1.0;
    let mut hh = 1.0;
    if h >= 1.0 {
        hh = h;
        f = if h >= SOLDIER_FULL_HEIGHT { 1.0 } else { (h - 1.0) * (1.0 - f) + f };
    }
    let g = hh * fall.damping;
    let g = if g < 1.0 { 1.0 } else { g };
    if s > SOLDIER_FULL_SPEED {
        f = 1.0;
    } else if s > SOLDIER_FULL_SPEED_FROM {
        f = (1.0 - f) * ((s - SOLDIER_FULL_SPEED_FROM) / (SOLDIER_FULL_SPEED - SOLDIER_FULL_SPEED_FROM)) + f;
    }
    (f, g)
}

/// `checkForHitOnSoldier`: the share of a soldier's points (by pose, [`SOLDIER_BLAST_POINTS`])
/// an explosion at `center` sees (`checkIfRayHitsSoldier`): a segment from the centre to each
/// point that no other object's faces (collision LOD 1) cut, nor the terrain. The terrain is
/// asked along the segment when it goes up; when it goes down, along the opposite one from the
/// centre less the segment (as the server does: that never meets the ground under it).
pub fn soldier_seen(level: &bytefield_world::Level, obstacles: &[crate::obstacle::Obstacle], vehicles: &[crate::mover::Vehicle], me: usize, center: V3, pos: V3, pose: usize) -> f32 {
    use crate::obstacle::{pick_lod, segment_on_skipping};
    use crate::projectile::{NON_SOLID_MATERIAL, terrain_segment};
    let pose = pose.min(2);
    let blocked = |dir: V3| {
        let near = |c: V3, r: f32| {
            let l2 = dot(dir, dir);
            let t = if l2 > 0.0 { (dot(sub(c, center), dir) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let p = [center[0] + dir[0] * t, center[1] + dir[1] * t, center[2] + dir[2] * t];
            let d = sub(c, p);
            dot(d, d) <= r * r
        };
        for o in obstacles.iter().filter(|o| near(o.center, o.radius + 1.0)) {
            if let Some(mesh) = pick_lod(&o.lods, 1)
                && segment_on_skipping(mesh, &o.rows, o.pos, o.scale, center, dir, false, Some(NON_SOLID_MATERIAL)).is_some()
            {
                return true;
            }
        }
        for (k, v) in vehicles.iter().enumerate() {
            if k == me || v.removed || !near(v.craft.body.pos, v.radius + 1.0) {
                continue;
            }
            for c in &v.contacts {
                let Some(mesh) = pick_lod(&c.faces, 1) else { continue };
                let (rows, at) = c.frame(&v.craft.body);
                if segment_on_skipping(mesh, &rows, at, c.scale, center, dir, false, Some(NON_SOLID_MATERIAL)).is_some() {
                    return true;
                }
            }
        }
        if dir[1] >= 0.0 {
            terrain_segment(level, center, dir).is_some()
        } else {
            terrain_segment(level, sub(center, dir), [-dir[0], -dir[1], -dir[2]]).is_some()
        }
    };
    let pts = SOLDIER_BLAST_POINTS[pose];
    let n = pts.iter().filter(|o| !blocked(sub([pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]], center))).count();
    n as f32 / SOLDIER_BLAST_DIVISOR[pose]
}

/// `handleCollisionLandOrWater`: a crash on the terrain, or into the water (`ground_material`
/// 1). |v̂ · n̂|³ (in the water ², and only with `damageFromWater`) × `speedMod` × |v|² × the
/// damage table (the ground's material, its own) × the ground material's damage; on land given
/// if over 1, in the water whatever it is. A soldier (`fall`) counts only the speed over 8 m/s,
/// and its fall's height and speed raise it ([`soldier_factors`]).
pub fn land_damage(armor: &Armor, materials: &Materials, v: V3, n: V3, ground_material: u32, own_material: u32, fall: Option<&Fall>) -> Option<f32> {
    let water = ground_material == crate::projectile::WATER_MATERIAL;
    let c = dot(normalize(v), normalize(n)).abs();
    let mut s = dot(v, v).sqrt();
    if fall.is_some() {
        s -= SOLDIER_SAFE_SPEED;
        if s < 0.0 {
            return None;
        }
    }
    if water && !armor.template.damage_from_water {
        return None;
    }
    let f = if water { c * c } else { c * c * c };
    let base = match fall {
        // `speedMod` × |v| × |v| in the FPU's precision
        Some(_) => (armor.template.speed_mod as f64 * s as f64 * s as f64) as f32,
        None => armor.template.speed_mod * dot(v, v),
    };
    let (dm, md) = (materials.damage_mod(ground_material, own_material), materials.damage(ground_material));
    let d = match fall {
        Some(fall) => {
            let (f, g) = soldier_factors(f, s, fall);
            f * base * dm * md * g * g
        }
        None => f * base * dm * md,
    };
    (water || d > 1.0).then_some(d)
}

/// `handleCollisionObjectVsObject` (no players): the rammer's `damageMod` × the angle factor of
/// the victim's armor × its `speedMod` × |v|² × the damage table (rammer's material, its own) ×
/// the rammer material's damage, given if over 1. A soldier victim (`fall`, with `speed`: the
/// other's speed along v plus its own over 8 m/s, [`soldier_ram_speed`]) has no rammer's
/// `damageMod` and its fall's factors ([`soldier_factors`]); under 8 m/s of v nothing.
#[allow(clippy::too_many_arguments)]
pub fn ram_damage(victim: &Armor, rammer_damage_mod: f32, materials: &Materials, v: V3, n: V3, rammer_material: u32, own_material: u32, soldier: Option<(&Fall, f32)>) -> Option<f32> {
    let a = angle_factor(victim.template.angle_mod, v, n);
    let (dm, md) = (materials.damage_mod(rammer_material, own_material), materials.damage(rammer_material));
    let d = match soldier {
        Some((fall, s)) => {
            if dot(v, v).sqrt() < SOLDIER_SAFE_SPEED {
                return None;
            }
            let base = (victim.template.speed_mod as f64 * s as f64 * s as f64) as f32;
            let (f, g) = soldier_factors(a, s, fall);
            f * base * dm * md * g * g
        }
        None => rammer_damage_mod * a * (victim.template.speed_mod * dot(v, v)) * dm * md,
    };
    (d > 1.0).then_some(d)
}

/// The speed of a crash on a soldier (`handleCollisionObjectVsObject`, no players): the length
/// of the other's velocity along v, plus that of its own less 8 m/s (at least 0).
pub fn soldier_ram_speed(v: V3, own: V3, other: V3) -> f32 {
    let along = |u: V3| {
        let l = dot(v, v);
        if l > 0.0 {
            let k = dot(u, v) / l;
            let p = [v[0] * k, v[1] * k, v[2] * k];
            dot(p, p).sqrt()
        } else {
            0.0
        }
    };
    let own = (along(own) - SOLDIER_SAFE_SPEED).max(0.0);
    along(other) + own
}

/// Where a damage came from (the bench's log).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Source {
    /// a projectile's hit (`handleCollisionForProjectile`)
    Hit,
    /// an explosion's (given the frame after it)
    Explosion,
    /// a crash against the terrain, an object or another vehicle
    Crash,
    /// the kick-back of a damage to a teammate's object ([`ServerRules::kickback`])
    Kickback,
    /// `hpLostWhileCriticalDamage`
    Burning,
    /// `hpLostWhileUpSideDown`
    UpsideDown,
    /// `hpLostWhileDamageFromWater`, `hpLostWhileDamageFromDeepWater`
    Water,
    /// `killPlayer`'s (100030): the driver of a vehicle destroyed under it
    Kill,
    /// `damageForBeingOutSideWorld`: its player past its time outside the play area
    Outside,
    /// `damageWhenLost`: abandoned far from its spawner past its `timeToLive`
    Lost,
    /// the medic's pack (`useMedPack`: a negative damage)
    Heal,
    /// the engineer's wrench (`useRepairPack` → the armor's heal)
    Repair,
    /// the AI's simulated hit on a bot at a level of detail above 0
    AiHit,
    /// 1e10 to destroy an object outright (`destroyOnExit`, `destroyVehicleWhenNoAmmo`)
    Destroy,
}

/// One damage given to a vehicle.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DamageEvent {
    pub step: u64,
    pub vehicle: usize,
    pub amount: f32,
    pub source: Source,
    /// what it did (None: the armor was already destroyed)
    pub status: Option<Status>,
    /// hit points after it
    pub hp: f32,
    /// who gave it (`giveDamage`'s attacker: the vehicle of the player that fired), when known
    pub by: Option<usize>,
}

/// An `ObjectSpawner` in the sim: its slot and its countdown.
#[derive(Debug, Clone, Serialize)]
pub struct SpawnerState {
    pub name: String,
    /// `Object.setName`
    pub label: Option<String>,
    /// what it makes now (its team's), and for each team (`setObjectTemplate`)
    pub template: String,
    pub templates: Vec<(u32, String)>,
    /// its team (`setTeam`, or its control point's); its control point (`setOSId`); whether it
    /// works (a neutral point's spawners do not)
    pub team: u32,
    pub os_id: Option<i64>,
    pub active: bool,
    /// where it makes the object (its position plus `spawnOffset`), its own position, rotation
    pub at: V3,
    pub pos: V3,
    pub rotation: V3,
    /// the objects in its slots (`maxNrOfObjectSpawned`; None: empty) and the last one made
    pub slots: Vec<Option<Made>>,
    pub last: Option<Made>,
    /// seconds until the next one (runs while the slot is empty)
    pub countdown: f32,
    /// `calcSpawnDelay`: with no players, `maxSpawnDelay`
    pub delay: f32,
    pub radius: f32,
    /// what it hangs from: a vehicle's or a projectile's spawner (a ship's planes; DC's deployed
    /// mortar and PKM, GC's E-Web) follows it each frame (`at`, `pos` and `rows`) and stops when it
    /// is gone; a static's stays where it was made
    pub parent: Parent,
    /// in a tree: its axes and position in the root's frame
    pub local: Option<([V3; 3], V3)>,
    /// its axes, when not those of `rotation` (in a tree)
    pub rows: Option<[V3; 3]>,
    /// `spawnOffset` (world axes)
    pub offset: V3,
    /// `holdObject`, and its object held still (the spawner's place, the carrier's speed there)
    /// until its engine turns or it is destroyed
    pub hold: bool,
    pub holding: bool,
    /// `teamOnVehicle`: what it makes has its team
    #[serde(default)]
    pub team_on_vehicle: bool,
    /// `distance`, `timeToLive`, `damageWhenLost`: what it gives the objects it makes
    pub lost: Option<(f32, f32, f32)>,
}

impl SpawnerState {
    /// A spawner `sp` in the tree of an object hanging from `parent` at `m` (the object's frame; it
    /// is placed by the next `follow`): its first object at once, unless `spawnDelayAtStart`.
    pub fn child(sp: &bytefield_world::Spawner, m: &bytefield_world::scene::Mat4, parent: Parent) -> SpawnerState {
        let c = m.0;
        let lr = [0, 1, 2].map(|r| [c[r][0] as f32, c[r][1] as f32, c[r][2] as f32]);
        let lp = [c[3][0] as f32, c[3][1] as f32, c[3][2] as f32];
        let f = |v: [f64; 3]| v.map(|x| x as f32);
        SpawnerState {
            name: sp.name.clone(),
            label: None,
            template: sp.template().unwrap_or("").to_string(),
            templates: sp.templates.clone(),
            team: sp.team,
            os_id: None,
            active: true,
            at: [0.0; 3],
            pos: [0.0; 3],
            rotation: [0.0; 3],
            slots: vec![None; sp.max_objects.max(1)],
            last: None,
            countdown: if sp.delay_at_start { sp.max_delay as f32 } else { -1.0 },
            delay: sp.max_delay as f32,
            radius: sp.radius as f32,
            parent,
            local: Some((lr, lp)),
            rows: None,
            offset: f(sp.offset),
            hold: sp.hold_object,
            team_on_vehicle: sp.team_on_vehicle,
            holding: false,
            lost: Some((sp.distance as f32, sp.time_to_live as f32, sp.damage_when_lost as f32)),
        }
    }

    /// The spawner's setting of team + `setActive` from its control point: the next object is the
    /// new team's; switching it restarts its countdown (unless it is −1, at the start).
    pub fn switch(&mut self, w: &crate::conquest::SpawnerSwitch) {
        self.team = w.team;
        self.template = self.templates.iter().rev().find(|(t, _)| *t == w.team).map_or(String::new(), |(_, n)| n.clone());
        self.active = w.active;
        if self.countdown != -1.0 {
            self.countdown = self.delay;
        }
    }

    /// The abandonment rule for an object it makes.
    pub fn abandon(&self) -> Option<crate::entry::Abandon> {
        self.lost.map(|(distance, ttl, damage)| crate::entry::Abandon { spawner: self.pos, distance, ttl, damage, timer: ttl })
    }

    /// The spawner `s` of the level (or of a static's tree, `Static`), as the round starts:
    /// its first object at once, unless `spawnDelayAtStart`.
    pub fn of(s: &bytefield_world::Spawner, parent: Parent) -> SpawnerState {
        let f = |v: [f64; 3]| v.map(|x| x as f32);
        SpawnerState {
            name: s.name.clone(),
            label: s.label.clone(),
            template: s.template().unwrap_or("").to_string(),
            templates: s.templates.clone(),
            team: s.team,
            os_id: s.os_id,
            active: true,
            at: f([0, 1, 2].map(|i| s.pos[i] + s.offset[i])),
            pos: f(s.pos),
            rotation: f(s.rotation),
            slots: vec![None; s.max_objects.max(1)],
            last: None,
            countdown: if s.delay_at_start { s.max_delay as f32 } else { -1.0 },
            delay: s.max_delay as f32,
            radius: s.radius as f32,
            parent,
            local: None,
            rows: None,
            offset: f(s.offset),
            hold: s.hold_object,
            team_on_vehicle: s.team_on_vehicle,
            holding: false,
            lost: Some((s.distance as f32, s.time_to_live as f32, s.damage_when_lost as f32)),
        }
    }

    /// Its parent is a projectile.
    pub fn on_projectile(&self) -> bool {
        matches!(self.parent, Parent::Projectile(_))
    }

    /// A child spawner's place from its parent's position and axes.
    pub fn follow(&mut self, pos: V3, b: [V3; 3]) {
        let Some((lr, lp)) = self.local else { return };
        let world = |v: V3| [0, 1, 2].map(|k| v[0] * b[0][k] + v[1] * b[1][k] + v[2] * b[2][k]);
        let p = world(lp);
        self.pos = [pos[0] + p[0], pos[1] + p[1], pos[2] + p[2]];
        self.at = [self.pos[0] + self.offset[0], self.pos[1] + self.offset[1], self.pos[2] + self.offset[2]];
        self.rows = Some(lr.map(world));
    }
}

impl crate::Sim<'_> {
    /// `giveDamage` to vehicle `v` (and what follows: a destroyed one's wreck explodes).
    pub fn give_damage(&mut self, v: usize, amount: f32, source: Source) {
        // (a `remote` copy's damage is the server's: its own crashes, water and burning give none; [`Snapshot::write`] kills what the
        // server destroyed with `remote` off)
        if self.role.remote {
            return;
        }
        // (The server's give damage does nothing out of status 1: in the pre-game (3) and the end of a round (2, 5), none)
        if self.round.pregame > 0.0 || self.round.round_end.is_some() {
            return;
        }
        let step = self.clock.steps;
        // the kick-back looks at who is in it before the damage (a destroyed one empties)
        let kick = self.kickback(v, amount, source);
        let Some(veh) = self.vehicles.get_mut(v) else { return };
        if veh.removed {
            return;
        }
        let Some(a) = veh.armor.as_mut() else { return };
        let status = a.give(amount);
        if status == Some(Status::Destroyed) {
            a.killed_by = self.damage.attacker;
            // The spring's message handling: under a destroyed object every spring's node keeps a quarter of its strength
            // and damping (the wreck sags; again on each destruction). GC's skiff wrecked on GC_BCanyon pushed 4 times the
            // original's on its wheels' first touch
            for c in veh.contacts.iter_mut() {
                if let Some(sp) = c.spring.as_mut() {
                    sp.strength *= WRECK_SPRING;
                    sp.damping *= WRECK_SPRING;
                }
            }
        }
        let (hp, t) = (a.hp, a.template.clone());
        let pos = veh.craft.body.pos;
        self.journal.damage_log.push(DamageEvent { step, vehicle: v, amount, source, status, hp, by: self.damage.attacker });
        // who is in it dies with it (`killPlayer`: out of the vehicle, the soldier destroyed and
        // given 100030; seen for the driver), before the wreck explodes
        // a soldier's death animation
        if status == Some(Status::Destroyed) && self.vehicles[v].movement.is_some() {
            let seated = self.vehicles.iter().any(|o| o.entry.occupants.contains(&Some(v)));
            let rows = self.vehicles[v].craft.body.rows;
            if let Some(m) = self.vehicles[v].movement.as_mut() {
                m.die(&rows, seated);
            }
        }
        // a hit it survives: off the ladder, and the upper body's flinch (same function; giveDamage
        // does nothing before the round is playing, and a bench's damage there shows nothing)
        if self.round.playing && amount > 0.0 && status != Some(Status::Destroyed) && hp > 0.0 && self.vehicles[v].movement.is_some() {
            let veh = &mut self.vehicles[v];
            let (body, m) = (&mut veh.craft.body, veh.movement.as_mut().unwrap());
            if m.climbing() {
                m.stop_climbing(body);
            }
            let rows = body.rows;
            m.hurt(&rows);
        }
        if status == Some(Status::Destroyed) {
            for d in self.vehicles[v].entry.occupants.clone().into_iter().flatten() {
                // one flying a guided missile goes back to its launcher (`exitAndPlaceCameraIfApplicable`), and dies
                // only if it is still in this one
                if self.return_seat(d, v).is_some() {
                    self.exit(d, v);
                    continue;
                }
                self.kill_driver(v, d);
            }
        }
        if status == Some(Status::Destroyed) && t.explosion_damage > DEAD_HP {
            let e = Explosion { center: pos, radius: t.explosion_radius, material: t.explosion_material, damage: t.explosion_damage, force: t.explosion_force, ymod: 1.0, by: self.damage.attacker };
            self.explode(e, None);
        }
        if let Some((s, d)) = kick {
            let by = self.damage.attacker.take();
            self.give_damage(s, d, Source::Kickback);
            self.damage.attacker = by;
        }
    }

    /// `_giveDamage`'s kick-back of `amount` to vehicle `v` ([`ServerRules::kickback`]): the attacker's player's
    /// soldier and its share, when the attacker is a player's, `v` has armor not yet destroyed and another player
    /// of the attacker's team is in it (its soldier, or in a seat), and the share is over 0.01.
    pub fn kickback(&self, v: usize, amount: f32, source: Source) -> Option<(usize, f32)> {
        let k = self.player_of_attacker(self.damage.attacker?)?;
        let veh = self.vehicles.get(v).filter(|x| !x.removed)?;
        veh.armor.as_ref().filter(|a| !a.destroyed)?;
        let team = self.players[k].team;
        let mate = self.players.iter().enumerate().any(|(j, p)| {
            j != k && p.team == team && p.soldier.is_some_and(|s| s == v || self.vehicles.get(s).is_some_and(|x| x.entry.inside == Some(v)))
        });
        // (`giveDamage`'s flag: the projectile's hit, the explosion and the crash ask for it)
        let r = match source {
            Source::Explosion => self.rules.server.kickback_splash,
            Source::Hit | Source::Crash => self.rules.server.kickback,
            _ => return None,
        };
        let d = amount * r;
        let s = self.players[k].soldier?;
        (mate && d > MIN_HIT_DAMAGE).then_some((s, d))
    }

    /// `handleCollisionObjectVsObject`'s attacker when object `j` hits another: its player (a soldier's own, a
    /// vehicle's driver), or the one who got out of it under 2 s before (the PCO's last player and its time). The soldier stands for the player ([`crate::Sim::attacker`]).
    pub fn rammer(&self, j: usize) -> Option<usize> {
        let v = self.vehicles.get(j)?;
        if v.movement.is_some() {
            return self.players.iter().any(|p| p.soldier == Some(j)).then_some(j);
        }
        let player = |s: usize| self.players.iter().any(|p| p.soldier == Some(s));
        if let Some(d) = v.entry.occupants.first().copied().flatten().filter(|&d| player(d)) {
            return Some(d);
        }
        (0..self.vehicles.len()).filter(|&s| player(s)).find(|&s| self.vehicles[s].entry.exited.is_some_and(|(x, t)| x == j && self.clock.time - t < RAMMER_SECONDS))
    }

    /// `calcDamage`'s friendly fire: the share of a damage `by` gives `target` ([`ServerRules`]). 1 but for an
    /// attacker of team 1 or 2 and a target of its team (the root's `getTeam`; every object of `vehicles` is a
    /// player-controlled one, `create.rs`), a soldier or not, in an explosion (`splash`) or not. It does not
    /// look at who: one's own grenade counts as a teammate's.
    pub fn friendly_ratio(&self, by: Option<usize>, target: usize, splash: bool) -> f32 {
        let Some(team) = by.and_then(|a| self.vehicles.get(a)).map(|v| v.team).filter(|t| (1..=2).contains(t)) else { return 1.0 };
        let Some(v) = self.vehicles.get(target).filter(|v| v.team == team) else { return 1.0 };
        let r = &self.rules;
        match (v.movement.is_some(), splash) {
            (true, false) => r.server.soldier_ff,
            (false, false) => r.server.vehicle_ff,
            (true, true) => r.server.soldier_ff_splash,
            (false, true) => r.server.vehicle_ff_splash,
        }
    }

    /// `handleExplosion`: every vehicle with armor but `skip` (the armor a projectile hit), pushed
    /// now, damaged the next frame.
    pub fn explode(&mut self, e: Explosion, skip: Option<usize>) {
        self.journal.explosions.push((self.clock.steps, e));
        if self.role.remote {
            return;
        }
        let materials = self.world.materials();
        let ground_n = contact::Ground::height_and_normal(&self.world.level, e.center[0], e.center[2]).1;
        // what the explosion sees of each soldier (`checkForHitOnSoldier`)
        let seen: Vec<Option<f32>> = (0..self.vehicles.len())
            .map(|j| {
                let v = &self.vehicles[j];
                let m = v.movement.as_ref().filter(|_| !v.removed && v.armor.is_some())?;
                Some(soldier_seen(&self.world.level, self.world.obstacles(), &self.vehicles, j, e.center, v.craft.body.pos, m.pose()))
            })
            .collect();
        // (`calcDamage` on the splash: a soldier's push goes as its damage)
        let ratios: Vec<f32> = (0..self.vehicles.len()).map(|j| self.friendly_ratio(e.by, j, true)).collect();
        for (j, v) in self.vehicles.iter_mut().enumerate() {
            if v.removed || Some(j) == skip {
                continue;
            }
            let Some(a) = v.armor.as_ref() else { continue };
            let body = &v.craft.body;
            let soldier = seen[j].map(|f| (f, body.underwater > 0.0));
            let Some(mut b) = blast(&e, a, materials, body.pos, &body.rows, v.radius, true, ground_n, soldier) else { continue };
            b.damage *= ratios[j];
            if v.movement.is_some()
                && let Some((_, acc)) = b.push.as_mut()
            {
                *acc = acc.map(|x| x * ratios[j]);
            }
            if let Some((point, acc)) = b.push {
                // (`triggerFallingAnimation`: a soldier may fly for a moment)
                if let Some(m) = v.movement.as_mut() {
                    m.blasted_at = m.world_time;
                }
                v.craft.body.sleepiness = crate::rigid::FRAMES_BEFORE_SLEEP;
                v.craft.body.add_acc_at(point, acc);
                self.journal.pushes.push((self.clock.steps, j, point, acc));
            }
            self.damage.pending_damage.push((j, b.damage, e.by));
        }
    }

    /// `gameStatusPlaying`: the explosions' damage of the frame before.
    pub(crate) fn apply_pending_damage(&mut self) {
        for (v, d, by) in std::mem::take(&mut self.damage.pending_damage) {
            self.damage.attacker = by;
            self.give_damage(v, d, Source::Explosion);
            self.damage.attacker = None;
        }
    }

    /// Every armor's update and its object's update handling (the wreck's
    /// clock): burning, upside down, a wreck whose time ran out goes.
    pub(crate) fn update_armors(&mut self, dt: f32) {
        let level = &self.world.level;
        for i in 0..self.vehicles.len() {
            let v = &mut self.vehicles[i];
            if v.removed {
                continue;
            }
            let (pos, up, asleep, radius, soldier) = (v.craft.body.pos, v.craft.body.rows[1], v.craft.body.is_sleeping(), v.radius, v.is_soldier());
            let Some(a) = v.armor.as_mut() else { continue };
            let ground = contact::Ground::height_and_normal(level, pos[0], pos[2]);
            // a soldier in a seat has no physics of its own: not in the water (a submarine's
            // crew drowns by its oxygen, `damageAllAttachedSoldiers`)
            let water = if v.entry.inside.is_some() { f32::NEG_INFINITY } else { contact::Ground::water_level(level) };
            let ((burn, flip), (wet, deep)) = a.tick_in(dt, pos[1], water, soldier, |a| is_upside_down(a, asleep, pos, up, radius, ground));
            if let Some(d) = burn {
                self.give_damage(i, d, Source::Burning);
            }
            if let Some(d) = flip {
                self.give_damage(i, d, Source::UpsideDown);
            }
            for d in [wet, deep].into_iter().flatten() {
                self.give_damage(i, d, Source::Water);
            }
            let v = &mut self.vehicles[i];
            let Some(a) = v.armor.as_mut() else { continue };
            a.in_water |= v.craft.body.underwater > 0.0 || v.craft.afloat;
            a.end_update();
            // The armor's play effect on a destroyed vehicle: the effects of threshold −1 in the water, 0
            // if not (the client's). Their emitters hang from the vehicle: an `IsSpawnEffect` one
            // creates nothing (the emitter's message handling wants no parent), so a sunk PT boat leaves
            // no raft
            if a.destroyed {
                a.wreck_effect = Some(if a.in_water { -1 } else { 0 });
            }
            if a.wreck_update(dt) {
                v.removed = true;
            }
        }
    }

    /// The `handleCollision`s of this frame's collision pass ([`Contactor::hits`]): each armor
    /// works out a crash once a second per other object (`isInColList`).
    // (a vehicle created into a building, a MiG-29's wing in El_Alamein's allied base, is pushed out with 33 m/s up and takes 215 of crash on
    // its first step, here and in the original's server, with Object.create or from a spawner)
    pub(crate) fn crash_damage(&mut self) {
        let materials = self.world.materials();
        let mut give = Vec::new();
        for i in 0..self.vehicles.len() {
            if self.vehicles[i].removed || self.vehicles[i].armor.is_none() {
                continue;
            }
            let hits: Vec<contact::Hit> = self.vehicles[i].contacts.iter().flat_map(|c| c.hits.iter().copied()).collect();
            for h in hits {
                // the other side's material 99 (the plane under a building's floor, `barack_m1`):
                // 1e10, before anything else (`SimpleObject`, `PlayerControlObject` and
                // The soldier's collision handling)
                if h.materials.1 == KILL_MATERIAL {
                    give.push((i, crate::entry::DESTROY_DAMAGE, Source::Destroy, None));
                    break;
                }
                // a soldier against an `Obstacle` (barbed wire): its damage, once a second, and
                // no collision handling (no crash, no collision height)
                if let Some(d) = h.barrier {
                    if self.vehicles[i].armor.as_mut().unwrap().add_col_object(h.other) {
                        give.push((i, d, Source::Crash, None));
                    }
                    continue;
                }
                // material 37 (0x25) on either side: nothing
                if h.materials.0 == 37 || h.materials.1 == 37 {
                    continue;
                }
                let rammer_mod = match h.other {
                    Other::Part(j, _) if j == i || j == crate::projectile::PROJECTILE_ID => continue,
                    Other::Part(j, _) => self.vehicles.get(j).and_then(|v| v.armor.as_ref()).map_or(1.0, |a| a.template.damage_mod),
                    _ => 1.0,
                };
                let soldier = self.vehicles[i].is_soldier();
                let own_vel = self.vehicles[i].craft.body.vel;
                // the other part's physics node: the root's moves with the body; a child's (the
                // T34's `T34Complex`) is still (dmg_t34_soldier: its hit gives nothing)
                let other_vel = match h.other {
                    Other::Part(j, y) if self.vehicles[j].contacts.get(y).is_some_and(|c| c.is_root) => self.vehicles[j].craft.body.vel,
                    _ => [0.0; 3],
                };
                // the open parachute is one of its active kit parts, and the least damping of
                // them counts: under 1/h (soldier_parachute: 119 m, no damage), taken as 0
                let m = self.vehicles[i].movement.as_ref();
                let parts = m.and_then(|m| m.kit_parts.iter().map(|p| p.t.damping).reduce(f32::min)).unwrap_or(1.0);
                let damping = if m.is_some_and(|m| m.parachute) { 0.0 } else { parts };
                let a = self.vehicles[i].armor.as_mut().unwrap();
                // the damage sees the height of the collision before this one
                let fall = Fall { last_height: a.last_collision_height, y: h.height, damping };
                if !a.collide_with(h.other, h.height) || a.destroyed {
                    continue;
                }
                let fall = soldier.then_some(&fall);
                let d = match h.other {
                    Other::Terrain => land_damage(a, materials, h.v, h.n, h.materials.1, h.materials.0, fall),
                    // against a static object (no physics node) a soldier's speed is all of v
                    Other::Obstacle(_) => ram_damage(a, rammer_mod, materials, h.v, h.n, h.materials.1, h.materials.0, fall.map(|f| (f, dot(h.v, h.v).sqrt()))),
                    Other::Part(..) => ram_damage(a, rammer_mod, materials, h.v, h.n, h.materials.1, h.materials.0, fall.map(|f| (f, soldier_ram_speed(h.v, own_vel, other_vel)))),
                };
                if let Some(d) = d {
                    // against another object, its player is the attacker (`calcDamage` and the kick-back)
                    // (none when the hit one is of its team and goes faster along v than it: its own fault)
                    let by = match h.other {
                        Other::Part(j, _) => self.rammer(j).filter(|&r| {
                            let along = |u: V3| {
                                let vv = dot(h.v, h.v);
                                if vv == 0.0 { 0.0 } else { dot(u, h.v).abs() / vv.sqrt() }
                            };
                            self.vehicles[r].team != self.vehicles[i].team || along(other_vel) >= along(own_vel)
                        }),
                        _ => None,
                    };
                    give.push((i, d, Source::Crash, by));
                }
            }
        }
        for (i, d, source, by) in give {
            self.damage.attacker = by;
            let d = d * self.friendly_ratio(by, i, false);
            self.give_damage(i, d, source);
            self.damage.attacker = None;
        }
    }

    /// `handleCollisionForProjectile` for a projectile's hit, and its explosion
    /// (`damageType` 1 with `hasCollisionEffect`, 0.1 m out along the normal, the radius rounded).
    pub(crate) fn projectile_hit(&mut self, p: &crate::projectile::Projectile, hit: &crate::projectile::Impact) {
        let pd = p.template.damage;
        self.journal.impacts.push((pd.material, hit.clone()));
        self.journal.hit_log.push((self.clock.steps, p.owner, hit.vehicle, hit.point));
        let materials = self.world.materials();
        let mut skip = None;
        if let Some(vi) = hit.vehicle
            && self.vehicles.get(vi).is_some_and(|v| v.armor.is_some())
        {
            skip = Some(vi);
            if let Some(b) = hit.bone
                && let Some(m) = self.vehicles[vi].movement.as_mut()
            {
                m.hit_bone(b, hit.normal);
            }
            let a = self.vehicles[vi].armor.as_ref().expect("checked above");
            if !a.destroyed
                && !self.role.remote
                && let Some(d) = hit_damage(a, &pd, materials, p.start, hit.point, hit.vel, hit.normal, hit.material)
            {
                self.damage.attacker = p.owner;
                let d = d * self.friendly_ratio(p.owner, vi, false);
                self.give_damage(vi, d, Source::Hit);
                self.damage.attacker = None;
            }
        }
        if pd.damage_type == 1 && p.template.has_collision_effect {
            let n = hit.normal;
            let center = [hit.point[0] + n[0] * 0.1, hit.point[1] + n[1] * 0.1, hit.point[2] + n[2] * 0.1];
            let e = Explosion { center, radius: pd.radius.round_ties_even(), material: pd.material2, damage: materials.damage(pd.material2), force: pd.force, ymod: pd.ymod, by: p.owner };
            self.explode(e, skip);
        }
    }

    /// The projectile's detonate → `startEndEffect`: `damageType` 1 or 4 explode where it is.
    pub(crate) fn projectile_detonated(&mut self, p: &crate::projectile::Projectile) {
        let pd = p.template.damage;
        if pd.damage_type == 1 || pd.damage_type == 4 {
            let e = Explosion { center: p.pos, radius: pd.radius, material: pd.material2, damage: self.world.materials().damage(pd.material2), force: pd.force, ymod: pd.ymod, by: p.owner };
            self.explode(e, None);
        }
    }

    /// The spawner's frame update handling of every spawner: a destroyed or removed object frees
    /// the slot; then the countdown runs (while the last object is gone, destroyed, or farther
    /// than `radius`); at 0 the wreck within `radius` is cleared and a new object made.
    pub(crate) fn update_spawners(&mut self, dt: f32) {
        for k in 0..self.spawners.len() {
            match self.spawners[k].parent {
                Parent::Projectile(p) => match self.projectiles.get(p).filter(|p| p.alive()) {
                    Some(q) => {
                        let (pos, rows) = (q.pos, q.rows);
                        self.spawners[k].follow(pos, rows);
                    }
                    None => self.spawners[k].active = false,
                },
                Parent::Vehicle(p) if self.vehicles.get(p).is_none_or(|v| v.removed) => self.spawners[k].active = false,
                Parent::Vehicle(p) => {
                    let b = &self.vehicles[p].craft.body;
                    let (pos, rows) = (b.pos, b.rows);
                    self.spawners[k].follow(pos, rows);
                }
                Parent::Level | Parent::Static => {}
            }
            self.hold_object(k);
            let s = &self.spawners[k];
            if !s.active {
                continue;
            }
            let (vs, items) = (&self.vehicles, &self.items);
            let gone = |o: Option<Made>| match o {
                Some(Made::Vehicle(i)) => vs.get(i).is_none_or(|v| v.removed),
                Some(Made::Item(i)) => items.get(i).is_none_or(|t| t.removed),
                None => true,
            };
            let destroyed = |o: Made| matches!(o, Made::Vehicle(i) if vs[i].armor.as_ref().is_some_and(|a| a.destroyed));
            let here = s.pos;
            let dist = |o: Made| {
                let at = match o {
                    Made::Vehicle(i) => vs[i].craft.body.pos,
                    Made::Item(i) => items[i].pos,
                };
                let d = sub(here, at);
                dot(d, d).sqrt()
            };
            // the slots (`maxNrOfObjectSpawned`): a full one empties when its object is gone or
            // destroyed; only the first empty one counts down or makes the next object
            let mut first = true;
            let mut free = Vec::new();
            let mut todo = None;
            for j in 0..s.slots.len() {
                if let Some(o) = s.slots[j] {
                    if gone(Some(o)) || destroyed(o) {
                        free.push(j);
                    }
                    continue;
                }
                if !std::mem::take(&mut first) {
                    continue;
                }
                if s.countdown > 0.0 {
                    let runs = match s.last {
                        None => true,
                        Some(o) if gone(Some(o)) => {
                            todo = Some((j, false, true));
                            continue;
                        }
                        Some(o) => destroyed(o) || s.radius < dist(o),
                    };
                    if runs {
                        todo = Some((j, false, false));
                    }
                    continue;
                }
                // (the wreck within `radius` is cleared)
                let clear = s.last.is_some_and(|o| !gone(Some(o)) && destroyed(o) && dist(o) < s.radius);
                todo = Some((j, true, clear));
            }
            for j in free {
                self.spawners[k].slots[j] = None;
            }
            let Some((j, make, flag)) = todo else { continue };
            if !make {
                // (a gone last object is forgotten; the countdown runs)
                if flag {
                    self.spawners[k].last = None;
                }
                self.spawners[k].countdown -= dt;
                continue;
            }
            if flag && let Some(Made::Vehicle(i)) = self.spawners[k].last {
                self.vehicles[i].removed = true;
            }
            let s = &self.spawners[k];
            let (t, at, rows) = (s.template.clone(), s.at, s.rows.unwrap_or_else(|| crate::craft::ypr(s.rotation)));
            if t.is_empty() {
                continue;
            }
            // (a human's soldier near: nothing made, the slot empty and no new delay, so it is
            // asked again next frame)
            if self.human_near(&t, at) {
                continue;
            }
            // (what the sim does not make, or makes elsewhere: asked again after a delay)
            // TODO: a projectile's spawner (DC's `MortarSpawner3`) makes its object the step of the shot here; the original's, 1 or 4
            // frames after it, where the bomb has got to by then (a reference run of the original and the test
            // `the_dc_mortar_is_deployed_as_the_original`). How: the spawner's frame-update cadence.
            let Some(o) = self.instantiate(&t, at, rows) else {
                self.spawners[k].countdown = self.spawners[k].delay;
                continue;
            };
            if let Made::Vehicle(i) = o {
                if self.spawners[k].team_on_vehicle {
                    self.vehicles[i].team = self.spawners[k].team;
                }
                self.vehicles[i].set_engines_on(false);
                self.vehicles[i].entry.abandon = self.spawners[k].abandon();
                self.journal.spawn_log.push((self.clock.steps, k, i));
            }
            let s = &mut self.spawners[k];
            s.slots[j] = Some(o);
            s.last = Some(o);
            s.countdown = s.delay;
            s.holding = s.hold;
        }
    }

    /// `createObjectOnAllClients`, as a spawner calls it: [`World::held_near_humans`]'s
    /// `template` is not made at `at` while a player that is not a bot (`getIsAIPlayer`) is
    /// alive and on foot (its control object a soldier) within 2 m (3D, from its soldier's
    /// position: 4.0 squared). Bots never hold it.
    pub(crate) fn human_near(&self, template: &str, at: V3) -> bool {
        self.world.held_near_humans(template)
            && self.players.iter().filter(|p| !p.ai && p.alive).filter_map(|p| p.soldier.and_then(|s| self.vehicles.get(s))).any(|v| {
                let d = sub(v.craft.body.pos, at);
                v.entry.inside.is_none() && !v.removed && dot(d, d) < HUMAN_NEAR * HUMAN_NEAR
            })
    }

    /// The spawner's `holdObject`: while its object is not destroyed and
    /// its engine is still (|rpm| < 0.1; no engine: let go), it is put at the spawner's place
    /// with its axes, at the speed of the spawner's root there.
    fn hold_object(&mut self, k: usize) {
        let s = &self.spawners[k];
        let Some(Made::Vehicle(i)) = s.last.filter(|_| s.holding) else { return };
        let (at, rows, parent) = (s.at, s.rows, s.parent);
        let v = &self.vehicles[i];
        let engines: Vec<f32> = v.craft.parts.iter().filter_map(|p| if let crate::craft::Kind::Engine(e) = &p.kind { Some(e.rpm) } else { None }).collect();
        // (nor one whose physics a bot's Medium move turned off: in the original with the level of
        // detail on, the landing craft leave their ships so, guessed)
        if v.removed || v.physics_off || v.armor.as_ref().is_some_and(|a| a.destroyed) || engines.is_empty() || engines.iter().any(|r| r.abs() >= HOLD_RPM) {
            self.spawners[k].holding = false;
            return;
        }
        let vel = match parent {
            Parent::Projectile(p) => self.projectiles.get(p).map_or([0.0; 3], |p| p.vel),
            Parent::Vehicle(p) => self.vehicles[p].craft.body.velocity_at(at),
            Parent::Level | Parent::Static => [0.0; 3],
        };
        let b = &mut self.vehicles[i].craft.body;
        b.pos = at;
        if let Some(r) = rows {
            b.rows = r;
        }
        b.vel = vel;
        // (setting the velocity wakes the node and clears what it had accumulated: the first
        // step after does not fall)
        b.acc = [0.0; 3];
    }
}


/// The damage being given: who gives it now, and the explosions' damage left for the next frame
#[derive(Default, Debug)]
pub struct DamageState {
    /// explosions' damage for the next frame (vehicle, amount, attacker)
    pub pending_damage: Vec<(usize, f32, Option<usize>)>,
    /// who gives the damage being given now (`giveDamage`'s attacker), when known
    pub attacker: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armor(hp: f32, crit: f32) -> Armor {
        Armor::new(ArmorTemplate {
            hit_points: hp,
            max_hit_points: hp,
            material: 50,
            damage_mod: 1.0,
            speed_mod: 0.75,
            angle_mod: 0.0,
            critical_damage: crit,
            hp_lost_critical: 1.5,
            hp_lost_upside_down: 7.0,
            explosion_radius: 8.0,
            explosion_damage: 5.0,
            explosion_material: 204,
            explosion_force: 150.0,
            explosion_force_mod: 13.0,
            explosion_force_max: 300.0,
            time_to_live_after_death: 10.0,
            fade_at_time_to_live_after_death: true,
            time_to_start_fade_after_death: 8.0,
            damage_from_water: false,
            water_damage_delay: 0.0,
            hp_lost_water: 1.0,
            damage_from_deep_water: false,
            deep_water_damage_delay: 1.0,
            hp_lost_deep_water: 1.0,
            deep_water_level: 0.0,
            reset_when_removed: false,
            stay_as_destroyed: false,
        })
    }

    #[test]
    fn critical_then_destroyed_then_gone() {
        let mut a = armor(100.0, 12.0);
        assert_eq!(a.give(90.0), Some(Status::Critical));
        assert!(a.critical && !a.destroyed);
        // burning: 1.5 hp per second
        let mut lost = Vec::new();
        for _ in 0..31 {
            lost.extend(a.tick(1.0 / 30.0, 0.0, false, |_| false).0);
        }
        assert_eq!(lost.len(), 1);
        assert!((lost[0] - 1.5).abs() < 1e-5);
        assert_eq!(a.give(20.0), Some(Status::Destroyed));
        assert_eq!(a.hp, 0.0);
        assert_eq!(a.give(5.0), None);
        // the first update starts the clock, then 10 s
        let mut n = 0;
        while !a.wreck_update(1.0 / 30.0) {
            n += 1;
        }
        assert!((300..=302).contains(&n), "{n}");
    }

    /// `sinkInToLandAfterDeathSpeed` puts the wreck's time back to 10, 8 and fading, whatever came
    /// before it (`scrap_metal1_m1`: 3 and 1 in its script); what comes after it stays.
    #[test]
    fn sink_in_to_land_puts_the_wreck_time_back() {
        let template = |props: &[(&str, &str)]| {
            let props = props.iter().map(|(n, v)| bytefield_con::templates::Property { name: n.to_string(), args: vec![v.to_string()] }).collect();
            let t = Template { namespace: "objecttemplate".into(), kind: "Bundle".into(), name: "T".into(), props, origin: String::new(), seq: 0, create_args: Vec::new() };
            ArmorTemplate::from_template(&t).unwrap()
        };
        let scrap = template(&[("hasarmor", "1"), ("timetoliveafterdeath", "3"), ("fadeattimetoliveafterdeath", "0"), ("timetostartfadeafterdeath", "1"), ("sinkintolandafterdeathspeed", "10")]);
        assert_eq!((scrap.time_to_live_after_death, scrap.time_to_start_fade_after_death, scrap.fade_at_time_to_live_after_death), (10.0, 8.0, true));
        let after = template(&[("hasarmor", "1"), ("sinkintolandafterdeathspeed", "1.5"), ("timetoliveafterdeath", "3"), ("fadeattimetoliveafterdeath", "0")]);
        assert_eq!((after.time_to_live_after_death, after.time_to_start_fade_after_death, after.fade_at_time_to_live_after_death), (3.0, 8.0, false));
    }

    /// The object's update handling: the wreck fades out over its last 2 s (10 − 8), from 255 down.
    #[test]
    fn a_wreck_fades_out_over_its_last_two_seconds() {
        let mut a = armor(100.0, 0.0);
        assert_eq!(a.give(200.0), Some(Status::Destroyed));
        let mut seen = Vec::new();
        let mut t = 0;
        while !a.wreck_update(0.1) {
            t += 1;
            seen.push((t, a.wreck_alpha()));
        }
        // the first update starts the clock; the timer is under 2 s from the 81st on (1.99999)
        assert!(seen.iter().take_while(|(k, _)| *k < 81).all(|(_, x)| x.is_none()), "{seen:?}");
        assert_eq!(seen.iter().find(|(k, _)| *k == 82).unwrap().1, Some(242));
        assert_eq!(seen.last(), Some(&(100, Some(13))));
        let mut b = armor(100.0, 0.0);
        b.template.fade_at_time_to_live_after_death = false;
        b.give(200.0);
        while !b.wreck_update(0.1) {
            assert_eq!(b.wreck_alpha(), None);
        }
    }

    #[test]
    fn a_collision_counts_once_a_second() {
        let mut a = armor(100.0, 12.0);
        assert!(a.collide_with(Other::Terrain, 0.0));
        assert!(!a.collide_with(Other::Terrain, 0.0));
        assert!(a.collide_with(Other::Part(1, 0), 0.0));
        for _ in 0..29 {
            a.tick(1.0 / 30.0, 0.0, false, |_| false);
        }
        assert!(!a.collide_with(Other::Terrain, 0.0));
        a.tick(2.0 / 30.0, 0.0, false, |_| false);
        assert!(a.collide_with(Other::Terrain, 0.0));
    }

    #[test]
    fn the_water_hurts_after_its_delay() {
        let mut a = armor(30.0, 0.0);
        a.template.damage_from_water = true;
        a.template.water_damage_delay = 2.0;
        a.water_timer = 2.0;
        let dt = 1.0 / 30.0;
        // out of the water nothing; under it, the first damage after 2 s, then one a second
        assert_eq!(a.tick_in(dt, 10.0, 5.0, true, |_| false).1, (None, None));
        let mut hits = Vec::new();
        for n in 0..150 {
            if let Some(d) = a.tick_in(dt, 4.0, 5.0, true, |_| false).1.0 {
                hits.push((n, d));
            }
        }
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![60, 90, 120]);
        assert!(hits.iter().all(|h| h.1 == 1.0));
        // out again: the delay starts over
        a.tick_in(dt, 10.0, 5.0, true, |_| false);
        assert_eq!(a.water_timer, 2.0);
    }

    #[test]
    fn a_square_hit_counts_whole() {
        assert!((angle_factor(0.0, [0.0, 0.0, -1.0], [0.0, 0.0, 1.0]) - 1.0).abs() < 1e-6);
        assert!(angle_factor(0.0, [1.0, 0.0, -0.01], [0.0, 0.0, 1.0]) < 0.02);
        assert!((angle_factor(0.25, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]) - 0.25).abs() < 1e-6);
    }
}

#[cfg(test)]
mod medpack {
    use crate::soldier::axis;

    /// A medic with the pack out, fire held, beside a hurt soldier (`useMedPack`): 0.25 of health
    /// a frame back (`healFactor`; a soldier has 30); with nobody hurt near and himself whole, the
    /// pack does not fire (its rounds stay); hurt himself, he heals himself (0.15,
    /// `selfHealFactor`), the other 50 m away (`healDistance` is 10).
    #[test]
    fn a_medic_heals() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let medic_kit = s.modes.conquest.as_ref().unwrap().teams.kits[0].iter().position(|k| k.as_deref().is_some_and(|k| k.to_ascii_lowercase().contains("medic"))).unwrap();
        let medic = s.add_player(1, "GermanSoldier", false);
        s.players[medic].kit = medic_kit;
        let hurt = s.add_player(1, "GermanSoldier", false);
        for _ in 0..30 * 30 {
            s.step();
            if s.players.iter().all(|p| p.alive) {
                break;
            }
        }
        let (m, h) = (s.players[medic].soldier.unwrap(), s.players[hurt].soldier.unwrap());
        let p = s.vehicles[m].craft.body.pos;
        s.vehicles[h].craft.body.pos = [p[0] + 1.5, p[1], p[2]];
        // the pack (slot 5) out, then fire held
        s.vehicles[m].movement.as_mut().unwrap().set_input(&[(axis::WEAPON1 + 4, 1.0)]);
        for _ in 0..30 {
            s.step();
        }
        let w = s.vehicles[m].movement.as_ref().unwrap().weapon.as_ref().unwrap();
        assert!(w.template.name.eq_ignore_ascii_case("MedPack"), "{}", w.template.name);
        let rounds = w.total_ammo();
        s.vehicles[m].movement.as_mut().unwrap().set_input(&[(axis::FIRE, 1.0)]);
        for _ in 0..10 {
            s.step();
        }
        let w = s.vehicles[m].movement.as_ref().unwrap().weapon.as_ref().unwrap();
        assert_eq!(w.total_ammo(), rounds, "nobody hurt: the pack does not fire");
        s.give_damage(h, 10.0, super::Source::Hit);
        let before = s.vehicles[h].armor.as_ref().unwrap().hp;
        for _ in 0..30 {
            s.step();
        }
        let after = s.vehicles[h].armor.as_ref().unwrap().hp;
        eprintln!("hp {before} -> {after}");
        assert!((after - before - 7.5).abs() < 0.2, "hp {before} -> {after}");
        assert!(s.vehicles[m].movement.as_ref().unwrap().weapon.as_ref().unwrap().total_ammo() < rounds);
        // the other away, the medic hurt: himself
        s.vehicles[h].craft.body.pos = [p[0] + 50.0, p[1], p[2]];
        s.give_damage(m, 10.0, super::Source::Hit);
        let before = s.vehicles[m].armor.as_ref().unwrap().hp;
        for _ in 0..30 {
            s.step();
        }
        let after = s.vehicles[m].armor.as_ref().unwrap().hp;
        assert!((after - before - 4.5).abs() < 0.2, "himself: hp {before} -> {after}");
    }
}

#[cfg(test)]
mod repairpack {
    use crate::soldier::axis;

    /// An engineer of team 1 on Berlin, alive, with the wrench (slot 6) out.
    fn engineer(s: &mut crate::Sim) -> usize {
        s.round.playing = true;
        s.start_conquest();
        let kit = s.modes.conquest.as_ref().unwrap().teams.kits[0].iter().position(|k| k.as_deref().is_some_and(|k| k.to_ascii_lowercase().contains("engineer"))).unwrap();
        let e = s.add_player(1, "GermanSoldier", false);
        s.players[e].kit = kit;
        for _ in 0..30 * 30 {
            s.step();
            if s.players[e].alive {
                break;
            }
        }
        let e = s.players[e].soldier.unwrap();
        s.vehicles[e].movement.as_mut().unwrap().set_input(&[(axis::WEAPON1 + 5, 1.0)]);
        for _ in 0..30 {
            s.step();
        }
        let w = s.vehicles[e].movement.as_ref().unwrap().weapon.as_ref().unwrap();
        assert!(w.template.name.eq_ignore_ascii_case("RepairPack"), "{}", w.template.name);
        e
    }

    fn game() -> Option<std::path::PathBuf> {
        let game = bytefield_testdata::game();
        game.is_dir().then_some(game)
    }

    /// `useRepairPack` standing: a hurt tank beside the engineer gets `repairFactor` (0.15) hit
    /// points a frame, and the HUD's status is how whole it is above its critical damage; whole,
    /// or of the other team, it gets nothing and the wrench keeps its rounds. (The reference run
    /// `repair_tank`: a T34 at 60 of 100 gets 0.15 in every frame of fire, the armor's heal from
    /// `useRepairPack`, until it is whole 266 frames later.)
    #[test]
    fn an_engineer_repairs_a_tank_of_his_team() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let e = engineer(&mut s);
        let p = s.vehicles[e].craft.body.pos;
        let tank = s.spawn_vehicle("PanzerIV", [p[0] + 2.0, p[1] + 0.5, p[2]], [0.0; 3]).unwrap();
        let r = s.vehicles[tank].radius;
        s.vehicles[tank].craft.body.pos = [p[0] + r + 1.0, p[1] + 0.5, p[2]];
        s.vehicles[tank].fixed = true;
        let rounds = s.vehicles[e].movement.as_ref().unwrap().weapon.as_ref().unwrap().total_ammo();
        s.vehicles[e].movement.as_mut().unwrap().set_input(&[(axis::FIRE, 1.0)]);
        for _ in 0..10 {
            s.step();
        }
        assert_eq!(s.vehicles[e].movement.as_ref().unwrap().weapon.as_ref().unwrap().total_ammo(), rounds, "whole: the wrench does not fire");
        let max = s.vehicles[tank].armor.as_ref().unwrap().max_hp;
        s.give_damage(tank, max * 0.3, super::Source::Hit);
        let before = s.vehicles[tank].armor.as_ref().unwrap().hp;
        for _ in 0..30 {
            s.step();
        }
        let a = s.vehicles[tank].armor.as_ref().unwrap();
        assert!((a.hp - before - 4.5).abs() < 0.05, "hp {before} -> {}", a.hp);
        let status = s.vehicles[e].movement.as_ref().unwrap().repair_status;
        let c = a.template.critical_damage;
        assert!((status - (a.hp - c) / (a.max_hp - c)).abs() < 1e-5, "{status}");
        assert!(s.vehicles[e].movement.as_ref().unwrap().weapon.as_ref().unwrap().total_ammo() < rounds);
        assert_eq!(s.journal.repairs, 30);
        // the other team's: nothing
        s.vehicles[tank].team = 2;
        let before = s.vehicles[tank].armor.as_ref().unwrap().hp;
        for _ in 0..10 {
            s.step();
        }
        assert_eq!(s.vehicles[tank].armor.as_ref().unwrap().hp, before);
    }

    /// A PT boat destroyed afloat plays its −1 effect (`e_PTBoatWreck`), but its emitter
    /// `Em_PTBoatSpawnRaft` (`IsSpawnEffect`) hangs from the boat and creates nothing: no
    /// `Elco80Raft` (original,b}.jsonl`: the boat goes the frame after the
    /// kill, `timetoliveafterdeath 0`, and nothing is created in 270 s). A tank destroyed on land
    /// plays its 0 effect.
    #[test]
    fn a_sunk_pt_boat_leaves_no_raft() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "Midway", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let sea = crate::contact::Ground::water_level(&w.level);
        let boat = s.spawn_vehicle("Elco80", [1000.0, sea, 1000.0], [0.0; 3]).unwrap();
        // settled on its floaters
        for _ in 0..30 {
            s.step();
        }
        assert!(s.vehicles[boat].craft.afloat);
        let n = s.vehicles.len();
        s.give_damage(boat, 10_000.0, super::Source::Hit);
        for _ in 0..60 {
            s.step();
        }
        assert_eq!(s.vehicles.len(), n, "{:?}", s.vehicles[n..].iter().map(|v| &v.template).collect::<Vec<_>>());
        assert_eq!(s.vehicles[boat].armor.as_ref().unwrap().wreck_effect, Some(-1));
        assert!(s.vehicles[boat].removed);
        let n = s.vehicles.len();
        let tank = s.spawn_vehicle("Sherman", [100.0, 0.0, 100.0], [0.0; 3]);
        if let Some(tank) = tank {
            s.vehicles[tank].craft.body.pos[1] = crate::contact::Ground::height_and_normal(&w.level, 100.0, 100.0).0 + 1.0;
            if s.vehicles[tank].craft.body.pos[1] > sea + 2.0 {
                s.give_damage(tank, 10_000.0, super::Source::Hit);
                s.step();
                assert_eq!(s.vehicles[tank].armor.as_ref().unwrap().wreck_effect, Some(0));
                assert_eq!(s.vehicles.len(), n + 1);
            }
        }
    }

    /// Crouched, the wrench takes the mine at his feet (anyone's) instead: it is gone and his
    /// mines get it back. (The reference run `repair_mine`: in the first frame of fire, `reloadAmmo(1)`
    /// of the mines and `resetProjectile` of the one on the ground; then nothing.)
    #[test]
    fn a_crouched_engineer_picks_up_a_mine() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let e = engineer(&mut s);
        let mines = |s: &crate::Sim| s.vehicles[e].movement.as_ref().unwrap().stowed.iter().find(|w| w.template.name.eq_ignore_ascii_case("Landmine")).unwrap().total_ammo();
        let full = mines(&s);
        s.vehicles[e].movement.as_mut().unwrap().stowed.iter_mut().find(|w| w.template.name.eq_ignore_ascii_case("Landmine")).unwrap().mags[0] -= 1;
        s.vehicles[e].movement.as_mut().unwrap().set_input(&[(axis::CROUCH, 1.0)]);
        for _ in 0..30 {
            s.step();
        }
        assert_eq!(s.vehicles[e].movement.as_ref().unwrap().pose(), 1);
        let p = s.vehicles[e].craft.body.pos;
        let shot = crate::projectile::Shot { pos: [p[0] + 1.0, p[1], p[2]], rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], vel: [0.0; 3], rotvel: [0.0; 3] };
        let mine = s.add_projectile("LandmineProjectile", &shot).unwrap();
        s.vehicles[e].movement.as_mut().unwrap().set_input(&[(axis::CROUCH, 1.0), (axis::FIRE, 1.0)]);
        s.step();
        s.step();
        assert_eq!(s.projectiles[mine].end, Some(crate::projectile::End::Removed));
        assert_eq!(mines(&s), full);
        assert_eq!(s.journal.disarmed, 1);
    }

    /// `changeWeaponWhenNoAmmo` and `cantSelectWhenNoAmmo` (the mines'): with the mines in his
    /// hand and none left, the engineer takes another weapon up by himself
    /// (`selectBestLoadedWeapon` from `handlePlayerInput`), and the key of the mines does not
    /// bring them back (the hand weapon's enable fails).
    #[test]
    fn an_engineer_out_of_mines_puts_them_away() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let e = engineer(&mut s);
        let held = |s: &crate::Sim| s.vehicles[e].movement.as_ref().unwrap().weapon.as_ref().unwrap().template.name.clone();
        s.vehicles[e].movement.as_mut().unwrap().set_input(&[(axis::WEAPON1 + 4, 1.0)]);
        for _ in 0..30 {
            s.step();
        }
        assert!(held(&s).eq_ignore_ascii_case("Landmine"), "{}", held(&s));
        s.vehicles[e].movement.as_mut().unwrap().weapon.as_mut().unwrap().mags.iter_mut().for_each(|m| *m = 0);
        s.vehicles[e].movement.as_mut().unwrap().set_input(&[]);
        for _ in 0..30 {
            s.step();
        }
        assert!(!held(&s).eq_ignore_ascii_case("Landmine"), "still the mines");
        s.vehicles[e].movement.as_mut().unwrap().set_input(&[(axis::WEAPON1 + 4, 1.0)]);
        for _ in 0..30 {
            s.step();
        }
        assert!(!held(&s).eq_ignore_ascii_case("Landmine"), "the mines came back with none");
    }
}

#[cfg(test)]
mod friendly_fire {
    use super::{Explosion, ServerRules};

    /// `calcDamage`: a teammate's soldier takes the soldiers' share, a teammate's vehicle the vehicles', an
    /// enemy all; in an explosion the splash shares, and a soldier's push goes with its damage. Team 0
    /// (an empty vehicle) is nobody's teammate.
    #[test]
    fn a_teammate_takes_the_rules_share() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let ps: Vec<usize> = [1, 1, 2].iter().map(|&t| s.add_player(t, if t == 1 { "GermanSoldier" } else { "USSoldier" }, false)).collect();
        for _ in 0..30 * 30 {
            s.step();
            if s.players.iter().all(|p| p.alive) {
                break;
            }
        }
        let [a, f, e] = [0, 1, 2].map(|k| s.players[ps[k]].soldier.unwrap());
        let tank = s.vehicles.iter().position(|v| v.movement.is_none() && v.armor.is_some() && !v.removed).unwrap();
        s.rules.server = ServerRules { soldier_ff: 0.5, vehicle_ff: 0.25, soldier_ff_splash: 0.0, vehicle_ff_splash: 0.75, ..Default::default() };
        assert_eq!(s.friendly_ratio(Some(a), f, false), 0.5);
        assert_eq!(s.friendly_ratio(Some(a), e, false), 1.0);
        assert_eq!(s.friendly_ratio(Some(a), a, true), 0.0, "one's own grenade, as a teammate's");
        assert_eq!(s.friendly_ratio(None, f, false), 1.0);
        s.vehicles[tank].team = 0;
        assert_eq!(s.friendly_ratio(Some(a), tank, false), 1.0);
        s.vehicles[tank].team = 1;
        assert_eq!(s.friendly_ratio(Some(a), tank, false), 0.25);
        assert_eq!(s.friendly_ratio(Some(a), tank, true), 0.75);
        assert_eq!(s.friendly_ratio(Some(tank), f, false), 0.5, "from a vehicle of the team");

        // a grenade's blast beside the teammate and beside the enemy, far from each other
        s.vehicles[a].craft.body.pos[1] += 500.0;
        let at = |s: &crate::Sim, v: usize| s.vehicles[v].craft.body.pos;
        let pf = at(&s, f);
        s.vehicles[e].craft.body.pos = [pf[0] + 100.0, pf[1], pf[2]];
        let pe = at(&s, e);
        let hp = |s: &crate::Sim, v: usize| s.vehicles[v].armor.as_ref().unwrap().hp;
        let (hf, he) = (hp(&s, f), hp(&s, e));
        let pushes = s.journal.pushes.len();
        for c in [pf, pe] {
            s.explode(Explosion { center: [c[0] + 1.0, c[1] + 0.5, c[2]], radius: 5.0, material: 0, damage: 20.0, force: 50.0, ymod: 1.0, by: Some(a) }, None);
        }
        s.step();
        assert_eq!(hp(&s, f), hf, "the splash share is 0");
        assert!(hp(&s, e) < he, "the enemy: {he} -> {}", hp(&s, e));
        let pushed = |v: usize| s.journal.pushes[pushes..].iter().find(|p| p.1 == v).map(|p| p.3.iter().map(|x| x.abs()).sum::<f32>());
        assert!(pushed(f).is_none_or(|x| x == 0.0) && pushed(e).is_some_and(|x| x > 0.0), "{:?} {:?}", pushed(f), pushed(e));
    }

    /// `_giveDamage`'s kick-back: the attacker's soldier takes back its share of what reaches a teammate (another
    /// player of its team), the splash share in an explosion, nothing from an enemy or by default; and
    /// `handleCollisionObjectVsObject`'s attacker: a player's soldier, or the one who left the vehicle under 2 s
    /// before.
    #[test]
    fn the_attacker_takes_back_the_kickback() {
        use super::Source;
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let ps: Vec<usize> = [1, 1, 2].iter().map(|&t| s.add_player(t, if t == 1 { "GermanSoldier" } else { "USSoldier" }, false)).collect();
        for _ in 0..30 * 30 {
            s.step();
            if s.players.iter().all(|p| p.alive) {
                break;
            }
        }
        let [a, f, e] = [0, 1, 2].map(|k| s.players[ps[k]].soldier.unwrap());
        let hp = |s: &crate::Sim, v: usize| s.vehicles[v].armor.as_ref().unwrap().hp;
        let hit = |s: &mut crate::Sim, v: usize, d: f32, source: Source| {
            s.damage.attacker = Some(a);
            s.give_damage(v, d, source);
            s.damage.attacker = None;
        };
        let h = hp(&s, a);
        hit(&mut s, f, 10.0, Source::Hit);
        assert_eq!(hp(&s, a), h, "by default, none");
        s.rules.server = ServerRules::default().with_kickback(50.0);
        s.rules.server.kickback_splash = 0.25;
        hit(&mut s, f, 10.0, Source::Hit);
        assert!((h - hp(&s, a) - 5.0).abs() < 1e-4, "{h} -> {}", hp(&s, a));
        let h = hp(&s, a);
        hit(&mut s, f, 10.0, Source::Explosion);
        assert!((h - hp(&s, a) - 2.5).abs() < 1e-4, "the splash share: {h} -> {}", hp(&s, a));
        let h = hp(&s, a);
        hit(&mut s, e, 10.0, Source::Hit);
        hit(&mut s, a, 10.0, Source::Hit);
        assert_eq!(hp(&s, a), h - 10.0, "an enemy, or oneself: no kick-back");
        let h = hp(&s, a);
        hit(&mut s, f, 0.01, Source::Hit);
        assert_eq!(hp(&s, a), h, "at or under 0.01, none");
        let tank = s.vehicles.iter().position(|v| v.movement.is_none() && v.armor.is_some() && !v.removed).unwrap();
        s.vehicles[tank].team = 1;
        hit(&mut s, tank, 10.0, Source::Hit);
        assert_eq!(hp(&s, a), h, "an empty vehicle of the team is nobody's");

        assert_eq!(s.rammer(f), Some(f));
        assert_eq!(s.rammer(tank), None);
        s.vehicles[f].entry.exited = Some((tank, s.clock.time - 1.0));
        assert_eq!(s.rammer(tank), Some(f), "out under 2 s before");
        s.vehicles[f].entry.exited = Some((tank, s.clock.time - 2.5));
        assert_eq!(s.rammer(tank), None);
    }
}
