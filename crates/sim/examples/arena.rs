//! One round with the bots on both sides, or a new [`Mind`] on team 1, and what it looks like from outside (`bytefield_sim::dumb`):
//!   arena <game> <mod> <level> <players> <minutes> [--mode conquest] [--out out.json] [--mind bots|none] [--both] [--mind-team 1|2] [--seed N]
//! Without `--mind`, the bots play both teams; with one, the sim's plain bot plays the other. `--both` gives the mind to team 2 as well. Prints who won, tickets, flags and kills. Register a new mind in `mind_of` below.
use bytefield_sim::bot_api::MindMaker;
use bytefield_sim::dumb::Watcher;
use std::path::Path;

fn mind_of(name: &str, level: &str, seed: u64) -> Option<MindMaker> {
    match name {
        // (new minds go here)
        // variants to measure what each piece brings: bots:no-squad, bots:no-assign+no-spawn...
        n if bytefield_sim::bots::is_name(n) => Some(bytefield_sim::bots::mind(n, level, seed)),
        "none" => None,
        other => panic!("no mind called {other}"),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("arena <game> <mod> <level> <players> <minutes> [--mode conquest] [--out out.json] [--mind bots|none] [--both] [--mind-team 1|2] [--seed N]");
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let (players, minutes): (usize, f64) = (a[4].parse().unwrap(), a[5].parse().unwrap());
    let mode = opt("--mode").unwrap_or("conquest");
    let w = bytefield_sim::World::load(Path::new(&a[1]), &a[2], &a[3], mode).expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    // the bots play both teams unless `--mind` is given: then the sim's plain bot (`bots::life`) plays the team that has no mind
    bytefield_sim::bots::use_by_default(opt("--mind").is_none());
    s.enable_bots();
    let seed: u64 = opt("--seed").map_or(0, |n| n.parse().unwrap());
    if seed != 0 {
        s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    if let Some(name) = opt("--mind") {
        if let Some(m) = mind_of(name, &a[3], seed) {
            s.set_minds(opt("--mind-team").map_or(1, |t| t.parse().unwrap()), m);
        }
        if a.iter().any(|x| x == "--both")
            && let Some(m) = mind_of(name, &a[3], seed)
        {
            s.set_minds(2, m);
        }
    }
    for i in 0..players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let mut watcher = Watcher::new();
    let steps = (minutes * 60.0 * 30.0) as u64;
    for _ in 0..steps {
        s.step();
        watcher.step(&s);
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
    let summary = serde_json::json!({"bots_version": bytefield_sim::bots::VERSION, "level": a[3], "mode": mode, "players": players, "seconds": s.clock.steps as f64 / 30.0, "flags": flags, "kills": kills, "deaths": deaths, "round_end": s.round.round_end, "tickets": s.modes.conquest.as_ref().map(|c| c.tickets), "winner": s.modes.conquest.as_ref().and_then(|c| c.winner)});
    println!("{summary}");
    if let Some(out) = opt("--out") {
        std::fs::write(out, serde_json::to_string_pretty(&watcher.finish(summary)).unwrap()).unwrap();
    }
}
