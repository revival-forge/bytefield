//! Where the vehicles of one template go in a bots round and who drives them: every `--every` seconds, position, speed and the
//! driver's player (to follow with `BYTEFIELD_BOTS_ME=<player>` on a second run with the same seed):
//!   vehicle_trip <game> <mod> <level> <template> <players> <minutes> [--mind bots:no-jam] [--seed N] [--every s] [--mode m]
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 7 {
        eprintln!("vehicle_trip <game> <mod> <level> <template> <players> <minutes> [--mind bots:no-jam] [--seed N] [--every s] [--mode m]");
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let template = a[4].to_ascii_lowercase();
    let (players, minutes): (usize, f64) = (a[5].parse().unwrap(), a[6].parse().unwrap());
    let w = bytefield_sim::World::load(Path::new(&a[1]), &a[2], &a[3], opt("--mode").unwrap_or("conquest")).expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    bytefield_sim::bots::use_by_default(true);
    s.enable_bots();
    let seed: u64 = opt("--seed").map_or(0, |n| n.parse().unwrap());
    if seed != 0 {
        s.rng = bytefield_sim::players::Rng(s.rng.0 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    if let Some(v) = opt("--mind").and_then(|m| m.strip_prefix("bots:").or_else(|| m.strip_prefix("bots:"))) {
        let parts: Vec<&str> = v.split('+').collect();
        for team in [1, 2] {
            s.set_minds(team, bytefield_sim::bots::maker_variant(&a[3], seed, &parts));
        }
    }
    for i in 0..players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let every = (opt("--every").map_or(2.0, |e| e.parse().unwrap()) * 30.0) as u64;
    for k in 0..(minutes * 60.0 * 30.0) as u64 {
        s.step();
        if k % every != 0 {
            continue;
        }
        for (i, v) in s.vehicles.iter().enumerate().filter(|(_, v)| v.in_world() && v.template.to_ascii_lowercase() == template) {
            let b = &v.craft.body;
            let speed = b.vel.iter().map(|x| x * x).sum::<f32>().sqrt();
            let driver = v.entry.driver.and_then(|d| s.players.iter().position(|p| p.soldier == Some(d)));
            // heading (degrees, 0 = +z, 90 = +x) and the ground 4 m ahead over the ground below: what it may be pushing against
            let f = b.rows[2];
            let yaw = f[0].atan2(f[2]).to_degrees();
            let h = |x: f32, z: f32| w.height_at(x as f64, z as f64).unwrap_or(0.0) as f32;
            let rise = h(b.pos[0] + 4.0 * f[0], b.pos[2] + 4.0 * f[2]) - h(b.pos[0], b.pos[2]);
            println!("{:6.1} s  #{i} at {:?} up {:.2} speed {speed:.1} yaw {yaw:.0} rise4m {rise:.1} driver {driver:?}", s.clock.time, b.pos.map(|x| x.round() as i32), b.rows[1][1]);
        }
    }
}
