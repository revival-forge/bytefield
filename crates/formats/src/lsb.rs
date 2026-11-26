//! A level's `Textures/LightmapShadowBits.lsb`: the terrain's baked shadow, one tile per colour tile
//! (`TxXXxYY.dds`) of the terrain's patch grid, z outer and x inner (the client's patch index is `(z << shift) + x`).
//! Per tile: width and height (u32; 0 for a patch without one), then per row the number of runs minus one (u32 × height),
//! then the runs, row after row: u16, 13 bits of length and bit 13 = in shadow. El Alamein: 64 tiles of 1024² over its
//! 256 m colour tiles, 0.25 m a bit.

#[derive(Debug, Clone)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    /// every row's runs, one after another
    runs: Vec<u16>,
    /// where each row's runs start in `runs` (one more: the end)
    rows: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Lsb {
    /// in the file's order (patches z outer, x inner); `None` for a patch without one
    pub tiles: Vec<Option<Tile>>,
}

impl Lsb {
    pub fn parse(b: &[u8]) -> Option<Lsb> {
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
        let mut o = 0;
        let mut tiles = Vec::new();
        while o < b.len() {
            let (w, h) = (u32_at(o)?, u32_at(o + 4)?);
            o += 8;
            if w == 0 {
                tiles.push(None);
                continue;
            }
            let counts: Vec<u32> = (0..h as usize).map(|z| u32_at(o + 4 * z)).collect::<Option<_>>()?;
            o += 4 * h as usize;
            let mut runs = Vec::new();
            let mut rows = Vec::with_capacity(h as usize + 1);
            for c in counts {
                rows.push(runs.len() as u32);
                for _ in 0..=c {
                    runs.push(u16::from_le_bytes(b.get(o..o + 2)?.try_into().unwrap()));
                    o += 2;
                }
            }
            rows.push(runs.len() as u32);
            tiles.push(Some(Tile { width: w, height: h, runs, rows }));
        }
        Some(Lsb { tiles })
    }
}

impl Tile {
    /// The lightmap bit of a patch: u and v in 0..1 across the tile (x, z), each scaled and truncated to
    /// a column and a row; the runs are walked while their sum is below the column (a column on a run's end is that run's).
    pub fn bit(&self, u: f64, v: f64) -> bool {
        let (col, row) = ((self.width as f64 * u) as i64, (self.height as f64 * v) as i64);
        let Some(row) = usize::try_from(row).ok().filter(|&r| r < self.height as usize) else { return false };
        let runs = &self.runs[self.rows[row] as usize..self.rows[row + 1] as usize];
        let mut sum = 0i64;
        for (k, r) in runs.iter().enumerate() {
            sum += (r & 0x1fff) as i64;
            if sum >= col || k + 1 == runs.len() {
                return r >> 13 & 1 == 1;
            }
        }
        false
    }

    /// How many of each row's bits are in shadow over `side` equal spans of its columns, for every row: the share
    /// (0–1) for each, `side` × `height` rows, row after row. A column is the run's whose running sum first reaches it, as
    /// in [`Self::bit`].
    pub fn shadow_shares(&self, side: usize) -> Vec<f32> {
        let w = self.width as usize;
        let mut out = vec![0.0; side * self.height as usize];
        let per = side as f32 / w as f32;
        for row in 0..self.height as usize {
            let runs = &self.runs[self.rows[row] as usize..self.rows[row + 1] as usize];
            let line = &mut out[row * side..(row + 1) * side];
            let (mut col, mut sum) = (0usize, 0usize);
            for (k, r) in runs.iter().enumerate() {
                sum += (r & 0x1fff) as usize;
                let end = if k + 1 == runs.len() { w } else { (sum + 1).min(w) };
                if r >> 13 & 1 == 1 {
                    for c in col..end {
                        line[c * side / w] += per;
                    }
                }
                col = col.max(end);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_are_walked_while_their_sum_is_below_the_column() {
        // one tile of 8 × 2: row 0 = 3 lit, 5 in shadow; row 1 = 8 in shadow
        let mut b = Vec::new();
        for v in [8u32, 2, 1, 0] {
            b.extend(v.to_le_bytes());
        }
        for r in [3u16, 5 | 0x2000, 8 | 0x2000] {
            b.extend(r.to_le_bytes());
        }
        let lsb = Lsb::parse(&b).unwrap();
        let t = lsb.tiles[0].as_ref().unwrap();
        assert!(!t.bit(0.0, 0.0));
        // column 3 is the first run's end: still the first run (the client's `<`)
        assert!(!t.bit(3.0 / 8.0, 0.0));
        assert!(t.bit(4.0 / 8.0, 0.0));
        assert!(t.bit(0.0, 0.5));
        // in two spans: the first run's four columns lit, the rest in shadow
        assert_eq!(t.shadow_shares(2), vec![0.0, 1.0, 1.0, 1.0]);
    }
}
