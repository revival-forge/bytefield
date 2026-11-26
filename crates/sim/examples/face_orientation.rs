//! Whether the level's collision meshes (the finer LOD, the one the navigation mesh is built from) tell their outside from
//! their inside: each distinct mesh's faces' stored normals against their winding, whether the winding is consistent edge
//! by edge (each edge, keyed by its ends' positions, met once each way), and, placed in the world, the sign of the volume
//! the winding encloses (closed and consistent: the sign says which way is out). By the area of the placed faces.
//!
//!   face_orientation <game> <mod/level>...
use bytefield_sim::World;
use std::collections::{BTreeMap, HashMap};

/// A mesh's faces' winding against its stored normals (agreeing, of all), and how its edges pair: met once each way, or
/// twice the same way (wound against a neighbour), or once (open).
fn shape(mesh: &bytefield_sim::obstacle::FaceMesh) -> (usize, usize, usize, usize, usize) {
    let v = &mesh.vertices;
    let key = |i: u32| v[i as usize].map(|c| (c * 1000.0).round() as i64);
    // per edge (its ends in order): how often it is met from the lower end and from the higher
    let mut edges: HashMap<([i64; 3], [i64; 3]), [u32; 2]> = HashMap::new();
    let (mut agree, mut n) = (0, 0);
    for f in &mesh.faces {
        if f.v.iter().any(|&i| i as usize >= v.len()) {
            continue;
        }
        let [p, q, r] = f.v.map(|i| v[i as usize]);
        let e1 = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let e2 = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
        let c = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        if c.iter().map(|x| x * x).sum::<f32>() < 1e-12 {
            continue;
        }
        n += 1;
        agree += (c[0] * f.normal[0] + c[1] * f.normal[1] + c[2] * f.normal[2] > 0.0) as usize;
        for k in 0..3 {
            let (a, b) = (key(f.v[k]), key(f.v[(k + 1) % 3]));
            if a == b {
                continue;
            }
            let (lo, hi, way) = if a < b { (a, b, 0) } else { (b, a, 1) };
            edges.entry((lo, hi)).or_default()[way] += 1;
        }
    }
    let (mut paired, mut against, mut open) = (0, 0, 0);
    for w in edges.values() {
        match *w {
            [1, 1] => paired += 1,
            [1, 0] | [0, 1] => open += 1,
            _ => against += 1,
        }
    }
    (agree, n, paired, against, open)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = std::path::Path::new(a.get(1).expect("face_orientation <game> <mod/level>..."));
    println!("{:<28} {:>6} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8}", "level", "meshes", "agree%", "paired%", "out%", "in%", "open%", "mixed%");
    for l in &a[2..] {
        let (m, level) = l.split_once('/').expect("mod/level");
        let world = World::load(game, m, level, "conquest").expect("the level");
        let mut shapes: BTreeMap<usize, (usize, usize, usize, usize, usize)> = BTreeMap::new();
        let (mut agree, mut faces, mut paired, mut edges) = (0, 0, 0, 0);
        // by placed area: closed and wound outwards (winding volume > 0), inwards (< 0), open (an edge met once), mixed
        let (mut out_a, mut in_a, mut open_a, mut mixed_a, mut all) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for o in world.obstacles() {
            let Some(mesh) = bytefield_sim::obstacle::pick_lod(&o.lods, 1) else { continue };
            let key = mesh as *const _ as usize;
            let sh = *shapes.entry(key).or_insert_with(|| shape(mesh));
            agree += sh.0;
            faces += sh.1;
            paired += sh.2;
            edges += sh.2 + sh.3 + sh.4;
            let v: Vec<[f64; 3]> = mesh.vertices.iter().map(|&p| bytefield_sim::navmesh::place_point(o, p).map(|c| c as f64)).collect();
            let (mut vol, mut area) = (0.0f64, 0.0f64);
            for f in &mesh.faces {
                if f.v.iter().any(|&i| i as usize >= v.len()) {
                    continue;
                }
                let [p, q, r] = f.v.map(|i| v[i as usize]);
                let e1 = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
                let e2 = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
                let c = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                area += 0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
                // (about the first vertex of the mesh: the sum does not depend on the origin when closed, and keeps its digits)
                let o0 = v[0];
                let (p, q, r) = ([p[0] - o0[0], p[1] - o0[1], p[2] - o0[2]], [q[0] - o0[0], q[1] - o0[1], q[2] - o0[2]], [r[0] - o0[0], r[1] - o0[1], r[2] - o0[2]]);
                vol += (p[0] * (q[1] * r[2] - q[2] * r[1]) - p[1] * (q[0] * r[2] - q[2] * r[0]) + p[2] * (q[0] * r[1] - q[1] * r[0])) / 6.0;
            }
            all += area;
            if sh.4 > 0 {
                open_a += area;
            } else if sh.3 > 0 {
                mixed_a += area;
            } else if vol > 0.0 {
                out_a += area;
            } else {
                in_a += area;
            }
        }
        let pc = |x: f64| 100.0 * x / all.max(1e-9);
        println!("{:<28} {:>6} {:>7.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1}", l, shapes.len(), 100.0 * agree as f64 / faces.max(1) as f64, 100.0 * paired as f64 / edges.max(1) as f64, pc(out_a), pc(in_a), pc(open_a), pc(mixed_a));
    }
}
