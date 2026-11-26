//! The variants tried and parked (`bots:X`, all off by default and identical when off): regroup, hunt, flank2, cover-flank, mines on
//! traffic and stealing what the enemy drove; each keeps its why in a TODO beside it.
use super::*;

/// `bots:cover-flank`: a mate of the squad flanks from `from` to `to`; the enemy was last seen at `enemy` near the flag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoverFlank {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub enemy: Option<[f32; 3]>,
}

/// `bots:cover-flank`: what a bot said counts this long, s.
pub const COVER_FRESH_S: f64 = 1.5;

/// `bots:steal-flank`: a vehicle the side saw an enemy in counts as the enemy's this long, s.
pub const ENEMY_DRIVEN_S: f64 = 300.0;

/// `bots:regroup`: an enemy flag's assembly point: where, since when, when the first of the side got there, and when it let them go;
/// with `bots:regroup-way`, each squad's point (by leader; `at` is the first).
#[derive(Clone, Debug)]
pub struct Rally {
    pub at: [f32; 3],
    pub made: f64,
    pub first: Option<f64>,
    pub released: Option<f64>,
    pub points: BTreeMap<usize, [f32; 3]>,
}

// TODO: off: it raised the tickets (+12 ± 7 over 128 rounds, +82 on DC_Al_Nas) but not B36 (attackers at least the defenders
// 20 s after a squad arrives: +9 LostVillage, +7 Berlin, +0.5 El_Alamein, +2 Kursk; +10 asked) and the squads stood more on
// El_Alamein and Kursk (still or stuck on foot +1.4..+1.5 points). Few get to the point: the squads change flags at the split and
// their people walk spread; a point per squad's way (its path, not the flag's bearing) and holding the members, not the leader, is
// `bots:regroup-way`. See `BYTEFIELD_BOTS_TRACE=1 tournament3 ... --a bots:regroup` (`regroup:` lines).
// TODO: the gain above did not come from waiting: a squad held at the point gets [`Goal::Point`], and `squad::orders` gives no orders to
// a squad without a flag, so its people kept their last orders (to the flag) and nobody was held. What changed was the side effects
// (no trip while held, the flag the split remembers). Kept as it was measured, the basket's `regroup.toml`; `bots:regroup-way` (R1)
// holds them for real.
/// `bots:regroup`: the numbers are named parameters ([`crate::bots::params::BotParams`], `rally_*`): the point's distance short of the
/// flag (tried along the way the squads come, then turned by these angles if that one cannot be walked to), when a person is there,
/// how long the squads wait, which squads are held and when the point is made.
pub const RALLY_TURNS: [f32; 5] = [0.0, 0.5, -0.5, 1.0, -1.0];

/// `bots:streetwise`: mines go in the cell of most enemy vehicle traffic within this of a flag of the side, m, if it has at least this
/// much.
// TODO: off: the mines go where the enemy's vehicles pass (50 % on El_Alamein, 73 % on Kursk, against 20 and 39), but they do not
// destroy twice as many (0.6 a round on Kursk, as many as before): wants more rounds, or the mines spread along the traffic.
pub const TRAFFIC_MINES_ON: bool = false;
// (400 m and 0.3: 73 % of the mines on traffic on Kursk, 14 % on El_Alamein, which is wide)
pub const MINES_NEAR_FLAG_M: f32 = 600.0;
pub const MINES_MIN_TRAFFIC: f32 = 0.15;

/// `bots:hunt`: the point is at most this far from the side (the middle of its people), m; it is kept unless the new one is this far
/// from it, m; only cells with this much traffic count, and the best few are asked whether they are reached by land.
pub const HUNT_MAX_M: f32 = 1000.0;
pub const HUNT_KEEP_M: f32 = 150.0;
pub const HUNT_MIN_TRAFFIC: f32 = 0.05;
pub const HUNT_ASK: usize = 5;

/// `bots:flank2`: what the flanker is told: the flag, where the enemy is (the centre of what the side saw near the flag lately, or
/// the flag), and the sectors round the flag the side has arrived by lately.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flank {
    pub flag: usize,
    pub enemy: [f32; 3],
    pub used: [bool; FLANK_SECTORS],
}

/// `bots:flank2`: a squad has contact with the enemy seen within this of its flag in this long, m and s; the flanker keeps the role
/// this long at most, s; an arrival is coming within this of a flag, m, and counts for this long, s, in one of these sectors.
pub const CONTACT_M: f32 = 80.0;
pub const CONTACT_S: f64 = 10.0;
pub const FLANKER_S: f64 = 60.0;
pub const ARRIVAL_M: f32 = 30.0;
pub const ARRIVALS_S: f64 = 180.0;
pub const FLANK_SECTORS: usize = 8;

/// `bots:regroup-way`: the flag a squad (by leader) held at a point at the last split keeps, unless its point was let go, the flag is the
/// side's or cannot be taken.
pub(super) fn held_flag(held: &BTreeMap<usize, usize>, rallies: &BTreeMap<usize, Rally>, leader: usize, view: &TeamView) -> Option<usize> {
    let &i = held.get(&leader)?;
    let f = view.flags.get(i)?;
    (f.owner != view.team && f.takeable && rallies.get(&i).is_some_and(|r| r.released.is_none())).then_some(i)
}

/// `bots:regroup-way`: the assembly point for a squad at `from` coming to the flag at `flag`: on its way on foot, `back_m` short of the flag;
/// where the map has no way, along the bearing it comes from, turned by [`RALLY_TURNS`] if that one cannot be walked to; never in water.
fn rally_point(from: [f32; 3], flag: [f32; 3], back_m: f32, land: &dyn trip::Land) -> Option<[f32; 3]> {
    let water = land.water();
    let on = |x: f32, z: f32| {
        let y = land.below(x, z);
        (y > water || y == f32::MIN).then_some([x, if y > f32::MIN { y } else { flag[1] }, z])
    };
    if let Some(p) = land.way_point(from, flag, 6.0, back_m).and_then(|p| on(p[0], p[1])) {
        return Some(p);
    }
    let a0 = (from[2] - flag[2]).atan2(from[0] - flag[0]);
    RALLY_TURNS.iter().find_map(|&da| {
        let p = on(flag[0] + back_m * (a0 + da).cos(), flag[2] + back_m * (a0 + da).sin())?;
        (land.reach(from, p, 6.0) != trip::Reach::NoPath).then_some(p)
    })
}

/// The sector of `p` round `centre`, of [`FLANK_SECTORS`].
pub fn sector(centre: [f32; 3], p: [f32; 3]) -> usize {
    let a = (p[2] - centre[2]).atan2(p[0] - centre[0]).rem_euclid(std::f32::consts::TAU);
    ((a / std::f32::consts::TAU * FLANK_SECTORS as f32) as usize).min(FLANK_SECTORS - 1)
}

/// B23: an attack run's target is looked for this far from the aircraft, m; one on foot only inside this distance of a flag of the

impl Team {
    /// `bots:streetwise`: where the enemy's vehicles pass near the side's flags: the cell of the traffic map with most vehicle traffic
    /// within [`MINES_NEAR_FLAG_M`] of a flag of the side, and the way they go through it (towards the hottest of its eight
    /// neighbours, or towards the flag if none is warm): (centre, direction along the traffic).
    pub fn mine_spot(&self, view: &TeamView) -> Option<([f32; 2], [f32; 2])> {
        let own: Vec<[f32; 3]> = view.flags.iter().filter(|f| f.owner == view.team).map(|f| f.pos).collect();
        let near = |p: [f32; 3]| own.iter().map(|f| flat(*f, p)).fold(f32::MAX, f32::min);
        let best = self
            .intel
            .traffic_cells()
            .filter(|c| c.2 >= MINES_MIN_TRAFFIC && near(c.0) < MINES_NEAR_FLAG_M)
            .max_by(|a, b| a.2.total_cmp(&b.2).then(b.0[0].total_cmp(&a.0[0])).then(b.0[2].total_cmp(&a.0[2])))?;
        let c = best.0;
        let k = crate::bots::intel::CELL_M;
        let mut hot = (0.0f32, [0.0f32; 2]);
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let p = [c[0] + dx as f32 * k, 0.0, c[2] + dz as f32 * k];
                let w = self.intel.traffic(p, true);
                if w > hot.0 {
                    hot = (w, [dx as f32, dz as f32]);
                }
            }
        }
        let d = if hot.0 > MINES_MIN_TRAFFIC * 0.5 {
            hot.1
        } else {
            let f = own.iter().min_by(|a, b| flat(**a, c).total_cmp(&flat(**b, c)))?;
            [f[0] - c[0], f[2] - c[2]]
        };
        let l = d[0].hypot(d[1]).max(1e-3);
        Some(([c[0], c[2]], [d[0] / l, d[1] / l]))
    }

    /// `bots:hunt`: where to look for the enemy when no flag changes hands: the cell of the side's traffic map with most enemy traffic
    /// times (own people near + 0.2), the front where both touch, within [`HUNT_MAX_M`] of the side and reached by land; with no
    /// traffic, none. The point held is kept unless the new one is [`HUNT_KEEP_M`] away.
    pub(super) fn hunt_point(&self, view: &TeamView, land: &dyn trip::Land) -> Option<[f32; 3]> {
        let n = view.mates.len().max(1) as f32;
        let side = [view.mates.iter().map(|c| c.pos[0]).sum::<f32>() / n, view.mates.iter().map(|c| c.pos[1]).sum::<f32>() / n, view.mates.iter().map(|c| c.pos[2]).sum::<f32>() / n];
        if view.mates.is_empty() {
            return self.hunt;
        }
        let own_near = |p: [f32; 3]| (view.mates.iter().filter(|c| flat(c.pos, p) < crate::bots::intel::PRESENCE_M).count() as f32 / 4.0).min(1.0);
        let mut cells: Vec<(f32, [f32; 3])> = self
            .intel
            .traffic_cells()
            .filter(|c| c.1 + c.2 > HUNT_MIN_TRAFFIC && flat(c.0, side) < HUNT_MAX_M)
            .map(|c| ((c.1 + c.2) * (own_near(c.0) + 0.2), [c.0[0], land.below(c.0[0], c.0[2]), c.0[2]]))
            .collect();
        cells.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1[0].total_cmp(&b.1[0])).then(a.1[2].total_cmp(&b.1[2])));
        let reached = |p: [f32; 3]| land.reach(side, p, 30.0) != trip::Reach::NoPath;
        // (with no traffic, `None`: to the enemy's bases as without the lever. Half way between the bases, they all stood there: 40 % of
        // the time still on foot on DC_No_Fly_Zone_Day2, and a quarter of the kills)
        let best = cells.into_iter().take(HUNT_ASK).map(|c| c.1).find(|&p| reached(p))?;
        match self.hunt {
            Some(h) if flat(h, best) < HUNT_KEEP_M => Some(h),
            _ => Some(best),
        }
    }

    /// `bots:regroup`: the squads on foot sent to an enemy flag wait at a point `rally_m` short of it, on the way they come and
    /// reached by land, until the people there cover the flag's need (by strength with `bots:strength`, else all of those squads) or
    /// `rally_wait_s` after the first got there; then they go in together. A squad already near the flag, in a vehicle or on a trip
    /// that is not a plain walk, is not held.
    pub(super) fn regroup(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        let pp = self.params;
        let fl = &view.flags;
        let mate = |j: usize| view.mates.iter().find(|c| c.player == j);
        self.held.clear();
        self.rallies.retain(|&i, r| fl.get(i).is_some_and(|f| f.owner != view.team) && r.released.is_none_or(|x| t - x < pp.rally_released_s));
        for (i, f) in fl.iter().enumerate() {
            if f.owner == view.team || !f.takeable {
                continue;
            }
            let going: Vec<usize> = (0..self.squads.len()).filter(|&k| self.squads[k].goal == Goal::Flag(i)).collect();
            if going.is_empty() {
                continue;
            }
            // the squads that can be held: the leader on foot, walking, and not yet in
            let holdable = |e: &Squad, at: [f32; 3]| {
                mate(e.leader).is_some_and(|c| c.vehicle.is_none() && flat(c.pos, f.pos) > pp.rally_inside * flat(at, f.pos)) && e.trip.as_ref().is_none_or(|tr| tr.plain()) && !self.urgent_ones.contains(&e.leader)
            };
            if !self.rallies.contains_key(&i) {
                let leaders: Vec<[f32; 3]> = going.iter().filter_map(|&k| mate(self.squads[k].leader)).filter(|c| c.vehicle.is_none() && (pp.rally_m + pp.rally_near_m..pp.rally_start_m).contains(&flat(c.pos, f.pos))).map(|c| c.pos).collect();
                if leaders.is_empty() {
                    continue;
                }
                let n = leaders.len() as f32;
                let mid = [leaders.iter().map(|p| p[0]).sum::<f32>() / n, 0.0, leaders.iter().map(|p| p[2]).sum::<f32>() / n];
                let a0 = (mid[2] - f.pos[2]).atan2(mid[0] - f.pos[0]);
                let water = land.water();
                let from = leaders[0];
                let at = RALLY_TURNS.iter().find_map(|&da| {
                    let (x, z) = (f.pos[0] + pp.rally_m * (a0 + da).cos(), f.pos[2] + pp.rally_m * (a0 + da).sin());
                    let y = land.below(x, z);
                    let p = [x, if y > f32::MIN { y } else { f.pos[1] }, z];
                    (y > water && land.reach(from, p, 6.0) != trip::Reach::NoPath).then_some(p)
                });
                let Some(at) = at else { continue };
                self.rallies.insert(i, Rally { at, made: t, first: None, released: None, points: BTreeMap::new() });
            }
            let r = self.rallies.get(&i).unwrap().clone();
            if r.released.is_some() {
                continue;
            }
            // (those that come on foot: a squad in a vehicle goes straight in)
            let people: Vec<usize> = going.iter().filter(|&&k| mate(self.squads[k].leader).is_some_and(|c| c.vehicle.is_none())).flat_map(|&k| self.squads[k].alive(view)).collect();
            let there = people.iter().filter(|&&j| mate(j).is_some_and(|c| c.vehicle.is_none() && flat(c.pos, r.at) < pp.rally_near_m)).count();
            let need = if self.parts.strength { ((NEED_PER_ENEMY * self.intel.presence(i)).ceil() as usize + 1).clamp(NEED_MIN, NEED_MAX) } else { people.len() }.min(people.len());
            let first = r.first.or((there > 0).then_some(t));
            let go = there >= need || first.is_some_and(|x| t - x >= pp.rally_wait_s) || (first.is_none() && t - r.made >= pp.rally_come_s);
            let rr = self.rallies.get_mut(&i).unwrap();
            rr.first = first;
            if go {
                rr.released = Some(t);
                if crate::bots::trace_team() {
                    eprintln!("t={t:.0} team {} regroup: flag {i} goes in with {there} of {need} (needed) at {:?}", view.team, r.at.map(|x| x as i32));
                }
                continue;
            }
            for &k in &going {
                if holdable(&self.squads[k], r.at) {
                    let e = &mut self.squads[k];
                    self.held.insert(e.leader, i);
                    e.goal = Goal::Point([r.at[0], r.at[2]]);
                }
            }
            if crate::bots::trace_team() {
                eprintln!("t={t:.0} team {} regroup: flag {i} at {:?}: {there} of {need} there, holds {:?}", view.team, r.at.map(|x| x as i32), self.held.iter().filter(|h| *h.1 == i).map(|h| *h.0).collect::<Vec<_>>());
            }
        }
    }

    /// `bots:regroup-way` (R1): as [`Team::regroup`], with three changes. Each squad's point is on its own way on foot, `rally_m` short
    /// of the flag (along the bearing it comes from, turned if needed, where the map has no way), and squads whose points are within
    /// `rally_merge_m` share one. The squad's people on foot are held at it, not only its leader ([`Team::hold_orders`], after the
    /// squads' orders). Its flag is kept while it waits (the split does not send it elsewhere, [`held_flag`]).
    pub(super) fn regroup_way(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        let pp = self.params;
        let fl = &view.flags;
        let mate = |j: usize| view.mates.iter().find(|c| c.player == j);
        self.held.clear();
        self.held_at.clear();
        self.rallies.retain(|&i, r| fl.get(i).is_some_and(|f| f.owner != view.team) && r.released.is_none_or(|x| t - x < pp.rally_released_s));
        for (i, f) in fl.iter().enumerate() {
            if f.owner == view.team || !f.takeable {
                continue;
            }
            let going: Vec<usize> = (0..self.squads.len()).filter(|&k| self.squads[k].goal == Goal::Flag(i)).collect();
            if going.is_empty() || self.rallies.get(&i).is_some_and(|r| r.released.is_some()) {
                continue;
            }
            // the squads that can be held: the leader on foot, walking, not urgent and not yet in
            let holdable = |e: &Squad, at: [f32; 3]| {
                mate(e.leader).is_some_and(|c| c.vehicle.is_none() && flat(c.pos, f.pos) > pp.rally_inside * flat(at, f.pos)) && e.trip.as_ref().is_none_or(|tr| tr.plain()) && e.planes.is_empty() && !self.urgent_ones.contains(&e.leader)
            };
            // each squad coming on foot gets its point once it is within `rally_start_m`
            for &k in &going {
                let e = &self.squads[k];
                if self.rallies.get(&i).is_some_and(|r| r.points.contains_key(&e.leader)) {
                    continue;
                }
                let Some(l) = mate(e.leader).filter(|c| c.vehicle.is_none() && (pp.rally_m + pp.rally_near_m..pp.rally_start_m).contains(&flat(c.pos, f.pos))) else { continue };
                let Some(at) = rally_point(l.pos, f.pos, pp.rally_m, land) else { continue };
                let r = self.rallies.entry(i).or_insert_with(|| Rally { at, made: t, first: None, released: None, points: BTreeMap::new() });
                let shared = r.points.values().copied().filter(|p| flat(*p, at) < pp.rally_merge_m).min_by(|a, b| flat(*a, at).total_cmp(&flat(*b, at)).then(a[0].total_cmp(&b[0])).then(a[2].total_cmp(&b[2])));
                r.points.insert(e.leader, shared.unwrap_or(at));
            }
            let Some(r) = self.rallies.get(&i).cloned() else { continue };
            // (those with a point that come on foot: a squad in a vehicle goes straight in)
            let with_point: Vec<usize> = going.iter().copied().filter(|&k| r.points.contains_key(&self.squads[k].leader) && mate(self.squads[k].leader).is_some_and(|c| c.vehicle.is_none())).collect();
            let people: Vec<(usize, [f32; 3])> = with_point.iter().flat_map(|&k| self.squads[k].alive(view).into_iter().map(move |j| (j, k))).map(|(j, k)| (j, r.points[&self.squads[k].leader])).collect();
            let there = people.iter().filter(|&&(j, at)| mate(j).is_some_and(|c| c.vehicle.is_none() && flat(c.pos, at) < pp.rally_near_m)).count();
            let need = if self.parts.strength { ((NEED_PER_ENEMY * self.intel.presence(i)).ceil() as usize + 1).clamp(NEED_MIN, NEED_MAX) } else { people.len() }.min(people.len());
            let first = r.first.or((there > 0).then_some(t));
            // (with nobody with a point on foot now, a leader that died or got in a vehicle, only the clock lets the point go)
            let timed_out = match first {
                Some(x) => t - x >= pp.rally_wait_s,
                None => t - r.made >= pp.rally_come_s,
            };
            let go = timed_out || (!people.is_empty() && there >= need);
            let rr = self.rallies.get_mut(&i).unwrap();
            rr.first = first;
            if go {
                rr.released = Some(t);
                if crate::bots::trace_team() {
                    eprintln!("t={t:.0} team {} regroup-way: flag {i} goes in with {there} of {need} (needed)", view.team);
                }
                continue;
            }
            for &k in &with_point {
                let e = &self.squads[k];
                let at = r.points[&e.leader];
                if holdable(e, at) {
                    self.held.insert(e.leader, i);
                    self.held_at.insert(e.leader, at);
                }
            }
            if crate::bots::trace_team() {
                let held: Vec<(usize, [i32; 2])> = self.held_at.iter().filter(|h| self.held.get(h.0) == Some(&i)).map(|(&l, p)| (l, [p[0] as i32, p[2] as i32])).collect();
                eprintln!("t={t:.0} team {} regroup-way: flag {i}: {there} of {need} there, holds {held:?}", view.team);
            }
        }
    }

    /// `bots:regroup-way`: the held squads' people on foot go to their point (over the squads' orders; a driver or passenger of a plan
    /// keeps its seat).
    pub(super) fn hold_orders(&mut self, view: &TeamView) {
        for e in &self.squads {
            let Some(at) = self.held_at.get(&e.leader) else { continue };
            for j in e.alive(view) {
                let on_foot = view.mates.iter().any(|c| c.player == j && c.vehicle.is_none());
                let seated = self.orders.get(&j).is_some_and(|o| matches!(o.role, Role::Drive(_) | Role::Passenger(..)));
                if on_foot && !seated {
                    self.orders.insert(j, Order { goal: Goal::Point([at[0], at[2]]), role: Role::Leader, kit: 0 });
                }
            }
        }
    }

    /// `bots:regroup-way`: whether `player` waits on purpose: its squad is held and it is at its point.
    pub fn waits_at_rally(&self, player: usize, pos: [f32; 3]) -> bool {
        self.squads.iter().find(|e| e.all().any(|j| j == player)).and_then(|e| self.held_at.get(&e.leader)).is_some_and(|at| flat(*at, pos) < self.params.rally_near_m)
    }

    /// `bots:cover-flank`: what `player` is doing this tick: whether it covers, and the flank it goes round (flag, point).
    pub fn note_cover(&mut self, player: usize, t: f64, at_spot: bool, flank: Option<(usize, [f32; 3])>, pos: [f32; 3]) {
        self.cover.insert(player, (t, at_spot, flank.map(|f| (f.0, f.1, pos))));
    }

    /// `bots:cover-flank`: the others of `player`'s squad.
    pub(super) fn squad_mates(&self, player: usize) -> Vec<usize> {
        self.squads.iter().find(|q| q.leader == player || q.members.contains(&player)).map_or(Vec::new(), |q| q.all().filter(|&j| j != player).collect())
    }

    /// `bots:cover-flank`: whether a mate of `player`'s squad covers it (at its firing spot or fighting).
    pub fn covered(&self, player: usize, t: f64) -> bool {
        self.squad_mates(player).iter().any(|j| self.cover.get(j).is_some_and(|c| t - c.0 < COVER_FRESH_S && c.1))
    }

    /// `bots:cover-flank`: the flank a mate of `player`'s squad goes round, with where the enemy was last seen at that flag.
    pub fn cover_for(&self, player: usize, t: f64) -> Option<CoverFlank> {
        let (flag, to, from) = self.squad_mates(player).iter().filter_map(|j| self.cover.get(j)).filter(|c| t - c.0 < COVER_FRESH_S).find_map(|c| c.2)?;
        let f = self.flag_pos(flag)?;
        let enemy = self.reports.iter().filter(|r| t - r.t < CONTACT_S && flat(r.pos, f) < CONTACT_M).max_by(|a, b| a.t.total_cmp(&b.t).then(a.player.cmp(&b.player))).map(|r| r.pos);
        Some(CoverFlank { from, to, enemy })
    }

    /// `bots:steal-flank`: the vehicles the side saw an enemy in within [`ENEMY_DRIVEN_S`], the latest first.
    pub fn enemy_vehicles(&self, t: f64) -> [usize; 8] {
        let mut l: Vec<(usize, f64)> = self.enemy_driven.iter().filter(|x| t - x.1 < ENEMY_DRIVEN_S).map(|(&k, &x)| (k, x)).collect();
        l.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut out = [usize::MAX; 8];
        for (o, x) in out.iter_mut().zip(l) {
            *o = x.0;
        }
        out
    }

    /// `bots:flank2`: what `player` is told if it is a flanker.
    pub fn flank(&self, player: usize, t: f64) -> Option<Flank> {
        let (_, &(_, flag, _)) = self.flankers.iter().find(|(_, x)| x.0 == player)?;
        let near: Vec<[f32; 3]> = self.flag_pos(flag).map_or(Vec::new(), |f| self.reports.iter().filter(|r| t - r.t < CONTACT_S && flat(r.pos, f) < CONTACT_M).map(|r| r.pos).collect());
        let enemy = if near.is_empty() { self.flag_pos(flag)? } else { [near.iter().map(|p| p[0]).sum::<f32>() / near.len() as f32, near.iter().map(|p| p[1]).sum::<f32>() / near.len() as f32, near.iter().map(|p| p[2]).sum::<f32>() / near.len() as f32] };
        let mut used = [false; FLANK_SECTORS];
        for a in self.arrivals.iter().filter(|a| a.1 == flag && t - a.0 < ARRIVALS_S) {
            used[a.2] = true;
        }
        Some(Flank { flag, enemy, used })
    }

    /// `bots:flank2`: the arrivals since the last split, and the flanker of each squad with contact at its flag.
    pub(super) fn flankers(&mut self, t: f64, view: &TeamView) {
        let fl = &view.flags;
        let mut now = BTreeSet::new();
        for c in view.mates.iter().filter(|c| c.vehicle.is_none()) {
            for (i, f) in fl.iter().enumerate().filter(|(_, f)| f.owner != view.team && flat(c.pos, f.pos) < ARRIVAL_M) {
                now.insert((c.player, i));
                if !self.arrived.contains(&(c.player, i)) {
                    self.arrivals.push((t, i, sector(f.pos, c.pos)));
                }
            }
        }
        self.arrived = now;
        self.arrivals.retain(|a| t - a.0 < ARRIVALS_S);
        let pos = |j: usize| view.mates.iter().find(|c| c.player == j && c.vehicle.is_none()).map(|c| c.pos);
        let mut keep = BTreeMap::new();
        for e in &self.squads {
            let Goal::Flag(i) = e.goal else { continue };
            let Some(f) = fl.get(i).filter(|f| f.owner != view.team) else { continue };
            let contact = self.reports.iter().any(|r| t - r.t < CONTACT_S && flat(r.pos, f.pos) < CONTACT_M);
            let alive: Vec<usize> = e.members.iter().copied().filter(|&m| pos(m).is_some()).collect();
            if !contact || pos(e.leader).is_none() || alive.is_empty() {
                continue;
            }
            let held = self.flankers.get(&e.leader).filter(|x| x.1 == i && t - x.2 < FLANKER_S && alive.contains(&x.0)).copied();
            let chosen = held.or_else(|| alive.iter().copied().min_by(|&a, &b| flat(pos(a).unwrap(), f.pos).total_cmp(&flat(pos(b).unwrap(), f.pos)).then(a.cmp(&b))).map(|m| (m, i, t)));
            if let Some(x) = chosen {
                keep.insert(e.leader, x);
            }
        }
        self.flankers = keep;
    }
}
