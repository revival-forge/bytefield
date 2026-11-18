//! The portals of `.sm` meshes: each polygon's centre, its normal as
//! the AI works it out ((v2 − v0) × (v1 − v0), not made unit) and whether it points away from the
//! middle of the mesh's box.
//! `cargo run --release -p bytefield-formats --example portals -- <mesh.sm>…`
fn main() {
    for p in std::env::args().skip(1) {
        let m = bytefield_formats::sm::parse(&std::fs::read(&p).expect("read")).expect("parse");
        let mid: Vec<f32> = (0..3).map(|i| (m.bounds[0][i] + m.bounds[1][i]) / 2.0).collect();
        println!("{p}: {} portals, box {:?}", m.portals.len(), m.bounds);
        for poly in m.portals.iter().filter(|p| p.len() >= 3) {
            let k = poly.len() as f32;
            let c: Vec<f32> = (0..3).map(|i| poly.iter().map(|v| v[i]).sum::<f32>() / k).collect();
            let (a, b, d) = (poly[0], poly[1], poly[2]);
            let u = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
            let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let out = (0..3).map(|i| n[i] * (c[i] - mid[i])).sum::<f32>() > 0.0;
            println!("  {} vertices, centre {c:.2?}, normal {n:.2?}, outwards {out}", poly.len());
        }
    }
}
