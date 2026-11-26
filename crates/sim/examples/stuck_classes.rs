//! What kind of place the bots of a stuck sweep got stuck in, from where they stood: how much of the time
//! stuck each fix in the way the bots find their way could win at most.
//!   stuck_classes <game> <sweep dir> [--min 15] [--list N]
//! The same events as `stuck_hotspots.py report` (stuck at least --min s, less than half of it fighting). On foot, the first of:
//! `deck` (over 3 m above the terrain: a ship's deck, a floor, a walkway; the path map is flat), `grid` (the path map's 2 m cells
//! and its 1 m pixels disagree within 2 m: a 2 m cell with free and blocked pixels, or the bot on a cell the grid shuts), `object`
//! (an object spawner within 8 m: a parked vehicle or a gun the map knows nothing of), `other`. Driving: `mounted`.
//! With `--mesh`, also where the walkable-surface mesh puts each bot on foot (its feet 1 m under the position): `on` a polygon
//! reached from the spawns within 1 m across and 1.5 m up or down, `near` one within 3 m, `island` a polygon not reached from
//! them, `off` none; by class, in seconds and weighted.
use bytefield_sim::bot_api::Nav;
use bytefield_sim::bots::body::paths::{Grid, CELL};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const MODS: [&str; 4] = ["bf1942", "DC_Final", "GCMOD", "DesertCombat"];

struct Ev {
    dur: f64,
    pos: [f32; 3],
    foot: bool,
    file: String,
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("stuck_classes <game> <sweep dir> [--min 15] [--list N] [--weights data/out/stuck/weights.json] [--mesh]");
        std::process::exit(2);
    }
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let min: f64 = opt("--min").map_or(15.0, |v| v.parse().unwrap());
    let list: usize = opt("--list").map_or(0, |v| v.parse().unwrap());
    let with_mesh = a.iter().any(|x| x == "--mesh");
    // (class, place on the mesh) -> seconds, and weighted
    let mut on_mesh: BTreeMap<(&str, &str), (f64, f64)> = BTreeMap::new();
    // the weight of each map by what people play: the classes again as seconds per round, weighted
    let weights: HashMap<String, f64> = opt("--weights")
        .map(|p| serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(p).expect("weights")).expect("weights json"))
        .and_then(|v| v["weights"].as_object().map(|o| o.iter().map(|(k, w)| (k.clone(), w.as_f64().unwrap_or(0.0))).collect()))
        .unwrap_or_default();
    let mut rounds: HashMap<String, f64> = HashMap::new();
    // (mod, level) -> events
    let mut by_level: BTreeMap<(String, String), Vec<Ev>> = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir(&a[2]).expect("dir").filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for f in &files {
        let Ok(r) = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(f).unwrap_or_default()) else { continue };
        let stem = f.file_stem().unwrap().to_string_lossy().to_string();
        let ml = stem.rsplit_once("_s").map_or(stem.as_str(), |x| x.0);
        let Some(m) = MODS.iter().find(|m| ml.starts_with(&format!("{m}_"))) else { continue };
        let level = ml[m.len() + 1..].to_string();
        *rounds.entry(ml.to_string()).or_default() += 1.0;
        for e in r["events"].as_array().into_iter().flatten() {
            if e["kind"] != "stuck" {
                continue;
            }
            let dur = e["dur"].as_f64().unwrap_or(0.0);
            let d = e["detail"].as_str().unwrap_or("");
            let fighting: f64 = d.rsplit_once('(').and_then(|x| x.1.split_once(" s")).and_then(|x| x.0.parse().ok()).unwrap_or(0.0);
            if dur < min || fighting > 0.5 * dur {
                continue;
            }
            let what = d.split_once(" in ").map_or("?", |x| x.1.split(" (").next().unwrap_or("?"));
            let p = &e["pos"];
            let pos = [0, 1, 2].map(|i| p[i].as_f64().unwrap_or(0.0) as f32);
            by_level.entry((m.to_string(), level.clone())).or_default().push(Ev { dur, pos, foot: what == "foot", file: stem.clone() });
        }
    }
    let mut total: HashMap<&str, f64> = HashMap::new();
    let mut weighted: HashMap<&str, f64> = HashMap::new();
    let mut flags: HashMap<&str, f64> = HashMap::new();
    let mut per_level: Vec<(String, &str, f64)> = Vec::new();
    // (seconds of the events on foot off decks, and those seconds weighted by how often places near them have the grid flag)
    let mut control = (0.0f64, 0.0f64, 0.0f64);
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    let mut rows: Vec<(f64, &str, String)> = Vec::new();
    for ((m, level), evs) in &by_level {
        let Ok(w) = bytefield_sim::World::load(Path::new(&a[1]), m, level, "conquest") else {
            eprintln!("{m} {level}: no world");
            continue;
        };
        let map = w.nav_map(Nav::Infantry);
        let grid = map.as_ref().map(|mm| Grid::from_map(mm, w.nav_map_shipped(Nav::Infantry)));
        let spawners: Vec<[f64; 3]> = w.level.spawners.iter().map(|s| s.pos).collect();
        let shipped = w.nav_map_shipped(Nav::Infantry);
        let free_px = |x: f32, z: f32| map.as_ref().is_some_and(|mm| x >= 0.0 && z >= 0.0 && (x as usize) < mm.width && (z as usize) < mm.height && mm.pixels[z as usize * mm.width + x as usize] == 0);
        // a 2 m cell within 2 m of (x, z) whose 1 m pixels are part free and part blocked; and whether the grid shuts the cell there
        let lie_at = |x: f32, z: f32| match (&map, &grid) {
            (Some(mm), Some(g)) => {
                let px = |px: i64, pz: i64| px >= 0 && pz >= 0 && (px as usize) < mm.width && (pz as usize) < mm.height && mm.pixels[pz as usize * mm.width + px as usize] != 0;
                let k = CELL as i64;
                let (cx, cz) = ((x / CELL).floor() as i64, (z / CELL).floor() as i64);
                let mixed = (cz - 1..=cz + 1).any(|qz| {
                    (cx - 1..=cx + 1).any(|qx| {
                        let b = (0..k * k).filter(|i| px(qx * k + i % k, qz * k + i / k)).count() as i64;
                        b > 0 && b < k * k
                    })
                });
                (mixed, !g.clear(x, z))
            }
            _ => (false, false),
        };
        let mesh = with_mesh.then(|| {
            let m = w.navmesh();
            let reached = m.reach(&bytefield_sim::navmesh::seeds(&w));
            (m, reached)
        });
        let mut here: HashMap<&str, f64> = HashMap::new();
        for e in evs {
            let [x, y, z] = e.pos;
            let above = w.height_at(x as f64, z as f64).map_or(0.0, |h| y as f64 - h);
            let deck = above > 3.0;
            let (mixed, own_shut) = lie_at(x, z);
            let lie = mixed || own_shut;
            // the same at free pixels 5-25 m away, picked at random: how often a place near here is that close to such a cell anyway
            let mut near = (0, 0);
            if e.foot && !deck {
                for _ in 0..40 {
                    let (ang, r) = (rnd() * std::f32::consts::TAU, 5.0 + 20.0 * rnd());
                    let (qx, qz) = (x + r * ang.cos(), z + r * ang.sin());
                    if !free_px(qx, qz) {
                        continue;
                    }
                    let (m2, o2) = lie_at(qx, qz);
                    near.0 += 1;
                    near.1 += (m2 || o2) as u32;
                }
                if near.0 > 0 {
                    control.0 += e.dur;
                    control.1 += e.dur * near.1 as f64 / near.0 as f64;
                    control.2 += if lie { e.dur } else { 0.0 };
                }
                if !free_px(x, z) {
                    *flags.entry(if shipped { "own px, level's" } else { "own px, navgen" }).or_default() += e.dur;
                    // how deep in the blocked pixels: 1 is against a wall (the map's margin, the mover's radius); deeper, the map and
                    // the bodies disagree (a slope or a mesh it shuts that a soldier walks, or a whole place left unfilled)
                    let depth = (1..=12).find(|&r| (-r..=r).any(|i: i32| (-r..=r).any(|j: i32| (i.abs() == r || j.abs() == r) && free_px(x + i as f32, z + j as f32)))).unwrap_or(99);
                    let band = match depth {
                        1 => "depth 1",
                        2..=3 => "depth 2-3",
                        4..=12 => "depth 4-12",
                        _ => "depth >12",
                    };
                    *flags.entry(band).or_default() += e.dur;
                }
                if own_shut {
                    *flags.entry(if shipped { "own shut, level's" } else { "own shut, navgen" }).or_default() += e.dur;
                }
                *flags.entry(if shipped { "grid, level's map" } else { "grid, navgen map" }).or_default() += if lie { e.dur } else { 0.0 };
                *flags.entry(if shipped { "foot, level's map" } else { "foot, navgen map" }).or_default() += e.dur;
            }
            let object = spawners.iter().any(|s| (s[0] as f32 - x).hypot(s[2] as f32 - z) < 8.0);
            let class = if !e.foot {
                "mounted"
            } else if deck {
                "deck"
            } else if lie {
                "grid"
            } else if object {
                "object"
            } else {
                "other"
            };
            *total.entry(class).or_default() += e.dur;
            let ml = format!("{m}_{level}");
            if let Some(w) = weights.get(&ml) {
                *weighted.entry(class).or_default() += w * e.dur / rounds[&ml];
            }
            *here.entry(class).or_default() += e.dur;
            if e.foot {
                for (f, on) in [("deck", deck), ("grid", lie), ("object", object)] {
                    if on {
                        *flags.entry(f).or_default() += e.dur;
                    }
                }
            }
            if e.foot
                && let Some((nm, reached)) = &mesh
            {
                let at = |r: f32, dy: f32| nm.nearest(x, y - 1.0, z, r, dy);
                let place = match at(1.0, 1.5) {
                    Some(p) if reached[p as usize] => "on",
                    Some(_) => "island",
                    None => match at(3.0, 2.0) {
                        Some(p) if reached[p as usize] => "near",
                        Some(_) => "island",
                        None => "off",
                    },
                };
                let ml = format!("{m}_{level}");
                let slot = on_mesh.entry((class, place)).or_default();
                slot.0 += e.dur;
                if let Some(w) = weights.get(&ml) {
                    slot.1 += w * e.dur / rounds[&ml];
                }
            }
            rows.push((e.dur, class, format!("{} {} {:.0} {:.0} {:.0} (above {:.1})", e.file, level, x, y, z, above)));
        }
        for (c, s) in here {
            per_level.push((format!("{m} {level}"), c, s));
        }
    }
    let sum: f64 = total.values().sum();
    println!("{} levels, {sum:.0} s stuck (events of {min:.0} s or more, less than half fighting)", by_level.len());
    println!("class      s      %   (on foot, overlapping: any flag)");
    for c in ["deck", "grid", "object", "other", "mounted"] {
        let s = total.get(c).copied().unwrap_or(0.0);
        let f = flags.get(c).map_or(String::new(), |v| format!("   any {v:.0}"));
        println!("{c:8} {s:6.0} {:5.1}{f}", 100.0 * s / sum.max(1.0));
    }
    if !weights.is_empty() {
        // renormalised over the maps the sweep has, as `stuck_hotspots.py report --weights`
        let covered: f64 = rounds.keys().filter_map(|ml| weights.get(ml)).sum();
        let wsum: f64 = weighted.values().sum::<f64>() / covered.max(1e-9);
        println!("weighted by what is played ({:.0} % of the weight): {wsum:.1} s stuck per round", 100.0 * covered);
        for c in ["deck", "grid", "object", "other", "mounted"] {
            let s = weighted.get(c).copied().unwrap_or(0.0) / covered.max(1e-9);
            println!("  {c:8} {s:6.1} {:5.1}", 100.0 * s / wsum.max(1e-9));
        }
    }
    for f in ["depth 1", "depth 2-3", "depth 4-12", "depth >12", "own px, level's", "own px, navgen", "own shut, level's", "own shut, navgen", "grid, level's map", "foot, level's map", "grid, navgen map", "foot, navgen map"] {
        println!("{f:18} {:6.0}", flags.get(f).copied().unwrap_or(0.0));
    }
    println!("grid flag at random free places 5-25 m from the events on foot (control): {:.0} % (at the events: {:.0} %)", 100.0 * control.1 / control.0.max(1.0), 100.0 * control.2 / control.0.max(1.0));
    if with_mesh {
        let covered: f64 = rounds.keys().filter_map(|ml| weights.get(ml)).sum::<f64>().max(1e-9);
        println!("\non foot, where the mesh puts them: s (weighted s per round)");
        println!("{:8} {:>14} {:>14} {:>14} {:>14}", "class", "on", "near", "island", "off");
        for c in ["deck", "grid", "object", "other", ""] {
            let cell = |pl: &str| {
                let (s, w) = on_mesh.iter().filter(|((k, q), _)| (c.is_empty() || *k == c) && *q == pl).fold((0.0, 0.0), |a, (_, v)| (a.0 + v.0, a.1 + v.1));
                format!("{s:6.0} ({:5.1})", w / covered)
            };
            println!("{:8} {:>14} {:>14} {:>14} {:>14}", if c.is_empty() { "all" } else { c }, cell("on"), cell("near"), cell("island"), cell("off"));
        }
    }
    per_level.sort_by(|a, b| b.2.total_cmp(&a.2));
    println!("\nby level and class (top 25)");
    for (l, c, s) in per_level.iter().take(25) {
        println!("{s:6.0}  {c:8} {l}");
    }
    if list > 0 {
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("\nlongest events");
        for (d, c, r) in rows.iter().take(list) {
            println!("{d:5.0}  {c:8} {r}");
        }
    }
}
