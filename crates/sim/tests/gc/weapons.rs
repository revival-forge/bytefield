//! GC's weapons that `examples/fire_all` found firing nothing. Skipped without the real data.
/// The B-wing's torpedo and the TIE bomber's rockets fire `SpaceRocket`, which GC creates as a
/// Projectile and then as a SimpleObject of the same name: the first wins,
/// so it is a projectile. With the second, none came out.
#[test]
fn the_space_rocket_is_a_projectile() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Vanguard", "conquest").unwrap();
    let t = w.projectile_template("SpaceRocket").expect("a projectile");
    assert!(t.has_collision_effect);
    for launcher in ["b-wing", "tiebomber_v"] {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        let (x, z) = (1024.0, 1024.0);
        let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 300.0;
        let l = s.spawn_vehicle(launcher, [x, y, z], [0.0; 3]).unwrap_or_else(|| panic!("{launcher}"));
        let soldier = s.spawn_vehicle("USSoldier", [x, y, z + 3.0], [0.0; 3]).unwrap();
        s.vehicles[soldier].team = 1;
        s.enter(soldier, l, 0);
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, 1.0), (bytefield_sim::soldier::axis::ALT_FIRE, 1.0)]);
        for _ in 0..90 {
            s.step();
        }
        let rockets = s.journal.fired.iter().filter(|f| f.3.is_some_and(|p| s.projectiles[p].template.name == "SpaceRocket")).count();
        assert!(rockets > 0, "{launcher}: no SpaceRocket");
    }
}
