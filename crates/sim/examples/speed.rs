//! How fast the bench runs a conquest round: the level's vehicles and `players` bots (half a
//! team each) running forward with their kits, `steps` steps of 1/30 s.
//! `cargo run --release -p bytefield-sim --example speed -- data/game bf1942 El_Alamein [players] [steps]`
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let players: usize = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(32);
    let steps: usize = a.get(5).and_then(|v| v.parse().ok()).unwrap_or(3000);
    let t = Instant::now();
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    println!("load {:.2} s", t.elapsed().as_secs_f64());
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let vehicles = s.start_conquest().len();
    for i in 0..players {
        let team = 1 + (i % 2) as u32;
        s.add_player(team, if team == 1 { "GermanSoldier" } else { "USSoldier" }, true);
    }
    // a first step spawns them; then they run
    s.step();
    for p in s.players.clone() {
        if let Some(v) = p.soldier {
            s.vehicles[v].set_move(1.0, 0.0, 0, false);
        }
    }
    let t = Instant::now();
    for _ in 0..steps {
        s.step();
    }
    let e = t.elapsed().as_secs_f64();
    let alive = s.players.iter().filter(|p| p.alive).count();
    println!(
        "{} vehicles, {} players ({} alive), {} objects; {} steps in {:.2} s: {:.0} steps/s, {:.0}× real time",
        vehicles,
        players,
        alive,
        s.vehicles.iter().filter(|v| !v.removed).count(),
        steps,
        e,
        steps as f64 / e,
        steps as f64 / 30.0 / e
    );
}
