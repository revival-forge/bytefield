//! What the `rigid_hit_damage` tests of several layers share.

/// Every damage the target took while not destroyed (one on a wreck does nothing): (source, amount).
pub fn shot(w: &bytefield_sim::World, target: &str, yaw: f32, weapon: &str, down: bool) -> Vec<(String, f32)> {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    let y = w.height_at(1000.0, 1000.0).unwrap() as f32 + 30.0;
    let at = [1000.0, y, 1000.0];
    let k = s.spawn_vehicle(target, at, [yaw, 0.0, 0.0]).expect("the target");
    let (rows, muzzle) = if down {
        ([[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]], [1000.0, y + 11.0, 1000.0])
    } else {
        ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [1000.0, y + 1.0, 985.0])
    };
    s.fire_barrel(weapon, rows, muzzle, Some([0.0; 3]), -1).expect("a projectile");
    for _ in 0..45 {
        let b = &mut s.vehicles[k].craft.body;
        (b.pos, b.vel, b.rotvel) = (at, [0.0; 3], [0.0; 3]);
        s.step();
    }
    s.journal.damage_log.iter().filter(|e| e.vehicle == k && e.status.is_some()).map(|e| (format!("{:?}", e.source), e.amount)).collect()
}

pub fn check(what: &str, got: &[(String, f32)], want: &[(&str, f32)]) {
    let ok = got.len() == want.len() && got.iter().zip(want).all(|(g, w)| g.0 == w.0 && (g.1 - w.1).abs() < 0.05);
    assert!(ok, "{what}: {got:?}, the original's {want:?}");
}
