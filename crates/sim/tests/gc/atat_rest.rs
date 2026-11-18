//! GC's AT-AT: its back springs hang at `setRotation 180/0/0` in their engine. Offset by their own axes
//! instead of the parent's, the rest point fell 34,8 m away and the empty walker flew off at 0,8 s and broke (from the
//! 323's chain, `contact::Spring::turn`). Empty, it stands still and upright. Skipped without the real data (`data/game`).
#[test]
fn the_empty_atat_stands_still() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let start: Vec<_> = s.vehicles.iter().filter(|v| v.template.eq_ignore_ascii_case("atat")).map(|v| v.craft.body.pos).collect();
    assert!(!start.is_empty());
    for _ in 0..300 {
        s.step();
    }
    for (v, p0) in s.vehicles.iter().filter(|v| v.template.eq_ignore_ascii_case("atat")).zip(start) {
        let b = &v.craft.body;
        assert!(b.rows[1][1] > 0.99, "{:?}", b.rows);
        let moved = ((b.pos[0] - p0[0]).powi(2) + (b.pos[2] - p0[2]).powi(2)).sqrt();
        assert!(moved < 1.0 && (b.pos[1] - p0[1]).abs() < 2.0, "{:?} -> {:?}", p0, b.pos);
        assert_eq!(v.armor.as_ref().map(|a| a.hp), Some(850.0));
    }
}
