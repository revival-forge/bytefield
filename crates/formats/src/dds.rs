//! DDS textures, decoded to RGBA8: the top mip level, or the file's whole mip chain.
//!
//! BF1942 uses DXT1/DXT3/DXT5 (terrain tiles, ingame maps, most textures) and a few
//! uncompressed 16/24/32-bit ones (described by bit masks in the header).

use std::fmt;

use crate::image::Image;

#[derive(Debug)]
pub struct DdsError(pub String);

impl fmt::Display for DdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DDS: {}", self.0)
    }
}

impl std::error::Error for DdsError {}

fn u32_at(b: &[u8], p: usize) -> Result<u32, DdsError> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| DdsError("truncated header".into()))
}

fn rgb565(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [((r << 3) | (r >> 2)) as u8, ((g << 2) | (g >> 4)) as u8, ((b << 3) | (b >> 2)) as u8]
}

/// The 4 colours of a DXT colour block; `dxt1` enables the 3-colour + transparent mode.
fn palette(c0: u16, c1: u16, dxt1: bool) -> [[u8; 4]; 4] {
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |wa: u16, wb: u16, d: u16| -> [u8; 4] {
        let c = |i: usize| ((a[i] as u16 * wa + b[i] as u16 * wb) / d) as u8;
        [c(0), c(1), c(2), 255]
    };
    if c0 > c1 || !dxt1 {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(1, 1, 2), [0, 0, 0, 0]]
    }
}

fn decode_block(kind: &[u8; 4], block: &[u8], out: &mut [[u8; 4]; 16]) {
    let color = if kind == b"DXT1" { block } else { &block[8..] };
    let c0 = u16::from_le_bytes([color[0], color[1]]);
    let c1 = u16::from_le_bytes([color[2], color[3]]);
    let pal = palette(c0, c1, kind == b"DXT1");
    let bits = u32::from_le_bytes(color[4..8].try_into().unwrap());
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
    }
    match kind {
        b"DXT3" => {
            for (i, px) in out.iter_mut().enumerate() {
                let a = (block[i / 2] >> (4 * (i % 2))) & 15;
                px[3] = a * 17;
            }
        }
        b"DXT5" => {
            let (a0, a1) = (block[0] as u16, block[1] as u16);
            let mut alphas = [a0, a1, 0, 0, 0, 0, 0, 0];
            if a0 > a1 {
                for i in 1..7 {
                    alphas[i + 1] = ((7 - i as u16) * a0 + i as u16 * a1) / 7;
                }
            } else {
                for i in 1..5 {
                    alphas[i + 1] = ((5 - i as u16) * a0 + i as u16 * a1) / 5;
                }
                alphas[6] = 0;
                alphas[7] = 255;
            }
            let mut bits = 0u64;
            for (i, &b) in block[2..8].iter().enumerate() {
                bits |= (b as u64) << (8 * i);
            }
            for (i, px) in out.iter_mut().enumerate() {
                px[3] = alphas[((bits >> (3 * i)) & 7) as usize] as u8;
            }
        }
        _ => {}
    }
}

/// Shift and width of a channel mask, to expand it to 8 bits.
fn channel(v: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 255;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).count_ones();
    let x = ((v & mask) >> shift) as u64;
    // (in 64 bits: a file can ask for a mask of all 32)
    let top = (1u64 << bits) - 1;
    ((x * 255 + top / 2) / top) as u8
}

pub fn decode(bytes: &[u8]) -> Result<Image, DdsError> {
    Ok(decode_mips(bytes)?.swap_remove(0))
}

/// The mip levels the file carries, the top one first (at least that one). The authored levels
/// matter for alpha-tested textures: a box filter thins the barbed wire away within a few metres.
pub fn decode_mips(bytes: &[u8]) -> Result<Vec<Image>, DdsError> {
    if bytes.get(..4) != Some(b"DDS ") {
        return Err(DdsError("no DDS magic".into()));
    }
    let height = u32_at(bytes, 12)? as usize;
    let width = u32_at(bytes, 16)? as usize;
    // DDSD_MIPMAPCOUNT
    let count = if u32_at(bytes, 8)? & 0x20000 != 0 { u32_at(bytes, 28)?.max(1) as usize } else { 1 };
    let pf_flags = u32_at(bytes, 80)?;
    let fourcc: [u8; 4] = bytes.get(84..88).and_then(|f| f.try_into().ok()).ok_or_else(|| DdsError("no header".into()))?;
    let mut data = bytes.get(128..).ok_or_else(|| DdsError("no header".into()))?;
    let mut out = Vec::new();
    let (mut w, mut h) = (width, height);
    for level in 0..count {
        let r = if pf_flags & 4 != 0 { decode_dxt(&fourcc, data, w, h) } else { decode_plain(bytes, pf_flags, data, w, h) };
        match r {
            Ok((img, used)) => {
                out.push(img);
                data = &data[used..];
            }
            // the lower levels are optional
            Err(e) if level == 0 => return Err(e),
            Err(_) => break,
        }
        if w == 1 && h == 1 {
            break;
        }
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    Ok(out)
}

/// A DXT file's blocks as they are, for the GPU (BC1/BC2/BC3): the levels it carries, the top
/// one first. Nothing for other formats or a truncated top level.
pub struct Compressed<'a> {
    pub fourcc: [u8; 4],
    pub width: usize,
    pub height: usize,
    pub levels: Vec<&'a [u8]>,
}

pub fn compressed(bytes: &[u8]) -> Option<Compressed<'_>> {
    if bytes.get(..4) != Some(b"DDS ") || u32_at(bytes, 80).ok()? & 4 == 0 {
        return None;
    }
    let height = u32_at(bytes, 12).ok()? as usize;
    let width = u32_at(bytes, 16).ok()? as usize;
    let count = if u32_at(bytes, 8).ok()? & 0x20000 != 0 { u32_at(bytes, 28).ok()?.max(1) as usize } else { 1 };
    let fourcc: [u8; 4] = bytes.get(84..88)?.try_into().ok()?;
    let block = match &fourcc {
        b"DXT1" => 8,
        b"DXT3" | b"DXT5" => 16,
        _ => return None,
    };
    let mut data = bytes.get(128..)?;
    let mut levels = Vec::new();
    let (mut w, mut h) = (width, height);
    for _ in 0..count {
        let Some(n) = w.div_ceil(4).checked_mul(h.div_ceil(4)).and_then(|n| n.checked_mul(block)) else { break };
        if data.len() < n {
            break;
        }
        levels.push(&data[..n]);
        data = &data[n..];
        if w == 1 && h == 1 {
            break;
        }
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    (!levels.is_empty()).then_some(Compressed { fourcc, width, height, levels })
}

fn decode_dxt(fourcc: &[u8; 4], data: &[u8], width: usize, height: usize) -> Result<(Image, usize), DdsError> {
    let block_size = match fourcc {
        b"DXT1" => 8,
        b"DXT3" | b"DXT5" => 16,
        other => return Err(DdsError(format!("unsupported fourcc {}", String::from_utf8_lossy(other)))),
    };
    let (bw, bh) = (width.div_ceil(4), height.div_ceil(4));
    // (what the header says is only as good as the file: nothing is made before the data is seen to be there)
    if bw.checked_mul(bh).and_then(|n| n.checked_mul(block_size)).is_none_or(|n| data.len() < n) {
        return Err(DdsError("truncated data".into()));
    }
    let mut img = Image::new(width, height);
    let mut px = [[0u8; 4]; 16];
    for by in 0..bh {
        for bx in 0..bw {
            let off = (by * bw + bx) * block_size;
            decode_block(fourcc, &data[off..off + block_size], &mut px);
            for i in 0..16 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < width && y < height {
                    img.put(x, y, px[i]);
                }
            }
        }
    }
    Ok((img, bw * bh * block_size))
}

fn decode_plain(bytes: &[u8], pf_flags: u32, data: &[u8], width: usize, height: usize) -> Result<(Image, usize), DdsError> {
    let bpp = u32_at(bytes, 88)? as usize;
    let masks = [u32_at(bytes, 92)?, u32_at(bytes, 96)?, u32_at(bytes, 100)?, if pf_flags & 1 != 0 { u32_at(bytes, 104)? } else { 0 }];
    let step = bpp / 8;
    if !(2..=4).contains(&step) || width.checked_mul(height).and_then(|n| n.checked_mul(step)).is_none_or(|n| data.len() < n) {
        return Err(DdsError(format!("unsupported uncompressed format ({bpp} bpp)")));
    }
    let mut img = Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let o = (y * width + x) * step;
            let mut v = 0u32;
            for i in 0..step {
                v |= (data[o + i] as u32) << (8 * i);
            }
            img.put(x, y, [channel(v, masks[0]), channel(v, masks[1]), channel(v, masks[2]), channel(v, masks[3])]);
        }
    }
    Ok((img, width * height * step))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_channel_of_all_32_bits() {
        assert_eq!(super::channel(u32::MAX, u32::MAX), 255);
    }

    use super::*;

    /// A file shorter than its header, or a header promising more than the file holds, is an error;
    /// not a panic, nor 300 GB asked of the allocator.
    #[test]
    fn a_short_or_lying_file_is_an_error() {
        let mut short = b"DDS ".to_vec();
        short.resize(110, 0);
        assert!(decode_mips(&short).is_err());
        assert!(compressed(&short).is_none());
        for flags in [4, 0x41] {
            let mut b = header(0xffff_fff0, 0xffff_fff0, flags, b"DXT1", 32, [0xff, 0xff00, 0xff_0000, 0]);
            b.extend([0u8; 64]);
            assert!(decode_mips(&b).is_err());
            assert!(compressed(&b).is_none());
        }
    }

    fn header(w: u32, h: u32, flags: u32, fourcc: &[u8; 4], bpp: u32, masks: [u32; 4]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"DDS ");
        b[12..16].copy_from_slice(&h.to_le_bytes());
        b[16..20].copy_from_slice(&w.to_le_bytes());
        b[80..84].copy_from_slice(&flags.to_le_bytes());
        b[84..88].copy_from_slice(fourcc);
        b[88..92].copy_from_slice(&bpp.to_le_bytes());
        for (i, m) in masks.iter().enumerate() {
            b[92 + 4 * i..96 + 4 * i].copy_from_slice(&m.to_le_bytes());
        }
        b
    }

    #[test]
    fn dxt1_solid_and_index() {
        // c0 = pure red, c1 = pure blue; indices: first pixel 0, the rest 1
        let mut b = header(4, 4, 4, b"DXT1", 0, [0; 4]);
        b.extend_from_slice(&0xF800u16.to_le_bytes());
        b.extend_from_slice(&0x001Fu16.to_le_bytes());
        b.extend_from_slice(&0x5555_5554u32.to_le_bytes());
        let img = decode(&b).unwrap();
        assert_eq!(img.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(1, 0), [0, 0, 255, 255]);
        assert_eq!(img.get(3, 3), [0, 0, 255, 255]);
    }

    #[test]
    fn mip_chain() {
        // 8x8 DXT1 with 4 levels (8, 4, 2, 1): one block each but the first; each a solid colour
        let mut b = header(8, 8, 4, b"DXT1", 0, [0; 4]);
        b[8..12].copy_from_slice(&0x20000u32.to_le_bytes());
        b[28..32].copy_from_slice(&4u32.to_le_bytes());
        for (blocks, c) in [(4, 0xF800u16), (1, 0x07E0), (1, 0x001F), (1, 0xFFFF)] {
            for _ in 0..blocks {
                b.extend_from_slice(&c.to_le_bytes());
                b.extend_from_slice(&c.to_le_bytes());
                b.extend_from_slice(&0u32.to_le_bytes());
            }
        }
        let mips = decode_mips(&b).unwrap();
        assert_eq!(mips.iter().map(|m| m.width).collect::<Vec<_>>(), [8, 4, 2, 1]);
        assert_eq!(mips[1].get(0, 0), [0, 255, 0, 255]);
        assert_eq!(mips[3].get(0, 0), [255, 255, 255, 255]);
        // a truncated chain keeps the levels it has
        assert_eq!(decode_mips(&b[..b.len() - 8]).unwrap().len(), 3);
    }

    #[test]
    fn uncompressed_565() {
        let mut b = header(1, 1, 0x40, b"\0\0\0\0", 16, [0xF800, 0x07E0, 0x001F, 0]);
        b.extend_from_slice(&0x07E0u16.to_le_bytes());
        assert_eq!(decode(&b).unwrap().get(0, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn compressed_levels() {
        // 8x8 DXT5 with 4 levels: 4 blocks, then 1 each
        let mut b = header(8, 8, 4, b"DXT5", 0, [0; 4]);
        b[8..12].copy_from_slice(&0x20000u32.to_le_bytes());
        b[28..32].copy_from_slice(&4u32.to_le_bytes());
        b.extend(std::iter::repeat_n(7u8, 16 * 7));
        let c = compressed(&b).unwrap();
        assert_eq!(c.levels.iter().map(|l| l.len()).collect::<Vec<_>>(), [64, 16, 16, 16]);
        assert!(compressed(&header(4, 4, 0x41, b"\0\0\0\0", 32, [0xff0000, 0xff00, 0xff, 0xff000000])).is_none());
    }
}
