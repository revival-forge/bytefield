//! The engineer's explosives as the original game keeps them: the proximity
//! fuse, the weapon's pool of projectiles
//! (The weapon's projectile pool init, `createProjectile`), the detonator
//! (The soldier's message handling → the weapon's detonate projectiles) and what is left of them when
//! the soldier dies (the kit it drops takes them with it 30 s later: `~FireArms` →
//! `releaseProjectilePool`).

use serde::Serialize;

use crate::mover::Vehicle;
use crate::projectile::{End, Projectile};

/// The proximity fuse: what moves within `explodeNearEnemyDistance` faster than this (squared,
/// m²/s²: 2.5 m/s) sets it off (on land)...
pub const FUSE_MIN_SPEED2: f32 = 6.25;
/// …if heavier than the projectile and no heavier than this (kg)
pub const FUSE_MAX_MASS: f32 = 100_000.0;
/// Under water, what is at least this heavy within the distance in height, moving or not.
pub const FUSE_WATER_MIN_MASS: f32 = 50_000.0;
/// The kit a dead soldier drops is destroyed this long after (s): its weapons' pools with it
/// (the reference run `explosives_death`: death at 7.4 s, the kit's destroy at 37.4 s).
pub const KIT_REMOVE_DELAY: f64 = 30.0;

/// The weapons whose pool is `magSize` × 2 + 1 (by name, in `initProjectilePool`).
pub fn pooled(weapon: &str) -> bool {
    weapon.eq_ignore_ascii_case("ExpPack") || weapon.eq_ignore_ascii_case("Landmine")
}

/// The projectile's proximity fuse: armed `ProximityFusePrimer` s after it was fired;
/// then the first object (no soldier) within the distance that, with the projectile on land, is
/// heavier than it (up to 100 t) and moves at 2.5 m/s or more, or, under water, weighs 50 t or
/// more and is within the distance in height. `under_water`: the projectile's node is.
pub fn fuse(p: &Projectile, vehicles: &[Vehicle], under_water: bool) -> bool {
    let t = &p.template;
    let d = t.explode_near_enemy;
    if !(d > 0.0) || !(p.age as f32 > t.proximity_fuse_primer) {
        return false;
    }
    let mass = p.rigid.as_ref().map_or(t.mass, |v| v.craft.body.mass);
    vehicles.iter().filter(|v| v.in_world() && v.movement.is_none()).any(|v| {
        let b = &v.craft.body;
        let r = [b.pos[0] - p.pos[0], b.pos[1] - p.pos[1], b.pos[2] - p.pos[2]];
        if r[0] * r[0] + r[1] * r[1] + r[2] * r[2] > d * d {
            return false;
        }
        if under_water {
            b.mass >= FUSE_WATER_MIN_MASS && r[1].abs() <= d
        } else {
            let v2 = b.vel[0] * b.vel[0] + b.vel[1] * b.vel[1] + b.vel[2] * b.vel[2];
            mass < b.mass && b.mass <= FUSE_MAX_MASS && v2 >= FUSE_MIN_SPEED2
        }
    })
}

/// A weapon's pool of projectiles: a ring that the next shot takes in turn, whatever became of
/// the one in it (the tenth mine takes the first one's place, even if that one went off).
#[derive(Debug, Clone, Serialize)]
pub struct Pool {
    /// the soldier (its vehicle) and the weapon
    pub soldier: usize,
    pub weapon: String,
    /// the projectiles in it (by index in `Sim::projectiles`) and the next slot
    pub slots: Vec<usize>,
    pub size: usize,
    pub next: usize,
    /// when its soldier was seen dead (its kit dropped)
    pub dead_since: Option<f64>,
}

impl crate::Sim<'_> {
    /// Projectile `k`, just fired by soldier `i` with `weapon` (`magSize` `mag`), takes its slot
    /// in the weapon's pool; the one it replaces is gone (`resetProjectile`).
    pub(crate) fn pool_shot(&mut self, i: usize, weapon: &str, mag: i32, k: usize) {
        if !pooled(weapon) {
            return;
        }
        let at = match self.pools.iter().position(|p| p.soldier == i && p.weapon == weapon) {
            Some(at) => at,
            None => {
                let size = (mag.max(0) * 2 + 1) as usize;
                self.pools.push(Pool { soldier: i, weapon: weapon.to_string(), slots: Vec::new(), size, next: 0, dead_since: None });
                self.pools.len() - 1
            }
        };
        let pool = &mut self.pools[at];
        if pool.slots.len() < pool.size {
            pool.slots.push(k);
        } else {
            let old = std::mem::replace(&mut pool.slots[pool.next], k);
            if let Some(p) = self.projectiles.get_mut(old).filter(|p| p.alive()) {
                p.end = Some(End::Removed);
            }
        }
        pool.next = (pool.next + 1) % pool.size;
    }

    /// The weapon's detonate projectiles of soldier `i`'s charges (`ExpPack`): every one in the
    /// pool that is still there goes off.
    pub(crate) fn detonate_charges(&mut self, i: usize) {
        let Some(pool) = self.pools.iter().find(|p| p.soldier == i && p.weapon.eq_ignore_ascii_case("ExpPack")) else { return };
        for k in pool.slots.clone() {
            if self.projectiles.get(k).is_some_and(|p| p.alive()) {
                self.projectiles[k].end = Some(End::Detonated);
                let p = self.projectiles[k].clone();
                self.projectile_detonated(&p);
            }
        }
    }

    /// The soldiers that fired with the detonator this frame set off their charges.
    pub(crate) fn use_detonators(&mut self) {
        let firing: Vec<usize> = self.vehicles.iter().enumerate().filter(|(_, v)| v.movement.as_ref().is_some_and(|m| m.detonate)).map(|(i, _)| i).collect();
        for i in firing {
            self.detonate_charges(i);
        }
    }

    /// The pools of the dead: their soldier's kit is destroyed 30 s after the death, and its
    /// weapons' projectiles with it (they do not go off).
    pub(crate) fn update_pools(&mut self) {
        let now = self.clock.time;
        for pool in self.pools.iter_mut() {
            let dead = self.vehicles.get(pool.soldier).is_none_or(|v| v.removed || v.armor.as_ref().is_some_and(|a| a.destroyed));
            if dead && pool.dead_since.is_none() {
                pool.dead_since = Some(now);
            }
        }
        let (gone, kept): (Vec<Pool>, Vec<Pool>) = std::mem::take(&mut self.pools).into_iter().partition(|p| p.dead_since.is_some_and(|t| now - t >= KIT_REMOVE_DELAY));
        self.pools = kept;
        for k in gone.iter().flat_map(|p| p.slots.iter().copied()) {
            if let Some(p) = self.projectiles.get_mut(k).filter(|p| p.alive()) {
                p.end = Some(End::Removed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::soldier::axis;
    use crate::projectile::{End, Shot};

    fn game() -> Option<std::path::PathBuf> {
        let game = bytefield_testdata::game();
        game.is_dir().then_some(game)
    }

    fn mine_at(s: &mut crate::Sim, pos: [f32; 3]) -> usize {
        let shot = Shot { pos, rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], vel: [0.0; 3], rotvel: [0.0; 3] };
        s.add_projectile("LandmineProjectile", &shot).unwrap()
    }

    /// The reference runs `mine_fuse_fast` and `mine_fuse_slow`: a T34 (25 t) driving at a mine 15 m
    /// ahead sets it off at 10 m/s when it is 3 m away (2.86 m in the reference run), and at 2.02 m/s it
    /// rolls over it.
    #[test]
    fn el_alamein_a_mine_goes_off_under_a_fast_tank_not_a_slow_one() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        for (speed, goes_off) in [(10.0f32, true), (2.02, false)] {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            let y = w.height_at(1500.0, 715.0).unwrap() as f32;
            let tank = s.spawn_vehicle("T34", [1500.0, y + 1.0, 700.0], [0.0; 3]).unwrap();
            let mine = mine_at(&mut s, [1500.0, y + 0.3, 715.0]);
            let mut at = None;
            for n in 0..30 * 6 {
                let z = 700.0 + speed * n as f32 / 30.0;
                let b = &mut s.vehicles[tank].craft.body;
                b.pos = [1500.0, y + 1.0, z];
                b.vel = [0.0, 0.0, speed];
                s.step();
                if at.is_none() && s.projectiles[mine].end == Some(End::Detonated) {
                    let p = s.projectiles[mine].pos;
                    let b = s.vehicles[tank].craft.body.pos;
                    at = Some(((b[0] - p[0]).powi(2) + (b[1] - p[1]).powi(2) + (b[2] - p[2]).powi(2)).sqrt());
                }
            }
            assert_eq!(at.is_some(), goes_off, "{speed} m/s: {at:?}");
            if let Some(d) = at {
                assert!(d <= 3.0 && d > 3.0 - speed / 30.0 - 0.05, "{d}");
            }
        }
    }

    /// An engineer's mines: a pool of 4 × 2 + 1; the tenth takes the first one's place.
    #[test]
    fn el_alamein_the_tenth_mine_removes_the_first() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let y = w.height_at(1500.0, 700.0).unwrap() as f32;
        let mines: Vec<usize> = (0..10).map(|n| mine_at(&mut s, [1500.0 + 2.0 * n as f32, y + 0.3, 700.0])).collect();
        for &k in &mines {
            s.pool_shot(0, "Landmine", 4, k);
        }
        assert_eq!(s.pools[0].size, 9);
        assert_eq!(s.projectiles[mines[0]].end, Some(End::Removed));
        assert!(mines[1..].iter().all(|&k| s.projectiles[k].alive()));
        // an eleventh: the second goes, even if it already went off
        s.projectiles[mines[1]].end = Some(End::Detonated);
        let k = mine_at(&mut s, [1480.0, y + 0.3, 700.0]);
        s.pool_shot(0, "Landmine", 4, k);
        assert_eq!(s.projectiles[mines[1]].end, Some(End::Detonated));
        assert!(mines[2..].iter().all(|&k| s.projectiles[k].alive()));
    }

    /// An engineer places his 4 charges; with the last the detonator comes up, and firing it
    /// sets them off. A mine and a charge of a dead one are there for 30 s more, then gone.
    #[test]
    fn el_alamein_charges_the_detonator_and_the_dead_engineers_kit() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        let y = w.height_at(1500.0, 700.0).unwrap() as f32;
        let sol = s.spawn_vehicle("GermanDesertSoldier", [1500.0, y + 1.0, 700.0], [0.0; 3]).unwrap();
        let ws = w.kit_weapons("German_Engineer_Desert");
        assert!(s.vehicles[sol].give_kit(ws, Some("ExpPack")));
        let press = |s: &mut crate::Sim, a: usize, frames: usize| {
            for f in 0..frames {
                let v = if f < 2 { 1.0 } else { 0.0 };
                s.vehicles[sol].movement.as_mut().unwrap().set_input(&[(a, v)]);
                s.step();
            }
        };
        for _ in 0..4 {
            press(&mut s, axis::FIRE, 45);
        }
        let charges: Vec<usize> = s.pools.iter().filter(|p| p.weapon == "ExpPack").flat_map(|p| p.slots.clone()).collect();
        assert_eq!(charges.len(), 4, "{:?}", s.pools);
        assert!(charges.iter().all(|&k| s.projectiles[k].alive()));
        let held = s.vehicles[sol].movement.as_ref().unwrap().weapon.as_ref().map(|w| w.template.name.clone());
        assert_eq!(held.as_deref(), Some("Detonator"));
        let booms = s.journal.explosions.len();
        press(&mut s, axis::FIRE, 10);
        assert!(charges.iter().all(|&k| s.projectiles[k].end == Some(End::Detonated)));
        assert_eq!(s.journal.explosions.len() - booms, 4);

        // a mine, then death: it stays 30 s
        let mine = mine_at(&mut s, [1510.0, y + 0.3, 700.0]);
        s.pool_shot(sol, "Landmine", 4, mine);
        s.give_damage(sol, crate::entry::KILL_DAMAGE, crate::damage::Source::Kill);
        for _ in 0..29 * 30 {
            s.step();
        }
        assert!(s.projectiles[mine].alive());
        for _ in 0..2 * 30 {
            s.step();
        }
        assert_eq!(s.projectiles[mine].end, Some(End::Removed));
        assert!(s.pools.is_empty());
    }

    /// The server's suicide and a death by one's own explosive: a TK's points (−2 in conquest,
    /// death 0), not counted as a team kill.
    #[test]
    fn el_alamein_suicide_costs_a_tks_points() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        for own_charge in [false, true] {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            let a = s.add_player(1, "GermanSoldier", false);
            for _ in 0..30 * 30 {
                s.step();
                if s.players[a].alive {
                    break;
                }
            }
            let sol = s.players[a].soldier.unwrap();
            if own_charge {
                s.damage.attacker = Some(sol);
                s.give_damage(sol, 1000.0, crate::damage::Source::Explosion);
                s.damage.attacker = None;
            } else {
                s.suicide(a);
            }
            s.step();
            let p = &s.players[a];
            assert_eq!((p.score, p.tks, p.team_kills_total, p.deaths, p.alive), (-2, 1, 0, 1, false), "own charge: {own_charge}");
        }
    }

    /// A spent projectile's place goes to a new one once it has waited `SPENT_REUSE_AFTER`, not before, and not
    /// while an explosives' pool names it.
    #[test]
    fn el_alamein_a_spent_projectiles_place_is_taken_later() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let y = w.height_at(1500.0, 715.0).unwrap() as f32;
        let at = [1500.0, y + 0.3, 715.0];
        let a = mine_at(&mut s, at);
        s.projectiles[a].end = Some(End::Removed);
        s.step();
        assert_ne!(mine_at(&mut s, at), a, "too soon");
        s.clock.time += crate::projectile::SPENT_REUSE_AFTER;
        let b = mine_at(&mut s, at);
        assert_eq!(b, a);
        assert!(s.projectiles[b].alive() && s.projectiles[b].gone_since.is_none());
        // named by a pool: it waits
        s.projectiles[b].end = Some(End::Removed);
        s.step();
        s.pools.push(crate::explosives::Pool { soldier: 0, weapon: String::new(), slots: vec![b], size: 1, next: 0, dead_since: None });
        s.clock.time += crate::projectile::SPENT_REUSE_AFTER;
        assert_ne!(mine_at(&mut s, at), b);
        s.pools.clear();
        s.clock.time += crate::projectile::SPENT_REUSE_AFTER;
        assert_eq!(mine_at(&mut s, at), b);
    }
}
