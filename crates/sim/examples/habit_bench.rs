//! B31, a habit punished: a scripted person walks back and forth along the same stretch (from where it spawns, [`ROUTE_M`] away from
//! the nearest enemy flag, behind its lines: towards the flag it died every 25 s whoever the enemy was) life after life, among bots; the bots of the other side are `--against` (default `bots`). Per map: the
//! person's deaths in the first half of the round and in the second, and their ratio (a side that learns the habit kills it more
//! in the second half).
//!   habit_bench <game> --maps mod:Level,... [--against bots:streetwise] [--seeds N] [--minutes M] [--players P] [--forward]
//! `--forward`: the stretch is the last [`ROUTE_M`] before the nearest enemy flag (40 m short of it) instead, on the measured side's
//! ground: the habit an ambush on its own side, `bots:ambush-own`, can punish.
//! The person is moved by hand along the ground at walking pace (a script, not a player: it does not shoot).

/// The stretch, m, and the pace, m/s.
const ROUTE_M: f32 = 150.0;
const PACE: f32 = 3.0;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let (Some(game), Some(maps)) = (a.get(1), opt("--maps")) else {
        eprintln!("habit_bench <game> --maps mod:Level,... [--against bots:streetwise] [--seeds N] [--minutes M] [--players P] [--forward]");
        std::process::exit(2);
    };
    let against = opt("--against").unwrap_or("bots");
    let seeds: u64 = opt("--seeds").map_or(4, |n| n.parse().unwrap());
    let minutes: f64 = opt("--minutes").map_or(8.0, |n| n.parse().unwrap());
    let players: usize = opt("--players").map_or(24, |n| n.parse().unwrap());
    let forward = a.iter().any(|x| x == "--forward");
    let (mut all1, mut all2) = (0u32, 0u32);
    for m in maps.split(',') {
        let (modname, level) = m.split_once(':').expect("mod:Level");
        let (mut first, mut second) = (0u32, 0u32);
        for seed in 1..=seeds {
            let w = bytefield_sim::World::load(std::path::Path::new(game), modname, level, "conquest").expect("world");
            let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            s.enable_bots();
            s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            // the person is on side 1; side 2's bots are the ones measured
            s.set_minds(1, bytefield_sim::bots::mind("bots", level, seed));
            s.set_minds(2, bytefield_sim::bots::mind(against, level, seed));
            let human = s.join(1, false);
            s.choose_spawn(human, None, 0);
            for i in 0..players {
                let p = s.join(0, true);
                s.players[p].kit = (i / 2) % 5;
            }
            let steps = (minutes * 1800.0) as usize;
            let mut route: Option<([f32; 3], [f32; 3])> = None;
            let mut soldier = None;
            let mut walked = 0.0f32;
            let mut deaths_seen = s.players[human].deaths;
            for k in 0..steps {
                s.step();
                let p = &s.players[human];
                if p.deaths > deaths_seen {
                    deaths_seen = p.deaths;
                    if k < steps / 2 {
                        first += 1;
                    } else {
                        second += 1;
                    }
                }
                let Some(i) = p.soldier.filter(|_| p.alive) else { continue };
                if soldier != Some(i) {
                    soldier = Some(i);
                    walked = 0.0;
                    if route.is_none() {
                        // (the same stretch every life: from the first spawn away from the nearest enemy flag)
                        let start = s.vehicles[i].craft.body.pos;
                        let flags: Vec<[f32; 3]> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().filter(|f| f.team != 1 && f.team != 0).map(|f| f.pos).collect());
                        let Some(f) = flags.iter().min_by(|a, b| (a[0] - start[0]).hypot(a[2] - start[2]).total_cmp(&(b[0] - start[0]).hypot(b[2] - start[2]))) else { continue };
                        let (dx, dz) = (f[0] - start[0], f[2] - start[2]);
                        let l = dx.hypot(dz).max(1.0);
                        // (forward: the stretch ends 40 m short of the enemy flag, on the measured side's ground; the script puts the
                        // person on it at once)
                        route = Some(if forward {
                            let (u0, u1) = ((l - 40.0 - ROUTE_M).max(0.0), (l - 40.0).max(0.0));
                            ([start[0] + dx / l * u0, start[1], start[2] + dz / l * u0], [start[0] + dx / l * u1, 0.0, start[2] + dz / l * u1])
                        } else {
                            (start, [start[0] - dx / l * ROUTE_M, 0.0, start[2] - dz / l * ROUTE_M])
                        });
                    }
                }
                let Some((a0, b0)) = route else { continue };
                walked += PACE / 30.0;
                let len = (b0[0] - a0[0]).hypot(b0[2] - a0[2]).max(1.0);
                let ph = (walked / len) % 2.0;
                let u = if ph < 1.0 { ph } else { 2.0 - ph };
                let (x, z) = (a0[0] + (b0[0] - a0[0]) * u, a0[2] + (b0[2] - a0[2]) * u);
                let y = s.world.height_at(x as f64, z as f64).map_or(a0[1], |h| h as f32) + 1.0;
                s.vehicles[i].craft.body.pos = [x, y, z];
            }
        }
        let ratio = if first > 0 { format!("{:.2}", second as f64 / first as f64) } else { "-".into() };
        println!("{level} against {against}: deaths first half {first}, second half {second}, ratio {ratio}");
        all1 += first;
        all2 += second;
    }
    println!("all against {against}: first {all1}, second {all2}, ratio {:.2}", all2 as f64 / all1.max(1) as f64);
}
