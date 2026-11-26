//! A round's events with their place (`bytefield_sim::round_events`, what `tournament3 --events` writes): they add up with what
//! the sim counts (each death of a side once, the flags taken, the seconds of presence, a hit before each kill by a hit), and
//! reading them does not change the round. Skipped without the real data.
use bytefield_sim::round_events::RoundEvents;

const SECS: u64 = 150;
const PLAYERS: usize = 16;

/// One Berlin round of bots; with `events`, read by [`RoundEvents`]. Returns the round's outcome (each player's kills, deaths and
/// place, the tickets), the events and the alive seconds of each side's players.
fn round(w: &bytefield_sim::World, events: bool) -> (String, Option<serde_json::Value>, [u32; 2], Vec<(usize, u32, u32)>) {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    for i in 0..PLAYERS {
        let p = s.join(0, true);
        s.players[p].kit = (i / 2) % 5;
    }
    let mut e = events.then(RoundEvents::new);
    let mut alive = [0u32; 2];
    let mut flags: Vec<u32> = s.modes.conquest.as_ref().unwrap().points.iter().map(|p| p.team).collect();
    let mut changes = Vec::new();
    for _ in 0..SECS * 30 {
        s.step();
        if let Some(e) = e.as_mut() {
            e.step(&s);
        }
        if s.clock.steps % 30 == 0 {
            for p in s.players.iter().filter(|p| p.alive && p.soldier.is_some() && (1..=2).contains(&p.team)) {
                alive[p.team as usize - 1] += 1;
            }
        }
        for (i, f) in s.modes.conquest.as_ref().unwrap().points.iter().enumerate() {
            if flags[i] != f.team {
                changes.push((i, flags[i], f.team));
                flags[i] = f.team;
            }
        }
    }
    let outcome = format!(
        "{:?} {:?}",
        s.modes.conquest.as_ref().unwrap().tickets,
        s.players.iter().map(|p| (p.team, p.kills, p.deaths, p.soldier.map(|i| s.vehicles[i].craft.body.pos.map(f32::to_bits)))).collect::<Vec<_>>()
    );
    let deaths: u32 = s.players.iter().filter(|p| (1..=2).contains(&p.team)).map(|p| p.deaths).sum();
    let ev = e.map(|e| {
        let mut v = e.finish(&s);
        v["_deaths"] = deaths.into();
        v
    });
    (outcome, ev, alive, changes)
}

#[test]
fn round_events_add_up_and_leave_the_round_alone() {
    let game = bytefield_testdata::require!(Base);
    let w = bytefield_sim::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
    let (plain, _, _, _) = round(&w, false);
    let (read, ev, alive, changes) = round(&w, true);
    assert_eq!(plain, read, "reading the events changed the round");
    let ev = ev.unwrap();
    let deaths = ev["deaths"].as_array().unwrap();
    assert!(!deaths.is_empty(), "nobody died in {SECS} s: the test sees nothing");
    assert_eq!(deaths.len() as u64, ev["_deaths"].as_u64().unwrap(), "a death counted twice or missed");
    for d in deaths {
        assert!(matches!(d["team"].as_u64(), Some(1 | 2)));
        // a death by an enemy has its distance; the killer's side is the other one, or the same for a team kill
        if let Some(dist) = d["distance"].as_f64() {
            assert!((0.0..2000.0).contains(&dist), "{d}");
            assert!(d["killer_team"].as_u64().is_some(), "{d}");
        }
    }
    let captures: Vec<(u64, u64, u64)> = ev["captures"].as_array().unwrap().iter().map(|c| (c["flag"].as_u64().unwrap(), c["from"].as_u64().unwrap(), c["to"].as_u64().unwrap())).collect();
    let want: Vec<(u64, u64, u64)> = changes.iter().map(|&(i, a, b)| (i as u64, a as u64, b as u64)).collect();
    assert_eq!(captures, want, "the flags taken are not the flag changes");
    // presence: every alive second of a side's player, in one cell (on foot or in a vehicle)
    let mut seconds = [0u64; 2];
    for p in ev["presence"].as_array().unwrap() {
        seconds[p[0].as_u64().unwrap() as usize - 1] += p[4].as_u64().unwrap();
    }
    assert_eq!(seconds, alive.map(u64::from), "presence seconds are not the alive seconds");
    // the hits on soldiers: by and on a player who is in the round, and every enemy killed by a hit was hit by its killer first
    let hits = ev["hits"].as_array().unwrap();
    for h in hits {
        let (by, on) = (h[1].as_u64().unwrap() as usize, h[2].as_u64().unwrap() as usize);
        assert!(by < PLAYERS && on < PLAYERS && h[3].as_f64().unwrap() > 0.0, "{h}");
    }
    for d in deaths.iter().filter(|d| d["source"].as_str() == Some("hit") && d["killer_team"].as_u64().is_some_and(|k| Some(k) != d["team"].as_u64())) {
        let (k, p, t) = (d["killer"].as_u64().unwrap(), d["player"].as_u64().unwrap(), d["t"].as_f64().unwrap());
        assert!(hits.iter().any(|h| h[1].as_u64() == Some(k) && h[2].as_u64() == Some(p) && h[0].as_f64().unwrap() <= t), "no hit before {d}");
    }
    for a in ev["arrivals"].as_array().unwrap() {
        assert!(matches!(a["outcome"].as_str(), Some("taken" | "died" | "left" | "open")), "{a}");
        if let Some(sec) = a["sector"].as_u64() {
            assert!(sec < 8, "{a}");
            assert!(a["approach_s"].as_f64().unwrap() >= 0.0, "{a}");
        }
        assert!(a["end_t"].as_f64().unwrap() >= a["t"].as_f64().unwrap(), "{a}");
    }
}
