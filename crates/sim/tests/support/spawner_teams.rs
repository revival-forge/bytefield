//! What the `spawner_teams` tests of several layers share.

pub fn vehicles(mod_name: &str, level: &str) -> Option<Vec<(String, [f32; 3])>> {
    let Some(game) = bytefield_testdata::with_mod(mod_name) else { return None; };
    let w = bytefield_sim::World::load(&game, mod_name, level, "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let vs = s.start_conquest();
    // the census has the mobile ones (rigid nodes), not the fixed guns
    Some(vs.iter().filter(|&&i| w.is_mobile(&s.vehicles[i].template)).map(|&i| (s.vehicles[i].template.clone(), s.vehicles[i].craft.body.pos)).collect())
}

pub fn at(vs: &[(String, [f32; 3])], x: f32, z: f32) -> Option<&str> {
    vs.iter().find(|(_, p)| (p[0] - x).abs() < 1.0 && (p[2] - z).abs() < 1.0).map(|(t, _)| t.as_str())
}
