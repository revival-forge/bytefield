//! GC's podracer: it hovers on four `FloatingBundle`s with `hullHeight 2500` that hang
//! from `PodRacerComplex`, a RotationalBundle with no `drag`. A floater damps with its parent
//! node's drag (0 there), so nothing damps it: it rises, comes back down and settles on its
//! springs where the original does (it used to bounce and crash on its 3rd step). Skipped
//! without the real data.
#[test]
fn the_podracer_hovers_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Podrace", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    // a reference run of the original: the one placed at (863.96, 191.57, 235.77)
    let i = s.vehicles.iter().position(|v| v.template.eq_ignore_ascii_case("podracerv1") && (v.craft.body.pos[0] - 863.96).abs() < 1.0).unwrap();
    // (step, y) from the reference run: the top of the rise, on the way down, settled on the springs
    let original = [(30, 193.876), (60, 192.152), (90, 191.988), (110, 191.987)];
    for k in 1..=110 {
        s.step();
        if let Some(&(_, y)) = original.iter().find(|o| o.0 == k) {
            let b = &s.vehicles[i].craft.body;
            assert!((b.pos[1] - y).abs() < 0.005, "step {k}: {:?}", b.pos);
        }
    }
    assert_eq!(s.vehicles[i].armor.as_ref().map(|a| a.hp), Some(500.0));
}
