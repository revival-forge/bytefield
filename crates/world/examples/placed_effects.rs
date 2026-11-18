//! The effects a level places (`scene::expand_level(..).effects`) near a point, to find which one covers a capture.
//!   cargo run --release -p bytefield-world --example placed_effects -- data/game GCMOD GC_Alaris_Prime 532 906 [50]
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [game, m, level, x, z, ..] = a.as_slice() else { panic!("usage: <game> <mod> <level> <x> <z> [radius]") };
    let (x, z): (f64, f64) = (x.parse().unwrap(), z.parse().unwrap());
    let reach: f64 = a.get(5).map_or(50.0, |r| r.parse().unwrap());
    let (_, vfs) = bytefield_con::session::mount(Path::new(game), m).expect("mount");
    let db = bytefield_con::session::load(&vfs, Some(level), "conquest").host.db;
    for (name, at, i) in bytefield_world::scene::expand_level(&db).effects {
        let p = [0, 1, 2].map(|k| at.0[3][k]);
        if (p[0] - x).hypot(p[2] - z) <= reach {
            println!("{name:<30} {:8.2} {:7.2} {:8.2}  instance of {}", p[0], p[1], p[2], db.instances[i].template);
        }
    }
}
