//! Smoke: one level of each mod (DC_Final, DesertCombat, GCMOD) loads and advances a few steps without panicking.
//! Skipped without the real data (`data/game`).
use crate::support::mods_smoke::*;

#[test]
fn dc_final_basrahs_edge_loads() {
    smoke("DC_Final", "DC_Basrahs_Edge");
}

/// DC 0.7 on its own.
#[test]
fn desert_combat_07_basrahs_edge_loads() {
    smoke("DesertCombat", "DC_Basrahs_Edge");
}
