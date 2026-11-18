//! Which throttle lifts GC's aircraft straight up off the ground ([`VehicleClass::lift_throttle`]): what bots3's VTOL takeoff
//! reads. Skipped without the real data.
use bytefield_sim::bot_api::VehicleClass;

/// Their racks lift with the throttle pulled back (`acceleration` < 0 on what the throttle drives); the Falcon's and the gunships'
/// engines lift with it pushed; a speeder bike floats, and is not lifted (`examples/air_envelope … --hover` measured each).
#[test]
fn gc_aircraft_lift_with_the_throttle_their_engines_say() {
    let game = bytefield_testdata::require!(Gc);
    for (level, expected) in [
        ("GC_Hoth", &[("snowspeeder", -1.0), ("x-wing", -1.0), ("tiefighter", -1.0), ("milleniumfalcon", 1.0)][..]),
        ("GC_Bespin", &[("cloudcar", -1.0)][..]),
        ("GC_Corellia", &[("impgunship", 1.0), ("speederbike", 0.0)][..]),
    ] {
        let w = bytefield_sim::World::load(&game, "GCMOD", level, "conquest").unwrap();
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        for &(template, lift) in expected {
            let c = s.spawn_vehicle(template, [1024.0, 2000.0, 1024.0], [0.0; 3]).unwrap_or_else(|| panic!("{level}: no {template}"));
            assert_eq!(VehicleClass::lift_throttle(&s.vehicles[c]), lift, "{level}: {template}");
        }
    }
}

/// Which of them fly hanging by that lift, as a helicopter ([`VehicleClass::hangs_of`]): the cloud car pushes too little to fly on
/// its wings (it falls with the throttle forward), the gunships lift with the throttle pushed; the snowspeeder and the X-Wing fly as
/// planes once off the ground (`examples/air_envelope … --hover`, `--cruise`).
#[test]
fn the_cloud_car_and_the_gunships_fly_hanging_the_fighters_on_their_wings() {
    let game = bytefield_testdata::require!(Gc);
    for (level, expected) in [("GC_Hoth", &[("snowspeeder", false), ("x-wing", false), ("tiefighter", false)][..]), ("GC_Bespin", &[("cloudcar", true)][..]), ("GC_Corellia", &[("impgunship", true), ("awing", false)][..])] {
        let w = bytefield_sim::World::load(&game, "GCMOD", level, "conquest").unwrap();
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        for &(template, hangs) in expected {
            let c = s.spawn_vehicle(template, [1024.0, 2000.0, 1024.0], [0.0; 3]).unwrap_or_else(|| panic!("{level}: no {template}"));
            let v = &s.vehicles[c];
            assert_eq!(VehicleClass::hangs_of(v, VehicleClass::lift_throttle(v)), hangs, "{level}: {template}");
        }
    }
}

/// A bots3 pilot in one of GC_Bespin's cloud cars, parked on a platform of the city over a void: it rises, flies hanging and keeps
/// to the city's height (before, it put the throttle forward as a plane, and fell into the clouds).
#[test]
fn a_bots3_pilot_flies_a_cloud_car_off_its_platform_and_keeps_to_the_citys_height() {
    let game = bytefield_testdata::require!(Gc);
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Bespin", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.set_minds(1, bytefield_sim::bots3::maker(&w.level.name, 1));
    let k = s.join(1, true);
    let mut soldier = None;
    for _ in 0..900 {
        s.step();
        if let Some(p) = s.players[k].soldier.filter(|_| s.players[k].alive) {
            soldier = Some(p);
            break;
        }
    }
    let soldier = soldier.expect("the soldier never spawned");
    let car = (0..s.vehicles.len()).find(|&j| s.vehicles[j].template.eq_ignore_ascii_case("cloudcar") && s.vehicles[j].in_world()).expect("no cloud car");
    let start = s.vehicles[car].craft.body.pos;
    s.enter(soldier, car, 0);
    let (mut lowest, mut furthest) = (f32::MAX, 0.0f32);
    for step in 0..(60 * 30) {
        s.step();
        let p = s.vehicles[car].craft.body.pos;
        if step > 15 * 30 {
            lowest = lowest.min(p[1]);
        }
        furthest = furthest.max((p[0] - start[0]).hypot(p[2] - start[2]));
        assert!(!s.vehicles[car].armor.as_ref().is_some_and(|a| a.destroyed), "the cloud car was destroyed at {:.0} s", step as f32 / 30.0);
    }
    assert_eq!(s.vehicles[soldier].entry.inside, Some(car), "the pilot left the cloud car");
    assert!(furthest > 200.0, "it went only {furthest:.0} m from where it stood");
    assert!(lowest > start[1] - 40.0, "it sank to {lowest:.0} m from the platform at {:.0} m", start[1]);
}
