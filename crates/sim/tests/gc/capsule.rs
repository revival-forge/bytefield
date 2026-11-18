//! GC's droid capsule: its template says `setHasResponsePhysics 0`, but the server
//! gives every object's root a response (`SimpleObject` calls `setResponsePhysicsComponent`
//! always), so it rests on the ground instead of falling through and drowning. Skipped without
//! the real data (`data/game`).
#[test]
fn the_droid_capsule_rests_on_the_ground_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    for _ in 0..300 {
        s.step();
    }
    let capsules: Vec<_> = s.vehicles.iter().filter(|v| v.template.eq_ignore_ascii_case("droidcapsule")).collect();
    assert!(!capsules.is_empty());
    for v in capsules {
        let b = &v.craft.body;
        assert!(b.rows[1][1] > 0.99, "{:?}", b.rows);
        assert_eq!(v.armor.as_ref().map(|a| a.hp), Some(10.0), "{}: {:?}", v.template, b.pos);
        // the one at (1868.23, 538.14), placed at 31.6, rests at 30.613 in the original from its
        // 12th update on
        if (b.pos[0] - 1868.23).abs() < 1.0 {
            assert!((b.pos[1] - 30.613).abs() < 0.05, "{:?}", b.pos);
        }
    }
}
