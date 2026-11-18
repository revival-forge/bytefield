//! DC's Ural tanker: two engines, one hanging from the other with its own wheels, and left wheels hung at
//! 179.999° from their steering. Each wheel asks the engine above it and pulls along its parent's z (`addFriction`), or
//! the truck stalls on this slope at 3 m/s; the original's server is there at 9. Skipped without the real data.
#[test]
fn the_tanker_climbs_the_slope_at_the_docks() {
    let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
    let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Al_Khafji_Docks", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let (x, z) = (992.82, 468.47);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let k = s.spawn_vehicle("UralTanker", [x, y, z], [0.0, 0.0, 0.0]).unwrap();
    s.vehicles[k].set_engines_on(true);
    for _ in 0..100 {
        s.vehicles[k].craft.set_input("c_PIThrottle", 1.0);
        s.step();
    }
    let b = &s.vehicles[k].craft.body;
    // (the original's reference run at step 100: 991.5, 84.5, 487.4, at 9.2 m/s)
    let speed = b.vel.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!(speed > 8.0 && b.pos[2] > 486.0 && (b.pos[0] - 991.5).abs() < 1.5, "at {:?}, {speed} m/s", b.pos);
}
