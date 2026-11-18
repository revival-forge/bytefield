//! Foot and vehicle paths: A* over the level's map, in cells of [`CELL`] m, then a straight line as long as possible.
//! The walls of urban maps are 1–3 m: a cell is only free if it is free entirely, or the walls would vanish.
use bytefield_formats::pathmap::PathMap;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

pub const CELL: f32 = 2.0;
/// How far from a goal shut in a small zone its nearest cell in the searcher's zone may be, cells ([`Grid::ends`]): 30 m.
pub const GOAL_ASIDE_CELLS: i64 = 15;
/// The walker asks for a goal moved aside ([`Searcher::request`]) only from farther than this, m: with it from any distance, bots3
/// against bots2 went on GC_Endor from +23 to −20 and on Berlin from +34 to +10 (it took the points the walker picks in a flag's
/// area, where another pick finds a way).
pub const ASIDE_FROM_M: f32 = 80.0;
/// The unit of [`Grid::distances_to`], m: a side step costs 10 of them (one [`CELL`]).
pub const WAY_UNIT_M: f32 = CELL / 10.0;
/// Nodes a search opens without waiting its turn: almost all fit, and those that do not stay in the [`Searcher`] queue.
const NODES_SHORT: u32 = 8_000;
/// Nodes per tick among all the bots: about 4 ms. Searches from one end of a map to the other open half a million.
const NODES_PER_TIC: u32 = 40_000;
/// Cells a straight leg of the path joins at most.
const MAX_LEG: usize = 96;

/// What a search remembers of each cell.
#[derive(Clone, Copy, Default)]
struct Node {
    /// which search this belongs to: the other values only count if it matches
    stamp: u32,
    cost: u32,
    came: u32,
}

/// A half-done search: what is needed to continue it later. Asking for and filling this memory on every search cost milliseconds.
#[derive(Default)]
struct Work {
    stamp: u32,
    nodes: Vec<Node>,
    open_ones: BinaryHeap<(Reverse<u32>, Reverse<u32>, Reverse<u32>)>,
}

impl Work {
    fn cost(&self, i: usize) -> u32 {
        if self.nodes[i].stamp == self.stamp { self.nodes[i].cost } else { u32::MAX }
    }
}

/// Whether `m` is less than `r` from the leg from `a` to `b`.
fn near_leg(a: [f32; 2], b: [f32; 2], m: [f32; 2], r: f32) -> bool {
    let d = [b[0] - a[0], b[1] - a[1]];
    let t = (((m[0] - a[0]) * d[0] + (m[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(1e-3)).clamp(0.0, 1.0);
    (a[0] + d[0] * t - m[0]).hypot(a[1] + d[1] * t - m[1]) < r
}

enum Search {
    Cells(Vec<(usize, usize)>),
    NoPath,
    /// has not finished yet, and has spent these nodes
    Follows,
}

/// What [`Searcher::request`] answers.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Path(Vec<[f32; 2]>),
    /// being searched: ask again on the next tick
    Wait,
    NoPath,
}

/// How close to what must be avoided a path does not go, m.
const AVOID_MARGIN: f32 = 10.0;

/// A long search, in the queue.
struct Request {
    whoever: usize,
    avoid: Vec<[f32; 2]>,
    requested_goal: [f32; 2],
    start: (usize, usize),
    meta: (usize, usize),
    exit_aside: bool,
    /// the goal was moved beside a small zone ([`Grid::ends`]): the path ends with the goal asked for, a straight last stretch
    goal_aside: bool,
    /// the last tick it asked on: whoever stops asking (has died, has changed its mind) loses its turn
    seen: u64,
    started: bool,
}

/// The path, and the goal at its end when it was moved aside ([`Request::goal_aside`]).
fn with_goal(mut path: Vec<[f32; 2]>, aside: bool, goal: [f32; 2]) -> Vec<[f32; 2]> {
    if aside {
        path.push(goal);
    }
    path
}

/// The path searches of a map, never exceeding [`NODES_PER_TIC`]: short ones are solved at once and long ones continue over
/// successive ticks, one at a time in order of arrival. It counts nodes, not time, and the bots think in order: it is deterministic.
#[derive(Default)]
pub struct Searcher {
    tic: u64,
    spent: u32,
    queue: VecDeque<Request>,
    short: Work,
    long: Work,
}

impl Searcher {
    /// The foot or vehicle path from `whoever` to `a`; see [`Grid::path_from`] and [`Grid::path`] for `leaving` and `se_ve`.
    /// It does not pass near whatever is in `avoid`, if possible.
    /// `aside`: a goal shut in a small zone is moved beside it, into the searcher's zone ([`Grid::ends`]; bots3: bots2, the fixed
    /// opponent of the tournaments, keeps its old paths).
    #[allow(clippy::too_many_arguments)]
    pub fn request(&mut self, g: &Grid, tic: u64, whoever: usize, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, avoid: &[[f32; 2]], aside: bool) -> Answer {
        if tic != self.tic {
            self.tic = tic;
            self.spent = 0;
        }
        let queued = self.queue.iter().position(|p| p.whoever == whoever && p.requested_goal == a);
        if let Some(k) = queued {
            self.queue[k].seen = tic;
        } else {
            // whatever it had asked before no longer interests it
            self.queue.retain(|p| p.whoever != whoever);
            let Some((start, meta)) = g.ends(de, a, se_ve, leaving, aside) else { return Answer::NoPath };
            let exit_aside = g.cell(de[0], de[1]) != Some(start);
            // (the goal moved beside its small zone: the last stretch straight to it, as before there was a path; through a door, into
            // the flag's radius. Ending the path beside it, outside, cost bots3 against bots2 on GC_Endor +23 → −20 and on Berlin
            // +34 → +10)
            let goal_aside = aside && g.cell(a[0], a[1]).and_then(|c| g.free_near(c, &|_| true)) != Some(meta);
            if self.spent < NODES_PER_TIC {
                g.begin(&mut self.short, start, meta, avoid);
                let quota = NODES_SHORT.min(NODES_PER_TIC - self.spent);
                let (r, n) = g.advance(&mut self.short, meta, start, quota);
                self.spent += n;
                match r {
                    Search::Cells(c) => return Answer::Path(with_goal(g.points(&c, exit_aside, avoid), goal_aside, a)),
                    // (what must be avoided blocks the way: better through there than not going)
                    Search::NoPath if !avoid.is_empty() => return self.request(g, tic, whoever, de, a, se_ve, leaving, &[], aside),
                    Search::NoPath => return Answer::NoPath,
                    Search::Follows => {}
                }
            }
            self.queue.push_back(Request { whoever, avoid: avoid.to_vec(), requested_goal: a, start, meta, exit_aside, goal_aside, seen: tic, started: false });
        }
        // from the queue, the first that is still asking; it is the asker's turn if it is them
        while self.queue.front().is_some_and(|p| p.seen + 1 < tic) {
            self.queue.pop_front();
        }
        if !self.queue.front().is_some_and(|p| p.whoever == whoever && p.requested_goal == a) || self.spent >= NODES_PER_TIC {
            return Answer::Wait;
        }
        let p = &mut self.queue[0];
        let (start, meta, exit_aside, goal_aside) = (p.start, p.meta, p.exit_aside, p.goal_aside);
        let avoid_requested = std::mem::take(&mut p.avoid);
        if !std::mem::replace(&mut p.started, true) {
            g.begin(&mut self.long, start, meta, &avoid_requested);
        }
        let (r, n) = g.advance(&mut self.long, meta, start, NODES_PER_TIC - self.spent);
        self.spent += n;
        match r {
            Search::Follows => {
                self.queue[0].avoid = avoid_requested;
                Answer::Wait
            }
            Search::NoPath => {
                self.queue.pop_front();
                if avoid_requested.is_empty() { Answer::NoPath } else { self.request(g, tic, whoever, de, a, se_ve, leaving, &[], aside) }
            }
            Search::Cells(c) => {
                self.queue.pop_front();
                Answer::Path(with_goal(g.points(&c, exit_aside, &avoid_requested), goal_aside, a))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Grid {
    pub n: usize,
    pub blocked: Vec<bool>,
    /// which connected zone each free cell belongs to: on some maps you spawn in a trench the map leaves closed
    pub zone: Vec<u32>,
    /// how many cells each zone has (index 0, the blocked ones, unused)
    zone_cells: Vec<u32>,
}

// TODO: GC_Tatooine, with 64 bots: steps of 19, 24 and 46 ms (1, 16 and 193; `examples/step_spikes`). Because of the clean room.
impl Grid {
    pub fn from_map(m: &PathMap) -> Grid {
        let k = CELL as usize;
        let n = m.width.min(m.height) / k;
        let mut blocked = vec![false; n * n];
        for y in 0..n * k {
            for x in 0..n * k {
                if m.pixels[y * m.width + x] != 0 {
                    blocked[(y / k) * n + x / k] = true;
                }
            }
        }
        Grid::new_one(n, blocked)
    }

    pub fn new_one(n: usize, blocked: Vec<bool>) -> Grid {
        let mut zone = vec![0u32; n * n];
        let mut stack = Vec::new();
        let mut z = 0;
        let mut zone_cells = vec![0u32];
        for i in 0..n * n {
            if blocked[i] || zone[i] != 0 {
                continue;
            }
            z += 1;
            zone[i] = z;
            zone_cells.push(0);
            stack.push(i);
            while let Some(j) = stack.pop() {
                zone_cells[z as usize] += 1;
                let (x, y) = (j % n, j / n);
                let neighbours = [(x > 0).then(|| j - 1), (x + 1 < n).then(|| j + 1), (y > 0).then(|| j - n), (y + 1 < n).then(|| j + n)];
                for k in neighbours.into_iter().flatten() {
                    if !blocked[k] && zone[k] == 0 {
                        zone[k] = z;
                        stack.push(k);
                    }
                }
            }
        }
        Grid { n, blocked, zone, zone_cells }
    }

    fn cell(&self, x: f32, z: f32) -> Option<(usize, usize)> {
        let (cx, cz) = ((x / CELL).floor(), (z / CELL).floor());
        (cx >= 0.0 && cz >= 0.0 && (cx as usize) < self.n && (cz as usize) < self.n).then_some((cx as usize, cz as usize))
    }

    /// The connected zone of that point's cell: 0 in a blocked cell, `None` outside the map. Two points in different zones have no
    /// path between them on this map.
    pub fn zone_at(&self, x: f32, z: f32) -> Option<u32> {
        self.cell(x, z).map(|c| self.zone[c.1 * self.n + c.0])
    }

    /// The zone of the free cell nearest that point (within 32 cells): where a soldier standing on a cell the map draws blocked (a
    /// bunker's floor, a doorway) walks from. `None` outside the map or with no free cell near.
    pub fn zone_near(&self, x: f32, z: f32) -> Option<u32> {
        let c = self.free_near(self.cell(x, z)?, &|_| true)?;
        Some(self.zone[c.1 * self.n + c.0])
    }

    /// The zones of the free cells within `r` of a point (a flag's circle), without repeats; empty when they are all blocked or
    /// outside the map.
    pub fn zones_near(&self, x: f32, z: f32, r: f32) -> Vec<u32> {
        let mut out = Vec::new();
        let k = (r / CELL).ceil() as i32;
        for i in -k..=k {
            for j in -k..=k {
                let (dx, dz) = (i as f32 * CELL, j as f32 * CELL);
                if dx * dx + dz * dz > (r + CELL) * (r + CELL) {
                    continue;
                }
                if let Some(c) = self.zone_at(x + dx, z + dz).filter(|&c| c != 0)
                    && !out.contains(&c)
                {
                    out.push(c);
                }
            }
        }
        out
    }

    /// How far each cell is from the circle of radius `r` around (x, z) walking over free cells (8 neighbours), in units of
    /// [`WAY_UNIT_M`]; `u16::MAX` where there is no way. Built once per goal ([`super::Maps::way_m`]): a 1024² grid costs ~30 ms.
    pub fn distances_to(&self, x: f32, z: f32, r: f32) -> Vec<u16> {
        let n = self.n;
        let mut d = vec![u16::MAX; n * n];
        // Dial's buckets: the costs of a step are 10 (side) and 14 (diagonal)
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); 16];
        let k = (r / CELL).ceil() as i64;
        let Some(c) = self.cell(x, z) else { return d };
        for i in -k..=k {
            for j in -k..=k {
                let (cx, cz) = (c.0 as i64 + i, c.1 as i64 + j);
                if (i * i + j * j) as f32 * CELL * CELL > (r + CELL) * (r + CELL) || cx < 0 || cz < 0 || cx as usize >= n || cz as usize >= n {
                    continue;
                }
                let idx = cz as usize * n + cx as usize;
                if !self.blocked[idx] {
                    d[idx] = 0;
                    buckets[0].push(idx as u32);
                }
            }
        }
        let mut cost: u32 = 0;
        let mut pending: usize = buckets[0].len();
        while pending > 0 {
            let b = (cost % 16) as usize;
            while let Some(idx) = buckets[b].pop() {
                pending -= 1;
                let idx = idx as usize;
                if d[idx] as u32 != cost {
                    continue;
                }
                let (cx, cz) = ((idx % n) as i64, (idx / n) as i64);
                for (dx, dz, w) in [(1, 0, 10), (-1, 0, 10), (0, 1, 10), (0, -1, 10), (1, 1, 14), (1, -1, 14), (-1, 1, 14), (-1, -1, 14)] {
                    let (x2, z2) = (cx + dx, cz + dz);
                    if x2 < 0 || z2 < 0 || x2 as usize >= n || z2 as usize >= n {
                        continue;
                    }
                    let j = z2 as usize * n + x2 as usize;
                    // (a diagonal does not cut a blocked corner)
                    if self.blocked[j] || (dx != 0 && dz != 0 && (self.blocked[cz as usize * n + x2 as usize] || self.blocked[z2 as usize * n + cx as usize])) {
                        continue;
                    }
                    let nc = cost + w;
                    if nc < d[j] as u32 && nc < u16::MAX as u32 {
                        d[j] = nc as u16;
                        buckets[(nc % 16) as usize].push(j as u32);
                        pending += 1;
                    }
                }
            }
            cost += 1;
        }
        d
    }

    /// The way from (x, z) in a field of [`Grid::distances_to`], m: from the nearest free cell (within 32 cells), `None` with no way.
    pub fn way_in(&self, field: &[u16], x: f32, z: f32) -> Option<f32> {
        let c = self.free_near(self.cell(x, z)?, &|_| true)?;
        let v = *field.get(c.1 * self.n + c.0)?;
        (v != u16::MAX).then(|| v as f32 * WAY_UNIT_M)
    }

    /// The area of a zone, in cells: a closed place is far smaller than the map around it.
    pub fn zone_size(&self, z: u32) -> u32 {
        self.zone_cells.get(z as usize).copied().unwrap_or(0)
    }

    /// Whether there is nothing at that point, according to the map; outside the map it is unknown, and taken as free.
    pub fn clear(&self, x: f32, z: f32) -> bool {
        self.cell(x, z).is_none_or(|c| self.free(c))
    }

    fn free(&self, c: (usize, usize)) -> bool {
        !self.blocked[c.1 * self.n + c.0]
    }

    /// The centre of a cell.
    fn centre(c: (usize, usize)) -> [f32; 2] {
        [(c.0 as f32 + 0.5) * CELL, (c.1 as f32 + 0.5) * CELL]
    }

    /// The free cell nearest to `c` (a respawn point or a flag in a blocked cell) that `still_valid` accepts.
    fn free_near(&self, c: (usize, usize), still_valid: &dyn Fn([f32; 2]) -> bool) -> Option<(usize, usize)> {
        if self.free(c) {
            return Some(c);
        }
        let mut tests = 0;
        for r in 0..32i64 {
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let (x, z) = (c.0 as i64 + dx, c.1 as i64 + dz);
                    if x >= 0 && z >= 0 && (x as usize) < self.n && (z as usize) < self.n && self.free((x as usize, z as usize)) {
                        tests += 1;
                        if tests > 200 {
                            return None;
                        }
                        if still_valid(Self::centre((x as usize, z as usize))) {
                            return Some((x as usize, z as usize));
                        }
                    }
                }
            }
        }
        None
    }

    /// The cell nearest to `c` in zone `z` that `still_valid` accepts: the way out of a place the map leaves closed.
    fn exit(&self, c: (usize, usize), z: u32, still_valid: &dyn Fn([f32; 2]) -> bool) -> Option<(usize, usize)> {
        let mut tests = 0;
        for r in 1..64i64 {
            for dz in -r..=r {
                for dx in -r..=r {
                    // the edge of the square, one in two: enough not to skip a step
                    if (dx.abs() != r && dz.abs() != r) || (dx + dz) % 2 != 0 {
                        continue;
                    }
                    let (x, y) = (c.0 as i64 + dx, c.1 as i64 + dz);
                    if x < 0 || y < 0 || x as usize >= self.n || y as usize >= self.n || self.zone[y as usize * self.n + x as usize] != z {
                        continue;
                    }
                    tests += 1;
                    if tests > 120 {
                        return None;
                    }
                    if still_valid(Self::centre((x as usize, y as usize))) {
                        return Some((x as usize, y as usize));
                    }
                }
            }
        }
        None
    }

    /// The cell nearest to `c` in zone `z`, within `r` cells (every cell of each ring: the nearest, not one in two).
    fn exit_within(&self, c: (usize, usize), z: u32, r: i64) -> Option<(usize, usize)> {
        for k in 0..=r {
            for dz in -k..=k {
                for dx in -k..=k {
                    if dx.abs() != k && dz.abs() != k {
                        continue;
                    }
                    let (x, y) = (c.0 as i64 + dx, c.1 as i64 + dz);
                    if x >= 0 && y >= 0 && (x as usize) < self.n && (y as usize) < self.n && !self.blocked[y as usize * self.n + x as usize] && self.zone[y as usize * self.n + x as usize] == z {
                        return Some((x as usize, y as usize));
                    }
                }
            }
        }
        None
    }

    /// Whether from one cell to another you go in a straight line through free cells.
    fn recta(&self, a: (usize, usize), b: (usize, usize)) -> bool {
        let (dx, dz) = (b.0 as f32 - a.0 as f32, b.1 as f32 - a.1 as f32);
        let steps = (dx.abs().max(dz.abs()) * 2.0).ceil().max(1.0) as usize;
        (0..=steps).all(|i| {
            let t = i as f32 / steps as f32;
            let (x, z) = (a.0 as f32 + 0.5 + dx * t, a.1 as f32 + 0.5 + dz * t);
            // also just enough to the sides, so as not to brush the corners
            [(0.0, 0.0), (0.35, 0.35), (-0.35, 0.35), (0.35, -0.35), (-0.35, -0.35)].iter().all(|(ox, oz)| self.free(((x + ox).max(0.0) as usize, (z + oz).max(0.0) as usize)))
        })
    }

    /// A* from (x, z) to (x, z), without cutting corners, with straight legs joined: the points to pass through, the first after the exit.
    /// All at once, without looking at the cost: in the game, [`Searcher::request`].
    pub fn path(&self, de: [f32; 2], a: [f32; 2]) -> Option<Vec<[f32; 2]>> {
        self.at_once(de, a, &|_| true, false)
    }

    /// The same; if the exit falls in a blocked cell (respawn points are sometimes right against a wall), it starts
    /// at the nearest free one that `se_ve` accepts: you cannot reach the other side of the wall. If it is in a zone with no connection to the
    /// goal, it starts at the nearest cell of the goal's zone that `se_ve` accepts.
    pub fn path_from(&self, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool) -> Option<Vec<[f32; 2]>> {
        self.at_once(de, a, se_ve, true)
    }

    fn at_once(&self, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool) -> Option<Vec<[f32; 2]>> {
        let (start, meta) = self.ends(de, a, se_ve, leaving, false)?;
        let mut t = Work::default();
        self.begin(&mut t, start, meta, &[]);
        match self.advance(&mut t, meta, start, u32::MAX).0 {
            Search::Cells(c) => Some(self.points(&c, self.cell(de[0], de[1]) != Some(start), &[])),
            _ => None,
        }
    }

    /// The exit and arrival cells of a path from `de` to `a`.
    fn ends(&self, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, aside: bool) -> Option<((usize, usize), (usize, usize))> {
        let start = self.free_near(self.cell(de[0], de[1])?, se_ve)?;
        let goal = self.cell(a[0], a[1])?;
        let meta = self.free_near(goal, &|_| true)?;
        let idx = |c: (usize, usize)| c.1 * self.n + c.0;
        // the free cell nearest the goal shut in a small zone of its own (a house beside a flag's pole): the nearest one of the start's
        // zone instead, if it lies within `GOAL_ASIDE_CELLS` (DC_LostVillage: every flag's nearest free cell was inside a building, and
        // the attackers never got a path to any)
        let (zs, zm) = (self.zone[idx(start)], self.zone[idx(meta)]);
        let meta = if aside && zs != zm && self.zone_cells[zm as usize] < self.zone_cells[zs as usize] { self.exit_within(goal, zs, GOAL_ASIDE_CELLS).unwrap_or(meta) } else { meta };
        let start = if self.zone[idx(start)] == self.zone[idx(meta)] {
            start
        } else if leaving {
            self.exit(start, self.zone[idx(meta)], se_ve)?
        } else {
            return None;
        };
        Some((start, meta))
    }

    fn h(c: (usize, usize), meta: (usize, usize)) -> u32 {
        let (dx, dz) = (c.0.abs_diff(meta.0) as u32, c.1.abs_diff(meta.1) as u32);
        dx.max(dz) * 10 + dx.min(dz) * 4
    }

    /// Prepares `t` for a new search: whatever was in it stops counting at once.
    fn begin(&self, t: &mut Work, start: (usize, usize), meta: (usize, usize), avoid: &[[f32; 2]]) {
        let cells = self.n * self.n;
        if t.nodes.len() != cells || t.stamp == u32::MAX {
            t.nodes.clear();
            t.nodes.resize(cells, Node::default());
            t.stamp = 0;
        }
        t.stamp += 1;
        t.open_ones.clear();
        // what must be avoided, as already seen and at no cost: no path improves it, and so nobody enters
        let r = (AVOID_MARGIN / CELL).ceil() as i64;
        for m in avoid {
            let Some(c) = self.cell(m[0], m[1]) else { continue };
            for dz in -r..=r {
                for dx in -r..=r {
                    let (x, z) = (c.0 as i64 + dx, c.1 as i64 + dz);
                    if dx * dx + dz * dz <= r * r && x >= 0 && z >= 0 && (x as usize) < self.n && (z as usize) < self.n {
                        t.nodes[z as usize * self.n + x as usize] = Node { stamp: t.stamp, cost: 0, came: u32::MAX };
                    }
                }
            }
        }
        let i = start.1 * self.n + start.0;
        t.nodes[i] = Node { stamp: t.stamp, cost: 0, came: u32::MAX };
        let h = Self::h(start, meta);
        t.open_ones.push((Reverse(h), Reverse(h), Reverse(i as u32)));
    }

    /// Continues a search opening at most `quota` nodes; also returns those it has opened.
    fn advance(&self, t: &mut Work, meta: (usize, usize), start: (usize, usize), quota: u32) -> (Search, u32) {
        let n = self.n;
        let idx = |c: (usize, usize)| c.1 * n + c.0;
        let stamp = t.stamp;
        let mut already_open = 0;
        while already_open < quota {
            let Some((_, _, Reverse(i))) = t.open_ones.pop() else { return (Search::NoPath, already_open) };
            already_open += 1;
            let i = i as usize;
            let c = (i % n, i / n);
            if c == meta {
                let mut cells = vec![c];
                let mut j = i;
                while j != idx(start) {
                    j = t.nodes[j].came as usize;
                    cells.push((j % n, j / n));
                }
                cells.reverse();
                return (Search::Cells(cells), already_open);
            }
            for (dx, dz) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (x, z) = (c.0 as i64 + dx, c.1 as i64 + dz);
                if x < 0 || z < 0 || x as usize >= n || z as usize >= n {
                    continue;
                }
                let d = (x as usize, z as usize);
                if !self.free(d) || (dx != 0 && dz != 0 && (!self.free((d.0, c.1)) || !self.free((c.0, d.1)))) {
                    continue;
                }
                let g = t.cost(i) + if dx != 0 && dz != 0 { 14 } else { 10 };
                if g < t.cost(idx(d)) {
                    t.nodes[idx(d)] = Node { stamp, cost: g, came: i as u32 };
                    let h = Self::h(d, meta);
                    t.open_ones.push((Reverse(g + h), Reverse(h), Reverse(idx(d) as u32)));
                }
            }
        }
        (Search::Follows, already_open)
    }

    /// The points to pass through: `exit_aside` if the first cell is not where it is.
    fn points(&self, cells: &[(usize, usize)], exit_aside: bool, avoid: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let mut points = self.straight(cells, avoid);
        if exit_aside {
            points.insert(0, Self::centre(cells[0]));
        }
        points
    }

    /// From the list of cells, the corners: from each point, to the farthest cell reachable in a straight line, without exceeding
    /// [`MAX_LEG`] cells (looking at ever longer lines in open country cost up to 60 ms on a 1 km path).
    fn straight(&self, cells: &[(usize, usize)], avoid: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < cells.len() {
            let mut j = i + 1;
            let fin = (i + MAX_LEG).min(cells.len() - 1);
            let mut k = (i + 2).min(fin);
            while k > j && k < cells.len() {
                if self.recta(cells[i], cells[k]) && !avoid.iter().any(|m| near_leg(Self::centre(cells[i]), Self::centre(cells[k]), *m, AVOID_MARGIN)) {
                    j = k;
                    k = (k + 4).min(fin);
                    if k == j {
                        break;
                    }
                } else {
                    break;
                }
            }
            out.push(Self::centre(cells[j]));
            i = j;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B20: a closed place is a zone of its own, far smaller than the map around it; the walker tells by that it is shut in.
    #[test]
    fn b20_a_closed_place_is_a_small_zone_of_its_own() {
        let n = 20;
        let mut blocked = vec![false; n * n];
        for k in 4..=8 {
            for (x, z) in [(k, 4), (k, 8), (4, k), (8, k)] {
                blocked[z * n + x] = true;
            }
        }
        let g = Grid::new_one(n, blocked);
        let (inside, outside, wall) = (g.zone_at(12.5, 12.5).unwrap(), g.zone_at(30.5, 30.5).unwrap(), g.zone_at(8.5, 8.5).unwrap());
        assert_eq!(wall, 0);
        assert!(inside != 0 && outside != 0 && inside != outside);
        assert_eq!(g.zone_size(inside), 9);
        assert_eq!(g.zone_size(outside), 400 - 25);
        assert_eq!(g.zone_at(-1.0, 3.0), None);
        assert!(g.path([12.5, 12.5], [30.5, 30.5]).is_none());
    }

    fn pared() -> Grid {
        // 10×10 cells, a wall at x = 4 from z = 0 to z = 7
        let n = 10;
        let mut blocked = vec![false; n * n];
        for z in 0..8 {
            blocked[z * n + 4] = true;
        }
        Grid::new_one(n, blocked)
    }

    #[test]
    fn goes_round_the_wall() {
        let g = pared();
        let p = g.path([1.0, 1.0], [17.0, 1.0]).unwrap();
        assert!(p.iter().any(|w| w[1] > 8.0 * CELL), "{p:?}");
        assert!(p.len() < 8, "it has to join the straight stretches: {p:?}");
        // no leg crosses the wall
        let mut de = [1.0, 1.0];
        for w in &p {
            for i in 0..=50 {
                let t = i as f32 / 50.0;
                let c = g.cell(de[0] + (w[0] - de[0]) * t, de[1] + (w[1] - de[1]) * t).unwrap();
                assert!(g.free(c), "goes through {c:?}: {p:?}");
            }
            de = *w;
        }
    }

    #[test]
    fn a_one_metre_map_keeps_the_thin_walls() {
        let (w, h) = (20, 20);
        let mut pixels = vec![0u8; w * h];
        for y in 0..h {
            pixels[y * w + 9] = 1;
        }
        let g = Grid::from_map(&PathMap { width: w, height: h, bpp: 1, pixels });
        assert_eq!(g.path([2.0, 2.0], [16.0, 2.0]), None, "una pared de 1 m cierra el step");
    }

    #[test]
    fn from_a_blocked_cell_it_leaves_by_the_visible_side() {
        // at the wall (x = 4), to the right although the left is equally near
        let g = pared();
        let de = [(4.0 + 0.5) * CELL, 3.0 * CELL];
        let p = g.path_from(de, [17.0, 1.0], &|c| c[0] > de[0]).unwrap();
        assert!(p[0][0] > de[0], "{p:?}");
    }

    #[test]
    fn from_a_closed_place_it_leaves_to_the_goal_zone() {
        // a closed box around (1, 1): the goal is outside
        let n = 12;
        let mut b = vec![false; n * n];
        for k in 0..4 {
            b[3 * n + k] = true;
            b[k * n + 3] = true;
        }
        let g = Grid::new_one(n, b);
        let (de, meta) = ([3.0, 3.0], [20.0, 20.0]);
        assert_eq!(g.path_from(de, meta, &|_| false), None, "if there is no way out anywhere, there is no path");
        let p = g.path_from(de, meta, &|_| true).unwrap();
        assert!(p[0][0] > 6.0 || p[0][1] > 6.0, "{p:?}");
        // and it does not leave where `se_ve` does not allow (a cliff)
        let p = g.path_from(de, meta, &|c| c[0] > 6.0).unwrap();
        assert!(p[0][0] > 6.0, "{p:?}");
    }

    /// A wall-to-wall map: a comb that forces you to cover all of it.
    fn comb() -> Grid {
        let n = 1024;
        let mut b = vec![false; n * n];
        for x in (8..n - 8).step_by(8) {
            for z in 0..n - 4 {
                b[if (x / 8) % 2 == 0 { z } else { z + 4 } * n + x] = true;
            }
        }
        Grid::new_one(n, b)
    }

    #[test]
    fn a_long_search_is_split_over_ticks_and_gives_the_same_path() {
        let g = comb();
        let (de, a) = ([1.0, 1.0], [2040.0, 1.0]);
        let mut b = Searcher::default();
        let mut tics = 0;
        let path = loop {
            match b.request(&g, tics + 1, 7, de, a, &|_| true, false, &[], false) {
                Answer::Path(c) => break c,
                Answer::Wait => assert!(b.spent <= NODES_PER_TIC, "{}", b.spent),
                Answer::NoPath => panic!("sin path"),
            }
            tics += 1;
        };
        assert!(tics > 2, "no se ha repartido: {tics}");
        assert_eq!(Some(path), g.path(de, a));
    }

    #[test]
    fn whoever_stops_asking_loses_its_turn() {
        let g = comb();
        let (de, a) = ([1.0, 1.0], [2040.0, 1.0]);
        let mut b = Searcher::default();
        assert_eq!(b.request(&g, 1, 7, de, a, &|_| true, false, &[], false), Answer::Wait);
        // another, to the queue; the first no longer returns
        assert_eq!(b.request(&g, 1, 8, de, a, &|_| true, false, &[], false), Answer::Wait);
        assert_eq!(b.request(&g, 2, 8, de, a, &|_| true, false, &[], false), Answer::Wait);
        assert_eq!(b.request(&g, 3, 8, de, a, &|_| true, false, &[], false), Answer::Wait);
        assert_eq!(b.queue.len(), 1);
        assert_eq!(b.queue[0].whoever, 8);
    }

    #[test]
    fn short_searches_do_not_wait_for_long_ones() {
        let g = comb();
        let mut b = Searcher::default();
        assert_eq!(b.request(&g, 1, 7, [1.0, 1.0], [2040.0, 1.0], &|_| true, false, &[], false), Answer::Wait);
        assert!(matches!(b.request(&g, 2, 8, [1.0, 1.0], [10.0, 1.0], &|_| true, false, &[], false), Answer::Path(_)));
    }

    #[test]
    fn the_path_goes_round_what_must_be_avoided_if_possible() {
        let g = Grid::new_one(64, vec![false; 64 * 64]);
        let (de, a) = ([10.0, 60.0], [110.0, 60.0]);
        let mine = [60.0, 60.0];
        let mut b = Searcher::default();
        let Answer::Path(p) = b.request(&g, 1, 1, de, a, &|_| true, false, &[mine], false) else { panic!() };
        let mut since = de;
        for w in &p {
            assert!(!near_leg(since, *w, mine, AVOID_MARGIN - 2.0), "{p:?}");
            since = *w;
        }
        // in a corridor with no other exit, it passes anyway
        let mut pared = vec![true; 64 * 64];
        for x in 0..64 {
            pared[30 * 64 + x] = false;
        }
        let g = Grid::new_one(64, pared);
        let (de, a) = ([10.0, 61.0], [110.0, 61.0]);
        assert!(matches!(b.request(&g, 2, 1, de, a, &|_| true, false, &[[60.0, 61.0]], false), Answer::Path(_)));
    }

    #[test]
    fn one_searchs_memory_does_not_dirty_the_next() {
        let g = pared();
        let first = g.path([1.0, 1.0], [17.0, 1.0]).unwrap();
        let mut b = Searcher::default();
        for tic in 1..4 {
            assert_eq!(b.request(&g, tic, 1, [1.0, 1.0], [17.0, 1.0], &|_| true, false, &[], false), Answer::Path(first.clone()));
            assert!(matches!(b.request(&g, tic, 1, [17.0, 1.0], [1.0, 1.0], &|_| true, false, &[], false), Answer::Path(_)));
        }
    }

    #[test]
    fn outside_the_map_there_is_no_path() {
        assert_eq!(pared().path([-5.0, 1.0], [1.0, 1.0]), None);
    }
}
