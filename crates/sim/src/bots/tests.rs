use super::team::{Team, Goal, Order, Role, TeamView};
use super::individual::{Action, Situation, choose};
use super::params::BotParams;
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
    choose(s, Action::GoToGoal, -100.0, 0.0, &BotParams::default())
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
    assert_eq!(choose(&s, Action::GoToGoal, 0.0, 5.0, &BotParams::default()), Action::Heal);
        // but not before a second since the last time it changed
    assert_eq!(choose(&s, Action::GoToGoal, 4.5, 5.0, &BotParams::default()), Action::GoToGoal);
        // by less than the margin it does not change
    let little = Situation { wounded: Some((59.0, 0.7)), ..Situation::calm() };
    assert!(Action::Heal.utility(&little, &BotParams::default()) < 0.5 + BotParams::default().util_margin);
    assert_eq!(choose(&little, Action::GoToGoal, 0.0, 5.0, &BotParams::default()), Action::GoToGoal);
        // if the current one stops making sense, it changes at once: urgency rules
    let wounded = Situation { health: 0.2, since_enemy: 3.0, ..Situation::calm() };
    assert_eq!(choose(&Situation { urgent: true, ..wounded }, Action::Retreat, 4.9, 5.0, &BotParams::default()), Action::GoToGoal);
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
    s.set_minds(1, Box::new(move |j| Box::new(super::Bots { pending_mod: false, radio: Default::default(), causes: Default::default(), spotted_at: [0.0; 3], answering: (None, None), waits: None, team: team.clone(), individual: super::Individual::new(super::MapProfile::de("El_Alamein"), maps.clone(), j, 1), maps: maps.clone() })));
    s.set_minds(2, super::life::maker("El_Alamein", 1));
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

/// With the line of fire checked, bots fires at a wall (shots that stop at < 10 m) less than without checking it, over
/// sixteen Berlin rounds each. (Per round the share is noisy. Since walkers walled in by a wall the path map misses run out the
/// way they see, fewer bots fire at walls from inside a building either way, and the gap narrowed: seeds 1-8 gave 0.87, 9-16
/// 0.67, all sixteen 0.77; before, over 30 seeds 0.5-0.6.)
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
        let runs: Vec<_> = (1..=16u64).flat_map(|seed| [false, true].map(|checked| (checked, sc.spawn(move || hidden_ones(seed, checked))))).collect();
        runs.into_iter().fold((0.0, 0.0), |(a, b), (checked, h)| {
            let v = h.join().unwrap();
            if checked { (a, b + v) } else { (a + v, b) }
        })
    });
        assert!(b < 0.85 * a, "with the line checked {b:.2} % of shots at the wall, without it {a:.2} %");
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
    s.set_minds(1, Box::new(move |j| Box::new(super::Bots { pending_mod: false, radio: Default::default(), causes: Default::default(), spotted_at: [0.0; 3], answering: (None, None), waits: None, team: e2.clone(), individual: super::Individual::new(super::MapProfile::de("El_Alamein"), maps.clone(), j, 1), maps: maps.clone() })));
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
            v.in_world() && !v.has_soldier_contact() && v.entry.seats.len() >= 2 && v.entry.occupants.iter().all(|o| o.is_none()) && (v.team == 0 || v.team == 1) && matches!(crate::bot_api::VehicleClass::of(v), crate::bot_api::VehicleClass::Car | crate::bot_api::VehicleClass::Tank)
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

/// `bots:armour`: an assault soldier looks at an enemy T-34-85 from 40 m with a driver inside and a machine gunner out of the turret
/// (seat 1, which shows him). With the variant its look leaves out the driver (its gun does nothing to the tank: the `.con` damage
/// table) and keeps the gunner, at his body above the hull; without it, both, at the hull's place. From 20 m the driver is a target
/// for its grenades only (one takes a fifth of the tank).
#[test]
fn armour_leaves_out_the_tank_driver_and_keeps_the_exposed_gunner() {
    if !has_data() {
        return;
    }
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let (kd, kg, kr) = (s.join(1, false), s.join(1, false), s.join(2, false));
    s.players[kr].kit = 1;
    let alive = |s: &crate::Sim, k: usize| s.players[k].soldier.filter(|_| s.players[k].alive);
    for _ in 0..900 {
        s.step();
        if [kd, kg, kr].iter().all(|&k| alive(&s, k).is_some()) {
            break;
        }
    }
    let (d, g) = (alive(&s, kd).expect("driver"), alive(&s, kg).expect("gunner"));
    alive(&s, kr).expect("rifleman");
    // (an open place, as `exposed_gunner`'s; the look is asked from 40 and 20 m of it)
    let (x, z) = (1000.0f32, 1000.0f32);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 0.5;
    let tank = s.spawn_vehicle("T34-85", [x, y, z], [0.0; 3]).expect("a T34-85");
    s.enter(d, tank, 0);
    s.enter(g, tank, 1);
    for _ in 0..30 {
        s.step();
    }
    let v = crate::bot_api::BotView::of(&s, kr).expect("the rifleman's view");
    assert!(v.me.items.iter().all(|i| !crate::bots::body::weapons::is_anti_tank(&i.1)), "an assault kit: {:?}", v.me.items);
    let others = v.others();
    let hull = s.vehicles[tank].craft.body.pos;
    let mut sh = crate::bots::body::shooter::Shooter::default();
    let eye = [hull[0] - 40.0, hull[1] + 1.0, hull[2]];
    let plain = sh.armour_view(&v, eye, &others);
    let find = |l: &[crate::bot_api::Other], k: usize| l.iter().find(|o| o.player == k).map(|o| o.pos);
    assert_eq!((find(&plain, kd), find(&plain, kg)), (Some(hull), Some(hull)), "without the variant, both at the hull");
    sh.fps.armour = true;
    let seen = sh.armour_view(&v, eye, &others);
    assert_eq!(find(&seen, kd), None, "the driver, inside the armour, is no target");
    let gunner = find(&seen, kg).expect("the exposed gunner is a target");
    assert_eq!(gunner, s.vehicles[g].craft.body.pos);
    assert!(gunner[1] > hull[1] + 1.0, "the gunner is up on the turret: {gunner:?}, hull {hull:?}");
    assert!(sh.grenade_only.is_empty());
    let near = [eye[0] + 20.0, eye[1], eye[2]];
    let seen = sh.armour_view(&v, near, &others);
    assert_eq!(find(&seen, kd), Some(hull), "from 20 m, the driver for a grenade");
    assert_eq!(sh.grenade_only, vec![kd]);
    // (`bots:tank-cover`: a tank it cannot hurt within 30 m is one to hide from; not one its grenades reach, nor one 40 m off)
    sh.fps.tank_cover = true;
    sh.armour_view(&v, near, &others);
    assert_eq!(sh.armour_near, None, "from 20 m it has grenades for him");
    sh.armour_view(&v, eye, &others);
    assert_eq!(sh.armour_near, None, "40 m off");
    let close = [eye[0] + 35.0, eye[1], eye[2]];
    sh.armour_view(&v, close, &others);
    assert_eq!(sh.armour_near, Some((kd, hull)), "5 m off, too near for a grenade: the driver's tank, to hide from");
}

/// `bots:fov`: with two enemies ahead, one 10 m behind the other, and a nearer one at its back, it takes the nearer of those ahead;
/// without the variant, the one at its back (it sees all round). One within [`FOV_TOUCH_M`] behind it is noticed, and the target it
/// follows is kept out of the cone. With `bots:recall`, a target lost from sight is watched for where it was, led by its motion.
///
/// [`FOV_TOUCH_M`]: crate::bots::body::shooter::FOV_TOUCH_M
#[test]
fn fov_takes_those_ahead_and_recall_watches_the_lost_one() {
    use crate::bot_api::Other;
    use crate::bots::body::shooter::{Shooter, RECALL_S};
    if !has_data() {
        return;
    }
    let game = bytefield_testdata::game();
    let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
    let mut s = crate::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let k = s.join(1, false);
    for _ in 0..900 {
        s.step();
        if s.players[k].alive {
            break;
        }
    }
    let v = crate::bot_api::BotView::of(&s, k).expect("its view");
    // (an open place, as `armour_leaves_out_the_tank_driver_and_keeps_the_exposed_gunner`'s)
    let (x, z) = (1000.0f32, 1000.0f32);
    let at = |dx: f32, dz: f32| [x + dx, w.height_at((x + dx) as f64, (z + dz) as f64).unwrap() as f32, z + dz];
    let eye = { let g = at(0.0, 0.0); [g[0], g[1] + 1.6, g[2]] };
    let f = v.me.view[2];
    let l = f[0].hypot(f[2]);
    let (fx, fz) = (f[0] / l, f[2] / l);
    let enemy = |player: usize, along: f32| Other { player, team: 2, pos: at(fx * along, fz * along), in_vehicle: false, vehicle: None, kit: None, health: None, exposed: None };
    let others = vec![enemy(50, 20.0), enemy(51, 30.0), enemy(52, -10.0)];
    for o in &others {
        assert!(!v.hidden(eye, [o.pos[0], o.pos[1] + crate::bots::body::CHEST, o.pos[2]]), "{} in sight", o.player);
    }
    let mut rng = crate::bots::body::rng::Rng::new(1);
    let mut sh = Shooter::default();
    sh.see(&v, &mut rng, eye, 150.0, &others);
    assert_eq!(sh.target.map(|b| b.0), Some(52), "without the variant, the nearest: the one at its back");
    let mut sh = Shooter::default();
    sh.fps.fov = true;
    let seen = sh.fov_view(&v, eye, others.clone());
    sh.see(&v, &mut rng, eye, 150.0, &seen);
    assert_eq!(sh.target.map(|b| b.0), Some(50), "with it, the nearer of those ahead");
    let near = vec![enemy(53, -2.0)];
    assert_eq!(sh.fov_view(&v, eye, near).len(), 1, "one at arm's length behind it is noticed");
    sh.target = Some((52, others[2].pos, 10.0));
    assert!(sh.fov_view(&v, eye, others.clone()).iter().any(|o| o.player == 52), "the one it follows is kept");
    // recall: the target it follows walks sideways, then a wall hides him (he leaves the list it sees)
    let mut sh = Shooter::default();
    sh.fps.recall = true;
    let mut v = v;
    let mut him = enemy(50, 20.0);
    for tick in 0..12 {
        v.time = tick as f64 / 30.0;
        him.pos[0] += 0.1 * fz;
        him.pos[2] -= 0.1 * fx;
        sh.see(&v, &mut rng, eye, 150.0, std::slice::from_ref(&him));
    }
    assert_eq!(sh.target.map(|b| b.0), Some(50));
    v.time = 0.4;
    sh.see(&v, &mut rng, eye, 150.0, &[]);
    assert!(sh.target.is_none() && sh.lost.is_some_and(|l| l.player == 50), "lost from sight: {:?}", sh.lost);
    v.time = 0.9;
    let watch = sh.recall(&v, std::slice::from_ref(&him)).expect("watched for");
    let last = sh.lost.unwrap().at;
    let led = [(watch[0] - last[0]) * fz - (watch[2] - last[2]) * fx, 0.0];
    assert!(led[0] > 0.5 && led[0] < 3.0, "led along its motion (3 m/s for 0.5 s): {led:?}");
    assert!(sh.recall(&v, &[]).is_none(), "dead: not watched for");
    sh.lost = Some(crate::bots::body::shooter::Lost { player: 50, at: last, velocity: [0.0; 3], t: 0.4 });
    v.time = 0.4 + RECALL_S;
    assert!(sh.recall(&v, std::slice::from_ref(&him)).is_none(), "after a while, forgotten");
}
