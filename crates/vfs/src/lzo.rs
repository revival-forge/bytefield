//! LZO1X decompression, the codec of compressed `.rfa` archives.
//!
//! A safe port of the decoder in minilzo (`lzo1x_d.ch`, GPL), the one Refractor links. The
//! stream is a sequence of literal runs and back-references. Each instruction byte says which
//! kind comes next and how long it is; long lengths go on in extra zero bytes worth 255 each.
//! The low two bits of a match's last offset byte say how many literals (0-3) follow it.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum LzoError {
    InputOverrun,
    OutputOverrun,
    LookBehindOverrun,
    /// The stream ended (end marker) before filling the expected output.
    ShortOutput { got: usize, expected: usize },
}

impl fmt::Display for LzoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LZO1X: {self:?}")
    }
}

impl std::error::Error for LzoError {}

/// Cursor over the compressed input, with bounds checks instead of minilzo's raw pointers.
struct Input<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Input<'_> {
    fn byte(&mut self) -> Result<usize, LzoError> {
        let b = *self.data.get(self.pos).ok_or(LzoError::InputOverrun)?;
        self.pos += 1;
        Ok(b as usize)
    }

    /// A length that continues in zero bytes: each zero adds 255, the first non-zero ends it.
    fn long_length(&mut self, base: usize) -> Result<usize, LzoError> {
        let mut t = 0;
        loop {
            match self.byte()? {
                0 => t += 255,
                b => return Ok(t + base + b),
            }
        }
    }

    fn bytes(&mut self, n: usize) -> Result<&[u8], LzoError> {
        let s = self.data.get(self.pos..self.pos + n).ok_or(LzoError::InputOverrun)?;
        self.pos += n;
        Ok(s)
    }
}

fn copy_match(out: &mut Vec<u8>, distance: usize, len: usize, cap: usize) -> Result<(), LzoError> {
    if distance == 0 || distance > out.len() {
        return Err(LzoError::LookBehindOverrun);
    }
    if out.len() + len > cap {
        return Err(LzoError::OutputOverrun);
    }
    // byte by byte: the source may overlap what is being written (that is how runs repeat)
    let start = out.len() - distance;
    for i in 0..len {
        out.push(out[start + i]);
    }
    Ok(())
}

fn copy_literals(out: &mut Vec<u8>, inp: &mut Input, n: usize, cap: usize) -> Result<(), LzoError> {
    if out.len() + n > cap {
        return Err(LzoError::OutputOverrun);
    }
    out.extend_from_slice(inp.bytes(n)?);
    Ok(())
}

/// Decompress `src` into exactly `out_len` bytes.
pub fn decompress(src: &[u8], out_len: usize) -> Result<Vec<u8>, LzoError> {
    if out_len == 0 {
        // nothing to produce. Encoders disagree on what an empty stream looks like: lzokay
        // writes nothing, and the tool that packed Desert Combat Final wrote `11 11 00 00`
        return Ok(Vec::new());
    }
    // (the size is only as good as the file: `cap` stops at it, but nothing is asked of the allocator on its word alone)
    let mut out = Vec::with_capacity(out_len.min(1 << 24));
    let mut inp = Input { data: src, pos: 0 };

    // What to do next. These are the labels of minilzo's goto-based loop.
    enum Step {
        Instruction,      // top of the loop: a literal run or a match
        FirstLiteralRun,  // right after a literal run: a special short match is possible
        Match(usize),     // a match instruction byte
        MatchNext(usize), // 1-3 trailing literals, then a match
    }

    let mut step = Step::Instruction;
    if src.first().is_some_and(|&b| b > 17) {
        let t = inp.byte()? - 17;
        if t < 4 {
            step = Step::MatchNext(t);
        } else {
            copy_literals(&mut out, &mut inp, t, out_len)?;
            step = Step::FirstLiteralRun;
        }
    }

    loop {
        match step {
            Step::Instruction => {
                let t = inp.byte()?;
                if t >= 16 {
                    step = Step::Match(t);
                    continue;
                }
                let t = if t == 0 { inp.long_length(15)? } else { t };
                copy_literals(&mut out, &mut inp, t + 3, out_len)?;
                step = Step::FirstLiteralRun;
            }
            Step::FirstLiteralRun => {
                let t = inp.byte()?;
                if t >= 16 {
                    step = Step::Match(t);
                    continue;
                }
                // a 3-byte match, at least 2049 bytes back
                let distance = 1 + 0x0800 + (t >> 2) + (inp.byte()? << 2);
                copy_match(&mut out, distance, 3, out_len)?;
                step = trailing(&mut inp)?;
            }
            Step::Match(t) => {
                if t >= 64 {
                    // M2: length 3-8, distance up to 2048
                    let distance = 1 + ((t >> 2) & 7) + (inp.byte()? << 3);
                    copy_match(&mut out, distance, (t >> 5) - 1 + 2, out_len)?;
                } else if t >= 32 {
                    // M3: distance up to 16384
                    let mut len = t & 31;
                    if len == 0 {
                        len = inp.long_length(31)?;
                    }
                    let lo = inp.byte()?;
                    let hi = inp.byte()?;
                    let distance = 1 + (lo >> 2) + (hi << 6);
                    copy_match(&mut out, distance, len + 2, out_len)?;
                } else if t >= 16 {
                    // M4: distance 16385-49151, or the end-of-stream marker
                    let mut len = t & 7;
                    if len == 0 {
                        len = inp.long_length(7)?;
                    }
                    let lo = inp.byte()?;
                    let hi = inp.byte()?;
                    let distance = ((t & 8) << 11) + (lo >> 2) + (hi << 6);
                    if distance == 0 {
                        break; // end marker
                    }
                    copy_match(&mut out, distance + 0x4000, len + 2, out_len)?;
                } else {
                    // M1: a 2-byte match, only right after a match
                    let distance = 1 + (t >> 2) + (inp.byte()? << 2);
                    copy_match(&mut out, distance, 2, out_len)?;
                }
                step = trailing(&mut inp)?;
            }
            Step::MatchNext(t) => {
                copy_literals(&mut out, &mut inp, t, out_len)?;
                step = Step::Match(inp.byte()?);
            }
        }
    }

    /// After a match: the low 2 bits of the previous byte are a count of literals that follow.
    fn trailing(inp: &mut Input) -> Result<Step, LzoError> {
        let t = inp.data[inp.pos - 2] & 3;
        Ok(if t == 0 { Step::Instruction } else { Step::MatchNext(t as usize) })
    }

    if out.len() != out_len {
        return Err(LzoError::ShortOutput { got: out.len(), expected: out_len });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let packed = lzokay_native::compress(data).expect("compress");
        assert_eq!(decompress(&packed, data.len()).expect("decompress"), data);
    }

    #[test]
    fn roundtrips() {
        roundtrip(b"");
        roundtrip(b"a");
        roundtrip(b"ObjectTemplate.create PlayerControlObject XWing\r\n");
        roundtrip(&b"ObjectTemplate.setPosition 0/1/2\r\n".repeat(500)); // short matches
        roundtrip(&vec![0u8; 100_000]); // long runs: lengths in zero bytes
        // pseudo-random bytes with repeats far apart: M3/M4 distances
        let mut x: u32 = 1;
        let noise: Vec<u8> = (0..40_000).map(|_| { x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345); (x >> 16) as u8 }).collect();
        let mut far = noise.clone();
        far.extend_from_slice(&noise[..20_000]);
        roundtrip(&far);
    }

    #[test]
    fn corrupt_input_is_an_error_not_a_panic() {
        let packed = lzokay_native::compress(&b"hello hello hello hello".repeat(10)).unwrap();
        for cut in 0..packed.len() {
            let _ = decompress(&packed[..cut], 230);
        }
        assert!(decompress(&[0x00, 0x00], 10).is_err());
        assert!(decompress(&packed, 5).is_err());
    }
}
