//! How dark the original draws an object that stands in the terrain's baked shadow:
//! the client's terrain intensity on the level's `LightmapShadowBits.lsb` and colour tiles, the vote of five
//! points towards the sun round the object, and what a tree makes of it (its TEXTUREFACTOR).
//! Level 1: against the original's rendering of El Alamein, the 13 trees in view, to the byte but for the
//! original's `rand % 8`.

use crate::Level;
use bytefield_formats::lsb::Lsb;
use bytefield_vfs::Vfs;

/// Below this a sample counts as in shadow (the client's 100: a point in the baked shadow whose colour tiles are bright
/// counts as lit).
const DARK: f32 = 100.0;
/// What the terrain answers out of the shadow, out of the map, or too high above the ground.
pub const LIT: f32 = 255.0;
/// The share of the intensity a skinned mesh takes: 140/255 in the sun.
pub const SKIN_INTENSITY: f32 = 0.55;
/// The five points, in degrees about Y from the direction to the sun, at 1.1 × the object's radius.
const FAN: [f32; 5] = [-22.5, -13.5, -4.5, 4.5, 13.5];
const REACH: f32 = 1.1;

/// What the terrain needs to answer how lit a point is.
pub struct ShadowSampler {
    lsb: Lsb,
    /// the colour tiles' grid, which the `.lsb`'s tiles follow (z outer, x inner): columns, rows, a tile's side in metres
    /// and where the first starts (`texOffset`, as the terrain draws them)
    cols: usize,
    rows: usize,
    patch: f64,
    origin: [f64; 2],
    /// each patch's colour tile, its DDS (DXT1) as read, `None` where the level has none
    colour: Vec<Option<Vec<u8>>>,
    /// `Terrain.ShadowAmbient`, 0–255
    pub ambient: [f32; 3],
    /// `Terrain.ShadowSamplingCullY`: a point further than this above the ground is lit
    pub cull_y: f32,
    /// `Terrain.ShadowStaticObjectsCullBox` (5/5/5 by default, set by no level): a mesh whose box is larger than this on
    /// any axis is not darkened (as in the client)
    pub cull_box: [f32; 3],
    /// `Terrain.ShadowBorderFadeTime` (0.5 by default), in seconds: how long a mesh takes to go from one intensity to
    /// another
    pub fade_time: f32,
    /// `sky.sunLightDirectionVec` on the ground (x, z), normalised
    sun: [f32; 2],
}

fn global<'a>(db: &'a bytefield_con::templates::TemplateDb, name: &str) -> Option<&'a str> {
    db.globals.iter().rev().find(|c| c.command.eq_ignore_ascii_case(name)).and_then(|c| c.args.first()).map(String::as_str)
}

fn nums(s: Option<&str>, d: [f32; 3]) -> [f32; 3] {
    let v: Vec<f32> = s.map(|a| a.split('/').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    match v.len() {
        3 => [v[0], v[1], v[2]],
        1 => [v[0]; 3],
        _ => d,
    }
}

impl ShadowSampler {
    /// None without a `.lsb` (then everything is lit, as in the client without one).
    pub fn new(vfs: &Vfs, db: &bytefield_con::templates::TemplateDb, level: &Level) -> Option<ShadowSampler> {
        let base = &level.terrain.tex_base;
        let dir = &base[..base.rfind(['/', '\\']).map_or(0, |i| i + 1)];
        let lsb = Lsb::parse(&vfs.read(&format!("{dir}LightmapShadowBits.lsb")).ok()?)?;
        // (as many tiles as the colour tiles' grid has: 64 in El Alamein, 16 in Wake and Midway, 8 × 6 in Tobruk; the
        // grid square but for missing last rows)
        let last = (0..32).flat_map(|x| (0..32).map(move |z| (x, z))).filter(|(x, z)| vfs.exists(&format!("{base}{x:02}x{z:02}.dds"))).map(|(x, z)| x.max(z) + 1).max()?;
        let cols = crate::tile_grid(last, level.terrain.world_size, level.terrain.tex_offset[0]);
        let rows = lsb.tiles.len() / cols.max(1);
        if cols == 0 || rows == 0 || rows > cols || rows * cols != lsb.tiles.len() {
            return None;
        }
        let [ox, oz] = level.terrain.tex_offset;
        let patch = level.terrain.world_size / (cols as f64 + 2.0 * ox);
        // TODO: where the client starts the patches with texOffset (Wake 2/2, Midway 6/6, Tobruk 4/−10): taken from how the
        // terrain is drawn (`tile_uv` in the client), unchecked against a reference run.
        let origin = [ox * patch, oz.max(0.0) * patch];
        let colour = (0..cols * rows).map(|k| vfs.read(&format!("{base}{:02}x{:02}.dds", k % cols, k / cols)).ok()).collect();
        let sun = nums(global(db, "sky.sunLightDirectionVec"), [-0.5, 0.7, 0.5]);
        let l = sun[0].hypot(sun[2]);
        let sun = if l > 1e-6 { [sun[0] / l, sun[2] / l] } else { [0.0, 0.0] };
        let ambient = nums(global(db, "Terrain.ShadowAmbient"), [0.0; 3]);
        let cull_y = global(db, "Terrain.ShadowSamplingCullY").and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
        let cull_box = nums(global(db, "Terrain.ShadowStaticObjectsCullBox"), [5.0; 3]);
        let fade_time = global(db, "Terrain.ShadowBorderFadeTime").and_then(|v| v.trim().parse().ok()).unwrap_or(0.5);
        Some(ShadowSampler { lsb, cols, rows, patch, origin, colour, ambient, cull_y, cull_box, fade_time, sun })
    }

    /// The terrain's intensity at `pos` (the client's; the server's always says 255): 255 out of the map, further than
    /// `ShadowSamplingCullY` above the ground, or out of the baked shadow; in it, the brightness of the colour tile's DXT1
    /// block under the point (its 16 texels decoded, 0.3 R + 0.59 G + 0.11 B).
    pub fn intensity(&self, level: &Level, p: [f32; 3]) -> f32 {
        let (x, z) = (p[0] as f64 - self.origin[0], p[2] as f64 - self.origin[1]);
        if !(x >= 0.0 && z >= 0.0 && x < self.patch * self.cols as f64 && z < self.patch * self.rows as f64) {
            return LIT;
        }
        let (ix, iz) = ((x / self.patch) as usize, (z / self.patch) as usize);
        if p[1] - level.engine_height_and_normal(p[0], p[2]).0 > self.cull_y {
            return LIT;
        }
        let k = iz * self.cols + ix;
        let (Some(Some(tex)), Some(Some(tile))) = (self.colour.get(k), self.lsb.tiles.get(k)) else { return LIT };
        let (u, v) = ((x - ix as f64 * self.patch) / self.patch, (z - iz as f64 * self.patch) / self.patch);
        if !tile.bit(u, v) {
            return LIT;
        }
        dxt1_block_brightness(tex, u, v).unwrap_or(LIT)
    }

    /// The grid it reads (columns, rows, a tile's side in metres, where the first starts), to print.
    pub fn grid(&self) -> (usize, usize, f64, [f64; 2]) {
        (self.cols, self.rows, self.patch, self.origin)
    }

    /// The five points of the vote round `origin` ([`FAN`], at [`REACH`] × `radius` towards the sun).
    pub fn fan(&self, origin: [f32; 3], radius: f32) -> [[f32; 3]; 5] {
        let d = [self.sun[0] * radius * REACH, self.sun[1] * radius * REACH];
        FAN.map(|a| {
            let (s, c) = a.to_radians().sin_cos();
            [origin[0] + d[0] * c + d[1] * s, origin[1], origin[2] - d[0] * s + d[1] * c]
        })
    }

    /// How lit the client takes an object at `origin` of bounding radius `radius`: lit if its origin is; if
    /// not, five points on the ground towards the sun ([`FAN`]) vote, and with more of them dark than lit, their mean.
    pub fn object_intensity(&self, level: &Level, origin: [f32; 3], radius: f32) -> f32 {
        if self.intensity(level, origin) >= DARK {
            return LIT;
        }
        let (mut lit, mut dark, mut sum) = (0u32, 0u32, 0u32);
        for p in self.fan(origin, radius) {
            let i = self.intensity(level, p);
            if i >= DARK {
                lit += 1;
            } else {
                dark += 1;
                sum = (sum as f32 + i) as u32;
            }
        }
        if dark > lit { (sum / dark) as f32 } else { LIT }
    }
}

impl ShadowSampler {
    /// Whether a mesh of this box (min, max) takes the terrain's shadow: no larger than `cull_box` on any axis.
    /// Whether a point is near enough the ground to be sampled (`ShadowSamplingCullY`). Above it a skinned soldier takes no
    /// factor at all, TF 0xff and the sun whole, where one in the sun
    /// below it takes [`Self::skin_factor`] of 255.
    pub fn samples(&self, level: &Level, p: [f32; 3]) -> bool {
        p[1] - level.engine_height_and_normal(p[0], p[2]).0 <= self.cull_y
    }

    pub fn takes_shadow(&self, bounds: [[f32; 3]; 2]) -> bool {
        (0..3).all(|i| bounds[1][i] - bounds[0][i] <= self.cull_box[i])
    }

    /// How lit the client takes a mesh of this box (min, max) at `origin` (trees and standard meshes alike): the
    /// vote of [`Self::object_intensity`] with half its diagonal as the radius if it fits in the cull box, lit if not
    /// (none of Salerno's trees fits, and the original draws them all white; El Alamein's palms neither).
    pub fn mesh_intensity(&self, level: &Level, origin: [f32; 3], bounds: [[f32; 3]; 2]) -> f32 {
        if self.takes_shadow(bounds) { self.object_intensity(level, origin, radius(bounds)) } else { LIT }
    }

    /// The TEXTUREFACTOR (rgb) the client gives a standard or animated mesh (a vehicle's, a soldier's, a small static's)
    /// at an intensity: each channel the template's `shadowAmbient` (0: no data sets
    /// it) + `Terrain.ShadowAmbient` + the intensity, truncated, between 0 and 255; stage 1 multiplies the lit texture by it.
    pub fn mesh_factor(&self, intensity: f32) -> [u8; 3] {
        self.ambient.map(|a| (a + intensity).clamp(0.0, 255.0) as u8)
    }

    /// The TEXTUREFACTOR (rgb) of a skinned mesh (a soldier's body and arms, a tank's tracks: drawn with the skinning
    /// vertex shader) at an intensity: `Terrain.ShadowAmbient` + [`SKIN_INTENSITY`] × the intensity, to 255. Level 2,
    /// measured: with ShadowAmbient 100 those draws are 0xf0
    /// in the sun and 0x7f–0x84 in shade where a Kübelwagen is 0x9f; with ShadowAmbient 0, 0x8c in the sun and 0x12–0x1f
    /// in shade. The template's `shadowAmbient` (0 in all the data) is assumed added as in [`Self::mesh_factor`].
    // TODO: where the client scales it (an immediate 0.55 is only in a constructor, by 0.45: not seen used).
    pub fn skin_factor(&self, intensity: f32) -> [u8; 3] {
        self.ambient.map(|a| (a + SKIN_INTENSITY * intensity).clamp(0.0, 255.0) as u8)
    }

    /// The mesh's intensity fading: from `was` towards `intensity` by `dt / ShadowBorderFadeTime` (all
    /// at once with no time, or with more than the fade's).
    pub fn fade(&self, was: f32, intensity: f32, dt: f32) -> f32 {
        let k = if dt > 0.0 && self.fade_time > 0.0 { (dt.min(self.fade_time) / self.fade_time).min(1.0) } else { 1.0 };
        (1.0 - k) * was + k * intensity
    }
}

/// A placed object's `Object.geometry.color` (1 without it): 0.494118 on most of El Alamein's bushes, none on its palms.
pub fn instance_colour(inst: &bytefield_con::templates::Instance) -> [f32; 3] {
    nums(inst.props.iter().rev().find(|p| p.name.eq_ignore_ascii_case("geometry.color")).and_then(|p| p.args.first()).map(String::as_str), [1.0; 3])
}

/// An object's bounding radius from its mesh's box (min, max): half its diagonal.
/// TODO: the client's object's geometry radius is not read; half the diagonal and the farthest corner from
/// the origin both give the 13 trees of the reference run. Without `geometry.scale` either.
pub fn radius(bounds: [[f32; 3]; 2]) -> f32 {
    let [a, b] = bounds;
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt() / 2.0
}

/// A tree's TEXTUREFACTOR (rgb) for an intensity: each channel (intensity + ShadowAmbient) / 255
/// × its colour (`Object.geometry.color`, 1 by default; 0.494 on El Alamein's bushes, so they never reach white), to 255.
/// The original adds `rand % 8 − 4` to the colour's byte: here its mean, −0.5.
pub fn tree_factor(intensity: f32, ambient: [f32; 3], colour: [f32; 3]) -> [u8; 3] {
    std::array::from_fn(|c| {
        let base = ((colour[c] * 255.0).round() - 0.5).max(0.0);
        ((intensity + ambient[c]) / 255.0 * base).min(255.0).round() as u8
    })
}

/// The brightness of the DXT1 block of a DDS (its first mip) at (u, v) in 0..1: the block's 16 texels decoded as the
/// client does (always four colours: 5-6-5 shifted up, the two between in thirds), their 0.3 R + 0.59 G + 0.11 B averaged.
fn dxt1_block_brightness(dds: &[u8], u: f64, v: f64) -> Option<f32> {
    let le32 = |o: usize| dds.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
    if dds.get(0..4)? != b"DDS " || dds.get(84..88)? != b"DXT1" {
        return None;
    }
    let (h, w) = (le32(12)? as usize, le32(16)? as usize);
    let (bw, bh) = (w >> 2, h >> 2);
    let (bx, by) = ((bw as f64 * u) as usize, (bh as f64 * v) as usize);
    if bx >= bw || by >= bh {
        return None;
    }
    let o = 128 + (by * bw + bx) * 8;
    let b = dds.get(o..o + 8)?;
    let (c0, c1, idx) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]), u32::from_le_bytes([b[4], b[5], b[6], b[7]]));
    let rgb = |c: u16| [((c >> 11) << 3) as u8 as i32, (((c >> 5) & 63) << 2) as u8 as i32, ((c & 31) << 3) as u8 as i32];
    let (p0, p1) = (rgb(c0), rgb(c1));
    let pal = [p0, p1, std::array::from_fn(|i| (2 * p0[i] + p1[i]) / 3), std::array::from_fn(|i| (p0[i] + 2 * p1[i]) / 3)];
    let mut s = [0i32; 3];
    for k in 0..16 {
        let p = pal[(idx >> (2 * k) & 3) as usize];
        for c in 0..3 {
            s[c] += p[c];
        }
    }
    Some(s[0] as f32 * 0.0625 * 0.3 + s[1] as f32 * 0.0625 * 0.59 + s[2] as f32 * 0.0625 * 0.11)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bush_out_of_the_shadow_is_never_white() {
        // El Alamein: ShadowAmbient 50, the bushes' colour 0.494118 (the reference run's 0x97 ± the original's rand)
        let lit = tree_factor(LIT, [50.0; 3], [0.494118; 3]);
        assert!((148..=154).contains(&lit[0]), "{lit:?}");
        // a palm without a colour saturates
        assert_eq!(tree_factor(LIT, [50.0; 3], [1.0; 3]), [255; 3]);
        // the dark bush of the reference run (0x37) has an intensity of about 60
        let dark = tree_factor(60.0, [50.0; 3], [0.494118; 3]);
        assert!((50..=60).contains(&dark[0]), "{dark:?}");
    }

    #[test]
    fn a_block_is_averaged_in_four_colours() {
        // a 4 × 4 DXT1: c0 white, c1 black, all texels index 2 (two thirds white)
        let mut d = vec![0u8; 128];
        d[0..4].copy_from_slice(b"DDS ");
        d[12..16].copy_from_slice(&4u32.to_le_bytes());
        d[16..20].copy_from_slice(&4u32.to_le_bytes());
        d[84..88].copy_from_slice(b"DXT1");
        d.extend(0xffffu16.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.extend(0xaaaa_aaaau32.to_le_bytes());
        let b = dxt1_block_brightness(&d, 0.5, 0.5).unwrap();
        // 248 · 2 / 3 = 165, 252 · 2 / 3 = 168
        assert!((b - (165.0 * 0.3 + 168.0 * 0.59 + 165.0 * 0.11)).abs() < 1e-3, "{b}");
    }
}
