//! The first vehicle of a spawner with a flag (`setOSId`) belongs to the flag's team, not
//! to its `Object.setTeam`: the original game creates the objects in the
//! first the spawner's frame update handling, after `restartMap`. Checked against the server's
//! census. Skipped without the data.
use crate::support::spawner_teams::*;

/// Liberation of Caen (bf1942): the German barracks' spawners say `setTeam 0` and their flag
/// belongs to team 1. The server's census (conquest, first frame): these 18 ground and
/// water vehicles.
#[test]
fn caens_spawners_make_their_flags_team() {
    let Some(vs) = vehicles("bf1942", "Liberation_of_Caen") else { return };
    let server: [(&str, f32, f32); 18] = [
        ("Hanomag", 989.6, 1514.9),
        ("KettenKrad", 985.6, 1513.9),
        ("PanzerIV", 961.6, 1518.2),
        ("PanzerIV", 1009.2, 1518.9),
        ("KettenKrad", 1120.9, 854.2),
        ("KettenKrad", 918.1, 989.4),
        ("Hanomag", 980.6, 1250.6),
        ("KettenKrad", 955.3, 1253.2),
        ("Wespe", 1240.6, 1023.4),
        ("PanzerIV", 1242.7, 1013.0),
        ("KettenKrad", 1241.9, 1031.3),
        ("CDNRaft", 1203.3, 1125.6),
        ("CDNRaft", 1207.5, 1125.6),
        ("Sexton", 1000.6, 617.0),
        ("Sherman", 1013.3, 632.9),
        ("Sherman", 989.9, 632.5),
        ("M3A1", 963.6, 630.9),
        ("Lynx", 970.8, 633.1),
    ];
    assert_eq!(vs.len(), server.len(), "{vs:?}");
    for (t, x, z) in server {
        assert_eq!(at(&vs, x, z).map(str::to_ascii_lowercase), Some(t.to_ascii_lowercase()), "{t} at {x}/{z}");
    }
}
