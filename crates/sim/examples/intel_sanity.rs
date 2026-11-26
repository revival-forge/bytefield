//! The side's estimate of the enemy around each flag (`bots::intel`) against the real presence (enemies alive within 150 m, which
//! the bench knows and the bots do not), at every split of a round of bots against bots.
//!   intel_sanity <game> --maps mod:Level,... [--seeds N] [--minutes M] [--players P]
//! Per map: of the (split, side, own or any flag) samples with 4 or more real enemies, % with an estimate above 0; of those with none,
//! % with an estimate below 1; and the mean estimate per real count bucket.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let (Some(game), Some(maps)) = (a.get(1), opt("--maps")) else {
        eprintln!("intel_sanity <game> --maps mod:Level,... [--seeds N] [--minutes M] [--players P]");
        std::process::exit(2);
    };
    let seeds: u64 = opt("--seeds").map_or(2, |n| n.parse().unwrap());
    let minutes: f64 = opt("--minutes").map_or(8.0, |n| n.parse().unwrap());
    let players: usize = opt("--players").map_or(32, |n| n.parse().unwrap());
    bytefield_sim::bots::intel::log_presence(true);
    let (mut all4, mut all0) = ([0u32; 2], [0u32; 2]);
    for m in maps.split(',') {
        let (modname, level) = m.split_once(':').expect("mod:Level");
        let (mut n4, mut n0) = ([0u32; 2], [0u32; 2]);
        let mut buckets = [(0.0f64, 0u32); 6];
        for seed in 1..=seeds {
            let w = bytefield_sim::World::load(std::path::Path::new(game), modname, level, "conquest").expect("world");
            let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            s.enable_bots();
            s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            for team in 1..=2 {
                s.set_minds(team, bytefield_sim::bots::mind("bots", level, seed));
            }
            for i in 0..players {
                let p = s.join(0, true);
                s.players[p].kit = (i / 2) % 5;
            }
            bytefield_sim::bots::intel::take_log();
            for _ in 0..(minutes * 1800.0) as usize {
                s.step();
                let log = bytefield_sim::bots::intel::take_log();
                if log.is_empty() {
                    continue;
                }
                let flags: Vec<[f32; 3]> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().map(|p| p.pos).collect());
                for (_, team, est) in log {
                    for (f, e) in flags.iter().zip(est) {
                        let real = s
                            .players
                            .iter()
                            .filter(|p| p.alive && p.team != team && (1..=2).contains(&p.team))
                            .filter_map(|p| s.vehicles.get(p.soldier?))
                            .filter(|v| (v.craft.body.pos[0] - f[0]).hypot(v.craft.body.pos[2] - f[2]) < 150.0)
                            .count();
                        if real >= 4 {
                            n4[1] += 1;
                            n4[0] += (e > 0.0) as u32;
                        }
                        if real == 0 {
                            n0[1] += 1;
                            n0[0] += (e < 1.0) as u32;
                        }
                        let b = real.min(5);
                        buckets[b].0 += e as f64;
                        buckets[b].1 += 1;
                    }
                }
            }
        }
        let pct = |x: [u32; 2]| if x[1] > 0 { format!("{:.0} % of {}", 100.0 * x[0] as f64 / x[1] as f64, x[1]) } else { "-".into() };
        let means: Vec<String> = buckets.iter().enumerate().map(|(k, b)| format!("{k}{}:{}", if k == 5 { "+" } else { "" }, if b.1 > 0 { format!("{:.2}", b.0 / b.1 as f64) } else { "-".into() })).collect();
        println!("{level}: real >= 4 with estimate > 0: {}; real 0 with estimate < 1: {}; mean estimate by real count {}", pct(n4), pct(n0), means.join(" "));
        for k in 0..2 {
            all4[k] += n4[k];
            all0[k] += n0[k];
        }
    }
    let pct = |x: [u32; 2]| if x[1] > 0 { 100.0 * x[0] as f64 / x[1] as f64 } else { f64::NAN };
    println!("all: real >= 4 with estimate > 0: {:.0} %; real 0 with estimate < 1: {:.0} %", pct(all4), pct(all0));
}
