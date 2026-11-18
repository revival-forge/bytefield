//! The tests on the expansions (`XPack1`, `XPack2`): one binary, a module per topic. Each test asks for its layers
//! (`bytefield_testdata::require!`) and is skipped without them.

#[allow(dead_code)]
#[path = "../support/mod.rs"]
mod support;

mod obstacle_reach;
