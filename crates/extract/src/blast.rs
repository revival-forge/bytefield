//! PKWARE Data Compression Library "implode" decoder (the format Setup Factory 5/6 uses).
//!
//! A port of `contrib/blast/blast.c` from zlib, by Mark Adler (zlib licence: "This software is
//! provided 'as-is' … Permission is granted to anyone to use this software for any purpose").
//! The stream: a byte saying whether literals are Huffman-coded, a byte with log2(dictionary)-6,
//! then a bit per item: 0 = literal, 1 = length/distance pair; length 519 ends the stream.
//! Bits are read LSB first; Huffman codes are stored bit-inverted.

const MAXBITS: usize = 13;

struct Huffman {
    count: [u16; MAXBITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// Build a code from run-length-compacted bit lengths: each byte is (repeat-1)<<4 | length.
    fn new(rep: &[u8]) -> Huffman {
        let mut length = Vec::new();
        for &r in rep {
            for _ in 0..(r >> 4) + 1 {
                length.push((r & 15) as usize);
            }
        }
        let mut count = [0u16; MAXBITS + 1];
        for &l in &length {
            count[l] += 1;
        }
        let mut offs = [0usize; MAXBITS + 1];
        for len in 1..MAXBITS {
            offs[len + 1] = offs[len] + count[len] as usize;
        }
        let mut symbol = vec![0u16; length.len()];
        for (s, &l) in length.iter().enumerate() {
            if l != 0 {
                symbol[offs[l]] = s as u16;
                offs[l] += 1;
            }
        }
        Huffman { count, symbol }
    }
}

const LITLEN: &[u8] = &[
    11, 124, 8, 7, 28, 7, 188, 13, 76, 4, 10, 8, 12, 10, 12, 10, 8, 23, 8, 9, 7, 6, 7, 8, 7, 6, 55, 8, 23, 24, 12, 11,
    7, 9, 11, 12, 6, 7, 22, 5, 7, 24, 6, 11, 9, 6, 7, 22, 7, 11, 38, 7, 9, 8, 25, 11, 8, 11, 9, 12, 8, 12, 5, 38, 5,
    38, 5, 11, 7, 5, 6, 21, 6, 10, 53, 8, 7, 24, 10, 27, 44, 253, 253, 253, 252, 252, 252, 13, 12, 45, 12, 45, 12, 61,
    12, 45, 44, 173,
];
const LENLEN: &[u8] = &[2, 35, 36, 53, 38, 23];
const DISTLEN: &[u8] = &[2, 20, 53, 230, 247, 151, 248];
const BASE: [usize; 16] = [3, 2, 4, 5, 6, 7, 8, 9, 10, 12, 16, 24, 40, 72, 136, 264];
const EXTRA: [u32; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];

#[derive(Debug, PartialEq, Eq)]
pub enum BlastError {
    OutOfInput,
    BadLiteralFlag,
    BadDictionary,
    DistanceTooFar,
    BadCode,
}

struct Bits<'a> {
    input: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl Bits<'_> {
    fn bits(&mut self, need: u32) -> Result<u32, BlastError> {
        let mut val = self.buf;
        while self.cnt < need {
            let b = *self.input.get(self.pos).ok_or(BlastError::OutOfInput)?;
            self.pos += 1;
            val |= (b as u32) << self.cnt;
            self.cnt += 8;
        }
        self.buf = val >> need;
        self.cnt -= need;
        Ok(val & ((1 << need) - 1))
    }

    fn decode(&mut self, h: &Huffman) -> Result<usize, BlastError> {
        let mut bitbuf = self.buf;
        let mut left = self.cnt;
        let (mut code, mut first, mut index): (i32, i32, i32) = (0, 0, 0);
        let mut len = 1usize;
        loop {
            while left > 0 {
                left -= 1;
                code |= ((bitbuf & 1) ^ 1) as i32; // codes are stored inverted
                bitbuf >>= 1;
                let count = h.count[len] as i32;
                if code < first + count {
                    self.buf = bitbuf;
                    self.cnt = (self.cnt.wrapping_sub(len as u32)) & 7;
                    return Ok(h.symbol[(index + code - first) as usize] as usize);
                }
                index += count;
                first += count;
                first <<= 1;
                code <<= 1;
                len += 1;
            }
            let remaining = (MAXBITS + 1) - len;
            if remaining == 0 {
                return Err(BlastError::BadCode);
            }
            bitbuf = *self.input.get(self.pos).ok_or(BlastError::OutOfInput)? as u32;
            self.pos += 1;
            left = remaining.min(8) as u32;
        }
    }
}

/// Decompress one stream. Returns the output and how many input bytes it used, so streams
/// stored back to back can be walked without knowing their sizes.
pub fn explode(input: &[u8]) -> Result<(Vec<u8>, usize), BlastError> {
    thread_local! {
        static CODES: (Huffman, Huffman, Huffman) = (Huffman::new(LITLEN), Huffman::new(LENLEN), Huffman::new(DISTLEN));
    }
    CODES.with(|(litcode, lencode, distcode)| {
        let mut s = Bits { input, pos: 0, buf: 0, cnt: 0 };
        let lit = s.bits(8)?;
        if lit > 1 {
            return Err(BlastError::BadLiteralFlag);
        }
        let dict = s.bits(8)?;
        if !(4..=6).contains(&dict) {
            return Err(BlastError::BadDictionary);
        }
        let mut out: Vec<u8> = Vec::new();
        loop {
            if s.bits(1)? == 1 {
                let symbol = s.decode(lencode)?;
                let len = BASE[symbol] + s.bits(EXTRA[symbol])? as usize;
                if len == 519 {
                    break; // end code
                }
                let shift = if len == 2 { 2 } else { dict };
                let mut dist = s.decode(distcode)? << shift;
                dist += s.bits(shift)? as usize;
                dist += 1;
                if dist > out.len() {
                    return Err(BlastError::DistanceTooFar);
                }
                let start = out.len() - dist;
                for i in 0..len {
                    out.push(out[start + i]);
                }
            } else {
                let symbol = if lit == 1 { s.decode(litcode)? } else { s.bits(8)? as usize };
                out.push(symbol as u8);
            }
        }
        Ok((out, s.pos))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test vector from blast.c: decompresses to "AIAIAIAIAIAIA".
    #[test]
    fn blast_test_vector() {
        let input = [0x00, 0x04, 0x82, 0x24, 0x25, 0x8f, 0x80, 0x7f];
        let (out, used) = explode(&input).unwrap();
        assert_eq!(out, b"AIAIAIAIAIAIA");
        assert_eq!(used, input.len());
    }
}
