//! Where the enhanced profile's grass grows (`bytefield_world::grass`), with each terrain material's share:
//! grass_map <game> <mod> <out dir> <level>...  writes <level>-grass.png (the density) and <level>-tiles.png (the tiles, at
//! the same size and orientation: row 0 at z 0).
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let out = std::path::Path::new(&a[2]);
    std::fs::create_dir_all(out).expect("out dir");
    for level in &a[3..] {
        let w = match bytefield_sim::World::load(std::path::Path::new(&a[0]), &a[1], level, "conquest") {
            Ok(w) => w,
            Err(e) => {
                println!("{level}: {e}");
                continue;
            }
        };
        let lvl = &w.level;
        if let Some((n, bytes)) = &lvl.material_map {
            let mut count = [0usize; 16];
            for b in &bytes[..n * n] {
                count[(b & 0xf) as usize] += 1;
            }
            let shares: Vec<String> = count.iter().enumerate().filter(|(_, c)| **c > 0).map(|(m, c)| format!("{m}:{:.0}%", 100.0 * *c as f64 / (n * n) as f64)).collect();
            print!("{level}: materials {} ", shares.join(" "));
        }
        let Some((mut img, grid)) = lvl.colour_mosaic(&w.vfs, 128) else {
            println!("{level}: no tiles");
            continue;
        };
        // (only the rows of tiles there are, as the client)
        let row_bytes = img.width * 128 * 4;
        let rows = (1..=grid).rev().find(|r| img.rgba[(r - 1) * row_bytes..r * row_bytes].iter().any(|&b| b != 0)).unwrap_or(grid);
        img.rgba.truncate(rows * row_bytes);
        img.height = rows * 128;
        let uv = bytefield_world::grass::tiles_uv(lvl, [grid, rows]);
        let map = bytefield_world::grass::GrassMap::new(lvl, &img, &uv);
        let share = map.density.iter().filter(|d| **d > 64).count() as f64 / map.density.len() as f64;
        // (and where it is densest, in 32 m squares of the middle half: to look at it; the edges are out of play)
        let step = (32.0 / map.cell) as usize;
        let (lo, hi) = (map.size / step / 4, map.size / step * 3 / 4);
        let best = (lo..hi).flat_map(|j| (lo..hi).map(move |i| (i, j))).max_by_key(|&(i, j)| (0..step * step).map(|k| map.density[(j * step + k / step) * map.size + i * step + k % step] as u32).sum::<u32>()).unwrap_or((0, 0));
        let (x, z) = ((best.0 as f64 + 0.5) * 32.0, (best.1 as f64 + 0.5) * 32.0);
        let y = lvl.height_at(x, z, bytefield_formats::image::Orientation::IDENTITY).unwrap_or(0.0);
        println!("grass {:.0}%, densest at {x:.0},{y:.1},{z:.0}", share * 100.0);
        map.image().write_png(&out.join(format!("{level}-grass.png"))).expect("png");
        let mut tiles = bytefield_formats::image::Image::new(map.size, map.size);
        for r in 0..map.size {
            for c in 0..map.size {
                let [u, v] = uv((c as f64 + 0.5) * map.cell, (r as f64 + 0.5) * map.cell);
                if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
                    tiles.put(c, r, img.get((u * img.width as f64) as usize, (v * img.height as f64) as usize));
                }
            }
        }
        tiles.write_png(&out.join(format!("{level}-tiles.png"))).expect("png");
    }
}
