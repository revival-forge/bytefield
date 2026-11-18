//! The tests on Desert Combat (`DC_Final`, and `DesertCombat` with `BYTEFIELD_DC_MOD`): one binary, a module per topic. Each test asks for its layers
//! (`bytefield_testdata::require!`) and is skipped without them.

#[allow(dead_code)]
#[path = "../support/mod.rs"]
mod support;

mod air_gunnery;
mod deployables;
mod dogfight;
mod hand_weapons;
mod helicopters;
mod landing_gear;
mod m2a3_bragg;
mod mods_smoke;
mod off_the_map;
mod placed_pco;
mod point_engine;
mod rigid_hit_damage;
mod rigid_rockets;
mod scout_mark;
mod seat_order;
mod spawner_teams;
mod spring_turn;
mod two_engines;
mod vehicle_class;
