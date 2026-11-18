//! The sonar on Midway's ships: the Fletcher's driver has the sonar's
//! minimap (`sonarPos`), the Gato's hears the pings (`subPos`); the destroyer senses what is under its sonar, and
//! the submarine's sonar finds the other team's destroyer. Skipped without the real data.
#[test]
fn the_destroyer_senses_the_submarine_and_the_submarine_hears_it() {
    let Some(game) = bytefield_testdata::base() else { return };
    let w = bytefield_sim::World::load(&game, "bf1942", "Midway", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let skins = s.modes.conquest.as_ref().unwrap().teams.skins.clone();
    let (x, z) = (1024.0, 1024.0);
    let fletcher = s.spawn_vehicle("Fletcher", [x, 0.0, z], [0.0; 3]).unwrap();
    let gato = s.spawn_vehicle("Gato", [x + 150.0, -5.0, z], [0.0; 3]).unwrap();

    let d = s.vehicles[fletcher].entry.sonar.clone().expect("the Fletcher's sonar");
    assert_eq!((d.template.as_str(), d.radius, d.scan, d.radar), ("DestroyerSonar", 400.0, false, false));
    let g = s.vehicles[gato].entry.sonar.clone().expect("the Gato's sonar");
    assert_eq!((g.template.as_str(), g.scan), ("SubmarineSonar", true));
    let seats: Vec<(bool, bool)> = s.vehicles[fletcher].entry.seats.iter().map(|q| (q.sonar, q.hatch.is_some())).collect();
    assert_eq!(seats[0], (true, false), "the driver's: {seats:?}");
    assert!(seats[1..].iter().all(|q| !q.0), "only the driver's: {seats:?}");

    // the sonar is over the water, so the submarine (and the destroyer itself) are under it
    let at = s.sonar_at(fletcher).unwrap();
    let contacts = s.sonar_contacts(fletcher);
    assert!(contacts.iter().any(|&(j, _)| j == gato), "{at:?} {contacts:?}");
    assert!(contacts.iter().all(|&(_, f)| f <= 1.0 && f > 0.9), "{contacts:?}");

    // a German in the Gato, a Japanese… nobody in the Fletcher: another team (0), so it pings
    let sol = s.spawn_vehicle(skins[0].as_deref().unwrap(), [x + 150.0, 2.0, z], [0.0; 3]).unwrap();
    s.vehicles[sol].team = 1;
    s.enter(sol, gato, 0);
    assert!(s.hears_sonars(sol) && !s.sonar_seat(sol), "the Gato's driver hears, without the sonar's minimap");
    let near = s.enemy_sonars(gato);
    assert_eq!(near.len(), 1, "{near:?}");
    assert!((near[0] - 150.0 / 400.0).abs() < 0.02, "{near:?}");
    assert!(s.enemy_sonars(fletcher).is_empty(), "the destroyer's sonar does not scan… and finds no other that does not");

    // the same team in both: no ping
    s.vehicles[fletcher].team = 1;
    assert!(s.enemy_sonars(gato).is_empty());

    let sol2 = s.spawn_vehicle(skins[1].as_deref().unwrap(), [x, 12.0, z], [0.0; 3]).unwrap();
    s.enter(sol2, fletcher, 0);
    assert!(s.sonar_seat(sol2) && s.hears_sonars(sol2), "the Fletcher's driver");
}
