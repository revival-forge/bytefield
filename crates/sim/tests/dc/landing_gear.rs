//! `LandingGear`: DC's F-14B hangs its
//! gear under the engines (`gearDownHeight 35`, `gearUpHeight 33`, `gearDownEngineInput 0.4`,
//! `gearUpEngineInput 0.7`). It folds up over 33 m with the engine at 0.7 or more, and comes down
//! under 35 m with it at 0.4 or less; the front wheels turn with it into the body. Skipped without
//! the real data.
use bytefield_sim::craft::Kind;

#[test]
fn the_f14_folds_its_gear_up_in_the_air() {
    for (_, w) in crate::support::dc_worlds("DC_No_Fly_Zone", "conquest") {
        gear_folds(&w);
    }
}

fn gear_folds(w: &bytefield_sim::World) {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    let i = s.spawn_vehicle("F-14B", [1000.0, 100.0, 1000.0], [0.0; 3]).unwrap();
    let mut v = s.vehicles[i].clone();
    v.set_engines_on(true);
    let gear = |v: &bytefield_sim::mover::Vehicle| {
        let k = v.craft.parts.iter().position(|p| p.name.eq_ignore_ascii_case("F14BFrontGear")).unwrap();
        let Kind::LandingGear(g) = &v.craft.parts[k].kind else { panic!() };
        (g.up, v.craft.parts[k].bundle.as_ref().unwrap().angle[1])
    };
    let y = v.craft.body.pos[1];
    let run = |v: &mut bytefield_sim::mover::Vehicle, over: f32, throttle: f32, steps: usize| {
        v.craft.inputs[bytefield_sim::soldier::axis::THROTTLE] = throttle;
        for _ in 0..steps {
            v.craft.update_parts(1.0 / 30.0, &|_, _| Some(y - over));
        }
    };
    // on the ground at full throttle: down
    run(&mut v, 2.0, 1.0, 150);
    assert_eq!(gear(&v), (false, 0.0));
    // 100 m up: up, folded to −90° at 180°/s² (top speed 180°/s)
    let wheel = |v: &bytefield_sim::mover::Vehicle| v.contacts.iter().filter(|c| c.name.to_ascii_lowercase().starts_with("f14bfrontwhe")).map(|c| c.pos[1]).fold(f32::NAN, f32::max);
    let low = wheel(&v);
    run(&mut v, 100.0, 1.0, 150);
    assert_eq!(gear(&v), (true, -90.0));
    // the front wheel goes up into the body
    v.place_contacts();
    assert!(wheel(&v) > low + 0.5, "{low} -> {}", wheel(&v));
    // throttle off, still high: stays up
    run(&mut v, 100.0, 0.0, 150);
    assert!(gear(&v).0);
    // low with the throttle off: down again
    run(&mut v, 10.0, 0.0, 150);
    assert_eq!(gear(&v), (false, 0.0));
}
