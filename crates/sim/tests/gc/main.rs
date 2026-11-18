//! The tests on Galactic Conquest (`GCMOD`): one binary, a module per topic. Each test asks for its layers
//! (`bytefield_testdata::require!`) and is skipped without them.

#[allow(dead_code)]
#[path = "../support/mod.rs"]
mod support;

mod air_gunnery;
mod air_gunner_seat;
mod vtol_lift;
mod atat_rest;
mod bespin_spawns;
mod capsule;
mod deploy_spawner;
mod deployables;
mod gain135;
mod lambda_front_wheel;
mod landspeeder;
mod level_meshes;
mod mods_smoke;
mod part_contacts;
mod placed_pco;
mod podracer;
mod rigid_rockets;
mod shield;
mod walkers;
mod weapons;
mod wreck_springs;
