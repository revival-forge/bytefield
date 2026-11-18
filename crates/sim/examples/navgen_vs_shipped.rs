//! Compares pixel by pixel the generated maps with those a level ships:
//! `navgen_vs_shipped [--game DIR] [--mod NAME] [--ppm DIR] <level>...`
//!
//! Gives two variants: with the level's `AIpathFinding.con` and without it (like a level that ships nothing).
//! Besides the overall agreement and that of the blocked areas it gives the free ones, because several official maps
//! are almost wholly blocked and there a single-valued map also gets the overall agreement.
//! "Zone" is the box of what is free in the official map, with 64 m more on each side.
use bytefield_formats::pathmap::PathMap;
use bytefield_sim::World;
use bytefield_sim::bot_api::Nav;
use bytefield_sim::navgen::{self, Confusion, Params};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The box (x0, z0, x1, z1) of what is free in the map, widened by `margin` pixels.
fn free_box(m: &PathMap, margin: usize) -> Option<[usize; 4]> {
    let mut b: Option<[usize; 4]> = None;
    for z in 0..m.height {
        for x in 0..m.width {
            if m.pixels[z * m.width + x] == 0 {
                b = Some(b.map_or([x, z, x, z], |c| [c[0].min(x), c[1].min(z), c[2].max(x), c[3].max(z)]));
            }
        }
    }
    b.map(|c| [c[0].saturating_sub(margin), c[1].saturating_sub(margin), (c[2] + margin).min(m.width - 1), (c[3] + margin).min(m.height - 1)])
}

fn pct(v: f64) -> String {
    format!("{:.1}", v * 100.0)
}

/// Image of the differences, with z growing upwards: black both blocked, grey both free,
/// red blocked only in the generated one, blue blocked only in the official one.
fn write_ppm(path: &Path, g: &PathMap, r: &PathMap, step: usize) -> std::io::Result<()> {
    let n = g.width / step;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(f, "P6\n{n} {n}\n255\n")?;
    for z in (0..n).rev() {
        for x in 0..n {
            let (a, b) = (g.get((x * step) as i64, (z * step) as i64) != 0, r.get((x * step) as i64, (z * step) as i64) != 0);
            f.write_all(match (a, b) {
                (true, true) => &[0, 0, 0],
                (false, false) => &[170, 170, 170],
                (true, false) => &[230, 40, 40],
                (false, true) => &[40, 80, 240],
            })?;
        }
    }
    Ok(())
}

fn main() {
    let mut game = PathBuf::from("data/game");
    let mut mod_name = "bf1942".to_string();
    let mut ppm: Option<PathBuf> = None;
    let mut levels = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next().expect("--game DIR").into(),
            "--mod" => mod_name = args.next().expect("--mod NAME"),
            "--ppm" => ppm = Some(args.next().expect("--ppm DIR").into()),
            _ => levels.push(a),
        }
    }
    println!("| Level | Kind | Rules | Overall % | Trivial % | Blocked hit % | Free hit % | Free IoU % | Zone: overall % | Zone: trivial % | Zone: blocked IoU % | s |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for level in &levels {
        let world = match World::load(&game, &mod_name, level, "conquest") {
            Ok(w) => w,
            Err(e) => {
                println!("| {level} | error: {e} | | | | | | | | | | |");
                continue;
            }
        };
        for (kind, file, label) in [(Nav::Infantry, "Infantry1Level0Map.raw", "on foot"), (Nav::Tank, "Tank0Level0Map.raw", "tank")] {
            let Some(reference) = world.pathfinding_file(file).and_then(|b| PathMap::parse(&b).ok()) else {
                println!("| {level} | {label} | no official map | | | | | | | | | |");
                continue;
            };
            let declared = Params::of_world(&world, kind);
            let fallback = Params::defaults_for(&world, kind);
            for (rules, params) in [("level", &declared), ("default", &fallback)] {
                if declared.source != navgen::Source::Level && rules == "level" {
                    println!("| {level} | {label} | the level declares no rules | | | | | | | | | |");
                    continue;
                }
                let t = std::time::Instant::now();
                let generated = navgen::generate_with(&world, params);
                let secs = t.elapsed().as_secs_f64();
                if (generated.width, generated.height) != (reference.width, reference.height) {
                    println!(
                        "| {level} | {label} | {rules}: {}x{} against {}x{} | | | | | | | | | |",
                        generated.width, generated.height, reference.width, reference.height
                    );
                    continue;
                }
                let all = Confusion::of(&generated, &reference, |_, _| true);
                let area = free_box(&reference, 64).unwrap_or([0, 0, reference.width - 1, reference.height - 1]);
                let zone = Confusion::of(&generated, &reference, |x, z| x >= area[0] && x <= area[2] && z >= area[1] && z <= area[3]);
                println!(
                    "| {level} | {label} | {rules} | {} | {} | {} | {} | {} | {} | {} | {} | {secs:.1} |",
                    pct(all.agreement()),
                    pct(all.trivial()),
                    pct(all.blocked_recall()),
                    pct(all.free_recall()),
                    pct(all.free_iou()),
                    pct(zone.agreement()),
                    pct(zone.trivial()),
                    pct(zone.blocked_iou()),
                );
                if let Some(dir) = &ppm {
                    let name = format!("{level}_{}_{}.ppm", if kind == Nav::Infantry { "inf" } else { "tank" }, if rules == "level" { "level" } else { "def" });
                    write_ppm(&dir.join(name), &generated, &reference, (generated.width / 1024).max(1)).expect("could not write the image");
                }
            }
        }
    }
}
