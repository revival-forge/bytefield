//! Cover points: places along the walking mesh's edges next to something that stops bullets, each with the directions it hides a
//! crouched soldier from, worked out once per level (in the manner of the cover points of *Killzone 3*, Game AI Pro's tactical point
//! systems): the position queries ([`crate::bots::tactics`]) take them as candidates and turn down, without a ray, those that do
//! not hide from where the enemy is.
//!
//! The mesh's border edges (no neighbour) are sampled every [`SPACING_M`]; at each sample a fan of short rays ([`RAY_M`]) with
//! [`World::hidden`], crouched ([`CROUCH_M`]) and standing ([`STAND_M`]), in [`SECTORS`] directions of [`FAN`] rays each, says
//! which directions something hides it in. A sector counts as covered when most of its rays are stopped. A sample covered in no
//! direction (an open field, a cliff's edge) is no cover point. `crouch` holds the sectors that hide it crouched and `stand`
//! those that also hide it standing: a parapet (`crouch` but not `stand`) is where it shoots from standing; a wall (`stand`) is a
//! corner to lean out of. `roof` marks a point under something (a building's inside, where [`World::hidden`] does not see the
//! walls from within: the inner faces of a wall are its back).
//!
//! Only the level's static objects count (vehicles and what is laid in the round are not in it). The points depend only on the
//! level and the mesh, so they are cached like the mesh ([`CoverPoints::load_or_build`]), keyed by the mesh's fingerprint and
//! [`GENERATOR`]; the same level gives the same file.
use crate::World;
use crate::navmesh::{NONE, NavMesh};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The generator's version: part of the cache's key, raised when what it builds changes.
pub const GENERATOR: u32 = 1;
/// The file's format.
pub const FORMAT: u32 = 1;
/// Samples along the border edges this far apart, m.
pub const SPACING_M: f32 = 2.0;
/// Samples nearer than this to one already kept are dropped, m.
pub const MERGE_M: f32 = 1.0;
/// The rays' length, m: past the mesh's margin (half a metre from a wall) and a thick wall's face.
pub const RAY_M: f32 = 3.0;
/// The crouched body's height over the feet and the standing eye's, m (as [`crate::bots::tactics`]).
pub const CROUCH_M: f32 = 0.8;
pub const STAND_M: f32 = 1.6;
/// Directions around a point, each of `TAU / SECTORS`: sector k is centred on the angle k · 45° of (x, z) (`atan2(z, x)`).
pub const SECTORS: usize = 8;
/// Rays per sector, spread over it (at -15°, 0 and 15° of its centre); covered when at least [`FAN_COVERED`] are stopped.
pub const FAN: usize = 3;
pub const FAN_COVERED: usize = 2;
/// How high over the standing eye something must be to be a roof, m.
pub const ROOF_M: f32 = 8.0;
/// The side of the cells of the lookup grid, m.
pub const CELL_M: f32 = 16.0;

/// A cover point: where to stand (x, the floor's height, z), the polygon it is on, and the sectors it hides a crouched and a
/// standing soldier from (bit k: sector k, [`sector`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CoverPoint {
    pub pos: [f32; 3],
    pub poly: u32,
    pub crouch: u8,
    pub stand: u8,
    pub roof: bool,
}

impl CoverPoint {
    /// Whether it hides a crouched soldier from `from` (its sector).
    pub fn hides_crouched_from(&self, from: [f32; 3]) -> bool {
        self.crouch & (1 << sector(from[0] - self.pos[0], from[2] - self.pos[2])) != 0
    }
    /// Whether it also hides a standing soldier from `from`: it cannot shoot over it.
    pub fn hides_standing_from(&self, from: [f32; 3]) -> bool {
        self.stand & (1 << sector(from[0] - self.pos[0], from[2] - self.pos[2])) != 0
    }
}

/// The sector of a direction across (dx, dz).
pub fn sector(dx: f32, dz: f32) -> usize {
    let a = dz.atan2(dx) / (std::f32::consts::TAU / SECTORS as f32);
    (a.round() as i32).rem_euclid(SECTORS as i32) as usize
}

/// A level's cover points, sorted by cell of the lookup grid (and in each cell by the order they were made), with the grid.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoverPoints {
    pub format: u32,
    pub generator: u32,
    /// the mesh's fingerprint ([`NavMesh::fingerprint`]) it was made from
    pub mesh: u64,
    /// the bots' version that made it (for information: the points do not depend on it)
    pub bots: String,
    pub x0: f32,
    pub z0: f32,
    pub cols: u32,
    pub rows: u32,
    /// the first point of each cell (row by row), and the total last
    pub start: Vec<u32>,
    pub points: Vec<CoverPoint>,
}

/// Whether the cache was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cached {
    Read,
    Built,
    NotWritten,
}

impl CoverPoints {
    /// The cover points of `world` over `mesh`, on `threads` threads (the same points with any number).
    pub fn build(world: &World, mesh: &NavMesh, threads: usize) -> CoverPoints {
        let polys = mesh.polys.len();
        let threads = threads.max(1);
        let chunk = polys.div_ceil(threads).max(1);
        let parts: Vec<Vec<CoverPoint>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..threads).map(|t| s.spawn(move || (t * chunk..((t + 1) * chunk).min(polys)).flat_map(|p| samples(world, mesh, p as u32)).collect::<Vec<_>>())).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        // (merged in the polygons' order: the first of two near ones is kept)
        let mut kept: Vec<CoverPoint> = Vec::new();
        let mut near: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::new();
        let key = |p: [f32; 3]| ((p[0] / MERGE_M).floor() as i32, (p[2] / MERGE_M).floor() as i32);
        for c in parts.into_iter().flatten() {
            let (kx, kz) = key(c.pos);
            let clash = (kx - 1..=kx + 1).any(|x| (kz - 1..=kz + 1).any(|z| near.get(&(x, z)).is_some_and(|v| v.iter().any(|&i| {
                let o = kept[i as usize].pos;
                (o[0] - c.pos[0]).hypot(o[2] - c.pos[2]) < MERGE_M && (o[1] - c.pos[1]).abs() < 1.0
            }))));
            if !clash {
                near.entry((kx, kz)).or_default().push(kept.len() as u32);
                kept.push(c);
            }
        }
        Self::from_points(kept, mesh.fingerprint())
    }

    /// The grid over `points` (sorted stably by cell).
    pub fn from_points(mut points: Vec<CoverPoint>, mesh: u64) -> CoverPoints {
        let (mut x0, mut z0, mut x1, mut z1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &points {
            x0 = x0.min(p.pos[0]);
            z0 = z0.min(p.pos[2]);
            x1 = x1.max(p.pos[0]);
            z1 = z1.max(p.pos[2]);
        }
        if points.is_empty() {
            (x0, z0, x1, z1) = (0.0, 0.0, 0.0, 0.0);
        }
        let (x0, z0) = ((x0 / CELL_M).floor() * CELL_M, (z0 / CELL_M).floor() * CELL_M);
        let cols = (((x1 - x0) / CELL_M).floor() as u32 + 1).max(1);
        let rows = (((z1 - z0) / CELL_M).floor() as u32 + 1).max(1);
        let cell = |p: &CoverPoint| (((p.pos[2] - z0) / CELL_M) as u32).min(rows - 1) * cols + (((p.pos[0] - x0) / CELL_M) as u32).min(cols - 1);
        points.sort_by_key(cell);
        let mut start = vec![0u32; (cols * rows) as usize + 1];
        for p in &points {
            start[cell(p) as usize + 1] += 1;
        }
        for i in 1..start.len() {
            start[i] += start[i - 1];
        }
        CoverPoints { format: FORMAT, generator: GENERATOR, mesh, bots: crate::bots::VERSION.to_string(), x0, z0, cols, rows, start, points }
    }

    /// The points within `r` of (x, z) across, in the grid's order.
    pub fn near(&self, x: f32, z: f32, r: f32) -> impl Iterator<Item = &CoverPoint> + '_ {
        let c = |v: f32, o: f32, n: u32| (((v - o) / CELL_M).floor() as i64).clamp(0, n as i64 - 1) as u32;
        let (cx0, cx1, cz0, cz1) = (c(x - r, self.x0, self.cols), c(x + r, self.x0, self.cols), c(z - r, self.z0, self.rows), c(z + r, self.z0, self.rows));
        let empty = self.points.is_empty();
        (cz0..=cz1).filter(move |_| !empty).flat_map(move |cz| {
            let (a, b) = (self.start[(cz * self.cols + cx0) as usize], self.start[(cz * self.cols + cx1) as usize + 1]);
            self.points[a as usize..b as usize].iter()
        }).filter(move |p| (p.pos[0] - x).hypot(p.pos[2] - z) <= r)
    }

    /// The points from the cache in `dir` if it holds them for this mesh and this generator, else built (and written there).
    pub fn load_or_build(world: &World, mesh: &NavMesh, threads: usize, dir: &Path) -> (CoverPoints, Cached) {
        let print = mesh.fingerprint();
        let path = Self::cache_path(world, print, dir);
        if let Some(c) = std::fs::read(&path).ok().and_then(|b| bincode::deserialize::<CoverPoints>(&b).ok())
            && c.format == FORMAT
            && c.generator == GENERATOR
            && c.mesh == print
        {
            return (c, Cached::Read);
        }
        let c = Self::build(world, mesh, threads);
        let written = std::fs::create_dir_all(dir).and_then(|_| {
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, bincode::serialize(&c).map_err(std::io::Error::other)?)?;
            std::fs::rename(&tmp, &path)
        });
        (c, if written.is_ok() { Cached::Built } else { Cached::NotWritten })
    }

    /// Where [`CoverPoints::load_or_build`] keeps this level's points.
    pub fn cache_path(world: &World, mesh_print: u64, dir: &Path) -> PathBuf {
        dir.join(format!("{}-{:016x}-g{GENERATOR}.cover", world.level.name, mesh_print))
    }

    /// The default cache directory: `cache/cover` next to the game's tree (`data/cache/cover` for `data/game`).
    pub fn default_cache_dir(game: &Path) -> PathBuf {
        game.parent().unwrap_or(Path::new(".")).join("cache").join("cover")
    }
}

/// The cover points along polygon `p`'s border edges, before merging.
fn samples(world: &World, mesh: &NavMesh, p: u32) -> Vec<CoverPoint> {
    let q = mesh.polys[p as usize];
    let v: Vec<[f32; 3]> = mesh.poly_verts(p).collect();
    let n = v.len();
    let mut out = Vec::new();
    for k in 0..n {
        if mesh.nei[q.first as usize + k] != NONE {
            continue;
        }
        let (a, b) = (v[k], v[(k + 1) % n]);
        let len = (b[0] - a[0]).hypot(b[2] - a[2]);
        // (the middles of equal pieces no longer than the spacing: never the corners, which two edges share)
        let pieces = (len / SPACING_M).ceil().max(1.0) as usize;
        for i in 0..pieces {
            let t = (i as f32 + 0.5) / pieces as f32;
            let (x, z) = (a[0] + (b[0] - a[0]) * t, a[2] + (b[2] - a[2]) * t);
            // (a hair inside the polygon, for its height)
            let c = mesh.centre(p);
            let (ix, iz) = (x + (c[0] - x) * 0.02, z + (c[2] - z) * 0.02);
            let y = mesh.height(p, ix, iz);
            if let Some(cp) = probe(world, [x, y, z], p) {
                out.push(cp);
            }
        }
    }
    out
}

/// The cover at feet `f` on polygon `p`, if something hides it in some direction.
pub fn probe(world: &World, f: [f32; 3], p: u32) -> Option<CoverPoint> {
    let fan = |h: f32, s: usize| {
        let body = [f[0], f[1] + h, f[2]];
        (0..FAN)
            .filter(|&i| {
                let a = s as f32 * std::f32::consts::TAU / SECTORS as f32 + (i as f32 - (FAN - 1) as f32 / 2.0) * 15f32.to_radians();
                world.hidden(body, [body[0] + RAY_M * a.cos(), body[1], body[2] + RAY_M * a.sin()])
            })
            .count()
            >= FAN_COVERED
    };
    let mut crouch = 0u8;
    let mut stand = 0u8;
    for s in 0..SECTORS {
        if fan(CROUCH_M, s) {
            crouch |= 1 << s;
            if fan(STAND_M, s) {
                stand |= 1 << s;
            }
        }
    }
    if crouch == 0 {
        return None;
    }
    let eye = [f[0], f[1] + STAND_M, f[2]];
    let roof = world.hidden(eye, [eye[0], eye[1] + ROOF_M, eye[2]]);
    Some(CoverPoint { pos: f, poly: p, crouch, stand, roof })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sectors_go_round() {
        assert_eq!(sector(1.0, 0.0), 0);
        assert_eq!(sector(0.0, 1.0), 2);
        assert_eq!(sector(-1.0, 0.0), 4);
        assert_eq!(sector(0.0, -1.0), 6);
        assert_eq!(sector(1.0, -0.1), 0);
        assert_eq!(sector(1.0, 1.0), 1);
    }

    #[test]
    fn the_grid_finds_the_near_ones() {
        let pt = |x: f32, z: f32| CoverPoint { pos: [x, 0.0, z], poly: 0, crouch: 1, stand: 0, roof: false };
        let c = CoverPoints::from_points(vec![pt(0.0, 0.0), pt(40.0, 5.0), pt(10.0, 0.0), pt(-30.0, -30.0)], 0);
        let mut got: Vec<[f32; 3]> = c.near(2.0, 0.0, 12.0).map(|p| p.pos).collect();
        got.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert_eq!(got, vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
        assert_eq!(c.near(-30.0, -30.0, 1.0).count(), 1);
        assert_eq!(CoverPoints::from_points(Vec::new(), 0).near(0.0, 0.0, 100.0).count(), 0);
    }
}
