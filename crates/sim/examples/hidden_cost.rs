//! What one line-of-sight ray (`Sim::hidden`) costs, with the soldiers where a round puts them: the budget of a position query.
//!   hidden_cost <game> <mod> <level> [bots] [seconds]
//! Plays `seconds` (default 60) with `bots` bots (default 64) so they spread over the map, then times rays between every pair of
//! soldiers on foot within 120 m (from eye height to a crouched body) and rays from each soldier to points on a 12 m ring (what a
//! query of nearby spots asks). Prints microseconds per ray and how many rays fit in 1 ms of a step.
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("hidden_cost <game> <mod> <level> [bots] [seconds]");
        std::process::exit(2);
    }
    let bots: usize = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(64);
    let secs: f64 = a.get(5).and_then(|v| v.parse().ok()).unwrap_or(60.0);
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    bytefield_sim::bots::use_by_default(true);
    s.enable_bots();
    for _ in 0..bots {
        s.join(0, true);
    }
    for _ in 0..(secs * 30.0) as usize {
        s.step();
    }
    let feet: Vec<[f32; 3]> = s
        .players
        .iter()
        .filter(|p| p.alive)
        .filter_map(|p| s.vehicles.get(p.soldier?))
        .filter(|v| v.entry.inside.is_none())
        .map(|v| v.craft.body.pos)
        .collect();
    let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let mut pairs = Vec::new();
    for (i, &p) in feet.iter().enumerate() {
        for &q in &feet[i + 1..] {
            if d(p, q) < 120.0 {
                pairs.push(([p[0], p[1] + 0.6, p[2]], [q[0], q[1] - 0.2, q[2]]));
            }
        }
        for k in 0..12 {
            let ang = k as f32 * std::f32::consts::TAU / 12.0;
            let x = p[0] + 12.0 * ang.cos();
            let z = p[2] + 12.0 * ang.sin();
            let y = s.world.height_at(x as f64, z as f64).map_or(p[1], |h| h as f32 + 0.8);
            pairs.push(([p[0], p[1] + 0.6, p[2]], [x, y, z]));
        }
    }
    if pairs.is_empty() {
        println!("hidden_cost {}: nobody on foot", a[3]);
        return;
    }
    let mut hidden = 0usize;
    let reps = (20_000 / pairs.len()).max(1);
    let t = Instant::now();
    for _ in 0..reps {
        for &(p, q) in &pairs {
            hidden += s.hidden(p, q) as usize;
        }
    }
    let n = reps * pairs.len();
    let us = t.elapsed().as_secs_f64() * 1e6 / n as f64;
    println!(
        "hidden_cost {}: {} on foot, {} rays ({:.0} % hidden), {us:.2} us per ray, {:.0} rays per ms",
        a[3],
        feet.len(),
        n,
        100.0 * hidden as f64 / n as f64,
        1000.0 / us
    );
}
