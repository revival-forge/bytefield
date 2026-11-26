//! The queries over the walkable-surface mesh: its graph of polygons (across their edges, the tiles' seams and the links),
//! the zones it splits into, the polygon under a point, an A* over polygons that can be continued tick after tick, the
//! straightest way through the polygons it finds (the funnel), lines over the mesh and distance fields.
//!
//! Everything that decides is deterministic: costs in whole centimetres, ties broken by the polygon's number, nothing
//! iterated out of a hash map.
use super::{LinkKind, NavMesh, NONE, PORTAL, SOLDIER_DROP};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// How a step from one polygon to the next is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// across an edge the two share in a tile
    Edge,
    /// across the tiles' border ([`NavMesh::seams`])
    Seam,
    /// by a link off the mesh ([`NavMesh::links`], by its number): a ladder or a drop
    Link(u32),
}

/// A step to a neighbouring polygon and the opening it goes through, `left` and `right` as seen going across; through a
/// link, `left` is where it is taken and `right` where it lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub to: u32,
    pub via: Via,
    pub left: [f32; 3],
    pub right: [f32; 3],
}

impl Step {
    /// Where a path crosses it when it goes through the middle (the landing, through a link).
    pub fn mid(&self) -> [f32; 3] {
        match self.via {
            Via::Link(_) => self.right,
            _ => [0.5 * (self.left[0] + self.right[0]), 0.5 * (self.left[1] + self.right[1]), 0.5 * (self.left[2] + self.right[2])],
        }
    }
}

/// What a climb or a drop costs beyond its length (m): a ladder is climbed at a third of the walking speed; a drop is quick,
/// unless it hurts.
const LADDER_FACTOR: f32 = 3.0;
const DROP_COST_M: f32 = 2.0;
const HURTING_DROP_COST_M: f32 = 40.0;
/// A jump may fail (a run-up too short, a body that catches on the sill): it costs as some walking more.
const JUMP_COST_M: f32 = 10.0;

/// The mesh with what the queries need: its steps by polygon (and the steps into each, for the distance fields) and its
/// zones.
#[derive(Debug, Clone)]
pub struct Query {
    pub mesh: std::sync::Arc<NavMesh>,
    first: Vec<u32>,
    steps: Vec<Step>,
    /// the steps into each polygon (indices into `steps`, and where each starts)
    into_first: Vec<u32>,
    into: Vec<(u32, u32)>,
    /// the zone of each polygon: those joined by any way (a drop counts, though one way only), numbered from 1 in the order
    /// of their lowest polygon
    pub zone: Vec<u32>,
    /// each zone's area across (m²; index 0 unused)
    pub zone_area: Vec<f32>,
}

/// Twice the signed area of the triangle (a, b, c) across: positive when c is to the right of a → b (x across, z ahead).
fn tri_area2(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let (ax, az) = (b[0] - a[0], b[2] - a[2]);
    let (bx, bz) = (c[0] - a[0], c[2] - a[2]);
    bx * az - ax * bz
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

fn cm(m: f32) -> u32 {
    (m * 100.0).round().max(0.0) as u32
}

impl Query {
    pub fn new(mesh: std::sync::Arc<NavMesh>) -> Query {
        let m = &*mesh;
        let n = m.polys.len();
        let mut by_a: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut by_b: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (i, l) in m.links.iter().enumerate() {
            by_a[l.a as usize].push(i as u32);
            if l.both_ways() {
                by_b[l.b as usize].push(i as u32);
            }
        }
        let mut first = Vec::with_capacity(n + 1);
        let mut steps = Vec::new();
        for p in 0..n as u32 {
            first.push(steps.len() as u32);
            let q = m.polys[p as usize];
            let c = m.centre(p);
            let at = |k: usize| m.verts[m.idx[q.first as usize + k % q.n as usize] as usize];
            let mut push = |to: u32, via: Via, l: [f32; 3], r: [f32; 3]| {
                // (left and right as seen from the polygon's centre going across)
                let (l, r) = if matches!(via, Via::Link(_)) || tri_area2(c, l, r) >= 0.0 { (l, r) } else { (r, l) };
                steps.push(Step { to, via, left: l, right: r });
            };
            for k in 0..q.n as usize {
                let nb = m.nei[q.first as usize + k];
                if nb < PORTAL {
                    push(nb, Via::Edge, at(k), at(k + 1));
                }
            }
            let s0 = m.seams.partition_point(|s| s.a < p);
            for s in m.seams[s0..].iter().take_while(|s| s.a == p) {
                if let Some((l, r)) = seam_portal(m, p, s.edge as usize, s.b) {
                    push(s.b, Via::Seam, l, r);
                }
            }
            for &i in &by_a[p as usize] {
                let l = m.links[i as usize];
                push(l.b, Via::Link(i), l.from, l.to);
            }
            for &i in &by_b[p as usize] {
                let l = m.links[i as usize];
                push(l.a, Via::Link(i), l.to, l.from);
            }
        }
        first.push(steps.len() as u32);
        // the steps into each polygon
        let mut count = vec![0u32; n + 1];
        for s in &steps {
            count[s.to as usize + 1] += 1;
        }
        for i in 0..n {
            count[i + 1] += count[i];
        }
        let into_first = count.clone();
        let mut into = vec![(0u32, 0u32); steps.len()];
        let mut fill = count;
        for p in 0..n {
            for k in first[p]..first[p + 1] {
                let t = steps[k as usize].to as usize;
                into[fill[t] as usize] = (p as u32, k);
                fill[t] += 1;
            }
        }
        // the zones: union–find over every step, numbered by their lowest polygon
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn root(parent: &mut [u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                parent[x as usize] = parent[parent[x as usize] as usize];
                x = parent[x as usize];
            }
            x
        }
        for p in 0..n {
            for k in first[p]..first[p + 1] {
                let (a, b) = (root(&mut parent, p as u32), root(&mut parent, steps[k as usize].to));
                if a != b {
                    let (lo, hi) = (a.min(b), a.max(b));
                    parent[hi as usize] = lo;
                }
            }
        }
        let mut zone = vec![0u32; n];
        let mut zone_area = vec![0.0f32];
        let mut number = vec![0u32; n];
        for (p, zp) in zone.iter_mut().enumerate() {
            let r = root(&mut parent, p as u32) as usize;
            if number[r] == 0 {
                zone_area.push(0.0);
                number[r] = zone_area.len() as u32 - 1;
            }
            *zp = number[r];
            zone_area[number[r] as usize] += m.area(p as u32);
        }
        Query { mesh, first, steps, into_first, into, zone, zone_area }
    }

    pub fn steps(&self, p: u32) -> &[Step] {
        &self.steps[self.first[p as usize] as usize..self.first[p as usize + 1] as usize]
    }

    /// The polygons whose outline holds (x, z) across, any height.
    pub fn polys_at(&self, x: f32, z: f32) -> Vec<u32> {
        let m = &*self.mesh;
        let Some(t) = self.tile_of(x, z) else { return Vec::new() };
        (m.tile_first[t]..m.tile_first[t + 1]).filter(|&p| self.inside(p, x, z)).collect()
    }

    fn tile_of(&self, x: f32, z: f32) -> Option<usize> {
        let tm = self.mesh.tile_m();
        let (tx, tz) = ((x / tm).floor(), (z / tm).floor());
        let side = self.mesh.side as f32;
        (tx >= 0.0 && tz >= 0.0 && tx < side && tz < side).then(|| (tz as u32 * self.mesh.side + tx as u32) as usize)
    }

    /// Whether (x, z) is inside polygon `p` across (its edges included).
    pub fn inside(&self, p: u32, x: f32, z: f32) -> bool {
        let m = &*self.mesh;
        let q = m.polys[p as usize];
        let v = |k: usize| m.verts[m.idx[q.first as usize + k % q.n as usize] as usize];
        let (mut pos, mut neg) = (false, false);
        for k in 0..q.n as usize {
            let (a, b) = (v(k), v(k + 1));
            let s = (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0]);
            pos |= s > 1e-4;
            neg |= s < -1e-4;
        }
        !(pos && neg)
    }

    /// The links that leave polygon `p` (a two-way one from either end), with the end they leave from first.
    pub fn links_from(&self, p: u32) -> impl Iterator<Item = (LinkKind, [f32; 3], [f32; 3])> + '_ {
        self.steps[self.first[p as usize] as usize..self.first[p as usize + 1] as usize].iter().filter_map(|s| match s.via {
            Via::Link(i) => Some((self.mesh.links[i as usize].kind, s.left, s.right)),
            _ => None,
        })
    }

    /// The polygon under (x, z) when the height is not known: the one on the terrain, if one holds it, else the lowest.
    pub fn locate_2d(&self, x: f32, z: f32) -> Option<u32> {
        let at = self.polys_at(x, z);
        if let Some(&p) = at.iter().find(|&&p| self.mesh.polys[p as usize].ground) {
            return Some(p);
        }
        at.into_iter().min_by(|&a, &b| self.mesh.height(a, x, z).total_cmp(&self.mesh.height(b, x, z)).then(a.cmp(&b)))
    }

    /// The polygon at a soldier's feet (x, y, z): the nearest within `r` across and `dy` up or down ([`NavMesh::nearest`]).
    pub fn locate(&self, p: [f32; 3], r: f32, dy: f32) -> Option<u32> {
        self.mesh.nearest(p[0], p[1], p[2], r, dy)
    }

    /// The polygons with a point within `r` of (x, z) across (any height), nearest first, with that point; at most `max`.
    pub fn polys_near(&self, x: f32, z: f32, r: f32, max: usize) -> Vec<(u32, [f32; 3])> {
        self.polys_near_in(x, z, r, max, |_| true)
    }

    /// [`Query::polys_near`] among the polygons `keep` accepts, asked before their nearest point is worked out: the same list as
    /// filtering the whole one, at a fraction of the cost when few are kept (a zone's, out of every polygon within 128 m).
    pub fn polys_near_in(&self, x: f32, z: f32, r: f32, max: usize, keep: impl Fn(u32) -> bool) -> Vec<(u32, [f32; 3])> {
        let m = &*self.mesh;
        let tm = m.tile_m();
        let side = m.side as i32;
        let t = |v: f32| ((v / tm).floor() as i32).clamp(0, side - 1);
        let mut out: Vec<(f32, u32, [f32; 3])> = Vec::new();
        for tz in t(z - r)..=t(z + r) {
            for tx in t(x - r)..=t(x + r) {
                let ti = (tz * side + tx) as usize;
                for p in (m.tile_first[ti]..m.tile_first[ti + 1]).filter(|&p| keep(p)) {
                    let q = m.closest_on(p, x, z);
                    let d = (q[0] - x).hypot(q[2] - z);
                    if d <= r {
                        out.push((d, p, q));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out.truncate(max);
        out.into_iter().map(|o| (o.1, o.2)).collect()
    }

    /// Whether a straight walk across the mesh from `a` (on polygon `from`) to `b` stays on it: it crosses from polygon to
    /// polygon through their edges and seams, never over an edge with nothing across (a wall or a ledge), nor by a link.
    /// `Some(polygon)` where `b` is reached.
    pub fn raycast(&self, from: u32, a: [f32; 2], b: [f32; 2]) -> Option<u32> {
        let m = &*self.mesh;
        let mut cur = from;
        let mut prev = u32::MAX;
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        for _ in 0..4096 {
            if self.inside(cur, b[0], b[1]) {
                return Some(cur);
            }
            // the edge the segment leaves the polygon by: the crossing farthest along it
            let q = m.polys[cur as usize];
            let v = |k: usize| m.verts[m.idx[q.first as usize + k % q.n as usize] as usize];
            let mut exit: Option<(f32, usize, [f32; 2])> = None;
            for k in 0..q.n as usize {
                let (p0, p1) = (v(k), v(k + 1));
                let (ex, ez) = (p1[0] - p0[0], p1[2] - p0[2]);
                let den = dx * ez - dz * ex;
                if den.abs() < 1e-9 {
                    continue;
                }
                let (wx, wz) = (p0[0] - a[0], p0[2] - a[1]);
                let t = (wx * ez - wz * ex) / den;
                let u = (wx * dz - wz * dx) / den;
                if (-1e-4..=1.0 + 1e-4).contains(&u) && exit.is_none_or(|e| t > e.0) {
                    exit = Some((t, k, [a[0] + dx * t, a[1] + dz * t]));
                }
            }
            let (_, k, at) = exit?;
            let nb = m.nei[q.first as usize + k];
            let next = if nb < PORTAL {
                Some(nb)
            } else if nb == NONE {
                None
            } else {
                // across the tile's border: the seam whose opening holds the crossing
                self.steps(cur).iter().filter(|s| s.via == Via::Seam && s.to != prev).find(|s| on_segment(s.left, s.right, at, 0.05)).map(|s| s.to)
            };
            let next = next?;
            prev = cur;
            cur = next;
        }
        None
    }

    /// How far each polygon is from the circle of radius `r` around (x, z) walking over the mesh (cm; `u32::MAX` with no
    /// way): from the middle of each opening to the next, as the searches count it. A goal's field is built once and kept.
    pub fn distances_to(&self, x: f32, z: f32, r: f32) -> Vec<u32> {
        let n = self.mesh.polys.len();
        let mut d = vec![u32::MAX; n];
        // where each polygon is entered on the way to the goal, to measure from
        let mut at = vec![[0.0f32; 3]; n];
        let mut open = BinaryHeap::new();
        for (p, q) in self.polys_near(x, z, r, usize::MAX) {
            d[p as usize] = 0;
            at[p as usize] = q;
            open.push((Reverse(0u32), p));
        }
        while let Some((Reverse(c), p)) = open.pop() {
            if c != d[p as usize] {
                continue;
            }
            let (a, b) = (self.into_first[p as usize] as usize, self.into_first[p as usize + 1] as usize);
            for &(from, k) in &self.into[a..b] {
                let s = &self.steps[k as usize];
                let mid = s.mid();
                let entry = match s.via {
                    Via::Link(_) => s.left,
                    _ => mid,
                };
                let nc = c.saturating_add(cm(dist(entry, at[p as usize]) + self.extra(s)));
                if nc < d[from as usize] {
                    d[from as usize] = nc;
                    at[from as usize] = entry;
                    open.push((Reverse(nc), from));
                }
            }
        }
        d
    }

    /// What a step costs beyond the distance walked (m): a link's own cost.
    fn extra(&self, s: &Step) -> f32 {
        match s.via {
            Via::Link(i) => {
                let l = &self.mesh.links[i as usize];
                match l.kind {
                    LinkKind::Ladder => LADDER_FACTOR * (l.to[1] - l.from[1]).abs(),
                    LinkKind::Drop if l.from[1] - l.to[1] > SOLDIER_DROP => HURTING_DROP_COST_M,
                    LinkKind::Drop => DROP_COST_M,
                    LinkKind::Jump => JUMP_COST_M,
                    LinkKind::Door | LinkKind::Exit => 0.0,
                }
            }
            _ => 0.0,
        }
    }

    /// The way from `from` to `to` (points at the feet), searched all at once: the corners to pass by, `to` (on the mesh)
    /// last; `None` when either is off the mesh or there is no way.
    pub fn path(&self, from: [f32; 3], to: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        let (fa, c, tb) = self.corridor(from, to)?;
        Some(self.funnel(fa, &c, tb))
    }

    /// The polygons' steps of the way from `from` to `to`, searched all at once, with its ends on the mesh.
    pub fn corridor(&self, from: [f32; 3], to: [f32; 3]) -> Option<([f32; 3], Vec<Step>, [f32; 3])> {
        let (a, b) = (self.locate(from, 2.0, 2.5)?, self.locate(to, 2.0, 2.5)?);
        let (fa, tb) = (self.mesh.closest_on(a, from[0], from[2]), self.mesh.closest_on(b, to[0], to[2]));
        let mut s = Search::default();
        s.begin(self, (a, fa), (b, tb), &[], 0.0);
        match s.advance(self, u32::MAX).0 {
            Progress::Found(c) => Some((fa, c, tb)),
            _ => None,
        }
    }

    /// The straightest way through `corridor` (the steps a search found, in order) from `start` to `goal`: the corners to
    /// pass by, the goal last; a link's two ends are corners.
    pub fn funnel(&self, start: [f32; 3], corridor: &[Step], goal: [f32; 3]) -> Vec<[f32; 3]> {
        let mut portals: Vec<([f32; 3], [f32; 3])> = Vec::with_capacity(corridor.len() + 2);
        portals.push((start, start));
        for s in corridor {
            match s.via {
                Via::Link(_) => {
                    portals.push((s.left, s.left));
                    portals.push((s.right, s.right));
                }
                _ => portals.push((s.left, s.right)),
            }
        }
        portals.push((goal, goal));
        string_pull(&portals)
    }
}

/// The opening between polygon `a`'s border edge `edge` and polygon `b` across the tiles' border: the part of the edge that
/// overlaps one of `b`'s on the same line.
fn seam_portal(m: &NavMesh, a: u32, edge: usize, b: u32) -> Option<([f32; 3], [f32; 3])> {
    let qa = m.polys[a as usize];
    let at = |q: super::Poly, k: usize| m.verts[m.idx[q.first as usize + k % q.n as usize] as usize];
    let (a0, a1) = (at(qa, edge), at(qa, edge + 1));
    let side = m.nei[qa.first as usize + edge].wrapping_sub(PORTAL);
    let opposite = PORTAL + (side + 2) % 4;
    let ax = if side.is_multiple_of(2) { 2 } else { 0 };
    let (lo, hi) = (a0[ax].min(a1[ax]), a0[ax].max(a1[ax]));
    let qb = m.polys[b as usize];
    let mut best: Option<(f32, f32, f32)> = None;
    for k in 0..qb.n as usize {
        if m.nei[qb.first as usize + k] != opposite {
            continue;
        }
        let (b0, b1) = (at(qb, k), at(qb, k + 1));
        let (s, e) = (lo.max(b0[ax].min(b1[ax])), hi.min(b0[ax].max(b1[ax])));
        if e > s && best.is_none_or(|bst| e - s > bst.0) {
            best = Some((e - s, s, e));
        }
    }
    let (_, s, e) = best?;
    let point = |v: f32| {
        let t = if (a1[ax] - a0[ax]).abs() < 1e-6 { 0.0 } else { (v - a0[ax]) / (a1[ax] - a0[ax]) };
        [a0[0] + (a1[0] - a0[0]) * t, a0[1] + (a1[1] - a0[1]) * t, a0[2] + (a1[2] - a0[2]) * t]
    };
    Some((point(s), point(e)))
}

/// Whether `p` lies on the segment a–b across, within `tol` m.
fn on_segment(a: [f32; 3], b: [f32; 3], p: [f32; 2], tol: f32) -> bool {
    let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
    let l2 = dx * dx + dz * dz;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[2]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (a[0] + t * dx - p[0]).hypot(a[2] + t * dz - p[1]) <= tol
}

/// The simple stupid funnel: through openings (`left`, `right`, as seen going along), the first a point (the start) and the
/// last a point (the goal); the corners where the way turns, the goal last.
fn string_pull(portals: &[([f32; 3], [f32; 3])]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    let Some(&(start, _)) = portals.first() else { return out };
    let (mut apex, mut left, mut right) = (start, portals[0].0, portals[0].1);
    let (mut left_i, mut right_i) = (0usize, 0usize);
    let same = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).abs() < 1e-4 && (a[2] - b[2]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-3;
    let mut i = 1;
    let mut guard = 0;
    while i < portals.len() && guard < 16 * portals.len() + 64 {
        guard += 1;
        let (l, r) = portals[i];
        // the right side
        if tri_area2(apex, right, r) <= 0.0 {
            if same(apex, right) || tri_area2(apex, left, r) > 0.0 {
                right = r;
                right_i = i;
            } else {
                // the right crosses over the left: the left is a corner
                if out.last() != Some(&left) {
                    out.push(left);
                }
                apex = left;
                (left, right) = (apex, apex);
                right_i = left_i;
                i = left_i + 1;
                continue;
            }
        }
        // the left side
        if tri_area2(apex, left, l) >= 0.0 {
            if same(apex, left) || tri_area2(apex, right, l) < 0.0 {
                left = l;
                left_i = i;
            } else {
                if out.last() != Some(&right) {
                    out.push(right);
                }
                apex = right;
                (left, right) = (apex, apex);
                left_i = right_i;
                i = right_i + 1;
                continue;
            }
        }
        i += 1;
    }
    let goal = portals[portals.len() - 1].0;
    // (a corner on the start itself, when it stands on an opening's end, is no corner)
    out.retain(|p| !same(*p, start));
    if out.last() != Some(&goal) {
        out.push(goal);
    }
    out
}

/// The polygon a step leaves from (the steps are stored polygon after polygon: the last whose first step is not past it).
fn from_of(q: &Query, step: u32) -> u32 {
    (q.first.partition_point(|&f| f <= step) - 1) as u32
}

/// What a search remembers of each polygon.
#[derive(Clone, Copy, Default)]
struct Node {
    /// which search this belongs to: the other values only count if it matches
    stamp: u32,
    cost: u32,
    /// the step it was reached by (its index in [`Query::steps`]); `u32::MAX` at the start
    came: u32,
    /// where it was entered
    at: [f32; 3],
}

/// What [`Search::advance`] has come to.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// the corridor's steps, in order
    Found(Vec<Step>),
    NoPath,
    /// not yet
    Follows,
}

/// An A* over polygons that can be left half done and continued (the same result as all at once), opening at most a quota of
/// polygons each time; its memory is kept from one search to the next.
#[derive(Default)]
pub struct Search {
    stamp: u32,
    nodes: Vec<Node>,
    open: BinaryHeap<(Reverse<u32>, Reverse<u32>, Reverse<u32>)>,
    goal: u32,
    goal_at: [f32; 3],
    start: u32,
    /// what entering each polygon costs on top (cm), for this search: (stamp, cost)
    toll: Vec<(u32, u32)>,
}

impl Search {
    /// Entering polygon `p` costs `cm` more in the search begun last (until the next [`Search::begin`]): places where walkers
    /// got stuck.
    pub fn toll(&mut self, p: u32, cm: u32) {
        if self.toll.len() != self.nodes.len() {
            self.toll = vec![(0, 0); self.nodes.len()];
        }
        let t = &mut self.toll[p as usize];
        *t = (self.stamp, if t.0 == self.stamp { t.1.saturating_add(cm) } else { cm });
    }

    /// A new search from `start` (polygon, point) to `goal`; the polygons near any of `avoid` (within `margin`, across) are
    /// left out, as if already seen.
    pub fn begin(&mut self, q: &Query, start: (u32, [f32; 3]), goal: (u32, [f32; 3]), avoid: &[[f32; 2]], margin: f32) {
        let n = q.mesh.polys.len();
        if self.nodes.len() != n || self.stamp == u32::MAX {
            self.nodes.clear();
            self.nodes.resize(n, Node::default());
            self.stamp = 0;
        }
        self.stamp += 1;
        self.open.clear();
        for m in avoid {
            for (p, _) in q.polys_near(m[0], m[1], margin, usize::MAX) {
                if p != goal.0 {
                    self.nodes[p as usize] = Node { stamp: self.stamp, cost: 0, came: u32::MAX, at: start.1 };
                }
            }
        }
        (self.start, self.goal, self.goal_at) = (start.0, goal.0, goal.1);
        self.nodes[start.0 as usize] = Node { stamp: self.stamp, cost: 0, came: u32::MAX, at: start.1 };
        let h = cm(dist(start.1, goal.1));
        self.open.push((Reverse(h), Reverse(h), Reverse(start.0)));
    }

    fn cost(&self, p: u32) -> u32 {
        let n = &self.nodes[p as usize];
        if n.stamp == self.stamp { n.cost } else { u32::MAX }
    }

    /// Continues, opening at most `quota` polygons; also returns how many it opened.
    pub fn advance(&mut self, q: &Query, quota: u32) -> (Progress, u32) {
        let mut opened = 0;
        while opened < quota {
            let Some((Reverse(f), Reverse(h), Reverse(p))) = self.open.pop() else { return (Progress::NoPath, opened) };
            let node = self.nodes[p as usize];
            // (an entry left behind by a cheaper way found later)
            if f != node.cost.saturating_add(h) {
                continue;
            }
            opened += 1;
            if p == self.goal {
                let mut corridor = Vec::new();
                let mut c = p;
                while self.nodes[c as usize].came != u32::MAX {
                    let came = self.nodes[c as usize].came;
                    corridor.push(q.steps[came as usize]);
                    c = from_of(q, came);
                }
                corridor.reverse();
                return (Progress::Found(corridor), opened);
            }
            let base = q.first[p as usize];
            for (k, s) in q.steps(p).iter().enumerate() {
                let entry = match s.via {
                    Via::Link(_) => s.right,
                    _ => s.mid(),
                };
                let walk = match s.via {
                    Via::Link(_) => dist(node.at, s.left) + flat(s.left, s.right),
                    _ => dist(node.at, entry),
                };
                let toll = self.toll.get(s.to as usize).filter(|t| t.0 == self.stamp).map_or(0, |t| t.1);
                let g = node.cost.saturating_add(cm(walk + q.extra(s))).saturating_add(toll);
                if g < self.cost(s.to) {
                    self.nodes[s.to as usize] = Node { stamp: self.stamp, cost: g, came: base + k as u32, at: entry };
                    let h = cm(dist(entry, self.goal_at));
                    self.open.push((Reverse(g.saturating_add(h)), Reverse(h), Reverse(s.to)));
                }
            }
        }
        (Progress::Follows, opened)
    }

    pub fn start(&self) -> u32 {
        self.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navmesh::{Link, Poly};
    use std::sync::Arc;

    /// An L of three squares of 10 m on flat ground (A at the origin, B east of it, C north of B) and an island D 2 m lower,
    /// reached from C only by a drop.
    fn ell() -> Query {
        let sq = |x: f32, z: f32, y: f32| [[x, y, z], [x + 10.0, y, z], [x + 10.0, y, z + 10.0], [x, y, z + 10.0]];
        let mut m = NavMesh { side: 1, ..Default::default() };
        // neighbour of each edge (k: from vertex k to k + 1)
        let polys = [(sq(0.0, 0.0, 0.0), [NONE, 1, NONE, NONE]), (sq(10.0, 0.0, 0.0), [NONE, NONE, 2, 0]), (sq(10.0, 10.0, 0.0), [1, NONE, NONE, NONE]), (sq(21.0, 10.0, -2.0), [NONE; 4])];
        for (vs, nei) in polys {
            let first = m.idx.len() as u32;
            for v in vs {
                m.idx.push(m.verts.len() as u32);
                m.verts.push(v);
            }
            m.nei.extend(nei);
            m.polys.push(Poly { first, n: 4, ..Default::default() });
        }
        m.tile_first = vec![0, 4];
        m.links.push(Link { a: 2, b: 3, from: [19.5, 0.0, 15.0], to: [21.5, -2.0, 15.0], kind: LinkKind::Drop });
        Query::new(Arc::new(m))
    }

    fn path(q: &Query, from: [f32; 3], to: [f32; 3], quota: u32) -> Option<Vec<[f32; 3]>> {
        let (a, b) = (q.locate_2d(from[0], from[2])?, q.locate_2d(to[0], to[2])?);
        let mut s = Search::default();
        s.begin(q, (a, from), (b, to), &[], 10.0);
        loop {
            match s.advance(q, quota).0 {
                Progress::Found(c) => return Some(q.funnel(from, &c, to)),
                Progress::NoPath => return None,
                Progress::Follows => {}
            }
        }
    }

    #[test]
    fn the_way_turns_at_the_inner_corner_only() {
        let q = ell();
        let p = path(&q, [2.0, 0.0, 2.0], [15.0, 0.0, 18.0], u32::MAX).unwrap();
        assert_eq!(p, vec![[10.0, 0.0, 10.0], [15.0, 0.0, 18.0]]);
        // in a straight line within one polygon and across an edge, no corner
        assert_eq!(path(&q, [2.0, 0.0, 2.0], [18.0, 0.0, 8.0], u32::MAX).unwrap(), vec![[18.0, 0.0, 8.0]]);
        // a search continued one polygon at a time finds the same
        assert_eq!(path(&q, [2.0, 0.0, 2.0], [15.0, 0.0, 18.0], 1), Some(p));
    }

    #[test]
    fn a_drop_is_taken_down_and_not_up() {
        let q = ell();
        let down = path(&q, [2.0, 0.0, 2.0], [25.0, -2.0, 15.0], u32::MAX).unwrap();
        assert!(down.contains(&[19.5, 0.0, 15.0]) && down.contains(&[21.5, -2.0, 15.0]), "by the drop's ends: {down:?}");
        assert_eq!(path(&q, [25.0, -2.0, 15.0], [2.0, 0.0, 2.0], u32::MAX), None);
        // one zone all the same (a drop joins), and the island's way back is none
        assert_eq!(q.zone, vec![1, 1, 1, 1]);
        let field = q.distances_to(15.0, 15.0, 1.0);
        assert_eq!(field[2], 0);
        assert!(field[1] > 0 && field[0] > field[1], "{field:?}");
        assert_eq!(field[3], u32::MAX);
        // the drop's foot is a place of its own with no way out (what [`NavMesh::sink_exits`] gives a way out of)
        let c = q.mesh.components();
        assert!(c[0] == c[1] && c[1] == c[2] && c[3] != c[0], "{c:?}");
    }

    #[test]
    fn a_line_stays_on_the_mesh_or_not() {
        let q = ell();
        assert_eq!(q.raycast(0, [2.0, 2.0], [18.0, 5.0]), Some(1));
        assert_eq!(q.raycast(0, [2.0, 2.0], [15.0, 18.0]), None, "the corner is in the way");
        assert_eq!(q.raycast(2, [15.0, 15.0], [25.0, 15.0]), None, "a drop is no line");
        assert_eq!(q.raycast(1, [12.0, 2.0], [15.0, 18.0]), Some(2));
    }

    #[test]
    fn what_must_be_avoided_is_gone_round_or_through() {
        let q = ell();
        // the only way runs through B: avoiding it there is no way; the caller then asks without
        let mut s = Search::default();
        s.begin(&q, (0, [2.0, 0.0, 2.0]), (2, [15.0, 0.0, 18.0]), &[[15.0, 5.0]], 3.0);
        assert_eq!(s.advance(&q, u32::MAX).0, Progress::NoPath);
    }
}
