//! Which shift of the sampling (x, z in metres) and which sun direction best give the original's intensities for a set of
//! objects: each `x,y,z,radius,want` (want: the TF's red byte − ShadowAmbient, or 255 for lit).
//!   cargo run --release -p bytefield-world --example shadow_fit -- data/game XPack1 cassino 527.2,150.7,401.3,2.19,255 …
use bytefield_world::{shadow, Level};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [game, m, level, rest @ ..] = a.as_slice() else { panic!("usage") };
    let (_, vfs) = bytefield_con::session::mount(Path::new(game), m).expect("mount");
    let db = bytefield_con::session::load(&vfs, Some(level), "conquest").host.db;
    let lvl = Level::from_db(&vfs, &db, level).expect("level");
    let s = shadow::ShadowSampler::new(&vfs, &db, &lvl).expect("lsb");
    let pts: Vec<[f32; 5]> = rest.iter().map(|r| {
        let v: Vec<f32> = r.split(',').map(|x| x.parse().unwrap()).collect();
        [v[0], v[1], v[2], v[3], v[4]]
    }).collect();
    let vote = |o: [f32; 3], r: f32, shift: [f32; 2], flip: bool| -> f32 {
        let at = |p: [f32; 3]| s.intensity(&lvl, [p[0] + shift[0], p[1], p[2] + shift[1]]);
        if at(o) >= 100.0 {
            return 255.0;
        }
        let (mut lit, mut dark, mut sum) = (0u32, 0u32, 0u32);
        for p in s.fan(o, if flip { -r } else { r }) {
            let i = at(p);
            if i >= 100.0 { lit += 1 } else { dark += 1; sum = (sum as f32 + i) as u32 }
        }
        if dark > lit { (sum / dark) as f32 } else { 255.0 }
    };
    let mut best = Vec::new();
    for flip in [false, true] {
        for ix in -8..=8 {
            for iz in -8..=8 {
                let shift = [ix as f32 * 0.25, iz as f32 * 0.25];
                let (mut err, mut miss) = (0.0, 0);
                for p in &pts {
                    let got = vote([p[0], p[1], p[2]], p[3], shift, flip);
                    if (got >= 255.0) != (p[4] >= 255.0) { miss += 1 } else { err += (got - p[4]).abs() }
                }
                best.push((miss, err, shift, flip));
            }
        }
    }
    best.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
    for b in best.iter().take(8) {
        println!("misses {} error {:.0} shift {:?} flip {}", b.0, b.1, b.2, b.3);
    }
    let zero = best.iter().find(|b| b.2 == [0.0, 0.0] && !b.3).unwrap();
    println!("as now: misses {} error {:.0}", zero.0, zero.1);
    if let Ok(v) = std::env::var("SHIFT") {
        let v: Vec<f32> = v.split(',').map(|x| x.parse().unwrap()).collect();
        let b = best.iter().find(|b| b.2 == [v[0], v[1]] && !b.3).unwrap();
        println!("shift {v:?}: misses {} error {:.0}", b.0, b.1);
    }
}
