//! A fighter's lasers against another fighter in flight. Skipped without the real data.
use crate::support::air_gunnery::tail_shots;

#[test]
fn a_fighter_laser_hits_a_fighter_flying_ahead() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").unwrap();
    for (shooter, target) in [("tiefighter", "x-wing"), ("x-wing", "tiefighter")] {
        let mut by_speed = Vec::new();
        for speed in [60.0, 120.0] {
            let mut map = String::new();
            let mut hit_spots = 0;
            for up in [3.0f32, 2.0, 1.0, 0.0, -1.0, -2.0, -3.0] {
                for right in [-6.0f32, -4.0, -2.0, -1.0, 0.0, 1.0, 2.0, 4.0, 6.0] {
                    let r = tail_shots(&w, shooter, target, 150.0, [right, up], speed, 1.5);
                    assert!(r.shots >= 4, "{shooter}: {r:?}");
                    assert_eq!(r.damage > 0.0, r.hits > 0, "{shooter} -> {target}: {r:?}");
                    hit_spots += (r.hits > 0) as u32;
                    map.push(if r.hits * 2 > r.shots { '#' } else if r.hits > 0 { '+' } else { '.' });
                }
                map.push('\n');
            }
            eprintln!("{shooter} -> {target} at {speed} m/s, the shooter from 6 m left to 6 m right and from 3 m above to 3 m below:\n{map}");
            by_speed.push(hit_spots);
        }
        assert!(by_speed.iter().all(|n| *n >= 5), "{shooter} -> {target}: hits from {by_speed:?} spots of 63");
    }
}
