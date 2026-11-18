//! A destroyed vehicle's springs keep a quarter of their strength and damping (the spring's message handling): the
//! original's reference run of GC's skiff wrecked on GC_BCanyon shows its `Skiff_Wheel` nodes at 6/3 from the frame it is
//! destroyed (24/12 in its template).
#[test]
fn a_wreck_keeps_a_quarter_of_its_springs() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_BCanyon", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let (x, z) = (1449.85, 1060.42);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let k = s.spawn_vehicle("skiff", [x, y, z], [0.0; 3]).unwrap();
    let springs = |s: &bytefield_sim::Sim| -> Vec<(f32, f32)> { s.vehicles[k].contacts.iter().filter_map(|c| c.spring.as_ref().map(|sp| (sp.strength, sp.damping))).collect() };
    let before = springs(&s);
    assert_eq!(before.len(), 4);
    assert!(before.iter().all(|&p| p == (24.0, 12.0)), "{before:?}");
    let max = s.vehicles[k].armor.as_ref().unwrap().max_hp;
    s.give_damage(k, max + 1.0, bytefield_sim::damage::Source::Hit);
    assert!(springs(&s).iter().all(|&p| p == (6.0, 3.0)), "{:?}", springs(&s));
    // a hit on the wreck gives nothing (the armor is destroyed): the springs stay
    s.give_damage(k, 10.0, bytefield_sim::damage::Source::Hit);
    assert!(springs(&s).iter().all(|&p| p == (6.0, 3.0)));
}
