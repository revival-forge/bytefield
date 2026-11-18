//! A jet's cannon against another jet in flight. Skipped without the real data.
use crate::support::air_gunnery::tail_shots;

/// The cannon fires along the nose with no spread, and from dead astern a jet is a thin target (a burst flown exactly along a
/// MiG-29's axis passes over its fuselage, between the fins): the shooter is moved around the target's line and the burst hits from
/// where the fuselage is, at any speed (the shot is tested against where the target is going, not where it was).
#[test]
fn a_jet_cannon_hits_a_jet_flying_ahead() {
    for (m, w) in crate::support::dc_worlds("DC_No_Fly_Zone", "conquest") {
        for (shooter, target) in [("mig29", "f16"), ("f16", "mig29")] {
            let mut by_speed = Vec::new();
            for speed in [60.0, 120.0, 200.0] {
                let mut map = String::new();
                let mut hit_spots = 0;
                for up in [1.5f32, 1.0, 0.5, 0.0, -0.5, -1.0, -1.5] {
                    for right in [-6.0f32, -4.0, -2.0, -1.0, 0.0, 1.0, 2.0, 4.0, 6.0] {
                        let r = tail_shots(&w, shooter, target, 150.0, [right, up], speed, 1.0);
                        assert!(r.shots >= 8, "{m} {shooter}: {r:?}");
                        assert_eq!(r.damage > 0.0, r.hits > 0, "{m} {shooter} -> {target}: {r:?}");
                        hit_spots += (r.hits > 0) as u32;
                        map.push(if r.hits * 2 > r.shots { '#' } else if r.hits > 0 { '+' } else { '.' });
                    }
                    map.push('\n');
                }
                eprintln!("{m} {shooter} -> {target} at {speed} m/s, the shooter from 6 m left to 6 m right and from 1.5 m above to 1.5 m below:\n{map}");
                by_speed.push(hit_spots);
            }
            // (the spots move a little with the speed: within a frame each jet's wings lift it off its rail, each its own way,
            // and the shot leaves with that)
            assert!(by_speed.iter().all(|n| *n >= 5), "{m} {shooter} -> {target}: hits from {by_speed:?} spots of 63");
        }
    }
}
