//! GC's rockets and bombs without `hasMobilePhysics`: they stayed in the air where they were fired, for all their
//! `timeToLive`. The original's server has this one at z 1126 at step 66 and 5 m lower at step 30. Skipped without the real data.
#[test]
fn a_t3b_rocket_flies() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_BCanyon", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let k = s.fire("Assault_Tank_Rocket_Launcher", rows, [1000.0, 178.5, 1000.0], None).unwrap();
    for _ in 0..66 {
        s.step();
    }
    let at = s.projectiles[k].pos;
    assert!((at[2] - 1126.7).abs() < 3.0 && (at[1] - 152.0).abs() < 4.0, "at {at:?}");
}
