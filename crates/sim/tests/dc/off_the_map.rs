//! Off the map the terrain repeats: a Humvee dropped there rests where
//! the original's server leaves it. Skipped without the real data.
#[test]
fn a_humvee_rests_on_the_terrain_past_the_edge() {
    let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
    let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Cornered", "conquest").unwrap();
    // (where it was dropped from 250 m; the height it was at in the original's reference run, 250 steps later)
    for (x, z, y) in [(-50.0, 500.0, 84.4), (2098.0, 500.0, 96.5), (500.0, -50.0, 71.5), (500.0, 2098.0, 73.7)] {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        let k = s.spawn_vehicle("humvee_mk19", [x, 250.0, z], [0.0, 0.0, 0.0]).unwrap();
        for _ in 0..250 {
            s.step();
        }
        let at = s.vehicles[k].craft.body.pos;
        assert!((at[1] - y).abs() < 2.5, "dropped at {x}, {z}: at {at:?}, the server's height {y}");
    }
}
