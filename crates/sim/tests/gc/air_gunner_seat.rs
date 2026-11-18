//! A bots3 mind in a turret of an aircraft against an enemy aircraft flying behind it. Skipped without the real data.
use bytefield_sim::soldier::axis;

type V3 = [f32; 3];

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The stick of a player who holds a height and a heading, level at `speed` (the same law the air bench uses for its drones).
fn level_stick(rows: [V3; 3], before: &mut [V3; 3], vel: V3, y: f32, target_y: f32, speed: f32) -> Vec<(usize, f32)> {
    let [r, u, f] = rows;
    let u0 = before[1];
    let rates = [dot(f, u0), -dot(r, u0)].map(|x| x.clamp(-1.0, 1.0).asin().to_degrees() * 30.0);
    *before = rows;
    let now = dot(vel, vel).sqrt();
    let want = ((target_y - y) * 0.4 - 0.5 * vel[1]).clamp(-12.0, 30.0);
    let k = (45.0 / now.max(20.0)).powi(2).clamp(0.3, 3.0);
    let pitch = k * (0.05 * (want - f[1].clamp(-1.0, 1.0).asin().to_degrees()) - 0.02 * rates[0]);
    let bank = k * (0.02 * r[1].atan2(u[1]).to_degrees() - 0.006 * rates[1]);
    vec![(axis::THROTTLE, (0.6 + 0.05 * (speed - now)).clamp(0.0, 1.0)), (1, -pitch.clamp(-1.0, 1.0)), (2, bank.clamp(-1.0, 1.0))]
}

/// B26: the Falcon's lower turret (seat 2), with a bots3 mind in it and the pilot's seat flown straight and level from outside, shoots at a
/// TIE fighter flying 250 m behind it, in the same direction, and hits it. (Its upper turret, seat 1, cannot point below the horizon:
/// no hits from there, which is why the seat is this one.)
#[test]
fn a_bots3_gunner_in_the_falcons_lower_turret_hits_a_tie_behind_it() {
    let game = bytefield_testdata::require!(Gc);
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Tatooine", "conquest").unwrap();
    let mut seeds_with_hits = 0;
    for seed in 1..=3u64 {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.set_minds(1, bytefield_sim::bots3::maker(&w.level.name, seed));
        let (kp, kb, kg) = (s.join(1, true), s.join(2, true), s.join(1, true));
        let alive = |s: &bytefield_sim::Sim, k: usize| s.players[k].soldier.filter(|_| s.players[k].alive);
        let mut soldiers = None;
        for _ in 0..900 {
            s.step();
            if let (Some(p), Some(b), Some(g)) = (alive(&s, kp), alive(&s, kb), alive(&s, kg)) {
                soldiers = Some((p, b, g));
                break;
            }
        }
        let (sp, sb, sg) = soldiers.expect("the soldiers never spawned");
        // the pilot is flown from outside: the team's mind must not take it
        s.players[kp].ai = false;
        let y = w.height_at(1024.0, 350.0).unwrap_or(0.0) as f32 + 600.0;
        let ca = s.spawn_vehicle("milleniumfalcon", [1024.0, y, 350.0], [0.0; 3]).expect("milleniumfalcon");
        let cb = s.spawn_vehicle("tiefighter", [1024.0, y, 100.0], [0.0; 3]).expect("tiefighter");
        for c in [ca, cb] {
            let b = &mut s.vehicles[c].craft.body;
            b.rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            b.vel = [0.0, 0.0, 110.0];
            b.rotvel = [0.0; 3];
        }
        s.enter(sp, ca, 0);
        s.enter(sb, cb, 0);
        s.enter(sg, ca, 2);
        let (mut before_a, mut before_b) = (s.vehicles[ca].craft.body.rows, s.vehicles[cb].craft.body.rows);
        let (mut shots, mut hits, mut pending) = (0, 0, Vec::new());
        let mut seen = s.journal.fired.len();
        for _ in 0..(20 * 30) {
            for (c, soldier, before) in [(ca, sp, &mut before_a), (cb, sb, &mut before_b)] {
                let b = &s.vehicles[c].craft.body;
                let axes = level_stick(b.rows, before, b.vel, b.pos[1], y, 110.0);
                s.vehicles[soldier].movement.as_mut().unwrap().set_input(&axes);
            }
            s.step();
            for f in s.journal.fired.get(seen..).unwrap_or(&[]) {
                if f.1 == ca
                    && let Some(k) = f.3
                    && s.projectiles.get(k).is_some_and(|p| p.seat == 2)
                {
                    shots += 1;
                    pending.push(k);
                }
            }
            seen = s.journal.fired.len();
            pending.retain(|&k| {
                let Some(p) = s.projectiles.get(k) else { return false };
                if p.impacts.iter().any(|i| i.vehicle == Some(cb)) {
                    hits += 1;
                    return false;
                }
                p.end.is_none()
            });
        }
        eprintln!("seed {seed}: the gunner fired {shots} rounds, {hits} hit");
        assert!(shots > 0, "seed {seed}: the gunner never fired");
        seeds_with_hits += (hits > 0) as u32;
    }
    assert!(seeds_with_hits >= 2, "the gunner hit the TIE in only {seeds_with_hits} of 3 duels");
}
