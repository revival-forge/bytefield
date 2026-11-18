//! What a kind of bot knows how to do, seen from outside: no scores, only «can it».
//!   bot_caps <game> <mod> <level> seat [--mind bots2] [--seconds 60] [--mode conquest]
//!   bot_caps <game> <mod> <level> round <players> <minutes> [--mind …] [--mode conquest] [--seed N]
//! `seat`, the synthetic one: for each template with seats, a new round with a bot put in the driver's seat of one of them and an
//! enemy on foot 60 m ahead. `round`, the real one: a whole round, what the bots did by itself, by template.
//! A line of JSON for each template, and one for what was done on foot, with `caps`: mines, charges, parachutes, landing, ladders
//! (counted from objects and effects: `Sim::fired` does not see shots a kind of bot only simulates). The tests need `--example bot_caps`.
use bytefield_sim::damage::Source;
use bytefield_sim::mover::Vehicle;
use bytefield_sim::projectile::End;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

fn class(v: &Vehicle) -> &'static str {
    bytefield_sim::bot_api::VehicleClass::of(v).name()
}

fn set_mind(s: &mut bytefield_sim::Sim, name: &str, level: &str, seed: u64) {
    s.enable_bots();
    match name {
        "bots2" => {
            for t in [1, 2] {
                s.set_minds(t, bytefield_sim::bots2::maker(level, seed));
            }
        }
        "bots3" => {
            for t in [1, 2] {
                s.set_minds(t, bytefield_sim::bots3::maker(level, seed));
            }
        }
        other => panic!("no mind called {other}"),
    }
}

/// What one vehicle template saw.
#[derive(Default, Clone)]
struct Tally {
    class: &'static str,
    boardings: u32,
    driver_s: f64,
    gunner_s: f64,
    gunner_shots: u32,
    gunner_kills: u32,
    path: f64,
    top_speed: f32,
    top_above_ground: f32,
    below_water: f32,
    shots: u32,
    kills: u32,
    damage: f32,
    bots: std::collections::BTreeSet<usize>,
}

impl Tally {
    fn line(&self, level: &str, mind: &str, template: &str) -> serde_json::Value {
        json!({"level": level, "mind": mind, "template": template, "class": self.class, "boardings": self.boardings, "bots": self.bots.len(),
            "driver_s": self.driver_s.round(), "gunner_s": self.gunner_s.round(), "gunner_shots": self.gunner_shots, "gunner_kills": self.gunner_kills, "path_m": self.path.round(), "top_speed": self.top_speed.round(),
            "top_above_ground": self.top_above_ground.round(), "below_water": self.below_water.round(), "shots": self.shots, "kills": self.kills, "damage": self.damage.round()})
    }
}

const MINE: usize = 0;
const CHARGE: usize = 1;
/// After leaving a boat: how long until we look at whether it went on on foot (ticks), and how far it must have gone (m).
const LANDED_TICKS: u64 = 30 * 30;
const LANDED_WALK: f32 = 15.0;

/// What is not tied to a vehicle's seat. `[MINE, CHARGE]` where it has two.
#[derive(Default, Debug)]
struct Caps {
    laid: [u32; 2],
    off: [u32; 2],
    vehicles_damaged: [u32; 2],
    vehicles_destroyed: [u32; 2],
    damage: [f32; 2],
    air_exits: u32,
    falls: u32,
    jumps: u32,
    chutes_open: u32,
    fell_alive: u32,
    fell_dead: u32,
    boat_exits: u32,
    near_shore: u32,
    went_on: u32,
    climbs: u32,
    climb_m: f32,
}

impl Caps {
    fn json(&self) -> serde_json::Value {
        json!({"mines_laid": self.laid[MINE], "mines_off": self.off[MINE], "mines_vehicles_damaged": self.vehicles_damaged[MINE], "mines_vehicles_destroyed": self.vehicles_destroyed[MINE],
            "c4_laid": self.laid[CHARGE], "c4_off": self.off[CHARGE], "c4_vehicles_damaged": self.vehicles_damaged[CHARGE], "c4_vehicles_destroyed": self.vehicles_destroyed[CHARGE], "c4_damage": self.damage[CHARGE].round(),
            "air_exits": self.air_exits, "falls": self.falls, "jumps": self.jumps, "chutes_open": self.chutes_open, "fell_alive": self.fell_alive, "fell_dead": self.fell_dead,
            "boat_exits": self.boat_exits, "near_shore": self.near_shore, "went_on": self.went_on, "climbs": self.climbs, "climb_m": self.climb_m.round()})
    }
}

fn explosive_kind(name: &str) -> Option<usize> {
    let n = name.to_ascii_lowercase();
    if n.contains("landmine") {
        Some(MINE)
    } else if n.contains("exppack") {
        Some(CHARGE)
    } else {
        None
    }
}

/// Dry ground here, or within 25 m.
fn near_shore(s: &bytefield_sim::Sim, pos: [f32; 3]) -> bool {
    let water = s.world.level.terrain.water_level as f32;
    let dry = |x: f32, z: f32| s.world.height_at(x as f64, z as f64).is_some_and(|h| h as f32 > water - 1.0);
    dry(pos[0], pos[2]) || [10.0f32, 25.0].iter().any(|r| (0..8).any(|k| {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        dry(pos[0] + r * a.cos(), pos[2] + r * a.sin())
    }))
}

/// A charge or mine that went off: until its damage is in the log (it is given the frame after).
struct Blast {
    step: u64,
    owner: Option<usize>,
    pos: [f32; 3],
    radius: f32,
    kind: usize,
}

/// A bot that left a boat near the shore.
struct Landing {
    player: usize,
    step: u64,
    deaths: u32,
    from: [f32; 3],
}

/// Watches who sits in what, once a tick.
#[derive(Default)]
struct Census {
    caps: Caps,
    projectiles_seen: usize,
    armed: Vec<(usize, usize)>,
    blasts: Vec<Blast>,
    falling: std::collections::BTreeSet<usize>,
    chute_counted: std::collections::BTreeSet<usize>,
    left_in_air: std::collections::BTreeSet<usize>,
    climbing: BTreeMap<usize, f32>,
    landings: Vec<Landing>,
    by: BTreeMap<String, Tally>,
    was_in: BTreeMap<usize, usize>,
    last_pos: BTreeMap<usize, [f32; 3]>,
    fired_seen: usize,
    damage_seen: usize,
    kills_seen: BTreeMap<usize, u32>,
}

impl Census {
    fn step(&mut self, s: &bytefield_sim::Sim) {
        let dt = s.clock.dt as f64;
        let water = s.world.level.terrain.water_level as f32;
        for (k, p) in s.players.iter().enumerate() {
            if !p.ai {
                continue;
            }
            let inside = p.soldier.filter(|_| p.alive).and_then(|i| s.vehicles[i].entry.inside.map(|c| (i, c)));
            let Some((i, c)) = inside else {
                self.left_vehicle(s, k);
                self.kills_seen.insert(k, p.kills);
                self.on_foot(s, k);
                continue;
            };
            self.falling.remove(&k);
            self.chute_counted.remove(&k);
            self.left_in_air.remove(&k);
            self.climbing.remove(&k);
            let v = &s.vehicles[c];
            let t = self.by.entry(v.template.clone()).or_default();
            t.class = class(v);
            if self.was_in.insert(k, c) != Some(c) {
                t.boardings += 1;
                t.bots.insert(k);
            }
            let body = &v.craft.body;
            if s.vehicles[i].entry.position == 0 {
                t.driver_s += dt;
                if let Some(last) = self.last_pos.get(&c) {
                    t.path += (0..3).map(|j| (body.pos[j] - last[j]).powi(2)).sum::<f32>().sqrt() as f64;
                }
                t.top_speed = t.top_speed.max(body.vel.iter().map(|x| x * x).sum::<f32>().sqrt());
                if let Some(h) = s.world.height_at(body.pos[0] as f64, body.pos[2] as f64) {
                    t.top_above_ground = t.top_above_ground.max(body.pos[1] - (h as f32).max(water));
                }
                t.below_water = t.below_water.max(water - body.pos[1]);
            } else {
                t.gunner_s += dt;
            }
            let had = self.kills_seen.insert(k, p.kills).unwrap_or(p.kills);
            t.kills += p.kills.saturating_sub(had);
            if s.vehicles[i].entry.position != 0 {
                t.gunner_kills += p.kills.saturating_sub(had);
            }
        }
        self.last_pos = s.vehicles.iter().enumerate().filter(|(_, v)| !v.is_soldier() && !v.removed && v.entry.occupants.iter().flatten().next().is_some()).map(|(j, v)| (j, v.craft.body.pos)).collect();
        for f in s.journal.fired.get(self.fired_seen..).unwrap_or(&[]) {
            let v = &s.vehicles[f.1];
            if !v.is_soldier() && let Some(t) = self.by.get_mut(&v.template) {
                t.shots += 1;
                if f.3.is_some_and(|k| s.projectiles[k].seat != 0) {
                    t.gunner_shots += 1;
                }
            }
        }
        self.fired_seen = s.journal.fired.len();
        self.explosives(s);
        for e in s.journal.damage_log.get(self.damage_seen..).unwrap_or(&[]) {
            if let Some(t) = e.by.and_then(|b| s.vehicles.get(b)).filter(|v| !v.is_soldier()).and_then(|v| self.by.get_mut(&v.template)) {
                t.damage += e.amount.max(0.0);
            }
            if e.source == Source::Explosion && e.amount > 0.0 {
                let at = s.vehicles[e.vehicle].craft.body.pos;
                let by_blast = self.blasts.iter().find(|b| {
                    let d = (0..3).map(|j| (at[j] - b.pos[j]).powi(2)).sum::<f32>().sqrt();
                    b.owner == e.by && e.step.abs_diff(b.step) <= 2 && d <= b.radius * 1.2
                });
                if let Some(kind) = by_blast.map(|b| b.kind) {
                    self.caps.damage[kind] += e.amount;
                    if !s.vehicles[e.vehicle].is_soldier() {
                        self.caps.vehicles_damaged[kind] += 1;
                        if e.status == Some(bytefield_sim::damage::Status::Destroyed) {
                            self.caps.vehicles_destroyed[kind] += 1;
                        }
                    }
                }
            }
        }
        self.damage_seen = s.journal.damage_log.len();
        let now = s.clock.steps;
        self.blasts.retain(|b| now.saturating_sub(b.step) <= 2);
        self.look_at_landings(s);
    }

    /// The mines and charges: by the projectiles themselves.
    fn explosives(&mut self, s: &bytefield_sim::Sim) {
        if s.projectiles.len() < self.projectiles_seen {
            self.projectiles_seen = 0;
            self.armed.clear();
        }
        for (k, p) in s.projectiles.iter().enumerate().skip(self.projectiles_seen) {
            if let Some(kind) = explosive_kind(&p.template.name) {
                self.caps.laid[kind] += 1;
                self.armed.push((k, kind));
            }
        }
        self.projectiles_seen = s.projectiles.len();
        let (caps, blasts) = (&mut self.caps, &mut self.blasts);
        self.armed.retain(|&(k, kind)| {
            let p = &s.projectiles[k];
            let off = matches!(p.end, Some(End::Detonated | End::Hit { detonated: true } | End::Expired { detonated: true }));
            if off {
                caps.off[kind] += 1;
                blasts.push(Blast { step: s.clock.steps, owner: p.owner, pos: p.pos, radius: p.template.damage.radius, kind });
            }
            p.end.is_none()
        });
    }

    /// Player `k` is not in a vehicle (any more): if it just left a boat or something in the air.
    fn left_vehicle(&mut self, s: &bytefield_sim::Sim, k: usize) {
        let Some(c) = self.was_in.remove(&k) else { return };
        let p = &s.players[k];
        let Some(i) = p.soldier.filter(|_| p.alive) else { return };
        let pos = s.vehicles[i].craft.body.pos;
        match class(&s.vehicles[c]) {
            "ship" => {
                self.caps.boat_exits += 1;
                if near_shore(s, pos) {
                    self.caps.near_shore += 1;
                    self.landings.push(Landing { player: k, step: s.clock.steps, deaths: p.deaths, from: pos });
                }
            }
            "plane" | "helicopter" | "airship" => {
                let h = s.world.height_at(pos[0] as f64, pos[2] as f64).map_or(0.0, |h| h as f32);
                if pos[1] - h > 10.0 {
                    self.caps.air_exits += 1;
                    self.left_in_air.insert(k);
                }
            }
            _ => {}
        }
    }

    /// Falling and climbing, for a player out of any vehicle.
    fn on_foot(&mut self, s: &bytefield_sim::Sim, k: usize) {
        let p = &s.players[k];
        let soldier = p.soldier.filter(|_| p.alive).map(|i| &s.vehicles[i]);
        let m = soldier.and_then(|v| v.movement.as_ref());
        if m.is_some_and(|m| m.parachuting()) {
            if self.falling.insert(k) {
                self.caps.falls += 1;
                // (else it was born falling, on a paratroopers' spawn point, or fell off something)
                if self.left_in_air.contains(&k) {
                    self.caps.jumps += 1;
                }
            }
            if m.is_some_and(|m| m.parachute) && self.chute_counted.insert(k) {
                self.caps.chutes_open += 1;
            }
        } else if self.falling.remove(&k) {
            self.chute_counted.remove(&k);
            self.left_in_air.remove(&k);
            if soldier.is_some() {
                self.caps.fell_alive += 1;
            } else {
                self.caps.fell_dead += 1;
            }
        }
        let y = soldier.map(|v| v.craft.body.pos[1]);
        if m.is_some_and(|m| m.climbing()) {
            if !self.climbing.contains_key(&k) {
                self.caps.climbs += 1;
                self.climbing.insert(k, y.unwrap());
            }
            // the height gained so far: a climb still going at the end of the round counts too
            if let Some(y0) = self.climbing.get_mut(&k) {
                let y = y.unwrap();
                if y > *y0 {
                    self.caps.climb_m += y - *y0;
                    *y0 = y;
                }
            }
        } else {
            self.climbing.remove(&k);
        }
    }

    /// Those who left a boat near the shore: alive a while later, in no vehicle, further on and on dry ground.
    fn look_at_landings(&mut self, s: &bytefield_sim::Sim) {
        let water = s.world.level.terrain.water_level as f32;
        let (caps, now) = (&mut self.caps, s.clock.steps);
        self.landings.retain(|l| {
            if now < l.step + LANDED_TICKS {
                return true;
            }
            let p = &s.players[l.player];
            let on_foot = p.soldier.filter(|_| p.alive && p.deaths == l.deaths).map(|i| &s.vehicles[i]).filter(|v| v.entry.inside.is_none());
            if let Some(v) = on_foot {
                let at = v.craft.body.pos;
                let far = ((at[0] - l.from[0]).powi(2) + (at[2] - l.from[2]).powi(2)).sqrt() >= LANDED_WALK;
                let dry = s.world.height_at(at[0] as f64, at[2] as f64).is_some_and(|h| h as f32 > water);
                if far && dry {
                    caps.went_on += 1;
                }
            }
            false
        });
    }
}

/// What soldiers did on foot in a round: shots by weapon name, and what was done to others.
fn on_foot(s: &bytefield_sim::Sim, level: &str, mind: &str, watched: &Census) -> serde_json::Value {
    let mut by_weapon: BTreeMap<String, u32> = BTreeMap::new();
    for f in &s.journal.fired {
        if s.vehicles[f.1].is_soldier() {
            let w = f.3.map_or("(no projectile)".to_string(), |i| s.projectiles[i].template.name.clone());
            *by_weapon.entry(w).or_default() += 1;
        }
    }
    let (mut heal, mut repair) = (0.0f32, 0.0f32);
    for e in &s.journal.damage_log {
        match e.source {
            Source::Heal => heal += e.amount.abs(),
            Source::Repair => repair += e.amount.abs(),
            _ => {}
        }
    }
    json!({"level": level, "mind": mind, "foot": true, "caps": watched.caps.json(), "shots_by_projectile": by_weapon, "healed": heal.round(), "repaired": repair.round(),
        "captures": s.players.iter().map(|p| p.captures).sum::<u32>(), "kills": s.players.iter().map(|p| p.kills).sum::<u32>(), "deaths": s.players.iter().map(|p| p.deaths).sum::<u32>(), "tks": s.players.iter().map(|p| p.tks).sum::<u32>()})
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("bot_caps <game> <mod> <level> seat|round [...]; see the head of the file");
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let mode = opt("--mode").unwrap_or("conquest");
    let mind = opt("--mind").unwrap_or("bots2");
    let seed: u64 = opt("--seed").map_or(0, |n| n.parse().unwrap());
    let level = &a[3];
    let w = bytefield_sim::World::load(Path::new(&a[1]), &a[2], level, mode).expect("world");
    let new_sim = || {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s
    };
    if a[4] == "round" {
        let (players, minutes): (usize, f64) = (a[5].parse().unwrap(), a[6].parse().unwrap());
        let mut s = new_sim();
        set_mind(&mut s, mind, level, seed);
        if seed != 0 {
            s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        }
        for i in 0..players {
            let p = s.join(0, true);
            s.players[p].kit = (i / 2) % 5;
        }
        let mut c = Census::default();
        for _ in 0..(minutes * 60.0 * 30.0) as u64 {
            s.step();
            c.step(&s);
            if s.round.round_end.is_some() || s.modes.conquest.as_ref().is_some_and(|c| c.winner.is_some()) {
                break;
            }
        }
        for (t, y) in &c.by {
            println!("{}", y.line(level, mind, t));
        }
        println!("{}", on_foot(&s, level, mind, &c));
        return;
    }
    // seat: which templates are there, once the spawners have made them
    let seconds: f64 = opt("--seconds").map_or(60.0, |n| n.parse().unwrap());
    let mut first = new_sim();
    for _ in 0..150 {
        first.step();
    }
    let mut templates: Vec<String> = first.vehicles.iter().filter(|v| v.in_world() && !v.is_soldier() && !v.entry.seats.is_empty()).map(|v| v.template.clone()).collect();
    templates.sort();
    templates.dedup();
    drop(first);
    for template in templates {
        let mut s = new_sim();
        set_mind(&mut s, mind, level, seed);
        let (p, q) = (s.join(1, true), s.join(2, true));
        // (they must be born, and the vehicle there)
        for _ in 0..150 {
            s.step();
        }
        let Some(k) = s.vehicles.iter().position(|v| v.in_world() && v.template == template && v.entry.occupants.iter().all(|o| o.is_none())) else { continue };
        let Some(i) = s.players[p].soldier.filter(|_| s.players[p].alive) else {
            println!("{}", json!({"level": level, "mind": mind, "template": template, "error": "not born"}));
            continue;
        };
        s.enter(i, k, 0);
        let start = s.vehicles[k].craft.body.pos;
        let rows = s.vehicles[k].craft.body.rows;
        // the enemy, in front
        let target = [start[0] + rows[2][0] * 60.0, start[1], start[2] + rows[2][2] * 60.0];
        let target_y = s.world.height_at(target[0] as f64, target[2] as f64).map_or(start[1], |h| h as f32 + 1.0);
        let mut c = Census::default();
        let mut kept = 0.0;
        for _ in 0..(seconds * 30.0) as u64 {
            if let Some(e) = s.players[q].soldier.filter(|_| s.players[q].alive) {
                s.vehicles[e].craft.body.pos = [target[0], target_y, target[2]];
                s.vehicles[e].craft.body.vel = [0.0; 3];
            }
            s.step();
            c.step(&s);
            if s.players[p].soldier.is_some_and(|i| s.vehicles[i].entry.inside == Some(k)) {
                kept += s.clock.dt as f64;
            }
        }
        let end = s.vehicles[k].craft.body.pos;
        let net = (0..3).map(|j| (end[j] - start[j]).powi(2)).sum::<f32>().sqrt();
        let y = c.by.get(&template).cloned().unwrap_or_default();
        let mut line = y.line(level, mind, &template);
        line["class"] = json!(class(&s.vehicles[k]));
        line["kept_s"] = json!((kept * 10.0_f64).round() / 10.0);
        line["net_m"] = json!(net.round());
        println!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytefield_sim::projectile::Shot;

    fn world(level: &str) -> Option<bytefield_sim::World> {
        let game = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/game");
        game.is_dir().then(|| bytefield_sim::World::load(&game, "bf1942", level, "conquest").unwrap())
    }

    fn sim(w: &bytefield_sim::World) -> bytefield_sim::Sim<'_> {
        let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
        s.playing = true;
        s.start_conquest();
        s
    }

    fn shot(pos: [f32; 3]) -> Shot {
        Shot { pos, rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], vel: [0.0; 3], rotvel: [0.0; 3] }
    }

    /// A bot, born.
    fn bot(s: &mut bytefield_sim::Sim) -> (usize, usize) {
        let p = s.join(1, true);
        for _ in 0..600 {
            s.step();
            if s.players[p].alive && s.players[p].soldier.is_some() {
                break;
            }
        }
        (p, s.players[p].soldier.unwrap())
    }

    #[test]
    fn a_mine_under_a_tank_and_charges_beside_one() {
        let Some(w) = world("El_Alamein") else { return };
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.playing = true;
        let ground = |x: f64, z: f64| w.height_at(x, z).unwrap() as f32;
        let y = ground(1500.0, 715.0);
        let tank = s.spawn_vehicle("T34", [1500.0, y + 1.0, 700.0], [0.0; 3]).unwrap();
        let parked = s.spawn_vehicle("T34", [1700.0, ground(1700.0, 703.0) + 1.0, 703.0], [0.0; 3]).unwrap();
        let sapper = s.spawn_vehicle("GermanDesertSoldier", [1700.0, ground(1700.0, 700.0) + 1.0, 700.0], [0.0; 3]).unwrap();
        assert!(s.vehicles[sapper].give_kit(w.kit_weapons("German_Engineer_Desert"), Some("ExpPack")));
        s.vehicles[sapper].movement.as_mut().unwrap().has_player = true;
        let mine = s.add_projectile("LandmineProjectile", &shot([1500.0, y + 0.3, 715.0])).unwrap();
        s.projectiles[mine].owner = Some(sapper);
        let mut c = Census::default();
        let hp = s.vehicles[parked].armor.as_ref().unwrap().hp;
        for n in 0..30 * 12 {
            let b = &mut s.vehicles[tank].craft.body;
            b.pos = [1500.0, y + 1.0, 700.0 + 10.0 * n as f32 / 30.0];
            b.vel = [0.0, 0.0, 10.0];
            // four charges, one press each 45 frames, then the detonator
            let press = n % 45 < 2 && n < 45 * 5;
            s.vehicles[sapper].movement.as_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, if press { 1.0 } else { 0.0 })]);
            s.step();
            c.step(&s);
        }
        assert_eq!((c.caps.laid, c.caps.off), ([1, 4], [1, 4]), "{:?}", c.caps);
        assert!(c.caps.vehicles_damaged[MINE] >= 1, "{:?}", c.caps);
        assert!(s.vehicles[parked].armor.as_ref().unwrap().hp < hp);
        assert!(c.caps.vehicles_damaged[CHARGE] >= 1 && c.caps.damage[CHARGE] > 0.0, "{:?}", c.caps);
    }

    #[test]
    fn a_fall_with_the_parachute_open_is_seen_and_ends() {
        let Some(w) = world("El_Alamein") else { return };
        let mut s = sim(&w);
        let (_, i) = bot(&mut s);
        let at = s.vehicles[i].craft.body.pos;
        let mut c = Census::default();
        s.vehicles[i].craft.body.pos = [at[0], at[1] + 150.0, at[2]];
        s.vehicles[i].craft.body.vel = [0.0, -20.0, 0.0];
        for _ in 0..30 * 40 {
            s.vehicles[i].movement.as_mut().unwrap().set_input(&[]);
            s.step();
            c.step(&s);
            if c.caps.falls > 0 && c.falling.is_empty() {
                break;
            }
        }
        assert_eq!(c.caps.falls, 1, "{:?}", c.caps);
        assert_eq!(c.caps.fell_alive + c.caps.fell_dead, 1, "{:?}", c.caps);
    }

    #[test]
    fn a_climb_counts_once_with_its_height() {
        let Some(w) = world("El_Alamein") else { return };
        let mut s = sim(&w);
        let (p, i) = bot(&mut s);
        let mut c = Census::default();
        let y = s.vehicles[i].craft.body.pos[1];
        let ladder = bytefield_sim::ladder::Ladder { pos: [0.0; 3], rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], bounds: [[0.0; 3], [1.0; 3]], material: 192 };
        // (not by the sim's step: what the census does with the state)
        s.vehicles[i].movement.as_mut().unwrap().ladder = Some(ladder);
        c.on_foot(&s, p);
        s.vehicles[i].craft.body.pos[1] = y + 6.0;
        c.on_foot(&s, p);
        s.vehicles[i].movement.as_mut().unwrap().ladder = None;
        c.on_foot(&s, p);
        assert_eq!((c.caps.climbs, c.caps.climb_m.round()), (1, 6.0), "{:?}", c.caps);
    }

    #[test]
    fn leaving_a_boat_near_the_shore_and_walking_on() {
        let Some(w) = world("Omaha_Beach") else { return };
        let mut s = sim(&w);
        let (p, i) = bot(&mut s);
        let at = s.vehicles[i].craft.body.pos;
        let boat = s.spawn_vehicle("LCVP", [at[0] + 5.0, at[1] + 1.0, at[2]], [0.0; 3]).expect("LCVP");
        for _ in 0..3 {
            s.step();
        }
        let mut c = Census::default();
        s.enter(i, boat, 0);
        s.step();
        c.step(&s);
        assert_eq!(c.was_in.get(&p), Some(&boat));
        s.exit(i, boat);
        s.step();
        c.step(&s);
        assert_eq!((c.caps.boat_exits, c.caps.near_shore), (1, 1), "{:?}", c.caps);
        let from = c.landings[0].from;
        // somewhere dry, further on
        let water = w.level.terrain.water_level as f32;
        let (x, h) = (15..300).map(|d| from[0] + d as f32).map(|x| (x, w.height_at(x as f64, from[2] as f64).unwrap() as f32)).find(|(x, h)| *h > water && (*x - from[0]).abs() >= LANDED_WALK).unwrap();
        s.vehicles[i].craft.body.pos = [x, h + 1.0, from[2]];
        s.steps += LANDED_TICKS;
        c.look_at_landings(&s);
        assert_eq!(c.caps.went_on, 1, "{:?}", c.caps);
        assert!(c.landings.is_empty());
    }
}
