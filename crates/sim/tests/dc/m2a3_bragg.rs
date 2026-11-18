//! DC_Operation_Bragg's `lighttank` spawner puts its M2A3 2.8 m under the ground: the contact
//! throws it out, it bounces at ±15 m/s and the two crashes (21.07 against an object, 107.23
//! against the land) destroy it on its 3rd step, in the original too (a reference run of the original,
//! with the round playing). Skipped without the real data.
#[test]
fn the_m2a3_dies_spawning_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("DC_Final") else { return; };
    let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Operation_Bragg", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let i = s.vehicles.iter().position(|v| v.template.eq_ignore_ascii_case("m2a3") && (v.craft.body.pos[0] - 419.3).abs() < 1.0).unwrap();
    // (step, y, vertical speed) from the reference run: thrown out, bouncing
    let original = [(1, 18.4477, 0.0), (2, 16.8361, 14.884), (3, 18.0360, -14.766)];
    for k in 1..=3 {
        s.step();
        let (_, y, vy) = original[k - 1];
        let b = &s.vehicles[i].craft.body;
        assert!((b.pos[1] - y).abs() < 0.01 && (b.vel[1] - vy).abs() < 0.05, "step {k}: {:?} {:?}", b.pos, b.vel);
    }
    let d: Vec<f32> = s.journal.damage_log.iter().filter(|d| d.vehicle == i && d.source == bytefield_sim::damage::Source::Crash).map(|d| d.amount).collect();
    assert_eq!(d.len(), 2, "{d:?}");
    assert!((d[0] - 21.073).abs() < 0.01 && (d[1] - 107.226).abs() < 0.01, "{d:?}");
    assert_eq!(s.vehicles[i].armor.as_ref().map(|a| a.hp), Some(0.0));
}
