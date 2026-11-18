//! The vehicles placed in a level's scripts, not by a spawner:
//! DC_First_Light's mortar, GC_BCanyon's medical bed, GC's E-Webs. The server makes them as any other
//! `PlayerControlObject` (a reference run of the original: `createObject` and the `Pco`
//! list); they used to be obstacles only. Skipped without the real data.
use crate::support::placed_pco::*;

#[test]
fn dc_first_light_mortar_is_a_vehicle() {
    placed("DC_Final", "DC_First_Light", "mortar", [817.42, 73.64, 1449.85], Some([-0.515, 0.0, 0.857]));
}
