//! What a projectile carries in its tree: its
//! `ObjectSpawner`s deploy their object where it is (DC's mortar, GC's camouflage), its
//! `SupplyDepot`s supply while it lasts (GC's astromech repairs). Skipped without the real data.
fn mortar(w: &bytefield_sim::World) {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let c = s.modes.conquest.as_ref().unwrap();
    let skin = c.teams.skins[1].clone().unwrap();
    let kit = c.teams.kits[1].iter().flatten().find(|k| w.kit_weapons(k).iter().any(|x| x.name == "Mortar_weap")).expect("a kit with the mortar").clone();
    let (x, z) = (1024.0, 1024.0);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let sol = s.spawn_vehicle(&skin, [x, y, z], [0.0; 3]).unwrap();
    s.vehicles[sol].team = 2;
    assert!(s.vehicles[sol].give_kit(w.kit_weapons(&kit), Some("Mortar_weap")));
    s.vehicles[sol].soldier_mut().unwrap().has_player = true;
    for f in 0..90 {
        s.vehicles[sol].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, if f < 2 { 1.0 } else { 0.0 })]);
        s.step();
    }
    let mortars: Vec<usize> = (0..s.vehicles.len()).filter(|&i| s.vehicles[i].template.eq_ignore_ascii_case("Mortar") && !s.vehicles[i].removed).collect();
    assert_eq!(mortars.len(), 1, "{:?}", s.journal.spawn_log);
    let (a, b) = (s.vehicles[mortars[0]].craft.body.pos, s.vehicles[sol].craft.body.pos);
    let d = ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    assert!(d < 5.0, "the mortar {d} m away");
    assert!(s.spawners.iter().filter(|p| p.on_projectile()).all(|p| !p.active), "the bomb is gone");
}

/// DC's support soldier fires his `Mortar_weap` once: its `mortarbomb` carries `MortarSpawner3`,
/// which makes a `Mortar` by him at once; the mortar stays when the bomb is gone (1 s).
#[test]
fn the_dc_mortar_is_deployed() {
    for (_, w) in crate::support::dc_worlds("Battle_of_the_Bulge", "conquest") {
        mortar(&w);
    }
}

/// The same against the original:
/// a US support soldier standing in DC_Basrahs_Edge raises the `Mortar_weap` (key 4, record 20) and presses fire (60); the
/// `mortarbomb` leaves after `fireDelay` 0.4 s (72) and its `MortarSpawner3` makes the `Mortar` where the bomb is (73), with
/// 10 hit points.
#[test]
fn the_dc_mortar_is_deployed_as_the_original() {
    for (_, w) in crate::support::dc_worlds("DC_Basrahs_Edge", "coop") {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        let at = [679.5038, 23.42736, 717.36694];
        let sol = s.spawn_vehicle("USSoldier", at, [0.0; 3]).unwrap();
        s.vehicles[sol].craft.body.rows = [[0.14349259, 0.0, -0.98965138], [0.0, 1.0, 0.0], [0.98965138, 0.0, 0.14349259]];
        s.vehicles[sol].team = 2;
        let kit = w.kit_weapons("US_Support");
        // (with the kit's first, then key 4 as a player: raising it, `enable`, sets `fireDelay`'s countdown)
        assert!(s.vehicles[sol].give_kit(kit, None));
        s.vehicles[sol].soldier_mut().unwrap().has_player = true;
        use bytefield_sim::soldier::axis;
        let (switch, pressed) = (0u64, 40u64);
        let (mut fired, mut made) = (None, None);
        for f in 0..80u64 {
            let on = |a: u64| if (a..a + 2).contains(&f) { 1.0 } else { 0.0 };
            s.vehicles[sol].soldier_mut().unwrap().set_input(&[(axis::WEAPON1 + 3, on(switch)), (axis::FIRE, on(pressed))]);
            s.step();
            if fired.is_none() && !s.journal.fired.is_empty() {
                fired = Some(f);
            }
            if made.is_none()
                && let Some(i) = (0..s.vehicles.len()).find(|&i| s.vehicles[i].template.eq_ignore_ascii_case("Mortar"))
            {
                made = Some((f, s.vehicles[i].craft.body.pos, s.vehicles[i].armor.as_ref().map(|a| a.hp)));
            }
        }
        let (step, pos, hp) = made.expect("a mortar");
        eprintln!("fired at {fired:?} (pressed {pressed}); mortar made at {step} at {pos:?}, {hp:?} hp");
        // the original: pressed 60, fired 72 (the countdown runs only with a shot waiting), made 73 at (679.5674, 25.2776, 717.3779),
        // 1.85 m over the soldier's root. Level 2: in a second run (another spawn point) it was made 4 frames after the shot and 1.75 m
        // over, when the spawner's update came; ours, the same step as the shot (TODO in `damage.rs`, the spawner's first frame).
        assert_eq!(fired, Some(pressed + 12));
        assert!((pressed + 12..=pressed + 16).contains(&step), "made at {step}");
        let d = [679.5674 - pos[0], 25.2776 - pos[1], 717.3779 - pos[2]];
        assert!(d.iter().all(|x| x.abs() < 0.3), "{d:?} off the original's");
        assert_eq!(hp, Some(10.0));
    }
}
