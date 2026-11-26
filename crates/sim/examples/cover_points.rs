//! The cover points of some levels (`bytefield_sim::cover`): how many, how long they take, how many hide in each way, and a
//! top-down picture over the walking mesh's (`navmesh::view::picture`) around a point.
//!   cover_points <game> <mod/level>... [--threads 15] [--no-cache] [--flags] [--around x,z,r [--png out.png] [--px 0.25]]
//! The picture: each point a dot (white: a wall, it hides standing too in some way; cyan: only parapets; magenta: under a roof),
//! with a short tick towards each sector it hides in crouched (white: standing too).
use bytefield_sim::World;
use bytefield_sim::cover::{self, CoverPoints};
use bytefield_sim::navmesh;
use std::path::Path;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let flag = |k: &str| a.iter().any(|x| x == k);
    let levels: Vec<&String> = a.get(2..).unwrap_or_default().iter().take_while(|s| !s.starts_with("--")).collect();
    if levels.is_empty() {
        eprintln!("cover_points <game> <mod/level>... [--threads 15] [--no-cache] [--flags] [--around x,z,r [--png out.png] [--px 0.25]]");
        std::process::exit(2);
    }
    let game = Path::new(&a[1]);
    let threads: usize = opt("--threads").map_or(15, |v| v.parse().expect("--threads"));
    for ml in levels {
        let (m, level) = ml.split_once('/').expect("mod/level");
        let Ok(world) = World::load(game, m, level, "conquest") else {
            eprintln!("{ml}: no world");
            continue;
        };
        let mesh = world.navmesh();
        world.obstacles();
        let t = Instant::now();
        let (c, how) = if flag("--no-cache") { (CoverPoints::build(&world, &mesh, threads), cover::Cached::Built) } else { CoverPoints::load_or_build(&world, &mesh, threads, &CoverPoints::default_cache_dir(game)) };
        let secs = t.elapsed().as_secs_f64();
        let n = c.points.len();
        let walls = c.points.iter().filter(|p| p.stand != 0).count();
        let roofs = c.points.iter().filter(|p| p.roof).count();
        let mut ways = [0usize; 9];
        for p in &c.points {
            ways[p.crouch.count_ones() as usize] += 1;
        }
        let bytes = bincode::serialize(&c).map_or(0, |b| b.len());
        println!("== {ml}: {n} points ({walls} with a wall, {roofs} under a roof) in {secs:.2} s ({how:?}, {threads} threads), {:.1} MB; by sectors hidden 1..8: {:?}", bytes as f64 / 1e6, &ways[1..]);
        if flag("--flags") {
            for f in world.level.placed.iter().filter(|p| p.kind.as_deref().is_some_and(|k| k.eq_ignore_ascii_case("controlpoint"))) {
                println!("   flag {} at {:.0},{:.0}", f.template, f.pos[0], f.pos[2]);
            }
        }
        if let Some(r) = opt("--around") {
            let v: Vec<f32> = r.split(',').map(|v| v.parse().expect("--around x,z,r")).collect();
            let px = opt("--px").map_or(0.25, |v| v.parse().expect("--px"));
            let reached = mesh.reach(&navmesh::seeds(&world));
            let (mut img, _) = navmesh::view::picture(&mesh, &world, &reached, v[0], v[1], v[2], px, [f32::MIN, f32::MAX]);
            let (x0, z0) = (v[0] - v[2], v[1] - v[2]);
            let (w, h) = (img.width as i64, img.height as i64);
            let mut put = |x: f32, z: f32, col: [u8; 4]| {
                let (i, j) = (((x - x0) / px) as i64, ((z - z0) / px) as i64);
                if i >= 0 && j >= 0 && i < w && j < h {
                    img.put(i as usize, (h - 1 - j) as usize, col);
                }
            };
            let mut shown = 0;
            for p in c.near(v[0], v[1], v[2] * 1.5) {
                shown += 1;
                for s in 0..cover::SECTORS {
                    if p.crouch & (1 << s) == 0 {
                        continue;
                    }
                    let col = if p.stand & (1 << s) != 0 { [255, 255, 255, 255] } else { [0, 230, 255, 255] };
                    let ang = s as f32 * std::f32::consts::TAU / cover::SECTORS as f32;
                    for k in 1..=6 {
                        let d = k as f32 * 0.25;
                        put(p.pos[0] + d * ang.cos(), p.pos[2] + d * ang.sin(), col);
                    }
                }
                let col = if p.roof { [230, 40, 230, 255] } else if p.stand != 0 { [255, 255, 255, 255] } else { [0, 230, 255, 255] };
                for (dx, dz) in [(0.0, 0.0), (px, 0.0), (-px, 0.0), (0.0, px), (0.0, -px)] {
                    put(p.pos[0] + dx, p.pos[2] + dz, col);
                }
            }
            let out = opt("--png").map_or_else(|| format!("data/out/cover/{level}_{:.0}_{:.0}.png", v[0], v[1]), str::to_string);
            if let Some(d) = Path::new(&out).parent() {
                let _ = std::fs::create_dir_all(d);
            }
            img.write_png(Path::new(&out)).expect("the picture");
            println!("   around {},{} ({} m): {shown} points -> {out}", v[0], v[1], v[2]);
        }
    }
}
