//! A soldier in a seat that shows him (`seatFlags`: half body, whole body, head, standing or outside) can be shot; one inside the
//! hull cannot. The T-34-85's machine gunner (seat 1, `c_SeatShowHalfBodySoldier`) stands out of the turret from the waist up:
//! a rifle round at his chest kills or wounds him, the same round lower meets the turret. Skipped without the real data.
//!
//! Level 3 (heuristic): the original server, on entering a seat, takes the soldier out of the physics
//! (`disablePhysicsOnSoldier`) and reads no seat flag, and how its projectiles still reach an exposed gunner has not been
//! traced; players of the original remember shooting them, which is what this keeps.

/// A rifle round fired level at the tank's seat-1 gunner from `from` metres to the side, `dy` over his seat; the damage the
/// gunner, the driver and the tank took.
fn shoot(dy: f32, from: f32) -> (f32, f32, f32) {
    let w = bytefield_sim::World::load(&crate::support::game(), "bf1942", "Berlin", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let (kd, kg) = (s.join(1, false), s.join(1, false));
    let alive = |s: &bytefield_sim::Sim, k: usize| s.players[k].soldier.filter(|_| s.players[k].alive);
    let mut both = None;
    for _ in 0..900 {
        s.step();
        if let (Some(d), Some(g)) = (alive(&s, kd), alive(&s, kg)) {
            both = Some((d, g));
            break;
        }
    }
    let (d, g) = both.expect("the soldiers never spawned");
    let (x, z) = (1000.0f32, 1000.0f32);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 0.5;
    let tank = s.spawn_vehicle("T34-85", [x, y, z], [0.0; 3]).expect("a T34-85");
    s.enter(d, tank, 0);
    s.enter(g, tank, 1);
    for _ in 0..30 {
        s.step();
    }
    let at = s.vehicles[g].craft.body.pos;
    let hp = |s: &bytefield_sim::Sim, k: usize| s.vehicles[k].armor.as_ref().map_or(0.0, |a| a.hp);
    let before = (hp(&s, g), hp(&s, d), hp(&s, tank));
    // from the tank's left (−x), towards +x
    let rows = [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
    s.fire_barrel("K98", rows, [at[0] - from, at[1] + dy, at[2]], Some([0.0; 3]), -1).expect("a round");
    for _ in 0..20 {
        s.step();
    }
    (before.0 - hp(&s, g), before.1 - hp(&s, d), before.2 - hp(&s, tank))
}

#[test]
fn a_rifle_round_hits_the_t34s_exposed_gunner_and_not_through_the_hull() {
    if !crate::support::has_level("bf1942", "Berlin") {
        eprintln!("no Berlin, skipped");
        return;
    }
    // (the tank loses 7 settling on the ground, shot or not)
    let (chest, head, turret) = (shoot(0.3, 15.0), shoot(0.6, 15.0), shoot(-0.2, 15.0));
    assert!(chest.0 > 0.0 && chest.1 == 0.0, "a round at the gunner's chest: {chest:?} (gunner, driver, tank)");
    assert!(head.0 > chest.0 && head.1 == 0.0, "at his head, more: {head:?} against {chest:?}");
    assert!(turret.0 == 0.0 && turret.1 == 0.0, "lower, the turret stops it: {turret:?}");
    assert!(shoot(1.0, 15.0).0 == 0.0, "over his head it misses him");
}
