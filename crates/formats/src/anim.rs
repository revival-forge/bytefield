//! Skeletons (`.ske`), bone animations (`.baf`) and skins (`.skn`), from `animations.rfa`
//!
//! ```text
//! ske: u32 version (1), u32 bones; per bone: u16 len, name (len bytes, NUL included),
//!       i16 parent (-1: the root), f32[12]: a 3x4 matrix by rows (rotation, translation in the
//!       4th column), relative to the parent
//! baf: u32 version (3), u16 bones, their names (u16 len + name), u32 frames, u8 precision;
//!       per bone: u16 words in all, then 7 channels (quaternion x y z w, translation x y z),
//!       each u16 n + n u16 words
//! skn: u32 version (1), u32 vertices; per vertex: f32[3] its place in the bind pose, u8 n,
//!       n × (u16 bone, f32 weight, f32[3] the place in that bone's frame); then u16 bones and
//!       their names (u16 len + name), and (most) the offset of that table in ASCII and a newline
//! ```
//! A channel is a run of blocks: a header word (low byte: bit 7 set = one value repeated, bits
//! 0-6 = how many frames; high byte: words from this header to the next one) and the block's
//! values as i16 (one if repeated, one per frame if not); a value is i16 / (2^precision − 1).

use std::fmt;

#[derive(Debug)]
pub struct AnimError(pub String);

impl fmt::Display for AnimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "anim: {}", self.0)
    }
}

impl std::error::Error for AnimError {}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], AnimError> {
        let s = self.b.get(self.p..self.p + n).ok_or_else(|| AnimError(format!("truncated at {} (+{n})", self.p)))?;
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, AnimError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, AnimError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, AnimError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn f32(&mut self) -> Result<f32, AnimError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn name(&mut self) -> Result<String, AnimError> {
        let n = self.u16()? as usize;
        let s = self.take(n)?;
        let s = s.split(|&c| c == 0).next().unwrap_or(&[]);
        Ok(String::from_utf8_lossy(s).into_owned())
    }
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    /// its parent's index (None: the root)
    pub parent: Option<usize>,
    /// rotation rows and the translation, relative to the parent
    pub rows: [[f32; 3]; 3],
    pub pos: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
}

impl Skeleton {
    pub fn parse(b: &[u8]) -> Result<Skeleton, AnimError> {
        let mut r = Reader { b, p: 0 };
        let version = r.u32()?;
        if version != 1 {
            return Err(AnimError(format!("ske version {version}")));
        }
        let n = r.u32()? as usize;
        // (a count is only as good as the file: each takes at least a byte)
        let mut bones = Vec::with_capacity(n.min(b.len()));
        for _ in 0..n {
            let name = r.name()?;
            let parent = r.u16()? as i16;
            let mut m = [0f32; 12];
            for v in m.iter_mut() {
                *v = r.f32()?;
            }
            bones.push(Bone {
                name,
                parent: (parent >= 0).then_some(parent as usize),
                rows: [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]],
                pos: [m[3], m[7], m[11]],
            });
        }
        if r.p != b.len() {
            return Err(AnimError(format!("{} bytes left", b.len() - r.p)));
        }
        Ok(Skeleton { bones })
    }

    /// The bone of this name (as `BoneManager`: case does not matter).
    pub fn index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone)]
pub struct BoneAnimation {
    pub bones: Vec<String>,
    pub frames: u32,
    pub precision: u8,
    /// per bone, its 7 channels' words
    pub channels: Vec<[Vec<u16>; 7]>,
}

impl BoneAnimation {
    pub fn parse(b: &[u8]) -> Result<BoneAnimation, AnimError> {
        let mut r = Reader { b, p: 0 };
        let version = r.u32()?;
        if version != 3 {
            return Err(AnimError(format!("baf version {version}")));
        }
        let n = r.u16()? as usize;
        let mut bones = Vec::with_capacity(n);
        for _ in 0..n {
            bones.push(r.name()?);
        }
        let frames = r.u32()?;
        let precision = r.u8()?;
        let mut channels = Vec::with_capacity(n);
        for _ in 0..n {
            let total = r.u16()? as usize;
            let mut ch: [Vec<u16>; 7] = Default::default();
            let mut sum = 0;
            for c in ch.iter_mut() {
                let k = r.u16()? as usize;
                for _ in 0..k {
                    c.push(r.u16()?);
                }
                sum += k;
            }
            if sum != total {
                return Err(AnimError(format!("bone channels {sum} words, header {total}")));
            }
            channels.push(ch);
        }
        if r.p != b.len() {
            return Err(AnimError(format!("{} bytes left", b.len() - r.p)));
        }
        Ok(BoneAnimation { bones, frames, precision, channels })
    }

    /// The value of a bone's channel at a frame.
    pub fn value(&self, bone: usize, frame: usize, channel: usize) -> f32 {
        let w = &self.channels[bone][channel];
        let scale = 1.0 / (((1u32 << self.precision) - 1) as f32);
        let mut at = 0usize;
        let mut f = frame;
        let header = |at: usize| w.get(at).copied().unwrap_or(0);
        let mut h = header(at);
        while f + 1 > (h & 0x7f) as usize && (h & 0x7f) != 0 {
            f -= (h & 0x7f) as usize;
            let next = (h >> 8) as usize;
            if next == 0 {
                break;
            }
            at += next;
            h = header(at);
        }
        let k = if h & 0x80 != 0 { at + 1 } else { at + 1 + f };
        w.get(k).copied().unwrap_or(0) as i16 as f32 * scale
    }

    /// A bone's rotation (quaternion x, y, z, w) and translation at a frame.
    pub fn pose(&self, bone: usize, frame: usize) -> ([f32; 4], [f32; 3]) {
        let v = |c| self.value(bone, frame, c);
        ([v(0), v(1), v(2), v(3)], [v(4), v(5), v(6)])
    }
}

/// One bone's share of a skinned vertex: where the vertex is in that bone's frame.
#[derive(Debug, Clone, Copy)]
pub struct Influence {
    /// an index in the skin's `bones`
    pub bone: u16,
    pub weight: f32,
    pub offset: [f32; 3],
}

/// How an animated mesh's vertices hang from a skeleton's bones (`GeometryTemplate.setSkin`):
/// one entry per vertex of the mesh, in its order.
#[derive(Debug, Clone)]
pub struct Skin {
    pub vertices: Vec<([f32; 3], Vec<Influence>)>,
    /// the bones it names (look them up in the skeleton by name)
    pub bones: Vec<String>,
}

impl Skin {
    pub fn parse(b: &[u8]) -> Result<Skin, AnimError> {
        let mut r = Reader { b, p: 0 };
        let version = r.u32()?;
        if version != 1 {
            return Err(AnimError(format!("skn version {version}")));
        }
        let n = r.u32()? as usize;
        let mut vertices = Vec::with_capacity(n.min(b.len()));
        let v3 = |r: &mut Reader| -> Result<[f32; 3], AnimError> { Ok([r.f32()?, r.f32()?, r.f32()?]) };
        for _ in 0..n {
            let p = v3(&mut r)?;
            let k = r.u8()? as usize;
            let mut inf = Vec::with_capacity(k);
            for _ in 0..k {
                inf.push(Influence { bone: r.u16()?, weight: r.f32()?, offset: v3(&mut r)? });
            }
            vertices.push((p, inf));
        }
        let table = r.p;
        let k = r.u16()? as usize;
        let mut bones = Vec::with_capacity(k);
        for _ in 0..k {
            bones.push(r.name()?);
        }
        // the table's offset, as text (a check; some files end without it)
        let tail = String::from_utf8_lossy(&b[r.p..]);
        if !tail.trim().is_empty() && tail.trim().parse::<usize>().ok() != Some(table) {
            return Err(AnimError(format!("skn tail {tail:?}, the table at {table}")));
        }
        if vertices.iter().any(|(_, inf)| inf.iter().any(|i| i.bone as usize >= bones.len())) {
            return Err(AnimError("skn bone out of the table".into()));
        }
        Ok(Skin { vertices, bones })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A count that lies ends the read, without asking the allocator for what it says.
    #[test]
    fn a_count_that_lies_is_an_error() {
        let mut b = 1u32.to_le_bytes().to_vec();
        b.extend(0xffff_ffffu32.to_le_bytes());
        assert!(Skeleton::parse(&b).is_err());
        assert!(Skin::parse(&b).is_err());
    }

    /// The game's own files (extracted with `rfa extract … --ext ske,baf`), if there.
    fn files(ext: &str) -> Vec<std::path::PathBuf> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/extracted/anim");
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
                    out.push(p);
                }
            }
        }
        out
    }

    #[test]
    fn every_skin_reads() {
        for f in files("skn") {
            let s = Skin::parse(&std::fs::read(&f).unwrap()).unwrap_or_else(|e| panic!("{f:?}: {e}"));
            // the weights of a vertex add up to 1
            assert!(s.vertices.iter().all(|(_, inf)| inf.is_empty() || (inf.iter().map(|i| i.weight).sum::<f32>() - 1.0).abs() < 1e-3), "{f:?}");
        }
    }

    #[test]
    fn every_skeleton_reads() {
        for f in files("ske") {
            let s = Skeleton::parse(&std::fs::read(&f).unwrap()).unwrap_or_else(|e| panic!("{f:?}: {e}"));
            assert!(s.bones.iter().enumerate().all(|(i, b)| b.parent.is_none_or(|p| p < i)), "{f:?}");
        }
    }

    #[test]
    fn every_animation_reads_with_unit_rotations() {
        let fs = files("baf");
        for f in &fs {
            let a = BoneAnimation::parse(&std::fs::read(f).unwrap()).unwrap_or_else(|e| panic!("{f:?}: {e}"));
            for b in 0..a.bones.len() {
                for fr in 0..a.frames as usize {
                    // the few files of lower precision keep their rotations at 15 bits: as the
                    // server reads them (one scale for every channel) they come out longer
                    let (q, _) = a.pose(b, fr);
                    let l = (q.iter().map(|x| x * x).sum::<f32>()).sqrt() / (1u32 << (15 - a.precision)) as f32;
                    assert!((l - 1.0).abs() < 0.01, "{f:?} bone {b} frame {fr}: |q| {l}");
                }
            }
        }
    }
}
