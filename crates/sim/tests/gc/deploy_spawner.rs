//! A rigid projectile does not run into what its own spawners made: GC's landing craft drops a `Speederbike_Deploy`
//! whose spawner makes the speeder bike where it is at once; the original's falls through it and ends on the ground
//! (100 steps from 60 m up), not on the bike in its second.
#[test]
fn the_deployed_speeder_bike_falls_to_the_ground() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let y = w.height_at(1000.0, 1000.0).unwrap() as f32 + 60.0;
    let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let k = s.fire_barrel("Imperial_LandingCraft_Speederbike", rows, [1000.0, y, 1000.0], Some([0.0; 3]), -1).unwrap();
    let mut ended = None;
    for n in 0..150 {
        if s.projectiles[k].end.is_some() {
            ended = Some(n);
            break;
        }
        s.step();
    }
    let n = ended.expect("it never ended");
    assert!(n > 90, "ended at step {n}, in the air");
}
