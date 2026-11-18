//! Where do the shots fired from the air go? Every projectile fired from a plane's or helicopter's seat is followed to its end;
//! the closest it passes to an enemy (a soldier on foot, or the vehicle an enemy is in, less that vehicle's radius) and whether it
//! hit one. The minds play a normal round; nothing in them is looked at.
//!   air_aim <game> <mod> <level> [seconds=300] [--mind bots3|bots2|bots3:<variant>] [--players 32] [--seed 1] [--seat 0|all]
//! Output: pilot-seconds per class (seated, in the air, with an enemy within 420 m); damage given by kind of target and source (a hit,
//! an explosion); one block per (class, seat, projectile template, so bombs and rockets apart from bullets): shots, hits on an enemy, and how many passed within 2, 5, 10, 20, 50 m (and, apart, how many passed that close to the centre of an enemy aircraft in flight, with nothing taken off for its size), with the median
//! distance to that enemy when fired; and the shooters.
use bytefield_sim::bot_api::{MindMaker, VehicleClass};
use std::collections::BTreeMap;

fn mind_of(name: &str, level: &str, seed: u64) -> MindMaker {
    match name {
        "bots2" => bytefield_sim::bots2::maker(level, seed),
        "bots3" => bytefield_sim::bots3::maker(level, seed),
        v if v.starts_with("bots3:") => bytefield_sim::bots3::maker_variant(level, seed, &v["bots3:".len()..].split('+').collect::<Vec<_>>()),
        other => panic!("no mind called {other}"),
    }
}

/// The distance from `m` to the segment from `a` to `b`.
fn to_segment(a: [f32; 3], b: [f32; 3], m: [f32; 3]) -> f32 {
    let d: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
    let dd: f32 = d.iter().map(|x| x * x).sum();
    let t = if dd < 1e-6 { 0.0 } else { ((0..3).map(|k| (m[k] - a[k]) * d[k]).sum::<f32>() / dd).clamp(0.0, 1.0) };
    (0..3).map(|k| (a[k] + d[k] * t - m[k]).powi(2)).sum::<f32>().sqrt()
}

#[derive(Default)]
struct Track {
    class: String,
    seat: usize,
    /// the projectile's template: which weapon (bullets, bombs, rockets)
    weapon: String,
    team: u32,
    shooter: usize,
    last: [f32; 3],
    /// where it was first seen and its velocity then (`AIR_AIM_DROPS=1`: each bomb's fall, to fit the pilot's release rule)
    first: Option<([f32; 3], [f32; 3], f64)>,
    /// closest pass to an enemy so far (m, beyond the target's radius) and how far that enemy was from where it was fired
    best: Option<(f32, f32)>,
    /// closest pass to the centre of an enemy aircraft in flight (nothing taken off for its size), m
    best_air: Option<f32>,
    hit: bool,
}

#[derive(Default)]
struct Block {
    shots: u32,
    hits: u32,
    within: [u32; 5],
    air_within: [u32; 5],
    none: u32,
    range: Vec<f32>,
    shooters: BTreeMap<usize, u32>,
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("air_aim <game> <mod> <level> [seconds=300] [--mind bots3|bots2] [--players 32] [--seed 1] [--seat 0|all]");
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let seconds: f32 = a.get(4).filter(|x| !x.starts_with("--")).map_or(300.0, |x| x.parse().unwrap());
    let mind = opt("--mind").unwrap_or("bots3".into());
    let players: usize = opt("--players").map_or(32, |x| x.parse().unwrap());
    let seed: u64 = opt("--seed").map_or(1, |x| x.parse().unwrap());
    let all_seats = opt("--seat").is_some_and(|x| x == "all");
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    for team in [1u32, 2] {
        s.set_minds(team, mind_of(&mind, &a[3], seed));
    }
    for _ in 0..players {
        s.join(0, true);
    }
    const BANDS: [f32; 5] = [2.0, 5.0, 10.0, 20.0, 50.0];
    let mut flying: BTreeMap<String, [u32; 3]> = BTreeMap::new();
    // damage given by an aircraft (`giveDamage`'s attacker): to soldiers and to vehicles, events, amount, destroyed
    let mut given: BTreeMap<(&'static str, String), (u32, f32, u32)> = BTreeMap::new();
    let mut damage_seen = 0usize;
    // the kills counted to players while they are in an aircraft's seat (what the scoreboard says)
    let mut kills_prev: Vec<u32> = Vec::new();
    let mut air_kills = 0u32;
    let mut live: BTreeMap<(usize, usize, [u32; 3]), Track> = BTreeMap::new();
    let mut blocks: BTreeMap<(String, usize, String), Block> = BTreeMap::new();
    let drops = std::env::var_os("AIR_AIM_DROPS").is_some();
    let close = |t: Track, blocks: &mut BTreeMap<(String, usize, String), Block>| {
        if let (true, Some((p0, v0, age))) = (drops && t.weapon.to_lowercase().contains("bomb"), t.first) {
            // the fall with no drag, from the first sight: what the pilot's rule predicts
            let fall = p0[1] - t.last[1];
            let tt = (v0[1] + (v0[1] * v0[1] + 2.0 * 9.81 * fall.max(0.0)).sqrt()) / 9.81;
            let flat = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            let pred = [p0[0] + v0[0] * tt, 0.0, p0[2] + v0[2] * tt];
            println!("drop {} fall {fall:.0} m speed {:.0} m/s vy {:.1} travel {:.0} m predicted {:.0} m miss {:.0} m (age at first sight {age:.2})", t.weapon, (v0[0] * v0[0] + v0[2] * v0[2]).sqrt(), v0[1], flat(p0, t.last), flat(p0, pred), flat(pred, t.last));
        }
        let b = blocks.entry((t.class.clone(), t.seat, t.weapon.clone())).or_default();
        b.shots += 1;
        b.hits += t.hit as u32;
        *b.shooters.entry(t.shooter).or_default() += 1;
        if let Some(d) = t.best_air {
            for (k, x) in BANDS.iter().enumerate() {
                b.air_within[k] += (d <= *x) as u32;
            }
        }
        match t.best {
            Some((d, r)) => {
                for (k, x) in BANDS.iter().enumerate() {
                    b.within[k] += (d <= *x) as u32;
                }
                b.range.push(r);
            }
            None => b.none += 1,
        }
    };
    for _ in 0..(seconds * 30.0) as u64 {
        s.step();
        // the enemies: where each player is, and the radius of what it is in
        let targets: Vec<(u32, usize, [f32; 3], f32)> = s
            .players
            .iter()
            .filter(|p| p.alive)
            .filter_map(|p| {
                let sv = s.vehicles.get(p.soldier?)?;
                match sv.entry.inside {
                    Some(c) => s.vehicles.get(c).map(|v| (p.team, c, v.craft.body.pos.map(|x| x as f32), v.radius.min(12.0))),
                    // (a soldier's chest, a metre over its feet)
                    None => Some((p.team, sv.id, [sv.craft.body.pos[0] as f32, sv.craft.body.pos[1] as f32 + 1.0, sv.craft.body.pos[2] as f32], 0.5)),
                }
            })
            .collect();
        // the enemy aircraft in flight (more than 15 m over the ground, somebody inside): where their centres are
        let flying_now: Vec<(u32, usize, [f32; 3])> = targets
            .iter()
            .filter(|t| s.vehicles.get(t.1).is_some_and(|v| VehicleClass::of(v).flies()))
            .filter(|t| w.height_at(t.2[0] as f64, t.2[2] as f64).is_some_and(|g| t.2[1] as f64 - g > 15.0))
            .map(|t| (t.0, t.1, t.2))
            .collect();
        // once a second: who is flying (pilot's seat, more than 10 m over the ground), and with an enemy within 420 m
        if s.clock.steps % 30 == 0 {
            for p in s.players.iter().filter(|p| p.alive) {
                let Some(c) = p.soldier.and_then(|i| s.vehicles.get(i)).filter(|v| v.entry.position == 0).and_then(|v| v.entry.inside) else { continue };
                let Some(car) = s.vehicles.get(c) else { continue };
                let class = VehicleClass::of(car);
                if !class.flies() {
                    continue;
                }
                let at = car.craft.body.pos.map(|x| x as f32);
                let up = w.height_at(at[0] as f64, at[2] as f64).is_some_and(|g| at[1] as f64 - g > 10.0);
                let e = flying.entry(format!("{class:?}")).or_insert([0u32; 3]);
                e[0] += 1;
                if up {
                    e[1] += 1;
                    let near = targets.iter().any(|t| t.0 != p.team && t.1 != c && (0..3).map(|k| (t.2[k] - at[k]).powi(2)).sum::<f32>() < 420.0 * 420.0);
                    e[2] += near as u32;
                }
            }
        }
        for e in s.journal.damage_log.get(damage_seen..).unwrap_or(&[]) {
            if std::env::var_os("AIR_AIM_DAMAGE").is_some() { eprintln!("dmg t {:.2} vehicle {} {} amount {:.1} source {:?} status {:?} by {:?}", s.clock.steps as f64 * s.clock.dt, e.vehicle, s.vehicles.get(e.vehicle).map_or("?", |x| x.template.as_str()), e.amount, e.source, e.status, e.by); }
            let Some(by) = e.by.and_then(|b| s.vehicles.get(b)) else { continue };
            if !VehicleClass::of(by).flies() || e.vehicle == by.id {
                continue;
            }
            // whose: the team of a player in the attacker, and of the soldier hit or of a player in the vehicle hit (0: empty)
            let team_in = |c: usize| s.players.iter().find(|p| p.soldier.and_then(|i| s.vehicles.get(i)).is_some_and(|v| v.entry.inside == Some(c) || v.id == c)).map_or(0, |p| p.team);
            let (ta, tv) = (team_in(by.id), team_in(e.vehicle));
            let soldier = s.vehicles.get(e.vehicle).is_some_and(|v| v.movement.is_some());
            let kind = match (soldier, tv) {
                (_, 0) => "empty vehicle",
                (true, t) if t == ta => "own soldier (a crash?)",
                (true, _) => "enemy soldier",
                (false, t) if t == ta => "own vehicle",
                (false, _) => "enemy vehicle",
            };
            let g = given.entry((kind, format!("{:?}", e.source))).or_default();
            g.0 += 1;
            g.1 += e.amount;
            g.2 += (e.status == Some(bytefield_sim::damage::Status::Destroyed)) as u32;
        }
        damage_seen = s.journal.damage_log.len();
        kills_prev.resize(s.players.len(), 0);
        for (k, p) in s.players.iter().enumerate() {
            let flies = p.soldier.and_then(|i| s.vehicles.get(i)).and_then(|v| v.entry.inside).and_then(|c| s.vehicles.get(c)).is_some_and(|c| VehicleClass::of(c).flies());
            if flies {
                air_kills += p.kills.saturating_sub(kills_prev[k]);
            }
            kills_prev[k] = p.kills;
        }
        let mut seen = std::collections::BTreeSet::new();
        for pr in &s.projectiles {
            let Some(owner) = pr.owner else { continue };
            let Some(car) = s.vehicles.get(owner) else { continue };
            let class = VehicleClass::of(car);
            if !class.flies() || (!all_seats && pr.seat != 0) {
                continue;
            }
            let start = pr.start.map(|x| x as f32);
            let key = (owner, pr.seat, start.map(f32::to_bits));
            seen.insert(key);
            let pos = pr.pos.map(|x| x as f32);
            let tr = live.entry(key).or_insert_with(|| {
                // who fired: the player in that seat
                let (team, shooter) = s
                    .players
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.soldier.and_then(|i| s.vehicles.get(i)).is_some_and(|v| v.entry.inside == Some(owner) && v.entry.position == pr.seat))
                    .map_or((0, usize::MAX), |(k, p)| (p.team, k));
                Track { class: format!("{class:?}"), seat: pr.seat, weapon: pr.template.name.clone(), team, shooter, last: start, ..Default::default() }
            });
            for &(team, id, p, r) in &targets {
                if team == tr.team || tr.team == 0 || id == owner {
                    continue;
                }
                let d = (to_segment(tr.last, pos, p) - r).max(0.0);
                if tr.best.is_none_or(|b| d < b.0) {
                    let r0 = (0..3).map(|k| (p[k] - start[k]).powi(2)).sum::<f32>().sqrt();
                    tr.best = Some((d, r0));
                }
            }
            for &(team, id, p) in &flying_now {
                if team != tr.team && tr.team != 0 && id != owner {
                    let d = to_segment(tr.last, pos, p);
                    tr.best_air = Some(tr.best_air.map_or(d, |b| b.min(d)));
                }
            }
            tr.hit |= pr.impacts.iter().any(|i| i.vehicle.is_some_and(|v| targets.iter().any(|t| t.1 == v && t.0 != tr.team)));
            if tr.first.is_none() {
                tr.first = Some((pos, pr.vel.map(|x| x as f32), pr.age));
            }
            tr.last = pos;
        }
        let gone: Vec<_> = live.keys().filter(|k| !seen.contains(*k)).cloned().collect();
        for k in gone {
            let t = live.remove(&k).unwrap();
            close(t, &mut blocks);
        }
    }
    for (_, t) in std::mem::take(&mut live) {
        close(t, &mut blocks);
    }
    println!("{} {} {}s, {mind}, seed {seed}", a[2], a[3], seconds);
    for (class, [seated, up, near]) in &flying {
        println!("{class}: pilot-seconds {seated}, in the air {up}, with an enemy within 420 m {near}");
    }
    println!("kills counted to players in an aircraft: {air_kills}");
    for ((kind, source), (n, amount, destroyed)) in &given {
        println!("damage from aircraft to an {kind} ({source}): {n} events, {amount:.0} in all, {destroyed} destroyed");
    }
    for ((class, seat, weapon), mut b) in blocks {
        b.range.sort_by(f32::total_cmp);
        let med = b.range.get(b.range.len() / 2).copied().unwrap_or(f32::NAN);
        let pct = |n: u32| 100.0 * n as f32 / b.shots.max(1) as f32;
        println!(
            "{class} seat {seat} {weapon}: {} shots, {} hits; within 2/5/10/20/50 m: {:.1}/{:.1}/{:.1}/{:.1}/{:.1} %; centre of an enemy aircraft in flight within 2/5/10/20/50 m: {}/{}/{}/{}/{} shots; no enemy {:.0} %; median range when fired {med:.0} m; shooters {:?}",
            b.shots,
            b.hits,
            pct(b.within[0]),
            pct(b.within[1]),
            pct(b.within[2]),
            pct(b.within[3]),
            pct(b.within[4]),
            b.air_within[0],
            b.air_within[1],
            b.air_within[2],
            b.air_within[3],
            b.air_within[4],
            pct(b.none),
            b.shooters
        );
    }
}
