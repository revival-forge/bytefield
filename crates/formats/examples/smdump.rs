//! The bounds, collision meshes and materials of one `.sm` (and the `.rs` beside it): `smdump <file.sm>`.
//! `SMDUMP_VERTS=1` also prints each vertex of lod 0 (position, normal, uv), and each triangle's face normal against its vertices'.
use bytefield_formats::sm;

fn main() {
    let path = std::env::args().nth(1).expect("smdump <file.sm>");
    let m = sm::parse(&std::fs::read(&path).unwrap()).unwrap();
    println!("bounds {:?}", m.bounds);
    for (i, c) in m.collision.iter().enumerate() {
        let mut mats: Vec<u32> = c.bsp_faces.iter().map(|f| f.material).collect();
        mats.sort();
        mats.dedup();
        println!("collision {i} verts {} faces {} materials {mats:?}", c.vertices.len(), c.faces.len());
    }
    if m.collision.is_empty() {
        println!("no collision mesh ({} unknown sections)", m.unknown_sections);
    }
    for (i, lod) in m.lods.iter().enumerate() {
        for mat in &lod.materials {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &mat.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            let (mut ulo, mut uhi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for u in &mat.uvs {
                for k in 0..2 {
                    ulo[k] = ulo[k].min(u[k]);
                    uhi[k] = uhi[k].max(u[k]);
                }
            }
            println!(
                "lod {i} {} verts {} tris {} pos {lo:?}..{hi:?} uv {ulo:?}..{uhi:?}",
                mat.name,
                mat.positions.len(),
                mat.triangles.len()
            );
            if i == 0 && std::env::var_os("SMDUMP_VERTS").is_some() {
                for (k, p) in mat.positions.iter().enumerate() {
                    let n = mat.normals.get(k).copied().unwrap_or_default();
                    let u = mat.uvs.get(k).copied().unwrap_or_default();
                    println!("  v{k} p {:.3} {:.3} {:.3} n {:.2} {:.2} {:.2} uv {:.3} {:.3}", p[0], p[1], p[2], n[0], n[1], n[2], u[0], u[1]);
                }
                for (k, t) in mat.triangles.iter().enumerate() {
                    let [a, b, c] = t.map(|x| mat.positions[x as usize]);
                    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    let f = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                    let l = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt().max(1e-9);
                    let vn = t.map(|x| mat.normals.get(x as usize).copied().unwrap_or_default());
                    let avg = [0, 1, 2].map(|j| (vn[0][j] + vn[1][j] + vn[2][j]) / 3.0);
                    let dot = (f[0] * avg[0] + f[1] * avg[1] + f[2] * avg[2]) / l;
                    println!("  t{k} {:?} face {:.2} {:.2} {:.2} vs normals {dot:.2}", t, f[0] / l, f[1] / l, f[2] / l);
                }
            }
        }
    }
    // the shadow LODs: what the original's silhouette pass draws
    for (i, lod) in m.shadow_lods.iter().enumerate() {
        for mat in &lod.materials {
            println!("shadow lod {i} {} verts {} tris {}", mat.name, mat.positions.len(), mat.triangles.len());
        }
    }
    if let Ok(rs) = std::fs::read_to_string(std::path::Path::new(&path).with_extension("rs")) {
        println!("{rs}");
    }
}
