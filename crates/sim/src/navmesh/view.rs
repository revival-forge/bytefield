//! A top-down picture of the mesh around a point: each pixel the floors over it, coloured by how many there are (one: by
//! its height over the terrain; several: the highest's colour, lighter), reached or not, with the ladders' links (cyan) and the drops (blue).
use super::NavMesh;
use crate::World;
use bytefield_formats::image::Image;

/// What a pixel shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Legend {
    /// pixels with a floor on the mesh joined to the seeds, an island, nothing
    pub reached: usize,
    pub island: usize,
    pub none: usize,
    /// pixels with two floors or more
    pub layered: usize,
}

/// The mesh around (x, z), `r` m each way, `px` m a pixel, north (+z) up, only the floors with heights in `band`. Reached floors: green on the terrain, then yellow,
/// orange, red, magenta the higher over it (1, 3, 6, 12 m); unreached (islands) in grey; where there are several floors the
/// pixel is lighter; the terrain without a floor, dark; ladders as cyan lines.
#[allow(clippy::too_many_arguments)]
pub fn picture(mesh: &NavMesh, world: &World, reached: &[bool], x: f32, z: f32, r: f32, px: f32, band: [f32; 2]) -> (Image, Legend) {
    let n = ((2.0 * r / px).ceil() as usize).max(1);
    let mut img = Image::new(n, n);
    let mut legend = Legend::default();
    let (x0, z0) = (x - r, z - r);
    // the polygons that touch the window, by pixel row: rasterize each polygon's box and test the pixels' centres
    let mut floors: Vec<Vec<(f32, bool)>> = vec![Vec::new(); n * n];
    let tm = mesh.tile_m();
    let side = mesh.side as i32;
    let t = |v: f32| ((v / tm).floor() as i32).clamp(0, side - 1);
    for tz in t(z0)..=t(z + r) {
        for tx in t(x0)..=t(x + r) {
            let ti = (tz * side + tx) as usize;
            for p in mesh.tile_first[ti]..mesh.tile_first[ti + 1] {
                let v: Vec<[f32; 3]> = mesh.poly_verts(p).collect();
                let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                for q in &v {
                    lo = [lo[0].min(q[0]), lo[1].min(q[2])];
                    hi = [hi[0].max(q[0]), hi[1].max(q[2])];
                }
                let i0 = (((lo[0] - x0) / px).floor().max(0.0)) as usize;
                let i1 = (((hi[0] - x0) / px).ceil().max(0.0) as usize).min(n);
                let j0 = (((lo[1] - z0) / px).floor().max(0.0)) as usize;
                let j1 = (((hi[1] - z0) / px).ceil().max(0.0) as usize).min(n);
                for j in j0..j1 {
                    for i in i0..i1 {
                        let (cx, cz) = (x0 + (i as f32 + 0.5) * px, z0 + (j as f32 + 0.5) * px);
                        let q = mesh.closest_on(p, cx, cz);
                        if (q[0] - cx).abs() < 1e-4 && (q[2] - cz).abs() < 1e-4 && q[1] >= band[0] && q[1] <= band[1] {
                            floors[j * n + i].push((q[1], reached[p as usize]));
                        }
                    }
                }
            }
        }
    }
    for j in 0..n {
        for i in 0..n {
            let (cx, cz) = (x0 + (i as f32 + 0.5) * px, z0 + (j as f32 + 0.5) * px);
            let ground = world.height_at(cx as f64, cz as f64).unwrap_or(0.0) as f32;
            let f = &mut floors[j * n + i];
            f.sort_by(|a, b| a.0.total_cmp(&b.0));
            // floors closer than a soldier's height are one (both sides of a seam)
            f.dedup_by(|b, a| b.0 - a.0 < 1.0);
            let colour = match f.last() {
                None => {
                    legend.none += 1;
                    [40, 40, 48, 255]
                }
                Some(&(y, on)) => {
                    if on {
                        legend.reached += 1;
                    } else {
                        legend.island += 1;
                    }
                    let mut c = if on { by_height(y - ground) } else { [130, 130, 130] };
                    if f.len() > 1 {
                        legend.layered += 1;
                        c = c.map(|v| v / 2 + 128);
                    }
                    [c[0], c[1], c[2], 255]
                }
            };
            // north up: the picture's first row is the window's highest z
            img.put(i, n - 1 - j, colour);
        }
    }
    for l in &mesh.links {
        let (a, b) = (l.from, l.to);
        // a ladder from centre to centre in cyan; a drop, from the edge above to where it lands, in blue; a jump in orange; a narrow
        // door in white
        let (ends, colour) = match l.kind {
            super::LinkKind::Ladder => (vec![mesh.centre(l.a), a, b, mesh.centre(l.b)], [0, 230, 255, 255]),
            super::LinkKind::Drop => (vec![a, b], [40, 90, 255, 255]),
            super::LinkKind::Jump => (vec![a, b], [255, 140, 0, 255]),
            super::LinkKind::Door => (vec![a, b], [255, 255, 255, 255]),
            super::LinkKind::Exit => (vec![a, b], [120, 255, 120, 255]),
        };
        for w in ends.windows(2) {
            line(&mut img, (w[0][0] - x0) / px, (w[0][2] - z0) / px, (w[1][0] - x0) / px, (w[1][2] - z0) / px, colour);
        }
    }
    (img, legend)
}

pub fn by_height(h: f32) -> [u8; 3] {
    match h {
        h if h < 1.0 => [40, 170, 60],
        h if h < 3.0 => [230, 220, 40],
        h if h < 6.0 => [240, 140, 30],
        h if h < 12.0 => [220, 40, 40],
        _ => [210, 60, 220],
    }
}

fn line(img: &mut Image, x0: f32, z0: f32, x1: f32, z1: f32, c: [u8; 4]) {
    let n = ((x1 - x0).abs().max((z1 - z0).abs()).ceil() as usize).max(1);
    let (w, h) = (img.width as i64, img.height as i64);
    for k in 0..=n {
        let t = k as f32 / n as f32;
        let (x, z) = ((x0 + (x1 - x0) * t) as i64, (z0 + (z1 - z0) * t) as i64);
        if x >= 0 && z >= 0 && x < w && z < h {
            img.put(x as usize, (h - 1 - z) as usize, c);
        }
    }
}
