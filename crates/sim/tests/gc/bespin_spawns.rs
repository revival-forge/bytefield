//! GC_Bespin's flags all start neutral, but the level places `echo_chair_imp` and
//! `echo_chair_reb`, bundles with three spawn points each of the global groups 70 (team 1) and
//! 67 (team 2). The original counts them: with nobody on the server its ticket timers stay at
//! 60 / rate (a reference run of the original, and gc_bespin_branch.jsonl: `gameStatusPlaying`
//! takes the branch with groups, never `canTeamGetAnySpawnGroup`). Without them we drained both
//! sides at the end rate and the round was over in 14 s. Skipped without the real data.
#[test]
fn bespin_spawns_at_the_chairs_and_keeps_its_tickets() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Bespin", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let c = s.modes.conquest.as_ref().unwrap();
    for (g, team, x) in [(70, 1, 277.66), (67, 2, 1766.87)] {
        let g = &c.groups[&g];
        assert_eq!((g.team, g.spawns.len()), (team, 3));
        assert!(g.spawns.iter().all(|p| (p.pos[0] - x).abs() < 21.0 && (p.pos[1] - 280.24).abs() < 0.01), "{:?}", g.spawns.iter().map(|p| p.pos).collect::<Vec<_>>());
    }
    for _ in 0..30 * 20 {
        s.step();
    }
    assert_eq!(s.modes.conquest.as_ref().unwrap().tickets, [160, 160]);
}
