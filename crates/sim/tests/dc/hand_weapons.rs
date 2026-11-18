//! DC_Final's hand weapons against a soldier standing on the ground, shot from 15 m behind at the head, the torso and a leg.
//! The numbers are the original game's:
//! 108 of 108 cases and the knives' 12 the same, `USSoldier` and `IraqSoldier`, DC_Al_Nas at (1000, 1000). Skipped without the real data.
const HEIGHT: [(&str, f32); 3] = [("head", 1.62), ("torso", 1.3), ("leg", 0.45)];

/// What the soldier lost to one shot of `weapon` at `height` over its feet.
fn shot(w: &bytefield_sim::World, soldier: &str, weapon: &str, height: f32) -> Vec<f32> {
    let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
    s.round.playing = true;
    let y = w.height_at(1000.0, 1000.0).unwrap() as f32;
    let at = [1000.0, y, 1000.0];
    let k = s.spawn_vehicle(soldier, at, [0.0; 3]).expect("the soldier");
    s.fire_barrel(weapon, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [1000.0, y + height, 985.0], Some([0.0; 3]), -1).expect("a projectile");
    for _ in 0..45 {
        let b = &mut s.vehicles[k].craft.body;
        (b.pos, b.vel, b.rotvel) = (at, [0.0; 3], [0.0; 3]);
        s.step();
    }
    s.journal.damage_log.iter().filter(|e| e.vehicle == k && e.status.is_some()).map(|e| e.amount).collect()
}

#[test]
fn dc_hand_weapons_hurt_a_soldier_like_the_original() {
    if !crate::support::has_level("DC_Final", "DC_Al_Nas") {
        eprintln!("no DC_Final, skipped");
        return;
    }
    let w = bytefield_sim::World::load(&crate::support::game(), "DC_Final", "DC_Al_Nas", "conquest").unwrap();
    // head, torso, leg
    let want: [(&str, [f32; 3]); 6] = [
        ("AK47", [25.0, 12.5, 9.5]),
        ("MP5", [15.0, 8.75, 2.5]),
        ("PKM", [35.0, 18.0, 15.5]),
        ("M82Sniper", [100.0, 100.0, 100.0]),
        ("Remington", [15.0, 8.75, 2.5]),
        ("RPG7", [150.0, 150.0, 150.0]),
    ];
    for soldier in ["USSoldier", "IraqSoldier"] {
        for (weapon, amounts) in want {
            for ((part, h), a) in HEIGHT.iter().zip(amounts) {
                let got = shot(&w, soldier, weapon, *h);
                assert!(got.len() == 1 && (got[0] - a).abs() < 1e-3, "{soldier} {weapon} {part}: {got:?}, the original's {a}");
            }
        }
    }
}
