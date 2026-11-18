//! `.tm` (TreeMesh): trees. Ported from OpenField42's `tree_mesh.cpp` (GPL-3).
//!
//! ```text
//! u32 version 3, u32 subversion, u32 angle count, f32[6] mesh box, f32[6] sprite box
//! 4 block lists (branches, trunks, sprites, billboards): u32 n, n x (u32 first index,
//!     u32 triangles, u32 len, texture name)
//! u32 collision magic FA C2 97 EB (as in the `.sm` format) then the collision mesh, or another u32
//! u32 vertices, n x (f32[3] pos, f32[3] normal, u32 colour, f32[2] uv, f32[2] sprite offset)
//! u32 indices, n x u16 (triangle list)
//! ```
//! Sprites are quads that face the camera: `sprite offset` is the corner's offset in view
//! space from the vertex position.

use crate::sm::SmError;

#[derive(Debug, Clone)]
pub struct Block {
    pub first_index: u32,
    pub triangles: u32,
    pub texture: String,
}

#[derive(Debug, Clone)]
pub struct TreeMesh {
    pub version: u32,
    /// how many views of the tree its billboard texture holds side by side (`TreeMesh/Billboards/<file>.dds`; 8 in every
    /// file of the games)
    pub angles: u32,
    pub bounds: [[f32; 3]; 2],
    /// the second box of the header (min, max): the sprites'
    pub sprite_bounds: [[f32; 3]; 2],
    pub branches: Vec<Block>,
    pub trunks: Vec<Block>,
    pub sprites: Vec<Block>,
    pub billboards: Vec<Block>,
    pub has_collision: bool,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub sprite_offsets: Vec<[f32; 2]>,
    /// each vertex's colour, D3DCOLOR (B, G, R, A in memory): its light, baked
    pub colours: Vec<[u8; 4]>,
    pub indices: Vec<u16>,
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], SmError> {
        let s = self.b.get(self.p..self.p + n).ok_or_else(|| SmError(format!("tm truncated at {} (+{n})", self.p)))?;
        self.p += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, SmError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, SmError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn count(&mut self, max: usize) -> Result<usize, SmError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(SmError(format!("tm count {n} at {}", self.p - 4)));
        }
        Ok(n)
    }
    fn blocks(&mut self) -> Result<Vec<Block>, SmError> {
        let n = self.count(4096)?;
        (0..n)
            .map(|_| {
                let first_index = self.u32()?;
                let triangles = self.u32()?;
                let len = self.count(4096)?;
                let texture = self.take(len)?.iter().map(|&b| b as char).collect::<String>().replace('\\', "/");
                Ok(Block { first_index, triangles, texture })
            })
            .collect()
    }
    fn bsp(&mut self, depth: usize) -> Result<(), SmError> {
        if depth > 256 {
            return Err(SmError("tm BSP too deep".into()));
        }
        self.take(24)?;
        let n = self.count(1 << 22)?;
        self.take(4 * n)?;
        for _ in 0..2 {
            if self.take(1)?[0] == 1 {
                self.bsp(depth + 1)?;
            }
        }
        Ok(())
    }
}

pub fn parse(bytes: &[u8]) -> Result<TreeMesh, SmError> {
    let mut r = R { b: bytes, p: 0 };
    let version = r.u32()?;
    if version != 3 {
        return Err(SmError(format!("tm version {version}")));
    }
    let _subversion = r.u32()?;
    let angles = r.u32()?;
    let mut f6 = || -> Result<[[f32; 3]; 2], SmError> { Ok([[r.f32()?, r.f32()?, r.f32()?], [r.f32()?, r.f32()?, r.f32()?]]) };
    let bounds = f6()?;
    let sprite_bounds = f6()?;
    let (branches, trunks, sprites, billboards) = (r.blocks()?, r.blocks()?, r.blocks()?, r.blocks()?);
    let has_collision = r.b.get(r.p..r.p + 4) == Some(&[0xFA, 0xC2, 0x97, 0xEB]);
    r.take(4)?;
    if has_collision && r.u32()? == 5 {
        let nv = r.count(1 << 22)?;
        r.take(16 * nv)?;
        let nf = r.count(1 << 22)?;
        r.take(8 * nf)?;
        r.take(8)?;
        let n = r.count(1 << 22)?;
        r.take(32 * n)?;
        r.bsp(0)?;
    }
    let nv = r.count(1 << 22)?;
    let (mut positions, mut normals, mut uvs, mut sprite_offsets, mut colours) = (Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv));
    for _ in 0..nv {
        positions.push([r.f32()?, r.f32()?, r.f32()?]);
        normals.push([r.f32()?, r.f32()?, r.f32()?]);
        colours.push(r.take(4)?.try_into().unwrap());
        uvs.push([r.f32()?, r.f32()?]);
        sprite_offsets.push([r.f32()?, r.f32()?]);
    }
    let ni = r.count(1 << 22)?;
    let indices = (0..ni).map(|_| Ok(u16::from_le_bytes(r.take(2)?.try_into().unwrap()))).collect::<Result<_, SmError>>()?;
    Ok(TreeMesh { version, angles, bounds, sprite_bounds, branches, trunks, sprites, billboards, has_collision, positions, normals, uvs, sprite_offsets, colours, indices })
}
