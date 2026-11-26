use super::*;

fn b(x: f32, owner: u32) -> FlagSighting {
    FlagSighting { pos: [x, 0.0, 0.0], owner, takeable: true, radio: 10.0, on_foot: true }
}
fn c(j: usize, x: f32) -> Mate {
    Mate { player: j, pos: [x, 0.0, 5.0], vehicle: None }
}

/// With a calm own flag and two to take, almost all attack; no own flag without an enemy keeps half.
#[test]
fn b01_the_side_spreads_out_and_does_not_bunch_up_on_its_own() {
    let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(300.0, 2), b(500.0, 0), b(900.0, 2)], vehicles: vec![] };
    let u: Vec<Unit> = (0..16).map(|j| Unit { pos: vec![c(j, j as f32).pos], previous: None, avoid: Vec::new() }).collect();
    let r = split_up(10.0, &view, &[], &[None; 4], &u, &Levers::default());
    assert!(r.iter().all(|x| x.is_some()));
    let n = |i| r.iter().filter(|d| d.unwrap().flag == i).count();
    assert!(n(0) <= 8, "{}", n(0));
    assert!(n(1) >= 4 && n(2) >= 3, "{} {}", n(1), n(2));
    assert!(n(3) < n(2) && n(2) < n(1), "more to the nearer ones: {} {} {}", n(1), n(2), n(3));
}

/// `bots:strength`: a flag with three estimated defenders needs six; the free people go there whole and the undefended one, far
/// as it is, goes first; with fewer people than the best one needs, they all go to one flag.
#[test]
fn the_attack_by_strength_concentrates() {
    let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(300.0, 2), b(600.0, 2), b(900.0, 2)], vehicles: vec![] };
    let mut intel = super::super::intel::Intel::default();
    intel.set_flags(&view.flags.iter().map(|f| f.pos).collect::<Vec<_>>());
    for s in 0..120 {
        for e in 0..3 {
            intel.sighting(s as f64, 50 + e, [300.0, 0.0, 0.0], super::super::intel::Seen::OnFoot);
            intel.sighting(s as f64, 60 + e, [600.0, 0.0, 0.0], super::super::intel::Seen::OnFoot);
        }
        intel.age(s as f64);
    }
    // the enemy is seen at the first two only: the third is undefended
    let reports = [Report { t: 119.0, player: 50, pos: [300.0, 0.0, 0.0], vehicle: false }, Report { t: 119.0, player: 60, pos: [600.0, 0.0, 0.0], vehicle: false }];
    let u: Vec<Unit> = (0..5).map(|j| Unit { pos: (0..2).map(|k| c(2 * j + k, 10.0).pos).collect(), previous: None, avoid: Vec::new() }).collect();
    let lv = Levers { guard: false, ..Levers::default() };
    let r = split_up_with(120.0, &view, &reports, &[None; 4], &u, &lv, &crate::bots::params::BotParams::default(), Some(&intel));
    let n = |i| r.iter().filter(|d| d.is_some_and(|d| d.flag == i)).count() * 2;
    assert!(n(3) >= 2, "the undefended one first: {r:?}");
    assert!(n(1) == 0 || n(1) >= 6, "no share too small for 3 defenders: {r:?}");
    // two units only (4 people) against a need of 6 or more: all to one flag
    let r = split_up_with(120.0, &TeamView { flags: view.flags[..3].to_vec(), ..view.clone() }, &reports, &[None; 3], &u[..2], &lv, &crate::bots::params::BotParams::default(), Some(&intel));
    let flags: BTreeSet<usize> = r.iter().flatten().map(|d| d.flag).collect();
    assert_eq!(flags.len(), 1, "{r:?}");
}

/// Without the guard lever, the own front flag keeps nobody if nobody threatens it (all attack).
#[test]
fn without_guard_all_attack() {
    let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(300.0, 2), b(500.0, 0)], vehicles: vec![] };
    let u: Vec<Unit> = (0..4).map(|j| Unit { pos: (0..4).map(|k| c(4 * j + k, 10.0).pos).collect(), previous: None, avoid: Vec::new() }).collect();
    let to_own = |p: &Levers| split_up(10.0, &view, &[], &[None; 3], &u, p).iter().filter(|d| d.unwrap().flag == 0).count();
    assert_eq!(to_own(&Levers::default()), 1, "with guard, one squad");
    let mut p = Levers::default();
    assert!(p.force("guard", 0.0) && !p.force("no_existe", 1.0));
    assert_eq!(to_own(&p), 0);
}

/// The pilot goes to the enemy vehicle on land the side reported a short while ago; not to the soldier, nor the boat, nor the old report.
#[test]
fn b11_the_pilot_goes_to_the_reported_armour() {
    let mut e = Team::default();
    e.reports = vec![
        Report { t: 9.0, player: 40, pos: [100.0, 0.0, 0.0], vehicle: false },
        Report { t: 9.0, player: 41, pos: [200.0, 0.0, 0.0], vehicle: true },
        Report { t: 9.0, player: 42, pos: [150.0, -5.0, 0.0], vehicle: true },
        Report { t: -20.0, player: 43, pos: [120.0, 0.0, 0.0], vehicle: true },
    ];
    // (whatever is below the water is a boat)
    let b = e.air_target2(10.0, [0.0, 100.0, 0.0], |p| p[1] >= 0.0);
    assert_eq!(b, Some([200.0, 0.0, 0.0]));
    assert_eq!(e.air_target2(30.0, [0.0, 100.0, 0.0], |p| p[1] >= 0.0), None, "old reports");
}

/// B23: each attack pilot gets its own enemy vehicle (one on foot only inside a flag being taken), keeps it while it is reported,
/// and two pilots share only a ship.
#[test]
fn b23_the_attack_pilots_share_out_the_targets() {
    let mut e = Team::default();
    // (y < 0: a ship; y > 50: in the air)
    let surface = |p: [f32; 3]| (p[1] < 50.0).then_some(p[1] < 0.0);
    e.reports = vec![
        Report { t: 9.0, player: 40, pos: [300.0, 0.0, 0.0], vehicle: true },
        Report { t: 9.0, player: 41, pos: [600.0, 0.0, 0.0], vehicle: true },
        Report { t: 9.0, player: 42, pos: [100.0, 0.0, 0.0], vehicle: false },
        Report { t: 9.0, player: 43, pos: [150.0, 200.0, 0.0], vehicle: true },
    ];
    let here = [0.0, 150.0, 0.0];
    assert_eq!(e.air_pass(10.0, 1, here, &[], surface), Some((40, [300.0, 0.0, 0.0])), "the nearest vehicle on the surface");
    assert_eq!(e.air_pass(10.0, 2, here, &[], surface).map(|x| x.0), Some(41), "another pilot, another vehicle");
    assert_eq!(e.air_pass(10.0, 3, here, &[[100.0, 0.0, 10.0]], surface).map(|x| x.0), Some(42), "the one taking our flag");
    assert_eq!(e.air_pass(10.0, 1, here, &[], surface).map(|x| x.0), Some(40), "it keeps its own");
    e.reports.push(Report { t: 9.0, player: 44, pos: [400.0, -2.0, 0.0], vehicle: true });
    e.passes.clear();
    assert_eq!(e.air_pass(10.0, 1, here, &[], surface).map(|x| x.0), Some(40));
    e.reports.retain(|r| r.player != 41);
    assert_eq!(e.air_pass(10.0, 2, here, &[], surface).map(|x| x.0), Some(44), "the ship");
    assert_eq!(e.air_pass(10.0, 3, here, &[], surface).map(|x| x.0), Some(44), "a ship is shared");
    assert_eq!(e.air_pass(40.0, 1, here, &[], surface), None, "old reports");
}

/// B23: an idle follower near a free plane of the side is sent to fly it; not the leader, not more than the share, and not one on
/// land to a plane on a ship's deck.
#[test]
fn b23_the_wing_sends_a_follower_to_the_plane() {
    let mut e = Team::default();
    for j in [1, 2, 3, 4, 5, 6, 7, 8] {
        e.kit_de.insert(j, 0);
    }
    let plane = |vehicle: usize, x: f32, afloat: bool| VehicleSighting { vehicle, pos: [x, 0.0, 0.0], team: 0, free_seats: 1, class: VehicleClass::Plane, health: 1.0, floats: false, seats: vec![0], radius: 6.0, afloat, vtol: false };
    let mates: Vec<Mate> = (1..=8).map(|j| Mate { player: j, pos: [j as f32 * 10.0, 0.0, 0.0], vehicle: None }).collect();
    let view = TeamView { team: 1, mates, flags: Vec::new(), vehicles: vec![plane(50, 0.0, false), plane(51, 5.0, false), plane(52, 10.0, false), plane(53, 20.0, true)] };
    e.orders.insert(1, Order { goal: Goal::Flag(0), role: Role::Leader, kit: 0 });
    for j in 2..=8 {
        e.orders.insert(j, Order { goal: Goal::Flag(0), role: Role::Follow(1), kit: 0 });
    }
    e.wing(0.0, &view, &trip::Unknown);
    // 8 bots: 2 pilots; the nearest followers (1 leads)
    assert_eq!(e.wing_vehicle(2), Some(50));
    assert_eq!(e.wing_vehicle(3), Some(51));
    assert_eq!(e.wing_vehicle(1), None);
    assert_eq!(e.order(2).role, Role::Drive(50));
    assert_eq!(e.wing_free, vec![], "the share is full: the rest wait for a pilot from the spawn screen");
    // on a ship's deck only one there gets it
    let mut e2 = Team::default();
    e2.kit_de.insert(1, 0);
    e2.orders.insert(1, Order { goal: Goal::Flag(0), role: Role::Follow(9), kit: 0 });
    let v2 = TeamView { team: 1, mates: vec![Mate { player: 1, pos: [200.0, 0.0, 0.0], vehicle: None }], flags: Vec::new(), vehicles: vec![plane(53, 0.0, true)] };
    e2.wing(0.0, &v2, &trip::Unknown);
    assert_eq!(e2.wing_vehicle(1), None);
    assert_eq!(e2.wing_free.len(), 1);
}

/// The leader in its tank knows how near the nearest of its own on foot is (those in a vehicle do not count).
/// B26: each enemy aircraft gets a fighter before any gets two, never more than two, and a fighter keeps the one it was given.
#[test]
fn b26_the_fighters_share_out_the_enemy_aircraft() {
    let mut e = Team::default();
    e.air_seen(10.0, 40, [1000.0, 300.0, 0.0]);
    e.air_seen(10.0, 41, [2000.0, 300.0, 0.0]);
    let here = [0.0, 300.0, 0.0];
    assert_eq!(e.engage(10.0, 1, here, &[]), Some((40, [1000.0, 300.0, 0.0])), "the nearest");
    assert_eq!(e.air_taken(2), [40, usize::MAX, usize::MAX, usize::MAX, usize::MAX, usize::MAX]);
    assert_eq!(e.engage(10.0, 2, here, &[]).map(|x| x.0), Some(41), "the one nobody is after");
    assert_eq!(e.engage(10.0, 3, here, &[]).map(|x| x.0), Some(40), "then a second on the nearest");
    assert_eq!(e.engage(10.0, 4, here, &[]).map(|x| x.0), Some(41));
    assert_eq!(e.engage(10.0, 5, here, &[]), None, "two each is the most");
    // it keeps its own while it is seen, wherever that one goes; and it is free again once its fighter stops asking
    e.air_seen(11.0, 41, [2900.0, 300.0, 0.0]);
    assert_eq!(e.engage(11.0, 2, here, &[]), Some((41, [2900.0, 300.0, 0.0])));
    assert_eq!(e.engage(14.0, 5, here, &[]).map(|x| x.0), Some(40), "those who asked at 10 are gone");
    // out of sight for too long, or too far: nothing to go after
    assert_eq!(e.engage(40.0, 2, here, &[]), None);
    e.air_seen(50.0, 42, [9000.0, 300.0, 0.0]);
    assert_eq!(e.engage(50.0, 2, here, &[]), None);
}

/// B26: the enemy aircraft over an own flag that is being taken goes first, though another is nearer; and a fighter that fixed on
/// one by itself counts as being after it.
#[test]
fn b26_what_threatens_an_own_flag_goes_first() {
    let mut e = Team::default();
    e.air_seen(10.0, 40, [500.0, 300.0, 0.0]);
    e.air_seen(10.0, 41, [2000.0, 300.0, 100.0]);
    let flag = [[2000.0, 20.0, 0.0]];
    assert_eq!(e.engage(10.0, 1, [0.0, 300.0, 0.0], &flag).map(|x| x.0), Some(41));
    assert_eq!(e.engage(10.0, 2, [0.0, 300.0, 0.0], &flag).map(|x| x.0), Some(41));
    assert_eq!(e.engage(10.0, 3, [0.0, 300.0, 0.0], &flag).map(|x| x.0), Some(40));
    let mut e = Team::default();
    e.air_seen(10.0, 40, [500.0, 300.0, 0.0]);
    e.air_seen(10.0, 41, [900.0, 300.0, 0.0]);
    e.air_locked(10.0, 7, 40);
    assert_eq!(e.engage(10.0, 1, [0.0, 300.0, 0.0], &[]).map(|x| x.0), Some(41));
}

#[test]
fn b02_the_tank_escort() {
    let mut e = Team::default();
    e.squads.push(Squad { vehicle: Some(5), ..Squad::new(1, vec![2, 3], Goal::Flag(0)) });
    let o = |player: usize, x: f32, in_vehicle: bool| crate::bot_api::Other { player, team: 1, pos: [x, 0.0, 0.0], in_vehicle, vehicle: None, kit: None, health: None, exposed: None };
    assert_eq!(e.escort(1, [0.0; 3], &[o(2, 60.0, false), o(3, 10.0, true), o(9, 5.0, false)]), Some(60.0));
    assert_eq!(e.escort(2, [0.0; 3], &[o(2, 60.0, false)]), None, "not a leader");
}

/// GC_Mos_Eisley, team 1: with enemy flags out of reach on foot, every front flag of its own gets a squad (none left empty).
#[test]
fn unreachable_enemy_flags_leave_no_front_flag_empty() {
    let high = |x: f32| FlagSighting { on_foot: false, ..b(x, 2) };
    let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(100.0, 1), b(150.0, 1), high(300.0), high(320.0)], vehicles: vec![] };
    let u: Vec<Unit> = (0..3).map(|j| Unit { pos: (0..3).map(|k| c(3 * j + k, 10.0).pos).collect(), previous: Some(0), avoid: Vec::new() }).collect();
    let r = split_up(10.0, &view, &[], &[None; 5], &u, &Levers { guard: false, ..Levers::default() });
    for i in 0..3 {
        assert!(r.iter().any(|d| d.is_some_and(|d| d.flag == i)), "flag {i} empty: {r:?}");
    }
}

/// B20, DC_Weapon_Bunkers: two bases that never change hands. Nobody waits at its own: the side goes to the enemy's.
#[test]
fn b20_with_nothing_to_take_the_side_goes_to_the_enemy() {
    let fixed = |x: f32, owner: u32| FlagSighting { takeable: false, ..b(x, owner) };
    let view = TeamView { team: 1, mates: vec![], flags: vec![fixed(0.0, 1), fixed(400.0, 2)], vehicles: vec![] };
    let u: Vec<Unit> = (0..4).map(|j| Unit { pos: (0..3).map(|k| c(3 * j + k, 10.0).pos).collect(), previous: None, avoid: Vec::new() }).collect();
    let r = split_up(10.0, &view, &[], &[None; 2], &u, &Levers::default());
    assert!(r.iter().all(|d| d.is_some_and(|d| d.flag == 1)), "{r:?}");
    // out of reach on foot, it is not a destination (that needs a flight)
    let mut v2 = view.clone();
    v2.flags[1].on_foot = false;
    assert!(split_up(10.0, &v2, &[], &[None; 2], &u, &Levers::default()).iter().all(|d| d.is_none()));
}

/// An own flag with an enemy on top calls the nearest, even if alone; the threatened one gets a defender.
/// GC_Mos_Eisley, team 1: both enemy flags are high up; nobody goes there, the front ones of its own are defended.
#[test]
fn what_cannot_be_taken_on_foot_is_not_attacked() {
    let high = |x: f32| FlagSighting { on_foot: false, ..b(x, 2) };
    let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(100.0, 1), high(300.0), high(320.0), b(-200.0, 1)], vehicles: vec![] };
    let u: Vec<Unit> = (0..4).map(|j| Unit { pos: (0..3).map(|k| c(3 * j + k, 10.0).pos).collect(), previous: Some(2), avoid: Vec::new() }).collect();
    let r = split_up(10.0, &view, &[], &[None; 5], &u, &Levers { guard: false, ..Levers::default() });
    assert!(r.iter().all(|d| d.is_some_and(|d| d.flag != 2 && d.flag != 3)), "{r:?}");
    assert!(r.iter().any(|d| d.unwrap().flag == 1), "the front one first: {r:?}");
    // within reach, they are attacked
    let mut v2 = view.clone();
    v2.flags[2].on_foot = true;
    assert!(split_up(10.0, &v2, &[], &[None; 5], &u, &Levers::default()).iter().any(|d| d.unwrap().flag == 2));
    // with a transport helicopter: still the one on land first (DC_Sea_Rigs); with none on land, the high ones (GC_Judicator)
    let heli = VehicleSighting { vehicle: 9, pos: [0.0; 3], team: 1, free_seats: 4, class: VehicleClass::Helicopter, health: 1.0, floats: false, seats: vec![0, 1, 2, 3], radius: 6.0, afloat: false, vtol: false };
    let mut v3 = view.clone();
    v3.vehicles.push(heli);
    v3.flags[2].owner = 1;
    let mut extra = v3.clone();
    extra.flags.push(b(200.0, 2));
    assert!(split_up(10.0, &extra, &[], &[None; 6], &u, &Levers { guard: false, ..Levers::default() }).iter().all(|d| d.is_some_and(|d| d.flag == 5 || d.flag == 1)), "the one on land");
    assert!(split_up(10.0, &v3, &[], &[None; 5], &u, &Levers { guard: false, ..Levers::default() }).iter().any(|d| d.unwrap().flag == 3), "by air when nothing is on land");
}

/// A squad with a ground leg that does not move for two windows fails it and, with no other way, avoids that flag for a while.
#[test]
fn a_squad_that_does_not_move_avoids_its_flag() {
    let mut e = Team::default();
    let mut view = TeamView { team: 1, mates: (0..4).map(|j| c(j, 0.0)).collect(), flags: vec![b(0.0, 1), b(600.0, 2), b(700.0, 2)], vehicles: vec![] };
    for m in view.mates.iter_mut() {
        m.pos[0] = -200.0;
    }
    let mut goals = Vec::new();
    for k in 0..12 {
        e.think(4.0 * k as f64, &view);
        goals.push(e.squads[0].goal);
    }
    assert_eq!(goals[0], Goal::Flag(1));
    assert!(e.squads[0].avoid.iter().any(|a| a.0 == 1), "{:?}", e.squads[0]);
    assert_eq!(*goals.last().unwrap(), Goal::Flag(2), "another flag: {goals:?}");
}

/// B20: the leader whose trip on foot fails for not advancing is told so, for a moment: a new flag does not unstick a body.
#[test]
fn b20_the_leader_that_does_not_advance_is_told_to_get_out() {
    let mut e = Team::default();
    let mut view = TeamView { team: 1, mates: (0..4).map(|j| c(j, 0.0)).collect(), flags: vec![b(0.0, 1), b(600.0, 2), b(700.0, 2)], vehicles: vec![] };
    for m in view.mates.iter_mut() {
        m.pos[0] = -200.0;
    }
    let mut told = Vec::new();
    for k in 0..12 {
        let t = 4.0 * k as f64;
        e.think(t, &view);
        let leader = e.squads[0].leader;
        if e.stuck_on_foot(leader, t) {
            told.push(t);
        }
        assert!(!e.stuck_on_foot(leader + 1, t), "only the leader");
    }
    assert!(!told.is_empty() && told[0] >= 2.0 * trip::WINDOW_S, "after two windows without moving: {told:?}");
    assert!(!e.stuck_on_foot(e.squads[0].leader, told[0] + 2.0), "and only for a moment");
}

#[test]
fn reach_on_foot_is_measured_in_3d() {
    let flat = |_: f32, _: f32| Some(1.0);
    assert!(reachable_on_foot([0.0, 13.0, 0.0], 18.0, 0.0, flat), "GC_Mos_Eisley 4: 12 m above the ground, radius 18");
    assert!(!reachable_on_foot([0.0, 22.0, 0.0], 18.0, 0.0, flat), "GC_Mos_Eisley 2");
    assert!(reachable_on_foot([0.0, 22.0, 0.0], 18.0, 0.0, |x, _| Some(if x > 10.0 { 10.0 } else { 1.0 })), "a high edge inside the circle");
    assert!(reachable_on_foot([0.0, 3.0, 0.0], 5.0, 2.5, |_, _| Some(-20.0)), "above the water, swimming");
}

#[test]
fn b18_the_flag_being_taken_calls_the_nearest() {
    let mut u: Vec<Unit> = (0..10).map(|j| Unit { pos: vec![c(j, 0.0).pos], previous: Some(1), avoid: Vec::new() }).collect();
    u.push(Unit { pos: vec![c(10, 880.0).pos], previous: Some(1), avoid: Vec::new() });
    let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(300.0, 2), b(900.0, 1)], vehicles: vec![] };
    let reports = [Report { t: 9.0, player: 40, pos: [905.0, 0.0, 0.0], vehicle: false }];
    let r = split_up(10.0, &view, &reports, &[None; 3], &u, &Levers::default());
    assert_eq!(r[10], Some(Destination { flag: 2, urgent: true }));
    assert!(r.iter().filter(|d| d.unwrap().urgent).count() <= 2);
}

#[test]
fn b25_the_nearest_free_medic_is_sent_once_per_call() {
    let mut e = Team::default();
    assert!(e.ask_medic(10.0, 5, &[7, 8]));
    assert_eq!(e.medic_call(7, 11.0), Some(5));
    assert_eq!(e.medic_call(8, 11.0), None);
    assert!(!e.ask_medic(11.0, 5, &[8]), "one medic per call");
    assert!(e.ask_medic(11.0, 6, &[7, 8]), "7 is busy: the next goes");
    assert_eq!(e.medic_call(8, 12.0), Some(6));
    assert!(!e.ask_medic(12.0, 9, &[]), "no medic to send");
    e.medic_call_done(5);
    assert_eq!(e.medic_call(7, 12.0), None);
    assert_eq!(e.medic_call(8, 10.0 + MEDIC_CALL_S + 2.0), None, "the order lapses");
}

#[test]
fn the_kit_is_recognised_by_what_it_carries() {
    let it = |n: &str| vec![(0, "Colt".to_string()), (1, n.to_string())];
    assert_eq!(Kind::de(&it("Medikit")), Kind::Medic);
    assert_eq!(Kind::de(&it("Wrench")), Kind::Engineer);
    assert_eq!(Kind::de(&it("Bazooka")), Kind::AntiTank);
    assert_eq!(Kind::de(&it("Thompson")), Kind::Assault);
}

/// Flat dry land, every point reached on foot (the way itself not known).
struct Dry;
impl trip::Land for Dry {
    fn reach(&self, _: [f32; 3], _: [f32; 3], _: f32) -> trip::Reach {
        trip::Reach::Path
    }
    fn wet(&self, _: [f32; 3], _: [f32; 3]) -> bool {
        false
    }
    fn below(&self, _: f32, _: f32) -> f32 {
        0.0
    }
    fn water(&self) -> f32 {
        -10.0
    }
}

/// `bots:regroup` as measured in phase 6: the squad held at the point gets [`Goal::Point`], which `squad::orders` skips, so its people
/// keep their orders to the flag: nobody is held. `bots:regroup-way` (R1) sends every one of the squad on foot to the point and keeps
/// the squad's flag, until enough are there or the wait runs out.
#[test]
fn regroup_way_holds_the_squad_at_its_point() {
    let flags = vec![b(0.0, 1), b(600.0, 2)];
    let view_at = |x: f32| TeamView { team: 1, mates: (0..8).map(|j| c(j, x + j as f32)).collect(), flags: flags.clone(), vehicles: vec![] };
    let team = |way: bool| {
        let mut e = Team::con(Parts { regroup: true, regroup_way: way, ..Parts::default() });
        e.set_levers(Levers { guard: false, ..Levers::default() });
        e
    };
    // the old one: sent at 500 m, held at 300 m, still ordered to the flag
    let mut old = team(false);
    old.think_on(0.0, &view_at(100.0), &Dry);
    old.think_on(4.0, &view_at(300.0), &Dry);
    assert!(old.squads.iter().any(|q| matches!(q.goal, Goal::Point(_))), "held: {:?}", old.squads);
    assert!((0..8).all(|j| old.order(j).goal == Goal::Flag(1)), "{:?}", (0..8).map(|j| old.order(j)).collect::<Vec<_>>());
    // R1: every one goes to its squad's point, on the way from where it comes (no map: the bearing), 125 m short of the flag
    let mut e = team(true);
    e.think_on(0.0, &view_at(100.0), &Dry);
    let v = view_at(300.0);
    e.think_on(4.0, &v, &Dry);
    assert!(e.squads.iter().all(|q| q.goal == Goal::Flag(1)), "the flag is kept: {:?}", e.squads);
    for j in 0..8 {
        let Goal::Point(p) = e.order(j).goal else { panic!("{j} not held: {:?}", e.order(j)) };
        assert!((p[0] - 475.0).abs() < 1.0 && p[1].abs() < 6.0, "{p:?}");
    }
    assert!(!e.waits_at_rally(0, v.mates[0].pos), "not there yet");
    // at the point: they wait there; all of them there, they go in together
    let mut v = view_at(470.0);
    v.mates[7].pos[0] = 380.0;
    e.think_on(8.0, &v, &Dry);
    assert!(e.waits_at_rally(0, v.mates[0].pos) && !e.waits_at_rally(7, v.mates[7].pos));
    assert!(matches!(e.order(7).goal, Goal::Point(_)));
    e.think_on(12.0, &view_at(470.0), &Dry);
    assert!((0..8).all(|j| e.order(j).goal == Goal::Flag(1)), "released: {:?}", (0..8).map(|j| e.order(j)).collect::<Vec<_>>());
    // the wait runs out: `rally_wait_s` after the first got there
    let mut e = team(true);
    e.params.rally_wait_s = 10.0;
    e.think_on(0.0, &view_at(100.0), &Dry);
    e.think_on(4.0, &view_at(300.0), &Dry);
    let mut v = view_at(470.0);
    v.mates[7].pos[0] = 300.0;
    e.think_on(8.0, &v, &Dry);
    e.think_on(12.0, &v, &Dry);
    assert!(matches!(e.order(0).goal, Goal::Point(_)));
    e.think_on(20.0, &v, &Dry);
    assert_eq!(e.order(0).goal, Goal::Flag(1), "{:?}", e.order(0));
}

/// `bots:regroup-way`: two squads coming by points nearer than `rally_merge_m` share one.
#[test]
fn regroup_way_merges_near_points() {
    let flags = vec![b(0.0, 1), b(600.0, 2)];
    let mut v = TeamView { team: 1, mates: (0..8).map(|j| c(j, 300.0)).collect(), flags, vehicles: vec![] };
    for (j, m) in v.mates.iter_mut().enumerate() {
        m.pos[2] = if j < 4 { -40.0 } else { 40.0 };
    }
    let points = |merge: f32| {
        let mut e = Team::con(Parts { regroup: true, regroup_way: true, ..Parts::default() });
        e.set_levers(Levers { guard: false, size: 4, ..Levers::default() });
        e.params.rally_merge_m = merge;
        e.think_on(0.0, &v, &Dry);
        e.think_on(4.0, &v, &Dry);
        (0..8).filter_map(|j| if let Goal::Point(p) = e.order(j).goal { Some([p[0] as i32, p[1] as i32]) } else { None }).collect::<BTreeSet<_>>()
    };
    assert_eq!(points(0.0).len(), 2, "each its own");
    assert_eq!(points(60.0).len(), 1, "shared");
}
