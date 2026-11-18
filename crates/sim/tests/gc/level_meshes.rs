//! A level's own meshes: GC_Alaris_Prime's speederbike names
//! `Vehicles/Speederbike/Speederbike1_bb`, which only the level brings
//! (`bf1942/levels/GC_Alaris_Prime/StandardMesh/`). Without it its root had no box: no contact
//! mesh and no floaters (they need their node's box), so it fell into the ground and blew up on
//! its 14th step. Now it hovers and settles where the original does. Skipped without the real data.
#[test]
fn the_speederbike_hovers_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Alaris_Prime", "objectivemode").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    // a reference run of the original: the one at (562.52, 220.5, 744.1)
    let i = s.vehicles.iter().position(|v| v.template.eq_ignore_ascii_case("speederbike") && (v.craft.body.pos[0] - 562.52).abs() < 1.0).unwrap();
    // (step, y) from the reference run (its record n is our step n + 1): the top of the rise, on the way
    // down, on the springs, settled
    let original = [(9, 220.5554), (31, 220.0952), (41, 219.9297), (91, 219.9406)];
    for k in 1..=91 {
        s.step();
        if let Some(&(_, y)) = original.iter().find(|o| o.0 == k) {
            let b = &s.vehicles[i].craft.body;
            assert!((b.pos[1] - y).abs() < 0.001, "step {k}: {:?}", b.pos);
        }
    }
    assert_eq!(s.vehicles[i].armor.as_ref().map(|a| a.hp), Some(15.0));
}

/// GC_Podrace's R2D2 is placed 0.6 m over the ground with 20 hit points: it lands at 3.9 m/s and
/// the crash (20.23) destroys it on its 10th step, in the original too
/// (a reference run of the original, with the round playing).
#[test]
fn the_r2d2_dies_landing_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Podrace", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let i = s.vehicles.iter().position(|v| v.template.eq_ignore_ascii_case("r2d2") && (v.craft.body.pos[0] - 826.47).abs() < 1.0).unwrap();
    for _ in 0..9 {
        s.step();
    }
    assert_eq!(s.vehicles[i].armor.as_ref().map(|a| a.hp), Some(20.0));
    s.step();
    let d = s.journal.damage_log.iter().find(|d| d.vehicle == i).expect("the landing's damage");
    assert!((d.amount - 20.229).abs() < 0.01, "{}", d.amount);
    assert_eq!(s.vehicles[i].armor.as_ref().map(|a| a.hp), Some(0.0));
}

/// GC_Ryloth's TIE fighters wait on a hangar floor that is one of the level's own meshes: without
/// it they fell 60 m, crashed and burnt upside down. On it they rise a little and settle where the
/// original does.
#[test]
fn the_tie_fighter_waits_on_its_hangar_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Ryloth", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let i = s.vehicles.iter().position(|v| v.template.eq_ignore_ascii_case("tiefighter") && (v.craft.body.pos[0] - 768.53).abs() < 1.0).unwrap();
    // (step, y): the top of the rise, settled
    let original = [(3, 73.8546), (30, 73.7534), (149, 73.7534)];
    for k in 1..=150 {
        s.step();
        if let Some(&(_, y)) = original.iter().find(|o| o.0 == k) {
            let b = &s.vehicles[i].craft.body;
            assert!((b.pos[1] - y).abs() < 0.001, "step {k}: {:?}", b.pos);
        }
    }
    assert_eq!(s.vehicles[i].armor.as_ref().map(|a| a.hp), Some(55.0));
}
