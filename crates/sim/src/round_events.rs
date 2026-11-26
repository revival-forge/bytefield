//! A round's events with their place, for the per-map memory the bots may read later (what a veteran of the map knows: where one
//! tends to die, which way into a flag works): the deaths (where, from what, how far from the killer), the vehicles lost, the flags
//! taken, every arrival at a flag the side does not hold (the sector it came in from, its last positions, whether it was taken) and
//! the time each side spent in each cell, on foot and in vehicles (deaths per second of presence need the seconds), and the hits
//! on soldiers (when, by whom, on whom, how much: a fight's accuracy needs the hits that killed no one too).
//!
//! It only reads the sim ([`RoundEvents::step`] after each [`crate::Sim::step`]): the round is the same with or without it. Maps are
//! `BTreeMap`s, so the same round gives the same output.
use crate::Sim;
use crate::bot_api::VehicleClass;
use crate::cover::sector;
use crate::damage::Source;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The presence grid's cell, m (the same as the side's intel grid, [`crate::bots::intel::CELL_M`]).
pub const CELL_M: f32 = crate::bots::intel::CELL_M;
/// A death is put down to the last damage its soldier (or the vehicle it was in) took within this long, s.
pub const KILL_WINDOW_S: f64 = 2.0;
/// An approach to a flag starts this far from it, m: the sector of the arrival is where the approach started.
pub const APPROACH_M: f32 = 60.0;
/// The arrival: inside the flag's radius, but at least this, m.
pub const ARRIVE_MIN_M: f32 = 10.0;
/// An arrival ends as left this much past the arrival radius, m.
pub const LEAVE_EXTRA_M: f32 = 10.0;
/// The positions kept of each player for an arrival's path: one every this long, s, the last [`PATH_POINTS`].
pub const PATH_EVERY_S: f64 = 5.0;
pub const PATH_POINTS: usize = 6;

/// How a player is in the round: on foot or in a vehicle of a class.
fn mount(s: &Sim, soldier: usize) -> &'static str {
    match s.vehicles[soldier].entry.inside.and_then(|v| s.vehicles.get(v)).map(VehicleClass::of) {
        None => "foot",
        Some(VehicleClass::Fixed) => "fixed",
        Some(VehicleClass::Plane) => "plane",
        Some(VehicleClass::Helicopter) => "helicopter",
        Some(VehicleClass::Airship) => "airship",
        Some(VehicleClass::Ship) => "ship",
        Some(VehicleClass::Submarine) => "submarine",
        Some(VehicleClass::Tank) => "tank",
        Some(VehicleClass::Car) => "car",
        Some(VehicleClass::Walker) => "walker",
        Some(VehicleClass::Other) => "other",
    }
}

fn source(s: Source) -> &'static str {
    match s {
        Source::Hit => "hit",
        Source::Explosion => "explosion",
        Source::Crash => "crash",
        Source::Kickback => "kickback",
        Source::Burning => "burning",
        Source::UpsideDown => "upside_down",
        #[allow(unreachable_patterns)]
        _ => "other",
    }
}

fn d2(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn r1(x: f32) -> f64 {
    (x as f64 * 10.0).round() / 10.0
}

fn xz(p: [f32; 3]) -> [f64; 3] {
    [r1(p[0]), r1(p[1]), r1(p[2])]
}

/// An arrival at a flag the side did not hold, while it lasts.
#[derive(Clone)]
struct Arrival {
    t: f64,
    player: usize,
    team: u32,
    flag: usize,
    sector: Option<usize>,
    approach_s: Option<f64>,
    mount: &'static str,
    path: Vec<[f32; 3]>,
}

/// What each player was last step.
#[derive(Clone, Default)]
struct Seen {
    deaths: u32,
    soldier: Option<usize>,
    inside: Option<usize>,
    pos: [f32; 3],
    mount: &'static str,
    /// the approach under way: flag, when and where it started
    approach: Option<(usize, f64, [f32; 3])>,
    path: Vec<[f32; 3]>,
    path_t: f64,
}

/// The round's events, filled by [`RoundEvents::step`] and given out by [`RoundEvents::finish`].
#[derive(Default)]
pub struct RoundEvents {
    seen: Vec<Seen>,
    /// per vehicle: its last damage (time, by, source)
    last_hit: BTreeMap<usize, (f64, Option<usize>, Source)>,
    /// per vehicle with a crew: the crew's side, last second
    crew: BTreeMap<usize, u32>,
    damage_seen: usize,
    flag_team: Vec<u32>,
    open: BTreeMap<usize, Arrival>,
    deaths: Vec<Value>,
    losses: Vec<Value>,
    captures: Vec<Value>,
    arrivals: Vec<Value>,
    hits: Vec<Value>,
    /// (side, mounted, cell x, cell z) -> seconds
    presence: BTreeMap<(u32, bool, i32, i32), u32>,
}

impl RoundEvents {
    pub fn new() -> Self {
        Self::default()
    }

    /// The side and player of a vehicle that gave damage: a soldier's player, or the player of its first occupant.
    fn who(s: &Sim, soldier_of: &BTreeMap<usize, usize>, v: usize) -> Option<usize> {
        soldier_of.get(&v).copied().or_else(|| s.vehicles.get(v)?.entry.occupants.iter().flatten().find_map(|o| soldier_of.get(o).copied()))
    }

    pub fn step(&mut self, s: &Sim) {
        let t = s.clock.steps as f64 / 30.0;
        if self.seen.len() < s.players.len() {
            self.seen.resize(s.players.len(), Seen::default());
        }
        let soldier_of: BTreeMap<usize, usize> = s.players.iter().enumerate().filter_map(|(k, p)| p.soldier.map(|i| (i, k))).collect();
        // the damages
        for e in s.journal.damage_log.get(self.damage_seen..).unwrap_or(&[]) {
            if e.amount > 0.0 {
                self.last_hit.insert(e.vehicle, (t, e.by, e.source));
                if let (Some(&victim), Some(by)) = (soldier_of.get(&e.vehicle), e.by.and_then(|b| Self::who(s, &soldier_of, b))) {
                    self.hits.push(json!([t, by, victim, r1(e.amount), source(e.source)]));
                }
            }
            let Some(v) = s.vehicles.get(e.vehicle) else { continue };
            if e.hp <= 0.0 && e.status.is_some() && !v.has_soldier_contact() {
                if let Some(&team) = self.crew.get(&e.vehicle) {
                    let by_team = e.by.and_then(|b| Self::who(s, &soldier_of, b)).map(|k| s.players[k].team);
                    self.losses.push(json!({"t": t, "team": team, "class": format!("{:?}", VehicleClass::of(v)).to_ascii_lowercase(), "pos": xz(v.craft.body.pos), "source": source(e.source), "by_team": by_team}));
                }
            }
        }
        self.damage_seen = s.journal.damage_log.len();
        let Some(c) = s.modes.conquest.as_ref() else { return };
        // the flags taken
        if self.flag_team.len() != c.points.len() {
            self.flag_team = c.points.iter().map(|p| p.team).collect();
        }
        let mut taken: Vec<(usize, u32)> = Vec::new();
        for (i, f) in c.points.iter().enumerate() {
            if self.flag_team[i] != f.team {
                self.captures.push(json!({"t": t, "flag": i, "from": self.flag_team[i], "to": f.team, "pos": xz(f.pos)}));
                taken.push((i, f.team));
                self.flag_team[i] = f.team;
            }
        }
        for (i, team) in taken.iter().copied().filter(|x| (1..=2).contains(&x.1)) {
            let done: Vec<usize> = self.open.iter().filter(|(_, a)| a.flag == i && a.team == team).map(|(&k, _)| k).collect();
            for k in done {
                let a = self.open.remove(&k).unwrap();
                self.close(a, t, "taken");
            }
        }
        let second = s.clock.steps % 30 == 0;
        for (k, p) in s.players.iter().enumerate() {
            let team = p.team;
            let deaths_before = self.seen[k].deaths;
            // the deaths
            if p.deaths > deaths_before && (1..=2).contains(&team) {
                let was = self.seen[k].clone();
                let hit = [was.soldier, was.inside].into_iter().flatten().filter_map(|v| self.last_hit.get(&v).copied()).filter(|h| t - h.0 <= KILL_WINDOW_S).max_by(|a, b| a.0.total_cmp(&b.0));
                let killer = hit.and_then(|h| h.1).and_then(|b| Some((b, Self::who(s, &soldier_of, b)?)));
                let (kteam, kpos, kmount) = match killer {
                    Some((b, kk)) => (Some(s.players[kk].team), s.vehicles.get(b).map(|v| v.craft.body.pos), s.players[kk].soldier.map(|i| mount(s, i))),
                    None => (None, None, None),
                };
                let mut ev = json!({"t": t, "player": k, "team": team, "pos": xz(was.pos), "mount": was.mount, "source": hit.map(|h| source(h.2)), "killer": killer.map(|x| x.1), "killer_team": kteam, "killer_mount": kmount});
                if let Some(kp) = kpos {
                    ev["killer_pos"] = json!(xz(kp));
                    ev["distance"] = json!(r1(d2(kp, was.pos)));
                }
                self.deaths.push(ev);
                if let Some(a) = self.open.remove(&k) {
                    self.close(a, t, "died");
                }
                self.seen[k].approach = None;
                self.seen[k].path.clear();
            }
            let sol = p.soldier.filter(|_| p.alive).and_then(|i| s.vehicles.get(i).map(|v| (i, v)));
            let me = &mut self.seen[k];
            me.deaths = p.deaths;
            let Some((i, v)) = sol else {
                me.soldier = None;
                me.inside = None;
                continue;
            };
            let pos = v.craft.body.pos;
            me.soldier = Some(i);
            me.inside = v.entry.inside;
            me.pos = pos;
            me.mount = mount(s, i);
            if !(1..=2).contains(&team) {
                continue;
            }
            if me.path.is_empty() || t - me.path_t >= PATH_EVERY_S {
                me.path.push(pos);
                me.path_t = t;
                if me.path.len() > PATH_POINTS {
                    me.path.remove(0);
                }
            }
            if second {
                let cell = ((pos[0] / CELL_M).floor() as i32, (pos[2] / CELL_M).floor() as i32);
                *self.presence.entry((team, me.inside.is_some(), cell.0, cell.1)).or_insert(0) += 1;
                if let Some(w) = me.inside {
                    self.crew.insert(w, team);
                }
            }
            // the approaches and arrivals
            if let Some(a) = self.open.get(&k) {
                let f = &c.points[a.flag];
                if d2(pos, f.pos) > f.template.radius.max(ARRIVE_MIN_M) + LEAVE_EXTRA_M {
                    let a = self.open.remove(&k).unwrap();
                    self.close(a, t, "left");
                }
                continue;
            }
            let me = &mut self.seen[k];
            let near = c
                .points
                .iter()
                .enumerate()
                .filter(|(_, f)| f.team != team && !f.template.unable_to_change_team)
                .map(|(fi, f)| (fi, d2(pos, f.pos)))
                .filter(|x| x.1 < APPROACH_M)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match near {
                None => me.approach = None,
                Some((fi, d)) => {
                    if me.approach.is_none_or(|x| x.0 != fi) {
                        me.approach = Some((fi, t, pos));
                    }
                    let f = &c.points[fi];
                    if d <= f.template.radius.max(ARRIVE_MIN_M) {
                        let (_, t0, p0) = me.approach.unwrap();
                        // (an approach that started inside the ring, at a spawn by the flag, has no sector)
                        let from_ring = d2(p0, f.pos) >= APPROACH_M - 5.0;
                        let a = Arrival { t, player: k, team, flag: fi, sector: from_ring.then(|| sector(p0[0] - f.pos[0], p0[2] - f.pos[2])), approach_s: from_ring.then(|| t - t0), mount: me.mount, path: me.path.clone() };
                        me.approach = None;
                        self.open.insert(k, a);
                    }
                }
            }
        }
        self.crew.retain(|v, _| s.vehicles.get(*v).is_some_and(|x| !x.removed));
    }

    fn close(&mut self, a: Arrival, t: f64, outcome: &str) {
        let path: Vec<[f64; 3]> = a.path.iter().map(|p| xz(*p)).collect();
        self.arrivals.push(json!({"t": a.t, "player": a.player, "team": a.team, "flag": a.flag, "sector": a.sector, "approach_s": a.approach_s, "mount": a.mount, "path": path, "outcome": outcome, "end_t": t}));
    }

    /// The events, as JSON: `deaths`, `losses`, `captures`, `arrivals` (the ones still open at the end as `open`), `presence`
    /// (`[side, mounted, cell x, cell z, seconds]`, cells of [`CELL_M`]) and `hits` on soldiers (`[t, by player, on player,
    /// amount, source]`).
    pub fn finish(mut self, s: &Sim) -> Value {
        let t = s.clock.steps as f64 / 30.0;
        for a in std::mem::take(&mut self.open).into_values() {
            self.close(a, t, "open");
        }
        let flags: Vec<Value> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().map(|f| json!({"pos": xz(f.pos), "radius": f.template.radius, "takeable": !f.template.unable_to_change_team})).collect());
        let presence: Vec<Value> = self.presence.iter().map(|(&(team, m, x, z), &n)| json!([team, m, x, z, n])).collect();
        json!({"format": 1, "cell_m": CELL_M, "flags": flags, "deaths": self.deaths, "losses": self.losses, "captures": self.captures, "arrivals": self.arrivals, "presence": presence, "hits": self.hits})
    }
}
