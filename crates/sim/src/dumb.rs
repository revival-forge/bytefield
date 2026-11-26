//! What looks dumb in a round, bot by bot: quiet or stuck, going round in circles, firing
//! at a wall or with no enemy about, a flag next to it that it ignores, how long until its first shot and kill, friendly
//! fire, vehicles left or damaged with no attacker. Every event has its time, place and bot, so a round can be replayed to look at it.
//!
//! [`Detector`] sees only neutral samples, once a second, so the same code measures the bench ([`Watcher`], `BOTS_DUMB` in
//! `examples/bots`) and a server's reference run (`examples/dumb_trace`): the numbers can go in one table
//!. What the reference run has not (flags, per bot kills) stays out of its report, not at 0.
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};


/// With bots that say nothing of their aim or plan, what is left is what can be seen from outside.
mod hooks {
    pub fn aimed_at(_: &crate::Sim, _: usize) -> Option<u32> {
        None
    }
    pub fn plan_of(_: &crate::Sim, _: usize) -> Option<String> {
        None
    }
    pub fn sample_human(_: &mut super::Watcher, _: &crate::Sim, _: f64) {}
    pub fn handle(_: &crate::Sim, v: usize) -> u32 {
        v as u32
    }
    /// A bot that says it is going somewhere.
    pub fn wants_to_move(plan: Option<&str>) -> bool {
        plan == Some("go")
    }
}

/// Seconds without moving before it counts: wanting to go somewhere, and anything else.
pub const STUCK_MIN_S: f64 = 10.0;
pub const STILL_MIN_S: f64 = 20.0;
/// Metres from where a run without moving began.
pub const STILL_RADIUS: f32 = 2.0;
/// The same in a vehicle that moves: a stuck driver backs up and tries again a few metres either way (GC_Tatooine's AT-ST at the wall).
pub const STILL_RADIUS_MOUNTED: f32 = 6.0;
/// The window for going round in circles, its path and the farthest it may get, on foot and mounted.
pub const CIRCLE_WINDOW: usize = 20;
pub const CIRCLE_PATH: [f32; 2] = [25.0, 80.0];
pub const CIRCLE_NET: [f32; 2] = [5.0, 15.0];
/// An enemy this near, or a shot this recent: what it does then is fighting, not idling.
pub const COMBAT_M: f32 = 60.0;
pub const COMBAT_S: f64 = 5.0;
/// A shot with no live enemy this near is at nothing.
pub const SPRAY_M: f32 = 150.0;
/// A shot stopped by the terrain or a static this near the muzzle was fired into a wall or the ground.
pub const BLOCKED_M: f32 = 10.0;
/// A flag it can take, this near, no enemy about, seconds without getting closer.
pub const FLAG_M: f32 = 50.0;
pub const FLAG_S: f64 = 20.0;
/// A ride this short is a vehicle got into and left at once.
pub const SHORT_RIDE_S: f64 = 5.0;

/// A player at a moment (seconds apart).
#[derive(Debug, Clone, Default)]
pub struct Sample {
    pub id: usize,
    pub team: u32,
    pub alive: bool,
    pub pos: Option<[f32; 3]>,
    /// the vehicle it is in: its template and something that tells it from another
    pub vehicle: Option<(String, u64)>,
    /// that vehicle has no mobile physics (a fixed gun)
    pub fixed: bool,
    /// the behaviour of its last plan
    pub plan: Option<String>,
    /// a person: it counts as an enemy, but it is not judged
    pub human: bool,
    /// kills so far (None where it is not known)
    pub kills: Option<u32>,
    /// its mind says it stands still on purpose, and why ([`crate::bot_api::Mind::intent`]; None where it is not known)
    pub waits: Option<&'static str>,
}

/// A control point at a moment.
#[derive(Debug, Clone)]
pub struct Flag {
    pub pos: [f32; 3],
    pub radius: f32,
    pub team: u32,
    /// `unableToChangeTeam` not set
    pub takeable: bool,
    /// `onlyTakeableByTeam` (0: any)
    pub only_by: u32,
}

/// What stopped a shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// the bot's fire target
    Target,
    /// another soldier or vehicle
    Other,
    /// a static object
    Static,
    /// the terrain or water, or it ran out
    Ground,
}

/// One thing seen, with where and when to go and look at it.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub t: f64,
    pub bot: Option<usize>,
    pub team: u32,
    pub kind: &'static str,
    pub pos: [i32; 3],
    /// how long it lasted (bursts: how many shots; damage: how much)
    pub dur: f64,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
struct Bot {
    team: u32,
    human: bool,
    first_seen: Option<f64>,
    first_shot: Option<f64>,
    first_kill: Option<f64>,
    alive_s: f64,
    fixed_s: f64,
    mounted_s: f64,
    shots: u64,
    lives: u32,
    deaths: u32,
    no_shot_deaths: u32,
    shots_this_life: u32,
    was_alive: bool,
    pos: Option<[f32; 3]>,
    last_shot: Option<(f64, Option<[f32; 3]>)>,
    plan: Option<String>,
    // a run without moving: where, since when, its seconds in a fight and wanting to move
    anchor: Option<[f32; 3]>,
    still_since: f64,
    still_combat: f64,
    still_wants: f64,
    /// of the run without moving, the seconds its mind said it was waiting on purpose ([`Sample::waits`])
    still_ordered: f64,
    /// why it waited, the last time it said so
    waits_why: Option<&'static str>,
    // going round in circles
    trail: VecDeque<[f32; 3]>,
    circle: Option<(f64, [f32; 3], f64)>,
    // the vehicle it is in, since when
    ride: Option<(String, u64, f64)>,
    // the flag it ignores, since when, how near it got
    flag: Option<(usize, f64, f32)>,
    // bursts: when one began, its last shot, its number, where
    spray: Option<(f64, f64, u32, [f32; 3])>,
    blocked: Option<(f64, f64, u32, [f32; 3])>,
}

/// See the module's comment. Feed it in time order: `sample`, once a second; `shot`, `stopped` and `damage` as they happen.
#[derive(Debug, Default)]
pub struct Detector {
    bots: BTreeMap<usize, Bot>,
    now: f64,
    sampled: bool,
    live: Vec<(u32, [f32; 3])>,
    events: Vec<Event>,
    n: BTreeMap<&'static str, f64>,
    dists: Vec<f32>,
    flags_seen: bool,
    flag_teams: Vec<u32>,
    first_capture: Option<f64>,
    first_shot: Option<f64>,
    first_hit: Option<f64>,
    first_death: Option<f64>,
    first_alive: Option<f64>,
    kills_known: bool,
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn whole(p: [f32; 3]) -> [i32; 3] {
    p.map(|x| x.round() as i32)
}

impl Detector {
    pub fn new() -> Detector {
        Detector::default()
    }

    fn nearest_enemy(live: &[(u32, [f32; 3])], team: u32, pos: [f32; 3]) -> Option<f32> {
        live.iter().filter(|e| e.0 != team).map(|e| flat(e.1, pos)).min_by(f32::total_cmp)
    }

    pub fn sample(&mut self, t: f64, players: &[Sample], flags: Option<&[Flag]>) {
        let step = if self.sampled { t - self.now } else { 1.0 };
        self.now = t;
        self.sampled = true;
        self.live = players.iter().filter(|p| p.alive).filter_map(|p| Some((p.team, p.pos?))).collect();
        if !self.live.is_empty() {
            self.first_alive.get_or_insert(t);
        }
        if let Some(f) = flags {
            self.flags_seen = true;
            if self.flag_teams.len() != f.len() {
                self.flag_teams = f.iter().map(|x| x.team).collect();
            }
            for (i, x) in f.iter().enumerate() {
                if self.flag_teams[i] != x.team {
                    self.flag_teams[i] = x.team;
                    if x.team != 0 {
                        *self.n.entry("flags_taken").or_default() += 1.0;
                        self.first_capture.get_or_insert(t);
                    }
                }
            }
        }
        for p in players {
            let b = self.bots.entry(p.id).or_default();
            b.team = p.team;
            b.human = p.human;
            if p.plan.is_some() {
                b.plan = p.plan.clone();
            }
            if let Some(k) = p.kills {
                self.kills_known = true;
                if k > 0 && b.first_kill.is_none() {
                    b.first_kill = Some(t);
                }
            }
            let at = if p.alive { p.pos } else { None };
            let Some(pos) = at else {
                if b.was_alive {
                    b.was_alive = false;
                    b.deaths += 1;
                    self.first_death.get_or_insert(t);
                    b.no_shot_deaths += (b.shots_this_life == 0) as u32;
                    self.end_runs(p.id, t);
                }
                continue;
            };
            if !b.was_alive {
                b.was_alive = true;
                b.lives += 1;
                b.shots_this_life = 0;
                b.first_seen.get_or_insert(t);
                b.anchor = Some(pos);
                b.still_since = t;
                b.still_combat = 0.0;
                b.still_wants = 0.0;
                b.still_ordered = 0.0;
                b.trail.clear();
            }
            b.pos = Some(pos);
            b.alive_s += step;
            if p.vehicle.is_some() {
                b.mounted_s += step;
            }
            let enemy = Self::nearest_enemy(&self.live, p.team, pos);
            let shot_lately = b.last_shot.is_some_and(|s| t - s.0 < COMBAT_S);
            let combat = shot_lately || enemy.is_some_and(|d| d < COMBAT_M);
            // getting in or out ends the runs without moving and in circles, under what it was in: a vehicle stuck for two minutes
            // that its driver then left was counted on foot, and the wait on foot before getting in as mounted
            let b = &self.bots[&p.id];
            if b.ride.as_ref().map(|r| r.1) != p.vehicle.as_ref().map(|v| v.1) {
                self.end_still(p.id, t);
                self.end_circle(p.id, t);
                let b = self.bots.get_mut(&p.id).unwrap();
                b.anchor = Some(pos);
                b.still_since = t;
                b.still_combat = 0.0;
                b.still_wants = 0.0;
                b.still_ordered = 0.0;
                b.trail.clear();
            }
            self.watch_ride(p, pos, t);
            if p.fixed {
                let b = self.bots.get_mut(&p.id).unwrap();
                b.fixed_s += step;
                b.anchor = None;
                b.flag = None;
                continue;
            }
            self.watch_still(p, pos, t, step, combat);
            self.watch_circle(p, pos, t, step, combat);
            if let Some(f) = flags {
                self.watch_flag(p, pos, t, enemy, f);
            }
        }
    }

    /// What was open for a bot that is gone or at the end of the round.
    fn end_runs(&mut self, id: usize, t: f64) {
        self.end_still(id, t);
        self.end_circle(id, t);
        self.end_flag(id, t);
        self.bots.get_mut(&id).unwrap().ride = None;
        self.end_spray(id);
        self.end_blocked(id);
    }

    fn end_spray(&mut self, id: usize) {
        let b = self.bots.get_mut(&id).unwrap();
        let team = b.team;
        if let Some((t0, _, n, pos)) = b.spray.take() {
            self.events.push(Event { t: t0, bot: Some(id), team, kind: "spray", pos: whole(pos), dur: n as f64, detail: format!("{n} shots with no enemy within {SPRAY_M} m") });
        }
    }

    fn end_blocked(&mut self, id: usize) {
        let b = self.bots.get_mut(&id).unwrap();
        let team = b.team;
        if let Some((t0, _, n, pos)) = b.blocked.take() {
            self.events.push(Event { t: t0, bot: Some(id), team, kind: "blocked", pos: whole(pos), dur: n as f64, detail: format!("{n} shots stopped within {BLOCKED_M} m") });
        }
    }

    fn end_still(&mut self, id: usize, t: f64) {
        let b = self.bots.get_mut(&id).unwrap();
        let Some(anchor) = b.anchor.take() else { return };
        let dur = t - b.still_since;
        let wants = b.still_wants >= 0.7 * dur;
        if b.human || !((wants && dur >= STUCK_MIN_S) || dur >= STILL_MIN_S) {
            return;
        }
        // M2: a run its mind said was a wait on purpose (an ambush, an assembly point) is counted apart, not as still or stuck
        if b.still_ordered >= 0.7 * dur {
            let detail = format!("waits {} in {} ({:.0} s fighting)", b.waits_why.unwrap_or("?"), b.ride.as_ref().map_or("foot", |r| r.0.as_str()), b.still_combat);
            let team = b.team;
            self.events.push(Event { t: t - dur, bot: Some(id), team, kind: "still_ordered", pos: whole(anchor), dur, detail });
            *self.n.entry("still_ordered_s").or_default() += dur;
            return;
        }
        let mounted = b.ride.is_some();
        let kind = if wants { "stuck" } else { "still" };
        let quiet = dur - b.still_combat;
        let detail = format!("plan {} in {} ({:.0} s fighting)", b.plan.as_deref().unwrap_or("?"), b.ride.as_ref().map_or("foot", |r| r.0.as_str()), b.still_combat);
        let team = b.team;
        if crate::debug::options().quality_b16 {
            eprintln!("b16 t={:.0} bot={id} team={team} kind={kind} dur={dur:.1} fighting={:.1} in={} at={:.0},{:.0}", t - dur, b.still_combat, b.ride.as_ref().map_or("foot", |r| r.0.as_str()), anchor[0], anchor[2]);
        }
        self.events.push(Event { t: t - dur, bot: Some(id), team, kind, pos: whole(anchor), dur, detail });
        let key: &'static str = match (wants, mounted) {
            (true, false) => "stuck_foot_s",
            (true, true) => "stuck_mounted_s",
            (false, false) => "still_foot_s",
            (false, true) => "still_mounted_s",
        };
        *self.n.entry(key).or_default() += dur;
        let key: &'static str = if wants { "stuck_quiet_s" } else { "still_quiet_s" };
        *self.n.entry(key).or_default() += quiet;
        if wants {
            *self.n.entry("stuck_runs").or_default() += 1.0;
        }
    }

    fn watch_still(&mut self, p: &Sample, pos: [f32; 3], t: f64, step: f64, combat: bool) {
        let b = self.bots.get_mut(&p.id).unwrap();
        // (in the air a mounted one is never still: its height counts too)
        let r = if p.vehicle.is_some() && !p.fixed { STILL_RADIUS_MOUNTED } else { STILL_RADIUS };
        let moved = b.anchor.is_none_or(|a| flat(a, pos) > r || (a[1] - pos[1]).abs() > 2.0 * r);
        if moved {
            self.end_still(p.id, t);
            let b = self.bots.get_mut(&p.id).unwrap();
            b.anchor = Some(pos);
            b.still_since = t;
            b.still_combat = 0.0;
            b.still_wants = 0.0;
            b.still_ordered = 0.0;
            return;
        }
        if combat {
            b.still_combat += step;
        }
        if hooks::wants_to_move(b.plan.as_deref()) {
            b.still_wants += step;
        }
        if p.waits.is_some() {
            b.still_ordered += step;
            b.waits_why = p.waits;
        }
    }

    fn end_circle(&mut self, id: usize, t: f64) {
        let b = self.bots.get_mut(&id).unwrap();
        let Some((t0, pos, combat)) = b.circle.take() else { return };
        b.trail.clear();
        if b.human {
            return;
        }
        let dur = t - t0;
        let team = b.team;
        let detail = format!("plan {} in {} ({combat:.0} s fighting)", b.plan.as_deref().unwrap_or("?"), b.ride.as_ref().map_or("foot", |r| r.0.as_str()));
        self.events.push(Event { t: t0, bot: Some(id), team, kind: "circling", pos: whole(pos), dur, detail });
        *self.n.entry("circling_s").or_default() += dur;
        *self.n.entry("circling_quiet_s").or_default() += (dur - combat).max(0.0);
    }

    fn watch_circle(&mut self, p: &Sample, pos: [f32; 3], t: f64, step: f64, combat: bool) {
        let b = self.bots.get_mut(&p.id).unwrap();
        b.trail.push_back(pos);
        while b.trail.len() > CIRCLE_WINDOW + 1 {
            b.trail.pop_front();
        }
        let m = p.vehicle.is_some() as usize;
        let path: f32 = b.trail.iter().zip(b.trail.iter().skip(1)).map(|(a, c)| flat(*a, *c)).sum();
        let far = b.trail.iter().map(|a| flat(*a, pos)).fold(0.0, f32::max);
        let round = b.trail.len() > CIRCLE_WINDOW && path >= CIRCLE_PATH[m] && far < CIRCLE_NET[m];
        match (&mut b.circle, round) {
            (Some(c), true) => c.2 += if combat { step } else { 0.0 },
            (None, true) => b.circle = Some((t - CIRCLE_WINDOW as f64, pos, 0.0)),
            (Some(_), false) => {
                let keep = b.trail.clone();
                self.end_circle(p.id, t);
                self.bots.get_mut(&p.id).unwrap().trail = keep;
            }
            (None, false) => {}
        }
    }

    fn watch_ride(&mut self, p: &Sample, pos: [f32; 3], t: f64) {
        let b = self.bots.get_mut(&p.id).unwrap();
        match (&b.ride, &p.vehicle) {
            (None, Some((name, at))) => {
                b.ride = Some((name.clone(), *at, t));
                if !p.fixed && !b.human {
                    *self.n.entry("rides").or_default() += 1.0;
                }
            }
            (Some(r), v) if v.as_ref().is_none_or(|v| v.1 != r.1) => {
                let (name, _, since) = b.ride.take().unwrap();
                if let Some((n, at)) = v {
                    b.ride = Some((n.clone(), *at, t));
                }
                if !b.human {
                    let dur = t - since;
                    let short = dur < SHORT_RIDE_S;
                    let team = b.team;
                    self.events.push(Event { t, bot: Some(p.id), team, kind: if short { "short_ride" } else { "left_vehicle" }, pos: whole(pos), dur, detail: name });
                    *self.n.entry(if short { "short_rides" } else { "left_vehicles" }).or_default() += 1.0;
                }
            }
            _ => {}
        }
    }

    fn watch_flag(&mut self, p: &Sample, pos: [f32; 3], t: f64, enemy: Option<f32>, flags: &[Flag]) {
        let b = self.bots.get_mut(&p.id).unwrap();
        if b.human {
            return;
        }
        let near = flags
            .iter()
            .enumerate()
            .filter(|(_, f)| f.takeable && f.team != p.team && (f.only_by == 0 || f.only_by == p.team))
            .map(|(i, f)| (i, flat(f.pos, pos), f.radius))
            .min_by(|a, c| a.1.total_cmp(&c.1));
        let quiet = enemy.is_none_or(|d| d > FLAG_M);
        let watching = near.filter(|n| n.1 < FLAG_M && n.1 > n.2 && quiet && p.vehicle.is_none());
        let Some((i, d, _)) = watching else {
            return self.end_flag(p.id, t);
        };
        match b.flag {
            // getting closer: not ignoring it
            Some((j, _, best)) if j == i && d < best - 2.0 => b.flag = Some((i, t, d)),
            Some((j, ..)) if j == i => {}
            _ => {
                self.end_flag(p.id, t);
                self.bots.get_mut(&p.id).unwrap().flag = Some((i, t, d));
            }
        }
    }

    fn end_flag(&mut self, id: usize, t: f64) {
        let b = self.bots.get_mut(&id).unwrap();
        let Some((i, t0, best)) = b.flag.take() else { return };
        let dur = t - t0;
        if dur >= FLAG_S {
            let (team, pos, plan) = (b.team, b.pos.unwrap_or([0.0; 3]), b.plan.clone());
            self.events.push(Event { t: t0, bot: Some(id), team, kind: "ignored_flag", pos: whole(pos), dur, detail: format!("flag {i} at {best:.0} m, plan {}", plan.as_deref().unwrap_or("?")) });
            *self.n.entry("ignored_flag_s").or_default() += dur;
        }
    }

    /// A shot of a player's, as it leaves.
    pub fn shot(&mut self, t: f64, id: usize, weapon: &str, muzzle: Option<[f32; 3]>) {
        let Some(b) = self.bots.get_mut(&id) else { return };
        let pos = muzzle.or(b.pos);
        b.last_shot = Some((t, pos));
        if b.human {
            return;
        }
        b.shots += 1;
        b.shots_this_life += 1;
        b.first_shot.get_or_insert(t);
        self.first_shot.get_or_insert(t);
        *self.n.entry("shots").or_default() += 1.0;
        let d = pos.and_then(|q| Self::nearest_enemy(&self.live, b.team, q));
        if let Some(d) = d {
            self.dists.push(d);
        }
        // (the far guns, and what a plane drops, are not "at nothing" so easily)
        let w = weapon.to_ascii_lowercase();
        let far_weapon = ["artillery", "wespe", "priest", "rocket", "bomb", "katyusha", "shell"].iter().any(|n| w.contains(n));
        if !far_weapon && d.is_none_or(|d| d > SPRAY_M) {
            *self.n.entry("spray_shots").or_default() += 1.0;
            let at = pos.unwrap_or([0.0; 3]);
            match &mut b.spray {
                Some(s) if t - s.1 < 5.0 => {
                    s.1 = t;
                    s.2 += 1;
                }
                _ => {
                    self.end_spray(id);
                    self.bots.get_mut(&id).unwrap().spray = Some((t, t, 1, at));
                }
            }
        }
    }

    /// What stopped a shot of a player's, and where.
    pub fn stopped(&mut self, t: f64, id: usize, what: Stopped, point: Option<[f32; 3]>) {
        let Some(b) = self.bots.get_mut(&id) else { return };
        if b.human {
            return;
        }
        self.first_hit = self.first_hit.or((what == Stopped::Target).then_some(t));
        let key: &'static str = match what {
            Stopped::Target => "stopped_target",
            Stopped::Other => "stopped_other",
            Stopped::Static => "stopped_static",
            Stopped::Ground => "stopped_ground",
        };
        *self.n.entry(key).or_default() += 1.0;
        let muzzle = b.last_shot.and_then(|s| s.1);
        if matches!(what, Stopped::Static | Stopped::Ground)
            && let (Some(a), Some(c)) = (muzzle, point)
            && ((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2) + (a[2] - c[2]).powi(2)).sqrt() < BLOCKED_M
        {
            *self.n.entry("blocked_shots").or_default() += 1.0;
            match &mut b.blocked {
                Some(s) if t - s.1 < 5.0 => {
                    s.1 = t;
                    s.2 += 1;
                }
                _ => {
                    self.end_blocked(id);
                    self.bots.get_mut(&id).unwrap().blocked = Some((t, t, 1, a));
                }
            }
        }
    }

    /// A damage given to a soldier or a vehicle. `attacker_team` None: nobody's (a crash, burning, the water).
    #[allow(clippy::too_many_arguments)]
    pub fn damage(&mut self, t: f64, victim_team: u32, attacker_team: Option<u32>, amount: f32, soldier: bool, template: &str, at: Option<[f32; 3]>) {
        if amount <= 0.0 {
            return;
        }
        match attacker_team {
            Some(a) if a == victim_team && a != 0 && soldier => {
                *self.n.entry("friendly_damage").or_default() += amount as f64;
                *self.n.entry("friendly_hits").or_default() += 1.0;
                self.events.push(Event { t, bot: None, team: a, kind: "friendly_fire", pos: whole(at.unwrap_or([0.0; 3])), dur: amount as f64, detail: template.to_string() });
            }
            Some(_) => {
                if soldier {
                    *self.n.entry("enemy_damage").or_default() += amount as f64;
                }
            }
            None if !soldier => {
                *self.n.entry("vehicle_self_damage").or_default() += amount as f64;
                *self.n.entry("vehicle_self_hits").or_default() += 1.0;
                self.events.push(Event { t, bot: None, team: victim_team, kind: "vehicle_self_damage", pos: whole(at.unwrap_or([0.0; 3])), dur: amount as f64, detail: template.to_string() });
            }
            None => {}
        }
    }

    /// The report (meta: what round it was).
    pub fn finish(mut self, meta: serde_json::Value) -> serde_json::Value {
        let t = self.now;
        let ids: Vec<usize> = self.bots.keys().copied().collect();
        for &id in &ids {
            self.end_runs(id, t);
        }
        self.events.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.bot.cmp(&b.bot)));
        let judged: Vec<&Bot> = self.bots.values().filter(|b| !b.human && b.lives > 0).collect();
        let alive: f64 = judged.iter().map(|b| b.alive_s).sum::<f64>().max(1.0);
        let free: f64 = judged.iter().map(|b| b.alive_s - b.fixed_s).sum::<f64>().max(1.0);
        let deaths: u32 = judged.iter().map(|b| b.deaths).sum();
        let shots = self.n.get("shots").copied().unwrap_or(0.0);
        let get = |k: &str| self.n.get(k).copied().unwrap_or(0.0);
        let mut totals = BTreeMap::<String, f64>::new();
        let mut put = |k: &str, v: f64| {
            totals.insert(k.to_string(), (v * 100.0).round() / 100.0);
        };
        let pct = |v: f64, of: f64| if of > 0.0 { 100.0 * v / of } else { 0.0 };
        put("bots seen", judged.len() as f64);
        put("alive bot-minutes", alive / 60.0);
        put("mounted %", pct(judged.iter().map(|b| b.mounted_s).sum(), alive));
        put("fixed gun %", pct(judged.iter().map(|b| b.fixed_s).sum(), alive));
        put("still foot %", pct(get("still_foot_s"), free));
        put("still mounted %", pct(get("still_mounted_s"), free));
        put("still ordered %", pct(get("still_ordered_s"), free));
        put("stuck foot %", pct(get("stuck_foot_s"), free));
        put("stuck mounted %", pct(get("stuck_mounted_s"), free));
        put("still quiet %", pct(get("still_quiet_s"), free));
        put("stuck quiet %", pct(get("stuck_quiet_s"), free));
        put("stuck runs per bot-min", get("stuck_runs") / (alive / 60.0));
        put("circling %", pct(get("circling_s"), free));
        put("circling quiet %", pct(get("circling_quiet_s"), free));
        put("shots per bot-min", shots / (alive / 60.0));
        put("spray % of shots", pct(get("spray_shots"), shots));
        put("blocked % of shots", pct(get("blocked_shots"), shots));
        put("stopped target % of shots", pct(get("stopped_target"), shots));
        put("stopped other % of shots", pct(get("stopped_other"), shots));
        put("stopped static % of shots", pct(get("stopped_static"), shots));
        put("stopped ground % of shots", pct(get("stopped_ground"), shots));
        self.dists.sort_by(f32::total_cmp);
        put("median m to enemy at shot", self.dists.get(self.dists.len() / 2).copied().unwrap_or(0.0) as f64);
        put("deaths per bot-min", deaths as f64 / (alive / 60.0));
        put("died without firing % of deaths", pct(judged.iter().map(|b| b.no_shot_deaths).sum::<u32>() as f64, deaths as f64));
        put("bots that fired %", pct(judged.iter().filter(|b| b.first_shot.is_some()).count() as f64, judged.len() as f64));
        let mut firsts: Vec<f64> = judged.iter().filter_map(|b| Some(b.first_shot? - b.first_seen?)).collect();
        firsts.sort_by(f64::total_cmp);
        put("median s to first shot", firsts.get(firsts.len() / 2).copied().unwrap_or(t));
        // (from the first spawn: the server's round starts its bots some 20 s in)
        let from = self.first_alive.unwrap_or(0.0);
        put("round first shot s", self.first_shot.unwrap_or(t) - from);
        put("round first hit s", self.first_hit.unwrap_or(t) - from);
        put("round first death s", self.first_death.unwrap_or(t) - from);
        put("friendly hits per 100 enemy hp", pct(get("friendly_damage"), get("enemy_damage")));
        put("rides per bot-min", get("rides") / (alive / 60.0));
        put("short rides % of rides", pct(get("short_rides"), get("rides")));
        put("left vehicles per bot-min", get("left_vehicles") / (alive / 60.0));
        put("vehicle self damage per bot-min", get("vehicle_self_damage") / (alive / 60.0));
        put("vehicle self hits per bot-min", get("vehicle_self_hits") / (alive / 60.0));
        if self.kills_known {
            let mut k: Vec<f64> = judged.iter().filter_map(|b| Some(b.first_kill? - b.first_seen?)).collect();
            k.sort_by(f64::total_cmp);
            put("median s to first kill", k.get(k.len() / 2).copied().unwrap_or(t));
            put("bots with a kill %", pct(k.len() as f64, judged.len() as f64));
        }
        if self.flags_seen {
            put("ignored flag %", pct(get("ignored_flag_s"), free));
            put("flags taken", get("flags_taken"));
            put("first flag taken s", self.first_capture.unwrap_or(t) - from);
        }
        let bots: Vec<serde_json::Value> = self
            .bots
            .iter()
            .filter(|(_, b)| !b.human)
            .map(|(id, b)| {
                serde_json::json!({"bot": id, "team": b.team, "alive_s": b.alive_s, "fixed_s": b.fixed_s, "mounted_s": b.mounted_s, "lives": b.lives, "deaths": b.deaths, "shots": b.shots, "first_seen": b.first_seen, "first_shot": b.first_shot, "first_kill": b.first_kill, "died_without_firing": b.no_shot_deaths})
            })
            .collect();
        serde_json::json!({"meta": meta, "seconds": t, "totals": totals, "bots": bots, "events": self.events})
    }
}

/// One life of a person standing still among the bots: how long, from when it appeared, until they see it, aim at it, fire
/// at it, hurt it and kill it (`BOTS_HUMAN` in `examples/bots`; a person in the round of the original game).
#[derive(Debug, Clone, Default, Serialize)]
pub struct HumanLife {
    pub spawn: f64,
    pub pos: [i32; 3],
    /// from `spawn`: an enemy within 60 m, one with it in its list of spotted enemies, one with it as its shooting target,
    /// the first shot by one with it as its target, the first damage, the death
    pub enemy_near: Option<f64>,
    pub spotted: Option<f64>,
    pub targeted: Option<f64>,
    pub shot_at: Option<f64>,
    pub damaged: Option<f64>,
    pub died: Option<f64>,
    /// the nearest enemy when it was first spotted, and when it was first fired at
    pub spotted_m: Option<f32>,
    pub shot_m: Option<f32>,
    pub shots_at: u32,
    pub damage: f32,
    pub lasted: f64,
}

/// See [`HumanLife`].
#[derive(Debug, Clone, Default)]
pub struct HumanLog {
    pub lives: Vec<HumanLife>,
    alive: bool,
    nearest: Option<f32>,
}

impl HumanLog {
    /// A moment: its soldier alive at `pos`, the enemies alive, and how many of them have it spotted, and as a target.
    pub fn sample(&mut self, t: f64, pos: Option<[f32; 3]>, enemies: &[[f32; 3]], spotted: usize, targeted: usize) {
        let Some(pos) = pos else {
            if self.alive {
                self.alive = false;
                if let Some(l) = self.lives.last_mut() {
                    l.died = Some(t - l.spawn);
                    l.lasted = t - l.spawn;
                }
            }
            return;
        };
        if !self.alive {
            self.alive = true;
            self.lives.push(HumanLife { spawn: t, pos: whole(pos), ..Default::default() });
        }
        let near = enemies.iter().map(|e| flat(*e, pos)).min_by(f32::total_cmp);
        self.nearest = near;
        let l = self.lives.last_mut().unwrap();
        l.lasted = t - l.spawn;
        let at = t - l.spawn;
        if near.is_some_and(|d| d < COMBAT_M) {
            l.enemy_near.get_or_insert(at);
        }
        if spotted > 0 && l.spotted.is_none() {
            l.spotted = Some(at);
            l.spotted_m = near;
        }
        if targeted > 0 {
            l.targeted.get_or_insert(at);
        }
    }

    /// A shot by an enemy with it as its target.
    pub fn shot_at(&mut self, t: f64) {
        let near = self.nearest;
        if let Some(l) = self.lives.last_mut().filter(|_| self.alive) {
            l.shots_at += 1;
            if l.shot_at.is_none() {
                l.shot_at = Some(t - l.spawn);
                l.shot_m = near;
            }
        }
    }

    pub fn damaged(&mut self, t: f64, amount: f32) {
        if let Some(l) = self.lives.last_mut().filter(|_| self.alive && amount > 0.0) {
            l.damage += amount;
            l.damaged.get_or_insert(t - l.spawn);
        }
    }
}

/// The bench's side: what a [`Detector`] needs from a [`crate::Sim`], every frame (`BOTS_DUMB` in `examples/bots`, and
/// whatever else plays rounds).
#[derive(Debug, Default)]
pub struct Watcher {
    pub detector: Detector,
    fired_seen: usize,
    damage_seen: usize,
    flying: Vec<(usize, usize, usize)>,
    plans: BTreeMap<usize, String>,
    pub human: HumanLog,
    /// the bots at each level of detail, added up per second (with a person in the round)
    pub lod_seconds: [u64; 3],
}

impl Watcher {
    pub fn new() -> Watcher {
        Watcher::default()
    }

    fn player_of(s: &crate::Sim, v: usize) -> Option<usize> {
        s.players.iter().position(|p| p.soldier == Some(v)).or_else(|| s.players.iter().position(|p| p.alive && p.soldier.is_some_and(|i| s.vehicles[i].entry.inside == Some(v))))
    }

    /// After each `Sim::step`.
    pub fn step(&mut self, s: &crate::Sim) {
        let t = s.clock.steps as f64 / 30.0;
        if s.clock.steps % 30 == 0 {
            self.sample(s, t);
        }
        for f in s.journal.fired.get(self.fired_seen..).unwrap_or(&[]) {
            // (a projectile fired and gone in the same step, or of a round that ended, is no longer in the list)
            let seat = f.3.and_then(|i| s.projectiles.get(i)).map(|p| p.seat);
            let inside = |p: &crate::players::Player| p.alive && p.soldier.is_some_and(|i| s.vehicles[i].entry.inside == Some(f.1) && seat.is_none_or(|k| s.vehicles[i].entry.position == k));
            let Some(k) = s.players.iter().position(|p| p.soldier == Some(f.1)).or_else(|| s.players.iter().position(inside)) else { continue };
            let w = f.3.and_then(|i| s.projectiles.get(i)).map_or(String::new(), |p| p.template.name.clone());
            self.detector.shot(f.0 as f64 / 30.0, k, &w, Some(f.2.pos));
            if let Some(h) = Self::human_handle(s)
                && s.players[k].ai
                && hooks::aimed_at(s, k) == Some(h)
            {
                self.human.shot_at(f.0 as f64 / 30.0);
            }
            if let Some(i) = f.3.filter(|&i| i < s.projectiles.len()) {
                self.flying.push((k, i, 0));
            }
        }
        self.fired_seen = s.journal.fired.len();
        let mut flying = std::mem::take(&mut self.flying);
        flying.retain_mut(|(k, i, seen)| {
            // (a shot of a round that ended: its projectiles are gone)
            let Some(p) = s.projectiles.get(*i) else { return false };
            // (where it ended, as what the original's reference run says: one for each shot)
            for imp in p.impacts.get(*seen..).unwrap_or(&[]).iter().filter(|i| i.ended) {
                let target = hooks::aimed_at(s, *k);
                let what = match (imp.vehicle, &imp.object) {
                    (Some(v), _) if Some(hooks::handle(s, v)) == target => Stopped::Target,
                    (Some(_), _) => Stopped::Other,
                    (None, Some(_)) => Stopped::Static,
                    (None, None) => Stopped::Ground,
                };
                self.detector.stopped(imp.step as f64 / 30.0, *k, what, Some(imp.point));
            }
            *seen = p.impacts.len();
            p.end.is_none()
        });
        self.flying = flying;
        for e in s.journal.damage_log.get(self.damage_seen..).unwrap_or(&[]) {
            if e.status.is_none() || e.amount <= 0.0 {
                continue;
            }
            let v = &s.vehicles[e.vehicle];
            if Self::human_handle(s) == Some(hooks::handle(s, e.vehicle)) {
                self.human.damaged(e.step as f64 / 30.0, e.amount);
            }
            let soldier = v.soldier().is_some();
            let team = if soldier { s.players.iter().find(|p| p.soldier == Some(e.vehicle)).map_or(v.team, |p| p.team) } else { v.team };
            use crate::damage::Source;
            if matches!(e.source, Source::Heal | Source::Repair | Source::Kill) {
                continue;
            }
            let attacker = e.by.and_then(|b| Self::player_of(s, b)).map(|k| s.players[k].team);
            self.detector.damage(e.step as f64 / 30.0, team, attacker, e.amount, soldier, &v.template, Some(v.craft.body.pos));
        }
        self.damage_seen = s.journal.damage_log.len();
    }

    /// The first person's soldier, alive.
    pub(crate) fn human_handle(s: &crate::Sim) -> Option<u32> {
        let p = s.players.iter().find(|p| !p.ai)?;
        p.soldier.filter(|_| p.alive).map(|v| hooks::handle(s, v))
    }

    /// The soldier `i` presses forward or back: on its own legs, or on the throttle of what it drives (a passenger presses nothing).
    fn pushes_on(s: &crate::Sim, i: usize) -> bool {
        const AXIS: usize = crate::soldier::axis::THROTTLE;
        match s.vehicles[i].entry.inside {
            Some(v) => s.vehicles[v].entry.driver == Some(i) && !s.vehicles[v].fixed && s.vehicles[v].craft.inputs.get(AXIS).is_some_and(|x| x.abs() > 0.1),
            None => s.vehicles[i].soldier().is_some_and(|m| m.input.get(AXIS).abs() > 0.1),
        }
    }

    fn sample(&mut self, s: &crate::Sim, t: f64) {
        hooks::sample_human(self, s, t);
        let samples: Vec<Sample> = s
            .players
            .iter()
            .enumerate()
            .map(|(k, p)| {
                let soldier = p.soldier.filter(|_| p.alive);
                let inside = soldier.and_then(|i| s.vehicles[i].entry.inside);
                // a mind with no plan to show (bots): what it does with the controls says whether it means to go somewhere
                let plan = hooks::plan_of(s, k).or_else(|| (p.ai && soldier.is_some()).then(|| if soldier.is_some_and(|i| Self::pushes_on(s, i)) { "go" } else { "idle" }.to_string()));
                if let Some(n) = &plan {
                    self.plans.insert(k, n.clone());
                }
                Sample {
                    id: k,
                    team: p.team,
                    alive: p.alive && soldier.is_some(),
                    pos: soldier.map(|i| s.vehicles[i].craft.body.pos),
                    vehicle: inside.map(|v| (s.vehicles[v].template.clone(), v as u64)),
                    fixed: inside.is_some_and(|v| s.vehicles[v].fixed),
                    plan: self.plans.get(&k).cloned(),
                    human: !p.ai,
                    kills: Some(p.kills),
                    waits: s.drivers.minds.intent(k).waits.filter(|_| p.alive).map(|w| w.name()),
                }
            })
            .collect();
        let flags: Option<Vec<Flag>> = s.modes.conquest.as_ref().map(|c| c.points.iter().map(|p| Flag { pos: p.pos, radius: p.template.radius, team: p.team, takeable: !p.template.unable_to_change_team, only_by: p.template.only_takeable_by }).collect());
        self.detector.sample(t, &samples, flags.as_deref());
    }

    pub fn finish(self, meta: serde_json::Value) -> serde_json::Value {
        let mut r = self.detector.finish(meta);
        r["human"] = serde_json::to_value(&self.human.lives).unwrap();
        r["lod_seconds"] = serde_json::to_value(self.lod_seconds).unwrap();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bot(id: usize, team: u32, pos: [f32; 3], plan: &str) -> Sample {
        Sample { id, team, alive: true, pos: Some(pos), plan: Some(plan.into()), ..Default::default() }
    }

    fn total(r: &serde_json::Value, k: &str) -> f64 {
        r["totals"][k].as_f64().unwrap_or_else(|| panic!("{k}"))
    }

    fn kinds(r: &serde_json::Value, k: &str) -> usize {
        r["events"].as_array().unwrap().iter().filter(|e| e["kind"] == k).count()
    }

    #[test]
    fn a_bot_that_wants_to_move_and_does_not_is_stuck() {
        let mut d = Detector::new();
        for t in 0..40 {
            d.sample(t as f64, &[bot(0, 1, [0.0, 0.0, 0.0], "go"), bot(1, 2, [500.0, 0.0, t as f32 * 3.0], "go")], None);
        }
        let r = d.finish(serde_json::json!({}));
        assert_eq!(kinds(&r, "stuck"), 1);
        assert_eq!(r["events"][0]["bot"], 0);
        assert!(total(&r, "stuck foot %") > 40.0);
        assert_eq!(total(&r, "still foot %"), 0.0);
    }

    /// M2: standing still while its mind says it waits on purpose is counted apart; without that, the same run is stuck.
    #[test]
    fn a_wait_on_purpose_is_not_stuck() {
        let run = |waits: Option<&'static str>| {
            let mut d = Detector::new();
            for t in 0..40 {
                let me = Sample { waits, ..bot(0, 1, [0.0, 0.0, 0.0], "go") };
                d.sample(t as f64, &[me, bot(1, 2, [500.0, 0.0, t as f32 * 3.0], "go")], None);
            }
            d.finish(serde_json::json!({}))
        };
        let r = run(Some("rally"));
        assert_eq!((kinds(&r, "still_ordered"), kinds(&r, "stuck"), kinds(&r, "still")), (1, 0, 0));
        assert!(r["events"][0]["detail"].as_str().unwrap().starts_with("waits rally"));
        assert!(total(&r, "still ordered %") > 40.0);
        assert_eq!((total(&r, "stuck foot %"), total(&r, "still foot %")), (0.0, 0.0));
        let r = run(None);
        assert_eq!((kinds(&r, "still_ordered"), kinds(&r, "stuck")), (0, 1));
        assert_eq!(total(&r, "still ordered %"), 0.0);
    }

    #[test]
    fn a_vehicle_stuck_and_then_left_counts_as_mounted() {
        let mut d = Detector::new();
        for t in 0..60 {
            // (0 waits on foot for 20 s, gets in, sits stuck 30 s and gets out beside it)
            let mut me = bot(0, 1, [if t < 50 { 0.0 } else { 3.0 }, 0.0, 0.0], "go");
            me.vehicle = (20..50).contains(&t).then(|| ("t-3b".to_string(), 7));
            d.sample(t as f64, &[me, bot(1, 2, [500.0, 0.0, t as f32 * 3.0], "go")], None);
        }
        let r = d.finish(serde_json::json!({}));
        let stuck: Vec<_> = r["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "stuck").map(|e| (e["dur"].as_f64().unwrap(), e["detail"].as_str().unwrap().to_string())).collect();
        assert_eq!(stuck, [(20.0, "plan go in foot (0 s fighting)".to_string()), (30.0, "plan go in t-3b (0 s fighting)".to_string())]);
    }

    #[test]
    fn going_round_in_circles_is_seen_and_walking_is_not() {
        let mut d = Detector::new();
        for t in 0..60 {
            let a = t as f32 * 0.7;
            d.sample(t as f64, &[bot(0, 1, [2.0 * a.cos(), 0.0, 2.0 * a.sin()], "look"), bot(1, 2, [500.0, 0.0, t as f32 * 3.0], "go")], None);
        }
        let r = d.finish(serde_json::json!({}));
        let c: Vec<_> = r["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "circling").collect();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["bot"], 0);
    }

    #[test]
    fn a_flag_ignored_is_seen_and_one_being_walked_to_is_not() {
        let flags = [Flag { pos: [100.0, 0.0, 0.0], radius: 10.0, team: 2, takeable: true, only_by: 0 }];
        let mut d = Detector::new();
        for t in 0..40 {
            // (0 stands 30 m from it; 1 walks to it)
            d.sample(t as f64, &[bot(0, 1, [70.0, 0.0, 0.0], "idle"), bot(1, 1, [100.0, 0.0, 45.0 - t as f32], "go"), bot(2, 2, [1000.0, 0.0, 0.0], "idle")], Some(&flags));
        }
        let r = d.finish(serde_json::json!({}));
        let c: Vec<_> = r["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "ignored_flag").collect();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["bot"], 0);
    }

    #[test]
    fn shots_at_nothing_into_a_wall_and_friends() {
        let mut d = Detector::new();
        d.sample(1.0, &[bot(0, 1, [0.0, 0.0, 0.0], "fight"), bot(1, 2, [400.0, 0.0, 0.0], "idle")], None);
        for k in 0..4 {
            d.shot(1.0 + k as f64 * 0.1, 0, "k98", Some([0.0, 1.0, 0.0]));
            d.stopped(1.0 + k as f64 * 0.1, 0, Stopped::Static, Some([3.0, 1.0, 0.0]));
        }
        d.damage(1.5, 1, Some(1), 10.0, true, "GermanSoldier", None);
        d.damage(1.6, 2, Some(1), 40.0, true, "USSoldier", None);
        d.damage(1.7, 2, None, 5.0, false, "Willy", None);
        let r = d.finish(serde_json::json!({}));
        assert_eq!(total(&r, "spray % of shots"), 100.0);
        assert_eq!(total(&r, "blocked % of shots"), 100.0);
        assert_eq!(total(&r, "friendly hits per 100 enemy hp"), 25.0);
        assert_eq!(kinds(&r, "friendly_fire"), 1);
        assert_eq!(kinds(&r, "vehicle_self_damage"), 1);
        assert_eq!(kinds(&r, "spray"), 1);
        assert_eq!(kinds(&r, "blocked"), 1);
    }

    #[test]
    fn a_person_seen_fired_at_and_killed() {
        let mut h = HumanLog::default();
        let at = Some([0.0, 0.0, 0.0]);
        h.sample(10.0, at, &[[500.0, 0.0, 0.0]], 0, 0);
        h.sample(20.0, at, &[[50.0, 0.0, 0.0]], 1, 0);
        h.sample(21.0, at, &[[50.0, 0.0, 0.0]], 2, 1);
        h.shot_at(21.5);
        h.damaged(22.0, 10.0);
        h.sample(23.0, None, &[], 0, 0);
        h.sample(40.0, at, &[], 0, 0);
        assert_eq!(h.lives.len(), 2);
        let l = &h.lives[0];
        assert_eq!((l.enemy_near, l.spotted, l.spotted_m, l.targeted, l.shot_at, l.damaged, l.died), (Some(10.0), Some(10.0), Some(50.0), Some(11.0), Some(11.5), Some(12.0), Some(13.0)));
    }

    #[test]
    fn a_fixed_gunner_is_neither_still_nor_stuck() {
        let mut d = Detector::new();
        for t in 0..40 {
            let mut g = bot(0, 1, [0.0, 0.0, 0.0], "idle");
            g.vehicle = Some(("mg".into(), 7));
            g.fixed = true;
            d.sample(t as f64, &[g], None);
        }
        let r = d.finish(serde_json::json!({}));
        assert_eq!(kinds(&r, "still"), 0);
        assert_eq!(total(&r, "fixed gun %"), 100.0);
    }

    #[test]
    fn a_death_without_firing_and_the_first_shot() {
        let mut d = Detector::new();
        let mut a = bot(0, 1, [0.0, 0.0, 0.0], "idle");
        d.sample(1.0, &[a.clone()], None);
        a.alive = false;
        a.pos = None;
        d.sample(2.0, &[a.clone()], None);
        let r = d.finish(serde_json::json!({}));
        assert_eq!(total(&r, "died without firing % of deaths"), 100.0);
        assert_eq!(total(&r, "round first death s"), 1.0);
        assert_eq!(total(&r, "bots that fired %"), 0.0);
    }
}
