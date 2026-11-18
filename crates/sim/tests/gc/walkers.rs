//! GC's walkers against the original's reference runs of them. Skipped without the real data.

/// One vehicle made alone and driven as `examples/phys_diff` does (the creation frame, then the inputs each step).
fn drive(s: &mut bytefield_sim::Sim, k: usize, steps: usize) {
    for _ in 0..steps {
        s.vehicles[k].craft.set_input("c_PIThrottle", 1.0);
        s.vehicles[k].craft.set_input("c_PIYaw", 0.5);
        s.step_ordered(true);
    }
}

fn alone<'a>(w: &'a bytefield_sim::World, template: &str, at: [f32; 3]) -> (bytefield_sim::Sim<'a>, usize) {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let k = s.spawn_vehicle(template, at, [0.0; 3]).expect("the template");
    s.vehicles[k].set_engines_on(true);
    s.creation_frame();
    s.vehicles[k].craft.push_floats();
    (s, k)
}

/// A part is placed in the world link by link down its parents, rounding to f32 at each (the original's chain of
/// matrices), and its vertices and the point its solve pushes at start from there. Placed in one go from the root, the
/// AT-ST's right foot (four bundles down the leg) fell one and two ulps of the world away, 0.06 and 0.12 mm: its first
/// touch (step 10) turned the walker 2.6e-4 off about x and z.
#[test]
fn the_atst_turns_from_its_first_touch_as_the_original() {
    let game = bytefield_testdata::require!(Gc);
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Endor", "conquest").unwrap();
    let (mut s, k) = alone(&w, "atstendor", [757.17, 114.152, 872.13]);
    let foot = s.vehicles[k].contacts.iter().find(|c| c.name.eq_ignore_ascii_case("ATSTRFoot")).expect("the right foot");
    assert!(foot.chain.len() >= 4, "the foot hangs {} links down", foot.chain.len());
    // the original's `rotvel` at record 11, after the foot's first touch
    drive(&mut s, k, 11);
    let (got, want) = (s.vehicles[k].craft.body.rotvel, [-0.22955586f32, 0.005373716, 0.671176732]);
    assert!((0..3).all(|i| (got[i] - want[i]).abs() < 1e-6), "step 11: {got:?}");
}

/// A part with engine grip asks its engine for the ground's speed, and the original looks for that engine going up the
/// part's parents: with none above, it asks for no friction at all. The AT-AT hangs its feet (`c_PGFEngineGrip`) from the
/// leg bundles, next to `atatdedummyengine` and not under it: standing on them it neither rubs nor turns, until the
/// engine's own dummy springs reach the ground (step 12 of the reference run).
#[test]
fn the_atat_feet_do_not_rub() {
    let game = bytefield_testdata::require!(Gc);
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").unwrap();
    let y = w.height_at(1394.9, 1837.0).unwrap() as f32 + 10.3;
    let (mut s, k) = alone(&w, "atat", [1394.9, y, 1837.0]);
    for c in &s.vehicles[k].contacts {
        let foot = c.name.eq_ignore_ascii_case("footl") || c.name.eq_ignore_ascii_case("footr");
        let dummy = c.name.to_ascii_lowercase().starts_with("atatdedummy");
        assert!(!(foot && c.engine_above) && (!dummy || c.engine_above), "{}: engine above {}", c.name, c.engine_above);
    }
    // on its feet alone the root does not turn (the original's rotvel is 0 through record 11)
    drive(&mut s, k, 8);
    let b = &s.vehicles[k].craft.body;
    let turn = b.rotvel.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(turn < 1e-5, "step 8: rotvel {:?}", b.rotvel);
}
