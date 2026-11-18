//! A spring's node stays in its parent's axes, whatever its own `setRotation` under `addTemplate`. DC's Urals and BM21
//! hang the left front wheel at -179.999/0/0: the original checks its mesh against the ground unmirrored (the contact
//! point of the original's first record, to the micrometre, on the Ural reference runs); mirrored, it
//! touched 1 mm off and every run of those trucks drifted apart from the first step. Skipped without the real data.
#[test]
fn a_turned_spring_checks_the_ground_in_its_parents_axes() {
    if !crate::support::has_level("DC_Final", "DC_Oil_Fields") {
        return;
    }
    let w = bytefield_sim::World::load(&crate::support::game(), "DC_Final", "DC_Oil_Fields", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let k = s.spawn_vehicle("ural_fueldrum", [1612.4, 20.0, 783.37], [0.0, 0.0, 0.0]).expect("the template");
    let v = &s.vehicles[k];
    let left = v.contacts.iter().find(|c| c.name.eq_ignore_ascii_case("Uralfueldrum_Spring_FrontL")).expect("the left front wheel");
    let right = v.contacts.iter().find(|c| c.name.eq_ignore_ascii_case("Uralfueldrum_Spring_FrontR")).expect("the right front wheel");
    assert!(left.spring.as_ref().and_then(|s| s.turn).is_some(), "the left wheel hangs turned");
    // both wheels in the truck's axes (the left one not mirrored about y)
    for c in [left, right] {
        assert!(c.rows[0][0] > 0.999 && c.rows[2][2] > 0.999, "{} {:?}", c.name, c.rows);
    }
}
