//! `topdown <game_dir> <mod> <level> <out_dir> [--mode conquest]`: a level seen from above,
//! with no GPU, and three automatic checks of how the data is laid out:
//!
//! 1. objects vs terrain: a placed object's y should match the terrain height under it, for
//!    most objects (buildings, trees, spawn points sit on the ground). Tried for each of the 8
//!    ways the heightmap could map onto x/z: the right one, and the height formula, give
//!    residuals near zero.
//! 2. the level's own `ingamemap.dds` (a picture of the combat area by the level's authors) vs
//!    the `TxXXxYY.dds` colour tiles cropped to the combat area: how the picture is laid out.
//!    Tile XX, YY is taken to cover x = XX*256 m, z = YY*256 m: Berlin ships only the 4 tiles
//!    under its combat area (06/07 x 06/07, area 1536..2048), which fixes that.
//! 3. the same picture vs where the heightmap says water: closes the triangle heightmap ->
//!    ingamemap -> tiles. (The tiles show the sea floor, not water: the engine draws water.)
//!
//! Writes ingamemap.png, tiles.png (combat area, laid out like the picture), relief.png (world
//! x right, z down, objects as red dots), relief_world.png and tiles_world.png (the whole
//! terrain, tile XX right, YY down) and report.json.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bytefield_con::session;
use bytefield_formats::dds;
use bytefield_formats::image::{Image, Orientation, correlation};
use bytefield_world::Level;
use serde::Serialize;

#[derive(Serialize)]
struct OrientationScore {
    orientation: String,
    score: f64,
}

#[derive(Serialize)]
struct Report {
    level: String,
    world_size: f64,
    heightmap_size: usize,
    combat_area: Option<[f64; 4]>,
    placed: usize,
    /// per heightmap orientation: share of objects within 1 m of the terrain
    objects_on_terrain: Vec<OrientationScore>,
    /// best share minus the next best (small = ambiguous, e.g. a flat city)
    heightmap_margin: f64,
    /// median |y - terrain| for the best orientation, metres
    objects_median_residual_m: f64,
    tile_grid: Option<usize>,
    tex_offset: [f64; 2],
    /// per orientation of the tiles' combat-area crop: correlation with ingamemap.dds
    tiles_vs_ingamemap: Vec<OrientationScore>,
    /// per orientation of the combat area: "heightmap says water" vs ingamemap blueness
    /// (empty for levels without water)
    water_vs_ingamemap: Vec<OrientationScore>,
}

fn name(o: Orientation) -> String {
    let mut s = String::new();
    if o.transpose {
        s.push_str("transpose ");
    }
    if o.flip_x {
        s.push_str("flip-x ");
    }
    if o.flip_y {
        s.push_str("flip-y ");
    }
    if s.is_empty() { "identity".into() } else { s.trim_end().into() }
}

fn best(scores: &[(Orientation, f64)]) -> (Orientation, f64) {
    scores.iter().copied().fold((Orientation::IDENTITY, f64::MIN), |a, b| if b.1 > a.1 { b } else { a })
}

fn margin(scores: &[(Orientation, f64)]) -> f64 {
    let (b, bs) = best(scores);
    bs - scores.iter().filter(|x| x.0 != b).map(|x| x.1).fold(f64::MIN, f64::max)
}

/// The part of a world-sized square image covering world rect (x, z, w, h).
fn crop_world(img: &Image, world: f64, area: [f64; 4]) -> Image {
    let s = img.width as f64 / world;
    let (x, y) = ((area[0] * s).max(0.0) as usize, (area[1] * s).max(0.0) as usize);
    let (w, h) = ((area[2] * s).max(1.0) as usize, (area[3] * s).max(1.0) as usize);
    img.crop(x, y, w, h)
}

fn run(game: &Path, mod_name: &str, level: &str, out: &Path, mode: &str) -> Result<(), String> {
    let (_, vfs) = session::mount(game, mod_name)?;
    let it = session::load(&vfs, Some(level), mode);
    let lvl = Level::from_db(&vfs, &it.host.db, level)?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let world = lvl.terrain.world_size;
    let area = lvl.combat_area.unwrap_or([0.0, 0.0, world, world]);

    // 1. objects vs terrain
    let mut on_terrain = Vec::new();
    let mut residuals = Vec::new();
    for o in Orientation::ALL {
        let mut res: Vec<f64> = lvl.placed.iter().filter_map(|p| lvl.height_at(p.pos[0], p.pos[2], o).map(|h| (p.pos[1] - h).abs())).collect();
        res.sort_by(f64::total_cmp);
        on_terrain.push((o, res.iter().filter(|&&r| r < 1.0).count() as f64 / res.len().max(1) as f64));
        residuals.push(res.get(res.len() / 2).copied().unwrap_or(f64::NAN));
    }
    // a flat level cannot tell orientations apart: then keep the convention the others show
    let hm_o = if margin(&on_terrain) < 0.05 { Orientation::IDENTITY } else { best(&on_terrain).0 };
    let hm_i = Orientation::ALL.iter().position(|&o| o == hm_o).unwrap();

    // relief in world layout (x right, z down), objects on top
    let px = 512;
    let cell = world / px as f64;
    let mut relief = Image::new(px, px);
    for r in 0..px {
        for c in 0..px {
            let (x, z) = ((c as f64 + 0.5) * cell, (r as f64 + 0.5) * cell);
            let h = lvl.height_at(x, z, hm_o).unwrap_or(0.0);
            let hx = lvl.height_at(x + cell, z, hm_o).unwrap_or(h) - h;
            let hz = lvl.height_at(x, z + cell, hm_o).unwrap_or(h) - h;
            let shade = (0.5 + (-hx - hz) / cell * 1.5).clamp(0.0, 1.0);
            let g = (60.0 + 180.0 * shade) as u8;
            relief.put(c, r, if h < lvl.terrain.water_level { [30, 60, (120.0 + 80.0 * shade) as u8, 255] } else { [g, g, g, 255] });
        }
    }
    for p in &lvl.placed {
        let (c, r) = ((p.pos[0] / cell) as isize, (p.pos[2] / cell) as isize);
        for (dc, dr) in [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1)] {
            let (c, r) = (c + dc, r + dr);
            if (0..px as isize).contains(&c) && (0..px as isize).contains(&r) {
                relief.put(c as usize, r as usize, [230, 30, 30, 255]);
            }
        }
    }
    crop_world(&relief, world, area).resize(512, 512).write_png(&out.join("relief.png")).map_err(|e| e.to_string())?;
    relief.write_png(&out.join("relief_world.png")).map_err(|e| e.to_string())?;

    // 2 and 3: against the ingamemap
    let (mut tiles_vs_map, mut water_vs_map, mut grid) = (Vec::new(), Vec::new(), None);
    let cmp = 128;
    if let Some(m) = vfs.read(&format!("bf1942/levels/{level}/textures/ingamemap.dds")).ok().and_then(|b| dds::decode(&b).ok()) {
        m.write_png(&out.join("ingamemap.png")).map_err(|e| e.to_string())?;
        let small = m.resize(cmp, cmp);
        let ml = small.luma();
        if let Some((mosaic, g)) = lvl.colour_mosaic(&vfs, 64) {
            grid = Some(g);
            mosaic.resize(512, 512).write_png(&out.join("tiles_world.png")).map_err(|e| e.to_string())?;
            let crop = crop_world(&mosaic, world, area).resize(cmp, cmp);
            for o in Orientation::ALL {
                tiles_vs_map.push((o, correlation(&ml, &crop.oriented(o).luma())));
            }
            crop.oriented(best(&tiles_vs_map).0).resize(512, 512).write_png(&out.join("tiles.png")).map_err(|e| e.to_string())?;
        }
        let mut mask = Image::new(cmp, cmp);
        let (cx, cz) = (area[2] / cmp as f64, area[3] / cmp as f64);
        let mut wet = 0;
        for r in 0..cmp {
            for c in 0..cmp {
                let (x, z) = (area[0] + (c as f64 + 0.5) * cx, area[1] + (r as f64 + 0.5) * cz);
                if lvl.height_at(x, z, hm_o).is_some_and(|h| h < lvl.terrain.water_level) {
                    mask.put(c, r, [255, 255, 255, 255]);
                    wet += 1;
                }
            }
        }
        if wet > 0 && wet < cmp * cmp {
            let blue: Vec<f64> = small.rgba.chunks_exact(4).map(|p| p[2] as f64 - (p[0] as f64 + p[1] as f64) / 2.0).collect();
            for o in Orientation::ALL {
                water_vs_map.push((o, correlation(&blue, &mask.oriented(o).luma())));
            }
        }
    }

    let round = |x: f64| (x * 1000.0).round() / 1000.0;
    let scores = |v: &[(Orientation, f64)]| v.iter().map(|&(o, s)| OrientationScore { orientation: name(o), score: round(s) }).collect();
    let report = Report {
        level: level.into(),
        world_size: world,
        heightmap_size: lvl.heightmap.size,
        combat_area: lvl.combat_area,
        placed: lvl.placed.len(),
        objects_on_terrain: scores(&on_terrain),
        heightmap_margin: round(margin(&on_terrain)),
        objects_median_residual_m: round(residuals[hm_i]),
        tile_grid: grid,
        tex_offset: lvl.terrain.tex_offset,
        tiles_vs_ingamemap: scores(&tiles_vs_map),
        water_vs_ingamemap: scores(&water_vs_map),
    };
    println!(
        "{level}: heightmap {} ({:.0}% of {} objects within 1 m, median {:.2} m, margin {:.2})",
        name(hm_o),
        on_terrain[hm_i].1 * 100.0,
        lvl.placed.len(),
        residuals[hm_i],
        margin(&on_terrain)
    );
    for (what, v) in [("tiles", &tiles_vs_map), ("water", &water_vs_map)] {
        if !v.is_empty() {
            let (o, s) = best(v);
            println!("{level}: ingamemap vs {what}: {} r={s:.3} (margin {:.3})", name(o), margin(v));
        }
    }
    std::fs::write(out.join("report.json"), serde_json::to_string_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).map(String::as_str);
    if args.len() < 4 {
        eprintln!("usage: topdown <game_dir> <mod> <level> <out_dir> [--mode conquest]");
        return ExitCode::FAILURE;
    }
    match run(Path::new(&args[0]), &args[1], &args[2], &PathBuf::from(&args[3]), opt("--mode").unwrap_or("conquest")) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("topdown: {e}");
            ExitCode::FAILURE
        }
    }
}
