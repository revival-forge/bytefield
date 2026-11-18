//! Each tree near a point with what its billboard needs: the template's `billboard` and `billboardDistance`, the
//! instance's scale and the mesh box; to compare with the leaves' fade (the alpha of the TEXTUREFACTOR in the original's rendering,
//!) from a camera.
//!   cargo run --release -p bytefield-world --example tree_billboard -- data/game XPack1 salerno 510.8 357.8 [120]
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [game, m, level, x, z, ..] = a.as_slice() else { panic!("usage: <game> <mod> <level> <x> <z> [radius]") };
    let (x, z): (f64, f64) = (x.parse().unwrap(), z.parse().unwrap());
    let reach: f64 = a.get(5).map_or(120.0, |r| r.parse().unwrap());
    let (_, vfs) = bytefield_con::session::mount(Path::new(game), m).expect("mount");
    let db = bytefield_con::session::load(&vfs, Some(level), "conquest").host.db;
    for d in bytefield_world::scene::expand_level(&db).draws.iter().filter(|d| d.kind.eq_ignore_ascii_case("treemesh")) {
        let p = [0, 1, 2].map(|i| d.world.0[3][i]);
        if (p[0] - x).hypot(p[2] - z) > reach {
            continue;
        }
        let s = [0, 1, 2].map(|r| (0..3).map(|c| d.world.0[r][c].powi(2)).sum::<f64>().sqrt());
        let t = db.get("geometrytemplate", &d.mesh);
        let prop = |k: &str| t.and_then(|t| t.get(k)).and_then(|a| a.first()).cloned().unwrap_or_default();
        let b = vfs.read(&format!("treemesh/{}.tm", d.mesh)).ok().and_then(|b| bytefield_formats::tm::parse(&b).ok()).map(|t| t.bounds);
        println!("{:<22} {:8.1} {:6.1} {:8.1}  scale {:.3}/{:.3}/{:.3}  billboard {:>1} dist {:>4}  box {:?}", d.mesh, p[0], p[1], p[2], s[0], s[1], s[2], prop("billboard"), prop("billboarddistance"), b);
    }
}
