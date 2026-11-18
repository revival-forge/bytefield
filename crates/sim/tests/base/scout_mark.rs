//! The scout's mark: the binoculars fire no shot, they place a camera 30 m
//! over the scout looking at what the ray meets; marking again sets off the last one; it ends with its projectile
//! (`BinocularsProjectile`, 120 s). Skipped without the real data.
use crate::support::scout_mark::*;

#[test]
fn the_binoculars_place_a_scout_camera() {
    let Some(game) = bytefield_testdata::base() else { return };
    let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let c = s.modes.conquest.as_ref().unwrap();
    let skin = c.teams.skins[1].clone().unwrap();
    let kit = c.teams.kits[1].iter().flatten().find(|k| w.kit_weapons(k).iter().any(|x| x.name == "Binoculars")).expect("a kit with binoculars").clone();
    let (x, z) = (1024.0, 1024.0);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let sol = s.spawn_vehicle(&skin, [x, y, z], [0.0; 3]).unwrap();
    s.vehicles[sol].team = 2;
    assert!(s.vehicles[sol].give_kit(w.kit_weapons(&kit), Some("Binoculars")));
    s.vehicles[sol].movement.as_mut().unwrap().has_player = true;
    for _ in 0..30 {
        s.step();
    }
    let shots = s.projectiles.len();
    fire_binoculars(&mut s, sol, 40, 1.0);
    let cams: Vec<(usize, bytefield_sim::projectile::ScoutMark, [f32; 3])> = s.scout_cameras().map(|(k, p)| (k, p.scout.unwrap(), p.pos)).collect();
    assert_eq!(cams.len(), 1, "{} projectiles made", s.projectiles.len() - shots);
    let (first, mark, eye) = cams[0];
    assert_eq!((mark.player, mark.team), (sol, 2));
    let at = s.vehicles[sol].craft.body.pos;
    assert!((eye[1] - at[1] - 30.0).abs() < 3.0, "the camera {eye:?} over the soldier {at:?}");
    let ground = w.height_at(mark.target[0] as f64, mark.target[2] as f64).unwrap() as f32;
    assert!((mark.target[1] - ground).abs() < 0.5, "the mark {:?} on the ground {ground}", mark.target);
    let fwd = s.projectiles[first].rows[2];
    let to = [mark.target[0] - eye[0], mark.target[1] - eye[1], mark.target[2] - eye[2]];
    let l = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
    assert!(fwd.iter().zip(to).map(|(a, b)| a * b / l).sum::<f32>() > 0.999, "the camera looks at the mark");

    // marking again: the first goes off, one camera left
    // (not straight down: the ray starts 2 m along it, `projectilePosition`, under the ground)
    fire_binoculars(&mut s, sol, 40, -0.6);
    let cams: Vec<usize> = s.scout_cameras().map(|(k, _)| k).collect();
    assert_eq!(cams.len(), 1, "{:?}", s.vehicles[sol].movement.as_ref().unwrap().camera_rows);
    assert_ne!(cams[0], first);
    assert_eq!(s.team_scout_cameras(2), cams, "the scout's team's");
    assert!(s.team_scout_cameras(1).is_empty(), "not the other team's");
    assert!(!s.projectiles[first].alive());

    // 120 s on, none
    for _ in 0..(121 * 30) {
        s.step();
    }
    assert_eq!(s.scout_cameras().count(), 0);
}

/// Who may look from the marks (`toggleScoutCamera` checks `artPos` of the occupant's PlayerControlObject): the
/// Priest's gunner, not its driver.
#[test]
fn the_priest_gunner_may_look_from_the_marks() {
    let Some(game) = bytefield_testdata::base() else { return };
    let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let skin = s.modes.conquest.as_ref().unwrap().teams.skins[1].clone().unwrap();
    let (x, z) = (1024.0, 1024.0);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let priest = s.spawn_vehicle("Priest", [x, y, z], [0.0; 3]).unwrap();
    let art: Vec<bool> = s.vehicles[priest].entry.seats.iter().map(|q| q.art).collect();
    assert_eq!(art, [false, true], "{:?}", s.vehicles[priest].entry.seats.iter().map(|q| &q.pco).collect::<Vec<_>>());
    let sol = s.spawn_vehicle(&skin, [x + 5.0, y, z], [0.0; 3]).unwrap();
    assert!(!s.art_seat(sol), "on foot");
    s.enter(sol, priest, 0);
    assert!(!s.art_seat(sol), "driving");
    s.exit(sol, priest);
    s.enter(sol, priest, 1);
    assert!(s.art_seat(sol), "the gun");
}
