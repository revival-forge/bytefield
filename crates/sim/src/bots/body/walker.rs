//! Going to a point on foot: asks the path map for a path, follows it point by point, notices when it is not advancing (a low wall, a
//! corner) and then jumps, steps aside, drops that point or, if it still does not move, runs off jumping in a free
//! direction. Sometimes it stops for a while. Never walks straight off a cliff. It does not decide where to go: it receives the destination zone.
use super::rng::Rng;
use super::paths::Answer;
use super::{TURN, Maps, with_ground, angle_between, flat, no_fall, feet, pitch};
use crate::bot_api::BotView;
use crate::soldier::axis;

#[derive(Debug)]
pub struct Walker {
    /// the points left to the destination, with the height of the floor at each (NaN where the path map does not know it: the
    /// flat grid knows none)
    pub path: Vec<[f32; 3]>,
    /// the point it is heading to
    pub point: Option<[f32; 2]>,
    /// stopped until (round seconds)
    pub stopped_until: f64,
    // jam: where it was and since when it has not advanced
    pub anchor: [f32; 2],
    pub anchor_t: f64,
    pub best: f32,
    /// when it looks at the path again (in a vehicle)
    pub look_path: f64,
    // not moving from the spot although it wants to: from a balcony or a corner it gets out by jumping
    pub lookout: [f32; 2],
    pub lookout_t: f64,
    pub flight_until: f64,
    pub flight: [f32; 2],
    /// points it could not reach in this life: a low wall that neither the map nor the view knows
    pub vetoes: Vec<[f32; 2]>,
    /// the point it asked a path for, while the search runs
    pub requested_point: Option<[f32; 2]>,
    /// bots: the places it does not want to go through (where several of its mates have just fallen); empty in the plain bot
    pub avoid: Vec<[f32; 2]>,
    /// when its path last ran off the edge of what it stands on (a platform high over the ground): on foot it does not get there
    pub edge_t: f64,
    /// bots: where the path map knows no way (a depot with one door that the map draws closed), it looks for the way out with its
    /// eyes instead of standing there; false in the plain bot
    pub seeks_way_out: bool,
    /// the directions it has already run in from the place it is shut in, without getting out
    pub ways_tried: Vec<[f32; 2]>,
    /// where it started its last run for a way out, until the map gives it a path again
    pub ran_from: Option<[f32; 2]>,
    /// it gave up a point it could not get nearer to: where it stood and the point, for [`Walker::next_step`] to tell the map
    pub jammed: Option<([f32; 3], [f32; 3])>,
}

/// A point of the path this close is passed whether the next one is in sight or not, m.
pub const PASSED_M: f32 = 0.7;
/// A point of the path is reached only this close in height, m, when its height is known (from a bridge's deck, the ground
/// under it is not; nor the top of a ladder from its foot).
const LEVEL_M: f32 = 1.5;

/// A point of the path this far across or closer, and above or below it, is up or down a ladder (or a drop), m.
const LADDER_ACROSS_M: f32 = 3.0;
/// The pitch it climbs a ladder with, up or down, degrees: on a ladder, the soldier goes the way it looks.
const CLIMB_PITCH: f32 = 30.0;

/// A path's point across.
pub fn xz(p: [f32; 3]) -> [f32; 2] {
    [p[0], p[2]]
}

/// A point with no height known.
pub fn unknown_height(p: [f32; 2]) -> [f32; 3] {
    [p[0], f32::NAN, p[1]]
}

/// Whether the soles of a soldier at `pos` are level with point `p` (yes, when its height is not known).
pub fn level(pos: [f32; 3], p: [f32; 3]) -> bool {
    p[1].is_nan() || (feet(pos)[1] - p[1]).abs() < LEVEL_M
}
/// How far [`Walker::way_out`] looks for an opening, m.
const WAY_OUT_M: f32 = 14.0;
/// The largest zone of the path map taken as a closed place to get out of, in cells (60 × 60 m).
pub const POCKET_CELLS: u32 = 900;

/// Where the straight line from the bot to `goal` leaves its zone of the path map and where it enters another: the two ends of a
/// crossing the map does not know (it draws water as closed, and a soldier swims). `None` if the bot is not in a zone, the goal
/// is in its own, or the line meets no other zone within 400 m.
fn crossing(v: &BotView, maps: &Maps, goal: [f32; 2]) -> Option<([f32; 2], [f32; 2])> {
    let g = maps.foot(v)?;
    let pos = v.me.pos;
    // (standing on a cell the map draws blocked, the zone it walks out into: DC_Weapon_Bunkers' sides spawn in their bunkers, and
    // stood there all round with the river between them)
    let mine = g.zone_at_feet(feet(pos)).filter(|&z| z != 0).or_else(|| g.zone_near(pos[0], pos[2]).filter(|&z| z != 0))?;
    let l = flat(pos, goal);
    if l < 4.0 || g.zone_at(goal[0], goal[1]) == Some(mine) {
        return None;
    }
    let d = [(goal[0] - pos[0]) / l, (goal[1] - pos[2]) / l];
    let mut edge = [pos[0], pos[2]];
    let mut m = 0.0;
    // (at the height of its feet: across, the mesh gives the ground's zone first, and on GC_Bespin's platform, 260 m over it, the
    // line met another zone where the bot stood; it took its own place for the crossing and walked on, straight off the edge)
    let y = feet(pos)[1];
    while m <= l.min(400.0) {
        let q = [pos[0] + m * d[0], pos[2] + m * d[1]];
        match g.zone_at_feet([q[0], y, q[1]]) {
            Some(z) if z == mine => edge = q,
            Some(z) if z != 0 => return Some((edge, q)),
            _ => {}
        }
        m += 1.0;
    }
    None
}

/// Whether the straight line from `a` to `b` can be crossed although the map does not say so: no drop onto dry ground higher than a
/// soldier takes (into water it just swims).
fn crossable(v: &BotView, a: [f32; 3], b: [f32; 2]) -> bool {
    let n = (flat(a, b) / 2.0).ceil().max(1.0) as usize;
    let high = |i: usize| {
        let t = i as f32 / n as f32;
        v.ground(a[0] + (b[0] - a[0]) * t, a[2] + (b[1] - a[2]) * t)
    };
    (0..n).all(|i| match (high(i), high(i + 1)) {
        (Some(h0), Some(h1)) => h0 - h1 < super::MAX_FALL || h1 < v.water(),
        _ => true,
    })
}

/// Whether a soldier at `pos` walks straight to `b` without meeting a wall, at knee height ([`BotView::walk_blocked`]: unlike
/// [`BotView::hidden`], a building's walls count from inside too). Level, or up to the knee over the terrain at `b` if that is higher
/// (the terrain under a building's floor is lower than the floor: a line down to it met the floor's slab).
/// The way along the hull of a vehicle that stands between the bot and `step`, touching it: of the two sides, the one nearer `step`
/// that is walkable (else the other). `None` with no vehicle there.
fn round_vehicle(v: &BotView, step: [f32; 2]) -> Option<[f32; 2]> {
    // (the hull's reach on the ground: `radius` bounds everything, guns and aerials too: a T-3B's is 13 m)
    let hull = |x: &crate::bot_api::VehicleSeen| x.radius.min(ROUND_HULL_M);
    let pos = v.me.pos;
    let to = [step[0] - pos[0], step[1] - pos[2]];
    let n = to[0].hypot(to[1]).max(1e-3);
    let to = [to[0] / n, to[1] / n];
    let all = v.vehicles();
    let x = all.iter().find(|x| {
        let d = [x.pos[0] - pos[0], x.pos[2] - pos[2]];
        let m = d[0].hypot(d[1]);
        // (not only straight ahead: a hull is long, and its centre may lie well to one side of what it blocks)
        m < hull(x) + 1.5 && m > 0.1 && (d[0] * to[0] + d[1] * to[1]) / m > 0.0 && flat(x.pos, step) > hull(x)
    })?;
    let d = [x.pos[0] - pos[0], x.pos[2] - pos[2]];
    let m = d[0].hypot(d[1]);
    let (a, b) = ([-d[1] / m, d[0] / m], [d[1] / m, -d[0] / m]);
    let (first, second) = if a[0] * to[0] + a[1] * to[1] >= b[0] * to[0] + b[1] * to[1] { (a, b) } else { (b, a) };
    // (nor into another vehicle: two parked side by side shut a gap; with both sides shut, back away from it)
    let fits = |s: [f32; 2]| {
        let e = [pos[0] + 4.0 * s[0], pos[2] + 4.0 * s[1]];
        walkable(v, pos, e) && no_fall(v, pos, e) && !all.iter().any(|o| o.vehicle != x.vehicle && flat(o.pos, e) < hull(o))
    };
    [first, second, [-d[0] / m, -d[1] / m]].into_iter().find(|&s| fits(s))
}

pub fn walkable(v: &BotView, pos: [f32; 3], b: [f32; 2]) -> bool {
    let knee = pos[1] - 0.3;
    let to = v.ground(b[0], b[1]).map_or(knee, |h| knee.max(h + 0.7));
    !v.walk_blocked([pos[0], knee, pos[2]], [b[0], to, b[1]])
}

impl Default for Walker {
    fn default() -> Walker {
        Walker { path: Vec::new(), point: None, stopped_until: 0.0, anchor: [0.0; 2], anchor_t: 0.0, best: f32::MAX, look_path: 0.0, lookout: [0.0; 2], lookout_t: 0.0, flight_until: 0.0, flight: [0.0; 2], vetoes: Vec::new(), requested_point: None, avoid: Vec::new(), edge_t: f64::NEG_INFINITY, seeks_way_out: false, ways_tried: Vec::new(), ran_from: None, jammed: None }
    }
}

/// How long it runs in a direction that looks free when its side tells it that it is not getting anywhere, s.
const BREAK_OUT_S: f64 = 6.0;
/// How long it walks along a vehicle's hull that stands in its way ([`Walker::unjam`]), s.
const ROUND_VEHICLE_S: f64 = 2.0;
/// The most a vehicle's hull reaches from its centre on the ground, for [`Walker::unjam`] going round it, m.
const ROUND_HULL_M: f32 = 6.0;

impl Walker {
    /// On the first tick of the life.
    pub fn spawn(&mut self, v: &BotView) {
        self.anchor = [v.me.pos[0], v.me.pos[2]];
        self.anchor_t = v.time;
    }

    /// Forgets the point and the path (and that it was stopped): it has to think of another.
    pub fn replan(&mut self) {
        self.point = None;
        self.path.clear();
        self.stopped_until = 0.0;
    }

    /// The same, and also the path it was waiting for: changes zone.
    pub fn new_zone(&mut self) {
        self.point = None;
        self.requested_point = None;
        self.path.clear();
        self.stopped_until = 0.0;
    }

    /// A point to go to inside the flag's zone: far from where it is, to avoid going round in circles. `enters`: the attacker already enters; on
    /// the zone; `threatened`: there are enemies on top.
    #[allow(clippy::too_many_arguments)]
    pub fn new_point(&mut self, rng: &mut Rng, enters: bool, threatened: bool, centre: [f32; 3], radio: f32, attacks: bool, pos: [f32; 3], front: [f32; 3]) -> [f32; 2] {
        // with enemies on top, the defender enters the flag: a lone enemy inside takes it
        // an attacker that does not enter yet circles around
        let around = attacks && !enters;
        let attacks = (attacks && !around) || threatened;
        let (r0, r1) = if around { (radio + 5.0, 30.0) } else if attacks { (0.0, radio * 0.6) } else if rng.si(0.4) { (0.0, radio) } else { (radio * 0.5, 35.0) };
        let mut best = [centre[0], centre[2]];
        for k in 0..8 {
            let (a, r) = (rng.uniform(0.0, std::f32::consts::TAU), rng.uniform(r0, r1));
            let p = [centre[0] + r * a.cos(), centre[2] + r * a.sin()];
            best = p;
            // keeps going more or less where it was heading: going back and forth is not advancing
            let ahead = (p[0] - pos[0]) * front[0] + (p[1] - pos[2]) * front[2] > 0.0;
            if k == 7 || (ahead || attacks) && flat(pos, p) > if attacks { 3.0 } else { 25.0 } {
                break;
            }
        }
        best
    }

    /// Where to go now (a point of the path), or None if it stays stopped. `centre` and `radio` are the zone; `attacks`, `enters` and
    /// `threatened`, what the bot has decided about it.
    #[allow(clippy::too_many_arguments)]
    pub fn next_step(&mut self, v: &BotView, rng: &mut Rng, maps: &Maps, attacks: bool, centre: [f32; 3], radio: f32, enters: bool, threatened: bool) -> Option<[f32; 2]> {
        let pos = v.me.pos;
        if let Some((at, next)) = self.jammed.take()
            && let Some(g) = maps.foot(v)
        {
            g.blocked(at, next);
        }
        // (running for the way out it saw: it thinks again when it gets there)
        if v.time < self.stopped_until || (self.seeks_way_out && v.time < self.flight_until) {
            return None;
        }
        let arrived = self.point.is_some_and(|p| flat(pos, p) < 3.0) || (self.point.is_some() && self.path.is_empty());
        if self.point.is_none() || arrived {
            self.path.clear();
            if arrived {
                // stop here for a while?
                let near = flat(pos, [centre[0], centre[2]]) < 35.0;
                if near && rng.si(0.2) {
                    let long = rng.si(0.02);
                    self.stopped_until = v.time + if long { rng.uniform(30.0, 45.0) } else { rng.lognormal(5.0, 0.6) } as f64;
                    self.point = None;
                    return None;
                }
            }
            // a point that can be reached: there are some in closed courtyards
            let eye = v.me.eye;
            let vetoes = self.vetoes.clone();
            // (bots: and it walks there; it is asked only of the cell a path starts from when the bot stands in a blocked one, and
            // from inside DC_No_Fly_Zone's control tower every cell round it was in sight through the walls)
            let walks = self.seeks_way_out;
            let se_ve = |c: [f32; 2]| !vetoes.iter().any(|w| flat([w[0], 0.0, w[1]], c) < 6.0) && (!v.hidden(eye, [c[0], eye[1], c[1]]) || v.ground(c[0], c[1]).is_some_and(|h| !v.hidden(eye, [c[0], h + 1.4, c[1]]))) && no_fall(v, pos, c) && (!walks || walkable(v, pos, c));
            let mut p = [centre[0], centre[2]];
            // TODO: bots shut in with no way to their goal ask five points each and fail them all, again every few seconds, and
            // those that spawned together do it in the same tick (GC_Judicator's hangar at 1611,883). The mesh remembers within
            // the tick a way out that failed, so the other four cost nothing (`examples/step_spikes data/game DC_Final
            // DC_LostVillage conquest 64 60 12`: its 20 ms step every 3 s is gone; that pocket, the upper floor of the base house
            // at 1226,940, was a ramp of 45° the mesh left out, and is joined now). Stopping the retries, or spacing them out,
            // changes the rounds: measure with the whole stuck sweep.
            for _ in 0..5 {
                // (if it was waiting for a path, it carries on with that one)
                p = match self.requested_point.take() {
                    Some(p) => p,
                    None => self.new_point(rng, enters, threatened, centre, radio, attacks, pos, v.me.view[2]),
                };
                // TODO: DC_LostVillage's attackers reach the village and take no flag: near, every point picked lies in a house. Moving it
                // aside on every pick took a flag there (one seed) but cost GC_Endor +23 → −20 and Berlin +34 → +10 against the plain bot; on the last
                // pick only, nothing. How: rounds of 8 min in DC_LostVillage, seeds 1..3, and
                // `BYTEFIELD_BOTS_ME=<leader>` near flag 1 (1022, 1003): what the walker picks and why it gets no path.
                let aside = self.seeks_way_out && flat(pos, p) > super::paths::ASIDE_FROM_M;
                match maps.request_foot_from(v, p, &se_ve, &self.avoid, aside) {
                    Answer::Path(c) => {
                        self.ways_tried.clear();
                        // (it is out: the others learn the door)
                        if let Some(from) = self.ran_from.take()
                            && flat(pos, from) > 6.0
                        {
                            maps.door_found(from, [pos[0], pos[2]]);
                        }
                        self.path = c;
                        break;
                    }
                    Answer::Wait => {
                        self.requested_point = Some(p);
                        return None;
                    }
                    Answer::NoPath => self.path.clear(),
                }
            }
            // no path because the map closes the place it stands in: through the opening it sees, running
            if self.path.is_empty()
                && self.seeks_way_out
                && let Some(d) = self.way_out(v, rng, maps, p)
            {
                self.ways_tried.push(d.0);
                if self.ways_tried.len() > 12 {
                    self.ways_tried.clear();
                }
                self.ran_from = d.2.then_some([pos[0], pos[2]]);
                self.flight = d.0;
                self.flight_until = v.time + d.1 as f64;
                self.lookout_t = v.time;
                self.point = None;
                return None;
            }
            // no path because the map does not join its zone to the goal's (a river, a ditch): to the edge of its own on the straight
            // line to the goal, and from there straight across to the other
            // (only where the crossing can be made from that edge: not a drop onto dry ground below)
            if self.path.is_empty()
                && self.seeks_way_out
                && let Some((edge, across)) = crossing(v, maps, p)
                && crossable(v, [edge[0], pos[1], edge[1]], across)
            {
                if flat(pos, edge) > 4.0 {
                    match maps.request_foot_from(v, edge, &|_| true, &self.avoid, self.seeks_way_out) {
                        Answer::Path(c) => {
                            self.path = c;
                            self.point = Some(edge);
                            self.path.push(unknown_height(edge));
                            return self.path.first().copied().map(xz);
                        }
                        Answer::Wait => return None,
                        Answer::NoPath => {}
                    }
                } else if crossable(v, pos, across) {
                    self.point = Some(across);
                    self.path.push(unknown_height(across));
                    return Some(across);
                }
            }
            // no path, in a straight line; but never off a cliff: waits and thinks again
            if self.path.is_empty() && !no_fall(v, pos, p) {
                self.stopped_until = v.time + 3.0;
                self.point = None;
                return None;
            }
            self.point = Some(p);
            self.path.push(unknown_height(p));
        }
        // a point within 2 m is passed if the next one is in sight over the map, or once it is reached: passing every point within
        // 2 m cut the corner of a door a metre wide (bf1942 Market_Garden's bunker at the iron bridge: the bots went for the next point
        // round the jamb and pushed against it)
        // (on the mesh, only level with it: the top of a ladder is not passed from its foot)
        let grid = maps.foot(v);
        while self.path.len() > 1 && flat(pos, xz(self.path[0])) < 2.0 && level(pos, self.path[0]) && (flat(pos, xz(self.path[0])) < PASSED_M || grid.is_none_or(|g| g.sight_from(feet(pos), xz(self.path[1])))) {
            self.path.remove(0);
        }
        // the path map is flat: a path off a platform's edge looks as good as any; the next metres are checked as it walks
        if let Some(p) = self.path.first().copied().map(xz)
            && !with_ground(v, pos, p)
        {
            self.vetoes.push(p);
            self.edge_t = v.time;
            self.stopped_until = v.time + 3.0;
            self.point = None;
            self.path.clear();
            return None;
        }
        self.path.first().copied().map(xz)
    }

    /// The direction of a way out of a place the path map leaves closed (it stands in a zone with no connection to `goal`'s): of 32
    /// directions, the one nearest the goal's bearing in which it sees, at eye height and with floor all the way, a point of another
    /// zone. Seeing none, the direction it sees farthest in; failing that, a few free metres in any direction: another place to look from. `None` where the map does not shut it
    /// in (the goal is in its own zone, its zone is not a small one, or there is no map).
    /// With the direction, the seconds to run in it and whether that run is a try at the door (one that got out is told to the others).
    fn way_out(&self, v: &BotView, rng: &mut Rng, maps: &Maps, goal: [f32; 2]) -> Option<([f32; 2], f32, bool)> {
        let g = maps.foot(v)?;
        let (pos, eye) = (v.me.pos, v.me.eye);
        let mine = g.zone_at_feet(feet(pos))?;
        // it is the one shut in: its zone is a small one, or it stands in a cell the map gives as taken (the inside of that depot).
        // A goal inside a closed courtyard, seen from the open, is not a place to break out of.
        if mine != 0 && (g.zone_at(goal[0], goal[1]) == Some(mine) || g.zone_size(mine) > POCKET_CELLS) {
            return None;
        }
        let outside = |z: Option<u32>| z.is_some_and(|z| z != 0 && z != mine);
        let bearing = (goal[1] - pos[2]).atan2(goal[0] - pos[0]);
        // (free at eye height and at knee height: a wall of sandbags is seen over and not walked through)
        let knee = pos[1] - 0.3;
        let free = |d: [f32; 2], m: f32| {
            let b = [pos[0] + m * d[0], pos[2] + m * d[1]];
            !v.hidden(eye, [b[0], eye[1], b[1]]) && !v.hidden([pos[0], knee, pos[2]], [b[0], knee, b[1]]) && no_fall(v, pos, b) && walkable(v, pos, b)
        };
        // (beside a wall in the open it also stands in a taken cell, and the street is right there: with open ground of a large zone
        // in sight within a few steps it is not shut in)
        if mine == 0 {
            let open_near = (-3..=3).flat_map(|dz| (-3..=3).map(move |dx| [dx as f32 * 2.0, dz as f32 * 2.0])).any(|o| {
                let l = o[0].hypot(o[1]);
                l >= 2.0 && g.zone_at(pos[0] + o[0], pos[2] + o[1]).is_some_and(|z| z != 0 && g.zone_size(z) > POCKET_CELLS) && free([o[0] / l, o[1] / l], l)
            });
            if open_near {
                return None;
            }
        }
        let tried = |d: [f32; 2]| self.ways_tried.iter().any(|t| t[0] * d[0] + t[1] * d[1] > 0.97);
        // the door somebody found before: to where they ran from, and from there the way they ran
        if let Some((inside, out)) = maps.door_near([pos[0], pos[2]], 25.0) {
            let to = |b: [f32; 2]| {
                let l = flat(pos, b).max(0.1);
                ([(b[0] - pos[0]) / l, (b[1] - pos[2]) / l], l)
            };
            let (d, l) = to(inside);
            if l > 1.5 && !tried(d) && free(d, l) {
                return Some((d, (l / 4.0).clamp(0.3, 3.0), false));
            }
            let (d, _) = to(out);
            if l <= 1.5 && !tried(d) {
                return Some((d, 3.0, true));
            }
        }
        // (to one side and the other of the goal's bearing, opening up)
        let dirs = (0..32).map(|k| bearing + ((k + 1) / 2) as f32 * if k % 2 == 0 { 1.0 } else { -1.0 } * std::f32::consts::TAU / 32.0).map(|a| [a.cos(), a.sin()]);
        // (where nothing of another zone is in sight, the direction it sees farthest in: a door looks like that from inside)
        let mut far: Option<([f32; 2], f32)> = None;
        for d in dirs {
            if tried(d) {
                continue;
            }
            let mut m = 4.0;
            while m <= WAY_OUT_M {
                if outside(g.zone_at(pos[0] + m * d[0], pos[2] + m * d[1])) {
                    if free(d, m) {
                        return Some((d, 3.0, true));
                    }
                    break;
                }
                m += 2.0;
            }
            if let Some(m) = [WAY_OUT_M, 10.0].into_iter().find(|&m| free(d, m))
                && far.is_none_or(|f| m > f.1)
            {
                far = Some((d, m));
            }
        }
        if let Some((d, _)) = far {
            return Some((d, 3.0, true));
        }
        let de = rng.uniform(0.0, std::f32::consts::TAU);
        (0..8).map(|k| de + k as f32 * std::f32::consts::FRAC_PI_4).map(|a| [a.cos(), a.sin()]).find(|&d| !tried(d) && free(d, 4.0)).map(|d| (d, 1.5, false))
    }

    /// How long it has gone without getting closer to `step`: moving sideways along a wall is not advancing. Up or down to a
    /// point of known height counts (a ladder is climbed on the spot).
    pub fn jam(&mut self, v: &BotView, step: Option<[f32; 2]>) -> f64 {
        // (on a ladder it is getting somewhere, if slowly: the climb starts with a pause; jumping, it lets go)
        let step = step.filter(|_| !(self.on_ladder(v)));
        let Some(p) = step else {
            self.anchor_t = v.time;
            self.best = f32::MAX;
            return 0.0;
        };
        let mut d = flat(v.me.pos, p);
        if let Some(q) = self.path.first()
            && xz(*q) == p
            && !q[1].is_nan()
        {
            d = d.hypot(feet(v.me.pos)[1] - q[1]);
        }
        if p != self.anchor || d < self.best - 1.0 {
            self.anchor = p;
            self.best = d;
            self.anchor_t = v.time;
        }
        v.time - self.anchor_t
    }

    /// Whether it is on a ladder that its path goes up or down (the grid's paths know none: never).
    fn on_ladder(&self, v: &BotView) -> bool {
        self.path.first().is_some_and(|p| !p[1].is_nan()) && v.climbing()
    }

    /// How long a vehicle has gone without moving from the spot (3 m).
    pub fn vehicle_jam(&mut self, v: &BotView, pos: [f32; 3]) -> f64 {
        if (pos[0] - self.anchor[0]).powi(2) + (pos[2] - self.anchor[1]).powi(2) > 9.0 {
            self.anchor = [pos[0], pos[2]];
            self.anchor_t = v.time;
        }
        v.time - self.anchor_t
    }

    /// If for 10 s it has wanted to walk less than 4 m (a balcony, a corner the map does not know): three seconds in a direction
    /// that looks free, jumping. Returns the axes while it lasts.
    pub fn flee(&mut self, v: &BotView, rng: &mut Rng, wants: bool) -> Option<Vec<(usize, f32)>> {
        let me = &v.me;
        let p = [me.pos[0], me.pos[2]];
        if v.time < self.flight_until {
            let g = angle_between(me.view[2], self.flight);
            return Some(vec![(axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)), (axis::MOUSE_Y, 0.0), (axis::THROTTLE, if g.abs() < 60.0 { 1.0 } else { 0.0 }), (axis::ACTION, 1.0)]);
        }
        if !wants || (p[0] - self.lookout[0]).powi(2) + (p[1] - self.lookout[1]).powi(2) > 16.0 || self.on_ladder(v) {
            self.lookout = p;
            self.lookout_t = v.time;
            return None;
        }
        if v.time - self.lookout_t < 10.0 {
            return None;
        }
        self.run_free(v, rng, 3.0);
        None
    }

    /// Runs for `seconds` in a direction that looks free (of eight, starting from a random one, the first), dropping its path.
    /// Without a safe direction, no running blind.
    fn run_free(&mut self, v: &BotView, rng: &mut Rng, seconds: f64) -> bool {
        let me = &v.me;
        let de = rng.uniform(0.0, std::f32::consts::TAU);
        let dirs: Vec<[f32; 2]> = (0..8).map(|k| de + k as f32 * std::f32::consts::FRAC_PI_4).map(|a| [a.cos(), a.sin()]).collect();
        // (bots: and walks there, the walls of the building it is in counting from inside)
        let free = dirs.iter().find(|d| !v.hidden(me.eye, [me.eye[0] + 8.0 * d[0], me.eye[1], me.eye[2] + 8.0 * d[1]]) && no_fall(v, me.pos, [me.pos[0] + 16.0 * d[0], me.pos[2] + 16.0 * d[1]]) && (!self.seeks_way_out || walkable(v, me.pos, [me.pos[0] + 8.0 * d[0], me.pos[2] + 8.0 * d[1]])));
        self.lookout_t = v.time;
        self.point = None;
        self.path.clear();
        let Some(free) = free else { return false };
        self.flight = *free;
        self.flight_until = v.time + seconds;
        true
    }

    /// Its side sees that it is not getting anywhere on foot (wandering a room-sized patch at full throttle is not the 4 m of
    /// [`Walker::flee`]: in Kursk two leaders appear inside a barn the path map gives as open ground, and spent a minute pushing
    /// against its east wall): it runs out the way it sees farthest in, as from a place the map shuts ([`Walker::way_out`]: a door
    /// looks like that from inside, and the one that gets out tells the others); seeing none, in any direction that looks free.
    pub fn break_out(&mut self, v: &BotView, rng: &mut Rng) -> bool {
        if v.time < self.flight_until {
            return false;
        }
        let pos = v.me.pos;
        let bearing = self.point.map_or_else(|| rng.uniform(0.0, std::f32::consts::TAU), |p| (p[1] - pos[2]).atan2(p[0] - pos[0]));
        let Some(d) = self.sees_farthest(v, bearing) else { return self.run_free(v, rng, BREAK_OUT_S) };
        self.ways_tried.push(d);
        if self.ways_tried.len() > 12 {
            self.ways_tried.clear();
        }
        self.ran_from = Some([pos[0], pos[2]]);
        self.flight = d;
        self.flight_until = v.time + BREAK_OUT_S;
        self.lookout_t = v.time;
        self.point = None;
        self.path.clear();
        true
    }

    /// Of 32 directions, opening up to one side and the other of `bearing`, the one it sees farthest in (free at eye height and at
    /// knee height, with floor all the way, for [`WAY_OUT_M`] or at least 10 m) among those it has not tried.
    fn sees_farthest(&self, v: &BotView, bearing: f32) -> Option<[f32; 2]> {
        let (pos, eye) = (v.me.pos, v.me.eye);
        let knee = pos[1] - 0.3;
        let free = |d: [f32; 2], m: f32| {
            let b = [pos[0] + m * d[0], pos[2] + m * d[1]];
            !v.hidden(eye, [b[0], eye[1], b[1]]) && !v.hidden([pos[0], knee, pos[2]], [b[0], knee, b[1]]) && no_fall(v, pos, b) && walkable(v, pos, b)
        };
        let mut far: Option<([f32; 2], f32)> = None;
        for d in (0..32).map(|k| bearing + ((k + 1) / 2) as f32 * if k % 2 == 0 { 1.0 } else { -1.0 } * std::f32::consts::TAU / 32.0).map(|a| [a.cos(), a.sin()]) {
            if self.ways_tried.iter().any(|t| t[0] * d[0] + t[1] * d[1] > 0.97) {
                continue;
            }
            if let Some(m) = [WAY_OUT_M, 10.0].into_iter().find(|&m| free(d, m))
                && far.is_none_or(|f| m > f.1)
            {
                far = Some((d, m));
            }
        }
        far.map(|f| f.0)
    }

    /// The end of each tick on foot: if it does not advance towards `step`, it jumps and steps aside; if it goes on like that, it drops that point or flees. `wants`:
    /// wants to walk now (has not stopped to shoot or is not clearing the flag). Returns the flight's axes, which replace all others.
    pub fn unjam(&mut self, v: &BotView, rng: &mut Rng, player: usize, step: Option<[f32; 2]>, wants: bool, no_target: bool, axes: &mut Vec<(usize, f32)>) -> Option<Vec<(usize, f32)>> {
        let t = self.jam(v, step.filter(|_| wants));
        if let Some(h) = self.flee(v, rng, wants) {
            return Some(h);
        }
        // bots: a vehicle standing in the way (the path map and the view know only the level's objects): along its hull on the side of
        // the next point, keeping the path. GC_Ryloth: two T-3Bs parked at the forward base's flag, and the squads they brought pushed
        // against them, jumping, for half a minute and more each
        if t > 1.5
            && self.seeks_way_out
            && let Some(p) = step
            && let Some(d) = round_vehicle(v, p)
        {
            self.flight = d;
            self.flight_until = v.time + ROUND_VEHICLE_S;
            self.anchor_t = v.time;
            return None;
        }
        if t > 1.5 {
            axes.push((axis::ACTION, 1.0));
            if no_target {
                let side_ = if (player + (v.time / 3.0) as usize) % 2 == 0 { 1.0 } else { -1.0 };
                axes.push((axis::YAW, side_));
            }
        }
        // bots: a wall at the knee between it and the next point (the path map's 2 m cells leave out a 1 m wall the level's own map
        // has: Battle_of_the_Bulge's supply shed, where a bot spawned by its east wall and pushed at it, the path asked again the same,
        // for the round): out the way it sees farthest, as from a place the map shuts (not in a fight: running about then, it fired at
        // walls more, Berlin `does_not_fire_at_the_wall_that_hides_the_target`)
        if t > 5.0 && no_target && self.seeks_way_out && step.is_some_and(|p| !walkable(v, v.me.pos, p)) && self.break_out(v, rng) {
            return None;
        }
        if t > 5.0 {
            if let (Some(_), Some(&next)) = (step, self.path.first()) {
                self.jammed = Some((feet(v.me.pos), next));
            }
            if let Some(p) = step {
                self.vetoes.push(p);
                if self.vetoes.len() > 16 {
                    self.vetoes.remove(0);
                }
            }
            self.point = None;
            self.path.clear();
            self.anchor_t = v.time;
        }
        None
    }

    /// The vertical look it wants ([`axis::MOUSE_Y`]) while its next point is close across and above or below it: up a
    /// ladder it looks up, down one, down. `None` otherwise (it looks level), and always where the path has no heights.
    pub fn look(&self, v: &BotView) -> Option<f32> {
        let p = *self.path.first()?;
        if p[1].is_nan() || level(v.me.pos, p) || (flat(v.me.pos, xz(p)) > LADDER_ACROSS_M && !self.on_ladder(v)) {
            return None;
        }
        let want = if p[1] > feet(v.me.pos)[1] { CLIMB_PITCH } else { -CLIMB_PITCH };
        Some(((pitch(v.me.view[2]) - want) / 2.5).clamp(-4.0, 4.0))
    }

    /// On a ladder its path goes up or down, after [`Walker::walk`]: it pushes on whichever way it faces (on a ladder it
    /// cannot turn, and the way it looks says up or down: [`Walker::look`]), and going down it lets go once the floor it
    /// heads for is no more than a harmless drop below.
    pub fn climb(&self, v: &BotView, axes: &mut Vec<(usize, f32)>) {
        if !self.on_ladder(v) {
            return;
        }
        let p = self.path[0];
        axes.retain(|a| a.0 != axis::THROTTLE && a.0 != axis::MOUSE_X);
        axes.push((axis::MOUSE_X, 0.0));
        if feet(v.me.pos)[1] - p[1] < crate::navmesh::SOLDIER_DROP && p[1] < feet(v.me.pos)[1] + LEVEL_M {
            axes.push((axis::ACTION, 1.0));
        } else {
            axes.push((axis::THROTTLE, 1.0));
        }
    }

    /// Its next point is where a jump from about here lands (on the mesh): after [`Walker::walk`], it jumps, running.
    pub fn leap(&self, v: &BotView, maps: &Maps, axes: &mut Vec<(usize, f32)>) {
        let Some(&p) = self.path.first() else { return };
        if !axes.iter().any(|a| a.0 == axis::THROTTLE && a.1 > 0.0) || !maps.foot(v).is_some_and(|g| g.leap(feet(v.me.pos), p)) {
            return;
        }
        axes.push((axis::ACTION, 1.0));
    }

    /// The axes for heading towards `d0` (flat relative direction) on foot, without combat: turn that way and advance if it is more or less ahead.
    pub fn walk(&self, f: [f32; 3], dir: Option<[f32; 2]>, axes: &mut Vec<(usize, f32)>) {
        match dir {
            Some(d0) => {
                let g = angle_between(f, d0);
                axes.push((axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)));
                axes.push((axis::THROTTLE, if g.abs() < 60.0 { 1.0 } else { 0.0 }));
            }
            None => axes.push((axis::MOUSE_X, 0.0)),
        }
    }
}
