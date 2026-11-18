//! The tests on the base game (`bf1942`): one binary, a module per topic. Each test asks for its layers
//! (`bytefield_testdata::require!`) and is skipped without them.

#[allow(dead_code)]
#[path = "../support/mod.rs"]
mod support;

mod air_gunner_sbd;
mod bench_doc_example;
mod kill_material;
mod lightmaps;
mod pco_near_human;
mod rigid_hit_damage;
mod scout_mark;
mod sonar;
mod spawn_groups_only_for;
mod spawner_teams;
mod vehicle_class;
