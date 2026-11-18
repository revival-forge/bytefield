use super::team::{Team, Goal, Order, Role, TeamView};
use super::individual::{Action, MARGIN, Situation, choose};
use super::profile::Profile;
use crate::bot_api::MindMaker;

#[test]
fn a_team_with_nobody_commands_nothing() {
    let mut e = Team::default();
    assert!(e.due(1.0));
        assert!(!e.due(1.0), "once per instant");
    assert!(e.due(1.0 + 1.0 / 30.0));
    e.think(1.0, &TeamView::default());
    let o = e.order(3);
    assert_eq!(o.role, Role::Free);
    assert_eq!(o, Order::free());
    assert!(matches!(o.goal, Goal::Point(_)));
}

/// Two casualties within 10 m in 20 s make a dangerous place; a single one, or old ones, do not.
#[test]
fn the_casualty_report_marks_where_several_fall() {
    let mut e = Team::default();
    e.aim_low(10.0, [100.0, 0.0, 100.0]);
        assert_eq!(e.dangers(10.0), [None; 4], "a single casualty");
    e.aim_low(12.0, [104.0, 0.0, 100.0]);
    e.aim_low(13.0, [300.0, 0.0, 100.0]);
    assert_eq!(e.dangers(13.0), [Some([102.0, 100.0]), None, None, None]);
        assert_eq!(e.dangers(31.0), [None; 4], "after 20 s it is forgotten");
}

#[test]
fn the_profiles_are_ordered() {
    assert!(Profile::Rookie.error_degrees() > Profile::Normal.error_degrees() && Profile::Normal.error_degrees() > Profile::Veteran.error_degrees());
    assert!(Profile::Rookie.reaction_s() > Profile::Normal.reaction_s() && Profile::Normal.reaction_s() > Profile::Veteran.reaction_s());
    assert!(Profile::Rookie.aggressiveness() > Profile::Veteran.aggressiveness());
    assert!(Profile::Rookie.low_health() < Profile::Normal.low_health() && Profile::Normal.low_health() < Profile::Veteran.low_health());
    assert_eq!(Profile::by_name("Veteran"), Some(Profile::Veteran));
    assert_eq!(Profile::by_name("expert"), None);
}

/// Measured from outside, in the same Berlin round, the rookie misses more than the veteran (and the veteran kills more).
#[test]
fn the_rookie_misses_more_than_the_veteran() {
    if !has_data() {
        return;
    }
    let (_, m) = measured_round("Berlin", 32, 120, super::maker_variant("Berlin", 3, &["veteran"]), super::maker_variant("Berlin", 3, &["rookie"]));
    let r = m.finish();
    let err = |t: &str| r["sides"][t]["B17_error_degrees"].as_f64().unwrap();
        assert!(err("2") > err("1") + 1.5, "rookie {:.2}°, veteran {:.2}°", err("2"), err("1"));
}

/// The best action in the menu for a situation, without hysteresis.
fn best(s: &Situation) -> Action {
    choose(s, Action::GoToGoal, -100.0, 0.0)
}

/// The utility table with synthetic situations (survival: B05, B06, B07, B18).
#[test]
fn the_utility_table_chooses_what_a_player_would_do() {
    let calm = Situation::calm();
        assert_eq!(best(&calm), Action::GoToGoal, "healthy and nobody around: its business");
        // wounded, no enemy near, after a contact: retreats
    let wounded = Situation { health: 0.2, since_enemy: 3.0, ..calm };
    assert_eq!(best(&wounded), Action::Retreat);
        // …with the enemy at 40 m in sight: takes cover (and with nowhere to, retreats)
    let far = Situation { enemy_m: Some(40.0), enemies_seen: 1, since_enemy: 0.0, ..wounded };
    assert_eq!(best(&far), Action::TakeCover);
        assert_eq!(best(&Situation { no_cover: true, ..far }), Action::Fight, "nowhere to hide, back turned no");
        // …with the enemy at 10 m: fights (fleeing with its back turned is dying)
    let on_top = Situation { enemy_m: Some(10.0), ..far };
    assert!(matches!(best(&on_top), Action::Fight | Action::TakeCover));
    assert_eq!(best(&on_top), Action::Fight);
        // urgency rules (B18): no taking cover
    assert_eq!(best(&Situation { urgent: true, ..far }), Action::GoToGoal);
    assert_eq!(best(&Situation { urgent: true, ..wounded }), Action::GoToGoal);
        // wounded with a medic at 50 m and nobody in sight: goes to the medic; the medic, to the wounded
    assert_eq!(best(&Situation { health: 0.4, medic_m: Some(50.0), ..calm }), Action::FindMedic);
        // …but with little health and a recent contact, it breaks contact first (except with the medic at hand)
    assert_eq!(best(&Situation { medic_m: Some(50.0), ..wounded }), Action::Retreat);
    assert_eq!(best(&Situation { medic_m: Some(10.0), ..wounded }), Action::FindMedic);
    assert_eq!(best(&Situation { medic_m: Some(50.0), since_enemy: 20.0, ..wounded }), Action::FindMedic);
    let medic = Situation { wounded: Some((15.0, 0.2)), ..calm };
    assert_eq!(best(&medic), Action::Heal);
        assert_eq!(best(&Situation { enemy_m: Some(25.0), enemies_seen: 1, ..medic }), Action::Fight, "with an enemy on top, fights first");
        // engineer with a damaged vehicle of its own at hand
    assert_eq!(best(&Situation { damaged_m: Some(30.0), ..calm }), Action::Repair);
        // in a vehicle none of this
    assert_eq!(best(&Situation { on_foot: false, ..wounded }), Action::GoToGoal);
}

#[test]
fn hysteresis_does_not_change_for_little_nor_every_moment() {
    let s = Situation { wounded: Some((50.0, 0.45)), ..Situation::calm() };
        // healing is worth 0.6 + 0.165 - 0.083 = 0.68: beats going to the goal (0.5) by more than the margin
    assert_eq!(choose(&s, Action::GoToGoal, 0.0, 5.0), Action::Heal);
        // but not before a second since the last time it changed
    assert_eq!(choose(&s, Action::GoToGoal, 4.5, 5.0), Action::GoToGoal);
        // by less than the margin it does not change
    let little = Situation { wounded: Some((59.0, 0.7)), ..Situation::calm() };
    assert!(Action::Heal.utility(&little) < 0.5 + MARGIN);
    assert_eq!(choose(&little, Action::GoToGoal, 0.0, 5.0), Action::GoToGoal);
        // if the current one stops making sense, it changes at once: urgency rules
    let wounded = Situation { health: 0.2, since_enemy: 3.0, ..Situation::calm() };
    assert_eq!(choose(&Situation { urgent: true, ..wounded }, Action::Retreat, 4.9, 5.0), Action::GoToGoal);
}

/// A short round with the given head on both sides: casualties, flag owners, shots and where each one ends.
fn round(map: &str, seconds: usize, head: fn(&str, u64) -> MindMaker) -> (Vec<u32>, Vec<u32>, usize, Vec<[i32; 3]>) {
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", map, "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, head(map, 1));
    s.set_minds(2, head(map, 1));
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

/// bots3 without a team (all free) plays exactly like bots2: same casualties, flags, shots and positions with the same seed.
#[test]
fn free_bots3_plays_like_bots2() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    fn free(map: &str, seed: u64) -> MindMaker {
        super::maker_variant(map, seed, &["no-assign", "no-spawn"])
    }
    for map in ["Berlin", "El_Alamein"] {
        let a = round(map, 60, crate::bots2::maker);
                // (in Berlin there is combat in the first minute; at El Alamein they are far and the split is compared)
                assert!(a.2 > 0 || map != "Berlin", "no shot in a minute");
                assert_eq!(a, round(map, 60, free), "bots3 free differs from bots2 on {map}");
    }
}

/// With a fixed order («to flag k»), the side goes to the requested flag and not to the nearest.
#[test]
fn with_a_fixed_order_it_goes_to_the_requested_flag() {
    let game = bytefield_testdata::require!(Base);
    let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    for i in 0..16 {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    for _ in 0..30 {
        s.step();
    }
        // the farthest takeable flag from where side 1 starts
    let flags: Vec<([f32; 3], bool)> = s.modes.conquest.as_ref().unwrap().points.iter().map(|p| (p.pos, !p.template.unable_to_change_team)).collect();
    let pos1 = |s: &crate::Sim| s.players.iter().filter(|p| p.team == 1 && p.alive).filter_map(|p| p.soldier.map(|i| s.vehicles[i].craft.body.pos)).collect::<Vec<_>>();
    let flat = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let mean = |ps: &[[f32; 3]], f: [f32; 3]| ps.iter().map(|p| flat(*p, f)).sum::<f32>() / ps.len().max(1) as f32;
    let before = pos1(&s);
    let k = (0..flags.len()).filter(|&i| flags[i].1).max_by(|&a, &b| mean(&before, flags[a].0).total_cmp(&mean(&before, flags[b].0))).unwrap();
    let mut e = Team::default();
    e.fixed = Some(Order { goal: Goal::Flag(k), role: Role::Leader, kit: 0 });
    let team = std::sync::Arc::new(std::sync::Mutex::new(e));
    let maps = std::sync::Arc::new(super::body::Maps::default());
    s.set_minds(1, Box::new(move |j| Box::new(super::Bots3 { pending_mod: false, radio: Default::default(), causes: Default::default(), spotted_at: [0.0; 3], answering: (None, None), team: team.clone(), individual: super::Individual::new(super::MapProfile::de("El_Alamein"), maps.clone(), j, 1), maps: maps.clone() })));
    s.set_minds(2, crate::bots2::maker("El_Alamein", 1));
    let d0 = mean(&before, flags[k].0);
    for _ in 0..120 * 30 {
        s.step();
    }
    let d1 = mean(&pos1(&s), flags[k].0);
        assert!(d1 < d0 - 150.0, "they do not approach flag {k}: {d0:.0} -> {d1:.0} m");
}

/// A round of `map` with `a` on side 1 and `b` on side 2, `seconds`, watched by a `dumb` `Watcher` and a `quality` meter.
pub(super) fn measured_round(map: &str, players: usize, seconds: usize, a: MindMaker, b: MindMaker) -> (crate::dumb::Watcher, crate::quality::Meter) {
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", map, "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, a);
    s.set_minds(2, b);
    for i in 0..players {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let mut d = crate::dumb::Watcher::new();
    let mut m = crate::quality::Meter::new();
    for _ in 0..seconds * 30 {
        s.step();
        d.step(&s);
        m.step(&s);
    }
    (d, m)
}

fn has_data() -> bool {
    bytefield_testdata::game().is_dir()
}

/// With the line of fire checked, bots3 fires at a wall (shots that stop at < 10 m) quite a bit less than
/// without checking it, over eight Berlin rounds each. (Per round the share is noisy: over 30 seeds the ratio is 0.5-0.6 but
/// two seeds alone fall above 0.75 one time in five; eight against 0.8, about one in two hundred.)
#[test]
fn does_not_fire_at_the_wall_that_hides_the_target() {
    if !has_data() {
        return;
    }
    let hidden_ones = |seed: u64, checked: bool| {
        let maker = |s| if checked { super::maker("Berlin", s) } else { super::maker_variant("Berlin", s, &["no-line"]) };
        let (d, _) = measured_round("Berlin", 32, 150, maker(seed), maker(seed));
        d.finish(serde_json::json!({}))["totals"]["blocked % of shots"].as_f64().unwrap()
    };
    let (a, b) = std::thread::scope(|sc| {
        let runs: Vec<_> = (1..=8u64).flat_map(|seed| [false, true].map(|checked| (checked, sc.spawn(move || hidden_ones(seed, checked))))).collect();
        runs.into_iter().fold((0.0, 0.0), |(a, b), (checked, h)| {
            let v = h.join().unwrap();
            if checked { (a, b + v) } else { (a + v, b) }
        })
    });
        assert!(b < 0.8 * a, "with the line checked {b:.2} % of shots at the wall, without it {a:.2} %");
}


/// B15: a person hurt to `health` stands 40 m from a medic bot of its team, with no enemy about, and says "medic!" (`calls`) or not.
/// The seconds until the medic is within 8 m of it or has begun to heal it (the kit heals from 5-6 m), if within 20 s.
fn medic_comes(health: f32, calls: bool) -> Option<f32> {
    medic_comes_from(health, calls, 40.0, false)
}

/// [`medic_comes`] with the person `away` m behind the medic, and, with `witness`, another bot (not a medic) beside the person to hear it.
fn medic_comes_from(health: f32, calls: bool, away: f32, witness: bool) -> Option<f32> {
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    let team = std::sync::Arc::new(std::sync::Mutex::new(Team::default()));
    let e2 = team.clone();
    let maps = std::sync::Arc::new(super::body::Maps::default());
    s.set_minds(1, Box::new(move |j| Box::new(super::Bots3 { pending_mod: false, radio: Default::default(), causes: Default::default(), spotted_at: [0.0; 3], answering: (None, None), team: e2.clone(), individual: super::Individual::new(super::MapProfile::de("El_Alamein"), maps.clone(), j, 1), maps: maps.clone() })));
    for i in 0..8 {
        let p = s.join(1, true);
        s.players[p].kit = i % 5;
    }
    let person = s.join_person(1);
    for _ in 0..20 * 30 {
        s.step();
    }
    let pos = |s: &crate::Sim, k: usize| s.players[k].soldier.map(|i| s.vehicles[i].craft.body.pos);
    let medic = (0..s.players.len()).find(|&k| s.players[k].ai && s.players[k].alive && team.lock().unwrap().kind(k) == Some(super::team::Kind::Medic) && s.players[k].soldier.is_some_and(|i| s.vehicles[i].entry.inside.is_none()))?;
    // the person 40 m from the medic, behind it (where it is not going anyway)
    let m = pos(&s, medic)?;
    let vel = s.vehicles[s.players[medic].soldier?].craft.body.vel;
    let n = vel[0].hypot(vel[2]).max(1e-3);
    let (dx, dz) = if n > 0.5 { (-vel[0] / n, -vel[2] / n) } else { (1.0, 0.0) };
    let (x, z) = (m[0] + away * dx, m[2] + away * dz);
    let y = w.height_at(x as f64, z as f64)? as f32 + 1.0;
    let sol = s.players[person].soldier?;
    s.vehicles[sol].craft.body.pos = [x, y, z];
    s.vehicles[sol].craft.body.vel = [0.0; 3];
    let a = s.vehicles[sol].armor.as_mut()?;
    a.hp = a.max_hp * health;
    if witness {
        let other = (0..s.players.len()).find(|&k| k != medic && s.players[k].ai && s.players[k].alive && team.lock().unwrap().kind(k) != Some(super::team::Kind::Medic) && s.players[k].soldier.is_some_and(|i| s.vehicles[i].entry.inside.is_none()))?;
        let o = s.players[other].soldier?;
        s.vehicles[o].craft.body.pos = [x + 5.0, y, z];
        s.vehicles[o].craft.body.vel = [0.0; 3];
    }
    for _ in 0..5 {
        s.step();
    }
    if calls {
        s.say(person, crate::radio::LOCAL_MEDIC, None);
    }
    for k in 0..20 * 30 {
        s.step();
        let (a, b) = (pos(&s, medic)?, pos(&s, person)?);
        let healed = s.vehicles[sol].armor.as_ref().is_some_and(|a| a.hp > a.max_hp * (health + 0.02));
        if (a[0] - b[0]).hypot(a[2] - b[2]) < 8.0 || healed {
            return Some(k as f32 / 30.0);
        }
    }
    None
}

/// B15: a medic bot comes to a person who calls "medic!" though it is not hurt enough for one to come by itself, and does not come
/// without the call; hurt more, it comes without one, as before.
#[test]
fn a_medic_comes_to_a_person_who_calls() {
    if !has_data() {
        return;
    }
    let called = medic_comes(0.7, true);
    assert!(called.is_some_and(|t| t < 20.0), "called at 70 %: {called:?}");
    assert_eq!(medic_comes(0.7, false), None, "at 70 % without a call it should not come");
    let hurt = medic_comes(0.4, false);
    assert!(hurt.is_some(), "at 40 % it comes without a call: {hurt:?}");
}

/// B25: a medic 85 m away (beyond the 70 m it answered from) comes to a person who calls, heard by a mate beside them: the team sends it.
#[test]
fn b25_a_medic_beyond_earshot_is_sent_to_a_person_who_calls() {
    if !has_data() {
        return;
    }
    let called = medic_comes_from(0.4, true, 85.0, true);
    assert!(called.is_some_and(|t| t < 20.0), "called from 85 m: {called:?}");
}

/// B15: a person at the wheel of an empty vehicle of its team's base says "get in" (`calls`) or not: how many bots are in it 30 s
/// later.
fn bots_get_in(calls: bool) -> usize {
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    s.set_minds(1, super::maker("El_Alamein", 1));
    for i in 0..8 {
        let p = s.join(1, true);
        s.players[p].kit = i % 5;
    }
    let person = s.join_person(1);
    for _ in 0..2 * 30 {
        s.step();
    }
    let sol = s.players[person].soldier.unwrap();
    let foot: Vec<[f32; 3]> = s.players.iter().filter(|p| p.ai && p.alive).filter_map(|p| p.soldier).filter(|&i| s.vehicles[i].entry.inside.is_none()).map(|i| s.vehicles[i].craft.body.pos).collect();
    let near = |p: [f32; 3]| foot.iter().filter(|q| (q[0] - p[0]).hypot(q[2] - p[2]) < 30.0).count();
    // the empty land vehicle with a passenger seat that has most bots on foot within 30 m
    let car = (0..s.vehicles.len())
        .filter(|&k| {
            let v = &s.vehicles[k];
            v.in_world() && !v.is_soldier() && v.entry.seats.len() >= 2 && v.entry.occupants.iter().all(|o| o.is_none()) && (v.team == 0 || v.team == 1) && matches!(crate::bot_api::VehicleClass::of(v), crate::bot_api::VehicleClass::Car | crate::bot_api::VehicleClass::Tank)
        })
        .max_by_key(|&k| (near(s.vehicles[k].craft.body.pos), usize::MAX - k))
        .unwrap();
    assert!(near(s.vehicles[car].craft.body.pos) >= 2, "no vehicle with bots around");
    s.enter(sol, car, 0);
    s.step();
    if calls {
        s.say(person, crate::radio::LOCAL_GET_IN, None);
    }
    // (the bot walking to the seat may stick on the way for some seconds before it goes straight: the body's, not the radio's)
    for _ in 0..30 * 30 {
        s.step();
    }
    s.vehicles[car].entry.occupants.iter().skip(1).filter(|o| o.is_some()).count()
}

/// B15: bots on foot near a person at the wheel get in when it says "get in", and not without it.
#[test]
fn bots_get_in_when_a_person_asks() {
    if !has_data() {
        return;
    }
    let (with, without) = (bots_get_in(true), bots_get_in(false));
    assert!(with >= 1 && with > without, "in the car: {with} with the call, {without} without");
}
