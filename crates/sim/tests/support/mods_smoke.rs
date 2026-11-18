//! What the `mods_smoke` tests of several layers share.

pub fn smoke(mod_name: &str, level: &str) {
    let Some(game) = bytefield_testdata::with_mod(mod_name) else { return; };
    let w = bytefield_sim::World::load(&game, mod_name, level, "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    for _ in 0..30 {
        s.step();
    }
    assert!(s.modes.conquest.is_some());
}
