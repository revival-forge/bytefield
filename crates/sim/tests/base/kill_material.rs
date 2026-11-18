//! Material 99 destroys what touches it: `SimpleObject`, `PlayerControlObject` and
//! the soldier's collision handling give 1e10 to themselves when the other side's material is 99,
//! before anything else. In vanilla it is a plane 40 cm under a building's floor (`barack_m1`:
//! up faces at 0.52 m of material 99, the floor at 0.93). In the original,
//! `pco158_willy_near.jsonl`: a Willy made by a spawner inside the barrack at
//! El_Alamein bursts two records later (its `WillyBackSpringR`, 1e10); 2.5 m away, outside the
//! barrack, it stays (`pco158_willy_far`). Skipped without the real data.
/// A Willy placed at `at` in El_Alamein: the frame it is destroyed, within `frames`.
fn willy(at: [f32; 3], frames: u64) -> Option<u64> {
    let w = bytefield_sim::World::load(&bytefield_testdata::base().unwrap(), "bf1942", "El_Alamein", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let v = s.spawn_vehicle("Willy", at, [0.0; 3]).expect("a Willy");
    for n in 1..=frames {
        s.step();
        if s.vehicles[v].armor.as_ref().is_some_and(|a| a.destroyed) {
            return Some(n);
        }
    }
    None
}

#[test]
fn a_willy_under_the_barrack_floor_bursts() {
    if bytefield_testdata::base().is_none() {
        return;
    }
    // the original's spawn points (the spawner's position)
    let near = willy([1672.11, 60.366, 851.237], 30);
    assert_eq!(near, Some(2), "inside the barrack: destroyed the second frame, as in the original");
    assert_eq!(willy([1671.351, 60.182, 849.387], 120), None, "outside it stays");
}
