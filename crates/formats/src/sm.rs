//! `.sm` (StandardMesh): visible LODs, collision meshes, shadow LODs.
//!
//! Layout, following ahrkylien's BF1942 Blender add-on (`src/standard_mesh.py`, MIT), all
//! little-endian, coordinates as stored (Refractor, Y up; the add-on swaps y/z for Blender):
//! ```text
//! u32 version (9, 10)   u8[4] 0   f32[6] bounding box (min xyz, max xyz)   [v10: u8 flag]
//! u32 collision count, each: u32 section size, then
//!     u8[4] FA C2 97 EB, u32 5, u32 nv, nv x (f32[3], u8[2], u8[2] pad),
//!     u32 nf, nf x (u16[3] vertex, u16 material), BSP tree
//! u32 LOD count, each: u32 materials; per material a header
//!     (u32 len, name, u8[12], u32 renderType 4 list / 5 strip, u32 vertexFormat,
//!      u32 vertex size 32/40, u32 vertices, u32 indices, u32 settings)
//!     then per material: vertices (pos f32[3], normal f32[3], uv f32[2], [lightmap uv f32[2]])
//!     and u16 indices
//! [u32 has shadow LODs, u32 count, LODs]  [u32 portal block size, block]
//! ```

use std::fmt;

#[derive(Debug)]
pub struct SmError(pub String);

impl fmt::Display for SmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sm: {}", self.0)
    }
}

impl std::error::Error for SmError {}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], SmError> {
        let s = self.b.get(self.p..self.p + n).ok_or_else(|| SmError(format!("truncated at {} (+{n})", self.p)))?;
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, SmError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, SmError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, SmError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, SmError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn vec3(&mut self) -> Result<[f32; 3], SmError> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    fn count(&mut self, max: usize, what: &str) -> Result<usize, SmError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(SmError(format!("{what} count {n} at {}", self.p - 4)));
        }
        Ok(n)
    }
    fn at_end(&self) -> bool {
        self.p >= self.b.len()
    }
}

#[derive(Debug, Clone)]
pub struct CollisionMesh {
    pub vertices: Vec<[f32; 3]>,
    /// each vertex's material id (the engine reads it from the vertex for terrain contact)
    pub vertex_materials: Vec<u16>,
    /// the two bytes after it (kept to check they are always zero)
    pub vertex_pad: Vec<u16>,
    /// vertex indices and the material id (an index into the material manager's table)
    pub faces: Vec<([u16; 3], u16)>,
    /// the faces as the BSP stores them, with their (outward) normals: what the engine tests
    /// segments against (normal, 4 unused bytes, 3 vertex indices, material)
    pub bsp_faces: Vec<BspFace>,
    /// the BSP tree, root first: which faces a segment can meet
    pub bsp_nodes: Vec<BspNode>,
}

/// A BSP node: a split plane (a point and its normal) with two children, or a leaf with faces
/// (indices into `bsp_faces`).
#[derive(Debug, Clone)]
pub struct BspNode {
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub faces: Vec<u32>,
    pub front: Option<u32>,
    pub back: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct BspFace {
    pub normal: [f32; 3],
    pub v: [u32; 3],
    pub material: u32,
}

#[derive(Debug, Clone)]
pub struct MeshMaterial {
    /// the shader/texture name, resolved through `.rs` files
    pub name: String,
    pub settings: u32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub lightmap_uvs: Vec<[f32; 2]>,
    /// triangles as a list (strips are expanded, alternating winding)
    pub triangles: Vec<[u16; 3]>,
}

#[derive(Debug, Clone)]
pub struct Lod {
    pub materials: Vec<MeshMaterial>,
}

#[derive(Debug, Clone)]
pub struct StandardMesh {
    pub version: u32,
    pub bounds: [[f32; 3]; 2],
    pub collision: Vec<CollisionMesh>,
    pub lods: Vec<Lod>,
    pub shadow_lods: Vec<Lod>,
    /// the portal mesh's polygons (doors and windows of a building, in its frame), if it has one
    pub portals: Vec<Vec<[f32; 3]>>,
    /// collision sections that were not the known collision mesh, skipped by their size
    pub unknown_sections: usize,
}

const COL_MAGIC: [u8; 4] = [0xFA, 0xC2, 0x97, 0xEB];
const MAX: usize = 1 << 22;

/// One node and its subtree, depth first: point and normal of the split
/// plane, its faces, then a flag and the subtree for the front and for the back child.
fn read_bsp_node(r: &mut Reader, depth: usize, out: &mut Vec<BspNode>) -> Result<u32, SmError> {
    if depth > 256 {
        return Err(SmError("BSP too deep".into()));
    }
    let point = r.vec3()?;
    let normal = r.vec3()?;
    let n = r.count(MAX, "bsp face")?;
    let mut faces = Vec::with_capacity(n);
    for _ in 0..n {
        faces.push(r.u32()?);
    }
    let me = out.len() as u32;
    out.push(BspNode { point, normal, faces, front: None, back: None });
    if r.u8()? == 1 {
        let c = read_bsp_node(r, depth + 1, out)?;
        out[me as usize].front = Some(c);
    }
    if r.u8()? == 1 {
        let c = read_bsp_node(r, depth + 1, out)?;
        out[me as usize].back = Some(c);
    }
    Ok(me)
}

fn read_collision(r: &mut Reader) -> Result<CollisionMesh, SmError> {
    let nv = r.count(MAX, "collision vertex")?;
    let mut vertices = Vec::with_capacity(nv);
    let (mut vertex_materials, mut vertex_pad) = (Vec::with_capacity(nv), Vec::with_capacity(nv));
    for _ in 0..nv {
        vertices.push(r.vec3()?);
        vertex_materials.push(r.u16()?);
        vertex_pad.push(r.u16()?);
    }
    let nf = r.count(MAX, "collision face")?;
    let mut faces = Vec::with_capacity(nf);
    for _ in 0..nf {
        faces.push(([r.u16()?, r.u16()?, r.u16()?], r.u16()?));
    }
    // BSP: u32, u32 node count, u32 n, n x (f32[3] normal, u32 0, u32[3] vertices, u32 material),
    // then the node tree
    r.take(8)?;
    let n = r.count(MAX, "bsp face")?;
    let mut bsp_faces = Vec::with_capacity(n);
    for _ in 0..n {
        let normal = r.vec3()?;
        r.take(4)?;
        let v = [r.u32()?, r.u32()?, r.u32()?];
        let material = r.u32()?;
        bsp_faces.push(BspFace { normal, v, material });
    }
    let mut bsp_nodes = Vec::new();
    read_bsp_node(r, 0, &mut bsp_nodes)?;
    Ok(CollisionMesh { vertices, vertex_materials, vertex_pad, faces, bsp_faces, bsp_nodes })
}

fn read_lod(r: &mut Reader) -> Result<Lod, SmError> {
    let nm = r.count(4096, "material")?;
    let mut heads = Vec::with_capacity(nm);
    for _ in 0..nm {
        let len = r.count(4096, "name length")?;
        let name: String = r.take(len)?.iter().map(|&b| b as char).collect();
        r.take(12)?;
        let render_type = r.u32()?;
        let _format = r.u32()?;
        let stride = r.u32()? as usize;
        let nv = r.count(MAX, "vertex")?;
        let ni = r.count(MAX, "index")?;
        let settings = r.u32()?;
        if stride < 32 {
            return Err(SmError(format!("vertex size {stride} in {name}")));
        }
        heads.push((name, render_type, stride, nv, ni, settings));
    }
    let mut materials = Vec::with_capacity(nm);
    for (name, render_type, stride, nv, ni, settings) in heads {
        let (mut positions, mut normals, mut uvs, mut lightmap_uvs) = (Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::with_capacity(nv), Vec::new());
        for _ in 0..nv {
            let v = r.take(stride)?;
            let f = |i: usize| f32::from_le_bytes(v[4 * i..4 * i + 4].try_into().unwrap());
            positions.push([f(0), f(1), f(2)]);
            normals.push([f(3), f(4), f(5)]);
            uvs.push([f(6), f(7)]);
            if stride >= 40 {
                lightmap_uvs.push([f(8), f(9)]);
            }
        }
        let mut idx = Vec::with_capacity(ni);
        for _ in 0..ni {
            idx.push(r.u16()?);
        }
        let triangles = if render_type == 5 {
            (0..ni.saturating_sub(2)).map(|i| if i % 2 == 0 { [idx[i], idx[i + 1], idx[i + 2]] } else { [idx[i + 1], idx[i], idx[i + 2]] }).collect()
        } else {
            idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
        };
        materials.push(MeshMaterial { name, settings, positions, normals, uvs, lightmap_uvs, triangles });
    }
    Ok(Lod { materials })
}

/// The portal block: u32 size of
/// what follows (0: none), u32 class id, u32 version, u32 count, each polygon a u32 count of
/// vertices and the vertices; then its BSP, skipped by the size.
fn read_portals(r: &mut Reader) -> Result<Vec<Vec<[f32; 3]>>, SmError> {
    let size = r.count(MAX, "portal block")?;
    if size == 0 {
        return Ok(Vec::new());
    }
    let end = r.p + size;
    r.take(8)?;
    let n = r.count(4096, "portal")?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let nv = r.count(4096, "portal vertex")?;
        out.push((0..nv).map(|_| r.vec3()).collect::<Result<Vec<_>, _>>()?);
    }
    r.p = end;
    Ok(out)
}

pub fn parse(bytes: &[u8]) -> Result<StandardMesh, SmError> {
    let mut r = Reader { b: bytes, p: 0 };
    let version = r.u32()?;
    r.take(4)?;
    let a = r.vec3()?;
    let b = r.vec3()?;
    if version == 10 {
        r.u8()?;
    } else if version != 9 {
        return Err(SmError(format!("version {version}")));
    }
    let mut collision = Vec::new();
    let mut unknown_sections = 0;
    let ncol = r.count(64, "collision mesh")?;
    for _ in 0..ncol {
        let size = r.count(bytes.len(), "section size")?;
        let start = r.p;
        let end = start + size;
        if r.b.get(start..start + 4) == Some(&COL_MAGIC) && r.b.get(start + 4..start + 8) == Some(&5u32.to_le_bytes()) {
            r.take(8)?;
            collision.push(read_collision(&mut r)?);
        } else {
            unknown_sections += 1;
        }
        if r.p != end {
            // trust the section size, like the game must
            r.p = end;
        }
    }
    let nlod = r.count(64, "LOD")?;
    let mut lods = Vec::with_capacity(nlod);
    for _ in 0..nlod {
        lods.push(read_lod(&mut r)?);
    }
    let mut shadow_lods = Vec::new();
    if !r.at_end() && r.u32()? == 1 {
        let n = r.count(64, "shadow LOD")?;
        for _ in 0..n {
            shadow_lods.push(read_lod(&mut r)?);
        }
    }
    let portals = if r.at_end() { Vec::new() } else { read_portals(&mut r)? };
    Ok(StandardMesh { version, bounds: [a, b], collision, lods, shadow_lods, unknown_sections, portals })
}
