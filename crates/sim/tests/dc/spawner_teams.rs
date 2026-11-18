//! The first vehicle of a spawner with a flag (`setOSId`) belongs to the flag's team, not
//! to its `Object.setTeam`: the original game creates the objects in the
//! first the spawner's frame update handling, after `restartMap`. Checked against the server's
//! census. Skipped without the data.
use crate::support::spawner_teams::*;

/// DC_Basrahs_Edge: the US base's `lighttank` (`setTeam 1`, USspawn flag of
/// team 2) is an M2A3 on the server, not a BMP2.
#[test]
fn basrahs_us_light_tank_is_the_flags() {
    let Some(vs) = vehicles("DC_Final", "DC_Basrahs_Edge") else { return };
    assert_eq!(at(&vs, 749.24, 706.19).map(str::to_ascii_lowercase).as_deref(), Some("m2a3"), "{vs:?}");
}
