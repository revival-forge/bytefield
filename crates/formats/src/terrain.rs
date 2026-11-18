//! Terrain: the heightmap and the parameters of a level's `PatchTerrain` geometry template.
//!
//! `Init/Terrain.con` of every level:
//! ```text
//! GeometryTemplate.create PatchTerrain patchGeometry
//! GeometryTemplate.file bf1942\levels\berlin\Heightmap      -> Heightmap.raw, u16 LE, square
//! GeometryTemplate.worldSize 2048                           -> metres per side
//! GeometryTemplate.yScale 0.6
//! GeometryTemplate.waterLevel 35
//! GeometryTemplate.texBaseName bf1942\levels\berlin\Textures\Tx   -> TxXXxYY.dds colour tiles
//! ```
//! Height in metres = raw / 65536 * 256 * yScale; node (c, r) sits at (c, r) * worldSize / size.
//! Both matched against the original game's terrain.

#[derive(Debug, Clone)]
pub struct Heightmap {
    pub size: usize,
    pub raw: Vec<u16>,
}

impl Heightmap {
    pub fn from_raw(bytes: &[u8]) -> Option<Heightmap> {
        let n = bytes.len() / 2;
        let size = (n as f64).sqrt() as usize;
        if size * size != n || size == 0 {
            return None;
        }
        Some(Heightmap { size, raw: bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() })
    }

    /// The raw sample at row `r`, column `c` (file order).
    pub fn at(&self, c: usize, r: usize) -> u16 {
        self.raw[r * self.size + c]
    }
}

#[derive(Debug, Clone)]
pub struct TerrainParams {
    pub heightmap: String,
    pub world_size: f64,
    pub y_scale: f64,
    pub water_level: f64,
    pub tex_base: String,
    /// `detailTexName`: the texture the original multiplies over the colour tiles (×2)
    pub detail_tex: String,
    /// `texOffsetX/Y`: where the colour tiles start, in tiles (0 in most levels; Wake 2/2: 2 + 4 + 2
    /// tiles of 256 m). Tobruk's 4/-10 is not understood; the tile checks leave them out.
    pub tex_offset: [f64; 2],
    /// `seaFloorLevel` (engine default 5): the terrain's height query answers it where
    /// the terrain's height comes out exactly 0
    pub sea_floor: f64,
}

impl TerrainParams {
    /// raw · 256 · yScale / 65536: checked against the original game's terrain.
    pub fn height_m(&self, raw: u16) -> f64 {
        raw as f64 / 65536.0 * 256.0 * self.y_scale
    }
}
