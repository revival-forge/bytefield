//! The bots' path map around a place, as bots sees it (2 m cells), with the path it finds from there to a goal and the obstacles
//! near it (how high their collision mesh goes over the ground there):
//!   nav_around <game> <mod> <level> <tank|swim|infantry> <x> <z> [radius] [--to gx,gz [--leaving]] [--mode m]
//! `#` blocked, `.` free, `*` the path, `@` the place, `o` an obstacle's centre. `--px`: the map's own 1 m pixels instead (no path).
use bytefield_sim::bot_api::Nav;
use bytefield_sim::bots::body::paths::{Grid, CELL};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(|s| s.as_str());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], opt("--mode").unwrap_or("conquest")).expect("world");
    let kind = if a[4] == "tank" || a[4] == "swim" { Nav::Tank } else { Nav::Infantry };
    let (x, z): (f32, f32) = (a[5].parse().unwrap(), a[6].parse().unwrap());
    let r: f32 = a.get(7).and_then(|v| v.parse().ok()).unwrap_or(40.0);
    let m = w.nav_map(kind).expect("no path map");
    // (`swim`: the tank map of what swims, with the water open; its deep water is not drawn apart)
    let m = if a[4] == "swim" { std::sync::Arc::new(bytefield_sim::navgen::swim_map(&w, &m).expect("swim map").0) } else { m };
    // (`--profile gx,gz`: the ground every 10 m on the straight line to there, against the water line; `~` under it)
    if let Some(t) = opt("--profile") {
        let (gx, gz) = t.split_once(',').unwrap();
        let (gx, gz): (f32, f32) = (gx.parse().unwrap(), gz.parse().unwrap());
        let water = w.level.terrain.water_level;
        let n = ((gx - x).hypot(gz - z) / 10.0).ceil() as usize;
        for i in 0..=n {
            let f = i as f32 / n.max(1) as f32;
            let (px, pz) = (x + (gx - x) * f, z + (gz - z) * f);
            let h = w.height_at(px as f64, pz as f64).unwrap_or(f64::NAN);
            let free = m.pixels.get(pz as usize * m.width + px as usize).is_some_and(|&p| p == 0);
            println!("{:5.0} {:5.0}  {h:6.1}{} {}", px, pz, if h < water { " ~" } else { "  " }, if free { "free" } else { "blocked" });
        }
        println!("water line {water:.1}");
        return;
    }
    let g = Grid::from_map(&m, w.nav_map_shipped(kind));
    // (`--gen <brush>`: the map navgen makes for this level with that mover's radius, instead of the level's own: 0 shows the walls)
    let m = match opt("--gen").and_then(|b| b.parse::<f64>().ok()) {
        Some(brush) => std::sync::Arc::new(bytefield_sim::navgen::generate_with(&w, &bytefield_sim::navgen::Params { brush, ..bytefield_sim::navgen::Params::of_world(&w, kind) })),
        None => m,
    };
    if a.iter().any(|x| x == "--px") {
        let (x0, x1, z0, z1) = ((x - r).max(0.0) as usize, (x + r) as usize, (z - r).max(0.0) as usize, (z + r) as usize);
        println!("1 m pixels, x {x0}..{x1} left to right, z {z0}..{z1} top to bottom");
        for pz in z0..=z1.min(m.height - 1) {
            let row: String = (x0..=x1.min(m.width - 1)).map(|px| if px == x as usize && pz == z as usize { '@' } else if m.pixels[pz * m.width + px] != 0 { '#' } else { '.' }).collect();
            println!("{row}");
        }
        return;
    }
    let path = opt("--to").map(|t| {
        let (gx, gz) = t.split_once(',').unwrap();
        let to = [gx.parse().unwrap(), gz.parse().unwrap()];
        // --leaving: from a cell cut off from the goal's zone, out by the nearest cell of that zone (as a walker leaving a closed spot)
        if a.iter().any(|x| x == "--leaving") { g.path_from([x, z], to, &|_| true) } else { g.path([x, z], to) }.unwrap_or_default()
    });
    if let Some(p) = &path {
        println!("path {} nodes, first {:?}", p.len(), p.iter().take(8).map(|q| q.map(|c| c.round() as i32)).collect::<Vec<_>>());
    }
    let on_path = |cx: i32, cz: i32| {
        path.as_ref().is_some_and(|p| {
            p.windows(2).any(|s| {
                let n = ((s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]) / 0.5).ceil().max(1.0) as i32;
                (0..=n).any(|k| {
                    let f = k as f32 / n as f32;
                    let q = [s[0][0] + f * (s[1][0] - s[0][0]), s[0][1] + f * (s[1][1] - s[0][1])];
                    (q[0] / CELL).floor() as i32 == cx && (q[1] / CELL).floor() as i32 == cz
                })
            })
        })
    };
    let near: Vec<_> = w.obstacles().iter().filter(|o| (o.center[0] - x).hypot(o.center[2] - z) < r + o.radius.min(30.0)).collect();
    let (c0, c1) = (((x - r) / CELL).floor() as i32, ((x + r) / CELL).floor() as i32);
    let (r0, r1) = (((z - r) / CELL).floor() as i32, ((z + r) / CELL).floor() as i32);
    println!("x {}..{} left to right, z {}..{} top to bottom", c0 as f32 * CELL, c1 as f32 * CELL, r0 as f32 * CELL, r1 as f32 * CELL);
    for cz in r0..=r1 {
        let row: String = (c0..=c1)
            .map(|cx| {
                let (px, pz) = ((cx as f32 + 0.5) * CELL, (cz as f32 + 0.5) * CELL);
                if (px / CELL).floor() == (x / CELL).floor() && (pz / CELL).floor() == (z / CELL).floor() {
                    '@'
                } else if near.iter().any(|o| (o.center[0] / CELL).floor() as i32 == cx && (o.center[2] / CELL).floor() as i32 == cz) {
                    'o'
                } else if on_path(cx, cz) {
                    '*'
                } else if !g.clear(px, pz) {
                    '#'
                } else {
                    '.'
                }
            })
            .collect();
        println!("{row}");
    }
    for o in &near {
        let ground = w.height_at(o.center[0] as f64, o.center[2] as f64).unwrap_or(0.0) as f32;
        let (lo, hi) = o.lods.first().map_or((f32::NAN, f32::NAN), |mesh| {
            mesh.vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| {
                let y = (o.rows[0][1] * v[0] * o.scale[0] + o.rows[1][1] * v[1] * o.scale[1] + o.rows[2][1] * v[2] * o.scale[2]) + o.pos[1];
                (lo.min(y), hi.max(y))
            })
        });
        println!("{} at {:?} centre {:?} r {:.1}: collision {:.1}..{:.1} m over the ground", o.name, o.pos.map(|c| c.round() as i32), o.center.map(|c| c.round() as i32), o.radius, lo - ground, hi - ground);
    }
}
