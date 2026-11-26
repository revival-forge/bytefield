//! The walkable-surface navigation mesh (`bytefield_sim::navmesh`) of some levels: generation time, size, polygons, the
//! seams between tiles and the ladders' links, and how much of it is reached from the spawn points and flags; a top-down
//! picture around a point.
//!   navmesh <game> <mod/level>... [--threads 4] [--cache DIR | --no-cache] [--no-detail] [--tile 128] [--cs 0.25] [--ch 0.1]
//!           [--climb 0.45] [--around x,z,r [--png out.png] [--px 0.25] [--band y0,y1]] [--ladders] [--seeds] [--at x,z[,x1,z1]] [--island x,y,z] [--drops x,y,z] [--jumps x,y,z] [--openings x,y,z] [--path x,y,z,x1,y1,z1]
use bytefield_sim::World;
use bytefield_sim::navmesh::{self, Cached, NavMesh, Params};
use std::path::Path;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let flag = |k: &str| a.iter().any(|x| x == k);
    let levels: Vec<&String> = a.get(2..).unwrap_or_default().iter().take_while(|s| !s.starts_with("--")).collect();
    if levels.is_empty() {
        eprintln!("navmesh <game> <mod/level>... [--threads 4] [--cache DIR | --no-cache] [--no-detail] [--around x,z,r [--png out.png]]");
        std::process::exit(2);
    }
    let game = Path::new(&a[1]);
    let threads: usize = opt("--threads").map_or(4, |v| v.parse().expect("--threads"));
    let cache = opt("--cache").map_or_else(|| NavMesh::default_cache_dir(game), Into::into);
    for ml in levels {
        let (m, level) = ml.split_once('/').expect("mod/level");
        let Ok(world) = World::load(game, m, level, "conquest") else {
            eprintln!("{ml}: no world");
            continue;
        };
        let t0 = Instant::now();
        world.obstacles();
        let t_load = t0.elapsed().as_secs_f64();
        let mut p = Params::infantry(&world);
        let num = |k: &str, d: f32| opt(k).map_or(d, |v| v.parse().expect(k));
        p.cs = num("--cs", p.cs);
        p.ch = num("--ch", p.ch);
        p.climb = num("--climb", p.climb);
        p.radius = num("--radius", p.radius);
        p.tile = num("--tile", p.tile as f32) as u32;
        p.detail = !flag("--no-detail");
        let t1 = Instant::now();
        let (mesh, how) = if flag("--no-cache") { (NavMesh::build(&world, &p, threads), Cached::Built) } else { NavMesh::load_or_build(&world, &p, threads, &cache) };
        let t_gen = t1.elapsed().as_secs_f64();
        let mut secs = [0.0f64; 4];
        for s in &mesh.stats {
            for i in 0..4 {
                secs[i] += s.secs[i] as f64;
            }
        }
        let seeds = navmesh::seeds(&world);
        let reached = mesh.reach(&seeds);
        let total: f64 = (0..mesh.polys.len() as u32).map(|q| mesh.area(q) as f64).sum();
        let on: f64 = (0..mesh.polys.len() as u32).filter(|&q| reached[q as usize]).map(|q| mesh.area(q) as f64).sum();
        let file = NavMesh::cache_path(&world, &p, &cache);
        println!(
            "== {ml}: {} m; {} tiles of {} m; {:?} in {t_gen:.2} s on {threads} threads (cpu: raster {:.1}, regions {:.1}, polys {:.1}, detail {:.1}); obstacles loaded in {t_load:.1} s",
            world.level.terrain.world_size,
            mesh.side * mesh.side,
            mesh.tile_m(),
            how,
            secs[0],
            secs[1],
            secs[2],
            secs[3]
        );
        println!(
            "   {} polygons ({} on the terrain), {} seams, {} ladder links, {} drops, {} jumps, {} narrow doors, {} ways out, {:.1} MB in memory, {:.1} MB on disk; {:.0} m² walkable, {:.0} % reached from {} seeds; fingerprint {:016x}",
            mesh.polys.len(),
            mesh.polys.iter().filter(|p| p.ground).count(),
            mesh.seams.len(),
            mesh.links.iter().filter(|l| l.kind == navmesh::LinkKind::Ladder).count(),
            mesh.links.iter().filter(|l| l.kind == navmesh::LinkKind::Drop).count(),
            mesh.links.iter().filter(|l| l.kind == navmesh::LinkKind::Jump).count(),
            mesh.links.iter().filter(|l| l.kind == navmesh::LinkKind::Door).count(),
            mesh.links.iter().filter(|l| l.kind == navmesh::LinkKind::Exit).count(),
            mesh.bytes() as f64 / 1e6,
            std::fs::metadata(&file).map_or(0.0, |m| m.len() as f64 / 1e6),
            total,
            100.0 * on / total.max(1.0),
            seeds.len(),
            mesh.fingerprint()
        );
        if flag("--seeds") {
            // each seed: the polygon it starts from (or none within 8 m and 3 m up or down), and how much it alone reaches
            for s in &seeds {
                let p = mesh.nearest(s[0], s[1], s[2], 8.0, 3.0);
                let alone = p.map_or(0.0, |_| {
                    let r = mesh.reach(&[*s]);
                    (0..mesh.polys.len() as u32).filter(|&q| r[q as usize]).map(|q| mesh.area(q) as f64).sum::<f64>()
                });
                let below = mesh.nearest(s[0], s[1], s[2], 8.0, 200.0).map(|q| mesh.closest_on(q, s[0], s[2])[1] - s[1]);
                println!("   seed {:.0},{:.0},{:.0}: polygon {p:?}, reaches {alone:.0} m²; nearest floor any height {below:?}", s[0], s[1], s[2]);
            }
        }
        if let Some(at) = opt("--at") {
            // the floors over a line of points: x0,z0,x1,z1 every 0.25 m (or one point x,z)
            let v: Vec<f32> = at.split(',').map(|v| v.parse().expect("--at x,z[,x1,z1]")).collect();
            let (b, e) = if v.len() >= 4 { ([v[0], v[1]], [v[2], v[3]]) } else { ([v[0], v[1]], [v[0], v[1]]) };
            let n = ((e[0] - b[0]).hypot(e[1] - b[1]) / 0.25).ceil().max(0.0) as usize;
            for k in 0..=n {
                let t = if n == 0 { 0.0 } else { k as f32 / n as f32 };
                let (x, z) = (b[0] + (e[0] - b[0]) * t, b[1] + (e[1] - b[1]) * t);
                let mut floors: Vec<String> = Vec::new();
                for p in 0..mesh.polys.len() as u32 {
                    let q = mesh.closest_on(p, x, z);
                    if (q[0] - x).abs() < 1e-4 && (q[2] - z).abs() < 1e-4 {
                        floors.push(format!("{:.2}{} (#{p})", q[1], if reached[p as usize] { "*" } else { "" }));
                    }
                }
                println!("   at {x:.2},{z:.2}: {}", floors.join("  "));
            }
        }
        if let Some(at) = opt("--path") {
            // the way from x,y,z to x1,y1,z1 (feet): its polygons and the corners to pass by
            let v: Vec<f32> = at.split(',').map(|v| v.parse().expect("--path x,y,z,x1,y1,z1")).collect();
            let q = bytefield_sim::navmesh::query::Query::new(std::sync::Arc::new(mesh.clone()));
            match q.corridor([v[0], v[1], v[2]], [v[3], v[4], v[5]]) {
                Some((a, steps, b)) => {
                    println!("   path from {a:?} to {b:?}: {} steps", steps.len());
                    for s in &steps {
                        println!("     {s:?}");
                    }
                    for c in q.path([v[0], v[1], v[2]], [v[3], v[4], v[5]]).unwrap_or_default() {
                        println!("   corner {:.1},{:.1},{:.1}", c[0], c[1], c[2]);
                    }
                }
                None => println!("   no path"),
            }
        }
        if let Some(at) = opt("--terrain") {
            // the terrain over a line x0,z0,x1,z1 every metre: height, slope, material, water
            let v: Vec<f64> = at.split(',').map(|v| v.parse().expect("--terrain x0,z0,x1,z1")).collect();
            let n = (v[2] - v[0]).hypot(v[3] - v[1]).ceil() as usize;
            for k in 0..=n {
                let t = k as f64 / n.max(1) as f64;
                let (x, z) = (v[0] + (v[2] - v[0]) * t, v[1] + (v[3] - v[1]) * t);
                let hn = world.level.height_and_normal(x, z, bytefield_formats::image::Orientation::IDENTITY);
                let slope = hn.map_or(0.0, |(_, n)| n[1].clamp(-1.0, 1.0).acos().to_degrees());
                println!("   terrain {x:.1},{z:.1}: {:?} m, slope {slope:.0}°, material {}, water {}", hn.map(|h| h.0), world.level.material_at(x, z), world.level.terrain.water_level);
            }
        }
        if let Some(at) = opt("--island") {
            // the island under x,y,z: its size, and the reached floor nearest to its open edges (how far across, how far up)
            let v: Vec<f32> = at.split(',').map(|v| v.parse().expect("--island x,y,z")).collect();
            if let Some(p0) = mesh.nearest(v[0], v[1], v[2], 3.0, 3.0) {
                let isl = mesh.reach(&[mesh.centre(p0)]);
                let polys: Vec<u32> = (0..mesh.polys.len() as u32).filter(|&q| isl[q as usize]).collect();
                let area: f32 = polys.iter().map(|&q| mesh.area(q)).sum();
                println!("   island of #{p0}: {} polygons, {area:.0} m²", polys.len());
                let mut gaps: Vec<(f32, f32, [f32; 3])> = Vec::new();
                for &q in &polys {
                    let vs: Vec<[f32; 3]> = mesh.poly_verts(q).collect();
                    let pq = mesh.polys[q as usize];
                    for k in 0..vs.len() {
                        if mesh.nei[pq.first as usize + k] != navmesh::NONE {
                            continue;
                        }
                        let (a, b) = (vs[k], vs[(k + 1) % vs.len()]);
                        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
                        if let Some(r) = nearest_other(&mesh, &isl, &reached, m) {
                            let c = mesh.closest_on(r, m[0], m[2]);
                            gaps.push(((c[0] - m[0]).hypot(c[2] - m[2]), c[1] - m[1], m));
                        }
                    }
                }
                gaps.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.abs().total_cmp(&b.1.abs())));
                for g in gaps.iter().take(8) {
                    println!("     open edge at {:.1},{:.1},{:.1}: reached floor {:.2} m across, {:+.2} m up", g.2[0], g.2[1], g.2[2], g.0, g.1);
                }
            }
        }
        if let Some(at) = opt("--openings") {
            // from a soldier's feet at a point, the bearings where it sees farther at a window's height than at the knee: openings
            // over a sill (how far the wall is at 0.3, 1.2 and 2.0 m over the feet)
            let v: Vec<f32> = at.split(',').map(|x| x.parse().expect("--openings x,y,z")).collect();
            let free = |deg: f32, h: f32| {
                let (dx, dz) = (deg.to_radians().cos(), deg.to_radians().sin());
                (1..=60).map(|k| k as f32 * 0.25).find(|&m| world.walk_blocked([v[0], v[1] + h, v[2]], [v[0] + m * dx, v[1] + h, v[2] + m * dz])).unwrap_or(99.0)
            };
            for k in 0..72 {
                let deg = k as f32 * 5.0;
                let d = [0.3, 1.2, 2.0].map(|h| free(deg, h));
                if d[1] > d[0] + 0.5 || flag("--all") {
                    println!("   bearing {deg:3.0}° (x {:+.2}, z {:+.2}): wall at {:.2} / {:.2} / {:.2} m", deg.to_radians().cos(), deg.to_radians().sin(), d[0], d[1], d[2]);
                }
            }
        }
        if let Some(at) = opt("--links") {
            // every link with an end within r of a point across: kind, polygons and ends
            let v: Vec<f32> = at.split(',').map(|x| x.parse().expect("--links x,z,r")).collect();
            for l in &mesh.links {
                if [l.from, l.to].iter().any(|e| (e[0] - v[0]).hypot(e[2] - v[1]) <= v[2]) {
                    println!("   {:?} {} -> {}: {:.1},{:.1},{:.1} -> {:.1},{:.1},{:.1}", l.kind, l.a, l.b, l.from[0], l.from[1], l.from[2], l.to[0], l.to[1], l.to[2]);
                }
            }
        }
        if let Some(at) = opt("--column") {
            // what is right over and under a point: the first thing blocking straight up and straight down from it (m)
            let v: Vec<f32> = at.split(',').map(|x| x.parse().expect("--column x,y,z")).collect();
            let o = [v[0], v[1], v[2]];
            let up = (1..=40).map(|k| k as f32 * 0.1).find(|&h| world.walk_blocked(o, [o[0], o[1] + h, o[2]]));
            let down = (1..=40).map(|k| k as f32 * 0.1).find(|&h| world.walk_blocked(o, [o[0], o[1] - h, o[2]]));
            println!("   column at {:.1},{:.1},{:.1}: blocked {:?} m up, {:?} m down; terrain {:?}", o[0], o[1], o[2], up, down, world.height_at(o[0] as f64, o[2] as f64));
        }
        if let Some(at) = opt("--jumps") {
            // the jumps off the polygons within 3 m of a point, spot by spot along their open edges, or why there is none
            let v: Vec<f32> = at.split(',').map(|x| x.parse().expect("--jumps x,y,z")).collect();
            for p in 0..mesh.polys.len() as u32 {
                let c = mesh.closest_on(p, v[0], v[2]);
                if (c[0] - v[0]).hypot(c[2] - v[2]) > 3.0 || (c[1] - v[1]).abs() > 2.0 {
                    continue;
                }
                for (e, r) in mesh.jumps_from(&world, p) {
                    if (e[0] - v[0]).hypot(e[2] - v[2]) > 4.0 {
                        continue;
                    }
                    match r {
                        Ok((b, l, kind)) => println!("   {kind:?} from {p} at {:.1},{:.1},{:.1}: to polygon {b} at {:.1},{:.1},{:.1} ({:+.1} m)", e[0], e[1], e[2], l[0], l[1], l[2], l[1] - e[1]),
                        Err(why) => println!("   no jump from {p} at {:.1},{:.1},{:.1}: {why}", e[0], e[1], e[2]),
                    }
                }
            }
        }
        if let Some(at) = opt("--drops") {
            // the drops off the polygon at a point, spot by spot along its open edges, or why there is none
            let v: Vec<f32> = at.split(',').map(|x| x.parse().expect("--drops x,y,z")).collect();
            match mesh.nearest(v[0], v[1], v[2], 2.0, 2.0) {
                None => println!("   no polygon at {at}"),
                Some(p) => {
                    for (e, r) in mesh.drops_from(&world, p) {
                        match r {
                            Ok((b, l)) => println!("   drop at {:.1},{:.1},{:.1}: to polygon {b} at {:.1},{:.1},{:.1} ({:.1} m down)", e[0], e[1], e[2], l[0], l[1], l[2], e[1] - l[1]),
                            Err(why) => println!("   no drop at {:.1},{:.1},{:.1}: {why}", e[0], e[1], e[2]),
                        }
                    }
                }
            }
        }
        if flag("--ladders") {
            for l in mesh.links.iter().filter(|l| l.kind == navmesh::LinkKind::Ladder) {
                println!("   ladder {:.0},{:.0},{:.0} -> {:.0}: polygons {} ({}) -> {} ({})", l.from[0], l.from[1], l.from[2], l.to[1], l.a, reached[l.a as usize], l.b, reached[l.b as usize]);
            }
        }
        if let Some(r) = opt("--around") {
            let v: Vec<f32> = r.split(',').map(|v| v.parse().expect("--around x,z,r")).collect();
            let px = opt("--px").map_or(p.cs, |v| v.parse().expect("--px"));
            let band = opt("--band").map_or([f32::MIN, f32::MAX], |b| {
                let v: Vec<f32> = b.split(',').map(|v| v.parse().expect("--band y0,y1")).collect();
                [v[0], v[1]]
            });
            let (img, legend) = navmesh::view::picture(&mesh, &world, &reached, v[0], v[1], v[2], px, band);
            let out = opt("--png").map_or_else(|| format!("data/out/navmesh/{level}_{:.0}_{:.0}.png", v[0], v[1]), str::to_string);
            if let Some(d) = Path::new(&out).parent() {
                let _ = std::fs::create_dir_all(d);
            }
            img.write_png(Path::new(&out)).expect("the picture");
            println!("   around {},{} ({} m): {legend:?} -> {out}", v[0], v[1], v[2]);
        }
    }
}

/// The reached polygon outside the island `isl` nearest to `m` (within 3 m across and 10 m up or down).
fn nearest_other(mesh: &NavMesh, isl: &[bool], reached: &[bool], m: [f32; 3]) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for p in 0..mesh.polys.len() as u32 {
        if isl[p as usize] || !reached[p as usize] {
            continue;
        }
        let c = mesh.closest_on(p, m[0], m[2]);
        let (dh, dv) = ((c[0] - m[0]).hypot(c[2] - m[2]), (c[1] - m[1]).abs());
        if dh <= 3.0 && dv <= 10.0 && best.is_none_or(|b| dh + 0.25 * dv < b.0) {
            best = Some((dh + 0.25 * dv, p));
        }
    }
    best.map(|b| b.1)
}
