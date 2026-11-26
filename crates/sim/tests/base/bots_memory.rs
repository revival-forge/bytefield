//! The bots' per-map memory (`bots:memory`, `bytefield_sim::bots::memory`) over a real level: a file is read only for its own
//! flags, and the tolls of a side's ways on foot fall on the polygons in the cells it learnt are worse than their baseline, and
//! nowhere else. Skipped without the real data.
use bytefield_sim::bots::memory::Memory;

#[test]
fn the_tolls_fall_on_the_dangerous_cells_only() {
    let game = bytefield_testdata::require!(Base);
    let w = bytefield_sim::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.start_conquest();
    let flags: Vec<[f32; 3]> = s.modes.conquest.as_ref().unwrap().points.iter().map(|p| p.pos).collect();
    let mesh = w.navmesh();
    // a cell round a polygon of the mesh: worse than its baseline for side 1, better for side 2
    let c = mesh.centre(mesh.polys.len() as u32 / 2);
    let (cx, cz) = ((c[0] / 32.0).floor() as i32, (c[2] / 32.0).floor() as i32);
    let side = |excess: f32| format!(r#"{{"foot": {{"mean": 0.01, "cells": [[{cx}, {cz}, 600.0, 30.0, 0.05, {excess}]]}}, "vehicle": {{"mean": 0.0, "cells": []}}}}"#);
    let text = format!(r#"{{"format": 2, "level": "Berlin", "flags": {}, "cell_m": 32.0, "sides": {{"1": {}, "2": {}}}}}"#, serde_json::to_string(&flags).unwrap(), side(0.02), side(-0.02));

    let dir = std::env::temp_dir().join(format!("bytefield-memory-berlin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Berlin.json"), &text).unwrap();
    let m = Memory::load(&dir, "Berlin", &flags).expect("made for these flags");
    let mut moved = flags.clone();
    moved[0][0] += 50.0;
    assert!(Memory::load(&dir, "Berlin", &moved).is_none(), "a file for other flags is left aside");
    std::fs::remove_dir_all(&dir).ok();

    let t1 = m.foot_tolls(&mesh, 1, 300.0);
    assert!(!t1.is_empty());
    for &(p, cm) in &t1 {
        let q = mesh.centre(p);
        assert_eq!(((q[0] / 32.0).floor() as i32, (q[2] / 32.0).floor() as i32), (cx, cz), "polygon {p} out of the cell");
        // at most one death's worth
        assert!(cm > 0 && cm <= 30_000, "{cm}");
    }
    assert!(m.foot_tolls(&mesh, 2, 300.0).is_empty(), "side 2 is better off there: no toll");
    assert!(m.foot_tolls(&mesh, 1, 0.0).is_empty(), "a death worth nothing: no toll");
}
