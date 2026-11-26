//! Where procedural grass grows (an enhanced-profile addition, not in the original): a coarse map of the
//! terrain with a density per cell, from the colour tiles' hue and the material map.
//!
//! The material map alone does not tell grass apart: most levels paint little of it (Omaha's ground is 82 %
//! material 7, «outside map»; Wake's island 95 % wet sand), so the tiles decide and the material map only
//! rules out what is never grass (water, gravel, frozen ground, rock and roads).

use bytefield_formats::image::{Image, Orientation};

use crate::Level;

/// The terrain materials (`materialManagerDefine.con`, the low nibble of the material map) where no grass grows:
/// water, gravel, frozen ground, rock and the three roads.
const BARE: [u32; 7] = [1, 8, 9, 12, 13, 14, 15];

/// The terrain materials that say grass: dry and juicy.
const GRASS: [u32; 2] = [2, 3];

/// The densest cell's grass. A cell's density is a byte, 0–255 of this.
pub struct GrassMap {
    /// cells a side, each `cell` metres
    pub size: usize,
    pub cell: f64,
    pub density: Vec<u8>,
}

impl GrassMap {
    /// From the level's colour tiles as one image ([`Level::colour_mosaic`]), with `uv` taking a point (x, z) of the level to
    /// that image's [0, 1]² (where the client draws the tiles).
    pub fn new(level: &Level, tiles: &Image, uv: impl Fn(f64, f64) -> [f64; 2]) -> GrassMap {
        let ws = level.terrain.world_size;
        let size = ((ws / 2.0) as usize).clamp(1, 2048);
        let cell = ws / size as f64;
        let mut density = vec![0u8; size * size];
        const K: usize = 3;
        for r in 0..size {
            for c in 0..size {
                let (x, z) = ((c as f64 + 0.5) * cell, (r as f64 + 0.5) * cell);
                let material = level.material_at(x, z);
                if BARE.contains(&material) {
                    continue;
                }
                // (where the level says grass, golden grass too: Dantooine's savanna is 94 % «dry grass» in that hue)
                let looks = if GRASS.contains(&material) { |px| grassiness(px).max(dry_grassiness(px)) } else { grassiness };
                let Some((h, n)) = level.height_and_normal(x, z, Orientation::IDENTITY) else { continue };
                // (not under the water nor on cliffs)
                if h < level.terrain.water_level + 0.3 || n[1] < 0.75 {
                    continue;
                }
                let mut sum = 0.0;
                for j in 0..K {
                    for i in 0..K {
                        let p = (x + ((i as f64 + 0.5) / K as f64 - 0.5) * cell, z + ((j as f64 + 0.5) / K as f64 - 0.5) * cell);
                        let [u, v] = uv(p.0, p.1);
                        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                            continue;
                        }
                        let px = tiles.get((u * tiles.width as f64) as usize, (v * tiles.height as f64) as usize);
                        if px[3] > 0 {
                            sum += looks([px[0], px[1], px[2]]);
                        }
                    }
                }
                density[r * size + c] = (sum / (K * K) as f32 * 255.0).round() as u8;
            }
        }
        // (a 3 × 3 box: the tiles' texels alone leave it speckled)
        let blurred = (0..size * size)
            .map(|i| {
                let (c, r) = ((i % size) as i64, (i / size) as i64);
                let (mut sum, mut n) = (0u32, 0u32);
                for (dc, dr) in (-1..=1).flat_map(|dc| (-1..=1).map(move |dr| (dc, dr))) {
                    let (cc, rr) = (c + dc, r + dr);
                    if (0..size as i64).contains(&cc) && (0..size as i64).contains(&rr) {
                        sum += density[rr as usize * size + cc as usize] as u32;
                        n += 1;
                    }
                }
                // (but what is bare stays bare: a road's edge)
                if density[i] == 0 { 0 } else { (sum / n) as u8 }
            })
            .collect();
        GrassMap { size, cell, density: blurred }
    }

    /// The density at (x, z), 0–1: the cell's, nearest.
    pub fn at(&self, x: f64, z: f64) -> f32 {
        let (c, r) = ((x / self.cell).floor(), (z / self.cell).floor());
        if c < 0.0 || r < 0.0 || c >= self.size as f64 || r >= self.size as f64 {
            return 0.0;
        }
        self.density[r as usize * self.size + c as usize] as f32 / 255.0
    }

    /// No grass in the box (x, z) `lo`–`hi`, grown by `margin` metres.
    pub fn clear(&mut self, lo: [f64; 2], hi: [f64; 2], margin: f64) {
        let cell = |v: f64| ((v / self.cell).floor().max(0.0) as usize).min(self.size);
        let (c0, c1) = (cell(lo[0] - margin), cell(hi[0] + margin));
        let (r0, r1) = (cell(lo[1] - margin), cell(hi[1] + margin));
        for r in r0..(r1 + 1).min(self.size) {
            for c in c0..(c1 + 1).min(self.size) {
                self.density[r * self.size + c] = 0;
            }
        }
    }

    /// The share of the cells with some grass.
    pub fn share(&self) -> f64 {
        self.density.iter().filter(|d| **d > 0).count() as f64 / self.density.len().max(1) as f64
    }

    /// The map as a grey image, row 0 at z 0 (to look at).
    pub fn image(&self) -> Image {
        let mut img = Image::new(self.size, self.size);
        for (i, d) in self.density.iter().enumerate() {
            img.put(i % self.size, i / self.size, [*d, *d, *d, 255]);
        }
        img
    }
}

/// How much a tile's colour (sRGB) looks like grass, 0–1: its hue between yellow-green and cyan-green, with some
/// saturation and neither too dark nor too light. Sand (hue 30–45°), snow, rock and asphalt stay at 0.
pub fn grassiness([r, g, b]: [u8; 3]) -> f32 {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if max <= 0.0 || d <= 0.0 {
        return 0.0;
    }
    let s = d / max;
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let hue = smoothstep(48.0, 66.0, h) * (1.0 - smoothstep(150.0, 175.0, h));
    let sat = smoothstep(0.10, 0.22, s);
    let val = smoothstep(0.08, 0.16, max) * (1.0 - smoothstep(0.85, 0.95, max));
    hue * sat * val
}

/// How much a tile's colour (sRGB) looks like golden, dry grass, 0–1: a strongly saturated yellow-orange (hue 32–48°).
/// Sand shares the hue but is paler, so this only counts where the material map says grass.
pub fn dry_grassiness([r, g, b]: [u8; 3]) -> f32 {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    if max <= 0.0 || d <= 0.0 || max != r {
        return 0.0;
    }
    let (s, h) = (d / max, 60.0 * ((g - b) / d).rem_euclid(6.0));
    let hue = smoothstep(30.0, 38.0, h) * (1.0 - smoothstep(58.0, 66.0, h));
    let sat = smoothstep(0.50, 0.65, s);
    let val = smoothstep(0.20, 0.35, max) * (1.0 - smoothstep(0.92, 0.98, max));
    hue * sat * val
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::{dry_grassiness, grassiness};

    #[test]
    fn grass_and_not() {
        // a meadow, dry grass, sand, snow, asphalt
        assert!(grassiness([70, 100, 40]) > 0.9);
        assert!(grassiness([120, 125, 60]) > 0.5);
        assert_eq!(grassiness([200, 170, 120]), 0.0);
        assert_eq!(grassiness([235, 240, 245]), 0.0);
        assert_eq!(grassiness([90, 90, 92]), 0.0);
        // golden grass (Dantooine) and not: pale sand, red rock (Ryloth)
        assert!(dry_grassiness([220, 160, 40]) > 0.9);
        assert_eq!(dry_grassiness([200, 170, 120]), 0.0);
        assert_eq!(dry_grassiness([170, 70, 30]), 0.0);
    }
}

/// Where a point (x, z) of the level falls on the colour tiles as one image of `cols` × `rows` tiles ([0, 1]²): tiles of
/// worldSize / (cols + 2 `texOffsetX`) metres, from `texOffsetX` tiles in x and `texOffsetY` (0 if negative) in z, as the
/// client draws the terrain.
pub fn tiles_uv(level: &Level, [cols, rows]: [usize; 2]) -> impl Fn(f64, f64) -> [f64; 2] + use<> {
    let ws = level.terrain.world_size;
    let [ox, oy] = level.terrain.tex_offset;
    let tile = ws / (cols as f64 + 2.0 * ox);
    let (cols, rows, oy) = (cols as f64, rows as f64, oy.max(0.0));
    move |x, z| [(x / tile - ox) / cols, (z / tile - oy) / rows]
}
