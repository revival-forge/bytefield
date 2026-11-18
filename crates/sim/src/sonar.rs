//! The sonar. Read from the original game: `SonarObject`, its template, its
//! `handleUpdate`, `getSensedObjects` and `scanForEnemySonars`; `findSonarObject`; the controlled object's sonar pos lookup
//! and the template's `getSubPos` and `getSonarPos`.
//!
//! A `SonarObject` is a part of a ship (the Fletcher's and the Hatsuzuki's `DestroyerSonar`, the submarines'
//! `SubmarineSonar`, the radar towers of Battle_of_Britain): `detectionRadius` (50 if not set),
//! `scanForEnemySonars` and `enableRadarMode`.
//!
//! - **What it senses** (`getSensedObjects`, which only the client calls: its minimap): every object of the world's
//!   grid with armour within the radius on the ground (x, z), under the sonar (in radar mode, over it) and no farther
//!   than the radius up or down; each with `1 − dy / radius`, dy = the sonar's height − the object's.
//! - **The pings** (`handleUpdate` with `scanForEnemySonars`): while the local player sits where `subPos` or
//!   `sonarPos` is set, every 0.1 s each sonar that scans looks for the sonars that do not (a destroyer's) on
//!   objects of another team, within its radius on the ground. The nearest sets its timer to `max(2 d/r, 0.4)` and
//!   plays its sound with `max(d/r, 0.1)` (the scripts' `Default`: the volume `1 − x`); when the timer runs out it
//!   plays again. Nothing in it looks whether that sonar is the player's: the sound's own distance fade
//!   (`SonarSub.ssc`: 10 to 20 m) keeps it in the submarine.

use crate::vehicle::{Component, Value};

use crate::vecmath::V3;
type M3 = [V3; 3];

/// The sonar template's constructor: `detectionRadius` when the template sets none.
pub const DEFAULT_RADIUS: f32 = 50.0;
/// `handleUpdate`: how often a scanning sonar looks for the others (`0x3dcccccd`, s).
pub const SCAN_EVERY: f32 = 0.1;
/// `scanForEnemySonars`: the shortest time between pings (`_DAT_086c4f70`, s) and the least value its sound gets
/// (`_DAT_086b1ca0`).
pub const MIN_PING_INTERVAL: f32 = 0.4;
pub const MIN_PING_VALUE: f32 = 0.1;

/// A vehicle's sonar: the first `SonarObject` of its tree (`findSonarObject`: depth first, a child before its own
/// children), where it is in the root's frame and its template's settings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Sonar {
    pub template: String,
    pub offset: V3,
    pub radius: f32,
    /// `scanForEnemySonars`: it pings when another team's sonar that does not scan is near (a submarine's)
    pub scan: bool,
    /// `enableRadarMode`: what is over it, not under it (the radar towers)
    pub radar: bool,
}

fn on(c: &Component, name: &str) -> bool {
    match c.param(name) {
        Some(Value::Number(n)) => *n != 0.0,
        Some(Value::Text(t)) => t.eq_ignore_ascii_case("c_true") || t.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn find(c: &Component, at: V3, rows: M3) -> Option<Sonar> {
    for k in &c.children {
        let p = crate::craft::to_parent([k.position[0] as f32, k.position[1] as f32, k.position[2] as f32], &rows);
        let pos = [at[0] + p[0], at[1] + p[1], at[2] + p[2]];
        if k.kind.eq_ignore_ascii_case("SonarObject") {
            let radius = k.param("detectionRadius").and_then(Value::number).map_or(DEFAULT_RADIUS, |v| v as f32);
            return Some(Sonar { template: k.name.clone(), offset: pos, radius, scan: on(k, "scanForEnemySonars"), radar: on(k, "enableRadarMode") });
        }
        let r = crate::craft::mul(&crate::craft::ypr([k.rotation[0] as f32, k.rotation[1] as f32, k.rotation[2] as f32]), &rows);
        if let Some(s) = find(k, pos, r) {
            return Some(s);
        }
    }
    None
}

impl Sonar {
    /// The sonar of a vehicle's model, if it has one.
    // TODO: the offset is where it was built: a sonar under a turning bundle would not follow it (none in vanilla)
    pub fn from_model(c: &Component) -> Option<Sonar> {
        find(c, [0.0; 3], crate::craft::ypr([0.0; 3]))
    }

    /// Where it is with its vehicle at `pos` with `rows`.
    pub fn world(&self, pos: V3, rows: &M3) -> V3 {
        let o = crate::craft::to_parent(self.offset, rows);
        [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]]
    }

    /// `getSensedObjects` for one object at `obj` seen from the sonar at `at`: its value, or None when out.
    pub fn sense(&self, at: V3, obj: V3) -> Option<f32> {
        let r = self.radius;
        let (dx, dy, dz) = (at[0] - obj[0], at[1] - obj[1], at[2] - obj[2]);
        let side = if self.radar { dy <= 0.0 } else { dy >= 0.0 };
        if !side || dx * dx + dz * dz > r * r {
            return None;
        }
        let near = if self.radar { dy * dy <= r * r } else { dy <= r };
        near.then(|| 1.0 - dy / r)
    }
}

/// A scanning sonar's ping (the time to the next scan, the time to the next ping),
/// on the local player's machine.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ping {
    pub scan_in: f32,
    pub timer: f32,
    /// the value its sound was last given (the sound keeps it when the timer runs out)
    pub value: f32,
}

impl Ping {
    /// `handleUpdate` (the local player in a `subPos` or `sonarPos` seat) over `dt`; `near` gives the ground
    /// distances, as fractions of the radius, of the other team's sonars that do not scan (only those within it
    /// count). Whether its sound plays in this frame (with [`Ping::value`]).
    pub fn update(&mut self, dt: f32, near: impl FnOnce() -> Vec<f32>) -> bool {
        let mut play = false;
        if self.timer > 0.0 {
            self.timer -= dt;
            if self.timer <= 0.0 {
                self.timer = 0.0;
                play = true;
            }
        }
        self.scan_in -= dt;
        if self.scan_in <= 0.0 {
            for f in near().into_iter().filter(|f| *f <= 1.0) {
                let interval = (f + f).max(MIN_PING_INTERVAL);
                if self.timer <= 0.0 || interval < self.timer {
                    self.timer = interval;
                    self.value = f.max(MIN_PING_VALUE);
                    play = true;
                }
            }
            self.scan_in = SCAN_EVERY;
        }
        play
    }
}

impl crate::Sim<'_> {
    /// The seat the soldier `i` sits in, if any.
    fn seat_of(&self, i: usize) -> Option<&crate::entry::Seat> {
        let e = &self.vehicles[i].entry;
        e.inside.and_then(|k| self.vehicles[k].entry.seats.get(e.position))
    }

    /// Whether the soldier `i` sits where the minimap is the sonar's: its seat's PlayerControlObject has `sonarPos`
    /// (the Fletcher's and the Hatsuzuki's driver).
    pub fn sonar_seat(&self, i: usize) -> bool {
        self.seat_of(i).is_some_and(|s| s.sonar)
    }

    /// Whether the soldier `i` sits where the sonars ping (the sonar's update handling: `getSubPos` or `getSonarPos`
    /// of the player's object).
    pub fn hears_sonars(&self, i: usize) -> bool {
        self.seat_of(i).is_some_and(|s| s.sonar || s.hatch.is_some())
    }

    /// Where the vehicle `k`'s sonar is now.
    pub fn sonar_at(&self, k: usize) -> Option<V3> {
        let v = &self.vehicles[k];
        v.entry.sonar.as_ref().map(|s| s.world(v.craft.body.pos, &v.craft.body.rows))
    }

    /// `getSensedObjects` of the vehicle `k`'s sonar: the objects in the world (not inside another) with armour it
    /// senses and their values, itself among them (the minimap leaves it out).
    pub fn sonar_contacts(&self, k: usize) -> Vec<(usize, f32)> {
        let (Some(s), Some(at)) = (self.vehicles[k].entry.sonar.as_ref(), self.sonar_at(k)) else { return Vec::new() };
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| v.in_world() && v.armor.is_some())
            .filter_map(|(j, v)| s.sense(at, v.craft.body.pos).map(|f| (j, f)))
            .collect()
    }

    /// For `scanForEnemySonars` of the vehicle `k`'s sonar: the ground distance, as a fraction of its radius, to each
    /// sonar that does not scan on another object (in the world, with armour) of another team than `k`'s, within
    /// the radius.
    pub fn enemy_sonars(&self, k: usize) -> Vec<f32> {
        let (Some(s), Some(at)) = (self.vehicles[k].entry.sonar.as_ref(), self.sonar_at(k)) else { return Vec::new() };
        let team = self.vehicles[k].team;
        (0..self.vehicles.len())
            .filter(|&j| j != k && self.vehicles[j].in_world() && self.vehicles[j].armor.is_some() && self.vehicles[j].team != team)
            .filter(|&j| self.vehicles[j].entry.sonar.as_ref().is_some_and(|o| !o.scan))
            .filter_map(|j| self.sonar_at(j))
            .map(|p| ((at[0] - p[0]).powi(2) + (at[2] - p[2]).powi(2)).sqrt() / s.radius)
            .filter(|f| *f <= 1.0)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sonar(radar: bool) -> Sonar {
        Sonar { template: "S".into(), offset: [0.0; 3], radius: 400.0, scan: false, radar }
    }

    #[test]
    fn a_sonar_senses_under_it_and_a_radar_over_it() {
        let s = sonar(false);
        assert_eq!(s.sense([0.0, 10.0, 0.0], [100.0, 10.0, 0.0]), Some(1.0));
        assert_eq!(s.sense([0.0, 10.0, 0.0], [100.0, -90.0, 0.0]), Some(0.75));
        assert_eq!(s.sense([0.0, 10.0, 0.0], [100.0, 11.0, 0.0]), None, "over it");
        assert_eq!(s.sense([0.0, 10.0, 0.0], [300.0, 0.0, 300.0]), None, "too far on the ground");
        let r = sonar(true);
        assert_eq!(r.sense([0.0, 10.0, 0.0], [0.0, 210.0, 0.0]), Some(1.5));
        assert_eq!(r.sense([0.0, 10.0, 0.0], [0.0, 5.0, 0.0]), None, "under it");
        assert_eq!(r.sense([0.0, 10.0, 0.0], [0.0, 500.0, 0.0]), None, "too high");
    }

    #[test]
    fn the_ping_comes_sooner_the_nearer() {
        let mut p = Ping::default();
        // the first scan: 100 m of 400, a ping now and the next in 0.5 s
        assert!(p.update(1.0 / 60.0, || vec![0.25, 0.9]));
        assert_eq!((p.timer, p.value), (0.5, 0.25));
        // the scans keep the timer (not shorter) and play nothing
        let mut plays = 0;
        for _ in 0..20 {
            if p.update(1.0 / 60.0, || vec![0.25]) {
                plays += 1;
            }
        }
        assert_eq!(plays, 0);
        // nobody near: it runs out and plays once more, then nothing
        let mut t = 0.0;
        let mut at = Vec::new();
        for _ in 0..120 {
            t += 1.0 / 60.0;
            if p.update(1.0 / 60.0, Vec::new) {
                at.push(t);
            }
        }
        assert_eq!(at.len(), 1, "{at:?}");
        // very near: never faster than 0.4 s, and the value no less than 0.1
        let mut p = Ping::default();
        assert!(p.update(0.01, || vec![0.01]));
        assert_eq!((p.timer, p.value), (MIN_PING_INTERVAL, MIN_PING_VALUE));
        // out of its radius: nothing
        assert!(!Ping::default().update(0.01, || vec![1.5]));
    }
}
