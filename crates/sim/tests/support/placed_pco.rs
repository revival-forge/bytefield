//! What the `placed_pco` tests of several layers share.

/// It is made where it was placed and, at rest, stays there (the original's mortar sleeps from the
/// first frame: a reference run of the original has one record); `x`: its x axis.
pub fn placed(m: &str, level: &str, template: &str, at: [f32; 3], x: Option<[f32; 3]>) {
    let Some(g) = bytefield_testdata::with_mod(m) else { return };
    let w = bytefield_sim::World::load(&g, m, level, "conquest").unwrap();
    assert!(w.placed_vehicles().iter().any(|(t, _, _)| t.eq_ignore_ascii_case(template)));
    assert!(!w.obstacles().iter().any(|o| o.name.eq_ignore_ascii_case(template)), "{template} is no longer an obstacle");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.start_conquest();
    let v = s.vehicles.iter().find(|v| v.template.eq_ignore_ascii_case(template)).expect("made as a vehicle");
    let i = s.vehicles.iter().position(|v| v.template.eq_ignore_ascii_case(template)).unwrap();
    if let Some(x) = x {
        let r = v.craft.body.rows[0];
        assert!((0..3).all(|k| (r[k] - x[k]).abs() < 1e-3), "{r:?}");
    }
    for _ in 0..150 {
        s.step();
    }
    let p = s.vehicles[i].craft.body.pos;
    assert!((0..3).all(|k| (p[k] - at[k]).abs() < 0.01), "{p:?}");
}
