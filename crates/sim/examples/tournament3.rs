//! A windowless tournament of minds, with the sides swapped:
//!   tournament3 <game> [--a <mind>] [--b <mind>] --maps mod:Level,mod:Level,… --seeds N --minutes M --players P --out <dir> [--first-seed F] [--mode conquest] [--threads T]
//! Without `--a` or `--b`, both minds are bots3 (`bots2` asks for the old one).
//! Per map and seed, two rounds with the same seed: A on side 1 and B on side 2, and the other way round (the map's bias cancels out).
//! With the same mind on both sides (`--a X --b X`) the two rounds come out identical (the sim is deterministic): just one per
//! seed, and the summary counts side 1 against side 2 (counting each round twice made «8 of 8» really 4 of 4).
//! Rounds run in parallel (one thread per core minus one, each with its own `World`). One line per round on stdout
//! (`map seed side winner tickets flags kills seconds`, `side` = A's side, `winner` = A, B or draw) and one JSON per
//! round in `--out` (`meta`, `summary` with `arena`'s header, `meter`: `bytefield_sim::quality` per side and B16). The table:
//! `python3 -I python/tools/bots3_table.py <out>`.
//! Minds: those of `mind_of` (below; `none` is the `bots2` the sim brings).
use bytefield_sim::bot_api::MindMaker;
use bytefield_sim::quality::Meter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn mind_of(name: &str, level: &str, seed: u64) -> Option<MindMaker> {
    match name {
        "bots2" => Some(bytefield_sim::bots2::maker(level, seed)),
        "bots3" => Some(bytefield_sim::bots3::maker(level, seed)),
        // variants to measure what each piece contributes: bots3:no-squad, bots3:no-assign+no-spawn...
        v if v.starts_with("bots3:") => Some(bytefield_sim::bots3::maker_variant(level, seed, &v["bots3:".len()..].split('+').collect::<Vec<_>>())),
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
}

fn round(j: &Job) -> serde_json::Value {
    let w = bytefield_sim::World::load(&j.game, &j.modname, &j.level, &j.mode).expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.rng = bytefield_sim::players::Rng(s.rng.0 ^ j.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    if !j.reference_ai {
        for (name, team) in [(&j.a, j.side_a), (&j.b, 3 - j.side_a)] {
            if let Some(m) = mind_of(name, &j.level, j.seed) {
                s.set_minds(team, m);
            }
        }
    }
    for i in 0..j.players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let mut meter = Meter::new();
    meter.b21_off = bytefield_sim::bots3::body::MapProfile::de(&j.level).enter_wait > 0.0;
    let steps = (j.minutes * 60.0 * 30.0) as u64;
    for _ in 0..steps {
        s.step();
        meter.step(&s);
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
    serde_json::json!({
        "meta": {"mod": j.modname, "level": j.level, "seed": j.seed, "side_a": j.side_a, "a": j.a, "b": j.b, "players": j.players, "minutes": j.minutes, "winner": winner_label, "reference_ai": j.reference_ai},
        "summary": summary,
        "meter": meter.finish(),
    })
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let (Some(game), Some(maps), Some(out)) = (a.get(1).filter(|g| !g.starts_with("--")), opt("--maps"), opt("--out")) else {
        eprintln!("tournament3 <game> [--a <mind>] [--b <mind>] --maps mod:Level,… --seeds N --minutes M --players P --out <dir> [--first-seed F] [--mode conquest] [--threads T]");
        std::process::exit(2);
    };
    // (a mind that is not named is bots3; `bots2` and `none` are the old ones)
    let (mind_a, mind_b) = (opt("--a").unwrap_or("bots3"), opt("--b").unwrap_or("bots3"));
    let reference_ai = false;
    let seeds: u64 = opt("--seeds").map_or(4, |n| n.parse().unwrap());
    // (one process per seed, so that a trace on stderr belongs to one round)
    let first_seed: u64 = opt("--first-seed").map_or(1, |n| n.parse().unwrap());
    let minutes: f64 = opt("--minutes").map_or(8.0, |n| n.parse().unwrap());
    let players: usize = opt("--players").map_or(32, |n| n.parse().unwrap());
    let mode = opt("--mode").unwrap_or("conquest").to_string();
    let threads: usize = opt("--threads").map_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1), |n| n.parse().unwrap());
    std::fs::create_dir_all(out).unwrap();
    let mut jobs = Vec::new();
    for m in maps.split(',') {
        let (modname, level) = m.split_once(':').unwrap_or_else(|| panic!("--maps: mod:Level, not {m}"));
        for seed in first_seed..first_seed + seeds {
            for side_a in if mind_a == mind_b { &[1u32][..] } else { &[1u32, 2][..] } {
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
                        Ok(r) => {
                            let (m, rs) = (&r["meta"], &r["summary"]);
                            let file = Path::new(&*out).join(format!("{}_{}_s{}_l{}.json", j.modname, j.level, j.seed, j.side_a));
                            std::fs::write(&file, serde_json::to_string_pretty(&r).unwrap()).unwrap();
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
    let (na, nb) = if mind_a == mind_b { ("side 1", "side 2") } else { ("A", "B") };
    println!(
        "tournament3 {mind_a} against {mind_b}: {total} rounds, {na} {ga} {nb} {gb} draws {e} errors {err}, {:.0} s wall clock with {threads} threads, JSON in {}",
        t0.elapsed().as_secs_f64(),
        out.display()
    );
    if err > 0 {
        std::process::exit(1);
    }
}
