//! What the `pathgen_vs_shipped` tests of several layers share.


pub fn world(mod_name: &str, level: &str) -> Option<bytefield_sim::World> {
    let Some(game) = bytefield_testdata::with_mod(mod_name) else { return None; };
    Some(bytefield_sim::World::load(&game, mod_name, level, "conquest").unwrap())
}
