//! Smoke: one level of each mod (DC_Final, DesertCombat, GCMOD) loads and advances a few steps without panicking.
//! Skipped without the real data (`data/game`).
use crate::support::mods_smoke::*;

#[test]
fn gcmod_hoth_loads() {
    smoke("GCMOD", "GC_Hoth");
}
