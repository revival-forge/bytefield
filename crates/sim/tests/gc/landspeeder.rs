//! GC's landspeeder: like the X-34 its template says `GravityModifier 50`, which object
//! templates don't have; with gravity 1 it rocks on its springs for a second and settles, slightly
//! tilted, where the original does (it used to shake and drift). Skipped without the real data.
#[test]
fn the_landspeeder_settles_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_BCanyon", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    // a reference run of the original: the one placed at (1292.79, 76.31, 1579.6)
    let at = |s: &bytefield_sim::Sim| s.vehicles.iter().find(|v| v.template.eq_ignore_ascii_case("landspeeder") && (v.craft.body.pos[0] - 1293.0).abs() < 3.0).map(|v| v.craft.body.clone()).unwrap();
    for k in 1..=119 {
        s.step();
        let b = at(&s);
        if k == 18 {
            // the deepest tilt of the rocking (0.965 in the original)
            assert!((b.pos[1] - 77.067).abs() < 0.005 && (b.rows[1][1] - 0.965).abs() < 0.002, "{:?} {:?}", b.pos, b.rows);
        }
    }
    let b = at(&s);
    for (p, o) in b.pos.iter().zip([1293.022, 76.789, 1579.361]) {
        assert!((p - o).abs() < 0.005, "{:?}", b.pos);
    }
    assert!((b.rows[1][1] - 0.997).abs() < 0.001, "{:?}", b.rows);
}
