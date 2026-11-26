//! What the bots ask of the level's navigation, behind one trait with two answers: the walkable-surface mesh ([`MeshNav`]),
//! which knows several floors over one spot, for the soldiers, and the flat path grid of cells of 2 m ([`Grid`], the first
//! version, on foot until bots 1.0) for the vehicles. Most questions are flat (a point is x, z): on the mesh, such a point stands on the polygon on the terrain
//! there, else on the lowest one. The walker's own come from its feet ([`NavBackend::zone_at_feet`],
//! [`NavBackend::sight_from`], [`NavBackend::request_from`]): on the mesh, from the floor it stands on (a bridge's deck, not
//! the ground under it); the grid, with one floor, leaves the height out.
// TODO: `way_out`, `break_out` and the learnt doors cut back where the mesh makes them needless. Turning them (and the walker's
// running blind) off wherever the mesh holds the bot's feet made a short sweep worse (143 → 434 s in 8 rounds of berlin, Wake,
// DC Berlin and DC_Al_Nas): they still get bots out of rooms the mesh does not join to the rest. Try again once the stuck places
// on the mesh are few, place by place.
use super::paths::{Answer, Grid, Searcher, CELL, GOAL_ASIDE_CELLS, WAY_UNIT_M};
use crate::navmesh::query::{Progress, Query, Search};
use std::collections::VecDeque;
use std::sync::Arc;

/// The navigation a round's bots share for one way of moving (on foot, in a tank).
pub trait NavBackend: Send + Sync {
    /// The connected zone at a point: 0 where nothing is walked, `None` outside the map. Two points in different zones have
    /// no way between them.
    fn zone_at(&self, x: f32, z: f32) -> Option<u32>;
    /// The zone of the walkable place nearest a point (within 64 m).
    fn zone_near(&self, x: f32, z: f32) -> Option<u32>;
    /// The zones within `r` of a point, without repeats.
    fn zones_near(&self, x: f32, z: f32, r: f32) -> Vec<u32>;
    /// A zone's area, in cells of [`CELL`] m.
    fn zone_size(&self, zone: u32) -> u32;
    /// Whether a point is walked; outside the map it is unknown, and taken as so.
    fn clear(&self, x: f32, z: f32) -> bool;
    /// Whether the straight line from `a` to `b` is walked all along, the first `skip` m not looked at.
    fn line_clear(&self, a: [f32; 2], b: [f32; 2], skip: f32) -> bool;
    /// Whether one walks straight from `a` to `b`.
    fn sight(&self, a: [f32; 2], b: [f32; 2]) -> bool;
    /// How far every place is from the circle of radius `r` around (x, z), in units of [`WAY_UNIT_M`]; read with
    /// [`NavBackend::way_in`].
    fn distances_to(&self, x: f32, z: f32, r: f32) -> Vec<u16>;
    fn way_in(&self, field: &[u16], x: f32, z: f32) -> Option<f32>;
    /// Down a field of [`NavBackend::distances_to`] from (x, z), the first point whose way is at most `left_m`, m (`None`: no way,
    /// or the backend cannot say).
    fn down_to(&self, _field: &[u16], _x: f32, _z: f32, _left_m: f32) -> Option<[f32; 2]> {
        None
    }
    /// The path from `de` to `a` for `whoever`, or let it wait ([`Searcher::request`]: the same queue, quota and turns).
    #[allow(clippy::too_many_arguments)]
    fn request(&self, s: &mut Searchers, tic: u64, whoever: usize, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, avoid: &[[f32; 2]], aside: bool) -> Answer;

    /// The zone at a soldier's feet (x, the height of its soles, z).
    fn zone_at_feet(&self, feet: [f32; 3]) -> Option<u32> {
        self.zone_at(feet[0], feet[2])
    }
    /// Whether one walks straight from a soldier's feet to `b`.
    fn sight_from(&self, feet: [f32; 3], b: [f32; 2]) -> bool {
        self.sight([feet[0], feet[2]], b)
    }
    /// [`NavBackend::line_clear`] from the floor at `feet` (x, its height, z): on the mesh, along the floor it stands on and not the
    /// terrain under a bridge; off it, across as the grid.
    fn line_clear_from(&self, feet: [f32; 3], b: [f32; 2], skip: f32) -> bool {
        self.line_clear([feet[0], feet[2]], b, skip)
    }
    /// A soldier at `feet` has gone [`crate::bots::body::walker`]'s jam time without getting nearer `next`, its path's next point:
    /// the way there is blocked by something the navigation does not know (a parked vehicle, a lip, the others in a doorway).
    /// The mesh learns it for every walker on the map; the grid does not.
    fn blocked(&self, _feet: [f32; 3], _next: [f32; 3]) {}
    /// Places to stand within `r` of a soldier's feet that it can walk to, nearest first, at most `max`: the centres of the mesh's
    /// polygons of the zone it stands in, no more than [`EXIT_DY`] up or down. The grid gives none.
    fn spots_near(&self, _feet: [f32; 3], _r: f32, _max: usize) -> Vec<[f32; 3]> {
        Vec::new()
    }
    /// The cover points of `cover` ([`crate::cover`], made over this mesh) within `r` of a soldier's feet that it can walk to, as
    /// [`NavBackend::spots_near`]: on polygons of the zone it stands in, no more than [`EXIT_DY`] up or down, nearest first, at most
    /// `max`. The grid gives none.
    fn cover_near(&self, _cover: &crate::cover::CoverPoints, _feet: [f32; 3], _r: f32, _max: usize) -> Vec<crate::cover::CoverPoint> {
        Vec::new()
    }
    /// Whether `next`, the point a soldier at `feet` heads for, is where a jump from about there lands (the mesh's jump
    /// links): it should jump now. Never on the grid.
    fn leap(&self, _feet: [f32; 3], _next: [f32; 3]) -> bool {
        false
    }
    /// [`NavBackend::request`] from a soldier's feet: the points carry the height of the floor at each, NaN where it is not
    /// known (all of them on the grid; a goal moved aside).
    #[allow(clippy::too_many_arguments)]
    fn request_from(&self, s: &mut Searchers, tic: u64, whoever: usize, feet: [f32; 3], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, avoid: &[[f32; 2]], aside: bool) -> Answer<[f32; 3]> {
        match self.request(s, tic, whoever, [feet[0], feet[2]], a, se_ve, leaving, avoid, aside) {
            Answer::Path(p) => Answer::Path(p.into_iter().map(|q| [q[0], f32::NAN, q[1]]).collect()),
            Answer::Wait => Answer::Wait,
            Answer::NoPath => Answer::NoPath,
        }
    }
}

/// The searches of one map, whichever navigation answers them.
#[derive(Default)]
pub struct Searchers {
    grid: Searcher,
    mesh: MeshSearcher,
}

impl Searchers {
    /// The tolls of the side of whoever asks next on the mesh (`None`: only the stuck places).
    pub fn set_extra_tolls(&mut self, tolls: Option<Arc<Vec<(u32, u32)>>>) {
        self.mesh.extra = tolls;
    }
}

impl NavBackend for Grid {
    fn zone_at(&self, x: f32, z: f32) -> Option<u32> {
        Grid::zone_at(self, x, z)
    }
    fn zone_near(&self, x: f32, z: f32) -> Option<u32> {
        Grid::zone_near(self, x, z)
    }
    fn zones_near(&self, x: f32, z: f32, r: f32) -> Vec<u32> {
        Grid::zones_near(self, x, z, r)
    }
    fn zone_size(&self, zone: u32) -> u32 {
        Grid::zone_size(self, zone)
    }
    fn clear(&self, x: f32, z: f32) -> bool {
        Grid::clear(self, x, z)
    }
    fn line_clear(&self, a: [f32; 2], b: [f32; 2], skip: f32) -> bool {
        Grid::line_clear(self, a, b, skip)
    }
    fn sight(&self, a: [f32; 2], b: [f32; 2]) -> bool {
        Grid::sight(self, a, b)
    }
    fn distances_to(&self, x: f32, z: f32, r: f32) -> Vec<u16> {
        Grid::distances_to(self, x, z, r)
    }
    fn way_in(&self, field: &[u16], x: f32, z: f32) -> Option<f32> {
        Grid::way_in(self, field, x, z)
    }
    fn down_to(&self, field: &[u16], x: f32, z: f32, left_m: f32) -> Option<[f32; 2]> {
        Grid::down_to(self, field, x, z, left_m)
    }
    fn request(&self, s: &mut Searchers, tic: u64, whoever: usize, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, avoid: &[[f32; 2]], aside: bool) -> Answer {
        s.grid.request(self, tic, whoever, de, a, se_ve, leaving, avoid, aside)
    }
}

/// Polygons a search on the mesh opens without waiting its turn, and among all the bots per tick: a polygon costs several
/// cells of the grid (its steps, their distances), and a mesh has 2–25 thousand of them, so a search from one end of a map
/// to the other fits in a tick or two. With 64 bots walking on the mesh (`examples/step_spikes`, 40 s) no step goes over the
/// grid's slowest on Berlin, Wake, Market_Garden, DC_Al_Nas, GC_Corellia and DC_Medina_Ridge (Corellia's fifty over 10 ms are
/// gone), the mean up half a millisecond at most.
const POLYS_SHORT: u32 = 2_000;
const POLYS_PER_TIC: u32 = 8_000;
/// How close to what must be avoided a path does not go, m (as on the grid).
const AVOID_MARGIN: f32 = 10.0;
/// How far from a point the walkable place nearest it is looked for, m (as the grid's 32 cells).
const NEAR_M: f32 = 64.0;
/// How far from where it stands a way out into the goal's zone is looked for, m (as the grid's 64 cells).
const EXIT_M: f32 = 128.0;
/// How much a way out up or down counts per metre of height, as metres across.
const EXIT_CLIMB: f32 = 4.0;
/// How far up or down a way out may be, m: a soldier shut in a ship's bridge does not walk out to the hangar 250 m below
/// (GC_Judicator, where the way down is a lift).
const EXIT_DY: f32 = 4.0;
/// How many places `se_ve` is asked about at most, nearest first (as on the grid).
const TESTS: usize = 200;
/// How far from a soldier's feet the floor it stands on is looked for, m: across (the mesh keeps the soldier's radius off
/// the walls) and up or down (on a step, in the air of a jump).
const FEET_R: f32 = 1.0;
/// How near the spot a jump leaves from a soldier jumps, m: the run-up is the way to it.
const TAKEOFF_M: f32 = 1.2;
const FEET_DY: f32 = 1.5;

/// The walkable-surface mesh behind the bots' flat questions.
pub struct MeshNav {
    pub q: Query,
    /// where walkers got stuck on it ([`NavBackend::blocked`]): the spot across, how many times, the height of the feet
    blocked: std::sync::Mutex<Vec<([f32; 2], u32, f32)>>,
}

/// A place where walkers got stuck: the polygons within [`BLOCKED_R`] of it cost [`BLOCKED_COST_M`] more to go into for each
/// time, up to [`BLOCKED_MAX`] times; two within [`BLOCKED_SAME_M`] are one; a map keeps [`BLOCKED_SPOTS`] at most.
const BLOCKED_R: f32 = 1.0;
const BLOCKED_COST_M: f32 = 20.0;
const BLOCKED_MAX: u32 = 3;
const BLOCKED_SAME_M: f32 = 1.0;
const BLOCKED_SPOTS: usize = 512;

impl MeshNav {
    pub fn new(mesh: Arc<crate::navmesh::NavMesh>) -> MeshNav {
        MeshNav { q: Query::new(mesh), blocked: Default::default() }
    }

    /// Puts the places where walkers got stuck on a search just begun, as tolls on the polygons by them.
    fn tolls(&self, s: &mut crate::navmesh::query::Search) {
        let b = self.blocked.lock().unwrap_or_else(|e| e.into_inner());
        for &(spot, n, y) in b.iter() {
            for (p, c) in self.q.polys_near(spot[0], spot[1], BLOCKED_R, usize::MAX) {
                if (c[1] - y).abs() < FEET_DY {
                    s.toll(p, (n as f32 * BLOCKED_COST_M * 100.0) as u32);
                }
            }
        }
    }

    /// The polygon a soldier stands on, from its feet; in the air or off the mesh, `None`.
    pub fn at_feet(&self, feet: [f32; 3]) -> Option<u32> {
        let p = self.q.locate(feet, FEET_R, FEET_DY)?;
        // (not a floor more than a jump over the feet: a soldier in a hole off the mesh took the floor 3 m over it for its own and
        // pushed against the wall below it for two minutes, Mos_Eisley 518,5,478)
        if self.q.mesh.closest_on(p, feet[0], feet[2])[1] - feet[1] <= crate::navmesh::JUMP_UP {
            return Some(p);
        }
        self.q.polys_near(feet[0], feet[2], FEET_R, usize::MAX).into_iter().find(|(_, c)| (-FEET_DY..=crate::navmesh::JUMP_UP).contains(&(c[1] - feet[1]))).map(|x| x.0)
    }

    /// The polygon at (x, z) and the point on it: the one there, else the nearest within `r` that `ok` accepts.
    fn spot(&self, p: [f32; 2], r: f32, ok: &dyn Fn([f32; 2]) -> bool) -> Option<(u32, [f32; 3])> {
        if let Some(poly) = self.q.locate_2d(p[0], p[1]) {
            return Some((poly, [p[0], self.q.mesh.height(poly, p[0], p[1]), p[1]]));
        }
        self.q.polys_near(p[0], p[1], r, TESTS).into_iter().find(|(_, c)| ok([c[0], c[2]]))
    }

    /// The nearest place of zone `zone` within `r` of `p` that `ok` accepts.
    fn in_zone(&self, p: [f32; 2], zone: u32, r: f32, ok: &dyn Fn([f32; 2]) -> bool) -> Option<(u32, [f32; 3])> {
        self.q.polys_near_in(p[0], p[1], r, usize::MAX, |poly| self.q.zone[poly as usize] == zone).into_iter().take(TESTS).find(|(_, c)| ok([c[0], c[2]]))
    }

    /// [`MeshNav::in_zone`] from a soldier's feet: a place up or down counts [`EXIT_CLIMB`] times its height as distance (the
    /// roof right over a room shut in is not its way out), and none more than [`EXIT_DY`] up or down.
    fn in_zone_from(&self, feet: [f32; 3], zone: u32, r: f32, ok: &dyn Fn([f32; 2]) -> bool) -> Option<(u32, [f32; 3])> {
        let mut near: Vec<(f32, u32, [f32; 3])> = self.q.polys_near_in(feet[0], feet[2], r, usize::MAX, |poly| self.q.zone[poly as usize] == zone).into_iter().filter(|(_, c)| (c[1] - feet[1]).abs() <= EXIT_DY).map(|(poly, c)| ((c[0] - feet[0]).hypot(c[2] - feet[2]) + EXIT_CLIMB * (c[1] - feet[1]).abs(), poly, c)).collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        // (first what is no higher than a jump: a soldier in a hole off the mesh was sent to the floor 3 m straight over it, Mos_Eisley
        // 518,5,478, for two minutes)
        let reachable = |c: &[f32; 3]| c[1] - feet[1] <= crate::navmesh::JUMP_UP;
        let first = near.iter().filter(|n| reachable(&n.2)).take(TESTS).map(|&(_, poly, c)| (poly, c)).find(|(_, c)| ok([c[0], c[2]]));
        // (the reachable ones among the nearest were all asked in the first pass, in the same order, and said no)
        first.or_else(|| near.into_iter().take(TESTS).filter(|n| !reachable(&n.2)).map(|(_, poly, c)| (poly, c)).find(|(_, c)| ok([c[0], c[2]])))
    }

    /// The ends of a path from `de` to `a`, as [`Grid`]'s: the start (and whether it is not where it stands), the goal (and
    /// whether it was moved beside a small zone of its own, into the start's). With the height of the feet, the start is
    /// the floor they stand on.
    #[allow(clippy::type_complexity)]
    fn ends(&self, de: [f32; 2], feet: Option<f32>, a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, _aside: bool, no_exit: &mut NoExit) -> Option<((u32, [f32; 3]), bool, (u32, [f32; 3]), bool)> {
        let here = match feet {
            Some(y) => self.at_feet([de[0], y, de[1]]),
            None => self.q.locate_2d(de[0], de[1]),
        };
        let mut start = match (here, feet) {
            (Some(p), _) => (p, self.q.mesh.closest_on(p, de[0], de[1])),
            // (off the mesh, the nearest floor it can get onto: none more than a jump over its feet, if there is one)
            (None, Some(y)) => {
                let near = self.q.polys_near(de[0], de[1], NEAR_M, TESTS);
                // (first a floor about the feet's height: the one under them, a storey down, was nearer across than the stairs' end
                // beside them, DC_Basrahs_Edge 314,7,398)
                let level = near.iter().copied().find(|(_, c)| (-FEET_DY..=crate::navmesh::JUMP_UP).contains(&(c[1] - y)) && se_ve([c[0], c[2]]));
                // (else a floor below, but no deeper than a soldier drops: off the mesh on a bridge, DC_Operation_Bragg's at 679,1387
                // with its deck at 42 m, the river 36 m below is not where it stands)
                let low = level.or_else(|| near.iter().copied().find(|(_, c)| (-crate::navmesh::SOLDIER_DROP_MAX..=crate::navmesh::JUMP_UP).contains(&(c[1] - y)) && se_ve([c[0], c[2]])));
                match low {
                    Some(x) => x,
                    None => self.spot(de, NEAR_M, se_ve)?,
                }
            }
            (None, None) => self.spot(de, NEAR_M, se_ve)?,
        };
        let mut goal = self.spot(a, NEAR_M, &|_| true)?;
        let zone = |p: (u32, [f32; 3])| self.q.zone[p.0 as usize];
        let mut goal_aside = false;
        // (on the mesh a goal on a floor nobody reaches, a storey of a house with no stairs, is not asked for as a way out of
        // the street: whether or not the walker asks for it aside, it goes to the nearest place of its own)
        if zone(start) != zone(goal)
            && self.q.zone_area[zone(goal) as usize] < self.q.zone_area[zone(start) as usize]
            && let Some(g) = self.in_zone(a, zone(start), GOAL_ASIDE_CELLS as f32 * CELL, &|_| true)
        {
            goal = g;
            goal_aside = true;
        }
        if zone(start) != zone(goal) {
            if !leaving {
                return None;
            }
            let looked = no_exit.key(de, feet, zone(goal), se_ve);
            if looked.is_some() && no_exit.failed == looked {
                return None;
            }
            let exit = match feet {
                Some(y) => self.in_zone_from([de[0], y, de[1]], zone(goal), EXIT_M, se_ve),
                None => self.in_zone(de, zone(goal), EXIT_M, se_ve),
            };
            if exit.is_none() {
                no_exit.failed = looked;
            }
            start = exit?;
        }
        Some((start, here != Some(start.0), goal, goal_aside))
    }
}

impl NavBackend for MeshNav {
    fn cover_near(&self, cover: &crate::cover::CoverPoints, feet: [f32; 3], r: f32, max: usize) -> Vec<crate::cover::CoverPoint> {
        let Some(here) = self.at_feet(feet) else { return Vec::new() };
        let zone = self.q.zone[here as usize];
        let mut out: Vec<(f32, crate::cover::CoverPoint)> = cover
            .near(feet[0], feet[2], r)
            .filter(|c| self.q.zone.get(c.poly as usize) == Some(&zone) && (c.pos[1] - feet[1]).abs() <= EXIT_DY)
            .map(|c| ((c.pos[0] - feet[0]).hypot(c.pos[2] - feet[2]), *c))
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.truncate(max);
        out.into_iter().map(|x| x.1).collect()
    }
    fn spots_near(&self, feet: [f32; 3], r: f32, max: usize) -> Vec<[f32; 3]> {
        let Some(here) = self.at_feet(feet) else { return Vec::new() };
        let zone = self.q.zone[here as usize];
        let mut out: Vec<(f32, [f32; 3])> = self
            .q
            .polys_near_in(feet[0], feet[2], r, usize::MAX, |poly| self.q.zone[poly as usize] == zone)
            .into_iter()
            .map(|(poly, _)| self.q.mesh.centre(poly))
            .filter(|c| (c[1] - feet[1]).abs() <= EXIT_DY)
            .map(|c| ((c[0] - feet[0]).hypot(c[2] - feet[2]), c))
            .filter(|x| x.0 <= r)
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.truncate(max);
        out.into_iter().map(|x| x.1).collect()
    }
    fn zone_at(&self, x: f32, z: f32) -> Option<u32> {
        let ws = self.q.mesh.side as f32 * self.q.mesh.tile_m();
        if !(0.0..ws).contains(&x) || !(0.0..ws).contains(&z) {
            return None;
        }
        Some(self.q.locate_2d(x, z).map_or(0, |p| self.q.zone[p as usize]))
    }
    fn zone_near(&self, x: f32, z: f32) -> Option<u32> {
        self.spot([x, z], NEAR_M, &|_| true).map(|p| self.q.zone[p.0 as usize])
    }
    fn zones_near(&self, x: f32, z: f32, r: f32) -> Vec<u32> {
        let mut out = Vec::new();
        for (p, _) in self.q.polys_near(x, z, r, usize::MAX) {
            let zn = self.q.zone[p as usize];
            if !out.contains(&zn) {
                out.push(zn);
            }
        }
        out
    }
    fn zone_size(&self, zone: u32) -> u32 {
        self.q.zone_area.get(zone as usize).map_or(0, |a| (a / (CELL * CELL)) as u32)
    }
    fn clear(&self, x: f32, z: f32) -> bool {
        self.zone_at(x, z) != Some(0)
    }
    fn line_clear(&self, a: [f32; 2], b: [f32; 2], skip: f32) -> bool {
        let d = (b[0] - a[0]).hypot(b[1] - a[1]);
        if skip >= d {
            return true;
        }
        let f = skip / d.max(1e-6);
        let s = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
        self.q.locate_2d(s[0], s[1]).is_some_and(|p| self.q.raycast(p, s, b).is_some())
    }
    fn sight(&self, a: [f32; 2], b: [f32; 2]) -> bool {
        self.line_clear(a, b, 0.0)
    }
    fn distances_to(&self, x: f32, z: f32, r: f32) -> Vec<u16> {
        let unit = (WAY_UNIT_M * 100.0) as u32;
        self.q.distances_to(x, z, r).into_iter().map(|c| if c == u32::MAX { u16::MAX } else { (c / unit).min(u16::MAX as u32 - 1) as u16 }).collect()
    }
    fn way_in(&self, field: &[u16], x: f32, z: f32) -> Option<f32> {
        let (p, _) = self.spot([x, z], NEAR_M, &|_| true)?;
        let v = *field.get(p as usize)?;
        (v != u16::MAX).then_some(v as f32 * WAY_UNIT_M)
    }
    fn request(&self, s: &mut Searchers, tic: u64, whoever: usize, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, avoid: &[[f32; 2]], aside: bool) -> Answer {
        match s.mesh.request(self, tic, whoever, de, None, a, se_ve, leaving, avoid, aside) {
            Answer::Path(p) => Answer::Path(p.into_iter().map(|q| [q[0], q[2]]).collect()),
            Answer::Wait => Answer::Wait,
            Answer::NoPath => Answer::NoPath,
        }
    }
    fn zone_at_feet(&self, feet: [f32; 3]) -> Option<u32> {
        match self.at_feet(feet) {
            Some(p) => Some(self.q.zone[p as usize]),
            None => self.zone_at(feet[0], feet[2]),
        }
    }
    fn sight_from(&self, feet: [f32; 3], b: [f32; 2]) -> bool {
        match self.at_feet(feet) {
            Some(p) => self.q.raycast(p, [feet[0], feet[2]], b).is_some(),
            None => self.sight([feet[0], feet[2]], b),
        }
    }
    fn line_clear_from(&self, feet: [f32; 3], b: [f32; 2], skip: f32) -> bool {
        match self.at_feet(feet) {
            Some(p) => self.q.raycast(p, [feet[0], feet[2]], b).is_some(),
            None => self.line_clear([feet[0], feet[2]], b, skip),
        }
    }
    fn blocked(&self, feet: [f32; 3], next: [f32; 3]) {
        // the spot a metre ahead, towards the next point (or the point itself, nearer)
        let (dx, dz) = (next[0] - feet[0], next[2] - feet[2]);
        let l = dx.hypot(dz);
        if !l.is_finite() {
            return;
        }
        let k = (1.0 / l.max(1e-3)).min(1.0);
        let spot = [feet[0] + k * dx, feet[2] + k * dz];
        let mut b = self.blocked.lock().unwrap_or_else(|e| e.into_inner());
        match b.iter().position(|q| (q.0[0] - spot[0]).hypot(q.0[1] - spot[1]) < BLOCKED_SAME_M) {
            Some(i) => b[i].1 = (b[i].1 + 1).min(BLOCKED_MAX),
            None if b.len() < BLOCKED_SPOTS => b.push((spot, 1, feet[1])),
            None => {}
        }
    }
    fn leap(&self, feet: [f32; 3], next: [f32; 3]) -> bool {
        if next[1].is_nan() {
            return false;
        }
        let near = |a: [f32; 3], b: [f32; 3], r: f32| (a[0] - b[0]).hypot(a[2] - b[2]) < r && (a[1] - b[1]).abs() < 0.5;
        self.q.polys_near(feet[0], feet[2], TAKEOFF_M, 6).into_iter().filter(|(_, q)| (q[1] - feet[1]).abs() < FEET_DY).any(|(p, _)| {
            self.q.links_from(p).any(|(kind, from, to)| kind == crate::navmesh::LinkKind::Jump && near(to, next, 0.3) && (from[0] - feet[0]).hypot(from[2] - feet[2]) < TAKEOFF_M && (from[1] - feet[1]).abs() < 0.6)
        })
    }
    fn request_from(&self, s: &mut Searchers, tic: u64, whoever: usize, feet: [f32; 3], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, avoid: &[[f32; 2]], aside: bool) -> Answer<[f32; 3]> {
        s.mesh.request(self, tic, whoever, [feet[0], feet[2]], Some(feet[1]), a, se_ve, leaving, avoid, aside)
    }
}

/// A long search on the mesh, in the queue.
struct MeshRequest {
    whoever: usize,
    avoid: Vec<[f32; 2]>,
    requested_goal: [f32; 2],
    start: (u32, [f32; 3]),
    goal: (u32, [f32; 3]),
    exit_aside: bool,
    goal_aside: bool,
    seen: u64,
    started: bool,
}

/// The mesh's searches of a map, as [`Searcher`]'s on the grid: short ones at once, long ones over successive ticks one at a
/// time in order of arrival, never more than [`POLYS_PER_TIC`] polygons a tick; whoever stops asking loses its turn.
#[derive(Default)]
pub struct MeshSearcher {
    tic: u64,
    spent: u32,
    queue: VecDeque<MeshRequest>,
    short: Search,
    long: Search,
    no_exit: NoExit,
    /// the tolls of the side of whoever asks now, on top of the stuck places (`bots:memory`, [`crate::bots::memory`])
    extra: Option<Arc<Vec<(u32, u32)>>>,
}

/// The way out of its zone a bot looked for in vain in this tick: the walker asks for up to five points in one thought with the
/// same test, and when they lie in one zone the way out fails alike for all; looking again cost ~3 ms each (on DC_LostVillage, a
/// soldier shut in a pocket at its base made a step of 20 ms every 3 s with 64 bots). The test is told apart by its address, so a
/// closure that captures nothing (every one at the same address) is never remembered.
#[derive(Default)]
pub struct NoExit {
    /// tick and bot the memory is for
    of: (u64, usize),
    /// the feet (or the point), the goal's zone and the test of the search that failed
    failed: Option<([u32; 3], u32, usize)>,
}

impl NoExit {
    fn begin(&mut self, tic: u64, whoever: usize) {
        if self.of != (tic, whoever) {
            self.of = (tic, whoever);
            self.failed = None;
        }
    }
    fn key(&self, de: [f32; 2], feet: Option<f32>, zone: u32, test: &dyn Fn([f32; 2]) -> bool) -> Option<([u32; 3], u32, usize)> {
        (std::mem::size_of_val(test) > 0).then(|| ([de[0].to_bits(), feet.unwrap_or(f32::NAN).to_bits(), de[1].to_bits()], zone, test as *const dyn Fn([f32; 2]) -> bool as *const () as usize))
    }
}

/// The points of a path on the mesh: from `start` (the first one if it is not where the bot stands) to the goal, with the
/// goal asked for last when it was moved aside (its height unknown).
fn points(nav: &MeshNav, corridor: &[crate::navmesh::query::Step], start: [f32; 3], goal: [f32; 3], exit_aside: bool, goal_aside: bool, asked: [f32; 2]) -> Vec<[f32; 3]> {
    let mut out = nav.q.funnel(start, corridor, goal);
    if exit_aside {
        out.insert(0, start);
    }
    if goal_aside {
        out.push([asked[0], f32::NAN, asked[1]]);
    }
    out
}

fn extra_tolls(s: &mut crate::navmesh::query::Search, extra: &Option<Arc<Vec<(u32, u32)>>>) {
    for &(p, cm) in extra.iter().flat_map(|x| x.iter()) {
        s.toll(p, cm);
    }
}

impl MeshSearcher {
    #[allow(clippy::too_many_arguments)]
    pub fn request(&mut self, nav: &MeshNav, tic: u64, whoever: usize, de: [f32; 2], feet: Option<f32>, a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, leaving: bool, avoid: &[[f32; 2]], aside: bool) -> Answer<[f32; 3]> {
        if tic != self.tic {
            self.tic = tic;
            self.spent = 0;
        }
        let queued = self.queue.iter().position(|p| p.whoever == whoever && p.requested_goal == a);
        if let Some(k) = queued {
            self.queue[k].seen = tic;
        } else {
            self.queue.retain(|p| p.whoever != whoever);
            self.no_exit.begin(tic, whoever);
            let Some((start, exit_aside, goal, goal_aside)) = nav.ends(de, feet, a, se_ve, leaving, aside, &mut self.no_exit) else { return Answer::NoPath };
            if self.spent < POLYS_PER_TIC {
                self.short.begin(&nav.q, start, goal, avoid, AVOID_MARGIN);
                nav.tolls(&mut self.short);
                extra_tolls(&mut self.short, &self.extra);
                let (r, n) = self.short.advance(&nav.q, POLYS_SHORT.min(POLYS_PER_TIC - self.spent));
                self.spent += n;
                match r {
                    Progress::Found(c) => return Answer::Path(points(nav, &c, start.1, goal.1, exit_aside, goal_aside, a)),
                    Progress::NoPath if !avoid.is_empty() => return self.request(nav, tic, whoever, de, feet, a, se_ve, leaving, &[], aside),
                    Progress::NoPath => return Answer::NoPath,
                    Progress::Follows => {}
                }
            }
            self.queue.push_back(MeshRequest { whoever, avoid: avoid.to_vec(), requested_goal: a, start, goal, exit_aside, goal_aside, seen: tic, started: false });
        }
        while self.queue.front().is_some_and(|p| p.seen + 1 < tic) {
            self.queue.pop_front();
        }
        if !self.queue.front().is_some_and(|p| p.whoever == whoever && p.requested_goal == a) || self.spent >= POLYS_PER_TIC {
            return Answer::Wait;
        }
        let p = &mut self.queue[0];
        let (start, goal, exit_aside, goal_aside) = (p.start, p.goal, p.exit_aside, p.goal_aside);
        let avoid_requested = std::mem::take(&mut p.avoid);
        if !std::mem::replace(&mut p.started, true) {
            self.long.begin(&nav.q, start, goal, &avoid_requested, AVOID_MARGIN);
            nav.tolls(&mut self.long);
            extra_tolls(&mut self.long, &self.extra);
        }
        let (r, n) = self.long.advance(&nav.q, POLYS_PER_TIC - self.spent);
        self.spent += n;
        match r {
            Progress::Follows => {
                self.queue[0].avoid = avoid_requested;
                Answer::Wait
            }
            Progress::NoPath => {
                self.queue.pop_front();
                if avoid_requested.is_empty() { Answer::NoPath } else { self.request(nav, tic, whoever, de, feet, a, se_ve, leaving, &[], aside) }
            }
            Progress::Found(c) => {
                self.queue.pop_front();
                Answer::Path(points(nav, &c, start.1, goal.1, exit_aside, goal_aside, a))
            }
        }
    }
}
