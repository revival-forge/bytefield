//! The tests on the base game (`bf1942`): one binary, a module per topic. Each test asks for its layers
//! (`bytefield_testdata::require!`) and is skipped without them.

#[allow(dead_code)]
#[path = "../support/mod.rs"]
mod support;

mod air_gunner_sbd;
mod bench_doc_example;
mod bots_memory;
mod cover;
mod exposed_gunner;
mod kill_material;
mod lightmaps;
mod meter_last_shot;
mod pco_near_human;
mod rigid_hit_damage;
mod round_share;
mod round_events;
mod scout_mark;
mod navmesh;
mod soldier_step;
mod walker_mesh;
mod sonar;
mod spawn_groups_only_for;
mod spawner_teams;
mod vehicle_class;
