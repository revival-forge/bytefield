//! A plain RGBA8 image with the few operations the headless checks need.

use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// row-major, top row first, 4 bytes per pixel
    pub rgba: Vec<u8>,
}

/// The 8 ways to lay a square image down: 4 rotations, each optionally mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orientation {
    pub transpose: bool,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl Orientation {
    pub const ALL: [Orientation; 8] = {
        let mut all = [Orientation { transpose: false, flip_x: false, flip_y: false }; 8];
        let mut i = 0;
        while i < 8 {
            all[i] = Orientation { transpose: i & 4 != 0, flip_x: i & 1 != 0, flip_y: i & 2 != 0 };
            i += 1;
        }
        all
    };
    pub const IDENTITY: Orientation = Orientation { transpose: false, flip_x: false, flip_y: false };
}

impl Image {
    pub fn new(width: usize, height: usize) -> Image {
        Image { width, height, rgba: vec![0; width * height * 4] }
    }

    pub fn get(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        self.rgba[i..i + 4].try_into().unwrap()
    }

    pub fn put(&mut self, x: usize, y: usize, p: [u8; 4]) {
        let i = (y * self.width + x) * 4;
        self.rgba[i..i + 4].copy_from_slice(&p);
    }

    /// Copy `src` with its top-left corner at (x, y).
    pub fn blit(&mut self, src: &Image, x: usize, y: usize) {
        for sy in 0..src.height.min(self.height.saturating_sub(y)) {
            for sx in 0..src.width.min(self.width.saturating_sub(x)) {
                self.put(x + sx, y + sy, src.get(sx, sy));
            }
        }
    }

    /// Box-filter (or nearest, when growing) to a new size.
    pub fn resize(&self, w: usize, h: usize) -> Image {
        let mut out = Image::new(w, h);
        for y in 0..h {
            let (y0, y1) = (y * self.height / h, ((y + 1) * self.height / h).max(y * self.height / h + 1));
            for x in 0..w {
                let (x0, x1) = (x * self.width / w, ((x + 1) * self.width / w).max(x * self.width / w + 1));
                let mut acc = [0u32; 4];
                for sy in y0..y1.min(self.height) {
                    for sx in x0..x1.min(self.width) {
                        let p = self.get(sx, sy);
                        for c in 0..4 {
                            acc[c] += p[c] as u32;
                        }
                    }
                }
                let n = ((y1.min(self.height) - y0) * (x1.min(self.width) - x0)).max(1) as u32;
                out.put(x, y, acc.map(|v| (v / n) as u8));
            }
        }
        out
    }

    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Image {
        let mut out = Image::new(w, h);
        for oy in 0..h.min(self.height.saturating_sub(y)) {
            for ox in 0..w.min(self.width.saturating_sub(x)) {
                out.put(ox, oy, self.get(x + ox, y + oy));
            }
        }
        out
    }

    pub fn oriented(&self, o: Orientation) -> Image {
        let (w, h) = if o.transpose { (self.height, self.width) } else { (self.width, self.height) };
        let mut out = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (mut sx, mut sy) = if o.transpose { (y, x) } else { (x, y) };
                if o.flip_x {
                    sx = self.width - 1 - sx;
                }
                if o.flip_y {
                    sy = self.height - 1 - sy;
                }
                out.put(x, y, self.get(sx, sy));
            }
        }
        out
    }

    pub fn luma(&self) -> Vec<f64> {
        self.rgba.chunks_exact(4).map(|p| 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64).collect()
    }

    pub fn write_png(&self, path: &Path) -> io::Result<()> {
        let f = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(io::BufWriter::new(f), self.width as u32, self.height as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(io::Error::other)?;
        w.write_image_data(&self.rgba).map_err(io::Error::other)?;
        Ok(())
    }
}

/// Pearson correlation of two equally long series (1 = same up to brightness and contrast).
pub fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len()) as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    if saa == 0.0 || sbb == 0.0 { 0.0 } else { sab / (saa * sbb).sqrt() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientations_are_distinct_and_invertible() {
        let mut img = Image::new(3, 3);
        for i in 0..9 {
            img.put(i % 3, i / 3, [i as u8, 0, 0, 255]);
        }
        let all: Vec<Image> = Orientation::ALL.iter().map(|&o| img.oriented(o)).collect();
        for i in 0..8 {
            for j in i + 1..8 {
                assert_ne!(all[i], all[j]);
            }
        }
        assert_eq!(img.oriented(Orientation::IDENTITY), img);
    }

    #[test]
    fn correlation_ignores_brightness() {
        let a = [1.0, 2.0, 3.0, 4.0];
        let b = [12.0, 14.0, 16.0, 18.0];
        assert!((correlation(&a, &b) - 1.0).abs() < 1e-12);
    }
}
