//! What a moving object can run into besides the terrain: the level's objects with collision
//! physics, and the engine's segment-against-faces test (`SimpleCollisionMesh::
//! getDistanceToGeometry` + `checkFaceAndEdgeCollision`), as read from the original game.

use crate::vecmath::sub;
use std::sync::Arc;

use bytefield_formats::sm::{BspFace, BspNode, CollisionMesh};

use crate::vecmath::V3;

/// One collision mesh: its vertices, the BSP's faces (outward normals) and its box.
#[derive(Debug, Clone)]
pub struct FaceMesh {
    pub vertices: Vec<V3>,
    pub faces: Vec<BspFace>,
    pub nodes: Vec<BspNode>,
    pub bounds: [V3; 2],
}

impl FaceMesh {
    pub fn from_collision(c: &CollisionMesh) -> FaceMesh {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in &c.vertices {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
        }
        FaceMesh { vertices: c.vertices.clone(), faces: c.bsp_faces.clone(), nodes: c.bsp_nodes.clone(), bounds: [lo, hi] }
    }

    /// The BSP's colliding faces lookup: down the tree, into the child of each side the
    /// segment's ends are on (both if they straddle the plane); in the leaves, the faces turned
    /// against the segment (dir · n < 0), each once, in the order met.
    pub fn colliding_faces(&self, start: V3, end: V3) -> Vec<u32> {
        // (one list of marks for every call, told apart by a number: a mesh's worth of flags per line was what this cost)
        thread_local!(static SEEN: std::cell::RefCell<(Vec<u32>, u32)> = const { std::cell::RefCell::new((Vec::new(), 0)) });
        let mut out = Vec::new();
        if !self.nodes.is_empty() {
            SEEN.with_borrow_mut(|(seen, mark)| {
                if seen.len() < self.faces.len() {
                    seen.resize(self.faces.len(), 0);
                }
                *mark = mark.wrapping_add(1);
                if *mark == 0 {
                    seen.fill(0);
                    *mark = 1;
                }
                self.walk(0, start, end, sub(end, start), seen, *mark, &mut out);
            });
        }
        out
    }

    fn walk(&self, k: u32, s: V3, e: V3, d: V3, seen: &mut [u32], mark: u32, out: &mut Vec<u32>) {
        let Some(node) = self.nodes.get(k as usize) else { return };
        if node.faces.is_empty() {
            let (p, n) = (node.point, node.normal);
            let side = |q: V3| if n[2] * (q[2] - p[2]) + n[1] * (q[1] - p[1]) + (q[0] - p[0]) * n[0] < 0.0 { 2u8 } else { 1u8 };
            let (a, b) = (side(s), side(e));
            let both = if a == b { a } else { 3 };
            if both & 1 != 0
                && let Some(c) = node.front
            {
                self.walk(c, s, e, d, seen, mark, out);
            }
            if both & 2 != 0
                && let Some(c) = node.back
            {
                self.walk(c, s, e, d, seen, mark, out);
            }
            return;
        }
        for &f in &node.faces {
            let Some(face) = self.faces.get(f as usize) else { continue };
            if std::mem::replace(&mut seen[f as usize], mark) == mark {
                continue;
            }
            let n = face.normal;
            if d[2] * n[2] + d[1] * n[1] + d[0] * n[0] < 0.0 {
                out.push(f);
            }
        }
    }
}

/// The level's obstacles by the ground they stand on: which ones a segment may touch, without asking all of them (the
/// AI's lines of sight: thousands a second, on thousands of obstacles). Only says which to look at: the caller
/// tests each as before, and they come in the order of the slice, so nothing changes but the time.
pub struct ObstacleGrid {
    origin: [f32; 2],
    n: [usize; 2],
    /// where each cell's list starts in `items`, and one more at the end
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl ObstacleGrid {
    const CELL: f32 = 32.0;

    /// `margin`: how much further than its bounding sphere an obstacle may be looked for.
    pub fn new(obstacles: &[Obstacle], margin: f32) -> ObstacleGrid {
        let reach = |o: &Obstacle| o.radius + margin;
        let finite: Vec<usize> = (0..obstacles.len()).filter(|&i| obstacles[i].center[0].is_finite() && obstacles[i].center[2].is_finite() && obstacles[i].radius.is_finite()).collect();
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for &i in &finite {
            let o = &obstacles[i];
            let r = reach(o);
            for (k, c) in [o.center[0], o.center[2]].into_iter().enumerate() {
                lo[k] = lo[k].min(c - r);
                hi[k] = hi[k].max(c + r);
            }
        }
        if finite.is_empty() {
            return ObstacleGrid { origin: [0.0; 2], n: [1, 1], starts: vec![0, 0], items: vec![] };
        }
        let n = [0, 1].map(|k| (((hi[k] - lo[k]) / Self::CELL) as usize + 1).min(4096));
        let mut g = ObstacleGrid { origin: lo, n, starts: vec![0; n[0] * n[1] + 1], items: vec![] };
        let cells = |g: &ObstacleGrid, o: &Obstacle| {
            let r = reach(o);
            let (a, b) = (g.cell(0, o.center[0] - r), g.cell(0, o.center[0] + r));
            let (c, d) = (g.cell(1, o.center[2] - r), g.cell(1, o.center[2] + r));
            (a, b, c, d)
        };
        for &i in &finite {
            let (a, b, c, d) = cells(&g, &obstacles[i]);
            for x in a..=b {
                for z in c..=d {
                    g.starts[x * n[1] + z + 1] += 1;
                }
            }
        }
        for k in 1..g.starts.len() {
            g.starts[k] += g.starts[k - 1];
        }
        g.items = vec![0; *g.starts.last().unwrap() as usize];
        let mut next = g.starts.clone();
        for &i in &finite {
            let (a, b, c, d) = cells(&g, &obstacles[i]);
            for x in a..=b {
                for z in c..=d {
                    let at = &mut next[x * n[1] + z];
                    g.items[*at as usize] = i as u32;
                    *at += 1;
                }
            }
        }
        g
    }

    fn cell(&self, axis: usize, v: f32) -> usize {
        (((v - self.origin[axis]) / Self::CELL).max(0.0) as usize).min(self.n[axis] - 1)
    }

    /// Into `out`, sorted and once each: the obstacles whose sphere (with the margin), seen from above, the segment
    /// start → start + dir may touch. May bring more; never leaves one out.
    pub fn along(&self, start: V3, dir: V3, out: &mut Vec<u32>) {
        out.clear();
        let end = [start[0] + dir[0], start[2] + dir[2]];
        if !(start[0].is_finite() && start[2].is_finite() && end[0].is_finite() && end[1].is_finite()) {
            return;
        }
        // from left to right; each column gets the piece of the segment in it, half a metre wider (what rounding may move)
        let (l, r) = if start[0] <= end[0] { ([start[0], start[2]], end) } else { (end, [start[0], start[2]]) };
        let (a, b) = (self.cell(0, l[0]), self.cell(0, r[0]));
        let width = r[0] - l[0];
        for x in a..=b {
            let z_at = |v: f32| l[1] + (r[1] - l[1]) * ((v - l[0]) / width).clamp(0.0, 1.0);
            let (p, q) = (if x == a { l[1] } else { z_at(self.origin[0] + x as f32 * Self::CELL - 0.5) }, if x == b { r[1] } else { z_at(self.origin[0] + (x + 1) as f32 * Self::CELL + 0.5) });
            let (c, d) = (self.cell(1, p.min(q) - 0.5), self.cell(1, p.max(q) + 0.5));
            let (from, to) = (self.starts[x * self.n[1] + c] as usize, self.starts[x * self.n[1] + d + 1] as usize);
            out.extend_from_slice(&self.items[from..to]);
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// A hit of a segment on a mesh, in the world.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Hit {
    pub normal: V3,
    pub point: V3,
    /// signed distance of the segment's end to the face's plane (≤ 0: the end is behind it)
    pub depth: f32,
    /// (t − 1) · |segment|: how far before its end the segment meets the face (≤ 0)
    pub along: f32,
    pub material: u32,
}

/// `checkFaceAndEdgeCollision(a, b, c, n, p0, p1, …, double_sided)`: the segment p0 → p1 goes
/// through the triangle from the front (p0 in front of the plane, p1 behind it). Returns the
/// point, the end's distance to the plane and (t − 1)·|p1 − p0|.
#[allow(clippy::too_many_arguments)]
pub fn face_hit(a: V3, b: V3, c: V3, n: V3, p0: V3, p1: V3, double_sided: bool) -> Option<(V3, f32, f32)> {
    let d = sub(p1, p0);
    if d == [0.0; 3] {
        return None;
    }
    let end = (p1[1] - a[1]) * n[1] + (p1[0] - a[0]) * n[0] + (p1[2] - a[2]) * n[2];
    if end > 0.0 {
        return None;
    }
    let start = (p0[0] - a[0]) * n[0] + (p0[1] - a[1]) * n[1] + (p0[2] - a[2]) * n[2];
    if start <= 0.0 {
        return None;
    }
    let dn = n[2] * d[2] + n[1] * d[1] + n[0] * d[0];
    if !(double_sided || dn < 0.0) {
        return None;
    }
    let t = start / -dn;
    // project on the plane of the two axes the normal leans on least
    let (u, w, k) = if n[1].abs() >= 0.7 {
        (0, 2, 1)
    } else if n[2].abs() <= 0.3 {
        (1, 2, 0)
    } else {
        (0, 1, 2)
    };
    let mut p = [0.0f32; 3];
    p[u] = t * d[u] + p0[u];
    p[w] = t * d[w] + p0[w];
    let e1 = (b[w] - a[w]) * (p[u] - a[u]) - (b[u] - a[u]) * (p[w] - a[w]);
    let e2 = (c[w] - b[w]) * (p[u] - b[u]) - (c[u] - b[u]) * (p[w] - b[w]);
    if !((0.0 <= e1 || e2 <= 0.0) && (e1 <= 0.0 || 0.0 <= e2)) {
        return None;
    }
    let e3 = (a[w] - c[w]) * (p[u] - c[u]) - (a[u] - c[u]) * (p[w] - c[w]);
    if !((0.0 <= e1 || e3 <= 0.0) && (e1 <= 0.0 || 0.0 <= e3)) {
        return None;
    }
    p[k] = t * d[k] + p0[k];
    let len = (d[2] * d[2] + d[0] * d[0] + d[1] * d[1]).sqrt();
    Some((p, end, (t - 1.0) * len))
}

/// A placed object part the movers can hit.
#[derive(Debug, Clone)]
pub struct Obstacle {
    /// its ObjectTemplate
    pub name: String,
    /// world placement: axes (rows) and position; `geometry.scale` per axis
    pub rows: [V3; 3],
    pub pos: V3,
    pub scale: V3,
    /// collision LODs (0: the coarse one movers' vertices use; 1: the finer one small movers
    /// are tested against)
    pub lods: Vec<Arc<FaceMesh>>,
    /// a bounding sphere in the world, for the broad phase
    pub center: V3,
    pub radius: f32,
    /// its template is of class `Obstacle` (barbed wire, tank traps…): a vehicle's
    /// `handleCollision` refuses it, so a vehicle that hits it fast enough goes through
    pub obstacle_class: bool,
    /// The obstacle template's damage (5 by default): what it does to a soldier that runs
    /// into it, once a second
    pub damage: f32,
    /// `addToCollisionGroup c_CGProjectiles` (barbed wire, sandbag walls…): the projectiles
    /// share the group, and two objects of one group are not tested (`checkObjectVsObjects`)
    pub projectiles_pass: bool,
    /// the box of its geometry (the `.sm`'s header; a ladder's top and bottom)
    pub bounds: [V3; 2],
    /// in a projectile's tree (GC's shield): that projectile (`projectiles`), which it moves with
    /// and does not run into
    pub carrier: Option<usize>,
}

/// What can be run into: the level's obstacles ([`crate::World::obstacles`], fixed) and those
/// in the trees of the projectiles flying now ([`crate::Sim::carried`]), placed for this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct Obstacles<'a> {
    pub fixed: &'a [Obstacle],
    pub carried: &'a [Obstacle],
}

impl<'a> Obstacles<'a> {
    pub fn iter(self) -> impl Iterator<Item = &'a Obstacle> {
        self.fixed.iter().chain(self.carried)
    }
}

impl<'a> From<&'a [Obstacle]> for Obstacles<'a> {
    fn from(fixed: &'a [Obstacle]) -> Obstacles<'a> {
        Obstacles { fixed, carried: &[] }
    }
}

impl Obstacle {
    /// `getDistanceToGeometry(double_sided, transform, start, dir, …)`: the segment
    /// start → start + dir against collision LOD `lod`; the face met first wins.
    pub fn segment(&self, lod: usize, start: V3, dir: V3, double_sided: bool) -> Option<Hit> {
        let mesh = pick_lod(&self.lods, lod)?;
        segment_on(mesh, &self.rows, self.pos, self.scale, start, dir, double_sided)
    }
}

/// `getVertexCollision(lod)`: the LOD asked for if the mesh has it, else LOD 0.
pub fn pick_lod(lods: &[Arc<FaceMesh>], lod: usize) -> Option<&FaceMesh> {
    lods.get(lod).or_else(|| lods.first()).map(|m| m.as_ref())
}

/// `getDistanceToGeometry` on a mesh placed in the world (axes `rows`, position `pos`,
/// `geometry.scale`): the segment start → start + dir; the face met first wins.
pub fn segment_on(mesh: &FaceMesh, rows: &[V3; 3], pos: V3, scale: V3, start: V3, dir: V3, double_sided: bool) -> Option<Hit> {
    segment_on_skipping(mesh, rows, pos, scale, start, dir, double_sided, None)
}

/// [`segment_on`] passing over the faces of material `skip` (`getDistanceToGeometry` with a
/// material slot holding 0: projectiles go through the faces of material 90).
#[allow(clippy::too_many_arguments)]
/// Whether [`segment_on_skipping`] would find something: the first face met ends it, not the nearest (the AI's lines of
/// sight only ask this).
pub fn segment_blocked_skipping(mesh: &FaceMesh, rows: &[V3; 3], pos: V3, scale: V3, start: V3, dir: V3, double_sided: bool, skip: Option<u32>) -> bool {
    let r = rows;
    let q = sub(start, pos);
    let mut s = [
        r[0][2] * q[2] + r[0][0] * q[0] + r[0][1] * q[1],
        r[1][0] * q[0] + r[1][1] * q[1] + r[1][2] * q[2],
        r[2][0] * q[0] + q[1] * r[2][1] + q[2] * r[2][2],
    ];
    let mut dl = [
        r[0][2] * dir[2] + r[0][1] * dir[1] + r[0][0] * dir[0],
        r[1][0] * dir[0] + r[1][1] * dir[1] + r[1][2] * dir[2],
        dir[0] * r[2][0] + dir[1] * r[2][1] + dir[2] * r[2][2],
    ];
    if scale != [1.0; 3] {
        for i in 0..3 {
            s[i] /= scale[i];
            dl[i] /= scale[i];
        }
    }
    let e = [dl[0] + s[0], dl[1] + s[1], dl[2] + s[2]];
    let [lo, hi] = mesh.bounds;
    for i in 0..3 {
        if s[i].max(e[i]) < lo[i] || hi[i] < s[i].min(e[i]) {
            return false;
        }
    }
    mesh.colliding_faces(s, e).into_iter().filter_map(|i| mesh.faces.get(i as usize)).filter(|f| Some(f.material) != skip).any(|f| {
        let v = |i: u32| mesh.vertices.get(i as usize).copied();
        let (Some(a), Some(b), Some(c)) = (v(f.v[0]), v(f.v[1]), v(f.v[2])) else { return false };
        face_hit(a, b, c, f.normal, s, e, double_sided).is_some()
    })
}

pub fn segment_on_skipping(mesh: &FaceMesh, rows: &[V3; 3], pos: V3, scale: V3, start: V3, dir: V3, double_sided: bool, skip: Option<u32>) -> Option<Hit> {
    let r = rows;
    let q = sub(start, pos);
    let mut s = [
        r[0][2] * q[2] + r[0][0] * q[0] + r[0][1] * q[1],
        r[1][0] * q[0] + r[1][1] * q[1] + r[1][2] * q[2],
        r[2][0] * q[0] + q[1] * r[2][1] + q[2] * r[2][2],
    ];
    let mut dl = [
        r[0][2] * dir[2] + r[0][1] * dir[1] + r[0][0] * dir[0],
        r[1][0] * dir[0] + r[1][1] * dir[1] + r[1][2] * dir[2],
        dir[0] * r[2][0] + dir[1] * r[2][1] + dir[2] * r[2][2],
    ];
    let scaled = scale != [1.0; 3];
    if scaled {
        for i in 0..3 {
            s[i] /= scale[i];
            dl[i] /= scale[i];
        }
    }
    let e = [dl[0] + s[0], dl[1] + s[1], dl[2] + s[2]];
    let [lo, hi] = mesh.bounds;
    for i in 0..3 {
        if s[i].max(e[i]) < lo[i] || hi[i] < s[i].min(e[i]) {
            return None;
        }
    }
    let mut best: Option<(V3, f32, f32, &BspFace)> = None;
    for f in mesh.colliding_faces(s, e).into_iter().filter_map(|i| mesh.faces.get(i as usize)).filter(|f| Some(f.material) != skip) {
        let v = |i: u32| mesh.vertices.get(i as usize).copied();
        let (Some(a), Some(b), Some(c)) = (v(f.v[0]), v(f.v[1]), v(f.v[2])) else { continue };
        if let Some((p, depth, along)) = face_hit(a, b, c, f.normal, s, e, double_sided)
            && best.as_ref().is_none_or(|bst| along < bst.2)
        {
            best = Some((p, depth, along, f));
        }
    }
    let (mut p, depth, along, f) = best?;
    let mut n = f.normal;
    if scaled {
        for i in 0..3 {
            p[i] *= scale[i];
            n[i] *= scale[i];
        }
    }
    let to_world = |v: V3| [r[0][0] * v[0] + r[1][0] * v[1] + r[2][0] * v[2], r[0][1] * v[0] + r[1][1] * v[1] + r[2][1] * v[2], v[0] * r[0][2] + v[1] * r[1][2] + v[2] * r[2][2]];
    let pw = to_world(p);
    Some(Hit { normal: to_world(n), point: [pw[0] + pos[0], pw[1] + pos[1], pw[2] + pos[2]], depth, along, material: f.material })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_through_a_floor_from_above() {
        // a triangle in y = 0 facing up (the engine's winding: normal = −(b − a) × (c − a))
        let (a, b, c) = ([0.0, 0.0, 0.0], [0.0, 0.0, 10.0], [10.0, 0.0, 0.0]);
        let hit = face_hit(a, b, c, [0.0, 1.0, 0.0], [1.0, 2.0, 1.0], [1.0, -1.0, 1.0], false).unwrap();
        assert_eq!(hit.0, [1.0, 0.0, 1.0]);
        assert_eq!(hit.1, -1.0);
        assert!((hit.2 + 1.0).abs() < 1e-6);
        // from below, single sided: nothing
        assert!(face_hit(a, b, c, [0.0, 1.0, 0.0], [1.0, -1.0, 1.0], [1.0, 2.0, 1.0], false).is_none());
        // outside the triangle
        assert!(face_hit(a, b, c, [0.0, 1.0, 0.0], [9.0, 2.0, 9.0], [9.0, -1.0, 9.0], false).is_none());
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    fn obstacle(center: V3, radius: f32) -> Obstacle {
        Obstacle { name: String::new(), rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], pos: center, scale: [1.0; 3], lods: vec![], center, radius, obstacle_class: false, damage: 0.0, projectiles_pass: false, bounds: [[0.0; 3]; 2], carrier: None }
    }

    /// Every obstacle the whole list finds, the grid brings, whatever the segment's slope or length, in or out of the level.
    #[test]
    fn the_grid_leaves_none_out() {
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        let mut r = move |lo: f32, hi: f32| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            lo + (hi - lo) * ((x >> 40) as f32 / (1u64 << 24) as f32)
        };
        let list: Vec<Obstacle> = (0..800).map(|i| obstacle([r(-500.0, 1500.0), r(0.0, 80.0), r(-500.0, 1500.0)], if i % 50 == 0 { r(30.0, 150.0) } else { r(0.3, 12.0) })).collect();
        let grid = ObstacleGrid::new(&list, 1.0);
        let (mut out, mut found) = (Vec::new(), 0);
        for k in 0..20000 {
            let a = [r(-700.0, 1700.0), r(0.0, 80.0), r(-700.0, 1700.0)];
            // long and short, along an axis or nearly
            let len = if k % 3 == 0 { 2000.0 } else { 150.0 };
            let mut d = [r(-len, len), r(-20.0, 20.0), r(-len, len)];
            match k % 10 {
                0 => d[0] = 0.0,
                1 => d[2] = 0.0,
                2 => d[0] *= 1e-4,
                3 => d[2] *= 1e-4,
                4 => (d[0], d[2]) = (0.0, 0.0),
                _ => {}
            }
            grid.along(a, d, &mut out);
            assert!(out.windows(2).all(|w| w[0] < w[1]), "sorted and once each");
            for (i, o) in list.iter().enumerate() {
                if crate::projectile::segment_near_sphere(a, d, o.center, o.radius + 1.0) {
                    found += 1;
                    assert!(out.binary_search(&(i as u32)).is_ok(), "obstacle {i} at {:?} r {} left out of {a:?} + {d:?}", o.center, o.radius);
                }
            }
        }
        assert!(found > 1000, "the test tests something: {found}");
    }
}
