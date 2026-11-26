//! Proof of concept: a walkable-surface navigation mesh built with rerecast from the terrain and the collision meshes, in
//! tiles, measured against what the bots' flat path maps (the level's 1 m maps or navgen's) get wrong.
//!   navmesh_n0 <game> <mod/level>... [--sweep data/out/stuck/r9w] [--weights data/out/stuck/weights.json] [--threads 4]
//!              [--cs 0.5] [--ch 0.2] [--tile 64] [--radius 0.5] [--climb 0.7] [--height 1.8] [--slope 0 (the level's)]
//!              [--around x,z,r] [--no-agree]
//! Per level: generation time and memory, polygons; the stuck events on foot of the sweep (same filter as `stuck_classes`), by
//! class and by where the mesh puts them (on the part reached from the spawns and flags, on an island, off it); and, where the
//! level ships its own map, how well the mesh at ground level agrees with it, next to navgen.
use bytefield_sim::bot_api::Nav;
use bytefield_sim::bots::body::paths::{CELL, Grid};
use bytefield_sim::{World, navgen};
use glam::{UVec3, Vec3, Vec3A};
use rerecast::{AreaType, BuildContoursFlags, HeightfieldBuilder, RegionId, TriMesh};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Clone, Copy)]
struct Cfg {
    cs: f32,
    ch: f32,
    tile: usize,
    radius: f32,
    climb: f32,
    height: f32,
    slope: f32,
    water_depth: f32,
    threads: usize,
}

impl Cfg {
    fn vx_climb(&self) -> u16 {
        (self.climb / self.ch).floor() as u16
    }
    fn vx_height(&self) -> u16 {
        (self.height / self.ch).ceil() as u16
    }
    fn vx_radius(&self) -> u16 {
        (self.radius / self.cs).ceil() as u16
    }
    fn border(&self) -> usize {
        self.vx_radius() as usize + 3
    }
}

/// An obstacle's collision mesh placed in the world, and whether each face is flat enough to stand on.
struct Solid {
    name: String,
    verts: Vec<Vec3A>,
    tris: Vec<UVec3>,
    walk: Vec<bool>,
    min: [f32; 2],
    max: [f32; 2],
}

/// A tile's interior cells: their spans (floor height in global voxels), whether each is on the mesh, and the links between
/// neighbouring spans of the tile.
struct Tile {
    cells: Vec<(u32, u8)>,
    y: Vec<u16>,
    ok: Vec<bool>,
    con: Vec<[u8; 4]>,
    why: Vec<u8>,
    polys: usize,
    bytes: usize,
    secs: [f64; 4],
}

const DX: [i32; 4] = [-1, 0, 1, 0];
const DZ: [i32; 4] = [0, 1, 0, -1];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let num = |k: &str, d: f32| opt(k).map_or(d, |v| v.parse().expect(k));
    let levels: Vec<&String> = a[2..].iter().take_while(|s| !s.starts_with("--")).collect();
    if levels.is_empty() {
        eprintln!("navmesh_n0 <game> <mod/level>... [--sweep DIR] [--weights FILE] [--threads 4] [--cs 0.5] [--ch 0.2] [--tile 64] ...");
        std::process::exit(2);
    }
    let base = Cfg {
        cs: num("--cs", 0.5),
        ch: num("--ch", 0.2),
        tile: num("--tile", 64.0) as usize,
        radius: num("--radius", 0.5),
        climb: num("--climb", 0.7),
        height: num("--height", 1.8),
        slope: num("--slope", 0.0),
        water_depth: 0.0,
        threads: num("--threads", 4.0) as usize,
    };
    let weights: BTreeMap<String, f64> = opt("--weights")
        .and_then(|p| serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(p).ok()?).ok())
        .and_then(|v| v["weights"].as_object().map(|o| o.iter().map(|(k, w)| (k.clone(), w.as_f64().unwrap_or(0.0))).collect()))
        .unwrap_or_default();
    let around = opt("--around").map(|s| s.split(',').map(|v| v.parse::<f32>().unwrap()).collect::<Vec<_>>());
    // (v1 class, mesh status) -> (seconds, weighted seconds per round)
    let mut all: BTreeMap<(String, &str), (f64, f64)> = BTreeMap::new();
    let mut wsum = 0.0;
    for ml in &levels {
        let (m, level) = ml.split_once('/').expect("mod/level");
        let t0 = Instant::now();
        let Ok(world) = World::load(Path::new(&a[1]), m, level, "conquest") else {
            eprintln!("{ml}: no world");
            continue;
        };
        let params = navgen::Params::of_world(&world, Nav::Infantry);
        let cfg = Cfg { slope: if base.slope > 0.0 { base.slope } else { params.max_slope as f32 }, water_depth: params.water_depth as f32, ..base };
        let solids = place_solids(&world, &cfg);
        let t_load = t0.elapsed().as_secs_f64();
        if let Some(pr) = opt("--tris") {
            // the collision faces over a point: height range, upright share, which obstacle
            let v: Vec<f32> = pr.split(',').map(|v| v.parse().unwrap()).collect();
            for (k, s) in solids.iter().enumerate() {
                for (tri, &w) in s.tris.iter().zip(&s.walk) {
                    let p = [tri.x, tri.y, tri.z].map(|i| s.verts[i as usize]);
                    let inside = {
                        let e = |a: Vec3A, b: Vec3A| (b.x - a.x) * (v[1] - a.z) - (b.z - a.z) * (v[0] - a.x);
                        let (d0, d1, d2) = (e(p[0], p[1]), e(p[1], p[2]), e(p[2], p[0]));
                        (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
                    };
                    if inside {
                        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
                        println!("   tri of solid {k} ({}): y {:.2}..{:.2} ny {:.2} walk {w}", s.name, p.iter().map(|q| q.y).fold(f32::MAX, f32::min), p.iter().map(|q| q.y).fold(f32::MIN, f32::max), n.y);
                    }
                }
            }
        }
        let t1 = Instant::now();
        let (tiles, side_tiles, ymin) = build(&world, &solids, &cfg);
        let t_gen = t1.elapsed().as_secs_f64();
        let rss_gen = vm_hwm_mb();
        let n_cells = cfg.tile * side_tiles;
        let mesh = Mesh::new(tiles, side_tiles, cfg, ymin);
        let reached = mesh.reach(&seeds(&world));
        let polys: usize = mesh.tiles.iter().map(|t| t.polys).sum();
        let bytes: usize = mesh.tiles.iter().map(|t| t.bytes).sum();
        let mut secs = [0.0; 4];
        for t in &mesh.tiles {
            for i in 0..4 {
                secs[i] += t.secs[i];
            }
        }
        let spans = mesh.y.len();
        // a fingerprint of what was built, to check it does not depend on the threads
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for (i, (&y, &o)) in mesh.y.iter().zip(&mesh.ok).enumerate() {
            fp = (fp ^ (y as u64 | (o as u64) << 16 | (reached[i] as u64) << 17)).wrapping_mul(0x100_0000_01b3);
        }
        for t in &mesh.tiles {
            fp = (fp ^ t.polys as u64).wrapping_mul(0x100_0000_01b3);
        }
        println!("   fingerprint {fp:016x}");
        let ok = mesh.ok.iter().filter(|&&o| o).count();
        let on = (0..spans).filter(|&i| reached[i]).count();
        println!(
            "\n== {ml}: {} m, {} tiles of {} m, {n_cells}² cells; load+solids {t_load:.1} s ({} solids); generation {t_gen:.1} s on {} threads (cpu: raster+filter {:.1}, compact+erode {:.1}, regions {:.1}, contours+poly {:.1}); peak RSS {rss_gen} MB",
            world.level.terrain.world_size,
            side_tiles * side_tiles,
            cfg.tile as f32 * cfg.cs,
            solids.len(),
            cfg.threads,
            secs[0],
            secs[1],
            secs[2],
            secs[3]
        );
        println!(
            "   {polys} polygons ({:.1} MB as a poly mesh); {spans} spans, {ok} walkable on the mesh, {on} reached from {} seeds ({:.0} %); slope {:.0}°, wading {:.1} m",
            bytes as f64 / 1e6,
            seeds(&world).len(),
            100.0 * on as f64 / ok.max(1) as f64,
            cfg.slope,
            cfg.water_depth
        );
        let map = world.nav_map(Nav::Infantry);
        let shipped = world.nav_map_shipped(Nav::Infantry);
        if let Some(pr) = opt("--probe") {
            let v: Vec<f32> = pr.split(',').map(|v| v.parse().unwrap()).collect();
            for dx in [-1, 0, 1] {
                let (gx, gz) = ((v[0] / cfg.cs) as i32 + dx, (v[1] / cfg.cs) as i32);
                let spans: Vec<String> = mesh.cell(gx, gz).map(|s| format!("{:.1}{}{}", mesh.world_y(s), if mesh.ok[s] { "" } else { "x" }, if reached[s] { "*" } else { "" })).collect();
                println!("   cell {gx},{gz}: {}", spans.join(" "));
            }
        }
        if let Some(r) = &around {
            mesh.show(&world, &reached, map.as_deref(), r[0], r[1], r[2]);
        }
        // the stuck events of this level in the sweep
        if let Some(dir) = opt("--sweep") {
            let key = format!("{m}_{level}");
            let (evs, rounds) = events(dir, &key);
            let w = weights.get(&key).copied();
            if let Some(w) = w {
                wsum += w;
            }
            let grid = map.as_ref().map(|mm| Grid::from_map(mm, shipped));
            let spawners: Vec<[f64; 3]> = world.level.spawners.iter().map(|s| s.pos).collect();
            let mut here: BTreeMap<(String, &str), f64> = BTreeMap::new();
            for (dur, [x, y, z]) in evs {
                let above = world.height_at(x as f64, z as f64).map_or(0.0, |h| y as f64 - h);
                let px_blocked = |px: f32, pz: f32| map.as_ref().is_none_or(|mm| px < 0.0 || pz < 0.0 || px as usize >= mm.width || pz as usize >= mm.height || mm.pixels[pz as usize * mm.width + px as usize] != 0);
                let mixed = map.as_ref().is_some_and(|_| {
                    let k = CELL as i32;
                    let (cx, cz) = ((x / CELL).floor() as i32, (z / CELL).floor() as i32);
                    (cz - 1..=cz + 1).any(|qz| {
                        (cx - 1..=cx + 1).any(|qx| {
                            let b = (0..k * k).filter(|i| px_blocked((qx * k + i % k) as f32, (qz * k + i / k) as f32)).count() as i32;
                            b > 0 && b < k * k
                        })
                    })
                });
                let shut = grid.as_ref().is_some_and(|g| !g.clear(x, z));
                let object = spawners.iter().any(|s| (s[0] as f32 - x).hypot(s[2] as f32 - z) < 8.0);
                let class = if above > 3.0 {
                    "deck"
                } else if mixed || shut {
                    if px_blocked(x, z) { "grid, own px blocked" } else { "grid, own px free" }
                } else if object {
                    "object"
                } else {
                    "other"
                };
                let status = mesh.status(&reached, x, y, z);
                if opt("--list").is_some() {
                    println!("     event {dur:4.0} s {class:22} {status:7} at {x:.0},{y:.0},{z:.0} (above {above:.1})");
                }
                *here.entry((class.to_string(), status)).or_default() += dur;
            }
            let total: f64 = here.values().sum();
            println!("   stuck on foot ({rounds} rounds, {total:.0} s, weight {:.3}):", w.unwrap_or(0.0));
            for ((c, s), d) in &here {
                println!("     {c:22} {s:7} {d:6.0} s");
                let e = all.entry((c.clone(), s)).or_default();
                e.0 += d;
                e.1 += w.unwrap_or(0.0) * d / rounds.max(1) as f64;
            }
        }
        // agreement at ground level with the level's own map, and navgen's for comparison
        if shipped && opt("--no-agree").is_none() {
            let mm = map.as_ref().unwrap();
            let gen_map = navgen::generate(&world, Nav::Infantry);
            let [cx, cz, cw, chh] = world.level.combat_area.unwrap_or([0.0, 0.0, mm.width as f64, mm.height as f64]);
            let (mut v2, mut ng) = ([0u64; 4], [0u64; 4]);
            let mut missing: BTreeMap<&str, u64> = BTreeMap::new();
            for pz in (cz.max(0.0) as usize)..((cz + chh) as usize).min(mm.height) {
                for px in (cx.max(0.0) as usize)..((cx + cw) as usize).min(mm.width) {
                    let off_free = mm.pixels[pz * mm.width + px] == 0;
                    let Some(h) = world.height_at(px as f64, pz as f64) else { continue };
                    let mesh_free = mesh.ground_free(&reached, px as f32, pz as f32, h as f32);
                    let gen_free = gen_map.pixels.get(pz * gen_map.width + px).is_some_and(|&p| p == 0);
                    v2[(off_free as usize) * 2 + mesh_free as usize] += 1;
                    if off_free && !mesh_free {
                        let (gx, gz) = ((px as f32 / cfg.cs).floor() as i32, (pz as f32 / cfg.cs).floor() as i32);
                        let near: Vec<usize> = mesh.cell(gx, gz).filter(|&s| (mesh.world_y(s) - h as f32).abs() <= 0.6).collect();
                        let why = if near.is_empty() {
                            if mesh.cell(gx, gz).next().is_none() { "no walkable span" } else { "walkable span at another height" }
                        } else if near.iter().any(|&s| mesh.ok[s]) {
                            "on the mesh, unreached"
                        } else {
                            let s = near[0];
                            let t = mesh.cfg.tile;
                            let ti = (gz as usize / t) * mesh.side + gx as usize / t;
                            if mesh.tiles[ti].why[s - mesh.first[ti]] == 1 { "eroded" } else { "regionless" }
                        };
                        *missing.entry(why).or_default() += 1;
                        // what the terrain says there
                        if let Some((_, nrm)) = world.level.height_and_normal(px as f64, pz as f64, bytefield_formats::image::Orientation::IDENTITY) {
                            if (nrm[1] as f32) < cfg.slope.to_radians().cos() {
                                *missing.entry("  (terrain steeper than the slope)").or_default() += 1;
                            }
                        }
                    }
                    ng[(off_free as usize) * 2 + gen_free as usize] += 1;
                }
            }
            // c[level free * 2 + here free]
            let pct = |c: [u64; 4]| {
                let t = c.iter().sum::<u64>().max(1) as f64;
                format!(
                    "agree {:.1} %; of the level's free, free here {:.1} %; of the free here, the level's blocked {:.1} % (free: level {:.0}, here {:.0} k px)",
                    100.0 * (c[0] + c[3]) as f64 / t,
                    100.0 * c[3] as f64 / (c[2] + c[3]).max(1) as f64,
                    100.0 * c[1] as f64 / (c[1] + c[3]).max(1) as f64,
                    (c[2] + c[3]) as f64 / 1e3,
                    (c[1] + c[3]) as f64 / 1e3
                )
            };
            println!("   vs the level's map, in the combat area:\n     mesh   {}\n     navgen {}", pct(v2), pct(ng));
            println!("     the level's free px the mesh misses: {missing:?}");
        }
    }
    if !all.is_empty() {
        let tot: f64 = all.values().map(|v| v.0).sum();
        let wtot: f64 = all.values().map(|v| v.1).sum();
        println!("\n== all levels: {tot:.0} s stuck on foot; weighted {:.1} s per round over {:.0} % of the weight", wtot / wsum.max(1e-9), 100.0 * wsum);
        println!("   class                  mesh        s     %    weighted/round   %");
        for ((c, s), (d, w)) in &all {
            println!("   {c:22} {s:7} {d:7.0} {:5.1}   {:6.2} {:5.1}", 100.0 * d / tot, w / wsum.max(1e-9), 100.0 * w / wtot.max(1e-9));
        }
    }
}

/// Peak resident memory of the process so far.
fn vm_hwm_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()))
        .map_or(0, |kb| kb / 1024)
}

/// Spawn points and flags inside the combat area, at their height.
fn seeds(world: &World) -> Vec<[f32; 3]> {
    let is_seed = |k: &str| k.eq_ignore_ascii_case("spawnpoint") || k.eq_ignore_ascii_case("controlpoint");
    let inside = |x: f64, z: f64| world.level.combat_area.is_none_or(|[cx, cz, w, h]| x >= cx && z >= cz && x < cx + w && z < cz + h);
    world.level.placed.iter().filter(|p| p.kind.as_deref().is_some_and(is_seed) && inside(p.pos[0], p.pos[2])).map(|p| p.pos.map(|v| v as f32)).collect()
}

/// The stuck events on foot of `key` (`<mod>_<level>`) in a sweep, as `stuck_classes` picks them, and the rounds.
fn events(dir: &str, key: &str) -> (Vec<(f64, [f32; 3])>, usize) {
    let mut out = Vec::new();
    let mut rounds = 0;
    let mut files: Vec<_> = std::fs::read_dir(dir).expect("sweep dir").filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    for f in files {
        let stem = f.file_stem().unwrap().to_string_lossy().to_string();
        if f.extension().is_none_or(|x| x != "json") || stem.rsplit_once("_s").map_or(true, |x| x.0 != key) {
            continue;
        }
        let Ok(r) = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&f).unwrap_or_default()) else { continue };
        rounds += 1;
        for e in r["events"].as_array().into_iter().flatten() {
            let dur = e["dur"].as_f64().unwrap_or(0.0);
            let d = e["detail"].as_str().unwrap_or("");
            let fighting: f64 = d.rsplit_once('(').and_then(|x| x.1.split_once(" s")).and_then(|x| x.0.parse().ok()).unwrap_or(0.0);
            let what = d.split_once(" in ").map_or("?", |x| x.1.split(" (").next().unwrap_or("?"));
            if e["kind"] != "stuck" || dur < 15.0 || fighting > 0.5 * dur || what != "foot" {
                continue;
            }
            let p = &e["pos"];
            out.push((dur, [0, 1, 2].map(|i| p[i].as_f64().unwrap_or(0.0) as f32)));
        }
    }
    (out, rounds)
}

/// Every obstacle's finer collision LOD placed in the world.
/// TODO: parked vehicles and guns (and ships that are vehicles, such as Midway's carriers) are not among them, so the mesh
/// does not see them: a quarter of the weighted time stuck on foot is next to one. Once bots walk on the mesh, mark an area
/// with a cost around each object spawner, or rebuild the tile with the placed vehicle's collision.
fn place_solids(world: &World, cfg: &Cfg) -> Vec<Solid> {
    let min_up = cfg.slope.to_radians().cos();
    let mut out = Vec::new();
    for o in world.obstacles() {
        let Some(mesh) = bytefield_sim::obstacle::pick_lod(&o.lods, 1) else { continue };
        let place = |v: [f32; 3]| {
            let p = [v[0] * o.scale[0], v[1] * o.scale[1], v[2] * o.scale[2]];
            Vec3A::from_array([0, 1, 2].map(|c| o.rows[0][c] * p[0] + o.rows[1][c] * p[1] + o.rows[2][c] * p[2] + o.pos[c]))
        };
        let verts: Vec<Vec3A> = mesh.vertices.iter().map(|&v| place(v)).collect();
        let mut tris = Vec::new();
        let mut walk = Vec::new();
        for f in &mesh.faces {
            if f.v.iter().any(|&i| i as usize >= verts.len()) {
                continue;
            }
            let [a, b, c] = f.v.map(|i| verts[i as usize]);
            let n = (b - a).cross(c - a);
            let l = n.length();
            if l < 1e-6 {
                continue;
            }
            // either winding: a face is something to stand on if it is flat enough, whichever way it was wound
            tris.push(UVec3::new(f.v[0] as u32, f.v[1] as u32, f.v[2] as u32));
            walk.push((n.y / l).abs() >= min_up);
        }
        if tris.is_empty() {
            continue;
        }
        let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
        for v in &verts {
            min = [min[0].min(v.x), min[1].min(v.z)];
            max = [max[0].max(v.x), max[1].max(v.z)];
        }
        out.push(Solid { name: o.name.clone(), verts, tris, walk, min, max });
    }
    out
}

/// All the tiles, built on `cfg.threads` threads; the result does not depend on the order they finish in.
fn build(world: &World, solids: &[Solid], cfg: &Cfg) -> (Vec<Tile>, usize, f32) {
    let ws = world.level.terrain.world_size as f32;
    let tile_m = cfg.tile as f32 * cfg.cs;
    let side = (ws / tile_m).ceil() as usize;
    let mut ymin = f32::MAX;
    let mut ymax = f32::MIN;
    let hm = world.level.heightmap.size;
    let hc = ws / hm as f32;
    for j in 0..hm {
        for i in 0..hm {
            if let Some(h) = world.height_at((i as f32 * hc) as f64, (j as f32 * hc) as f64) {
                ymin = ymin.min(h as f32);
                ymax = ymax.max(h as f32);
            }
        }
    }
    for s in solids {
        for v in &s.verts {
            ymax = ymax.max(v.y);
        }
    }
    ymin -= 1.0;
    ymax += cfg.height + 1.0;
    // the solids by tile, with the border
    let b = cfg.border() as f32 * cfg.cs;
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); side * side];
    for (k, s) in solids.iter().enumerate() {
        let t0 = |v: f32| (((v - b) / tile_m).floor().max(0.0) as usize).min(side - 1);
        let t1 = |v: f32| (((v + b) / tile_m).floor().max(0.0) as usize).min(side - 1);
        for tz in t0(s.min[1])..=t1(s.max[1]) {
            for tx in t0(s.min[0])..=t1(s.max[0]) {
                bins[tz * side + tx].push(k as u32);
            }
        }
    }
    let next = AtomicUsize::new(0);
    let mut out: Vec<Option<Tile>> = (0..side * side).map(|_| None).collect();
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|sc| {
        for _ in 0..cfg.threads.max(1) {
            sc.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= side * side {
                        break;
                    }
                    let t = build_tile(world, solids, &bins[i], cfg, i % side, i / side, ymin, ymax);
                    results.lock().unwrap().push((i, t));
                }
            });
        }
    });
    for (i, t) in results.into_inner().unwrap() {
        out[i] = Some(t);
    }
    (out.into_iter().map(|t| t.unwrap()).collect(), side, ymin)
}

#[allow(clippy::too_many_arguments)]
fn build_tile(world: &World, solids: &[Solid], bin: &[u32], cfg: &Cfg, tx: usize, tz: usize, ymin: f32, ymax: f32) -> Tile {
    let t = cfg.tile;
    let bd = cfg.border();
    let x0 = (tx * t) as f32 * cfg.cs - bd as f32 * cfg.cs;
    let z0 = (tz * t) as f32 * cfg.cs - bd as f32 * cfg.cs;
    let span = (t + 2 * bd) as f32 * cfg.cs;
    let mut tm = TriMesh::default();
    // the terrain, at the heightmap's own resolution
    let ws = world.level.terrain.world_size as f32;
    let hm = world.level.heightmap.size;
    let hc = ws / hm as f32;
    let water = world.level.terrain.water_level as f32;
    let min_up = cfg.slope.to_radians().cos();
    let i0 = ((x0 / hc).floor().max(0.0)) as usize;
    let j0 = ((z0 / hc).floor().max(0.0)) as usize;
    let i1 = (((x0 + span) / hc).ceil() as usize).min(hm - 1);
    let j1 = (((z0 + span) / hc).ceil() as usize).min(hm - 1);
    let at = |i: usize, j: usize| {
        let (x, z) = (i as f32 * hc, j as f32 * hc);
        Vec3A::new(x, world.height_at(x as f64, z as f64).unwrap_or(ymin as f64) as f32, z)
    };
    let cols = i1 + 1 - i0;
    for j in j0..=j1 {
        for i in i0..=i1 {
            tm.vertices.push(at(i, j));
        }
    }
    for j in 0..(j1 - j0) {
        for i in 0..(i1 - i0) {
            let q = [j * cols + i, j * cols + i + 1, (j + 1) * cols + i + 1, (j + 1) * cols + i].map(|v| v as u32);
            for tri in [[q[0], q[2], q[1]], [q[0], q[3], q[2]]] {
                let [a, b, c] = tri.map(|v| tm.vertices[v as usize]);
                let n = (b - a).cross(c - a).normalize_or_zero();
                let g = (a + b + c) / 3.0;
                let deep = a.y.min(b.y).min(c.y) < water - cfg.water_depth;
                let ok = n.y.abs() >= min_up && !deep && world.level.material_at(g.x as f64, g.z as f64) != navgen::NO_GO_MATERIAL;
                tm.indices.push(UVec3::from_array(tri));
                tm.area_types.push(if ok { AreaType::DEFAULT_WALKABLE } else { AreaType::NOT_WALKABLE });
            }
        }
    }
    for &k in bin {
        let s = &solids[k as usize];
        let off = tm.vertices.len() as u32;
        tm.vertices.extend_from_slice(&s.verts);
        for (tri, &w) in s.tris.iter().zip(&s.walk) {
            tm.indices.push(*tri + off);
            tm.area_types.push(if w { AreaType::DEFAULT_WALKABLE } else { AreaType::NOT_WALKABLE });
        }
    }
    let aabb = rerecast::Aabb3d { min: Vec3::new(x0, ymin, z0), max: Vec3::new(x0 + span, ymax, z0 + span) };
    let (climb, height, radius) = (cfg.vx_climb(), cfg.vx_height(), cfg.vx_radius());
    let c0 = Instant::now();
    let mut hf = HeightfieldBuilder { aabb, cell_size: cfg.cs, cell_height: cfg.ch }.build().unwrap();
    hf.rasterize_triangles(&tm, climb).unwrap();
    let col = std::env::var("N0_COLUMN").ok().map(|v| v.split(',').map(|c| c.parse::<f32>().unwrap()).collect::<Vec<_>>());
    let dump = |hf: &rerecast::Heightfield, what: &str| {
        if let Some(c) = &col {
            let (lx, lz) = (((c[0] - x0) / cfg.cs) as i64, ((c[1] - z0) / cfg.cs) as i64);
            if lx >= bd as i64 && lz >= bd as i64 && lx < (t + bd) as i64 && lz < (t + bd) as i64 {
                let mut k = hf.span_key_at(lx as u16, lz as u16);
                let mut out = Vec::new();
                while let Some(key) = k {
                    let sp = hf.span(key);
                    out.push(format!("[{:.1}..{:.1} a{}]", ymin + sp.min as f32 * cfg.ch, ymin + sp.max as f32 * cfg.ch, sp.area.0));
                    k = sp.next;
                }
                println!("   column {what}: {}", out.join(" "));
            }
        }
    };
    dump(&hf, "rasterized");
    hf.filter_low_hanging_walkable_obstacles(climb);
    dump(&hf, "low hanging");
    hf.filter_ledge_spans(height, climb);
    dump(&hf, "ledges");
    hf.filter_walkable_low_height_spans(height);
    dump(&hf, "low height");
    let c1 = Instant::now();
    let mut chf = hf.into_compact(height, climb).unwrap();
    chf.erode_walkable_area(radius);
    let c2 = Instant::now();
    chf.build_distance_field();
    chf.build_regions(bd as u16, 8, 20).unwrap();
    let c3 = Instant::now();
    let contours = chf.build_contours(1.3, (12.0 / cfg.cs) as u16, BuildContoursFlags::DEFAULT);
    let pm = contours.into_polygon_mesh(6).unwrap();
    let c4 = Instant::now();
    let polys = pm.polygon_count();
    if std::env::var("N0_LANDMARK").is_ok() {
        landmark_tile(&tm, aabb, cfg, bd, polys, (c4 - c0).as_secs_f64());
    }
    let bytes = pm.vertices.len() * 6 + pm.polygons.len() * 4 + polys * 6;
    // the interior cells
    let w = chf.width as usize;
    let mut cells = Vec::with_capacity(t * t);
    // walkable after the filters but not on the mesh: 1 eroded (or marked), 2 without a region
    let (mut y, mut ok, mut con, mut why) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for lz in 0..t {
        for lx in 0..t {
            let c = chf.cell_at((lx + bd) as u16, (lz + bd) as u16);
            cells.push((y.len() as u32, c.count()));
            for si in c.index_range() {
                let s = &chf.spans[si];
                y.push(s.y);
                let region = s.region;
                ok.push(chf.areas[si].is_walkable() && region != RegionId::NONE && !region.contains(RegionId::BORDER_REGION));
                why.push(if !chf.areas[si].is_walkable() { 1 } else { 2 });
                con.push([0, 1, 2, 3].map(|d| s.con(d).unwrap_or(0xff)));
            }
        }
    }
    let _ = w;
    Tile { cells, y, ok, con, why, polys, bytes, secs: [(c1 - c0).as_secs_f64(), (c2 - c1).as_secs_f64(), (c3 - c2).as_secs_f64(), (c4 - c3).as_secs_f64()] }
}

/// The tiles stitched into one layered grid of spans, for the measures.
struct Mesh {
    tiles: Vec<Tile>,
    side: usize,
    cfg: Cfg,
    ymin: f32,
    /// first global span id of each tile
    first: Vec<usize>,
    y: Vec<u16>,
    ok: Vec<bool>,
}

impl Mesh {
    fn new(tiles: Vec<Tile>, side: usize, cfg: Cfg, ymin: f32) -> Mesh {
        let mut first = Vec::with_capacity(tiles.len());
        let (mut y, mut ok) = (Vec::new(), Vec::new());
        for t in &tiles {
            first.push(y.len());
            y.extend_from_slice(&t.y);
            ok.extend_from_slice(&t.ok);
        }
        Mesh { tiles, side, cfg, ymin, first, y, ok }
    }

    fn n(&self) -> i32 {
        (self.cfg.tile * self.side) as i32
    }

    /// The global span ids of the cell (gx, gz).
    fn cell(&self, gx: i32, gz: i32) -> std::ops::Range<usize> {
        let n = self.n();
        if gx < 0 || gz < 0 || gx >= n || gz >= n {
            return 0..0;
        }
        let t = self.cfg.tile;
        let (gx, gz) = (gx as usize, gz as usize);
        let ti = (gz / t) * self.side + gx / t;
        let (s, c) = self.tiles[ti].cells[(gz % t) * t + gx % t];
        let f = self.first[ti] + s as usize;
        f..f + c as usize
    }

    fn world_y(&self, s: usize) -> f32 {
        self.ymin + self.y[s] as f32 * self.cfg.ch
    }

    /// Union-find over the walkable spans; true for those joined to a span near a seed.
    fn reach(&self, seeds: &[[f32; 3]]) -> Vec<bool> {
        let total = self.y.len();
        let mut parent: Vec<u32> = (0..total as u32).collect();
        fn find(p: &mut [u32], mut i: u32) -> u32 {
            while p[i as usize] != i {
                p[i as usize] = p[p[i as usize] as usize];
                i = p[i as usize];
            }
            i
        }
        let n = self.n();
        let t = self.cfg.tile as i32;
        let climb = self.cfg.vx_climb() as i32;
        for gz in 0..n {
            for gx in 0..n {
                let ti = ((gz / t) as usize) * self.side + (gx / t) as usize;
                for s in self.cell(gx, gz) {
                    if !self.ok[s] {
                        continue;
                    }
                    // the two forward neighbours (+x, +z): inside the tile by the tile's own links, across by height
                    for d in [1usize, 2] {
                        let (nx, nz) = (gx + DX[d], gz + DZ[d]);
                        let same = nx / t == gx / t && nz / t == gz / t && nx < n && nz < n;
                        let tile = &self.tiles[ti];
                        let local = s - self.first[ti];
                        let target = if same {
                            let c = tile.con[local][d];
                            (c != 0xff).then(|| self.cell(nx, nz).start + c as usize)
                        } else {
                            self.cell(nx, nz).filter(|&o| self.ok[o] && (self.y[o] as i32 - self.y[s] as i32).abs() <= climb).min_by_key(|&o| (self.y[o] as i32 - self.y[s] as i32).abs())
                        };
                        if let Some(o) = target.filter(|&o| self.ok[o]) {
                            let (a, b) = (find(&mut parent, s as u32), find(&mut parent, o as u32));
                            if a != b {
                                parent[a.max(b) as usize] = a.min(b);
                            }
                        }
                    }
                }
            }
        }
        let mut roots = std::collections::BTreeSet::new();
        for p in seeds {
            if let Some(s) = self.nearest(p[0], p[1], p[2], 8.0, 3.0) {
                roots.insert(find(&mut parent, s as u32));
            }
        }
        (0..total).map(|s| self.ok[s] && roots.contains(&find(&mut parent, s as u32))).collect()
    }

    /// The walkable span nearest to (x, y, z) within `r` m across and `dy` m up or down.
    fn nearest(&self, x: f32, y: f32, z: f32, r: f32, dy: f32) -> Option<usize> {
        let cs = self.cfg.cs;
        let (cx, cz) = ((x / cs).floor() as i32, (z / cs).floor() as i32);
        let k = (r / cs).ceil() as i32;
        let mut best: Option<(f32, usize)> = None;
        for qz in cz - k..=cz + k {
            for qx in cx - k..=cx + k {
                for s in self.cell(qx, qz) {
                    let sy = self.world_y(s);
                    if !self.ok[s] || (sy - y).abs() > dy {
                        continue;
                    }
                    let d = ((qx - cx) as f32 * cs).hypot((qz - cz) as f32 * cs) + 0.25 * (sy - y).abs();
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, s));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Where the mesh puts a bot at (x, y, z) (the sweep rounds the position to the metre): on a reached span within 1 m,
    /// near one (within 3 m), on an unreached island, or off.
    fn status(&self, reached: &[bool], x: f32, y: f32, z: f32) -> &'static str {
        // the event's height is rounded; a bot stands on the floor
        match self.nearest(x + 0.5, y, z + 0.5, 1.0, 1.5) {
            Some(s) if reached[s] => "on",
            Some(_) => "island",
            None => match self.nearest(x + 0.5, y, z + 0.5, 3.0, 2.0) {
                Some(s) if reached[s] => "near",
                Some(_) => "island",
                None => "off",
            },
        }
    }

    /// Whether the 1 m pixel at (x, z) has a reached span at ground height `h`.
    fn ground_free(&self, reached: &[bool], x: f32, z: f32, h: f32) -> bool {
        let cs = self.cfg.cs;
        let (gx, gz) = ((x / cs).floor() as i32, (z / cs).floor() as i32);
        self.cell(gx, gz).any(|s| reached[s] && (self.world_y(s) - h).abs() <= 0.6)
    }

    /// The layers around (x, z), a cell per character: '.' none walkable, '1'.. reached spans (how many), 'i' island only, and
    /// '#' where the 1 m map is blocked and the mesh has nothing reached.
    fn show(&self, world: &World, reached: &[bool], map: Option<&bytefield_formats::pathmap::PathMap>, x: f32, z: f32, r: f32) {
        let cs = self.cfg.cs;
        let k = (r / cs) as i32;
        let (cx, cz) = ((x / cs) as i32, (z / cs) as i32);
        println!("   around {x} {z} (r {r} m, north up, a cell is {cs} m; heights of the reached layers listed below)");
        let mut heights = std::collections::BTreeSet::new();
        for qz in (cz - k..=cz + k).rev() {
            let mut line = String::from("   ");
            for qx in cx - k..=cx + k {
                let spans: Vec<usize> = self.cell(qx, qz).filter(|&s| self.ok[s]).collect();
                let on = spans.iter().filter(|&&s| reached[s]).count();
                for &s in &spans {
                    if reached[s] {
                        let g = world.height_at((qx as f32 * cs) as f64, (qz as f32 * cs) as f64).unwrap_or(0.0) as f32;
                        heights.insert(((self.world_y(s) - g) * 2.0).round() as i32);
                    }
                }
                let blocked = map.is_some_and(|m| {
                    let (px, pz) = ((qx as f32 * cs) as usize, (qz as f32 * cs) as usize);
                    px < m.width && pz < m.height && m.pixels[pz * m.width + px] != 0
                });
                line.push(match (on, spans.len(), blocked) {
                    (0, 0, true) => '#',
                    (0, 0, false) => '.',
                    (0, _, _) => 'i',
                    (c, _, _) => char::from_digit(c.min(9) as u32, 10).unwrap(),
                });
            }
            println!("{line}");
        }
        println!("   reached layers above the terrain (m): {:?}", heights.iter().map(|h| *h as f32 / 2.0).collect::<Vec<_>>());
    }
}

/// The same tile through landmark (the other pure-Rust Recast port), to compare time and polygons with rerecast.
fn landmark_tile(tm: &TriMesh, aabb: rerecast::Aabb3d, cfg: &Cfg, bd: usize, rr_polys: usize, rr_secs: f64) {
    use landmark::*;
    static SUM: std::sync::Mutex<(f64, f64, usize, usize, usize)> = std::sync::Mutex::new((0.0, 0.0, 0, 0, 0));
    let t0 = Instant::now();
    let verts: Vec<f32> = tm.vertices.iter().flat_map(|v| [v.x, v.y, v.z]).collect();
    let tris: Vec<i32> = tm.indices.iter().flat_map(|t| [t.x as i32, t.y as i32, t.z as i32]).collect();
    let areas: Vec<u8> = tm.area_types.iter().map(|a| a.0).collect();
    let (climb, height, radius) = (cfg.vx_climb() as i32, cfg.vx_height() as i32, cfg.vx_radius() as i32);
    let side = ((aabb.max.x - aabb.min.x) / cfg.cs + 0.5) as i32;
    let mn = glam030::Vec3::new(aabb.min.x, aabb.min.y, aabb.min.z);
    let mx = glam030::Vec3::new(aabb.max.x, aabb.max.y, aabb.max.z);
    let mut hf = Heightfield::new(side, side, mn, mx, cfg.cs, cfg.ch);
    rasterize_triangles(&verts, &tris, &areas, areas.len(), &mut hf, climb).unwrap();
    hf.filter_low_hanging_walkable_obstacles(climb as i16).unwrap();
    hf.filter_ledge_spans(height as i16, climb as i16).unwrap();
    hf.filter_walkable_low_height_spans(height as i16).unwrap();
    let mut chf = CompactHeightfield::build_from_heightfield(&hf, height, climb).unwrap();
    erode_walkable_area(&mut chf, radius).unwrap();
    chf.build_distance_field().unwrap();
    chf.build_regions(bd as i32, 8, 20).unwrap();
    let cset = ContourSet::build_from_compact_heightfield(&chf, 1.3, (12.0 / cfg.cs) as i32, 8, 20).unwrap();
    let pm = PolyMesh::build_from_contour_set(&cset, 6).unwrap();
    let secs = t0.elapsed().as_secs_f64();
    let mut s = SUM.lock().unwrap();
    s.0 += rr_secs;
    s.1 += secs;
    s.2 += rr_polys;
    s.3 += pm.poly_count();
    s.4 += 1;
    if s.4 % 1024 == 0 {
        eprintln!("   landmark vs rerecast after {} tiles: cpu {:.1} s vs {:.1} s, polygons {} vs {}", s.4, s.1, s.0, s.3, s.2);
    }
}
