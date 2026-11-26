//! The scout's mark: the binoculars fire no shot, they place a camera 30 m
//! over the scout looking at what the ray meets; marking again sets off the last one; it ends with its projectile
//! (`BinocularsProjectile`, 120 s). Skipped without the real data.
use crate::support::scout_mark::*;

/// DC_Final: its binoculars are vanilla's file; with a DC kit the mark is made the same way, one camera
/// at a time. Bench only. Skipped without DC.
#[test]
fn the_binoculars_of_a_dc_kit_mark_too() {
    let Some(game) = bytefield_testdata::base() else { return };
    if !game.join("Mods/DC_Final").is_dir() {
        return;
    }
    let w = bytefield_sim::World::load(&game, "DC_Final", "DC_Basrahs_Edge", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let (x, z) = (721.85f32, 719.78f32);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let sol = s.spawn_vehicle("USSoldier", [x, y, z], [0.0; 3]).unwrap();
    s.vehicles[sol].team = 2;
    assert!(s.vehicles[sol].give_kit(w.kit_weapons("Us_Assault"), Some("Binoculars")));
    s.vehicles[sol].soldier_mut().unwrap().has_player = true;
    for _ in 0..30 {
        s.step();
    }
    fire_binoculars(&mut s, sol, 40, 0.3);
    let first: Vec<usize> = s.scout_cameras().map(|(k, _)| k).collect();
    assert_eq!(first.len(), 1);
    fire_binoculars(&mut s, sol, 40, -0.2);
    let second: Vec<usize> = s.scout_cameras().map(|(k, _)| k).collect();
    assert_eq!(second.len(), 1);
    assert_ne!(first, second);
}
