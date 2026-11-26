//! What a round of a hand weapon takes off a vehicle of Berlin, as the bots read it to choose a target
//! ([`bytefield_sim::Sim::round_share`], from the `.con` damage table): a rifle, a sub-machine gun or a pistol does nothing to a
//! tank or a half-track, a little to a car; a grenade's blast and a bazooka hurt them all. Skipped without the real data.

#[test]
fn a_rifle_does_nothing_to_a_tank_and_a_grenade_does() {
    if !crate::support::has_level("bf1942", "Berlin") {
        eprintln!("no Berlin, skipped");
        return;
    }
    let w = bytefield_sim::World::load(&crate::support::game(), "bf1942", "Berlin", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let (x, z) = (1000.0f32, 1000.0f32);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 0.5;
    let best = |s: &bytefield_sim::Sim, g: &str, j: usize| s.round_share(g, j).map(|x| x[0].max(x[2])).unwrap_or_else(|| panic!("{g}: no share"));
    for t in ["T34-85", "Tiger", "Sherman", "M3A1"] {
        let j = s.spawn_vehicle(t, [x, y, z], [0.0; 3]).unwrap_or_else(|| panic!("a {t}"));
        for g in ["K98", "Thompson", "MP40", "Colt"] {
            assert_eq!(best(&s, g, j), 0.0, "{g} against the {t}");
        }
        assert!(best(&s, "GrenadeAllies", j) >= 0.15, "a grenade against the {t}");
        assert!(best(&s, "Bazooka", j) >= 0.25, "a bazooka against the {t}");
    }
    let car = s.spawn_vehicle("Kubelwagen", [x + 20.0, y, z], [0.0; 3]).expect("a Kubelwagen");
    for g in ["K98", "Thompson", "MP40", "Colt"] {
        assert!(best(&s, g, car) >= 0.02, "{g} against the Kubelwagen");
    }
}
