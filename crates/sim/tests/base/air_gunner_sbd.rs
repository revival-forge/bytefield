//! A bots mind in the rear seat of a dive bomber against an enemy fighter flying behind it. Skipped without the real data.
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

/// The SBD's rear gun (seat 1) turns 60 degrees either side and 40 up but only 1 degree down: a bots gunner in it hits a Zero 150 m
/// behind and 20 m above it. (A Zero level with the SBD or below is outside the arc and is never hit.)
#[test]
fn a_bots_gunner_in_the_sbds_rear_seat_hits_a_zero_above_and_behind_it() {
    let game = bytefield_testdata::require!(Base);
    let w = bytefield_sim::World::load(&game, "bf1942", "Midway", "conquest").unwrap();
    let mut seeds_with_hits = 0;
    for seed in 1..=3u64 {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.set_minds(1, bytefield_sim::bots::maker(&w.level.name, seed));
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
        let ca = s.spawn_vehicle("SBD", [1024.0, y, 350.0], [0.0; 3]).expect("SBD");
        let cb = s.spawn_vehicle("Zero", [1024.0, y + 20.0, 200.0], [0.0; 3]).expect("Zero");
        for c in [ca, cb] {
            let b = &mut s.vehicles[c].craft.body;
            b.rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            b.vel = [0.0, 0.0, 80.0];
            b.rotvel = [0.0; 3];
        }
        s.enter(sp, ca, 0);
        s.enter(sb, cb, 0);
        s.enter(sg, ca, 1);
        let (mut before_a, mut before_b) = (s.vehicles[ca].craft.body.rows, s.vehicles[cb].craft.body.rows);
        let (mut shots, mut hits, mut pending) = (0, 0, Vec::new());
        let mut seen = s.journal.fired.len();
        for _ in 0..(20 * 30) {
            for (c, soldier, before) in [(ca, sp, &mut before_a), (cb, sb, &mut before_b)] {
                let b = &s.vehicles[c].craft.body;
                let axes = level_stick(b.rows, before, b.vel, b.pos[1], if c == cb { y + 20.0 } else { y }, 80.0);
                s.vehicles[soldier].soldier_mut().unwrap().set_input(&axes);
            }
            s.step();
            for f in s.journal.fired.get(seen..).unwrap_or(&[]) {
                if f.1 == ca
                    && let Some(k) = f.3
                    && s.projectiles.get(k).is_some_and(|p| p.seat == 1)
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
    assert!(seeds_with_hits >= 2, "the gunner hit the Zero in only {seeds_with_hits} of 3 duels");
}
