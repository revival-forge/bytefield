//! The generation: the terrain and the level's collision meshes as one triangle soup per tile, through rerecast up to a
//! polygon mesh (and, if asked, its detail mesh), on several threads with a result that does not depend on their order.
use super::{NavMesh, Params, Poly, TileStats};
use crate::World;
use crate::navgen::NO_GO_MATERIAL;
use glam::{UVec3, Vec3, Vec3A};
use rerecast::{AreaType, BuildContoursFlags, DetailNavmesh, HeightfieldBuilder, PolygonNavmesh, TriMesh};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// An obstacle's collision mesh placed in the world, and whether each face is flat enough to stand on.
pub(crate) struct Solid {
    pub verts: Vec<Vec3A>,
    pub tris: Vec<UVec3>,
    pub walk: Vec<bool>,
    /// each face's outward normal's height in the world (−1 down .. 1 up), when the mesh's faces are wound consistently
    /// ([`wound_alike`]); `None` otherwise (its outside is not known)
    pub up: Option<Vec<f32>>,
    pub min: [f32; 2],
    pub max: [f32; 2],
}

/// Whether a collision mesh's faces are wound alike: no edge (by its ends' positions) is met twice the same way. Then its
/// faces' stored normals look out of the solid (the level's meshes are not closed, but where they are, the stored normals
/// look out: notes on the navigation mesh, `examples/face_orientation`).
pub fn wound_alike(mesh: &crate::obstacle::FaceMesh) -> bool {
    let v = &mesh.vertices;
    let key = |i: u32| v[i as usize].map(|c| (c * 1000.0).round() as i64);
    let mut seen = std::collections::HashSet::new();
    for f in &mesh.faces {
        if f.v.iter().any(|&i| i as usize >= v.len()) {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (key(f.v[k]), key(f.v[(k + 1) % 3]));
            if a != b && !seen.insert((a, b)) {
                return false;
            }
        }
    }
    true
}

/// Every obstacle's finer collision LOD placed in the world (the faces the soldier's own collision is tested against). A
/// face is stood on if it looks up and is flat enough; in a mesh not wound alike, if it is flat enough whichever way it
/// looks (else the underside of a thick slab, or of a roof, was a floor).
/// TODO: parked vehicles and guns (and ships that are vehicles, such as Midway's carriers) are not among them, so the mesh
/// does not see them: a quarter of the weighted time stuck on foot is next to one. Mark an area with a cost around each
/// object spawner, or rebuild the tile with the placed vehicle's collision.
pub(crate) fn place_solids(world: &World, p: &Params) -> Vec<Solid> {
    let min_up = libm::cosf(p.object_slope.to_radians());
    let mut out = Vec::new();
    let mut alike: std::collections::HashMap<usize, bool> = std::collections::HashMap::new();
    for o in world.obstacles() {
        let Some(mesh) = crate::obstacle::pick_lod(&o.lods, 1) else { continue };
        let oriented = *alike.entry(mesh as *const _ as usize).or_insert_with(|| wound_alike(mesh));
        let verts: Vec<Vec3A> = mesh.vertices.iter().map(|&v| Vec3A::from_array(super::place(o, v))).collect();
        let mut tris = Vec::new();
        let mut walk = Vec::new();
        let mut up = Vec::new();
        for f in &mesh.faces {
            if f.v.iter().any(|&i| i as usize >= verts.len()) {
                continue;
            }
            let [a, b, c] = f.v.map(|i| verts[i as usize]);
            let n = (b - a).cross(c - a);
            let l = n.length();
            if l < 1e-6 {
                continue;
            }
            // (the stored normal into the world: the inverse transpose of the placing, rows over scale)
            let nw = [0, 1, 2].map(|c| (0..3).map(|k| o.rows[k][c] * f.normal[k] / o.scale[k]).sum::<f32>());
            let u = nw[1] / (nw[0] * nw[0] + nw[1] * nw[1] + nw[2] * nw[2]).sqrt().max(1e-9);
            tris.push(UVec3::new(f.v[0], f.v[1], f.v[2]));
            walk.push(if oriented { u >= min_up } else { (n.y / l).abs() >= min_up });
            up.push(u);
        }
        if tris.is_empty() {
            continue;
        }
        let up = oriented.then_some(up);
        let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
        for v in &verts {
            min = [min[0].min(v.x), min[1].min(v.z)];
            max = [max[0].max(v.x), max[1].max(v.z)];
        }
        out.push(Solid { verts, tris, walk, up, min, max });
    }
    out
}

/// Whether point `c` (a polygon's middle) is shut inside a solid: going straight up from [`INSIDE_FROM_M`] above it, the
/// first face of a mesh wound alike that is not upright looks up (it is that solid's top, seen from inside). A mesh not
/// wound alike says nothing.
fn inside_solid(solids: &[Solid], bin: &[u32], c: [f32; 3]) -> bool {
    let (x, y0, z) = (c[0], c[1] + INSIDE_FROM_M, c[2]);
    let mut best: Option<(f32, f32)> = None;
    for &k in bin {
        let s = &solids[k as usize];
        let Some(up) = &s.up else { continue };
        if x < s.min[0] || x > s.max[0] || z < s.min[1] || z > s.max[1] {
            continue;
        }
        for (t, &u) in s.tris.iter().zip(up) {
            if u.abs() < 0.05 {
                continue;
            }
            let [a, b, d] = [t.x, t.y, t.z].map(|i| s.verts[i as usize]);
            // where the vertical through (x, z) meets the face, if it does
            let det = (b.z - d.z) * (a.x - d.x) + (d.x - b.x) * (a.z - d.z);
            if det.abs() < 1e-9 {
                continue;
            }
            let l1 = ((b.z - d.z) * (x - d.x) + (d.x - b.x) * (z - d.z)) / det;
            let l2 = ((d.z - a.z) * (x - d.x) + (a.x - d.x) * (z - d.z)) / det;
            let l3 = 1.0 - l1 - l2;
            if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                continue;
            }
            let y = l1 * a.y + l2 * b.y + l3 * d.y;
            if y > y0 && best.is_none_or(|b| y < b.0) {
                best = Some((y, u));
            }
        }
    }
    best.is_some_and(|b| b.1 > 0.0)
}

/// How far above a polygon's middle [`inside_solid`] starts looking up, m: a polygon on a ramp lies up to a few decimetres
/// under the ramp's face (0.33 m on a synthetic one of 31°), and a solid's top is at least a soldier's height above any
/// floor shut inside it.
const INSIDE_FROM_M: f32 = 1.0;

/// A tile's polygons in world coordinates, before the tiles are put together.
pub(crate) struct TileOut {
    pub verts: Vec<[f32; 3]>,
    /// per polygon: its vertices (indices into `verts`), the neighbour of each edge ([`NavMesh::nei`]'s encoding, local), its
    /// area and whether it lies on the terrain
    pub polys: Vec<(Vec<u16>, Vec<u16>, u8, bool)>,
    /// per polygon, its detail triangles (world corners), when asked
    pub detail: Vec<Vec<[[f32; 3]; 3]>>,
    pub stats: TileStats,
}

/// The height range of the whole level (terrain and solids), with room above for a soldier.
pub(crate) fn height_range(world: &World, solids: &[Solid], p: &Params) -> (f32, f32) {
    let ws = world.level.terrain.world_size as f32;
    let hm = world.level.heightmap.size;
    let hc = ws / hm as f32;
    let (mut ymin, mut ymax) = (f32::MAX, f32::MIN);
    for j in 0..hm {
        for i in 0..hm {
            if let Some(h) = world.height_at((i as f32 * hc) as f64, (j as f32 * hc) as f64) {
                ymin = ymin.min(h as f32);
                ymax = ymax.max(h as f32);
            }
        }
    }
    for s in solids {
        for v in &s.verts {
            ymin = ymin.min(v.y);
            ymax = ymax.max(v.y);
        }
    }
    // on the grid of `ch`, so every tile quantizes heights alike
    ((((ymin - 1.0) / p.ch).floor()) * p.ch, ymax + p.height + 1.0)
}

/// All the tiles (or those that touch `area`, x0, z0, x1, z1: the others are left empty), built on `threads` threads; the
/// result does not depend on the order they finish in.
pub(crate) fn build(world: &World, solids: &[Solid], p: &Params, threads: usize, area: Option<[f32; 4]>) -> NavMesh {
    let ws = world.level.terrain.world_size as f32;
    let tile_m = p.tile as f32 * p.cs;
    let side = (ws / tile_m).ceil() as usize;
    let (ymin, ymax) = height_range(world, solids, p);
    // the solids by tile, with the border
    let b = p.border() as f32 * p.cs;
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); side * side];
    for (k, s) in solids.iter().enumerate() {
        let t0 = |v: f32| (((v - b) / tile_m).floor().max(0.0) as usize).min(side - 1);
        let t1 = |v: f32| (((v + b) / tile_m).floor().max(0.0) as usize).min(side - 1);
        for tz in t0(s.min[1])..=t1(s.max[1]) {
            for tx in t0(s.min[0])..=t1(s.max[0]) {
                bins[tz * side + tx].push(k as u32);
            }
        }
    }
    let next = AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|sc| {
        for _ in 0..threads.max(1) {
            sc.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= side * side {
                        break;
                    }
                    let (tx, tz) = (i % side, i / side);
                    let skip = area.is_some_and(|a| {
                        let (x0, z0) = (tx as f32 * tile_m, tz as f32 * tile_m);
                        x0 > a[2] || z0 > a[3] || x0 + tile_m < a[0] || z0 + tile_m < a[1]
                    });
                    let t = if skip {
                        TileOut { verts: Vec::new(), polys: Vec::new(), detail: Vec::new(), stats: TileStats::default() }
                    } else {
                        build_tile(world, solids, &bins[i], p, tx, tz, ymin, ymax)
                    };
                    results.lock().unwrap().push((i, t));
                }
            });
        }
    });
    let mut tiles: Vec<(usize, TileOut)> = results.into_inner().unwrap();
    tiles.sort_by_key(|t| t.0);
    assemble(tiles.into_iter().map(|t| t.1).collect(), side, *p, ymin)
}

/// The tiles in one mesh: global vertices and polygons, in tile order.
fn assemble(tiles: Vec<TileOut>, side: usize, params: Params, ymin: f32) -> NavMesh {
    let mut m = NavMesh { params, side: side as u32, ymin, ..Default::default() };
    for (ti, t) in tiles.into_iter().enumerate() {
        let vbase = m.verts.len() as u32;
        let pbase = m.polys.len() as u32;
        m.tile_first.push(pbase);
        m.verts.extend_from_slice(&t.verts);
        for (k, (idx, nei, area, ground)) in t.polys.into_iter().enumerate() {
            let first = m.idx.len() as u32;
            m.idx.extend(idx.iter().map(|&i| vbase + i as u32));
            m.nei.extend(nei.iter().map(|&n| match n {
                PolygonNavmesh::NO_CONNECTION => super::NONE,
                n if n & 0x8000 != 0 => super::PORTAL | (n & 3) as u32,
                n => pbase + n as u32,
            }));
            let dfirst = m.detail.len() as u32;
            if let Some(d) = t.detail.get(k) {
                m.detail.extend_from_slice(d);
            }
            m.polys.push(Poly { first, n: idx.len() as u8, area, ground, tile: ti as u32, detail: dfirst, ndetail: m.detail.len() as u32 - dfirst });
        }
        m.stats.push(t.stats);
    }
    m.tile_first.push(m.polys.len() as u32);
    m.stitch();
    m
}

#[allow(clippy::too_many_arguments)]
fn build_tile(world: &World, solids: &[Solid], bin: &[u32], p: &Params, tx: usize, tz: usize, ymin: f32, ymax: f32) -> TileOut {
    let t = p.tile as usize;
    let bd = p.border();
    let x0 = (tx * t) as f32 * p.cs - bd as f32 * p.cs;
    let z0 = (tz * t) as f32 * p.cs - bd as f32 * p.cs;
    let span = (t + 2 * bd) as f32 * p.cs;
    let c0 = Instant::now();
    let mut tm = TriMesh::default();
    // the terrain, at the heightmap's own resolution
    let ws = world.level.terrain.world_size as f32;
    let hm = world.level.heightmap.size;
    let hc = ws / hm as f32;
    let water = world.level.terrain.water_level as f32;
    let min_up = libm::cosf(p.slope.to_radians());
    let i0 = ((x0 / hc).floor().max(0.0)) as usize;
    let j0 = ((z0 / hc).floor().max(0.0)) as usize;
    let i1 = (((x0 + span) / hc).ceil() as usize).min(hm - 1);
    let j1 = (((z0 + span) / hc).ceil() as usize).min(hm - 1);
    let at = |i: usize, j: usize| {
        let (x, z) = (i as f32 * hc, j as f32 * hc);
        Vec3A::new(x, world.height_at(x as f64, z as f64).unwrap_or(ymin as f64) as f32, z)
    };
    let cols = i1 + 1 - i0;
    for j in j0..=j1 {
        for i in i0..=i1 {
            tm.vertices.push(at(i, j));
        }
    }
    for j in 0..(j1 - j0) {
        for i in 0..(i1 - i0) {
            let q = [j * cols + i, j * cols + i + 1, (j + 1) * cols + i + 1, (j + 1) * cols + i].map(|v| v as u32);
            for tri in [[q[0], q[2], q[1]], [q[0], q[3], q[2]]] {
                let [a, b, c] = tri.map(|v| tm.vertices[v as usize]);
                let n = (b - a).cross(c - a).normalize_or_zero();
                let g = (a + b + c) / 3.0;
                // TODO: wading water and the no-go material are left out of the mesh, as navgen does; areas with a cost
                // (wading) may serve the bots better once they walk on it
                let deep = a.y.min(b.y).min(c.y) < water - p.water_depth;
                let ok = n.y.abs() >= min_up && !deep && world.level.material_at(g.x as f64, g.z as f64) != NO_GO_MATERIAL;
                tm.indices.push(UVec3::from_array(tri));
                tm.area_types.push(if ok { AreaType::DEFAULT_WALKABLE } else { AreaType::NOT_WALKABLE });
            }
        }
    }
    for &k in bin {
        let s = &solids[k as usize];
        let off = tm.vertices.len() as u32;
        tm.vertices.extend_from_slice(&s.verts);
        for (tri, &w) in s.tris.iter().zip(&s.walk) {
            tm.indices.push(*tri + off);
            tm.area_types.push(if w { AreaType::DEFAULT_WALKABLE } else { AreaType::NOT_WALKABLE });
        }
    }
    let aabb = rerecast::Aabb3d { min: Vec3::new(x0, ymin, z0), max: Vec3::new(x0 + span, ymax, z0 + span) };
    let (climb, height, radius) = (p.vx_climb(), p.vx_height(), p.vx_radius());
    let mut hf = HeightfieldBuilder { aabb, cell_size: p.cs, cell_height: p.ch }.build().expect("a heightfield");
    hf.rasterize_triangles(&tm, climb).expect("rasterized");
    hf.filter_low_hanging_walkable_obstacles(climb);
    // (Recast's ledge filter also cuts a span whose walkable neighbours differ by more than the climb, which leaves out any
    // slope steeper than atan(climb / 2 cs), 38.7° with 0.4 m and 0.25 m cells: the object ramps a soldier walks up to 46° went
    // with it, such as the 45° stairs of DC_LostVillage's base house, whose upper floor holds a spawn point and was cut off.
    // The filter is given the rise that slope makes over two cells; the triangles' own slope already leaves out what is
    // steeper, and the heightfield is joined with the real climb, so a step that high is still no way up.)
    let ledge = climb.max((2.0 * p.cs * p.object_slope.to_radians().tan() / p.ch).ceil() as u16);
    hf.filter_ledge_spans(height, ledge);
    hf.filter_walkable_low_height_spans(height);
    let c1 = Instant::now();
    let mut chf = hf.into_compact(height, climb).expect("a compact heightfield");
    chf.erode_walkable_area(radius);
    chf.build_distance_field();
    chf.build_regions(bd as u16, 8, 20).expect("regions");
    let c2 = Instant::now();
    let contours = chf.build_contours(1.3, (12.0 / p.cs) as u16, BuildContoursFlags::DEFAULT);
    let pm = contours.into_polygon_mesh(6).expect("a polygon mesh");
    let c3 = Instant::now();
    let o = pm.aabb.min;
    let verts: Vec<[f32; 3]> = pm.vertices.iter().map(|v| [o.x + v.x as f32 * pm.cell_size, o.y + v.y as f32 * pm.cell_height, o.z + v.z as f32 * pm.cell_size]).collect();
    let nvp = pm.max_vertices_per_polygon as usize;
    let row = |i: usize| {
        let row = &pm.polygons[i * nvp..(i + 1) * nvp];
        &row[..row.iter().take_while(|&&v| v != PolygonNavmesh::NO_INDEX).count()]
    };
    // on the terrain: every corner near it (the terrain gives the height inside); the others are over objects
    let ground: Vec<bool> = (0..pm.polygon_count())
        .map(|i| {
            row(i).iter().all(|&k| {
                let v = verts[k as usize];
                world.height_at(v[0] as f64, v[2] as f64).is_some_and(|h| (-super::GROUND_BELOW..=super::GROUND_ABOVE).contains(&(v[1] - h as f32)))
            })
        })
        .collect();
    // the detail triangles of those over objects only: with the terrain's too, El_Alamein took 30 s
    let objects: Vec<usize> = (0..pm.polygon_count()).filter(|&i| !ground[i]).collect();
    let dm = if p.detail && !objects.is_empty() {
        let mut sub = pm.clone();
        sub.polygons = objects.iter().flat_map(|&i| pm.polygons[i * nvp..(i + 1) * nvp].iter().copied()).collect();
        sub.polygon_neighbors = objects.iter().flat_map(|&i| pm.polygon_neighbors[i * nvp..(i + 1) * nvp].iter().copied()).collect();
        sub.flags = objects.iter().map(|&i| pm.flags[i]).collect();
        sub.regions = objects.iter().map(|&i| pm.regions[i]).collect();
        sub.areas = objects.iter().map(|&i| pm.areas[i]).collect();
        DetailNavmesh::new(&sub, &chf, 6.0 * p.cs, p.ch).ok()
    } else {
        None
    };
    let c4 = Instant::now();
    let mut polys = Vec::with_capacity(pm.polygon_count());
    let mut detail = vec![Vec::new(); pm.polygon_count()];
    for (i, &on) in ground.iter().enumerate() {
        let r = row(i);
        polys.push((r.to_vec(), pm.polygon_neighbors[i * nvp..i * nvp + r.len()].to_vec(), pm.areas[i].0, on));
    }
    if let Some(dm) = &dm {
        for (k, &i) in objects.iter().enumerate() {
            let sm = &dm.meshes[k];
            let vs = &dm.vertices[sm.base_vertex_index as usize..][..sm.vertex_count as usize];
            let ts = &dm.triangles[sm.base_triangle_index as usize..][..sm.triangle_count as usize];
            detail[i] = ts.iter().map(|tri| tri.map(|k| vs[k as usize].to_array())).collect();
        }
    }
    // the polygons shut inside a solid (the terrain under a closed building or a tower, between its walls) are left out
    let inside: Vec<bool> = polys.iter().map(|(r, ..)| {
        let n = r.len() as f32;
        let c = r.iter().fold([0.0f32; 3], |a, &k| {
            let v = verts[k as usize];
            [a[0] + v[0] / n, a[1] + v[1] / n, a[2] + v[2] / n]
        });
        inside_solid(solids, bin, c)
    }).collect();
    if inside.iter().any(|&x| x) {
        let mut new = vec![PolygonNavmesh::NO_CONNECTION; polys.len()];
        let mut k = 0u16;
        for (i, &x) in inside.iter().enumerate() {
            if !x {
                new[i] = k;
                k += 1;
            }
        }
        let old = std::mem::take(&mut polys);
        let old_detail = std::mem::take(&mut detail);
        for (i, ((r, nei, area, on), d)) in old.into_iter().zip(old_detail).enumerate() {
            if inside[i] {
                continue;
            }
            let nei = nei.into_iter().map(|n| if n != PolygonNavmesh::NO_CONNECTION && n & 0x8000 == 0 { new[n as usize] } else { n }).collect();
            polys.push((r, nei, area, on));
            detail.push(d);
        }
    }
    let stats = TileStats { secs: [(c1 - c0).as_secs_f32(), (c2 - c1).as_secs_f32(), (c3 - c2).as_secs_f32(), (c4 - c3).as_secs_f32()] };
    TileOut { verts, polys, detail, stats }
}
