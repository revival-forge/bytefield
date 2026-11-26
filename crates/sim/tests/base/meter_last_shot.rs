//! The quality meter in the step a round ends: a shot fired in that very step names a projectile `clear_world` has already
//! taken away, and the meter must read it as gone (it panicked: a tournament on Coral_Sea, two seeds of four at 8 minutes).
//! Skipped without the real data.
use bytefield_sim::soldier::axis;

/// One Berlin round with a soldier holding the trigger: returns the step of a shot fired after `from` (a dry run), or, with
/// `end_at`, empties team 2's tickets just before that step and steps the meter through it.
fn round(w: &bytefield_sim::World, end_at: Option<u64>) -> (Option<u64>, usize) {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let shooter = s.join(1, true);
    s.join(2, true);
    let mut meter = bytefield_sim::quality::Meter::new();
    for _ in 0..(40 * 30) {
        if end_at == Some(s.clock.steps + 1) {
            s.modes.conquest.as_mut().unwrap().tickets[1] = 0;
        }
        if let Some(i) = s.players[shooter].soldier.filter(|_| s.players[shooter].alive) {
            // a trigger pulled and let go every other step: a rifle that fires once per pull fires too
            let fire = if s.clock.steps % 2 == 0 { 1.0 } else { 0.0 };
            s.vehicles[i].soldier_mut().unwrap().set_input(&[(axis::FIRE, fire)]);
        }
        let before = s.journal.fired.len();
        s.step();
        meter.step(&s);
        let shot = s.journal.fired[before..].iter().find(|f| f.3.is_some()).map(|f| f.0);
        if end_at.is_none() && s.clock.steps > 300 && shot.is_some() {
            return (shot, s.projectiles.len());
        }
        if end_at.is_some() && end_at == shot {
            return (shot, s.projectiles.len());
        }
    }
    (None, 0)
}

#[test]
fn a_shot_in_the_step_the_round_ends_does_not_break_the_meter() {
    let game = bytefield_testdata::require!(Base);
    let w = bytefield_sim::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
    let (step, _) = round(&w, None);
    let step = step.expect("the soldier never fired with a projectile");
    // the same round (the sim is deterministic), now ending in the step of that shot
    let (again, left) = round(&w, Some(step));
    assert_eq!(again, Some(step), "the shot did not come in the same step the second time");
    assert_eq!(left, 0, "the round did not end in the step of the shot: its projectile is still there");
}
