//! GC's shield: the engineer's `Build_Shield` throws a
//! `ShieldProjectile` whose tree holds the `Obstacle ShieldBlock` (`shield_down`, 12 m wide), and
//! the obstacle goes with the projectile while it lasts. Against the original game on GC_BCanyon:
//! a soldier running into it is slowed down while crossing its shell and
//! goes through (`shield_run.jsonl`, an `Obstacle` as barbed wire); a DL-44 bolt stops on it, from
//! the front or from behind, at the same point (`shield_shot_{front,back}.jsonl`). Skipped without
//! the real data.
use bytefield_sim::projectile::Shot;

const AT: [f32; 3] = [1320.8, 75.225, 1559.6];

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn world() -> Option<bytefield_sim::World> {
    let g = bytefield_testdata::game();
    g.join("Mods/GCMOD").is_dir().then(|| bytefield_sim::World::load(&g, "GCMOD", "GC_BCanyon", "conquest").unwrap())
}

/// The shield made at rest where the original's console put it (`Object.create` + `activate`).
fn shield(s: &mut bytefield_sim::Sim) -> usize {
    s.add_projectile("ShieldProjectile", &Shot { pos: AT, rows: IDENTITY, vel: [0.0; 3], rotvel: [0.0; 3] }).expect("a shield")
}

#[test]
fn the_shield_carries_its_obstacle() {
    let Some(w) = world() else { return };
    let local = w.carried_obstacles("ShieldProjectile");
    assert_eq!(local.len(), 1);
    let o = &local[0];
    assert!(o.name.eq_ignore_ascii_case("ShieldBlock") && o.obstacle_class && !o.projectiles_pass);
    // the `setPosition 0/-0.9/2` after it is the glow's
    assert_eq!(o.pos, [0.0; 3]);
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let k = shield(&mut s);
    for _ in 0..60 {
        s.step();
    }
    // it lies on the ground (the original's: 74.927) with its obstacle where it is
    let p = s.projectiles[k].pos;
    assert!((p[1] - 74.927).abs() < 0.02, "{p:?}");
    let c = s.obstacles().carried;
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].pos, c[0].carrier), (p, Some(k)));
    assert_eq!(s.obstacles().iter().count(), w.obstacles().len() + 1);
}

/// The original's soldier: from its record 40 on, `c_PIThrottle` 1 (6.13 m/s towards −z); it
/// crosses the shell between z 1565.2 and 1563.6 (records 76 to 89) slowed down (0.4 × its speed
/// while it touches), with no impulse back and no damage (the `ShieldBlock` of `Objects.con` says
/// `damage 0`), and runs on. Its z at records 70 to 100 (ours one step earlier: a record is the
/// state at the start of the server's update).
const RUN_Z: [f32; 31] = [
    1566.429, 1566.224, 1566.020, 1565.816, 1565.611, 1565.407, 1565.205, 1565.049, 1564.938, 1564.857, 1564.772, 1564.635, 1564.448, 1564.308, 1564.212, 1564.130,
    1564.048, 1563.963, 1563.826, 1563.636, 1563.432, 1563.227, 1563.023, 1562.818, 1562.614, 1562.410, 1562.205, 1562.001, 1561.796, 1561.592, 1561.387,
];

/// Its y and z at records 105 to 115, over the projectile's own box (`shield_hand`, at its feet):
/// it does not stumble, as a soldier does not collide with a projectile.
const OVER_BOX: [[f32; 2]; 11] = [
    [75.937, 1560.365], [75.936, 1560.161], [75.933, 1559.956], [75.928, 1559.751], [75.924, 1559.547], [75.919, 1559.342], [75.914, 1559.138], [75.910, 1558.934],
    [75.905, 1558.729], [75.900, 1558.525], [75.896, 1558.320],
];

#[test]
fn a_soldier_goes_through_the_shield_slowed_down() {
    let Some(w) = world() else { return };
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    shield(&mut s);
    let sol = s.spawn_vehicle("RussianSoldier", [1320.84, 75.992, 1571.599], [180.0, 0.0, 0.0]).unwrap();
    s.vehicles[sol].give_kit(w.kit_weapons("Rebel_Engineer"), None);
    let hp = s.vehicles[sol].armor.as_ref().unwrap().hp;
    let (mut z, mut yz) = (Vec::new(), Vec::new());
    for f in 0..240 {
        let axes: &[(usize, f32)] = if f >= 40 { &[(bytefield_sim::soldier::axis::THROTTLE, 1.0)] } else { &[] };
        s.vehicles[sol].soldier_mut().unwrap().set_input(axes);
        s.step();
        let p = s.vehicles[sol].craft.body.pos;
        z.push(p[2]);
        yz.push([p[1], p[2]]);
        // it does not push the shield (its `ShieldBlock` is no part of the projectile's body, and
        // the projectile's own box does not touch a soldier)
        assert!((s.projectiles[0].pos[2] - 1559.599).abs() < 0.001, "{f}: {:?}", s.projectiles[0].pos);
    }
    for (i, want) in RUN_Z.iter().enumerate() {
        let got = z[70 + i - 1];
        assert!((got - want).abs() < 0.003, "record {}: {got} (original {want})", 70 + i);
    }
    for (i, want) in OVER_BOX.iter().enumerate() {
        let got = yz[105 + i - 1];
        assert!((0..2).all(|k| (got[k] - want[k]).abs() < 0.003), "record {}: {got:?} (original {want:?})", 105 + i);
    }
    assert!(z[239] < 1536.0, "it goes through: {}", z[239]);
    assert_eq!(s.vehicles[sol].armor.as_ref().unwrap().hp, hp);
}

/// The original's DL-44 (made by the console, 1.5 m above the ground, `fireBarrel` with no player):
/// its bolt (its first record: where and how fast it left, with its deviation) stops on the
/// `ShieldBlock` (material 45) at z 1564.82 from the front and 1564.83 from behind (`rel` of the
/// hit + the segment's end). The reference run's unmanned weapon fires from the rigid updates, early: the
/// shield is still falling, at y 75.209 and 75.176.
#[test]
fn a_bolt_stops_on_the_shield_from_either_side() {
    let Some(w) = world() else { return };
    let bolts = [
        ([1320.8, 76.56, 1571.6], [-0.3785, 0.2482, -340.0], [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]], 75.209, [1320.7924, 76.5518, 1564.8206]),
        ([1320.8, 76.089, 1549.6], [0.3785, 0.2482, 340.0], IDENTITY, 75.176, [1320.8172, 76.0679, 1564.8294]),
    ];
    for (pos, vel, rows, y, want) in bolts {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        let j = shield(&mut s);
        while s.projectiles[j].pos[1] > y + 0.005 {
            s.step();
        }
        let k = s.add_projectile("Dl44Projectile", &Shot { pos, rows, vel, rotvel: [0.0; 3] }).expect("a bolt");
        for _ in 0..3 {
            s.step();
        }
        let p = &s.projectiles[k];
        assert!(!p.alive());
        let hit = p.impacts.first().expect("a hit");
        assert_eq!(hit.material, 45, "{hit:?}");
        assert!(hit.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case("ShieldBlock")), "{hit:?}");
        // (the original's point is `rel` + the end of the segment as the reference run has it: from behind it
        // falls 3 cm under the bolt's straight line, on the same face, so 1 cm further in z)
        assert!((0..3).all(|i| (hit.point[i] - want[i]).abs() < 0.04), "{hit:?}");
    }
}
