//! Each vehicle of a template, alone in the level (no players): position, how upright, speed and
//! hit points every half second, and each damage it takes (for empty vehicles that break or move on their own):
//! `cargo run --release -p bytefield-sim --example vehtrack -- <game> <mod> <level> <template> [seconds] [--mode m] [--every frames]`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let template = a[4].to_ascii_lowercase();
    let seconds: f64 = a.get(5).and_then(|v| v.parse().ok()).unwrap_or(60.0);
    let mode = a.iter().position(|x| x == "--mode").and_then(|i| a.get(i + 1)).map_or("conquest", |m| m.as_str());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], mode).expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let every: u64 = a.iter().position(|x| x == "--every").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(15);
    let mut damage = 0;
    for k in 1..=(seconds * 30.0) as u64 {
        s.step();
        for d in &s.journal.damage_log[damage..] {
            if s.vehicles[d.vehicle].template.to_ascii_lowercase() == template {
                println!("{:6.2} s  #{} damage {:.2} {:?} {:?} hp {:.1}", s.clock.time, d.vehicle, d.amount, d.source, d.status, d.hp);
            }
        }
        damage = s.journal.damage_log.len();
        if k % every != 0 {
            continue;
        }
        for (i, v) in s.vehicles.iter().enumerate().filter(|(_, v)| v.template.to_ascii_lowercase() == template) {
            let b = &v.craft.body;
            let speed = b.vel.iter().map(|x| x * x).sum::<f32>().sqrt();
            // (the up row's y: 1 upright, below 0 upside down)
            println!("{:6.2} s  #{i} at {:?} up {:.2} speed {speed:.1} hp {:?}", s.clock.time, b.pos.map(|x| (x * 10.0).round() / 10.0), b.rows[1][1], v.armor.as_ref().map(|a| a.hp));
        }
    }
}
