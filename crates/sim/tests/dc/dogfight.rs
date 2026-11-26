//! A bots pilot against another aircraft in the air. Skipped without the real data.
use bytefield_sim::damage::Status;
use bytefield_sim::soldier::axis;

type V3 = [f32; 3];

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// B26: a fighter with an enemy plane flying straight and level 500 m ahead of it goes after it, gets behind it and shoots it down
/// with its cannon, without help and without hitting the ground. The target is flown from outside with a player's stick and never
/// fires; the fighter is a bots mind sat in its seat.
#[test]
fn a_bots_fighter_shoots_down_a_plane_flying_straight() {
    for (m, w) in crate::support::dc_worlds("DC_No_Fly_Zone", "conquest") {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.set_minds(1, bytefield_sim::bots::maker(&w.level.name, 1));
        let (ka, kb) = (s.join(1, true), s.join(2, true));
        let mut soldiers = None;
        for _ in 0..900 {
            s.step();
            if let (Some(a), Some(b)) = (s.players[ka].soldier.filter(|_| s.players[ka].alive), s.players[kb].soldier.filter(|_| s.players[kb].alive)) {
                soldiers = Some((a, b));
                break;
            }
        }
        let (sa, sb) = soldiers.expect("the soldiers never spawned");
        let y = w.height_at(1024.0, 1024.0).unwrap_or(0.0) as f32 + 600.0;
        let ca = s.spawn_vehicle("f16", [1024.0, y, 524.0], [0.0; 3]).expect("f16");
        let cb = s.spawn_vehicle("mig29", [1024.0, y, 1024.0], [0.0; 3]).expect("mig29");
        for c in [ca, cb] {
            let b = &mut s.vehicles[c].craft.body;
            b.rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            b.vel = [0.0, 0.0, 110.0];
            b.rotvel = [0.0; 3];
        }
        s.enter(sa, ca, 0);
        s.enter(sb, cb, 0);
        let mut before = s.vehicles[cb].craft.body.rows;
        let (mut seen, mut shot_down, mut fired) = (s.journal.damage_log.len(), None, 0);
        for step in 0..(45 * 30) {
            // the target: wings level, at its height, at 90 m/s
            let b = &s.vehicles[cb].craft.body;
            let [r, u, f] = b.rows;
            let rates = [dot(f, before[1]), -dot(r, before[1])].map(|x| x.clamp(-1.0, 1.0).asin().to_degrees() * 30.0);
            before = b.rows;
            let speed = dot(b.vel, b.vel).sqrt();
            let want = ((y - b.pos[1]) * 0.4 - 0.5 * b.vel[1]).clamp(-12.0, 30.0);
            let k = (45.0 / speed.max(20.0)).powi(2).clamp(0.3, 3.0);
            let pitch = k * (0.05 * (want - f[1].clamp(-1.0, 1.0).asin().to_degrees()) - 0.02 * rates[0]);
            let bank = k * (0.02 * r[1].atan2(u[1]).to_degrees() - 0.006 * rates[1]);
            s.vehicles[sb].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, (0.6 + 0.05 * (90.0 - speed)).clamp(0.0, 1.0)), (1, -pitch.clamp(-1.0, 1.0)), (2, bank.clamp(-1.0, 1.0))]);
            s.step();
            fired += s.journal.fired.iter().rev().take_while(|e| e.0 == s.clock.steps).filter(|e| e.1 == ca).count();
            for e in &s.journal.damage_log[seen..] {
                if e.vehicle == cb && e.status == Some(Status::Destroyed) {
                    shot_down = Some((step as f32 / 30.0, e.by));
                }
            }
            seen = s.journal.damage_log.len();
            if shot_down.is_some() {
                break;
            }
        }
        let own = &s.vehicles[ca];
        assert!(own.armor.as_ref().is_none_or(|a| !a.destroyed), "{m}: the fighter was lost");
        assert_eq!(shot_down.map(|x| x.1), Some(Some(ca)), "{m}: not shot down by the fighter in 45 s ({fired} rounds fired)");
        eprintln!("{m}: shot down after {:.1} s with {fired} rounds", shot_down.unwrap().0);
    }
}
