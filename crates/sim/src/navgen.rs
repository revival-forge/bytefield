//! Generates the level-0 blocking maps (1 m per pixel) for levels that do not ship them.
//!
//! It follows the rules each level declares in `AIpathFinding.con`
//! (`ai.addSearchMap name waterHeight waterDepth maxSlope brush cutLow cutHigh aiTypes`
//! and `ai.setMapSpawnPoints`), with which they are matched to the official maps:
//! 1. terrain: blocked if the slope exceeds the maximum, if the water covers more than the accepted depth
//!    or if it is of material [`NO_GO_MATERIAL`], the terrain outside the play area;
//! 2. objects: blocked where a collision face crosses the band [ground + low, ground + high];
//! 3. brush: what is blocked is widened by the mover's radius;
//! 4. fill: what cannot be reached from the seeds is left blocked.
//!
//! This module never reads the official maps: they are only the yardstick (`examples/navgen_vs_shipped.rs`).

use bytefield_formats::pathmap::PathMap;

use crate::World;
use crate::bot_api::Nav;

/// The terrain material the official maps do not leave free. No file says so: it comes from comparing
/// the 17 `bf1942` levels that have maps; it appears in 6 and there the official free share is 0 %, against 89–100 % for the
/// other materials. In a mod it could mean something else.
pub const NO_GO_MATERIAL: u32 = 7;

/// Where a generated map's parameters come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// the level's `AIpathFinding.con`
    Level,
    /// default values and seeds at the spawn points and the flags
    Defaults,
}

/// A map's rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// water depth that can still be crossed (m)
    pub water_depth: f64,
    /// maximum slope (degrees)
    pub max_slope: f64,
    /// mover's radius (m)
    pub brush: f64,
    /// the height band above the ground in which an object gets in the way (m)
    pub low_clip: f64,
    pub high_clip: f64,
    /// (x, z) from where it is filled; with none it is not filled
    pub seeds: Vec<[f64; 2]>,
    pub source: Source,
}

impl Params {
    /// The most common ones in `bf1942`'s `AIpathFinding.con` files.
    pub fn defaults(kind: Nav) -> Params {
        match kind {
            Nav::Infantry => Params { water_depth: 1.5, max_slope: 30.0, brush: 1.0, low_clip: 0.4, high_clip: 2.0, seeds: Vec::new(), source: Source::Defaults },
            Nav::Tank => Params { water_depth: 0.0, max_slope: 30.0, brush: 3.0, low_clip: 0.3, high_clip: 2.5, seeds: Vec::new(), source: Source::Defaults },
        }
    }

    /// A map's own in the text of an `AIpathFinding.con`; None if it does not declare them.
    pub fn from_con(text: &str, kind: Nav) -> Option<Params> {
        let prefix = match kind {
            Nav::Infantry => "infantry",
            Nav::Tank => "tank",
        };
        let lines = || text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>());
        let mut count = 0usize;
        let mut found = None;
        for w in lines() {
            if w.first().is_some_and(|c| c.eq_ignore_ascii_case("ai.addsearchmap")) {
                if found.is_none() && w.get(1).is_some_and(|n| n.to_ascii_lowercase().starts_with(prefix)) {
                    let num = |i: usize| w.get(i).and_then(|v| v.parse::<f64>().ok());
                    found = Some((count, num(3)?, num(4)?, num(5)?, num(6)?, num(7)?));
                }
                count += 1;
            }
        }
        let (index, water_depth, max_slope, brush, low_clip, high_clip) = found?;
        let mut seeds = Vec::new();
        for w in lines() {
            if w.first().is_some_and(|c| c.eq_ignore_ascii_case("ai.setmapspawnpoints")) && w.get(1).and_then(|v| v.parse::<usize>().ok()) == Some(index) {
                // separated by spaces or by commas (Bocage, Kharkov)
                for p in w[2..].iter().flat_map(|p| p.split(',')) {
                    if let Some(v) = bytefield_world::parse_vec(p).filter(|v| v.len() == 2) {
                        seeds.push([v[0], v[1]]);
                    }
                }
            }
        }
        Some(Params { water_depth, max_slope, brush, low_clip, high_clip, seeds, source: Source::Level })
    }

    /// The level's if it declares them; if not, the defaults with seeds at its spawns and flags.
    pub fn of_world(world: &World, kind: Nav) -> Params {
        let dir = world.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0);
        let declared = world.vfs.read(&format!("{dir}/AIpathFinding.con")).ok().and_then(|b| Params::from_con(&String::from_utf8_lossy(&b), kind));
        match declared {
            Some(mut p) => {
                // the level's alone fall short: a bridge or a narrow pass that comes out closed here
                // would leave half an island unreached (Market_Garden, Peenemunde)
                p.seeds.extend(placed_seeds(world));
                p
            }
            None => Params::defaults_for(world, kind),
        }
    }

    /// [`Params::defaults`] with the seeds deduced from the level.
    pub fn defaults_for(world: &World, kind: Nav) -> Params {
        Params { seeds: placed_seeds(world), ..Params::defaults(kind) }
    }
}

/// The positions of the spawn points and flags of the loaded mode.
fn placed_seeds(world: &World) -> Vec<[f64; 2]> {
    let is_seed = |k: &str| k.eq_ignore_ascii_case("spawnpoint") || k.eq_ignore_ascii_case("controlpoint");
    // those outside the combat zone do not count: nobody plays there
    let inside = |x: f64, z: f64| world.level.combat_area.is_none_or(|[cx, cz, w, h]| x >= cx && z >= cz && x < cx + w && z < cz + h);
    world.level.placed.iter().filter(|p| p.kind.as_deref().is_some_and(is_seed) && inside(p.pos[0], p.pos[2])).map(|p| [p.pos[0], p.pos[2]]).collect()
}

impl World {
    /// The bots' map of `kind`: the level's, or made once here when it ships none. `BYTEFIELD_NAVGEN=0`: not made.
    pub fn nav_map(&self, kind: Nav) -> Option<std::sync::Arc<PathMap>> {
        let (slot, name) = match kind {
            Nav::Infantry => (0, "Infantry1Level0Map.raw"),
            Nav::Tank => (1, "Tank0Level0Map.raw"),
        };
        self.nav_maps[slot]
            .get_or_init(|| {
                let dir = self.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0);
                if let Ok(b) = self.vfs.read(&format!("{dir}/Pathfinding/{name}")) {
                    return PathMap::parse(&b).ok().map(std::sync::Arc::new);
                }
                if !crate::debug::options().navgen {
                    return None;
                }
                Some(std::sync::Arc::new(generate(self, kind)))
            })
            .clone()
    }
}

impl World {
    /// Whether the level ships its own map of `kind` (the official ones, drawn with the level), not one made here.
    pub fn nav_map_shipped(&self, kind: Nav) -> bool {
        let name = match kind {
            Nav::Infantry => "Infantry1Level0Map.raw",
            Nav::Tank => "Tank0Level0Map.raw",
        };
        let dir = self.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0);
        self.vfs.read(&format!("{dir}/Pathfinding/{name}")).is_ok()
    }
}

/// The level-0 map of `kind`, generated with [`Params::of_world`].
pub fn generate(world: &World, kind: Nav) -> PathMap {
    generate_with(world, &Params::of_world(world, kind))
}

/// The level-0 map with these rules.
pub fn generate_with(world: &World, params: &Params) -> PathMap {
    let n = world.level.terrain.world_size as usize;
    let mut blocked = terrain_mask(world, n, params);
    stamp_obstacles(world, n, params, &mut blocked);
    finish(n, blocked, params)
}

/// How high over the water line a bank still takes [`swim_map`]'s own verdict, m (the official tank maps close a band of land along
/// the water, the brush's width, that a swimmer has to cross to come ashore).
pub const SWIM_BANK_M: f64 = 2.0;
/// Water at least this deep floats a swimmer, m: [`swim_map`]'s dear pixels.
pub const SWIM_DEEP_M: f64 = 1.0;

/// The tank map for what swims, and which of its pixels are water deep enough to float in. In deep water only the level's objects
/// (and the brush round them) close a pixel; on a bank up to [`SWIM_BANK_M`] over the water line, the objects and the tank's slope;
/// further up, the tank map as it is. `None` when the tank map is not the level's size.
pub fn swim_map(world: &World, tank: &PathMap) -> Option<(PathMap, Vec<bool>)> {
    let n = world.level.terrain.world_size as usize;
    if tank.width != n || tank.height != n || tank.pixels.len() != n * n {
        return None;
    }
    let mut params = Params::of_world(world, Nav::Tank);
    let min_up = params.max_slope.to_radians().cos();
    // the objects alone: no slope, no water
    params.max_slope = 90.0;
    params.water_depth = f64::INFINITY;
    params.seeds.clear();
    let objects = generate_with(world, &params);
    let water = world.level.terrain.water_level;
    let mut pixels = tank.pixels.clone();
    let mut deep = vec![false; n * n];
    for i in 0..n * n {
        let (x, z) = ((i % n) as f64, (i / n) as f64);
        let Some((h, normal)) = world.level.height_and_normal(x, z, bytefield_formats::image::Orientation::IDENTITY) else { continue };
        if h < water - SWIM_DEEP_M {
            deep[i] = true;
            pixels[i] = objects.pixels[i];
        } else if h < water + SWIM_BANK_M {
            pixels[i] = (objects.pixels[i] != 0 || normal[1] < min_up) as u8;
        }
    }
    Some((PathMap { width: n, height: n, bpp: tank.bpp, pixels }, deep))
}

/// Brush and fill, over what the terrain and objects have blocked.
pub fn finish(n: usize, blocked: Vec<u8>, params: &Params) -> PathMap {
    let mut blocked = dilate(n, &blocked, params.brush);
    flood(n, &mut blocked, &params.seeds);
    PathMap { width: n, height: n, bpp: 1, pixels: blocked }
}

/// What the terrain blocks: slope, water and material. Each pixel is measured at its whole (x, z) corner.
fn terrain_mask(world: &World, n: usize, params: &Params) -> Vec<u8> {
    let min_up = params.max_slope.to_radians().cos();
    let water = world.level.terrain.water_level;
    let mut out = vec![1u8; n * n];
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get());
    let rows = n.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (k, chunk) in out.chunks_mut(n * rows).enumerate() {
            s.spawn(move || {
                for (i, px) in chunk.iter_mut().enumerate() {
                    let (x, z) = ((i % n) as f64, (i / n + k * rows) as f64);
                    // outside the terrain grid (the last edge) it stays blocked
                    if let Some((h, normal)) = world.level.height_and_normal(x, z, bytefield_formats::image::Orientation::IDENTITY) {
                        *px = (normal[1] < min_up || h < water - params.water_depth || world.level.material_at(x, z) == NO_GO_MATERIAL) as u8;
                    }
                }
            });
        }
    });
    out
}

/// Marks the pixels where an object's collision face crosses the band over the ground.
fn stamp_obstacles(world: &World, n: usize, params: &Params, blocked: &mut [u8]) {
    // the fine LOD (if none, `pick_lod` gives 0): the one that best fits the official ones, on foot and in a tank
    const LOD: usize = 1;
    for o in world.obstacles() {
        let Some(mesh) = crate::obstacle::pick_lod(&o.lods, LOD) else { continue };
        let place = |v: [f32; 3]| {
            let p = [v[0] * o.scale[0], v[1] * o.scale[1], v[2] * o.scale[2]];
            [0, 1, 2].map(|c| o.rows[0][c] * p[0] + o.rows[1][c] * p[1] + o.rows[2][c] * p[2] + o.pos[c])
        };
        for f in &mesh.faces {
            let corners = f.v.map(|i| mesh.vertices.get(i as usize).copied().map(place));
            let [Some(a), Some(b), Some(c)] = corners else { continue };
            stamp_triangle(a, b, c, n, blocked, |x, z| world.height_at(x, z), params.low_clip, params.high_clip);
        }
    }
}

/// Walks the triangle every half metre and marks the pixel of each point that falls in the band.
#[allow(clippy::too_many_arguments)]
fn stamp_triangle(a: [f32; 3], b: [f32; 3], c: [f32; 3], n: usize, blocked: &mut [u8], ground: impl Fn(f64, f64) -> Option<f64>, low: f64, high: f64) {
    const STEP: f32 = 0.5;
    let dist = |p: [f32; 3], q: [f32; 3]| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
    let steps = (dist(a, b).max(dist(b, c)).max(dist(c, a)) / STEP).ceil().clamp(1.0, 4096.0) as usize;
    for i in 0..=steps {
        for j in 0..=steps - i {
            let (u, v) = (i as f32 / steps as f32, j as f32 / steps as f32);
            let w = 1.0 - u - v;
            let p = [0, 1, 2].map(|k| a[k] * u + b[k] * v + c[k] * w);
            let Some(g) = ground(p[0] as f64, p[2] as f64) else { continue };
            // rounded: pixels go with their corner, as in `terrain_mask`
            let (px, pz) = ((p[0] + 0.5) as usize, (p[2] + 0.5) as usize);
            if (low..=high).contains(&(p[1] as f64 - g)) && px < n && pz < n {
                blocked[pz * n + px] = 1;
            }
        }
    }
}

/// Widens what is blocked with a disc of radius `radius`.
fn dilate(n: usize, blocked: &[u8], radius: f64) -> Vec<u8> {
    let mut out = blocked.to_vec();
    let r = radius.ceil() as i64;
    if r <= 0 {
        return out;
    }
    let disc: Vec<(i64, i64)> = (-r..=r).flat_map(|z| (-r..=r).map(move |x| (x, z))).filter(|&(x, z)| ((x * x + z * z) as f64) <= radius * radius).collect();
    let n_i = n as i64;
    for z in 0..n_i {
        for x in 0..n_i {
            if blocked[(z * n_i + x) as usize] == 0 {
                continue;
            }
            // what is inside a blotch changes nothing
            let inner = x > 0 && z > 0 && x + 1 < n_i && z + 1 < n_i && [-1, 1, -n_i, n_i].iter().all(|d| blocked[(z * n_i + x + d) as usize] != 0);
            if inner {
                continue;
            }
            for &(dx, dz) in &disc {
                let (px, pz) = (x + dx, z + dz);
                if px >= 0 && pz >= 0 && px < n_i && pz < n_i {
                    out[(pz * n_i + px) as usize] = 1;
                }
            }
        }
    }
    out
}

/// Blocks the free cells not reached from the seeds (4 neighbours). A seed over something blocked
/// moves to the nearest free pixel within 32 m all round; without seeds nothing is touched.
fn flood(n: usize, blocked: &mut [u8], seeds: &[[f64; 2]]) {
    if seeds.is_empty() {
        return;
    }
    let mut reached = vec![false; n * n];
    let mut stack = Vec::new();
    for s in seeds {
        if let Some(i) = nearest_free(n, blocked, *s, 32)
            && !reached[i]
        {
            reached[i] = true;
            stack.push(i);
        }
    }
    while let Some(i) = stack.pop() {
        let (x, z) = (i % n, i / n);
        let neighbours = [(x > 0).then(|| i - 1), (x + 1 < n).then(|| i + 1), (z > 0).then(|| i - n), (z + 1 < n).then(|| i + n)];
        for j in neighbours.into_iter().flatten() {
            if blocked[j] == 0 && !reached[j] {
                reached[j] = true;
                stack.push(j);
            }
        }
    }
    for (px, r) in blocked.iter_mut().zip(&reached) {
        *px = (!*r) as u8;
    }
}

fn nearest_free(n: usize, blocked: &[u8], at: [f64; 2], reach: i64) -> Option<usize> {
    let (cx, cz) = (at[0].floor() as i64, at[1].floor() as i64);
    let mut best: Option<(i64, usize)> = None;
    for z in cz - reach..=cz + reach {
        for x in cx - reach..=cx + reach {
            if x < 0 || z < 0 || x as usize >= n || z as usize >= n {
                continue;
            }
            let i = z as usize * n + x as usize;
            let d = (x - cx).pow(2) + (z - cz).pow(2);
            if blocked[i] == 0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
    }
    best.map(|b| b.1)
}

/// The map as a `.raw` file read by [`PathMap::parse`]: 64 m patches, 1 bit per pixel, and the
/// whole single-valued patches as a reference to their fill words, like the official ones.
/// The side must be a power of 2 of at least 64.
pub fn encode(m: &PathMap) -> Vec<u8> {
    const PATCH_BITS: u32 = 6;
    assert!(m.width == m.height && m.width.is_power_of_two() && m.width >= 1 << PATCH_BITS, "side {}x{}", m.width, m.height);
    let side = 1usize << PATCH_BITS;
    let patches = m.width / side;
    let bits = patches.trailing_zeros() as i32;
    let mut out = Vec::new();
    for v in [bits, bits, PATCH_BITS as i32, 0, 0, 2, 0, -1] {
        out.extend(v.to_le_bytes());
    }
    for pz in 0..patches {
        for px in 0..patches {
            let mut words = vec![0u32; side * side / 32];
            for z in 0..side {
                for x in 0..side {
                    if m.pixels[(pz * side + z) * m.width + px * side + x] != 0 {
                        words[(z * side + x) / 32] |= 1 << ((z * side + x) % 32);
                    }
                }
            }
            if words.iter().all(|&w| w == 0) {
                out.extend(0i32.to_le_bytes());
            } else if words.iter().all(|&w| w == u32::MAX) {
                out.extend(1i32.to_le_bytes());
            } else {
                out.extend((-1i32).to_le_bytes());
                for w in words {
                    out.extend(w.to_le_bytes());
                }
            }
        }
    }
    out
}

/// How similar two maps of the same size are. `blocked` is what is blocked in the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Confusion {
    pub both_blocked: u64,
    pub both_free: u64,
    /// free in the reference, blocked in the generated one
    pub only_generated: u64,
    /// blocked in the reference, free in the generated one
    pub only_reference: u64,
}

impl Confusion {
    /// Pixel-by-pixel comparison; only the pixels `keep` accepts (x, z) count.
    pub fn of(generated: &PathMap, reference: &PathMap, keep: impl Fn(usize, usize) -> bool) -> Confusion {
        assert_eq!((generated.width, generated.height), (reference.width, reference.height));
        let mut c = Confusion::default();
        for z in 0..reference.height {
            for x in 0..reference.width {
                if !keep(x, z) {
                    continue;
                }
                let i = z * reference.width + x;
                match (generated.pixels[i] != 0, reference.pixels[i] != 0) {
                    (true, true) => c.both_blocked += 1,
                    (false, false) => c.both_free += 1,
                    (true, false) => c.only_generated += 1,
                    (false, true) => c.only_reference += 1,
                }
            }
        }
        c
    }

    pub fn total(&self) -> u64 {
        self.both_blocked + self.both_free + self.only_generated + self.only_reference
    }

    /// Fraction of equal pixels.
    pub fn agreement(&self) -> f64 {
        ratio(self.both_blocked + self.both_free, self.total())
    }

    /// Of what is blocked in the reference, how much was generated blocked.
    pub fn blocked_recall(&self) -> f64 {
        ratio(self.both_blocked, self.both_blocked + self.only_reference)
    }

    /// Of what is free in the reference, how much was generated free.
    pub fn free_recall(&self) -> f64 {
        ratio(self.both_free, self.both_free + self.only_generated)
    }

    /// Intersection over union of what is blocked.
    pub fn blocked_iou(&self) -> f64 {
        ratio(self.both_blocked, self.both_blocked + self.only_generated + self.only_reference)
    }

    /// Intersection over union of what is free.
    pub fn free_iou(&self) -> f64 {
        ratio(self.both_free, self.both_free + self.only_generated + self.only_reference)
    }

    /// The agreement a single-valued map would get, the better of the two.
    pub fn trivial(&self) -> f64 {
        let blocked = self.both_blocked + self.only_reference;
        ratio(blocked.max(self.total() - blocked), self.total())
    }
}

fn ratio(a: u64, b: u64) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(brush: f64, seeds: Vec<[f64; 2]>) -> Params {
        Params { brush, seeds, ..Params::defaults(Nav::Infantry) }
    }

    const CON: &str = "rem *** comment\nai.addSearchMap Tank0 0 0 25 3.0 0.3 2.5 0\nai.setMapSpawnPoints 0 10/20\nAI.addSearchMap Infantry1 0 1.5 40 1.0 0.4 2.0 1\nai.addSearchType Infantry 1 0\nai.setMapSpawnPoints 1 1/2 3.5/4,5/6\n";

    #[test]
    fn reads_the_level_rules() {
        let p = Params::from_con(CON, Nav::Infantry).unwrap();
        assert_eq!((p.water_depth, p.max_slope, p.brush, p.low_clip, p.high_clip), (1.5, 40.0, 1.0, 0.4, 2.0));
        assert_eq!(p.seeds, vec![[1.0, 2.0], [3.5, 4.0], [5.0, 6.0]]);
        assert_eq!(p.source, Source::Level);
        let t = Params::from_con(CON, Nav::Tank).unwrap();
        assert_eq!((t.max_slope, t.brush), (25.0, 3.0));
        assert_eq!(t.seeds, vec![[10.0, 20.0]]);
        assert_eq!(Params::from_con("rem nothing\n", Nav::Tank), None);
    }

    #[test]
    fn the_fill_blocks_what_cannot_be_reached() {
        // a wall at x = 4 splits an 8×8 map in two; the seed is on the left
        let n = 8;
        let mut b = vec![0u8; n * n];
        for z in 0..n {
            b[z * n + 4] = 1;
        }
        let m = finish(n, b.clone(), &params(0.0, vec![[1.0, 1.0]]));
        assert_eq!((m.get(3, 7), m.get(4, 0), m.get(5, 0)), (0, 1, 1));
        // without seeds nothing is filled
        let m = finish(n, b, &params(0.0, vec![]));
        assert_eq!((m.get(3, 7), m.get(4, 0), m.get(5, 0)), (0, 1, 0));
    }

    #[test]
    fn a_seed_on_something_blocked_moves_to_free_ground() {
        let n = 8;
        let mut b = vec![0u8; n * n];
        b[3 * n + 3] = 1;
        let m = finish(n, b, &params(0.0, vec![[3.2, 3.7]]));
        assert_eq!((m.get(3, 3), m.get(0, 0)), (1, 0));
    }

    #[test]
    fn the_brush_widens_as_a_disc() {
        let n = 9;
        let mut b = vec![0u8; n * n];
        b[4 * n + 4] = 1;
        let m = finish(n, b, &params(1.0, vec![]));
        assert_eq!((m.get(4, 4), m.get(3, 4), m.get(4, 5), m.get(3, 3), m.get(2, 4)), (1, 1, 1, 0, 0));
    }

    #[test]
    fn an_object_only_gets_in_the_way_inside_the_band() {
        let n = 16;
        let tri = |y: f32| ([2.0, y, 2.0], [6.0, y, 2.0], [2.0, y, 6.0]);
        let stamp = |y: f32| {
            let mut b = vec![0u8; n * n];
            let (p, q, r) = tri(y);
            stamp_triangle(p, q, r, n, &mut b, |_, _| Some(10.0), 0.4, 2.0);
            b.iter().filter(|&&v| v != 0).count()
        };
                assert_eq!(stamp(10.1), 0, "below the band it is stepped on");
        assert!(stamp(11.0) > 5, "in the band it is in the way");
                assert_eq!(stamp(13.0), 0, "above it you pass underneath");
    }

    #[test]
    fn the_file_reads_the_same_as_the_map() {
        // patches of 64: the first free, the second blocked, the others with a loose pixel or half full
        let n = 128;
        let mut pixels = vec![0u8; n * n];
        for z in 0..64 {
            for x in 64..128 {
                pixels[z * n + x] = 1;
            }
        }
        pixels[70 * n + 3] = 1;
        for z in 64..128 {
            for x in 64..96 {
                pixels[z * n + x] = 1;
            }
        }
        let m = PathMap { width: n, height: n, bpp: 1, pixels };
        let file = encode(&m);
        let back = PathMap::parse(&file).unwrap();
        assert_eq!((back.width, back.height, back.bpp), (n, n, 1));
        assert_eq!(back.pixels, m.pixels);
    }

    #[test]
    fn the_measure_is_not_fooled_by_a_single_valued_map() {
        let n = 10;
        let mut reference = vec![1u8; n * n];
        reference[0] = 0;
        let r = PathMap { width: n, height: n, bpp: 1, pixels: reference };
        let all_blocked = PathMap { width: n, height: n, bpp: 1, pixels: vec![1; n * n] };
        let c = Confusion::of(&all_blocked, &r, |_, _| true);
        assert_eq!(c.agreement(), 0.99);
        assert_eq!(c.trivial(), 0.99);
        assert_eq!(c.blocked_recall(), 1.0);
        assert_eq!((c.free_recall(), c.free_iou()), (0.0, 0.0));
        let only_corner = Confusion::of(&all_blocked, &r, |x, z| x == 0 && z == 0);
        assert_eq!(only_corner.total(), 1);
    }

    /// With the game's data, if present: what is generated resembles the official in the free part, not just globally.
    #[test]
    fn resembles_the_official_maps() {
        let game = bytefield_testdata::require!(Base);
        let world = World::load(&game, "bf1942", "Tobruk", "conquest").unwrap();
        let reference = PathMap::parse(&world.pathfinding_file("Infantry1Level0Map.raw").unwrap()).unwrap();
        let generated = generate(&world, Nav::Infantry);
        assert_eq!(PathMap::parse(&encode(&generated)).unwrap().pixels, generated.pixels);
        let c = Confusion::of(&generated, &reference, |_, _| true);
        assert!(c.agreement() > 0.99, "agreement {}", c.agreement());
                assert!(c.free_iou() > 0.9, "free IoU {}", c.free_iou());
    }
}
