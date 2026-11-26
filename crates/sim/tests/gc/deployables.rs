//! What a projectile carries in its tree: its
//! `ObjectSpawner`s deploy their object where it is (DC's mortar, GC's camouflage), its
//! `SupplyDepot`s supply while it lasts (GC's astromech repairs). Skipped without the real data.
/// The X-wing's astromech (seat 1) fires `Astromech_Droid_repair_projectile`, which carries
/// `Astromech_Droid_repair_point`: a damaged X-wing gets repaired while it lasts.
#[test]
fn the_gc_astromech_repairs() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let (x, z) = (1024.0, 1024.0);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 2.0;
    let xw = s.spawn_vehicle("x-wing", [x, y, z], [0.0; 3]).unwrap();
    let soldier = s.spawn_vehicle("USSoldier", [x, y, z + 4.0], [0.0; 3]).unwrap();
    s.vehicles[soldier].team = 2;
    s.vehicles[xw].team = 2;
    s.enter(soldier, xw, 1);
    let max = s.vehicles[xw].armor.as_ref().unwrap().max_hp;
    s.give_damage(xw, max * 0.5, bytefield_sim::damage::Source::Kill);
    let hp = s.vehicles[xw].armor.as_ref().unwrap().hp;
    s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, 1.0)]);
    for _ in 0..150 {
        s.step();
    }
    assert!(s.depots.iter().any(|d| d.on_projectile && d.template.name == "Astromech_Droid_repair_point"));
    let now = s.vehicles[xw].armor.as_ref().unwrap().hp;
    assert!(now > hp, "{hp} -> {now}");
}
