//! An obstacle whose collision reaches past what is drawn: Anzio's merged rails, 39 m drawn and 200 m of collision.
//! Out of the broad phase, the M11-39 crossed them without noticing; on the server it hits them at step 57, from 8.2 m/s to 4.3.
//! Skipped without the real data.
#[test]
fn the_tank_hits_anzios_rails() {
    let Some(game) = bytefield_testdata::with_mod("XPack1") else { return; };
    let w = bytefield_sim::World::load(&game, "XPack1", "Anzio", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let (x, z) = (796.11, 1690.99);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let k = s.spawn_vehicle("M11-39", [x, y, z], [0.0, 0.0, 0.0]).unwrap();
    for _ in 0..60 {
        s.vehicles[k].craft.set_input("c_PIThrottle", 1.0);
        s.step();
    }
    let u = s.vehicles[k].craft.body.vel;
    let speed = u.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!(speed < 6.0, "{speed} m/s at {:?}", s.vehicles[k].craft.body.pos);
}
