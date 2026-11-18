//! Truevision TGA, the kinds the game's HUD and menus use: true colour (type 2) and its
//! run-length form (type 10), 24 or 32 bits per pixel, either origin; grey (type 3, and 11 run-length, 8 bits: some
//! statics' lightmaps, GC_Podrace's `pod_finishlinetower`); and the statics' lightmaps'
//! indices (type 1, 8 bits, [`indexed`]).

use crate::image::Image;

#[derive(Debug, PartialEq, Eq)]
pub enum TgaError {
    Short,
    Unsupported(u8, u8),
}

/// Decodes a TGA into RGBA (no alpha: opaque), top row first.
pub fn decode(b: &[u8]) -> Result<Image, TgaError> {
    if b.len() < 18 {
        return Err(TgaError::Short);
    }
    let (id_len, cmap_type, kind) = (b[0] as usize, b[1], b[2]);
    let cmap_len = u16::from_le_bytes([b[5], b[6]]) as usize;
    let cmap_bits = b[7] as usize;
    let width = u16::from_le_bytes([b[12], b[13]]) as usize;
    let height = u16::from_le_bytes([b[14], b[15]]) as usize;
    let (bits, desc) = (b[16], b[17]);
    let grey = matches!(kind, 3 | 11);
    if !(matches!(kind, 2 | 10) && matches!(bits, 24 | 32) || grey && bits == 8) {
        return Err(TgaError::Unsupported(kind, bits));
    }
    let px = bits as usize / 8;
    let mut at = 18 + id_len + if cmap_type == 1 { cmap_len * cmap_bits.div_ceil(8) } else { 0 };
    let n = width * height;
    // (317, the fuzz) the least data that many pixels take: one per pixel raw, a packet of 128 run-length; a header that asks
    // for more than the file holds is short before 17 GB are reserved
    let raw = matches!(kind, 2 | 3);
    let least = if raw { n * px } else { n.div_ceil(128) * (1 + px) };
    if b.len().saturating_sub(at) < least {
        return Err(TgaError::Short);
    }
    let mut pixels: Vec<[u8; 4]> = Vec::with_capacity(n);
    let read = |at: usize| -> Result<[u8; 4], TgaError> {
        let p = b.get(at..at + px).ok_or(TgaError::Short)?;
        Ok(if px == 1 { [p[0], p[0], p[0], 255] } else { [p[2], p[1], p[0], if px == 4 { p[3] } else { 255 }] })
    };
    while pixels.len() < n {
        if raw {
            pixels.push(read(at)?);
            at += px;
            continue;
        }
        let head = *b.get(at).ok_or(TgaError::Short)?;
        at += 1;
        let count = (head & 0x7f) as usize + 1;
        if head & 0x80 != 0 {
            let p = read(at)?;
            at += px;
            pixels.extend(std::iter::repeat_n(p, count));
        } else {
            for _ in 0..count {
                pixels.push(read(at)?);
                at += px;
            }
        }
    }
    pixels.truncate(n);
    // bit 5 of the descriptor: the first row is the top one; otherwise the bottom one
    let top_first = desc & 0x20 != 0;
    let mut img = Image::new(width, height);
    for y in 0..height {
        let src = if top_first { y } else { height - 1 - y };
        for x in 0..width {
            img.rgba[(y * width + x) * 4..][..4].copy_from_slice(&pixels[src * width + x]);
        }
    }
    Ok(img)
}

/// A colour-mapped TGA's indices (type 1, 8 bits; its own map skipped), top row first: the statics'
/// lightmaps (`ObjectLightmaps/*.tga`), whose index is the level's palette's (`ObjectLightmaps/Palette.pal`,
/// [`riff_palette`]; their own is grey). Returns the width, the height and the indices.
pub fn indexed(b: &[u8]) -> Result<(usize, usize, Vec<u8>), TgaError> {
    if b.len() < 18 {
        return Err(TgaError::Short);
    }
    let (id_len, cmap_type, kind) = (b[0] as usize, b[1], b[2]);
    let cmap_len = u16::from_le_bytes([b[5], b[6]]) as usize;
    let cmap_bits = b[7] as usize;
    let width = u16::from_le_bytes([b[12], b[13]]) as usize;
    let height = u16::from_le_bytes([b[14], b[15]]) as usize;
    let (bits, desc) = (b[16], b[17]);
    if kind != 1 || bits != 8 {
        return Err(TgaError::Unsupported(kind, bits));
    }
    let at = 18 + id_len + if cmap_type == 1 { cmap_len * cmap_bits.div_ceil(8) } else { 0 };
    let px = b.get(at..at + width * height).ok_or(TgaError::Short)?;
    let top_first = desc & 0x20 != 0;
    let rows = (0..height).flat_map(|y| {
        let src = if top_first { y } else { height - 1 - y };
        &px[src * width..(src + 1) * width]
    });
    Ok((width, height, rows.copied().collect()))
}

/// A Windows palette (`RIFF` … `PAL data`: version, count, then red, green, blue and a flag per entry).
pub fn riff_palette(b: &[u8]) -> Option<Vec<[u8; 3]>> {
    if b.get(0..4)? != b"RIFF" || b.get(8..16)? != b"PAL data" {
        return None;
    }
    let n = u16::from_le_bytes([*b.get(22)?, *b.get(23)?]) as usize;
    let e = b.get(24..24 + 4 * n)?;
    Some(e.chunks_exact(4).map(|c| [c[0], c[1], c[2]]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, w: u16, h: u16, bits: u8, desc: u8) -> Vec<u8> {
        let mut b = vec![0u8; 18];
        b[2] = kind;
        b[12..14].copy_from_slice(&w.to_le_bytes());
        b[14..16].copy_from_slice(&h.to_le_bytes());
        b[16] = bits;
        b[17] = desc;
        b
    }

    /// (317, the fuzz) a header of 65535 × 65535 with a few bytes behind it: short, without reserving 17 GB
    #[test]
    fn a_header_bigger_than_the_file_is_short() {
        for (kind, bits) in [(2, 32), (10, 32), (3, 8), (11, 8)] {
            let mut b = header(kind, 65535, 65535, bits, 0);
            b.extend([1, 2, 3, 4, 5]);
            assert!(matches!(decode(&b), Err(TgaError::Short)));
        }
    }

    #[test]
    fn plain_bottom_up() {
        // 1×2, 24 bits, bottom row first: blue then red (BGR)
        let mut b = header(2, 1, 2, 24, 0);
        b.extend([255, 0, 0, 0, 0, 255]);
        let i = decode(&b).unwrap();
        assert_eq!(i.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(i.get(0, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn run_length() {
        // 3×1, 32 bits, top first: a run of two, then one raw
        let mut b = header(10, 3, 1, 32, 0x28);
        b.extend([0x81, 1, 2, 3, 4, 0x00, 5, 6, 7, 8]);
        let i = decode(&b).unwrap();
        assert_eq!(i.get(1, 0), [3, 2, 1, 4]);
        assert_eq!(i.get(2, 0), [7, 6, 5, 8]);
    }

    /// GC_Podrace's `pod_finishlinetower` lightmaps: 8-bit grey, plain and run-length
    #[test]
    fn grey() {
        let mut b = header(3, 2, 1, 8, 0x20);
        b.extend([10, 200]);
        let i = decode(&b).unwrap();
        assert_eq!((i.get(0, 0), i.get(1, 0)), ([10, 10, 10, 255], [200, 200, 200, 255]));
        let mut b = header(11, 3, 1, 8, 0x20);
        b.extend([0x82, 42]);
        assert_eq!(decode(&b).unwrap().get(2, 0), [42, 42, 42, 255]);
        assert_eq!(decode(&header(3, 1, 1, 16, 0)), Err(TgaError::Unsupported(3, 16)));
    }

    #[test]
    fn indexed_bottom_up() {
        // 2×2, a map of two grey entries skipped, bottom row first
        let mut b = header(1, 2, 2, 8, 0);
        b[1] = 1;
        b[5..7].copy_from_slice(&2u16.to_le_bytes());
        b[7] = 24;
        b.extend([0, 0, 0, 9, 9, 9]);
        b.extend([1, 2, 3, 4]);
        assert_eq!(indexed(&b).unwrap(), (2, 2, vec![3, 4, 1, 2]));
        assert_eq!(indexed(&header(2, 1, 1, 24, 0)), Err(TgaError::Unsupported(2, 24)));
    }

    #[test]
    fn a_riff_palette() {
        let mut b = b"RIFF\x10\x00\x00\x00PAL data\x0c\x00\x00\x00".to_vec();
        b.extend([0, 3, 2, 0]);
        b.extend([34, 34, 40, 0, 240, 228, 186, 0]);
        assert_eq!(riff_palette(&b), Some(vec![[34, 34, 40], [240, 228, 186]]));
        assert_eq!(riff_palette(b"RIFF"), None);
    }
}
