//! A windowless tournament of minds, with the sides swapped:
//!   tournament3 <game> [--a <mind>] [--b <mind>] --maps mod:Level,mod:Level,… --seeds N --minutes M --players P --out <dir> [--first-seed F] [--mode conquest] [--threads T] [--twin] [--split] [--events]
//! Without `--a` or `--b`, both minds are bots .
//! Per map and seed, two rounds with the same seed: A on side 1 and B on side 2, and the other way round (the map's bias cancels out).
//! With the same mind on both sides (`--a X --b X`) the two rounds come out identical (the sim is deterministic): just one per
//! seed, and the summary counts side 1 against side 2 (counting each round twice made «8 of 8» really 4 of 4).
//! Rounds run in parallel (one thread per core minus one, each with its own `World`). One line per round on stdout
//! (`map seed side winner tickets flags kills seconds`, `side` = A's side, `winner` = A, B or draw) and one JSON per
//! round in `--out` (`meta`, `summary` with `arena`'s header, `meter`: `bytefield_sim::quality` per side and B16). The table:
//! `python3 -I python/tools/bots_table.py <out>`.
//! `--twin`: with the same mind on both sides, B's mind gets another seed (the bots' own random draws differ, the world's do
//! not), and both rounds per seed are played: a true A against A, to measure the noise floor of a paired comparison.
//! `--split` (A/B within a side, only for what each bot does on its own): every side plays with one head, half of its bots with A's
//! settings and half with B's, drawn by seed in pairs of consecutive players of the side (the join order sets the kit, so each kit
//! is split too); per seed, a second round with the halves swapped. The JSON's `bots` has each bot's half and its numbers. With
//! `--twin` too, B's half draws from another seed (A against A, to check the test is fair).
//! A side's mind is a name (`bots:regroup`, `bots:regroup-way+rally_m=150`) or a parameter file (`params/bots/anchor-0.toml`: `mind`,
//! `levers` and `params`, `bytefield_sim::bots::params`): a candidate of the learning loop is a side file.
//! `--events`: also `<out>/events/<round>.json`, beside each round's JSON, the round's events with their place
//! (`bytefield_sim::round_events`: deaths, vehicles lost, flags taken, arrivals at flags, presence per cell); the round is the same.
//! Minds: those of `mind_of` (below; `none` is the plain bot the sim brings).
use bytefield_sim::bot_api::MindMaker;
use bytefield_sim::quality::Meter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn mind_of(name: &str, level: &str, seed: u64) -> Option<MindMaker> {
    match name {
        // variants to measure what each piece brings: bots:no-squad, bots:no-assign+no-spawn...
        n if bytefield_sim::bots::is_name(n) => Some(bytefield_sim::bots::mind(n, level, seed)),
        "none" => None,
        other => panic!("no mind called {other}"),
    }
}

#[derive(Clone)]
struct Job {
    game: PathBuf,
    modname: String,
    level: String,
    seed: u64,
    /// A's side (1 or 2)
    side_a: u32,
    a: String,
    b: String,
    players: usize,
    minutes: f64,
    mode: String,
    reference_ai: bool,
    /// B's mind seed differs from A's (`--twin`)
    twin: bool,
    /// A/B within each side (`--split`): `side_a` 2 is the round with the halves swapped
    split: bool,
    /// what was asked for each side (a name or a parameter file)
    specs: (String, String),
    /// also each round's events with their place (`--events`)
    events: bool,
}

/// A bot's numbers in the round, for `--split` (one per second).
#[derive(Default, Clone)]
struct Tally {
    alive_s: u32,
    /// alive within [`DISPUTED_M`] of a flag that changes hands with both sides alive within that distance
    disputed_s: u32,
    /// flags taken by its side while it was within their radius
    captures: u32,
}

const DISPUTED_M: f32 = 60.0;

/// Which players play with B's settings in `--split`: in each side, of each pair of consecutive players (by index), one, by a
/// draw on the seed; `swap` gives the other one.
fn split_halves(s: &bytefield_sim::Sim, seed: u64, swap: bool) -> Vec<bool> {
    let mut b = vec![false; s.players.len()];
    for team in [1u32, 2] {
        let side: Vec<usize> = (0..s.players.len()).filter(|&k| s.players[k].team == team).collect();
        for (n, pair) in side.chunks(2).enumerate() {
            let mut x = seed ^ ((team as u64) << 40) ^ (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            // (splitmix64's finaliser)
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            x ^= x >> 31;
            let first = (x & 1 == 1) != swap;
            b[pair[0]] = first;
            if let Some(&k) = pair.get(1) {
                b[k] = !first;
            }
        }
    }
    b
}

/// What `--twin` mixes into B's mind seed.
const TWIN_SALT: u64 = 0x7417_0000_0000_0001;

fn round(j: &Job) -> (serde_json::Value, Option<serde_json::Value>) {
    let w = bytefield_sim::World::load(&j.game, &j.modname, &j.level, &j.mode).expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.rng = bytefield_sim::players::Rng(s.rng.0 ^ j.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    // (`--split`: who is in B's half is known once all have joined; the makers ask it at each bot's first spawn)
    let halves: Arc<std::sync::OnceLock<Vec<bool>>> = Arc::default();
    if j.split {
        let seed_b = if j.twin { j.seed ^ TWIN_SALT } else { j.seed };
        for team in [1, 2] {
            let h = halves.clone();
            let in_b = Arc::new(move |k: usize| h.get().and_then(|b| b.get(k).copied()).unwrap_or(false));
            let m = bytefield_sim::bots::mind_split(&j.a, &j.b, &j.level, j.seed, seed_b, in_b).unwrap_or_else(|e| panic!("--split: {e}"));
            s.set_minds(team, m);
        }
    } else if !j.reference_ai {
        let seed_b = if j.twin { j.seed ^ TWIN_SALT } else { j.seed };
        for (name, team, seed) in [(&j.a, j.side_a, j.seed), (&j.b, 3 - j.side_a, seed_b)] {
            if let Some(m) = mind_of(name, &j.level, seed) {
                s.set_minds(team, m);
            }
        }
    }
    for i in 0..j.players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    if j.split {
        halves.set(split_halves(&s, j.seed, j.side_a == 2)).unwrap();
    }
    let mut tally = vec![Tally::default(); s.players.len()];
    let mut flag_team: Vec<u32> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().map(|p| p.team).collect());
    let mut meter = Meter::new();
    let mut events = j.events.then(bytefield_sim::round_events::RoundEvents::new);
    meter.b21_off = bytefield_sim::bots::body::MapProfile::de(&j.level).enter_wait > 0.0;
    let steps = (j.minutes * 60.0 * 30.0) as u64;
    for _ in 0..steps {
        s.step();
        meter.step(&s);
        if let Some(e) = events.as_mut() {
            e.step(&s);
        }
        if j.split && s.clock.steps % 30 == 0 {
            count(&s, &mut tally, &mut flag_team);
        }
        if s.round.round_end.is_some() || s.modes.conquest.as_ref().is_some_and(|c| c.winner.is_some()) {
            break;
        }
    }
    let (mut flags, mut kills, mut deaths) = ([0u32; 3], [0u32; 3], [0u32; 3]);
    if let Some(c) = s.modes.conquest.as_ref() {
        for p in &c.points {
            flags[p.team.min(2) as usize] += 1;
        }
    }
    for p in &s.players {
        kills[p.team.min(2) as usize] += p.kills;
        deaths[p.team.min(2) as usize] += p.deaths;
    }
    let winner = s.modes.conquest.as_ref().and_then(|c| c.winner);
    // (no winner when time runs out: whoever has more tickets wins; equal, a draw)
    let winning_side = match winner {
        Some(w) if w > 0 => Some(w as u32),
        Some(_) => Some(0),
        None => s.modes.conquest.as_ref().map(|c| match c.tickets[0].cmp(&c.tickets[1]) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => 2,
            _ => 0,
        }),
    };
    let winner_label = match winning_side {
        Some(t) if t == j.side_a => "A",
        Some(0) | None => "draw",
        Some(_) => "B",
    };
    // (flags that change hands: none, and no flag can be taken: DC_Weapon_Bunkers' two are bases)
    let takeable = s.modes.conquest.as_ref().map_or(0, |c| c.points.iter().filter(|p| !p.template.unable_to_change_team).count());
    let summary = serde_json::json!({"level": j.level, "mode": j.mode, "players": j.players, "seconds": s.clock.steps as f64 / 30.0, "flags": flags, "takeable_flags": takeable, "kills": kills, "deaths": deaths, "round_end": s.round.round_end, "tickets": s.modes.conquest.as_ref().map(|c| c.tickets), "winner": winner});
    let events = events.map(|e| e.finish(&s));
    let r = serde_json::json!({
        "meta": {"bots_version": bytefield_sim::bots::VERSION, "mod": j.modname, "level": j.level, "seed": j.seed, "side_a": j.side_a, "a": j.a, "b": j.b, "players": j.players, "minutes": j.minutes, "winner": winner_label, "reference_ai": j.reference_ai, "twin": j.twin, "split": j.split, "spec_a": j.specs.0, "spec_b": j.specs.1},
        "summary": summary,
        "meter": meter.finish(),
        "bots": if j.split { bots_json(&s, &tally, halves.get().map(|v| v.as_slice()).unwrap_or(&[])) } else { serde_json::Value::Null },
    });
    (r, events)
}

/// One second of `--split`'s tally.
fn count(s: &bytefield_sim::Sim, tally: &mut [Tally], flag_team: &mut [u32]) {
    let Some(c) = s.modes.conquest.as_ref() else { return };
    let pos: Vec<Option<(u32, [f32; 3])>> = s.players.iter().map(|p| p.soldier.filter(|_| p.alive && (1..=2).contains(&p.team)).and_then(|i| s.vehicles.get(i)).map(|v| (p.team, v.craft.body.pos))).collect();
    let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let disputed: Vec<[f32; 3]> = c
        .points
        .iter()
        .filter(|f| !f.template.unable_to_change_team)
        .filter(|f| [1u32, 2].iter().all(|&t| pos.iter().flatten().any(|(pt, x)| *pt == t && d(*x, f.pos) < DISPUTED_M)))
        .map(|f| f.pos)
        .collect();
    for (k, p) in pos.iter().enumerate() {
        let Some((_, x)) = p else { continue };
        tally[k].alive_s += 1;
        if disputed.iter().any(|f| d(*f, *x) < DISPUTED_M) {
            tally[k].disputed_s += 1;
        }
    }
    for (i, f) in c.points.iter().enumerate() {
        if flag_team.get(i).is_some_and(|&t| t != f.team) && (1..=2).contains(&f.team) {
            for (k, p) in pos.iter().enumerate() {
                if p.is_some_and(|(t, x)| t == f.team && d(x, f.pos) < f.template.radius.max(10.0)) {
                    tally[k].captures += 1;
                }
            }
        }
        if let Some(t) = flag_team.get_mut(i) {
            *t = f.team;
        }
    }
}

fn bots_json(s: &bytefield_sim::Sim, tally: &[Tally], in_b: &[bool]) -> serde_json::Value {
    s.players
        .iter()
        .enumerate()
        .filter(|(_, p)| p.ai && (1..=2).contains(&p.team))
        .map(|(k, p)| {
            let t = &tally[k];
            serde_json::json!({"player": k, "team": p.team, "half": if in_b.get(k).copied().unwrap_or(false) { "B" } else { "A" }, "kit": p.kit, "kills": p.kills, "deaths": p.deaths, "score": p.score, "alive_s": t.alive_s, "disputed_s": t.disputed_s, "captures": t.captures})
        })
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let (Some(game), Some(maps), Some(out)) = (a.get(1).filter(|g| !g.starts_with("--")), opt("--maps"), opt("--out")) else {
        eprintln!("tournament3 <game> [--a <mind>] [--b <mind>] --maps mod:Level,… --seeds N --minutes M --players P --out <dir> [--first-seed F] [--mode conquest] [--threads T] [--twin] [--split] [--events]");
        std::process::exit(2);
    };
    // (a mind that is not named is `bots`; `none` leaves the plain bot; a `.toml` is a side's parameter file)
    let (spec_a, spec_b) = (opt("--a").unwrap_or("bots"), opt("--b").unwrap_or("bots"));
    let resolve = |spec: &str| -> String {
        if spec.ends_with(".toml") {
            bytefield_sim::bots::params::SideFile::load(Path::new(spec)).unwrap_or_else(|e| panic!("{spec}: {e}")).mind_name()
        } else {
            spec.to_string()
        }
    };
    let (mind_a, mind_b) = (resolve(spec_a), resolve(spec_b));
    let (mind_a, mind_b) = (mind_a.as_str(), mind_b.as_str());
    let reference_ai = false;
    let seeds: u64 = opt("--seeds").map_or(4, |n| n.parse().unwrap());
    // (one process per seed, so that a trace on stderr belongs to one round)
    let first_seed: u64 = opt("--first-seed").map_or(1, |n| n.parse().unwrap());
    let minutes: f64 = opt("--minutes").map_or(8.0, |n| n.parse().unwrap());
    let players: usize = opt("--players").map_or(32, |n| n.parse().unwrap());
    let mode = opt("--mode").unwrap_or("conquest").to_string();
    let twin = a.iter().any(|x| x == "--twin");
    if twin && mind_a != mind_b {
        eprintln!("--twin only makes sense with the same mind on both sides");
        std::process::exit(2);
    }
    let split = a.iter().any(|x| x == "--split");
    let events = a.iter().any(|x| x == "--events");
    if split && (reference_ai || !bytefield_sim::bots::is_name(mind_a) || !bytefield_sim::bots::is_name(mind_b)) {
        eprintln!("--split is for two bots minds");
        std::process::exit(2);
    }
    if split {
        if let Err(e) = bytefield_sim::bots::mind_split(mind_a, mind_b, "", 0, 0, Arc::new(|_| false)) {
            eprintln!("--split: {e}");
            std::process::exit(2);
        }
    }
    // (the same mind on both sides plays one round per seed, unless its twin has another seed)
    let one_side = mind_a == mind_b && !twin;
    let threads: usize = opt("--threads").map_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1), |n| n.parse().unwrap());
    std::fs::create_dir_all(out).unwrap();
    let mut jobs = Vec::new();
    for m in maps.split(',') {
        let (modname, level) = m.split_once(':').unwrap_or_else(|| panic!("--maps: mod:Level, not {m}"));
        for seed in first_seed..first_seed + seeds {
            for side_a in if one_side { &[1u32][..] } else { &[1u32, 2][..] } {
                let side_a = *side_a;
                jobs.push(Job {
                    game: PathBuf::from(game),
                    modname: modname.into(),
                    level: level.into(),
                    seed,
                    side_a,
                    a: mind_a.into(),
                    b: mind_b.into(),
                    players,
                    minutes,
                    mode: mode.clone(),
                    reference_ai,
                    twin,
                    split,
                    specs: (spec_a.to_string(), spec_b.to_string()),
                    events,
                });
            }
        }
    }
    let total = jobs.len();
    let t0 = std::time::Instant::now();
    let jobs = Arc::new(jobs);
    let next = Arc::new(AtomicUsize::new(0));
    let stdout = Arc::new(Mutex::new(()));
    let out = Arc::new(PathBuf::from(out));
    let summary = Arc::new(Mutex::new((0usize, 0usize, 0usize, 0usize)));
    let handles: Vec<_> = (0..threads.min(total).max(1))
        .map(|_| {
            let (jobs, next, stdout, out, summary) = (jobs.clone(), next.clone(), stdout.clone(), out.clone(), summary.clone());
            std::thread::spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(j) = jobs.get(i) else { break };
                    let t = std::time::Instant::now();
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| round(j)));
                    let _g = stdout.lock().unwrap();
                    match r {
                        Ok((r, ev)) => {
                            let (m, rs) = (&r["meta"], &r["summary"]);
                            let stem = format!("{}_{}_s{}_l{}", j.modname, j.level, j.seed, j.side_a);
                            std::fs::write(Path::new(&*out).join(format!("{stem}.json")), serde_json::to_string_pretty(&r).unwrap()).unwrap();
                            if let Some(ev) = ev {
                                // (in a folder of their own: the round's JSON and what globs `<out>/*.json` stay as they were)
                                let dir = Path::new(&*out).join("events");
                                std::fs::create_dir_all(&dir).unwrap();
                                std::fs::write(dir.join(format!("{stem}.json")), serde_json::to_string(&ev).unwrap()).unwrap();
                            }
                            let g = m["winner"].as_str().unwrap_or("?");
                            println!(
                                "{}:{} {} {} {} {} {} {} {:.0}s ({:.1}s wall clock)",
                                j.modname,
                                j.level,
                                j.seed,
                                j.side_a,
                                g,
                                rs["tickets"],
                                rs["flags"],
                                rs["kills"],
                                rs["seconds"].as_f64().unwrap_or(0.0),
                                t.elapsed().as_secs_f64()
                            );
                            let mut x = summary.lock().unwrap();
                            match g {
                                "A" => x.0 += 1,
                                "B" => x.1 += 1,
                                _ => x.2 += 1,
                            }
                        }
                        Err(_) => {
                            println!("{}:{} {} {} ERROR (panic)", j.modname, j.level, j.seed, j.side_a);
                            summary.lock().unwrap().3 += 1;
                        }
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let (ga, gb, e, err) = *summary.lock().unwrap();
    let (na, nb) = if one_side { ("side 1", "side 2") } else { ("A", "B") };
    println!(
        "tournament3 {mind_a} against {mind_b}: {total} rounds, {na} {ga} {nb} {gb} draws {e} errors {err}, {:.0} s wall clock with {threads} threads, JSON in {}",
        t0.elapsed().as_secs_f64(),
        out.display()
    );
    if err > 0 {
        std::process::exit(1);
    }
}
