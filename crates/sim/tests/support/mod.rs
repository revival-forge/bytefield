//! What the simulation's tests on the real data share, across the layer binaries (`base/`, `xpack/`, `dc/`, `gc/`):
//! the Desert Combat versions to run on (switching versions costs validation, not code), and one module per topic
//! whose tests live in several layers.
use std::path::PathBuf;

pub fn game() -> PathBuf {
    bytefield_testdata::game()
}

/// The DC versions the DC tests run on ([`bytefield_testdata::dc_mods`]); none available: the test is skipped.
pub fn dc_mods() -> Vec<String> {
    let mods = bytefield_testdata::dc_mods();
    if mods.is_empty() {
        bytefield_testdata::skipped("dc");
    }
    mods
}

/// Whether the mod (with its chain) has the level.
pub fn has_level(mod_name: &str, level: &str) -> bool {
    bytefield_con::session::mount(&game(), mod_name).is_ok_and(|(_, vfs)| vfs.exists(&format!("bf1942/levels/{level}/init.con")))
}

/// The level loaded in each DC version of [`dc_mods`] that has it, with the version's name. A version without it is
/// skipped, and said on stderr.
pub fn dc_worlds(level: &str, mode: &str) -> Vec<(String, bytefield_sim::World)> {
    dc_mods()
        .into_iter()
        .filter(|m| {
            let has = has_level(m, level);
            if !has {
                eprintln!("{m}: no {level}, skipped");
            }
            has
        })
        .map(|m| {
            let w = bytefield_sim::World::load(&game(), &m, level, mode).unwrap_or_else(|e| panic!("{m} {level}: {e}"));
            (m, w)
        })
        .collect()
}
pub mod air_gunnery;
pub mod mods_smoke;
pub mod placed_pco;
pub mod rigid_hit_damage;
pub mod scout_mark;
pub mod spawner_teams;
pub mod vehicle_class;
