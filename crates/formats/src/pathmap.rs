//! The original AI's pathfinding maps (`Levels/<level>/Pathfinding/<Type><n>Level<k>Map.raw`),
//! read as the server's cell map does: five int32 (patches in
//! x and in y as bits, a patch's side as bits, a scale shift, log2 of the bits per pixel), the
//! fill words (patches of one value), then per patch, row by row, the index of its fill word or
//! −1 and its own words. A patch covers 2^side m of the map in pixels of 2^scale m (level k:
//! scale k); in a patch the pixels go row by row, 32/bpp to a word, the first in the lowest bits.
//! Level 0 of a 2048 m map is 2048² pixels of 1 m, x along x and y along z; 1 is blocked for that
//! kind of mover. `CellMap` keeps the words and answers `getPixel` as the server does (the A*'s
//! maps); `PathMap` is the same as an image.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathMapError {
    Short,
    BadIndex(i32),
    Unsupported(u32),
}

/// A decoded map: its side in pixels and one value per pixel (row-major, rows along z).
#[derive(Debug, Clone)]
pub struct PathMap {
    pub width: usize,
    pub height: usize,
    pub bpp: u32,
    pub pixels: Vec<u8>,
}

fn i32_at(b: &[u8], at: usize) -> Result<i32, PathMapError> {
    b.get(at..at + 4).map(|s| i32::from_le_bytes(s.try_into().unwrap())).ok_or(PathMapError::Short)
}

/// A map as the server keeps it (`CellMap`): the header and, per patch, its words (fill patches
/// share theirs).
#[derive(Debug, Clone)]
pub struct CellMap {
    /// patches in x and in z, as bits
    pub x_bits: u32,
    pub z_bits: u32,
    /// a patch's side in m, as bits
    pub patch_bits: u32,
    /// a pixel's side in m, as bits
    pub scale: u32,
    /// log2 of the bits per pixel
    pub bpp_bits: u32,
    /// the start of each patch's words in `words`
    patches: Vec<usize>,
    words: Vec<u32>,
    pub words_per_patch: usize,
}

impl CellMap {
    pub fn parse(b: &[u8]) -> Result<CellMap, PathMapError> {
        let x_bits = i32_at(b, 0)? as u32;
        let z_bits = i32_at(b, 4)? as u32;
        let patch_bits = i32_at(b, 8)? as u32;
        let scale = i32_at(b, 12)? as u32;
        let bpp_bits = i32_at(b, 16)? as u32;
        if x_bits > 8 || z_bits > 8 || patch_bits > 10 || scale > patch_bits || bpp_bits > 3 {
            return Err(PathMapError::Unsupported(bpp_bits));
        }
        let mut at = 20;
        let n = i32_at(b, at)?.max(0) as usize;
        at += 4;
        // CellMap::CellMap: 2^(bpp bits − 5 + 2·(patch bits − scale)) words, at least 2
        let words_per_patch = 1usize << (bpp_bits as i32 - 5 + 2 * (patch_bits - scale) as i32).max(1);
        // the fill patches first (one word repeated), then the patches' own words
        let mut words = Vec::new();
        for k in 0..n {
            let f = i32_at(b, at + 4 * k)? as u32;
            words.extend(std::iter::repeat_n(f, words_per_patch));
        }
        at += 4 * n;
        let count = 1usize << (x_bits + z_bits);
        let mut patches = Vec::with_capacity(count);
        for _ in 0..count {
            let k = i32_at(b, at)?;
            at += 4;
            if k < 0 {
                patches.push(words.len());
                for w in 0..words_per_patch {
                    words.push(i32_at(b, at + 4 * w)? as u32);
                }
                at += 4 * words_per_patch;
            } else if (k as usize) < n {
                patches.push(k as usize * words_per_patch);
            } else {
                return Err(PathMapError::BadIndex(k));
            }
        }
        Ok(CellMap { x_bits, z_bits, patch_bits, scale, bpp_bits, patches, words, words_per_patch })
    }

    /// The pixel at the map position (x, z) (m; inside the map).
    pub fn pixel(&self, x: u32, z: u32) -> u32 {
        let side = 1u32 << self.patch_bits;
        let px = ((x & (side - 1)) as i32 >> self.scale) as u32;
        let pz = ((z & (side - 1)) as i32 >> self.scale) as u32;
        let word_bits = 5 - self.bpp_bits; // pixels per word, as bits
        let mut bit = (px & ((1 << word_bits) - 1)) << self.bpp_bits;
        let row_bits = self.patch_bits - self.scale; // a patch row's size as bits
        if row_bits < 5 {
            bit += (pz & ((1 << (5 - row_bits)) - 1)) << row_bits;
        }
        let patch = ((x as i32 >> self.patch_bits) + ((z as i32 >> self.patch_bits) << self.z_bits)) as usize;
        let word = (((pz << row_bits) + px) as i32 >> word_bits) as usize;
        let mask = (1u32 << (1 << self.bpp_bits)) - 1;
        // (a shift past the word wraps, as `SHL`/`SHR` do on the original's x86: Midway's maps get there)
        // (off the patches or its words, only in a spoilt file: as an empty pixel)
        let at = self.patches.get(patch).map_or(usize::MAX, |p| p + word);
        (self.words.get(at).copied().unwrap_or(0) & mask.wrapping_shl(bit)).wrapping_shr(bit)
    }

    /// Its side in m.
    pub fn size(&self) -> (u32, u32) {
        (1 << (self.x_bits + self.patch_bits), 1 << (self.z_bits + self.patch_bits))
    }
}

impl PathMap {
    pub fn parse(b: &[u8]) -> Result<PathMap, PathMapError> {
        let m = CellMap::parse(b)?;
        let (w, h) = m.size();
        let (width, height) = ((w >> m.scale) as usize, (h >> m.scale) as usize);
        let mut pixels = vec![0u8; width * height];
        for y in 0..height {
            for x in 0..width {
                pixels[y * width + x] = m.pixel((x as u32) << m.scale, (y as u32) << m.scale) as u8;
            }
        }
        Ok(PathMap { width, height, bpp: 1 << m.bpp_bits, pixels })
    }

    /// The value at pixel (x, y), 0 outside.
    pub fn get(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return 0;
        }
        self.pixels[y as usize * self.width + x as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fill_and_a_patch() {
        // 2×1 patches of 32×32, 1 bit: the first all 1 (fill 1), the second with pixel (1, 0) set
        let mut b = Vec::new();
        for v in [1i32, 0, 5, 0, 0, 2, 0, -1, 1, -1] {
            b.extend(v.to_le_bytes());
        }
        let mut words = vec![0u32; 32];
        words[0] = 0b10;
        for w in words {
            b.extend(w.to_le_bytes());
        }
        let m = PathMap::parse(&b).unwrap();
        assert_eq!((m.width, m.height), (64, 32));
        assert_eq!(m.get(5, 7), 1);
        assert_eq!((m.get(32, 0), m.get(33, 0), m.get(33, 1)), (0, 1, 0));
        let c = CellMap::parse(&b).unwrap();
        assert_eq!((c.pixel(5, 7), c.pixel(32, 0), c.pixel(33, 0), c.pixel(33, 1)), (1, 0, 1, 0));
    }

    #[test]
    fn a_coarse_level() {
        // 1×1 patch of 4 m with pixels of 2 m (scale 1): 2×2 pixels, 1 bit, pixel (1, 1) set
        let mut b = Vec::new();
        for v in [0i32, 0, 2, 1, 0, 0, -1, 0b1000, 0] {
            b.extend(v.to_le_bytes());
        }
        let c = CellMap::parse(&b).unwrap();
        assert_eq!(c.words_per_patch, 2);
        assert_eq!((c.pixel(0, 0), c.pixel(3, 1), c.pixel(1, 3), c.pixel(2, 3)), (0, 0, 0, 1));
    }
}
