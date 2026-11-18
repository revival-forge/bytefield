//! Two aircraft already in flight near the middle of the map, one flown by a mind and the other by a mind or by a script, and what
//! came of it, measured from outside (`Sim::projectiles`, `journal.damage_log`, `vehicles`): the bench for air-to-air fighting.
//!   air_duel <game> <mod> <level> --a <template> --b <template> [--mind-a bots3[:variant]] [--mind-b bots3|drone:straight|drone:circle:<radius>|drone:weave|drone:climb]
//!            [--seeds 8] [--seconds 120] [--range 600] [--aspect tail|head|beam|rear] [--height 600] [--drone-speed 110] [--at x,z] [--out file.jsonl]
//!            [--gunner-a <seat> [--pilot-a drone:straight|drone:circle:<radius>|drone:weave|drone:climb]]
//!   air_duel <game> <mod> <level> --list-seats <template>[,<template>...]   (the seats of each aircraft and the weapons of each seat; no duel)
//! `--gunner-a <seat>`: A's mind (`--mind-a`) sits in that seat of A, not in the pilot's; the pilot's seat is flown by a script (`--pilot-a`, level by default)
//! like a drone, so what is measured is the gunner's. The line then ends with `gun_shots gun_hits gun_bearing_s` (see below), and `shots_a`/`hits_a` and `behind_s`
//! are still counted for the whole aircraft.
//! Both start at 110 m/s (a drone then keeps `--drone-speed`), `--height` m over the ground at the middle of the map, `--range` m apart: `tail` (A behind B, same heading),
//! `head` (nose to nose) or `beam` (B crossing in front of A at 90 degrees). The seed moves B's start (100 m to the side, 50 m in
//! height) and is the minds' seed: the same seed is the same duel. A drone is flown from outside with a player's stick (level, in a
//! circle, weaving with 60 degrees of bank every 4 s, or climbing at 20 degrees) and never fires.
//! `rear` (for a gunner): B behind A, same heading. `--at x,z` is where A starts (B goes where the aspect says from there; the default is the middle of the map, as above). Without it, a `tail`
//! duel against `drone:straight` starts far enough back for the drone to have `--seconds` of map ahead at `--drone-speed` (the line `# start` says
//! where, and how many seconds the map gives when it is not enough); every other duel starts at the middle.
//! A duel ends at `--seconds`, when one aircraft is destroyed, or when one leaves the map (`out`).
//! One line per duel: `seed winner first_hit_s kill_s shots_a hits_a behind_s bearing_s min_range crash_a crash_b collided out end_s`,
//! and a summary per (a, b, mind).
//!   winner:     a, b or none: who downed the other: destroyed, any way, within 20 s of a hit of this one (as `B26_air_downed`), or its
//!               pilot jumped out within those 20 s; a crash or a jump with no such hit is `crash_*`
//!   behind_s:   seconds with A within 600 m of B, inside B's rear cone of 60 degrees (30 degrees each side of its tail) and with B within
//!               30 degrees of A's nose
//!   bearing_s:  seconds with A's nose within 1 degree of where `dogfight::lead` says to shoot (the real velocities, the muzzle speed of the
//!               key-8 gun at seat 0) and B within 450 m
//!   gun_shots, gun_hits: (`--gunner-a`) projectiles fired by the gunner's seat, and those that hit B
//!   gun_bearing_s: (`--gunner-a`) seconds with the seat's camera within 1 degree of where `dogfight::lead` says to shoot from the camera (A's velocity
//!               counted, the muzzle speed of the seat's gun) and B within 450 m
use bytefield_sim::bot_api::MindMaker;
use bytefield_sim::bots3::body::dogfight::lead;
use bytefield_sim::damage::{Source, Status};
use bytefield_sim::soldier::axis;

type V3 = [f32; 3];
const PITCH: usize = 1;
const BANK: usize = 2;
const DT: f32 = 1.0 / 30.0;
const SPEED: f32 = 110.0;
/// An aircraft destroyed or given up within this long of the other's hit was downed by it, s (`quality::B26_DOWNED_S`).
const DOWNED_S: f32 = bytefield_sim::quality::B26_DOWNED_S as f32;

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn angle(a: V3, b: V3) -> f32 {
    (dot(a, b) / (len(a) * len(b)).max(1e-6)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// What a drone does with its stick.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Script {
    Straight,
    Circle(f32),
    Weave,
    Climb,
}

impl Script {
    fn parse(d: &str) -> Script {
        let mut it = d.split(':');
        match it.next() {
            Some("straight") => Script::Straight,
            Some("circle") => Script::Circle(it.next().and_then(|r| r.parse().ok()).expect("drone:circle:<radius>")),
            Some("weave") => Script::Weave,
            Some("climb") => Script::Climb,
            _ => panic!("no drone called {d}"),
        }
    }
}

/// The stick of a player who flies a script: the same law `air_envelope` uses to hold an attitude (a pitch and a bank, the stick's reach
/// less the faster it goes), with the height and the speed kept by the pitch and the throttle.
struct Drone {
    script: Script,
    speed: f32,
    target_y: f32,
    before: [V3; 3],
    rates: V3,
}

impl Drone {
    fn axes(&mut self, rows: [V3; 3], vel: V3, y: f32, t: f32) -> Vec<(usize, f32)> {
        let [r, u, f] = rows;
        let [r0, u0, _] = self.before;
        self.rates = [dot(f, u0), dot(f, r0), -dot(r, u0)].map(|x| x.clamp(-1.0, 1.0).asin().to_degrees() / DT);
        self.before = rows;
        let speed = len(vel);
        let (pitch, bank) = (f[1].clamp(-1.0, 1.0).asin().to_degrees(), -r[1].atan2(u[1]).to_degrees());
        let level = ((self.target_y - y) * 0.4 - 0.5 * vel[1]).clamp(-12.0, 30.0);
        let (want_pitch, want_bank) = match self.script {
            Script::Straight => (level, 0.0),
            // a level turn: tan(bank) = v^2 / (g r)
            Script::Circle(radius) => (level, (speed * speed / (9.81 * radius.max(50.0))).atan().to_degrees().min(75.0)),
            Script::Weave => (level, if (t / 4.0) as u32 % 2 == 0 { 60.0 } else { -60.0 }),
            Script::Climb => (20.0, 0.0),
        };
        let k = (45.0 / speed.max(20.0)).powi(2).clamp(0.3, 3.0);
        let p = (k * (0.05 * (want_pitch - pitch) - 0.02 * self.rates[0])).clamp(-1.0, 1.0);
        let b = (k * (0.02 * (want_bank - bank) - 0.006 * self.rates[2])).clamp(-1.0, 1.0);
        let throttle = (0.6 + 0.05 * (self.speed - speed)).clamp(0.0, 1.0);
        vec![(axis::THROTTLE, throttle), (PITCH, -p), (BANK, b)]
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Pilot {
    Mind(String),
    Drone(Script),
}

impl Pilot {
    fn parse(s: &str) -> Pilot {
        if let Some(d) = s.strip_prefix("drone:") {
            return Pilot::Drone(Script::parse(d));
        }
        Pilot::Mind(s.to_string())
    }
    fn name(&self) -> String {
        match self {
            Pilot::Mind(m) => m.clone(),
            Pilot::Drone(Script::Straight) => "drone:straight".into(),
            Pilot::Drone(Script::Circle(r)) => format!("drone:circle:{r}"),
            Pilot::Drone(Script::Weave) => "drone:weave".into(),
            Pilot::Drone(Script::Climb) => "drone:climb".into(),
        }
    }
}

fn maker(name: &str, level: &str, seed: u64) -> MindMaker {
    match name {
        "bots3" => bytefield_sim::bots3::maker(level, seed),
        v if v.starts_with("bots3:") => bytefield_sim::bots3::maker_variant(level, seed, &v["bots3:".len()..].split('+').collect::<Vec<_>>()),
        other => panic!("no mind called {other}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Aspect {
    Tail,
    Head,
    Beam,
    /// B behind A, same heading (for a gunner that shoots backwards)
    Rear,
}

struct Config {
    a: String,
    b: String,
    pilot_a: Pilot,
    pilot_b: Pilot,
    seconds: f32,
    range: f32,
    aspect: Aspect,
    height: f32,
    drone_speed: f32,
    /// metres B starts above A (the arcs of a turret that cannot depress)
    above: f32,
    at: Option<(f32, f32)>,
    /// the seat of A that the mind takes, and the script that flies A's pilot seat then
    gunner: Option<usize>,
    script_a: Script,
}

#[derive(Default, Debug, Clone)]
struct Duel {
    seed: u64,
    winner: &'static str,
    first_hit_s: Option<f32>,
    kill_s: Option<f32>,
    shots_a: u32,
    hits_a: u32,
    behind_s: f32,
    bearing_s: f32,
    gun_shots: u32,
    gun_hits: u32,
    gun_bearing_s: f32,
    min_range: f32,
    crash_a: bool,
    crash_b: bool,
    collided: bool,
    out: &'static str,
    end_s: f32,
}

/// A small deterministic generator for the start's jitter.
fn jitter(seed: u64, k: u64) -> f32 {
    let mut z = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(k.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0
}

/// Axes of a body that looks along `yaw` (0: +z, 90 degrees: +x), level.
fn rows_of(yaw: f32) -> [V3; 3] {
    let (s, c) = yaw.to_radians().sin_cos();
    [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]]
}

fn duel(w: &bytefield_sim::World, cfg: &Config, seed: u64) -> Duel {
    let level = w.level.name.clone();
    let mut s = bytefield_sim::Sim::new(w, DT as f64);
    s.round.playing = true;
    s.start_conquest();
    s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    if let Pilot::Mind(m) = &cfg.pilot_a {
        s.set_minds(1, maker(m, &level, seed));
    }
    if let Pilot::Mind(m) = &cfg.pilot_b {
        s.set_minds(2, maker(m, &level, seed));
    }
    let (ka, kb) = (s.join(1, true), s.join(2, true));
    // with a gunner, a third player takes the seat and the first one is the scripted pilot
    let kg = cfg.gunner.map(|_| s.join(1, true));
    // the soldiers come out of their spawns; then they are put in their aircraft
    let mut soldiers = None;
    let alive = |s: &bytefield_sim::Sim, k: usize| s.players[k].soldier.filter(|_| s.players[k].alive);
    for _ in 0..900 {
        s.step();
        if let (Some(a), Some(b)) = (alive(&s, ka), alive(&s, kb))
            && kg.is_none_or(|k| alive(&s, k).is_some())
        {
            soldiers = Some((a, b));
            break;
        }
    }
    let (sa, sb) = soldiers.expect("the soldiers never spawned");
    let sg = kg.and_then(|k| alive(&s, k));
    if cfg.gunner.is_some() {
        // the mind of team 1 must not fly the scripted pilot: it only drives players that are computer's
        s.players[ka].ai = false;
    }
    // where A starts when nothing is said (the middle of the map), and the shift `--at` makes of both
    let nominal_a = (1024.0, 1024.0 - match cfg.aspect { Aspect::Head => cfg.range / 2.0, Aspect::Rear => 0.0, _ => cfg.range });
    let (sx, sz) = cfg.at.map_or((0.0, 0.0), |(x, z)| (x - nominal_a.0, z - nominal_a.1));
    let ground = w.height_at((1024.0 + sx) as f64, (1024.0 + sz) as f64).unwrap_or(0.0) as f32;
    let (dx, dy) = (100.0 * jitter(seed, 1), 50.0 * jitter(seed, 2));
    let y = ground + cfg.height;
    // B at the middle (moved by the seed); A where the aspect says
    let (pos_b, yaw_b, pos_a, yaw_a) = match cfg.aspect {
        Aspect::Tail => ([1024.0 + dx, y + dy, 1024.0], 0.0, [1024.0 + dx, y + dy, 1024.0 - cfg.range], 0.0),
        Aspect::Head => ([1024.0 + dx, y + dy, 1024.0 + cfg.range / 2.0], 180.0, [1024.0 + dx, y + dy, 1024.0 - cfg.range / 2.0], 0.0),
        Aspect::Beam => ([1024.0 + dx, y + dy, 1024.0], 90.0, [1024.0 + dx, y + dy, 1024.0 - cfg.range], 0.0),
        Aspect::Rear => ([1024.0 + dx, y + dy, 1024.0 - cfg.range], 0.0, [1024.0 + dx, y + dy, 1024.0], 0.0),
    };
    let (pos_a, pos_b) = ([pos_a[0] + sx, pos_a[1], pos_a[2] + sz], [pos_b[0] + sx, pos_b[1] + cfg.above, pos_b[2] + sz]);
    let ca = s.spawn_vehicle(&cfg.a, pos_a, [0.0; 3]).unwrap_or_else(|| panic!("no aircraft {}", cfg.a));
    let cb = s.spawn_vehicle(&cfg.b, pos_b, [0.0; 3]).unwrap_or_else(|| panic!("no aircraft {}", cfg.b));
    for (c, rows, pos) in [(ca, rows_of(yaw_a), pos_a), (cb, rows_of(yaw_b), pos_b)] {
        let b = &mut s.vehicles[c].craft.body;
        b.pos = pos;
        b.rows = rows;
        b.vel = rows[2].map(|x| x * SPEED);
        b.rotvel = [0.0; 3];
    }
    s.enter(sa, ca, 0);
    s.enter(sb, cb, 0);
    if let (Some(g), Some(seat)) = (sg, cfg.gunner) {
        s.enter(g, ca, seat);
    }
    let mut drone = match cfg.pilot_b {
        Pilot::Drone(script) => Some(Drone { script, speed: cfg.drone_speed, target_y: y + dy + cfg.above, before: rows_of(yaw_b), rates: [0.0; 3] }),
        Pilot::Mind(_) => None,
    };
    // A's pilot seat flown by a script when a gunner has the mind
    let mut drone_a = cfg.gunner.map(|_| Drone { script: cfg.script_a, speed: cfg.drone_speed, target_y: y + dy, before: rows_of(yaw_a), rates: [0.0; 3] });
    // the muzzle speed of A's gun: the pilot's (key 8, seat 0), or the gunner's seat's (key 8, else any that fires)
    let gseat = cfg.gunner.unwrap_or(0);
    let gun_of = |s: &bytefield_sim::Sim, key: Option<usize>| {
        s.vehicles[ca].weapons.iter().find(|(part, f)| s.vehicles[ca].craft.parts[*part].seat == gseat && key.is_none_or(|k| f.template.input_fire == k) && f.template.velocity > 0.0).map(|(_, f)| f.template.velocity)
    };
    let muzzle = gun_of(&s, Some(axis::FIRE)).unwrap_or(1000.0);
    let gun_muzzle = gun_of(&s, Some(axis::FIRE)).or_else(|| gun_of(&s, None)).unwrap_or(1000.0);
    let mut d = Duel { seed, winner: "none", min_range: f32::MAX, out: "", ..Default::default() };
    let (mut damage_seen, mut fired_seen) = (s.journal.damage_log.len(), s.journal.fired.len());
    let mut pending: Vec<usize> = Vec::new();
    let mut pending_gun: Vec<usize> = Vec::new();
    // closest approach of each gunner round to B (for the debug trace)
    let mut closest: std::collections::HashMap<usize, f32> = Default::default();
    let mut miss: std::collections::HashMap<usize, (V3, f32, f32)> = Default::default();
    // when each aircraft was last hit by the other (s), for what is a crash and what is not
    let mut hit_by_other = [f32::NEG_INFINITY; 2];
    let t0 = s.clock.steps;
    let steps = (cfg.seconds * 30.0) as u64;
    for _ in 0..steps {
        let t = (s.clock.steps - t0) as f32 * DT;
        if let Some(dr) = drone.as_mut() {
            let b = &s.vehicles[cb].craft.body;
            let axes = dr.axes(b.rows, b.vel, b.pos[1], t);
            s.vehicles[sb].movement.as_mut().unwrap().set_input(&axes);
        }
        if let Some(dr) = drone_a.as_mut() {
            let b = &s.vehicles[ca].craft.body;
            let axes = dr.axes(b.rows, b.vel, b.pos[1], t);
            s.vehicles[sa].movement.as_mut().unwrap().set_input(&axes);
        }
        s.step();
        let t = (s.clock.steps - t0) as f32 * DT;
        d.end_s = t;
        let (pa, pb) = (s.vehicles[ca].craft.body.pos, s.vehicles[cb].craft.body.pos);
        let range = len(sub(pb, pa));
        d.min_range = d.min_range.min(range);
        // the shots
        for f in s.journal.fired.get(fired_seen..).unwrap_or(&[]) {
            if f.1 == ca {
                d.shots_a += 1;
                if f.3.is_some_and(|k| cfg.gunner.is_some() && s.projectiles.get(k).is_some_and(|p| p.seat == gseat)) {
                    d.gun_shots += 1;
                    pending_gun.extend(f.3);
                }
                pending.extend(f.3);
            }
        }
        fired_seen = s.journal.fired.len();
        pending.retain(|&k| {
            let Some(p) = s.projectiles.get(k) else { return false };
            if p.impacts.iter().any(|i| i.vehicle == Some(cb)) {
                d.hits_a += 1;
                return false;
            }
            p.end.is_none()
        });
        pending_gun.retain(|&k| {
            let Some(p) = s.projectiles.get(k) else { return false };
            let near = closest.entry(k).or_insert(f32::MAX);
            // the closest approach of the step's segment, in B's frame (its own speed taken out)
            let vb = s.vehicles[cb].craft.body.vel;
            let rel = sub(p.vel, vb);
            let (a, b) = (sub(sub(p.pos, [rel[0] * DT, rel[1] * DT, rel[2] * DT]), pb), sub(p.pos, pb));
            let ab = sub(b, a);
            let l2 = dot(ab, ab);
            let u = if l2 > 0.0 { (-dot(a, ab) / l2).clamp(0.0, 1.0) } else { 1.0 };
            let q = [a[0] + ab[0] * u, a[1] + ab[1] * u, a[2] + ab[2] * u];
            if len(q) < *near {
                *near = len(q);
                let r = s.vehicles[cb].craft.body.rows;
                miss.insert(k, ([dot(q, r[0]), dot(q, r[1]), dot(q, r[2])], len(p.vel), len(rel)));
            }
            if p.end.is_some() && std::env::var_os("AIR_DUEL_DEBUG_GUNNER").is_some() {
                eprintln!("    gunner round {} ended {:?}, closest to B {:.1} m (in B's frame {:?}, speed {:.0}, relative {:.0}), {:.0} m from its start, age {:.2}, ttl {:.2}, impacts {:?}", p.template.name, p.end, near, miss.get(&k).map(|m| m.0), miss.get(&k).map_or(0.0, |m| m.1), miss.get(&k).map_or(0.0, |m| m.2), len(sub(p.pos, p.start)), p.age, p.ttl, p.impacts.iter().map(|i| (i.object.clone(), i.vehicle)).collect::<Vec<_>>());
            }
            if p.impacts.iter().any(|i| i.vehicle == Some(cb)) {
                d.gun_hits += 1;
                return false;
            }
            p.end.is_none()
        });
        // the damage
        let mut over = false;
        for e in s.journal.damage_log.get(damage_seen..).unwrap_or(&[]) {
            let (me, other, slot) = if e.vehicle == ca {
                (ca, cb, 0)
            } else if e.vehicle == cb {
                (cb, ca, 1)
            } else {
                continue;
            };
            let _ = me;
            if std::env::var_os("AIR_DUEL_DEBUG").is_some() {
                eprintln!("t {t:.2} vehicle {} amount {:.1} source {:?} status {:?} by {:?} range {range:.0}", e.vehicle, e.amount, e.source, e.status, e.by);
                for p in s.projectiles.iter().filter(|p| p.owner == Some(ca) && p.template.name != "30mm_Projectile") {
                    eprintln!("    {} age {:.2} ttl {:.2} end {:?} at {:.0} m from a, {:.0} m from its start; impacts {:?}", p.template.name, p.age, p.ttl, p.end, len(sub(p.pos, pa)), len(sub(p.pos, p.start)), p.impacts.iter().map(|i| (i.object.clone(), i.vehicle)).collect::<Vec<_>>());
                }
            }
            if e.by == Some(other) && e.amount > 0.0 {
                hit_by_other[slot] = t;
                if slot == 1 && d.first_hit_s.is_none() {
                    d.first_hit_s = Some(t);
                }
            }
            if e.source == Source::Crash && range < 30.0 {
                d.collided = true;
            }
            if e.status == Some(Status::Destroyed) {
                over = true;
                let by_other = e.by == Some(other) || t - hit_by_other[slot] <= DOWNED_S;
                if by_other {
                    if slot == 1 {
                        d.winner = "a";
                        d.kill_s = Some(t);
                    } else if d.winner == "none" {
                        d.winner = "b";
                    }
                } else if e.source == Source::Crash {
                    if slot == 0 {
                        d.crash_a = true;
                    } else {
                        d.crash_b = true;
                    }
                }
            }
        }
        damage_seen = s.journal.damage_log.len();
        // a pilot that jumps out gives its aircraft up: downed, if the other hit it a moment ago
        for (slot, soldier, craft) in [(0, sa, ca), (1, sb, cb)] {
            if !over && s.vehicles[soldier].entry.inside != Some(craft) {
                over = true;
                if t - hit_by_other[slot] <= DOWNED_S {
                    d.winner = if slot == 1 { "a" } else { "b" };
                    if slot == 1 {
                        d.kill_s = Some(t);
                    }
                } else if slot == 0 {
                    d.crash_a = true;
                } else {
                    d.crash_b = true;
                }
            }
        }
        // where they are
        let (ba, bb) = (&s.vehicles[ca].craft.body, &s.vehicles[cb].craft.body);
        let (fa, fb) = (ba.rows[2], bb.rows[2]);
        let to_b = sub(pb, pa);
        let behind = range < 600.0 && angle(sub(pa, pb), fb.map(|x| -x)) < 30.0 && angle(to_b, fa) < 30.0;
        d.behind_s += behind as u32 as f32 * DT;
        if range < 450.0
            && let Some((at, _)) = lead(pa, ba.vel, pb, bb.vel, muzzle, 0.0)
        {
            d.bearing_s += (angle(fa, sub(at, pa)) < 1.0) as u32 as f32 * DT;
        }
        // AIR_DUEL_DEBUG_GUNNER: once a second, whether the gunner is in its seat, where its camera points against the lead and what its keys say
        if let (Some(g), Some(_)) = (sg, cfg.gunner)
            && std::env::var_os("AIR_DUEL_DEBUG_GUNNER").is_some()
            && (s.clock.steps - t0) % 30 == 0
        {
            let car = &s.vehicles[ca].craft;
            let frames = car.frames_still_cameras();
            let cam = car.parts.iter().position(|q| matches!(q.kind, bytefield_sim::craft::Kind::Camera) && q.seat == gseat);
            let off = cam.and_then(|c| lead(frames[c].pos, ba.vel, pb, bb.vel, gun_muzzle, 0.0).map(|(at, _)| angle(frames[c].child_rows[2], sub(at, frames[c].pos))));
            let axes = s.vehicles[g].movement.as_ref().map(|m| (m.input.axes[axis::FIRE], m.input.axes[axis::ALT_FIRE]));
            // where the camera points and where the target is, in the aircraft's frame (azimuth from the nose, elevation)
            let local = |v: V3| {
                let r = ba.rows;
                let l = [dot(v, r[0]), dot(v, r[1]), dot(v, r[2])];
                (l[0].atan2(l[2]).to_degrees(), l[1].atan2((l[0] * l[0] + l[2] * l[2]).sqrt()).to_degrees())
            };
            if let Some(c) = cam {
                if std::env::var_os("AIR_DUEL_DEBUG_GUNNER").is_some() && s.clock.steps - t0 < 40 {
                    for q in car.parts.iter().filter(|q| q.seat == gseat) {
                        if let Some(b) = &q.bundle {
                            eprintln!("    seat part {} {:?}: min {:?} max {:?} speed {:?} accel {:?}", q.name, q.kind, b.min_rotation, b.max_rotation, b.max_speed, b.acceleration);
                        }
                    }
                }
                eprintln!("    camera az/el {:.0?} target az/el {:.0?}", local(frames[c].child_rows[2]), local(sub(pb, frames[c].pos)));
            }
            eprintln!("t {t:.0} gunner inside {:?} seat {} camera {} off_lead {off:?} range {range:.0} fire/alt {axes:?} alive {}", s.vehicles[g].entry.inside, s.vehicles[g].entry.position, cam.is_some(), s.players[kg.unwrap()].alive);
        }
        // the gunner's aim: its seat's camera against where the lead says to shoot from there
        if cfg.gunner.is_some() && range < 450.0 {
            let car = &s.vehicles[ca].craft;
            let frames = car.frames_still_cameras();
            if let Some(c) = car.parts.iter().position(|q| matches!(q.kind, bytefield_sim::craft::Kind::Camera) && q.seat == gseat)
                && let Some((at, _)) = lead(frames[c].pos, ba.vel, pb, bb.vel, gun_muzzle, 0.0)
            {
                d.gun_bearing_s += (angle(frames[c].child_rows[2], sub(at, frames[c].pos)) < 1.0) as u32 as f32 * DT;
            }
        }
        // out of the map
        for (c, name) in [(ca, "a"), (cb, "b")] {
            let p = s.vehicles[c].craft.body.pos;
            if w.height_at(p[0] as f64, p[2] as f64).is_none() && d.out.is_empty() {
                d.out = if name == "a" { "a" } else { "b" };
                over = true;
            }
        }
        if over {
            break;
        }
    }
    d
}

fn line(d: &Duel, gunner: bool) -> String {
    let f = |x: Option<f32>| x.map_or("-".to_string(), |x| format!("{x:.1}"));
    let extra = if gunner { format!(" gun_shots {} gun_hits {} gun_bearing_s {:.1}", d.gun_shots, d.gun_hits, d.gun_bearing_s) } else { String::new() };
    format!(
        "seed {} winner {} first_hit_s {} kill_s {} shots_a {} hits_a {} behind_s {:.1} bearing_s {:.1} min_range {:.0} crash_a {} crash_b {} collided {} out {} end_s {:.1}{}",
        d.seed,
        d.winner,
        f(d.first_hit_s),
        f(d.kill_s),
        d.shots_a,
        d.hits_a,
        d.behind_s,
        d.bearing_s,
        d.min_range,
        d.crash_a as u8,
        d.crash_b as u8,
        d.collided as u8,
        if d.out.is_empty() { "-" } else { d.out },
        d.end_s,
        extra
    )
}

fn json(cfg: &Config, d: &Duel) -> serde_json::Value {
    serde_json::json!({
        "a": cfg.a, "b": cfg.b, "mind_a": cfg.pilot_a.name(), "mind_b": cfg.pilot_b.name(), "aspect": format!("{:?}", cfg.aspect), "seed": d.seed, "winner": d.winner,
        "first_hit_s": d.first_hit_s, "kill_s": d.kill_s, "shots_a": d.shots_a, "hits_a": d.hits_a, "behind_s": d.behind_s, "bearing_s": d.bearing_s,
        "min_range": d.min_range, "crash_a": d.crash_a, "crash_b": d.crash_b, "collided": d.collided, "out": d.out, "end_s": d.end_s,
        "gunner_seat": cfg.gunner, "gun_shots": d.gun_shots, "gun_hits": d.gun_hits, "gun_bearing_s": d.gun_bearing_s,
    })
}

/// The seats of each template and the weapons of each seat, from a round started on the level.
fn list_seats(a: &[String], templates: &str) {
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, DT as f64);
    s.round.playing = true;
    s.start_conquest();
    for t in templates.split(',') {
        let Some(c) = s.spawn_vehicle(t, [1024.0, 800.0, 1024.0], [0.0; 3]) else {
            println!("{t}: no such template");
            continue;
        };
        let v = &s.vehicles[c];
        println!("{t}: {} seats", v.entry.seats.len());
        for seat in 0..v.entry.seats.len() {
            let guns: Vec<String> = v
                .weapons
                .iter()
                .filter(|(part, _)| v.craft.parts[*part].seat == seat)
                .map(|(_, f)| format!("{} (key {}, muzzle {:.0} m/s, projectile {}, rounds {})", f.template.name, f.template.input_fire, f.template.velocity, f.template.projectile, f.template.mag_size))
                .collect();
            println!("  seat {seat}: {}", if guns.is_empty() { "no weapon".to_string() } else { guns.join("; ") });
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("air_duel <game> <mod> <level> --a <template> --b <template> [--mind-a bots3[:variant]] [--mind-b bots3|drone:straight|drone:circle:<radius>|drone:weave|drone:climb] [--seeds 8] [--seconds 120] [--range 600] [--aspect tail|head|beam|rear] [--height 600] [--drone-speed 110] [--out file.jsonl]");
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    if let Some(list) = opt("--list-seats") {
        list_seats(&a, &list);
        return;
    }
    let need = |k: &str| opt(k).unwrap_or_else(|| panic!("{k} is needed"));
    let cfg = Config {
        a: need("--a"),
        b: need("--b"),
        pilot_a: Pilot::parse(&opt("--mind-a").unwrap_or("bots3".into())),
        pilot_b: Pilot::parse(&opt("--mind-b").unwrap_or("bots3".into())),
        seconds: opt("--seconds").map_or(120.0, |x| x.parse().unwrap()),
        range: opt("--range").map_or(600.0, |x| x.parse().unwrap()),
        aspect: match opt("--aspect").as_deref().unwrap_or("tail") {
            "tail" => Aspect::Tail,
            "head" => Aspect::Head,
            "beam" => Aspect::Beam,
            "rear" => Aspect::Rear,
            other => panic!("no aspect {other}"),
        },
        height: opt("--height").map_or(600.0, |x| x.parse().unwrap()),
        above: opt("--above").map_or(0.0, |x| x.parse().unwrap()),
        drone_speed: opt("--drone-speed").map_or(SPEED, |x| x.parse().unwrap()),
        at: opt("--at").map(|v| {
            let (x, z) = v.split_once(',').expect("--at x,z");
            (x.parse().expect("--at x,z"), z.parse().expect("--at x,z"))
        }),
        gunner: opt("--gunner-a").map(|x| x.parse().expect("--gunner-a <seat>")),
        script_a: Script::parse(opt("--pilot-a").as_deref().map_or("straight", |x| x.strip_prefix("drone:").unwrap_or(x))),
    };
    assert!(matches!(cfg.pilot_a, Pilot::Mind(_)), "A is always a mind");
    let seeds: u64 = opt("--seeds").map_or(8, |x| x.parse().unwrap());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mut cfg = cfg;
    let straight_ahead = (cfg.aspect == Aspect::Tail && cfg.pilot_b == Pilot::Drone(Script::Straight)) || (cfg.aspect == Aspect::Rear && cfg.gunner.is_some() && cfg.script_a == Script::Straight);
    if cfg.at.is_none() && straight_ahead {
        // a drone that flies straight (+z) needs `--seconds` of map ahead: find the map's ends along the line x = 1024
        let inside = |z: f32| w.height_at(1024.0, z as f64).is_some();
        let low = (0..4096).map(|i| i as f32 * 4.0).find(|&z| inside(z));
        let high = low.and_then(|l| (0..4096).map(|i| l + i as f32 * 4.0).take_while(|&z| inside(z)).last());
        if let (Some(low), Some(high)) = (low, high) {
            let margin = 100.0;
            let reach = cfg.drone_speed * cfg.seconds;
            let want = (high - margin - reach).min(1024.0);
            let b_z = want.max(low + margin + cfg.range);
            if b_z < 1024.0 {
                // (the aircraft in front is B for a tail duel and A for a rear one)
                cfg.at = Some((1024.0, if cfg.aspect == Aspect::Rear { b_z } else { b_z - cfg.range }));
                let given = (high - margin - b_z) / cfg.drone_speed;
                if given < cfg.seconds {
                    println!("# start A at 1024,{:.0}: the map gives the drone {:.0} s of the {:.0} s asked at {} m/s", cfg.at.unwrap().1, given, cfg.seconds, cfg.drone_speed);
                } else {
                    println!("# start A at 1024,{:.0}: the drone has {:.0} s of map ahead", cfg.at.unwrap().1, given);
                }
            }
        }
    }
    let mut out = opt("--out").map(|p| std::fs::File::create(p).expect("--out"));
    let mut all = Vec::new();
    for seed in 1..=seeds {
        let d = duel(&w, &cfg, seed);
        println!("{}", line(&d, cfg.gunner.is_some()));
        if let Some(f) = out.as_mut() {
            use std::io::Write;
            writeln!(f, "{}", json(&cfg, &d)).unwrap();
        }
        all.push(d);
    }
    let n = all.len().max(1) as f32;
    let pct = |k: usize| 100.0 * k as f32 / n;
    let kills = all.iter().filter(|d| d.winner == "a").count();
    let mut kt: Vec<f32> = all.iter().filter_map(|d| d.kill_s).collect();
    kt.sort_by(f32::total_cmp);
    let (shots, hits): (u32, u32) = (all.iter().map(|d| d.shots_a).sum(), all.iter().map(|d| d.hits_a).sum());
    let time: f32 = all.iter().map(|d| d.end_s).sum::<f32>().max(1e-3);
    println!(
        "summary {} ({}) vs {} ({}), {:?}, {} duels: kills {:.0} %, median kill_s {}, losses {:.0} %, hits {:.1} % of {} shots, behind {:.1} % of the time, nose on {:.1} %, crashed {:.0} % (b {:.0} %), out {:.0} %",
        cfg.a,
        cfg.pilot_a.name(),
        cfg.b,
        cfg.pilot_b.name(),
        cfg.aspect,
        all.len(),
        pct(kills),
        kt.get(kt.len() / 2).map_or("-".to_string(), |x| format!("{x:.1}")),
        pct(all.iter().filter(|d| d.winner == "b").count()),
        if shots > 0 { 100.0 * hits as f32 / shots as f32 } else { 0.0 },
        shots,
        100.0 * all.iter().map(|d| d.behind_s).sum::<f32>() / time,
        100.0 * all.iter().map(|d| d.bearing_s).sum::<f32>() / time,
        pct(all.iter().filter(|d| d.crash_a).count()),
        pct(all.iter().filter(|d| d.crash_b).count()),
        pct(all.iter().filter(|d| !d.out.is_empty()).count()),
    );
}
