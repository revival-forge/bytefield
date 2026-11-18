//! The slow simulation steps with bots: when and how much (the census's stutters).
//!   step_spikes <game> <mod> <level> [mode] [bots] [segundos] [umbral_ms]
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mode = a.get(4).map_or("conquest", |s| s);
    let bots: usize = a.get(5).and_then(|v| v.parse().ok()).unwrap_or(64);
    let secs: f64 = a.get(6).and_then(|v| v.parse().ok()).unwrap_or(25.0);
    let threshold: f64 = a.get(7).and_then(|v| v.parse().ok()).unwrap_or(20.0);
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], mode).expect("world");
    // PIECES=1: what each part of the path maps costs, on a cold load
    if std::env::var_os("PIECES").is_some() {
        use bytefield_sim::bot_api::Nav;
        for kind in [Nav::Infantry, Nav::Tank] {
            let t = Instant::now();
            let m = w.nav_map(kind);
            let a = t.elapsed().as_secs_f64() * 1e3;
            let Some(m) = m else { continue };
            let t = Instant::now();
            let c = (*m).clone();
            let b = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            let r = bytefield_sim::bots2::paths::Grid::from_map(&c);
            let d = t.elapsed().as_secs_f64() * 1e3;
            println!("{}x{}: nav_map {a:.1} ms, copy {b:.1} ms, Grid {d:.1} ms ({} cells)", m.width, m.height, r.n * r.n);
        }
    }
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    // bots3 unless `--mind bots2`
    if a.iter().position(|x| x == "--mind").and_then(|i| a.get(i + 1)).is_none_or(|m| m.starts_with("bots3")) {
        bytefield_sim::bots3::use_by_default(true);
    }
    let t = Instant::now();
    s.enable_bots();
    println!("enable_bots: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    for _ in 0..bots {
        s.join(0, true);
    }
    let mut total = 0.0;
    let n = (secs * 30.0) as usize;
    for i in 0..n {
        let t = Instant::now();
        s.step();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        total += ms;
        if ms > threshold {
            println!("step {i} (t={:.1} s): {ms:.1} ms", i as f64 / 30.0);
        }
    }
    println!("{} steps, {:.2} ms on average", n, total / n as f64);
}
