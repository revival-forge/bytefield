//! The example of `docs/dev/headless-bench.md`, kept compiling and running. Skipped without the real data.
#[test]
fn the_documented_bench_example_runs() {
    if !crate::support::has_level("bf1942", "El_Alamein") {
        return;
    }
    let w = bytefield_sim::World::load(&crate::support::game(), "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let jeep = s.spawn_vehicle("Willy", [1000.0, 30.0, 1200.0], [0.0, 0.0, 0.0]).unwrap();
    for _ in 0..300 {
        s.step();
    }
    assert!(s.vehicles[jeep].craft.body.pos.iter().all(|c| c.is_finite()));
}
