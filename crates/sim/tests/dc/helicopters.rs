//! DC's light helicopters: their box is the fuselage's, as
//! `findLodGeometry` picks it, and an empty one stays upright on its skids. Skipped without the
//! real data (`data/game`).
/// (DC_Final's: the numbers are its server's)
fn world(level: &str) -> Option<bytefield_sim::World> {
    let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return None; };
    Some(bytefield_sim::World::load(&game, "DC_Final", level, "conquest").unwrap())
}

#[test]
fn the_box_comes_from_the_first_dist_compare_lod() {
    for (_, w) in crate::support::dc_worlds("DC_Basrahs_Edge", "conquest") {
        boxes(&w);
    }
}

fn boxes(w: &bytefield_sim::World) {
    // lodMH6 (DistCompareSelector2) holds a bundle without geometry; lodH6Cockpit
    // (DistCompareSelector) the fuselage. Before, the control stick's 9 × 71 × 29 cm.
    assert_eq!(w.physics_geometry("MH-6").as_deref(), Some("H6_Fus_M1"));
    assert_eq!(w.physics_geometry("AH-6").as_deref(), Some("H6_Fus_M1"));
    assert_eq!(w.physics_geometry("MD-500").as_deref(), Some("MD500_Fus_M1"));
    // vanilla's way: the first child is the LodObject and its first LOD has the hull
    assert_eq!(w.physics_geometry("SA-342S").as_deref(), Some("SA342_Fus_M1"));
}

#[test]
fn an_empty_mh6_stays_on_its_skids() {
    let Some(w) = world("DC_Basrahs_Edge") else { return };
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    for _ in 0..300 {
        s.step();
    }
    let helis: Vec<_> = s.vehicles.iter().filter(|v| v.template.eq_ignore_ascii_case("MH-6")).collect();
    assert!(!helis.is_empty());
    for v in helis {
        let b = &v.craft.body;
        // the original's settles at 28.48 m
        assert!(b.rows[1][1] > 0.99, "{:?}", b.rows);
        assert!((b.pos[1] - 28.48).abs() < 0.1, "{:?}", b.pos);
        assert_eq!(v.armor.as_ref().map(|a| a.hp), Some(100.0));
    }
}
