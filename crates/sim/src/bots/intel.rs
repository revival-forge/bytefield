//! What a side knows of the enemy: two layers of influence fed only by what its bots see and what it hears (never the real
//! positions), in the manner of the influence maps of multiplayer bots.
//!
//! 1. **Presence per flag**: for each flag, the estimated enemy strength around it, in enemies. Each sighting adds its weight (on
//!    foot 1, armour [`ARMOUR_W`], another vehicle 1, an aircraft nothing: the air goes apart) times a linear fall-off with the
//!    distance (1 at the flag, 0 at [`PRESENCE_M`]) times [`RATE`], and the whole decays with a half-life of [`PRESENCE_HALF_S`]:
//!    an enemy seen every second by someone converges to its weight. The side's own casualties ([`Intel::death`]) add as
//!    evidence of an enemy, [`DEATH_SIGHTINGS`] sightings' worth.
//! 2. **Enemy traffic**: a coarse grid of [`CELL_M`] cells (sparse, a `BTreeMap`, so the order is fixed) where each sighting is
//!    stamped with a precomputed template of radius [`STAMP_CELLS`] (normalised: it adds the weight in all), on foot and in a vehicle
//!    apart, with a long half-life ([`TRAFFIC_HALF_S`]): where they really pass, and where the enemy is now.
//!
//! A sighting counts once per enemy and second however many bots see it. The layers age when the side splits up (every few
//! seconds), not every tick.
use std::collections::{BTreeMap, BTreeSet};

/// The presence of a sighting falls to 0 at this distance from a flag, m.
pub const PRESENCE_M: f32 = 150.0;
/// The presence halves in this, s; the traffic, in this.
pub const PRESENCE_HALF_S: f64 = 20.0;
pub const TRAFFIC_HALF_S: f64 = 90.0;
/// What one sighting adds to the presence, per unit of weight: so an enemy seen every second converges to its weight.
pub const RATE: f32 = 0.034_06;
/// An armoured vehicle weighs this many soldiers.
pub const ARMOUR_W: f32 = 3.0;
/// One of the side's casualties counts as this many sightings at the place it fell.
pub const DEATH_SIGHTINGS: f32 = 10.0;
/// The traffic grid's cells, m, and the template's radius, cells.
pub const CELL_M: f32 = 32.0;
pub const STAMP_CELLS: i32 = 2;
/// Habits: the same enemy entering the same cell this many times within this, s, is a habit.
pub const HABIT_PASSES: usize = 3;
pub const HABIT_S: f64 = 300.0;

/// What was seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    OnFoot,
    Armour,
    /// another ground vehicle or a boat
    Vehicle,
    Aircraft,
}

impl Seen {
    pub fn weight(self) -> f32 {
        match self {
            Seen::OnFoot | Seen::Vehicle => 1.0,
            Seen::Armour => ARMOUR_W,
            Seen::Aircraft => 0.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Intel {
    flags: Vec<[f32; 3]>,
    presence: Vec<f32>,
    /// per cell: (on foot, in a vehicle)
    traffic: BTreeMap<(i32, i32), [f32; 2]>,
    /// the (enemy, second) already counted
    counted: BTreeSet<(usize, i64)>,
    aged: Option<f64>,
    /// habits: per enemy, the cell it was last seen in; per (enemy, cell), when it entered it
    last_cell: BTreeMap<usize, (i32, i32)>,
    passes: BTreeMap<(usize, (i32, i32)), Vec<f64>>,
}

/// The stamp: per cell offset, its share (they add to 1).
fn template() -> &'static [((i32, i32), f32)] {
    static T: std::sync::OnceLock<Vec<((i32, i32), f32)>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut v = Vec::new();
        for dz in -STAMP_CELLS..=STAMP_CELLS {
            for dx in -STAMP_CELLS..=STAMP_CELLS {
                let w = 1.0 - ((dx * dx + dz * dz) as f32).sqrt() / (STAMP_CELLS as f32 + 1.0);
                if w > 0.0 {
                    v.push(((dx, dz), w));
                }
            }
        }
        let total: f32 = v.iter().map(|x| x.1).sum();
        v.into_iter().map(|(k, w)| (k, w / total)).collect()
    })
}

fn cell(pos: [f32; 3]) -> (i32, i32) {
    ((pos[0] / CELL_M).floor() as i32, (pos[2] / CELL_M).floor() as i32)
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

impl Intel {
    /// The flags (their order is the presence's); a change of flags starts the presence again.
    pub fn set_flags(&mut self, flags: &[[f32; 3]]) {
        if self.flags.len() != flags.len() || self.flags.iter().zip(flags).any(|(a, b)| flat(*a, *b) > 1.0) {
            self.flags = flags.to_vec();
            self.presence = vec![0.0; flags.len()];
        }
    }

    fn add_presence(&mut self, pos: [f32; 3], w: f32) {
        for (f, p) in self.flags.iter().zip(self.presence.iter_mut()) {
            let fall = 1.0 - flat(*f, pos) / PRESENCE_M;
            if fall > 0.0 {
                *p += w * fall;
            }
        }
    }

    /// An enemy (`player`) seen at `pos` at `t`; counted once per enemy and second.
    pub fn sighting(&mut self, t: f64, player: usize, pos: [f32; 3], what: Seen) {
        if !self.counted.insert((player, t.floor() as i64)) {
            return;
        }
        // (habits, of those on foot: where the same one passes again and again)
        if what == Seen::OnFoot {
            let c = cell(pos);
            if self.last_cell.insert(player, c) != Some(c) {
                let v = self.passes.entry((player, c)).or_default();
                v.retain(|&w| t - w < HABIT_S);
                v.push(t);
            }
        }
        let w = what.weight();
        if w <= 0.0 {
            return;
        }
        self.add_presence(pos, w * RATE);
        let (cx, cz) = cell(pos);
        let layer = (what != Seen::OnFoot) as usize;
        for &((dx, dz), share) in template() {
            self.traffic.entry((cx + dx, cz + dz)).or_insert([0.0; 2])[layer] += w * share;
        }
    }

    /// One of the side fell at `pos`: evidence of an enemy near.
    pub fn death(&mut self, pos: [f32; 3]) {
        self.add_presence(pos, DEATH_SIGHTINGS * RATE);
    }

    /// Ages the layers to `t`.
    pub fn age(&mut self, t: f64) {
        let dt = self.aged.map_or(0.0, |a| (t - a).max(0.0));
        self.aged = Some(t);
        if dt > 0.0 {
            let kp = 0.5f32.powf((dt / PRESENCE_HALF_S) as f32);
            self.presence.iter_mut().for_each(|p| *p *= kp);
            let kt = 0.5f32.powf((dt / TRAFFIC_HALF_S) as f32);
            self.traffic.retain(|_, c| {
                c[0] *= kt;
                c[1] *= kt;
                c[0] + c[1] > 1e-3
            });
        }
        let now = t.floor() as i64;
        self.counted.retain(|k| now - k.1 < 2);
        self.passes.retain(|_, v| {
            v.retain(|&w| t - w < HABIT_S);
            !v.is_empty()
        });
    }

    /// The estimated enemy presence at flag `i`, in enemies.
    pub fn presence(&self, i: usize) -> f32 {
        self.presence.get(i).copied().unwrap_or(0.0)
    }

    /// The presence at every flag.
    pub fn presences(&self) -> &[f32] {
        &self.presence
    }

    /// The enemy traffic in the cell of `pos`: on foot, or in a vehicle.
    pub fn traffic(&self, pos: [f32; 3], vehicle: bool) -> f32 {
        self.traffic.get(&cell(pos)).map_or(0.0, |c| c[vehicle as usize])
    }

    /// The strongest habit at `t`: the centre of the cell an enemy on foot has entered at least [`HABIT_PASSES`] times in the last
    /// [`HABIT_S`], with that enemy (the one with most passes; ties, the first in order).
    pub fn habit(&self, t: f64) -> Option<([f32; 3], usize)> {
        self.habit_where(t, |_| true)
    }

    /// [`Intel::habit`] among the cells `keep` accepts (by centre).
    pub fn habit_where(&self, t: f64, keep: impl Fn([f32; 3]) -> bool) -> Option<([f32; 3], usize)> {
        let centre = |x: i32, z: i32| [(x as f32 + 0.5) * CELL_M, 0.0, (z as f32 + 0.5) * CELL_M];
        self.passes
            .iter()
            .map(|(k, v)| (k, v.iter().filter(|&&w| t - w < HABIT_S).count()))
            .filter(|x| x.1 >= HABIT_PASSES && keep(centre((x.0).1.0, (x.0).1.1)))
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
            .map(|((p, (x, z)), _)| (centre(*x, *z), *p))
    }

    /// The cells with traffic, as (centre, on foot, in a vehicle), in a fixed order.
    pub fn traffic_cells(&self) -> impl Iterator<Item = ([f32; 3], f32, f32)> + '_ {
        self.traffic.iter().map(|(&(x, z), c)| ([(x as f32 + 0.5) * CELL_M, 0.0, (z as f32 + 0.5) * CELL_M], c[0], c[1]))
    }
}

/// For checking the estimate against the real presence (`examples/intel_sanity`): when on, each side's presence per flag at every
/// split goes to a log, read with [`take_log`]. Off, nothing is kept.
static LOG_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LOG: std::sync::Mutex<Vec<(f64, u32, Vec<f32>)>> = std::sync::Mutex::new(Vec::new());

pub fn log_presence(on: bool) {
    LOG_ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn note(t: f64, team: u32, intel: &Intel) {
    if LOG_ON.load(std::sync::atomic::Ordering::Relaxed) {
        LOG.lock().unwrap_or_else(|e| e.into_inner()).push((t, team, intel.presence.clone()));
    }
}

/// The presence logged since the last call: (when, side, per flag).
pub fn take_log() -> Vec<(f64, u32, Vec<f32>)> {
    std::mem::take(&mut *LOG.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_enemy_seen_every_second_converges_to_its_weight() {
        let mut i = Intel::default();
        i.set_flags(&[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]]);
        for s in 0..200 {
            let t = s as f64;
            // seen by three bots in the same second: once
            for _ in 0..3 {
                i.sighting(t, 7, [0.0, 0.0, 0.0], Seen::OnFoot);
            }
            if s % 4 == 0 {
                i.age(t);
            }
        }
        assert!((i.presence(0) - 1.0).abs() < 0.15, "{}", i.presence(0));
        assert_eq!(i.presence(1), 0.0);
        // armour at 75 m: half of 3
        let mut j = Intel::default();
        j.set_flags(&[[0.0, 0.0, 0.0]]);
        for s in 0..200 {
            j.sighting(s as f64, 1, [75.0, 0.0, 0.0], Seen::Armour);
            j.age(s as f64);
        }
        assert!((j.presence(0) - 1.5).abs() < 0.2, "{}", j.presence(0));
    }

    #[test]
    fn it_decays_and_the_traffic_keeps_longer() {
        let mut i = Intel::default();
        i.set_flags(&[[0.0, 0.0, 0.0]]);
        i.age(0.0);
        for s in 0..60 {
            i.sighting(s as f64, 1, [10.0, 0.0, 10.0], Seen::OnFoot);
        }
        i.age(60.0);
        let p0 = i.presence(0);
        let t0 = i.traffic([10.0, 0.0, 10.0], false);
        i.age(80.0);
        assert!((i.presence(0) / p0 - 0.5).abs() < 0.01);
        assert!(i.traffic([10.0, 0.0, 10.0], false) / t0 > 0.8);
        assert_eq!(i.traffic([10.0, 0.0, 10.0], true), 0.0);
        // the stamp adds the weight in all
        let mut j = Intel::default();
        j.sighting(0.0, 1, [10.0, 0.0, 10.0], Seen::Armour);
        let total: f32 = j.traffic_cells().map(|c| c.2).sum();
        assert!((total - ARMOUR_W).abs() < 1e-4);
    }

    #[test]
    fn three_passes_make_a_habit() {
        let mut i = Intel::default();
        // back and forth between two cells: enters the first at 0, 20, 40
        for (k, x) in [10.0, 40.0, 10.0, 40.0, 10.0].iter().enumerate() {
            i.sighting(k as f64 * 10.0, 5, [*x, 0.0, 10.0], Seen::OnFoot);
        }
        let (c, p) = i.habit(40.0).expect("a habit");
        assert_eq!(p, 5);
        assert!((c[0] - 16.0).abs() < 1e-3 && (c[2] - 16.0).abs() < 1e-3, "{c:?}");
        assert!(i.habit(40.0 + HABIT_S).is_none());
    }

    #[test]
    fn a_casualty_is_evidence() {
        let mut i = Intel::default();
        i.set_flags(&[[0.0, 0.0, 0.0]]);
        i.death([0.0, 0.0, 0.0]);
        assert!((i.presence(0) - DEATH_SIGHTINGS * RATE).abs() < 1e-5);
    }
}
