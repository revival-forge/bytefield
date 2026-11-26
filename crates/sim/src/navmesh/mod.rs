//! Walkable-surface navigation: a polygon mesh of what a soldier can stand on and walk between (terrain, bridges, hangars,
//! decks, floors), built with rerecast from the terrain and the level's collision meshes, in tiles stitched along their
//! shared edges, with ladders as links off the mesh. It knows several floors over one spot, where the bots' flat maps
//! know only "ground or wall". This is the mesh, its cache on disk and its debugging view; the queries over it (paths,
//! the polygon under a point, lines, zones, distance fields) are in [`query`].
//!
//! Its parameters come from the soldier's own physics, measured (`tests/base/soldier_step.rs`): the highest step it walks
//! onto and the steepest ramp it walks up.
mod build;
pub mod query;
pub mod view;

use crate::World;
use crate::obstacle::Obstacle;
use crate::vecmath::V3;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The generator's version: part of the cache's key, raised when what it builds changes.
pub const GENERATOR: u32 = 21;

/// The highest step a soldier walks onto (0.45 m does, 0.5 m does not), measured.
pub const SOLDIER_STEP: f32 = 0.45;
/// The steepest object ramp a soldier walks up (46° does, 48° does not), measured.
pub const SOLDIER_RAMP_DEG: f32 = 46.0;
/// The steepest terrain a soldier walks up (every stretch of 24–40° does, those of 40–48° do not), measured; the levels'
/// own slope for the bots' maps is lower (30°), and Recast's ledge filter would cut steeper terrain anyway (about
/// atan(climb / 2 cs), 38.7° with 0.4 m and 0.25 m cells).
pub const SOLDIER_TERRAIN_DEG: f32 = 38.0;
/// The highest drop a soldier takes without losing any health (3 m does; 3.5 m costs 2 of its 30 hit points, 7 m kills),
/// measured.
pub const SOLDIER_DROP: f32 = 3.0;
/// The highest drop that costs a soldier under a third of its health (5 m costs 9 of 30, 5.5 m 12), measured: the mesh's
/// links down a ledge go no deeper, and those beyond [`SOLDIER_DROP`] cost more to the searches (DC_Al_Nas: a team spawns
/// on a roof 4.4 m over the street, with no other way down).
pub const SOLDIER_DROP_MAX: f32 = 5.0;
/// The highest ledge a soldier gets onto running and jumping (m), and the gap between a sill and the lintel above it it jumps
/// through whatever the sill up to that (m; 1.4 m goes at some sills only); measured in `tests/base/soldier_step.rs`.
pub const SOLDIER_JUMP: f32 = 1.4;
pub const SOLDIER_JUMP_GAP: f32 = 1.5;
/// The highest a jump link goes up (m): the measured jump had two metres' run-up, square on; a bot jumps from where its path
/// takes it, off a narrow strip or a slope (Omaha_Beach's sandbags at 939,1150: links of +1.4 m that no bot made).
pub const JUMP_UP: f32 = 1.0;
/// The largest place with no way out that [`NavMesh::sink_exits`] looks for a way out of, m².
pub const SINK_M2: f32 = 400.0;
/// The height over its feet where a walking soldier's head goes by, m (under the 1.8 m it stands).
const HEAD_M: f32 = 1.6;
/// Half a walking soldier's width, m.
const SIDE_M: f32 = 0.3;
/// The narrowest door a soldier walks through (m), measured in `tests/base/soldier_step.rs`.
pub const SOLDIER_DOOR: f32 = 0.5;
/// The lowest door a soldier walks through standing (m), measured in `tests/base/soldier_step.rs`.
pub const SOLDIER_DOOR_HIGH: f32 = 1.5;
/// How far up or down the floor beyond a narrow door may be (m).
pub const DOOR_STEP: f32 = 0.5;
/// How far apart along a ledge its drops are looked for, and how far beyond it (m): the edge of the floor above and the
/// floor below are each eroded a radius back from the wall between them.
pub const DROP_SPACING: f32 = 2.0;
pub const DROP_REACH: f32 = 1.5;
/// Where the floor beyond a jump is looked for: this far beyond the edge (m), within [`JUMP_SPREAD`] of it across, and no
/// nearer the edge than [`JUMP_AHEAD`] (a window's wall and the erosion on both sides of it).
pub const JUMP_REACH: f32 = 1.3;
pub const JUMP_SPREAD: f32 = 0.9;
pub const JUMP_AHEAD: f32 = 0.4;
/// How far either way of straight out from an edge a jump is also looked for (radians).
pub const JUMP_SLANT: f32 = 0.5;
/// The threads the jumps are looked for on.
const JUMP_THREADS: usize = 4;
/// Half the width of the opening a jump goes through (m): the soldier's shoulders.
pub const JUMP_HALF_WIDTH: f32 = 0.3;
/// How far apart along an edge its jumps are looked for (m): a window is a metre or so wide.
pub const JUMP_SPACING: f32 = 0.5;

/// A polygon edge's neighbour: none (a wall or a ledge)…
pub const NONE: u32 = u32::MAX;
/// …or on the tile's border (low bits: the side, 0 −x, 1 +z, 2 +x, 3 −z), joined through [`NavMesh::seams`]; any other
/// value is the neighbouring polygon in the same tile.
pub const PORTAL: u32 = 0xffff_fff0;

/// What the mesh is built with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Params {
    /// cell size across and up (m)
    pub cs: f32,
    pub ch: f32,
    /// cells per tile side
    pub tile: u32,
    /// the walker's radius, height and step (m)
    pub radius: f32,
    pub height: f32,
    pub climb: f32,
    /// the steepest terrain and object faces that count as ground (degrees)
    pub slope: f32,
    pub object_slope: f32,
    /// how deep the terrain may be under the water and still be walked (m)
    pub water_depth: f32,
    /// also build the detail triangles (the fine height) of the polygons over objects; those on the terrain take its height
    pub detail: bool,
    /// also the soldier's links: ladders, drops, jumps and the ways out of small places
    pub links: bool,
}

impl Params {
    /// A soldier's, on `world`: the measured step, object ramp and terrain slope, the lowest door it walks through, and the
    /// level's wading depth. A step under
    /// 0.6 m needs the finer cells: with 0.5 m cells, Recast's ledge filter leaves out terrain steeper than about
    /// atan(climb / 2 cs).
    pub fn infantry(world: &World) -> Params {
        let nav = crate::navgen::Params::of_world(world, crate::bot_api::Nav::Infantry);
        Params {
            cs: 0.25,
            ch: 0.1,
            tile: 128,
            radius: 0.5,
            height: SOLDIER_DOOR_HIGH,
            climb: SOLDIER_STEP,
            slope: SOLDIER_TERRAIN_DEG,
            object_slope: SOLDIER_RAMP_DEG,
            water_depth: nav.water_depth as f32,
            detail: true,
            links: true,
        }
    }

    // (the small margin keeps a measured 0.45 m from becoming 4 cells of 0.1 m by rounding)
    pub fn vx_climb(&self) -> u16 {
        (self.climb / self.ch + 1e-3).floor() as u16
    }
    pub fn vx_height(&self) -> u16 {
        (self.height / self.ch - 1e-3).ceil() as u16
    }
    pub fn vx_radius(&self) -> u16 {
        (self.radius / self.cs - 1e-3).ceil() as u16
    }
    /// the cells each tile is built with around it, so that its own cells come out as with the whole level
    pub fn border(&self) -> usize {
        self.vx_radius() as usize + 3
    }
}

/// A polygon: its vertices (`first..first + n` in [`NavMesh::idx`], with each edge's neighbour at the same place in
/// [`NavMesh::nei`]: edge k goes from vertex k to k + 1), area, tile and detail triangles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Poly {
    pub first: u32,
    pub n: u8,
    pub area: u8,
    /// on the terrain (every vertex within [`GROUND_BELOW`]..[`GROUND_ABOVE`] of it): its height inside is the terrain's
    pub ground: bool,
    pub tile: u32,
    pub detail: u32,
    pub ndetail: u32,
}

/// How far a vertex of a polygon on the terrain may be below it and above it (m): Recast puts a polygon's corner at the top
/// of the highest cell around it, so on a slope of 38° with cells of 0.25 m a corner on the terrain is up to ~0.4 m above.
pub const GROUND_BELOW: f32 = 0.3;
pub const GROUND_ABOVE: f32 = 0.5;

/// The terrain's heights at its heightmap's nodes, interpolated over the same triangles as [`World::height_at`] (in f32,
/// to ~1 mm): the height inside the polygons on the terrain. It is not cached with the mesh; [`NavMesh::load_or_build`]
/// and [`NavMesh::build`] fill it from the world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ground {
    n: usize,
    cell: f32,
    h: Vec<f32>,
}

impl Ground {
    pub fn of(world: &World) -> Ground {
        let n = world.level.heightmap.size;
        let cell = (world.level.terrain.world_size / n as f64) as f32;
        let mut h = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                h.push(world.height_at(i as f64 * cell as f64, j as f64 * cell as f64).unwrap_or(0.0) as f32);
            }
        }
        Ground { n, cell, h }
    }

    /// The height at (x, z); `None` off the terrain.
    pub fn at(&self, x: f32, z: f32) -> Option<f32> {
        let n = self.n;
        let (u, v) = (x / self.cell, z / self.cell);
        if n < 2 || !(0.0..=(n - 1) as f32).contains(&u) || !(0.0..=(n - 1) as f32).contains(&v) {
            return None;
        }
        let (c, r) = ((u as usize).min(n - 2), (v as usize).min(n - 2));
        let (fu, fv) = (u - c as f32, v - r as f32);
        let s = |c: usize, r: usize| self.h[r * n + c];
        let (a, b, d0, d) = (s(c, r), s(c + 1, r), s(c, r + 1), s(c + 1, r + 1));
        // the same diagonal as the terrain's (`World::height_at`)
        Some(if (c + r) % 2 == 0 {
            if fu > fv { a + (b - a) * fu + (d - b) * fv } else { a + (d - d0) * fu + (d0 - a) * fv }
        } else if fu + fv < 1.0 {
            a + (b - a) * fu + (d0 - a) * fv
        } else {
            d + (d0 - d) * (1.0 - fu) + (b - d) * (1.0 - fv)
        })
    }
}

/// Two polygons of neighbouring tiles that share part of a border edge: `a`'s edge `edge` leads into `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Seam {
    pub a: u32,
    pub edge: u8,
    pub b: u32,
}

/// A way between two polygons off the mesh: a ladder (both ways), a drop off a ledge or a jump onto one or through a window
/// (from `a` to `b` only).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub a: u32,
    pub b: u32,
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub kind: LinkKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkKind {
    Ladder,
    Drop,
    Jump,
    /// a door narrower than the mesh's erosion lets through, walked
    Door,
    /// walked out of a small place with no other way out ([`NavMesh::sink_exits`]); one way
    Exit,
}

impl Link {
    pub fn both_ways(&self) -> bool {
        matches!(self.kind, LinkKind::Ladder | LinkKind::Door)
    }
}

/// How long each tile took (s): rasterize and filter, compact and regions, contours and polygons, detail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TileStats {
    pub secs: [f32; 4],
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NavMesh {
    pub params: Params,
    /// tiles per side; tile (tx, tz) is number tz · side + tx
    pub side: u32,
    pub ymin: f32,
    pub verts: Vec<[f32; 3]>,
    pub polys: Vec<Poly>,
    pub idx: Vec<u32>,
    pub nei: Vec<u32>,
    /// the first polygon of each tile, and the total last
    pub tile_first: Vec<u32>,
    /// across the tiles' borders, sorted
    pub seams: Vec<Seam>,
    pub links: Vec<Link>,
    pub detail: Vec<[[f32; 3]; 3]>,
    pub stats: Vec<TileStats>,
    /// the terrain, for the height inside the polygons on it (not cached: filled from the world)
    #[serde(skip)]
    pub ground: Option<Ground>,
}

impl Default for Params {
    fn default() -> Params {
        Params { cs: 0.25, ch: 0.1, tile: 128, radius: 0.5, height: 1.8, climb: SOLDIER_STEP, slope: SOLDIER_TERRAIN_DEG, object_slope: SOLDIER_RAMP_DEG, water_depth: 0.0, detail: true, links: true }
    }
}

/// [`place`], for the tools.
pub fn place_point(o: &Obstacle, v: V3) -> V3 {
    place(o, v)
}

/// An obstacle's mesh vertex placed in the world.
pub(crate) fn place(o: &Obstacle, v: V3) -> V3 {
    let p = [v[0] * o.scale[0], v[1] * o.scale[1], v[2] * o.scale[2]];
    [0, 1, 2].map(|c| o.rows[0][c] * p[0] + o.rows[1][c] * p[1] + o.rows[2][c] * p[2] + o.pos[c])
}

/// Whether the cache was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cached {
    Read,
    Built,
    /// built; the cache could not be written
    NotWritten,
}

impl NavMesh {
    /// Builds the mesh of `world` on `threads` threads (the same mesh with any number).
    pub fn build(world: &World, params: &Params, threads: usize) -> NavMesh {
        Self::build_in(world, params, threads, None)
    }

    /// [`NavMesh::build`] with only the tiles that touch `area` (x0, z0, x1, z1), the others empty: for tests and probes.
    pub fn build_in(world: &World, params: &Params, threads: usize, area: Option<[f32; 4]>) -> NavMesh {
        let solids = build::place_solids(world, params);
        let mut m = build::build(world, &solids, params, threads, area);
        m.ground = Some(Ground::of(world));
        if !params.links {
            return m;
        }
        let t = std::time::Instant::now();
        m.ladders(world);
        let t1 = t.elapsed().as_secs_f32();
        m.drops(world);
        let t2 = t.elapsed().as_secs_f32();
        m.jumps(world);
        m.sink_exits(world);
        if crate::debug::options().navmesh_link_times {
            eprintln!("links: ladders {t1:.2} s, drops {:.2} s, jumps {:.2} s", t2 - t1, t.elapsed().as_secs_f32() - t2);
        }
        m
    }

    /// The mesh from the cache in `dir` if it holds one for this level, these parameters and this generator, else built
    /// (and written there). The key is a hash of what goes in (the terrain, the collision meshes, the water, the
    /// materials), the parameters and [`GENERATOR`], so a changed mod or level does not read a stale mesh.
    pub fn load_or_build(world: &World, params: &Params, threads: usize, dir: &Path) -> (NavMesh, Cached) {
        let path = Self::cache_path(world, params, dir);
        if let Some(mut m) = std::fs::read(&path).ok().and_then(|b| bincode::deserialize::<NavMesh>(&b).ok()) {
            m.ground = Some(Ground::of(world));
            return (m, Cached::Read);
        }
        let m = Self::build(world, params, threads);
        let written = std::fs::create_dir_all(dir).and_then(|_| {
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, bincode::serialize(&m).map_err(std::io::Error::other)?)?;
            std::fs::rename(&tmp, &path)
        });
        (m, if written.is_ok() { Cached::Built } else { Cached::NotWritten })
    }

    /// Where [`NavMesh::load_or_build`] keeps this level's mesh.
    pub fn cache_path(world: &World, params: &Params, dir: &Path) -> PathBuf {
        dir.join(format!("{}-{:016x}.navmesh", world.level.name, input_hash(world, params)))
    }

    /// The default cache directory: `cache/navmesh` next to the game's tree (`data/cache/navmesh` for `data/game`).
    pub fn default_cache_dir(game: &Path) -> PathBuf {
        game.parent().unwrap_or(Path::new(".")).join("cache").join("navmesh")
    }

    pub fn tile_m(&self) -> f32 {
        self.params.tile as f32 * self.params.cs
    }

    /// The vertices of polygon `p`, in order.
    pub fn poly_verts(&self, p: u32) -> impl Iterator<Item = [f32; 3]> + '_ {
        let q = self.polys[p as usize];
        self.idx[q.first as usize..q.first as usize + q.n as usize].iter().map(|&i| self.verts[i as usize])
    }

    /// The polygons next to `p`: across its edges, its tile's borders and its links.
    pub fn neighbours(&self, p: u32, out: &mut Vec<u32>) {
        out.clear();
        let q = self.polys[p as usize];
        for &n in &self.nei[q.first as usize..q.first as usize + q.n as usize] {
            if n < PORTAL {
                out.push(n);
            }
        }
        let s = self.seams.partition_point(|s| s.a < p);
        out.extend(self.seams[s..].iter().take_while(|s| s.a == p).map(|s| s.b));
        out.extend(self.links.iter().filter_map(|l| if l.a == p { Some(l.b) } else if l.b == p && l.both_ways() { Some(l.a) } else { None }));
    }

    /// Joins the polygons of neighbouring tiles: two border edges on the same line that overlap along it, at heights within
    /// a step of each other at both ends of the overlap.
    fn stitch(&mut self) {
        let side = self.side as i64;
        let climb = self.params.climb + self.params.ch;
        // the border edges of each tile by side: (polygon, edge, a, b) with a, b the ends
        type Edges = Vec<(u32, u8, [f32; 3], [f32; 3])>;
        let mut border: Vec<[Edges; 4]> = (0..self.side * self.side).map(|_| Default::default()).collect();
        for (pi, q) in self.polys.iter().enumerate() {
            for k in 0..q.n as usize {
                let n = self.nei[q.first as usize + k];
                if (PORTAL..PORTAL + 4).contains(&n) {
                    let a = self.verts[self.idx[q.first as usize + k] as usize];
                    let b = self.verts[self.idx[q.first as usize + (k + 1) % q.n as usize] as usize];
                    border[q.tile as usize][(n - PORTAL) as usize].push((pi as u32, k as u8, a, b));
                }
            }
        }
        let mut seams = Vec::new();
        for t in 0..(side * side) {
            let (tx, tz) = (t % side, t / side);
            for d in 0..4usize {
                let (nx, nz) = match d {
                    0 => (tx - 1, tz),
                    1 => (tx, tz + 1),
                    2 => (tx + 1, tz),
                    _ => (tx, tz - 1),
                };
                if nx < 0 || nz < 0 || nx >= side || nz >= side {
                    continue;
                }
                let other = &border[(nz * side + nx) as usize][(d + 2) % 4];
                // along the shared line: z for the sides across x, x for the others
                let ax = if d % 2 == 0 { 2 } else { 0 };
                for &(pa, ea, a0, a1) in &border[t as usize][d] {
                    let (lo, hi) = (a0[ax].min(a1[ax]), a0[ax].max(a1[ax]));
                    for &(pb, _, b0, b1) in other {
                        let (blo, bhi) = (b0[ax].min(b1[ax]), b0[ax].max(b1[ax]));
                        let (s, e) = (lo.max(blo), hi.min(bhi));
                        if e - s < 0.5 * self.params.cs {
                            continue;
                        }
                        let y = |p: [f32; 3], q: [f32; 3], at: f32| if (q[ax] - p[ax]).abs() < 1e-6 { p[1] } else { p[1] + (q[1] - p[1]) * (at - p[ax]) / (q[ax] - p[ax]) };
                        if (y(a0, a1, s) - y(b0, b1, s)).abs() <= climb && (y(a0, a1, e) - y(b0, b1, e)).abs() <= climb {
                            seams.push(Seam { a: pa, edge: ea, b: pb });
                        }
                    }
                }
            }
        }
        seams.sort();
        seams.dedup();
        self.seams = seams;
    }

    /// The ladders as links: from the polygon at the foot of each to the one at its head. A ladder is an obstacle with faces
    /// of a ladder's material; its foot is in front of those faces at their bottom, its head at their top (where the
    /// soldier steps off it).
    fn ladders(&mut self, world: &World) {
        let mut links = Vec::new();
        for o in world.obstacles() {
            let Some(mesh) = crate::obstacle::pick_lod(&o.lods, 1) else { continue };
            let faces: Vec<_> = mesh.faces.iter().filter(|f| crate::ladder::MATERIALS.contains(&f.material)).collect();
            if faces.is_empty() {
                continue;
            }
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            let mut nrm = [0.0f32; 3];
            for f in &faces {
                for &i in &f.v {
                    let Some(&v) = mesh.vertices.get(i as usize) else { continue };
                    let w = place(o, v);
                    for c in 0..3 {
                        lo[c] = lo[c].min(w[c]);
                        hi[c] = hi[c].max(w[c]);
                    }
                }
                for c in 0..3 {
                    nrm[c] += o.rows[0][c] * f.normal[0] + o.rows[1][c] * f.normal[1] + o.rows[2][c] * f.normal[2];
                }
            }
            if hi[1] - lo[1] < 1.0 {
                continue;
            }
            let l = nrm[0].hypot(nrm[2]);
            let (nx, nz) = if l > 1e-3 { (nrm[0] / l, nrm[2] / l) } else { (0.0, 0.0) };
            let (cx, cz) = (0.5 * (lo[0] + hi[0]), 0.5 * (lo[2] + hi[2]));
            // the foot: on the side the faces look to (or either), at the bottom
            let foot = [1.0f32, -1.0].iter().find_map(|&s| self.nearest(cx + s * 0.8 * nx, lo[1], cz + s * 0.8 * nz, 1.5, 1.5));
            // the head: near the top, on either side or on it (a hatch)
            let head = [0.0f32, -1.0, 1.0].iter().find_map(|&s| self.nearest(cx + s * 1.0 * nx, hi[1], cz + s * 1.0 * nz, 2.0, 2.5));
            if let (Some(a), Some(b)) = (foot, head)
                && a != b
            {
                let (pa, pb) = (self.centre(a), self.centre(b));
                if pb[1] - pa[1] >= 1.0 {
                    links.push(Link { a, b, from: [cx, lo[1], cz], to: [cx, hi[1], cz], kind: LinkKind::Ladder });
                }
            }
        }
        self.links = links;
    }

    /// The drops off ledges as links of one way: from each edge of a polygon with nothing across it (a ledge or a wall), every
    /// [`DROP_SPACING`] m along it, to the floor below just beyond it, if it lies [`Params::climb`]..[`SOLDIER_DROP_MAX`] lower
    /// and a soldier falls there without meeting a wall; one link per pair of polygons, from the first such spot along the
    /// edge.
    fn drops(&mut self, world: &World) {
        let mut links = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for p in 0..self.polys.len() as u32 {
            for (e, found) in self.drops_from(world, p) {
                if let Ok((r, l)) = found
                    && seen.insert((p, r))
                {
                    links.push(Link { a: p, b: r, from: e, to: l, kind: LinkKind::Drop });
                }
            }
        }
        self.links.extend(links);
    }

    /// The drops off polygon `p`'s open edges, spot by spot along them ([`NavMesh::drops`]): the spot on the edge and the
    /// floor it lands on (polygon, point), or why there is none.
    pub fn drops_from(&self, world: &World, p: u32) -> Vec<([f32; 3], Result<(u32, [f32; 3]), &'static str>)> {
        let climb = self.params.climb + self.params.ch;
        let tm = self.tile_m();
        let side = self.side as i32;
        let mut out = Vec::new();
        let q = self.polys[p as usize];
        let c = self.centre(p);
        for k in 0..q.n as usize {
            if self.nei[q.first as usize + k] < PORTAL {
                continue;
            }
            let a = self.verts[self.idx[q.first as usize + k] as usize];
            let b = self.verts[self.idx[q.first as usize + (k + 1) % q.n as usize] as usize];
            let len = (b[0] - a[0]).hypot(b[2] - a[2]);
            if len < 0.3 {
                continue;
            }
            // outwards: across the edge, away from the polygon's centre
            let (mut ox, mut oz) = ((b[2] - a[2]) / len, -(b[0] - a[0]) / len);
            if ox * (a[0] - c[0]) + oz * (a[2] - c[2]) < 0.0 {
                (ox, oz) = (-ox, -oz);
            }
            let n = (len / DROP_SPACING).floor().max(0.0) as usize + 1;
            for i in 0..n {
                let t = (i as f32 + 0.5) / n as f32;
                let e = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
                // the floor below, just beyond: from the eroded edge above to the eroded foot of the wall below
                let at = [e[0] + ox * DROP_REACH, e[2] + oz * DROP_REACH];
                let tile = |v: f32| ((v / tm).floor() as i32).clamp(0, side - 1);
                let mut best: Option<(f32, u32, [f32; 3])> = None;
                for tz in tile(at[1] - 1.0)..=tile(at[1] + 1.0) {
                    for tx in tile(at[0] - 1.0)..=tile(at[0] + 1.0) {
                        let t = (tz * side + tx) as usize;
                        for r in self.tile_first[t]..self.tile_first[t + 1] {
                            if r == p {
                                continue;
                            }
                            let l = self.closest_on(r, at[0], at[1]);
                            let (dh, down) = ((l[0] - at[0]).hypot(l[2] - at[1]), e[1] - l[1]);
                            // (beyond the edge, not back under it)
                            let ahead = (l[0] - e[0]) * ox + (l[2] - e[2]) * oz;
                            if dh > DROP_REACH || down <= climb || down > SOLDIER_DROP_MAX || ahead < 0.25 {
                                continue;
                            }
                            if best.is_none_or(|bst| dh < bst.0) {
                                best = Some((dh, r, l));
                            }
                        }
                    }
                }
                let Some((_, r, l)) = best else {
                    out.push((e, Err("no floor below within reach")));
                    continue;
                };
                // a soldier steps over the edge at chest height and falls straight onto the floor below
                let over = [l[0], e[1] + 1.0, l[2]];
                // (and at head height, as wide as a soldier: under a low lintel over the edge one pushed on the spot for minutes, DC_Weapon_Bunkers
                // 503,76,398; at the knee too cost more than it saved, the drops it took away left pockets, DC_Basrahs_Edge 314,7,398)
                let head = e[1] + HEAD_M;
                // (a soldier is as wide as two rays a hand to each side of the middle: the middle alone went by a crate's corner)
                let (dx, dz) = (l[0] - e[0], l[2] - e[2]);
                let n = dx.hypot(dz).max(1e-3);
                let (sx, sz) = (-dz / n * SIDE_M, dx / n * SIDE_M);
                let wide = |h: f32| [-1.0, 0.0, 1.0].iter().any(|k: &f32| world.walk_blocked([e[0] + k * sx, h, e[2] + k * sz], [l[0] + k * sx, h, l[2] + k * sz]));
                if world.walk_blocked([e[0], e[1] + 1.0, e[2]], over) || wide(head) {
                    out.push((e, Err("a wall at the edge")));
                } else if world.walk_blocked(over, [l[0], l[1] + 0.5, l[2]]) {
                    out.push((e, Err("something in the way down")));
                } else {
                    out.push((e, Ok((r, l))));
                }
            }
        }
        out
    }

    /// The jumps as links of one way, and the doors too narrow for the mesh as links both ways: from each edge of a polygon with nothing across it, every [`JUMP_SPACING`] m along it, to
    /// a floor just beyond it ([`JUMP_REACH`]) that a soldier does not walk to (a ledge over the step, or a wall in between)
    /// but gets to running and jumping: no higher than [`JUMP_UP`], no lower than [`SOLDIER_DROP`], with an opening
    /// [`SOLDIER_JUMP_GAP`] high between the two at some height the jump reaches (a window over its sill), nothing over the
    /// spot it jumps from and nothing in the way down onto the floor. One link per pair of polygons not already joined by a
    /// drop, from the first such spot along the edge.
    fn jumps(&mut self, world: &World) {
        let mut seen: std::collections::BTreeSet<(u32, u32)> = self.links.iter().map(|l| (l.a, l.b)).collect();
        let mut links = Vec::new();
        let boxes = self.boxes();
        // (on several threads, every JUMP_THREADS-th polygon each; taken in order after, so the same mesh with any number)
        let n = self.polys.len();
        let me: &NavMesh = self;
        let mut found: Vec<Vec<_>> = vec![Vec::new(); n];
        let parts: Vec<Vec<(usize, Vec<_>)>> = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..JUMP_THREADS)
                .map(|t0| {
                    let boxes = &boxes;
                    sc.spawn(move || (t0..n).step_by(JUMP_THREADS).map(|p| (p, me.jumps_among(world, p as u32, boxes, false))).collect::<Vec<_>>())
                })
                .collect();
            hs.into_iter().map(|h| h.join().expect("a jump thread")).collect()
        });
        for (p, spots) in parts.into_iter().flatten() {
            found[p] = spots;
        }
        for (p, spots) in found.into_iter().enumerate() {
            let p = p as u32;
            for (e, found) in spots {
                if let Ok((r, l, kind)) = found
                    && !(kind == LinkKind::Door && seen.contains(&(r, p)))
                    && seen.insert((p, r))
                {
                    links.push(Link { a: p, b: r, from: e, to: l, kind });
                }
            }
        }
        self.links.extend(links);
    }

    /// The jumps off polygon `p`'s open edges, spot by spot along them ([`NavMesh::jumps`]): the spot on the edge and the
    /// floor it lands on (polygon, point), or why there is none.
    pub fn jumps_from(&self, world: &World, p: u32) -> Vec<([f32; 3], Result<(u32, [f32; 3], LinkKind), &'static str>)> {
        self.jumps_among(world, p, &self.boxes(), false)
    }

    /// Each polygon's strongly connected component over its neighbours and the links (what can be walked to and back), numbered
    /// from 0.
    pub fn components(&self) -> Vec<u32> {
        let n = self.polys.len();
        let mut out: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (p, q) in self.polys.iter().enumerate() {
            for k in 0..q.n as usize {
                let nb = self.nei[q.first as usize + k];
                if nb < PORTAL {
                    out[p].push(nb);
                }
            }
        }
        for l in &self.links {
            out[l.a as usize].push(l.b);
            if l.both_ways() {
                out[l.b as usize].push(l.a);
            }
        }
        // (Tarjan's, without recursion)
        const UNSEEN: u32 = u32::MAX;
        let (mut index, mut low, mut comp) = (vec![UNSEEN; n], vec![0u32; n], vec![UNSEEN; n]);
        let (mut on, mut stack, mut next, mut count) = (vec![false; n], Vec::new(), 0u32, 0u32);
        for root in 0..n {
            if index[root] != UNSEEN {
                continue;
            }
            let mut call: Vec<(usize, usize)> = vec![(root, 0)];
            index[root] = next;
            low[root] = next;
            next += 1;
            stack.push(root);
            on[root] = true;
            while let Some(&mut (v, ref mut i)) = call.last_mut() {
                if let Some(&w) = out[v].get(*i) {
                    *i += 1;
                    let w = w as usize;
                    if index[w] == UNSEEN {
                        index[w] = next;
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on[w] = true;
                        call.push((w, 0));
                    } else if on[w] {
                        low[v] = low[v].min(index[w]);
                    }
                } else {
                    call.pop();
                    if let Some(&(u, _)) = call.last() {
                        low[u] = low[u].min(low[v]);
                    }
                    if low[v] == index[v] {
                        while let Some(w) = stack.pop() {
                            on[w] = false;
                            comp[w] = count;
                            if w == v {
                                break;
                            }
                        }
                        count += 1;
                    }
                }
            }
        }
        comp
    }

    /// Ways out, walking, of the small places a soldier gets into and not out of (a component of [`NavMesh::components`] with
    /// nothing leaving it and less than [`SINK_M2`]): the floor beyond the edge that the mesh does not join, where nothing is
    /// in the way at the knee or the chest, as links out only ([`LinkKind::Exit`]: both ways, they let paths into rubble and
    /// up onto roofs, DC_Basrahs_Edge 87 → 267 s stuck per round). DC_Bridge's spawn under the stairs of a market building:
    /// jumps and drops led in, a step of 0.45 m between floors the mesh left apart led out.
    fn sink_exits(&mut self, world: &World) {
        let comp = self.components();
        let k = comp.iter().max().map_or(0, |&c| c as usize + 1);
        let mut area = vec![0.0f32; k];
        let mut leaves = vec![false; k];
        for p in 0..self.polys.len() {
            area[comp[p] as usize] += self.area(p as u32);
            let q = self.polys[p];
            for j in 0..q.n as usize {
                let nb = self.nei[q.first as usize + j];
                if nb < PORTAL && comp[nb as usize] != comp[p] {
                    leaves[comp[p] as usize] = true;
                }
            }
        }
        for l in &self.links {
            if comp[l.a as usize] != comp[l.b as usize] {
                leaves[comp[l.a as usize] as usize] = true;
                if l.both_ways() {
                    leaves[comp[l.b as usize] as usize] = true;
                }
            }
        }
        let boxes = self.boxes();
        let mut seen: std::collections::BTreeSet<(u32, u32)> = self.links.iter().flat_map(|l| [(l.a, l.b), (l.b, l.a)]).collect();
        let mut links = Vec::new();
        for p in 0..self.polys.len() as u32 {
            let c = comp[p as usize] as usize;
            if leaves[c] || area[c] >= SINK_M2 {
                continue;
            }
            for (e, found) in self.jumps_among(world, p, &boxes, true) {
                if let Ok((r, l, LinkKind::Door)) = found
                    && comp[r as usize] as usize != c
                    && seen.insert((p, r))
                {
                    links.push(Link { a: p, b: r, from: e, to: l, kind: LinkKind::Exit });
                }
            }
        }
        self.links.extend(links);
    }

    /// Each polygon's extent across (x0, z0, x1, z1).
    fn boxes(&self) -> Vec<[f32; 4]> {
        (0..self.polys.len() as u32).map(|p| self.poly_verts(p).fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, v| [b[0].min(v[0]), b[1].min(v[2]), b[2].max(v[0]), b[3].max(v[2])])).collect()
    }

    /// `walk`: also where it walks across, without a door's jambs ([`NavMesh::sink_exits`]).
    fn jumps_among(&self, world: &World, p: u32, boxes: &[[f32; 4]], walk: bool) -> Vec<([f32; 3], Result<(u32, [f32; 3], LinkKind), &'static str>)> {
        let climb = self.params.climb + self.params.ch;
        let mut out = Vec::new();
        let q = self.polys[p as usize];
        let c = self.centre(p);
        for k in 0..q.n as usize {
            if self.nei[q.first as usize + k] < PORTAL {
                continue;
            }
            let a = self.verts[self.idx[q.first as usize + k] as usize];
            let b = self.verts[self.idx[q.first as usize + (k + 1) % q.n as usize] as usize];
            let len = (b[0] - a[0]).hypot(b[2] - a[2]);
            if len < 0.3 {
                continue;
            }
            let (mut ox, mut oz) = ((b[2] - a[2]) / len, -(b[0] - a[0]) / len);
            if ox * (a[0] - c[0]) + oz * (a[2] - c[2]) < 0.0 {
                (ox, oz) = (-ox, -oz);
            }
            let n = (len / JUMP_SPACING).floor().max(0.0) as usize + 1;
            for i in 0..n {
                let t = (i as f32 + 0.5) / n as f32;
                let e = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
                // straight out from the edge, and slanting either way (a window need not face the edge square)
                let mut first_err = None;
                let mut found = None;
                for ang in [0.0f32, JUMP_SLANT, -JUMP_SLANT] {
                    let (sn, cs) = ang.sin_cos();
                    let (dx, dz) = (ox * cs - oz * sn, ox * sn + oz * cs);
                    match self.jump_toward(world, p, e, [dx, dz], climb, boxes, walk) {
                        Ok(x) => {
                            found = Some(x);
                            break;
                        }
                        Err(why) => {
                            first_err.get_or_insert(why);
                        }
                    }
                }
                out.push((e, found.ok_or(first_err.unwrap_or("no floor beyond within reach"))));
            }
        }
        out
    }

    /// A jump from spot `e` on polygon `p`'s edge in direction (dx, dz) across: the floor it lands on, or why there is none.
    #[allow(clippy::too_many_arguments)]
    fn jump_toward(&self, world: &World, p: u32, e: [f32; 3], d: [f32; 2], climb: f32, boxes: &[[f32; 4]], walk: bool) -> Result<(u32, [f32; 3], LinkKind), &'static str> {
        let tm = self.tile_m();
        let side = self.side as i32;
        let (ox, oz) = (d[0], d[1]);
        let at = [e[0] + ox * JUMP_REACH, e[2] + oz * JUMP_REACH];
        let tile = |v: f32| ((v / tm).floor() as i32).clamp(0, side - 1);
        let mut best: Option<(f32, u32, [f32; 3])> = None;
        for tz in tile(at[1] - 1.0)..=tile(at[1] + 1.0) {
            for tx in tile(at[0] - 1.0)..=tile(at[0] + 1.0) {
                let t = (tz * side + tx) as usize;
                for r in self.tile_first[t]..self.tile_first[t + 1] {
                    let b = boxes[r as usize];
                    if r == p || at[0] < b[0] - JUMP_SPREAD || at[0] > b[2] + JUMP_SPREAD || at[1] < b[1] - JUMP_SPREAD || at[1] > b[3] + JUMP_SPREAD {
                        continue;
                    }
                    let l = self.closest_on(r, at[0], at[1]);
                    let (dh, up) = ((l[0] - at[0]).hypot(l[2] - at[1]), l[1] - e[1]);
                    let ahead = (l[0] - e[0]) * ox + (l[2] - e[2]) * oz;
                    if dh > JUMP_SPREAD || up > JUMP_UP || up < -SOLDIER_DROP || ahead < JUMP_AHEAD {
                        continue;
                    }
                    if best.is_none_or(|bst| dh < bst.0) {
                        best = Some((dh, r, l));
                    }
                }
            }
        }
        let Some((_, r, l)) = best else { return Err("no floor beyond within reach") };
        let up = l[1] - e[1];
        // (up from terrain to terrain is a slope too steep to walk: a jump up it slides back)
        if self.polys[p as usize].ground && self.polys[r as usize].ground && up > climb {
            return Err("a bank of terrain");
        }
        // (across the way it goes, for the body's width)
        let len = (l[0] - e[0]).hypot(l[2] - e[2]).max(1e-3);
        let (ux, uz) = ((l[0] - e[0]) / len, (l[2] - e[2]) / len);
        let ray_at = |h: f32, side: f32| world.walk_blocked([e[0] - uz * side, e[1] + h, e[2] + ux * side], [l[0] - uz * side, e[1] + h, l[2] + ux * side]);
        let ray = |h: f32| ray_at(h, 0.0);
        // walked: no wall at the knee or the chest and no ledge over the step. Level, between objects, through a gap as wide as a
        // soldier's body and as high: a door the mesh's erosion shut (or that the mesh has anyway: then it is not needed)
        if up <= climb && !ray(0.5) && !ray(1.0) {
            let both_terrain = self.polys[p as usize].ground && self.polys[r as usize].ground;
            let half = 0.5 * SOLDIER_DOOR - 0.05;
            // (a door: jambs on both sides, nearer than the mesh's erosion lets through, and the floor level; rubble and gaps
            // between things standing apart are not doors)
            let jamb = self.params.radius + 0.1;
            let jambs = [-jamb, jamb].iter().all(|&sd| ray_at(1.0, sd));
            let top = SOLDIER_DOOR_HIGH - 0.1;
            // (and up and down every 10 cm across, from over the higher floor: a railing's bars pass between level rays, DC_Weapon_Bunkers' fence by the
            // guard tower at 551,701, bars at 0.3, 0.85, 1.5 and 2.1 m, was a way out of the alley behind it that nobody walked)
            let bars = || {
                let n = (len / 0.1).ceil() as usize;
                (0..=n).any(|k| {
                    let t = k as f32 / n.max(1) as f32;
                    let (x, z) = (e[0] + (l[0] - e[0]) * t, e[2] + (l[2] - e[2]) * t);
                    [-half, 0.0, half].iter().any(|&sd| world.walk_blocked([x - uz * sd, e[1].max(l[1]) + 0.3, z + ux * sd], [x - uz * sd, e[1] + top, z + ux * sd]))
                })
            };
            let wide = [0.3, 1.0, top].iter().all(|&h| [-half, half].iter().all(|&sd| !ray_at(h, sd))) && !ray(top) && !bars();
            return match (both_terrain || up.abs() > DOOR_STEP, jambs, wide) {
                (false, true, true) => Ok((r, l, LinkKind::Door)),
                // (a step, up or down: a floor further down, beyond the edge, may be the storey below, under this one)
                _ if walk && wide && up.abs() <= climb => Ok((r, l, LinkKind::Door)),
                (true, _, _) => Err("walked"),
                (false, false, _) => Err("walked, no jambs"),
                (false, true, false) => Err("walked, too narrow or low"),
            };
        }
        // the feet at `f` over the spot: through an opening from the feet to a gap's height above them
        let lowest = up.max(0.0) + 0.1;
        // (the body's width too: a line through the edge of a window is not a way through it)
        // (the middle of the body first: most heights fail there)
        let clears = |f: f32| [0.5 * SOLDIER_JUMP_GAP, 0.05, SOLDIER_JUMP_GAP - 0.1].iter().all(|&h| [0.0, -JUMP_HALF_WIDTH, JUMP_HALF_WIDTH].iter().all(|&sd| !ray_at(f + h, sd)));
        let Some(f) = (0..=((JUMP_UP + 0.1 - lowest) / 0.2).floor().max(0.0) as usize).map(|j| lowest + 0.2 * j as f32).find(|&f| f <= JUMP_UP + 0.1 && clears(f)) else {
            return Err("no opening a jump reaches");
        };
        if world.walk_blocked([e[0], e[1] + 1.0, e[2]], [e[0], e[1] + f + SOLDIER_JUMP_GAP - 0.1, e[2]]) {
            Err("a ceiling over the spot")
        } else if world.walk_blocked([l[0], e[1] + f + 0.05, l[2]], [l[0], l[1] + 0.3, l[2]]) {
            Err("something in the way down")
        } else {
            Ok((r, l, LinkKind::Jump))
        }
    }

    /// The centre of polygon `p` (the mean of its vertices).
    pub fn centre(&self, p: u32) -> [f32; 3] {
        let n = self.polys[p as usize].n as f32;
        let s = self.poly_verts(p).fold([0.0; 3], |s, v| [s[0] + v[0], s[1] + v[1], s[2] + v[2]]);
        s.map(|c| c / n)
    }

    /// The point of polygon `p` nearest to (x, z) across, with the polygon's height there.
    pub fn closest_on(&self, p: u32, x: f32, z: f32) -> [f32; 3] {
        let v: Vec<[f32; 3]> = self.poly_verts(p).collect();
        let n = v.len();
        let side = |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0]);
        let s: Vec<f32> = (0..n).map(|k| side(v[k], v[(k + 1) % n])).collect();
        if s.iter().all(|&d| d >= 0.0) || s.iter().all(|&d| d <= 0.0) {
            return [x, self.height_in(p, &v, x, z), z];
        }
        let mut best = (f32::MAX, v[0]);
        for k in 0..n {
            let (a, b) = (v[k], v[(k + 1) % n]);
            let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 0.0 { (((x - a[0]) * dx + (z - a[2]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let q = [a[0] + t * dx, a[1] + t * (b[1] - a[1]), a[2] + t * dz];
            let d = (q[0] - x).hypot(q[2] - z);
            if d < best.0 {
                best = (d, q);
            }
        }
        best.1
    }

    /// The height of polygon `p` at (x, z), a point inside it across: the terrain's on the terrain, its detail triangles'
    /// over objects, else that of its vertices' fan.
    pub fn height(&self, p: u32, x: f32, z: f32) -> f32 {
        let v: Vec<[f32; 3]> = self.poly_verts(p).collect();
        self.height_in(p, &v, x, z)
    }

    fn height_in(&self, p: u32, v: &[[f32; 3]], x: f32, z: f32) -> f32 {
        let q = self.polys[p as usize];
        if q.ground
            && let Some(h) = self.ground.as_ref().and_then(|g| g.at(x, z))
        {
            return h;
        }
        for t in &self.detail[q.detail as usize..(q.detail + q.ndetail) as usize] {
            if let Some(y) = tri_height(t[0], t[1], t[2], x, z) {
                return y;
            }
        }
        for k in 1..v.len() - 1 {
            if let Some(y) = tri_height(v[0], v[k], v[k + 1], x, z) {
                return y;
            }
        }
        v[0][1]
    }

    /// The polygon nearest to (x, y, z), within `r` m across and `dy` m up or down (across first: a quarter of the height
    /// difference counts as distance).
    pub fn nearest(&self, x: f32, y: f32, z: f32, r: f32, dy: f32) -> Option<u32> {
        let tm = self.tile_m();
        let side = self.side as i32;
        let t0 = |v: f32| (((v - r) / tm).floor() as i32).clamp(0, side - 1);
        let t1 = |v: f32| (((v + r) / tm).floor() as i32).clamp(0, side - 1);
        let mut best: Option<(f32, u32)> = None;
        for tz in t0(z)..=t1(z) {
            for tx in t0(x)..=t1(x) {
                let t = (tz * side + tx) as usize;
                for p in self.tile_first[t]..self.tile_first[t + 1] {
                    let q = self.closest_on(p, x, z);
                    let (dh, dv) = ((q[0] - x).hypot(q[2] - z), (q[1] - y).abs());
                    if dh > r || dv > dy {
                        continue;
                    }
                    let d = dh + 0.25 * dv;
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, p));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// The polygons joined to those near `seeds` (spawn points, flags), through edges, seams and links.
    pub fn reach(&self, seeds: &[[f32; 3]]) -> Vec<bool> {
        let mut on = vec![false; self.polys.len()];
        let mut stack: Vec<u32> = seeds.iter().filter_map(|s| self.nearest(s[0], s[1], s[2], 8.0, 3.0)).collect();
        for &p in &stack {
            on[p as usize] = true;
        }
        let mut nb = Vec::new();
        while let Some(p) = stack.pop() {
            self.neighbours(p, &mut nb);
            for &q in &nb {
                if !std::mem::replace(&mut on[q as usize], true) {
                    stack.push(q);
                }
            }
        }
        on
    }

    /// Polygon `p`'s area across (m²).
    pub fn area(&self, p: u32) -> f32 {
        let v: Vec<[f32; 3]> = self.poly_verts(p).collect();
        let mut a = 0.0;
        for k in 0..v.len() {
            let (p, q) = (v[k], v[(k + 1) % v.len()]);
            a += p[0] * q[2] - q[0] * p[2];
        }
        0.5 * a.abs()
    }

    /// A hash of the whole mesh, to check that it does not depend on the threads.
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv::new();
        for v in &self.verts {
            v.iter().for_each(|c| h.f32(*c));
        }
        self.idx.iter().chain(&self.nei).for_each(|&i| h.u64(i as u64));
        for p in &self.polys {
            h.u64(p.first as u64 | (p.n as u64) << 32 | (p.area as u64) << 40 | (p.ground as u64) << 48);
        }
        for s in &self.seams {
            h.u64(s.a as u64 | (s.b as u64) << 32);
        }
        for l in &self.links {
            h.u64(l.a as u64 | (l.b as u64) << 32);
            h.u64(l.kind as u64);
        }
        for t in &self.detail {
            t.iter().flatten().for_each(|c| h.f32(*c));
        }
        h.0
    }

    /// Its size in memory (bytes), roughly.
    pub fn bytes(&self) -> usize {
        self.verts.len() * 12 + self.polys.len() * std::mem::size_of::<Poly>() + (self.idx.len() + self.nei.len()) * 4 + self.seams.len() * 12 + self.detail.len() * 36
    }
}

/// The height of triangle (a, b, c) at (x, z), if (x, z) is inside it across.
fn tri_height(a: [f32; 3], b: [f32; 3], c: [f32; 3], x: f32, z: f32) -> Option<f32> {
    let (v0, v1, v2) = ([c[0] - a[0], c[2] - a[2]], [b[0] - a[0], b[2] - a[2]], [x - a[0], z - a[2]]);
    let den = v0[0] * v1[1] - v0[1] * v1[0];
    if den.abs() < 1e-9 {
        return None;
    }
    let u = (v1[1] * v2[0] - v1[0] * v2[1]) / den;
    let v = (v0[0] * v2[1] - v0[1] * v2[0]) / den;
    const E: f32 = 1e-4;
    (u >= -E && v >= -E && u + v <= 1.0 + E).then(|| a[1] + (c[1] - a[1]) * u + (b[1] - a[1]) * v)
}

/// Spawn points and flags inside the combat area, at their height: where the bots start and go.
pub fn seeds(world: &World) -> Vec<[f32; 3]> {
    let is_seed = |k: &str| k.eq_ignore_ascii_case("spawnpoint") || k.eq_ignore_ascii_case("controlpoint");
    let inside = |x: f64, z: f64| world.level.combat_area.is_none_or(|[cx, cz, w, h]| x >= cx && z >= cz && x < cx + w && z < cz + h);
    world.level.placed.iter().filter(|p| p.kind.as_deref().is_some_and(is_seed) && inside(p.pos[0], p.pos[2])).map(|p| p.pos.map(|v| v as f32)).collect()
}

/// What the mesh is built from, hashed with the parameters and the generator's version.
fn input_hash(world: &World, params: &Params) -> u64 {
    let mut h = Fnv::new();
    h.u64(GENERATOR as u64);
    h.bytes(&bincode::serialize(params).unwrap_or_default());
    // (the links' own measures)
    [SOLDIER_DROP, SOLDIER_DROP_MAX, DROP_SPACING, DROP_REACH, GROUND_BELOW, GROUND_ABOVE].iter().for_each(|c| h.f32(*c));
    let l = &world.level;
    h.f32(l.terrain.world_size as f32);
    h.f32(l.terrain.water_level as f32);
    let hm = l.heightmap.size;
    let hc = l.terrain.world_size as f32 / hm as f32;
    for j in 0..hm {
        for i in 0..hm {
            h.f32(world.height_at((i as f32 * hc) as f64, (j as f32 * hc) as f64).unwrap_or(f64::NAN) as f32);
        }
    }
    if let Some((w, m)) = &l.material_map {
        h.u64(*w as u64);
        h.bytes(m);
    }
    for o in world.obstacles() {
        h.bytes(o.name.as_bytes());
        o.rows.iter().flatten().chain(&o.pos).chain(&o.scale).for_each(|c| h.f32(*c));
        if let Some(m) = crate::obstacle::pick_lod(&o.lods, 1) {
            h.u64(m.vertices.len() as u64 | (m.faces.len() as u64) << 32);
            m.vertices.iter().flatten().for_each(|c| h.f32(*c));
            m.faces.iter().for_each(|f| {
                h.u64(f.v[0] as u64 | (f.v[1] as u64) << 32);
                h.u64(f.v[2] as u64 | (f.material as u64) << 32);
            });
        }
    }
    h.0
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn f32(&mut self, v: f32) {
        self.u64(v.to_bits() as u64);
    }
    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ x as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
}
