//! How lit the terrain's baked shadow leaves a point and an object there: the origin's
//! `getIntensity`, and the five-point vote for a few radii (`RADII=0.5,1`), or the mesh's own (half its box's diagonal)
//! when one is given, to set against the original's TF (TF = ShadowAmbient + intensity, to 255).
//!   cargo run --release -p bytefield-world --example object_shadow -- data/game XPack1 Husky 1339.9,42.24,1423.64,Eubed_M1 1352,43,1429
use bytefield_world::{shadow, Level};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [game, m, level, rest @ ..] = a.as_slice() else { panic!("usage: <game> <mod> <level> <x,y,z[,mesh]> …") };
    let (_, vfs) = bytefield_con::session::mount(Path::new(game), m).expect("mount");
    let db = bytefield_con::session::load(&vfs, Some(level), "conquest").host.db;
    let lvl = Level::from_db(&vfs, &db, level).expect("level");
    let Some(s) = shadow::ShadowSampler::new(&vfs, &db, &lvl) else { return println!("no LightmapShadowBits.lsb: all lit") };
    println!("ShadowAmbient {:?}, ShadowSamplingCullY {}", s.ambient, s.cull_y);
    let radii: Vec<f32> = std::env::var("RADII").map_or(vec![0.5, 1.0, 2.0, 3.0, 5.0], |v| v.split(',').map(|x| x.parse().unwrap()).collect());
    for arg in rest {
        let f: Vec<&str> = arg.split(',').collect();
        let p: [f32; 3] = [0, 1, 2].map(|i| f[i].parse().unwrap());
        let mesh = f.get(3).and_then(|m| vfs.read(&format!("standardmesh/{m}.sm")).ok()).and_then(|b| bytefield_formats::sm::parse(&b).ok());
        let rs = mesh.map_or(radii.clone(), |m| vec![shadow::radius(m.bounds)]);
        let ground = lvl.engine_height_and_normal(p[0], p[2]).0;
        let votes: Vec<String> = rs.iter().map(|&r| format!("r{r:.2}:{:.0}", s.object_intensity(&lvl, p, r))).collect();
        println!("{:8.1} {:6.1} {:8.1} {:<16} ground {ground:6.1}  origin {:5.1}  {}", p[0], p[1], p[2], f.get(3).unwrap_or(&""), s.intensity(&lvl, p), votes.join(" "));
        if std::env::var_os("FAN").is_some() {
            for &r in &rs {
                let pts: Vec<String> = s.fan(p, r).iter().map(|q| format!("{:.1},{:.1}:{:.0}", q[0], q[2], s.intensity(&lvl, *q))).collect();
                println!("    r{r:.2}  {}", pts.join("  "));
            }
        }
    }
}
