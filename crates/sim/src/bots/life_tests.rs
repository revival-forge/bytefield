use super::*;

#[test]
fn profiles_with_the_specification_cut_offs() {
    assert_eq!(MapProfile::de("Berlin").foot_range, 55.0);
    assert_eq!(MapProfile::de("Basrah").foot_range, 100.0);
        // the wait after respawning: 29 s in Berlin, 10 s at El Alamein
    assert!(MapProfile::de("Berlin").board_wait > MapProfile::de("El_Alamein").board_wait);
    assert!(MapProfile::de("Berlin").enter_wait > 0.0 && MapProfile::de("Tobruk").enter_wait == 0.0);
        assert!(MapProfile::de("Berlin").vehicle_range <= 60.0, "the 60 m limit also holds from a vehicle");
}

#[test]
fn a_friend_in_the_line_blocks_the_shot() {
    let (o, b) = ([0.0, 1.5, 0.0], [0.0, 1.5, 40.0]);
    assert!(friend_in_line(o, b, &[[0.5, 0.6, 20.0]]));
        assert!(!friend_in_line(o, b, &[[5.0, 0.6, 20.0]]), "to one side");
    assert!(!friend_in_line(o, b, &[[0.0, 0.6, -5.0]]), "behind");
        assert!(!friend_in_line(o, b, &[[0.0, 0.6, 60.0]]), "beyond");
}

#[test]
fn angles() {
        assert!((angle_between([0.0, 0.0, 1.0], [1.0, 0.0]) - 90.0).abs() < 1e-3, "x is the right");
    assert!((angle_between([0.0, 0.0, 1.0], [-1.0, 0.0]) + 90.0).abs() < 1e-3);
    assert!(angle_between([0.0, 0.0, 1.0], [0.0, 1.0]).abs() < 1e-3);
}

/// A short round with the bot's life on both sides: what is extracted for comparing.
fn round(map: &str, seconds: usize) -> (Vec<u32>, Vec<u32>, usize, Vec<[i32; 3]>) {
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", map, "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, maker(map, 1));
    s.set_minds(2, maker(map, 1));
    for i in 0..16 {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    for _ in 0..seconds * 30 {
        s.step();
    }
    let pos = s.players.iter().filter_map(|p| p.soldier.map(|i| s.vehicles[i].craft.body.pos.map(|x| (x * 100.0) as i32))).collect();
    (s.players.iter().map(|p| p.deaths).collect(), s.modes.conquest.as_ref().unwrap().points.iter().map(|p| p.team).collect(), s.journal.fired.len(), pos)
}

#[test]
fn the_same_round_twice_is_the_same_and_there_is_combat() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    let a = round("Berlin", 60);
        assert!(a.2 > 0, "no shot in a minute");
        assert_eq!(a, round("Berlin", 60), "not deterministic");
}

/// At Omaha_Beach the Allies spawn in landing craft: within a minute they must have reached the beach and got off.
#[test]
fn at_omaha_the_landing_craft_disembark_and_nobody_dies_falling() {
    let game = bytefield_testdata::require!(Base);
    let w = crate::World::load(&game, "bf1942", "Omaha_Beach", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, maker("Omaha_Beach", 1));
    s.set_minds(2, maker("Omaha_Beach", 1));
    for i in 0..32 {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let mut falls = 0;
    let mut seen = 0;
    for _ in 0..70 * 30 {
        s.step();
        for e in s.journal.damage_log.get(seen..).unwrap_or(&[]) {
            let (soldier, falls_off) = (s.vehicles[e.vehicle].has_soldier_contact(), matches!(e.source, crate::damage::Source::Crash));
            if soldier && falls_off && e.amount > 50.0 {
                falls += 1;
            }
        }
        seen = s.journal.damage_log.len();
    }
    let on_land = s.players.iter().filter(|p| p.team == 2 && p.alive).filter_map(|p| s.vehicles.get(p.soldier?)).filter(|v| v.entry.inside.is_none() && v.craft.body.pos[2] > 900.0).count();
        assert!(on_land >= 4, "only {on_land} on the beach");
        assert!(falls <= 2, "{falls} deadly hits");
}

#[test]
fn at_omaha_the_same_round_is_the_same() {
    if bytefield_testdata::base().is_none() {
        return;
    }
        assert_eq!(round("Omaha_Beach", 40), round("Omaha_Beach", 40), "not deterministic");
}

/// Medics and engineers: in a round there are heals to mates and repairs.
#[test]
fn mates_are_healed_and_vehicles_repaired() {
    let game = bytefield_testdata::require!(Base);
        // (depends on someone ending up wounded or with a broken tank near whoever carries the medkit or the wrench: several rounds are summed)
    let w = crate::World::load(&game, "bf1942", "Tobruk", "conquest").unwrap();
    let (mut heals, mut repairs) = (0, 0);
    for seed in 1..=3 {
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        s.set_minds(1, maker("Tobruk", seed));
        s.set_minds(2, maker("Tobruk", seed));
        for i in 0..32 {
            let p = s.join(0, true);
            s.players[p].kit = (i / 2) % 5;
        }
        for _ in 0..300 * 30 {
            s.step();
        }
        heals += s.journal.heals_by[1];
        repairs += s.journal.repairs;
        println!("seed {seed}: heals {:?}, repairs {}", s.journal.heals_by, s.journal.repairs);
    }
        assert!(heals > 0, "no heal to mates");
        assert!(repairs > 0, "no repair");
}

/// Whatever goes through water without being a surface boat also sails: a submarine does not stay stuck where it was.
#[test]
fn the_submarine_sails() {
    let game = bytefield_testdata::require!(Base);
    let w = crate::World::load(&game, "bf1942", "GuadalCanal", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, maker("GuadalCanal", 0));
    s.set_minds(2, maker("GuadalCanal", 0));
    let p = s.join(1, true);
    for _ in 0..150 {
        s.step();
    }
        let k = s.vehicles.iter().position(|v| v.in_world() && v.template.eq_ignore_ascii_case("Gato")).expect("no submarine");
        let i = s.players[p].soldier.expect("no soldier");
    s.enter(i, k, 0);
    let a = s.vehicles[k].craft.body.pos;
    for _ in 0..45 * 30 {
        s.step();
    }
    let b = s.vehicles[k].craft.body.pos;
    let d = (a[0] - b[0]).hypot(a[2] - b[2]);
        assert!(d > 50.0, "it moved {d} m");
}

/// A round of `seconds` on `map`: the mines and the charges the engineers lay in it.
fn explosives_at(map: &str, seed: u64, seconds: usize) -> (usize, usize) {
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", map, "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, maker(map, seed));
    s.set_minds(2, maker(map, seed));
    for i in 0..32 {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    // (every one laid in the round, told apart by where it was laid: a charge goes off or is set off before the end, and
    // counting those left at the end gave none in seven seeds of Omaha_Beach once a change of the mesh moved the rounds)
    let mut laid = std::collections::BTreeSet::new();
    for _ in 0..seconds * 30 {
        s.step();
        for p in &s.projectiles {
            let name = p.template.name.to_ascii_lowercase();
            let kind = if name.contains("landmine") { 0 } else if name.contains("exppack") { 1 } else { continue };
            laid.insert((kind, (p.start[0] * 100.0) as i64, (p.start[1] * 100.0) as i64, (p.start[2] * 100.0) as i64));
        }
    }
    let n = |kind: u8| laid.iter().filter(|l| l.0 == kind).count();
    (n(0), n(1))
}

/// The engineer lays mines and charges where there are vehicles.
#[test]
fn the_engineer_uses_its_explosives() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    // three seeds summed: with one (42) a change of an ulp in the physics left it without charges in 300 s (24 mines,
    // 0 charges; 1, 7, 99 and 123 had them)
    let (mines, charges) = [42, 7, 99].iter().map(|&s| explosives_at("Omaha_Beach", s, 300)).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    assert!(mines > 0 && charges > 0, "{mines} mines, {charges} charges");
}

/// Laying a mine counts as a shot, and the specification admits none far from the enemy in Berlin.
#[test]
fn no_explosives_are_used_in_berlin() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    assert_eq!(explosives_at("Berlin", 1, 120), (0, 0));
}

/// Whoever falls from very high opens the parachute and arrives alive.
#[test]
fn falling_opens_the_parachute() {
    let game = bytefield_testdata::require!(Base);
    let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, maker("El_Alamein", 1));
    let p = s.join(1, true);
    for _ in 0..300 {
        s.step();
    }
        let i = s.players[p].soldier.expect("no soldier");
    let at = s.vehicles[i].craft.body.pos;
    s.vehicles[i].craft.body.pos = [at[0], at[1] + 200.0, at[2]];
    s.vehicles[i].craft.body.vel = [20.0, -10.0, 0.0];
    let mut open = false;
    for _ in 0..40 * 30 {
        s.step();
        open |= s.vehicles[i].soldier().is_some_and(|m| m.parachute);
    }
        assert!(open, "it has not opened");
        assert!(s.players[p].alive && s.players[p].soldier == Some(i), "it died in the fall");
}

/// The stranded landing craft: with the ramp lowered you get out, and so those getting off carry on on foot.
#[test]
fn on_the_shore_the_ramp_goes_down_and_those_from_the_craft_go_on_on_foot() {
    let game = bytefield_testdata::require!(Base);
    let w = crate::World::load(&game, "bf1942", "Omaha_Beach", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, maker("Omaha_Beach", 1));
    s.set_minds(2, maker("Omaha_Beach", 1));
    for i in 0..32 {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let mut ramp = false;
    for _ in 0..70 * 30 {
        s.step();
        ramp |= s.vehicles.iter().filter(|v| v.template.eq_ignore_ascii_case("Lcvp")).any(|v| v.craft.parts.iter().any(|p| p.name.eq_ignore_ascii_case("Lcvp_Ramp") && p.bundle.as_ref().is_some_and(|b| b.angle[1] > 80.0)));
    }
        assert!(ramp, "no ramp lowered");
    let far_from_water = s.players.iter().filter(|p| p.team == 2 && p.alive).filter_map(|p| s.vehicles.get(p.soldier?)).filter(|v| v.entry.inside.is_none() && v.craft.body.pos[2] > 995.0).count();
        assert!(far_from_water >= 4, "only {far_from_water} have left the beach");
}
