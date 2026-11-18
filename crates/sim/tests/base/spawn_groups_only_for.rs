//! `spawnPointManager.onlyForAI` / `onlyForHuman`: a group
//! closed to people or to bots (the spawn group's active lookup. On
//! Battle of Britain (conquest) team 2 has groups 64–68 for bots only and 74–77 for
//! people only. Skipped without the data.
#[test]
fn battle_of_britain_groups_open_to_one_kind() {
    let Some(game) = bytefield_testdata::with_mod("bf1942") else { return; };
    let w = bytefield_sim::World::load(&game, "bf1942", "Battle_of_Britain", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.start_conquest();
    let c = s.modes.conquest.as_ref().unwrap();
    for g in 64..=68 {
        assert!(c.groups[&g].only_ai && !c.groups[&g].only_human, "group {g}");
    }
    for g in 74..=77 {
        assert!(c.groups[&g].only_human && !c.groups[&g].only_ai, "group {g}");
    }
    let person = s.add_player(2, "Soldier", false);
    let bot = s.add_player(2, "Soldier", true);
    let (mine, theirs) = (s.spawn_groups(person), s.spawn_groups(bot));
    assert!(!mine.is_empty() && !theirs.is_empty());
    assert!(mine.iter().all(|g| !(64..=68).contains(g)), "{mine:?}");
    assert!(theirs.iter().all(|g| !(74..=77).contains(g)), "{theirs:?}");
}

/// Coral Sea's first group of team 2 only has points for bots: a person who does not
/// choose a group waited for ever; it spawns at the first one that has a point for it.
#[test]
fn a_person_with_no_group_spawns_where_it_can() {
    let Some(game) = bytefield_testdata::with_mod("bf1942") else { return; };
    let w = bytefield_sim::World::load(&game, "bf1942", "Coral_Sea", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let person = s.add_player(2, "USSoldier", false);
    for _ in 0..30 * 40 {
        s.step();
    }
    assert!(s.players[person].alive, "groups {:?}", s.spawn_groups(person));
}
