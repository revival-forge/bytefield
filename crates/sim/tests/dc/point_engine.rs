//! DC's MD-500 TOW: a point projectile (`DefenderTOW`, no `setHasPointPhysics 0`) with an `Engine` child.
//! In the original's server its engine pushes the point node: 150 m/s at the start, 149.23 at step 5, 361.63 at 90 and
//! 807.46 at 299 (DC_Basrahs_Edge). Skipped without the real data.
#[test]
fn the_tow_speeds_up_as_the_servers() {
    let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
    let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Basrahs_Edge", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let k = s.fire("MD500RocketFirearms", rows, [1000.0, 62.9, 1000.0], Some([0.0; 3])).unwrap();
    let mut speeds = Vec::new();
    for _ in 0..300 {
        let v = s.projectiles[k].vel;
        speeds.push(v.iter().map(|c| c * c).sum::<f32>().sqrt());
        s.step();
    }
    for (n, want) in [(5, 149.23), (30, 188.42), (90, 361.63), (299, 807.46)] {
        assert!((speeds[n] - want).abs() < 0.05, "step {n}: {} m/s, the server {want}", speeds[n]);
    }
}
