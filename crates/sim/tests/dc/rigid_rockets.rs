//! DC's helicopter rockets (Hydra, Hellfire, AT2) are rigid projectiles whose template has no `hasMobilePhysics`:
//! they flew but had no contact points and went through the hills, so nothing a helicopter
//! fired at the ground went off. Here they are fired level, 3 m over DC_DustBowl's ground, at the rise ahead (from
//! 87 to 107 m in 150 m): they must end on it, not in the air beyond. Skipped without the real data.
#[test]
fn helicopter_rockets_hit_the_ground() {
    for (m, w) in crate::support::dc_worlds("DC_DustBowl", "conquest") {
        for weapon in ["AH64HydraRack", "AH64HellfireRack", "Mi24D_AT2Arms"] {
            let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            let (x, z) = (1082.0, 1740.0);
            let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 3.0;
            let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            let k = s.fire(weapon, rows, [x, y, z], None).unwrap_or_else(|| panic!("{m} {weapon}: no projectile"));
            for _ in 0..(4 * 30) {
                s.step();
            }
            let p = &s.projectiles[k];
            assert!(p.end.is_some(), "{m} {weapon}: still flying at {:?}", p.pos);
            let ground = w.height_at(p.pos[0] as f64, p.pos[2] as f64).unwrap() as f32;
            assert!((p.pos[1] - ground).abs() < 3.0 && p.pos[2] > z && p.pos[2] < z + 200.0, "{m} {weapon}: ended at {:?} (ground {ground})", p.pos);
        }
    }
}
