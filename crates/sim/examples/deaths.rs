//! Why do a team's players die? One headless round with a mind per team, and one line per death of the watched team:
//!   deaths <game> <mod> <level> --a <mind> --b <mind> [--seed N] [--minutes M] [--players J] [--team 1|2] [--side-a 1|2]
//! Each line: time, player, seconds alive, where it spawned and where it died (x y z), the fall from the highest point of
//! its life, the vehicle it was in, and the damage events of its last 3 s (source, amount, who gave it). A summary by
//! cause at the end, and `deaths N falls M` (falls: deaths on foot after dropping more than 20 m from the highest point of a life spent wholly on foot). The minds are the ones of `torneo3` (bots, bots:<variant>, none).
use bytefield_sim::bot_api::MindMaker;
use bytefield_sim::damage::Source;
use bytefield_sim::players::PlayerEvent;
use std::collections::BTreeMap;

fn mind_of(name: &str, level: &str, seed: u64) -> Option<MindMaker> {
    match name {
        // variants to measure what each piece brings: bots:no-squad, bots:no-assign+no-spawn...
        n if bytefield_sim::bots::is_name(n) => Some(bytefield_sim::bots::mind(n, level, seed)),
        "none" => None,
        other => panic!("no mind called {other}"),
    }
}

struct Life {
    spawn_step: u64,
    spawn_at: [f32; 3],
    top: f32,
    /// it has been in a vehicle in this life
    rode: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("deaths <game> <mod> <level> --a <mind> --b <mind> [--seed N] [--minutes M] [--players J] [--team 1|2] [--side-a 1|2]");
        std::process::exit(2);
    }
    let opt = |k: &str, d: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned().unwrap_or(d.to_string());
    let (game, modname, level) = (&args[1], &args[2], &args[3]);
    let (a, b) = (opt("--a", "bots"), opt("--b", "bots"));
    let seed: u64 = opt("--seed", "1").parse().unwrap();
    let minutes: f64 = opt("--minutes", "2").parse().unwrap();
    let players: usize = opt("--players", "32").parse().unwrap();
    let side_a: u32 = opt("--side-a", "1").parse().unwrap();
    let watch: u32 = opt("--team", &side_a.to_string()).parse().unwrap();

    let w = bytefield_sim::World::load(std::path::Path::new(game), modname, level, "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for (name, team) in [(&a, side_a), (&b, 3 - side_a)] {
        if let Some(m) = mind_of(name, level, seed) {
            s.set_minds(team, m);
        }
    }
    for i in 0..players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let (mut seen_p, mut seen_d) = (s.journal.player_log.len(), s.journal.damage_log.len());
    let mut lives: BTreeMap<usize, Life> = BTreeMap::new();
    // recent damage per soldier vehicle: (step, source, amount, by)
    let mut recent: BTreeMap<usize, Vec<(u64, Source, f32, Option<usize>)>> = BTreeMap::new();
    let mut by_cause: BTreeMap<String, u32> = BTreeMap::new();
    let (mut deaths, mut falls) = (0u32, 0u32);
    let steps = (minutes * 60.0 * 30.0) as u64;
    for _ in 0..steps {
        s.step();
        // the logs can be trimmed: read what is new each step
        seen_d = seen_d.min(s.journal.damage_log.len());
        for e in &s.journal.damage_log[seen_d..] {
            recent.entry(e.vehicle).or_default().push((e.step, e.source, e.amount, e.by));
        }
        seen_d = s.journal.damage_log.len();
        for (k, l) in lives.iter_mut() {
            if let Some(v) = s.players[*k].soldier.and_then(|i| s.vehicles.get(i)) {
                l.rode |= v.entry.inside.is_some();
                let y = v.craft.body.pos[1];
                if y > l.top {
                    l.top = y;
                }
            }
        }
        seen_p = seen_p.min(s.journal.player_log.len());
        let new: Vec<_> = s.journal.player_log[seen_p..].to_vec();
        seen_p = s.journal.player_log.len();
        for (step, k, ev) in new {
            if s.players[k].team != watch {
                continue;
            }
            match ev {
                PlayerEvent::Spawned(v, _, _) => {
                    let at = s.vehicles.get(v).map(|v| v.craft.body.pos).unwrap_or([0.0; 3]);
                    lives.insert(k, Life { spawn_step: step, spawn_at: at, top: at[1], rode: false });
                    recent.remove(&v);
                }
                PlayerEvent::Died(v) => {
                    let at = s.vehicles.get(v).map(|v| v.craft.body.pos).unwrap_or([f32::NAN; 3]);
                    let inside = s.vehicles.get(v).and_then(|x| x.entry.inside).and_then(|i| s.vehicles.get(i)).map(|x| x.template.clone());
                    let life = lives.remove(&k);
                    let alive = life.as_ref().map(|l| (step - l.spawn_step) as f32 / 30.0).unwrap_or(f32::NAN);
                    let fall = life.as_ref().map(|l| l.top - at[1]).unwrap_or(f32::NAN);
                    let hits: Vec<_> = recent.remove(&v).unwrap_or_default().into_iter().filter(|h| h.0 + 90 >= step).collect();
                    let cause = match hits.last() {
                        None => "no damage".to_string(),
                        Some(h) => {
                            let foe = h.3.and_then(|b| s.players.iter().find(|p| p.soldier == Some(b) || s.vehicles.get(b).is_some_and(|x| x.entry.occupants.contains(&p.soldier))));
                            let who = match foe {
                                Some(p) if p.team == watch => "team",
                                Some(_) => "enemy",
                                None => "?",
                            };
                            format!("{:?}/{}", h.1, who)
                        }
                    };
                    *by_cause.entry(cause.clone()).or_default() += 1;
                    deaths += 1;
                    falls += (fall > 20.0 && !life.as_ref().is_some_and(|l| l.rode)) as u32;
                    let sp = life.as_ref().map(|l| l.spawn_at).unwrap_or([f32::NAN; 3]);
                    println!(
                        "t={:6.1} p={:2} alive={:5.1}s spawn=({:.0} {:.0} {:.0}) died=({:.0} {:.0} {:.0}) fall={:5.1} in={:?} cause={} hits={:?}",
                        step as f32 / 30.0, k, alive, sp[0], sp[1], sp[2], at[0], at[1], at[2], fall, inside, cause,
                        hits.iter().rev().take(4).map(|h| (format!("{:?}", h.1), h.2.round() as i32)).collect::<Vec<_>>()
                    );
                }
                _ => {}
            }
        }
    }
    println!("by cause: {by_cause:?}");
    println!("deaths {deaths} falls {falls}");
}
