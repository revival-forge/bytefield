//! Each tree's TEXTUREFACTOR near a point, as `bytefield_world::shadow` works it out: to compare with the original's rendering
//! or a screenshot of the original.
//!   cargo run --release -p bytefield-world --example tree_shadow -- data/game bf1942 Tobruk 1983 688 [30]
use bytefield_world::{shadow, Level};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [game, m, level, x, z, ..] = a.as_slice() else { panic!("usage: <game> <mod> <level> <x> <z> [radius]") };
    let (x, z): (f64, f64) = (x.parse().unwrap(), z.parse().unwrap());
    let reach: f64 = a.get(5).map_or(30.0, |r| r.parse().unwrap());
    let (_, vfs) = bytefield_con::session::mount(Path::new(game), m).expect("mount");
    let db = bytefield_con::session::load(&vfs, Some(level), "conquest").host.db;
    let lvl = Level::from_db(&vfs, &db, level).expect("level");
    let Some(s) = shadow::ShadowSampler::new(&vfs, &db, &lvl) else { return println!("no LightmapShadowBits.lsb: all lit") };
    println!("ShadowAmbient {:?}, ShadowSamplingCullY {}, grid {:?}", s.ambient, s.cull_y, s.grid());
    for d in bytefield_world::scene::expand_level(&db).draws.iter().filter(|d| d.kind.eq_ignore_ascii_case("treemesh")) {
        let p = [0, 1, 2].map(|i| d.world.0[3][i]);
        if (p[0] - x).hypot(p[2] - z) > reach {
            continue;
        }
        let tm = vfs.read(&format!("treemesh/{}.tm", d.mesh)).ok().and_then(|b| bytefield_formats::tm::parse(&b).ok());
        let r = tm.as_ref().map_or(0.0, |t| shadow::radius(t.bounds));
        if std::env::var_os("BOUNDS").is_some() {
            if let Some(t) = &tm {
                println!("  {} bounds {:?} sprites {:?} (radius {:.1} / {:.1})", d.mesh, t.bounds, t.sprite_bounds, r, shadow::radius(t.sprite_bounds));
            }
        }
        let pf = p.map(|v| v as f32);
        let (origin, i) = (s.intensity(&lvl, pf), tm.as_ref().map_or(shadow::LIT, |t| s.mesh_intensity(&lvl, pf, t.bounds)));
        let colour = shadow::instance_colour(&db.instances[d.instance]);
        let f = shadow::tree_factor(i, s.ambient, colour);
        println!("{:<26} {:8.3} {:6.3} {:8.3}  r {:4.2}  colour {:.3}  origin {:5.1}  intensity {:5.1}  TF {:02x}{:02x}{:02x}", d.mesh, p[0], p[1], p[2], r, colour[0], origin, i, f[0], f[1], f[2]);
    }
}
