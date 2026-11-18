//! A spawner's vehicle is not made while a human's soldier is near (a gap in the original's reference runs):
//! `createObjectOnAllClients` returns nothing for a land or air `PlayerControlObject`
//! (not the Wasserfall; never a sea one) with a living human on foot within 2 m, and the spawner
//! asks again every frame. In the original (El_Alamein: a spawner made by
//! console by the local player, a human until record 120) the Willy 1.7 m away and the Spitfire
//! wait until he is a bot again and come the next spawner frame; the Willy 3 m away comes at once.
//! Skipped without the real data.
/// The first land spawner of El_Alamein emptied (its vehicle gone, its countdown up), `template`
/// in it and player `ai`'s soldier `off` from where it makes it: whether it makes its vehicle in
/// `frames`, and in the frame after the player turns a bot.
fn case(template: &str, off: [f32; 3], ai: bool, frames: usize) -> (bool, bool) {
    let g = bytefield_testdata::base().unwrap();
    let w = bytefield_sim::World::load(&g, "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.start_conquest();
    let k = s.spawners.iter().position(|sp| sp.active && sp.parent == bytefield_sim::create::Parent::Level && w.held_near_humans(&sp.template)).expect("a land spawner");
    if let Some(bytefield_sim::create::Made::Vehicle(i)) = s.spawners[k].last {
        s.vehicles[i].removed = true;
    }
    let sp = &mut s.spawners[k];
    sp.template = template.to_string();
    sp.slots.iter_mut().for_each(|o| *o = None);
    sp.last = None;
    sp.countdown = 0.0;
    let at = sp.at;
    let p = s.add_player(1, "Soldier", ai);
    let soldier = s.spawn_vehicle("GermanDesertSoldier", [at[0] + off[0], at[1] + off[1], at[2] + off[2]], [0.0; 3]).expect("a soldier");
    s.players[p].soldier = Some(soldier);
    s.players[p].alive = true;
    let made = |s: &bytefield_sim::Sim| s.journal.spawn_log.iter().any(|&(_, j, _)| j == k);
    for _ in 0..frames {
        s.step();
    }
    let early = made(&s);
    s.players[p].ai = true;
    s.step();
    (early, made(&s))
}

#[test]
fn a_human_near_holds_a_land_vehicle_back() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    // (the original's: 0.5 m ahead on the ground, the soldier's position 1.6 m above it)
    assert_eq!(case("Willy", [0.4, 1.5, 0.3], false, 60), (false, true), "held while he is near, made the frame after");
    assert_eq!(case("Willy", [0.4, 1.5, 0.3], true, 60).0, true, "a bot does not hold it");
    assert_eq!(case("Willy", [2.0, 1.5, 1.0], false, 60).0, true, "3 m away it comes at once");
}

#[test]
fn air_waits_and_sea_does_not() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    assert_eq!(case("Spitfire", [0.4, 1.5, 0.3], false, 60), (false, true));
    let w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "El_Alamein", "conquest").unwrap();
    assert!(!w.held_near_humans("Lcvp") && !w.held_near_humans("Elco80"), "a sea vehicle is never held");
    assert!(w.held_near_humans("Willy") && w.held_near_humans("Spitfire"));
    assert!(!w.held_near_humans("GermanDesertSoldier") && !w.held_near_humans("Willy_spawner_none"));
}
