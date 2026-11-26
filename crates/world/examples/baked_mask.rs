//! The terrain's baked shadow as the client's mask ([`ShadowSampler::baked_mask`]): written as a PGM (white in shadow, z down
//! as the tiles go), with how long it took, where it lies and a few points well inside the shadow, to look at.
//!   cargo run --release -p bytefield-world --example baked_mask -- data/game bf1942 El_Alamein mask.pgm [side]
use bytefield_world::{shadow, Level};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [game, m, level, out, rest @ ..] = a.as_slice() else { panic!("usage: <game> <mod> <level> <out.pgm> [side]") };
    let (_, vfs) = bytefield_con::session::mount(Path::new(game), m).expect("mount");
    let db = bytefield_con::session::load(&vfs, Some(level), "conquest").host.db;
    let lvl = Level::from_db(&vfs, &db, level).expect("level");
    let s = shadow::ShadowSampler::new(&vfs, &db, &lvl).expect("lsb");
    let (cols, rows, patch, _) = s.grid();
    let side = rest.first().map_or_else(|| (patch.round() as usize).min(4096 / cols.max(rows).max(1)), |v| v.parse().unwrap());
    let t = std::time::Instant::now();
    let (w, h, mask, at, reach) = s.baked_mask(side);
    println!("{w}x{h} in {:.0} ms; from x {:.0} z {:.0}, {:.0} x {:.0} m", t.elapsed().as_secs_f64() * 1e3, at[0], at[1], reach[0], reach[1]);
    let mut pgm = format!("P5\n{w} {h}\n255\n").into_bytes();
    pgm.extend(&mask);
    std::fs::write(out, pgm).expect("write");
    // a few texels whose 9 × 9 neighbourhood is all in shadow, spread over the map
    let mut found = 0;
    for y in (8..h.saturating_sub(8)).step_by((h / 23).max(1)) {
        for x in (8..w.saturating_sub(8)).step_by((w / 23).max(1)) {
            if (0..9).all(|j| (0..9).all(|i| mask[(y + j - 4) * w + x + i - 4] == 255)) && found < 12 {
                let (px, pz) = (at[0] + (x as f64 + 0.5) / w as f64 * reach[0], at[1] + (y as f64 + 0.5) / h as f64 * reach[1]);
                println!("in shadow: {px:.0},{pz:.0} (ground {:.1}, intensity {:.0})", lvl.engine_height_and_normal(px as f32, pz as f32).0, s.intensity(&lvl, [px as f32, lvl.engine_height_and_normal(px as f32, pz as f32).0, pz as f32]));
                found += 1;
            }
        }
    }
}
