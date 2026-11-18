//! What the `vehicle_class` tests of several layers share.

use bytefield_sim::bot_api::VehicleClass::{self};

pub fn check(mod_name: &str, level: &str, want: &[(&str, VehicleClass)]) {
    let Some(game) = bytefield_testdata::with_mod(mod_name) else { return; };
    let w = bytefield_sim::World::load(&game, mod_name, level, "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    for _ in 0..150 {
        s.step();
    }
    for (template, class) in want {
        let v = s.vehicles.iter().find(|v| v.in_world() && v.template.eq_ignore_ascii_case(template)).unwrap_or_else(|| panic!("no {template} in {level}"));
        assert_eq!(VehicleClass::of(v), *class, "{template}");
    }
}
