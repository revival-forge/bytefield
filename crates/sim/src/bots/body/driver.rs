//! Riding a ground vehicle or a fixed weapon: look for a free one nearby (or a passenger seat), get to it, board, and at the
//! wheel head for a zone over the tanks' path map, stopping now and then to fire, braking for a mate on foot and
//! reversing when stuck. Leaving it when it flips, burns or is stuck is decided by whoever drives it (`life::Bot`, today).
use super::rng::Rng;
use super::walker::{Walker, xz, unknown_height};
use super::paths::Answer;
use super::{Maps, MapProfile, with_ground, dist3, flat, angle_between, vehicle_floor};
use crate::bot_api::{BotView, Inside, VehicleClass, VehicleSeen};
use crate::soldier::axis;

/// How close a fixed weapon must be to catch someone who was not heading for it, m.
pub const NEAR_FIXED: f32 = 90.0;
/// How far it looks for a vehicle of interest, m.
pub const VEHICLE_RADIUS: f32 = 100.0;
/// Height above the ground from which nobody boards a fixed weapon: you die getting off, m.
pub const FIXED_HEIGHT: f32 = 6.0;
/// How far on foot it goes for a plane when it wants to fly, m.
pub const PLANE_RADIUS: f32 = 280.0;
/// Seconds without moving from the spot after which the driver leaves the vehicle.
pub const STUCK: f64 = 25.0;
/// Speed at which it stops accelerating, m/s: faster, cars flip and crash.
pub const MAX_SPEED: f32 = 13.0;
/// The throttle of a car or tank turning to a point well off its nose ([`Driver::drive_fine`]).
pub const THROTTLE_TURNING: f32 = 0.5;
/// A point of the path this close is reached, whether the next one is in sight or not, m.
pub const REACHED_M: f32 = 4.0;
/// The stretch next to the vehicle not looked at when telling whether the next point is in sight ([`super::paths::Grid::line_clear`]), m.
pub const LINE_SKIP_M: f32 = 3.0;
/// Half the lane a vehicle brakes in for a mate on foot ahead ([`Driver::drive_fine`]), m.
pub const LANE_M: f32 = 2.5;
/// bots: seconds without moving a metre on its way to a vehicle it picked itself (the way is a straight line) before it gives that one up
/// for this life: GC_Judicator's sides spawn on a deck walled round, and one went for a fighter 150 m off through the wall, pushing for
/// a minute and a half.
pub const OWN_STILL_S: f64 = 8.0;
/// Closer than this to the shore it swims from, it goes straight into the water without a path to it, m.
pub const SHORE_M: f32 = 12.0;
/// A swimmer leaves the water by a bank at most this high over the water line, m.
pub const LOW_BANK_M: f32 = 2.0;
/// bots: seconds with the next point of the path out of sight over the map before the path is asked again ([`Driver::lost_since`]).
pub const LOST_S: f64 = 3.0;
/// `bots:streetwise`: a free vehicle of the enemy's side is taken within this, m, with health over [`STEAL_HEALTH`] and no enemy seen
/// within [`STEAL_CLEAR_M`] of it, m.
pub const STEAL_M: f32 = 60.0;
pub const STEAL_HEALTH: f32 = 0.5;
pub const STEAL_CLEAR_M: f32 = 40.0;
/// `bots:streetwise`: an empty vehicle this near an enemy flag is the enemy's, m.
pub const STEAL_FLAG_M: f32 = 150.0;

#[derive(Debug, Default)]
pub struct Driver {
    /// when it looks for a vehicle, if this life looks for one
    pub wants_board_at: Option<f64>,
    /// `bots:streetwise`: it also takes a free vehicle of the enemy's side ([`STEAL_M`])
    pub steals: bool,
    /// `bots:steal-flank`: the vehicles its side saw an enemy drive: the only ones of the enemy it steals
    pub enemy_seen: Option<Vec<usize>>,
    /// `bots:steal-flank`: on its flank it goes only for a vehicle it steals, whatever it wanted before
    pub only_steal: bool,
    /// the vehicle it goes for is one it steals
    pub stealing: bool,
    /// when it looks again for a vehicle to steal
    pub steal_look: f64,
    /// the vehicle it is heading to
    pub vehicle: Option<usize>,
    pub vehicle_since: f64,
    /// where and when it last moved a metre on its way to [`Driver::vehicle`] (bots: [`OWN_STILL_S`]), and when it last went for it
    pub approach: ([f32; 3], f64, f64),
    /// when it gets off
    pub leave_at: Option<f64>,
    pub fixed_weapon: bool,
    /// vehicles it could not reach or could not fix in this life
    pub vetoed: Vec<usize>,
    /// this life it stops at a fixed weapon if it is caught nearby
    pub wants_fixed: bool,
    /// this life it prefers a plane
    pub wants_fly: bool,
    pub mines_seen: usize,
    /// vehicles its team has given to someone else (bots): it does not take them on its own. Empty in the plain bot
    pub reserved: Vec<usize>,
    /// the bots' driving (B01: cars 20-30 s at the spawn; false in the plain bot): (1) a car (not a tank) with its next point well off the
    /// nose turns on the move instead of stopping to turn (stopped, a car does not turn: it only got out by the jam's reversing
    /// every few seconds); (2) it brakes for a mate on foot only in its lane ([`LANE_M`]), not for the squad walking to the same
    /// flag some metres to the side
    pub drive_fine: bool,
    /// goes round the vehicle's body to the driver's entry when it is on the far side (bots; the plain bot walks straight at it)
    pub go_round_hull: bool,
    /// at the wheel of a vehicle that swims ([`crate::bot_api::Inside::swims`]), with no way on the vehicle path map and water on the
    /// straight line to the goal, it drives straight at the goal across the water (bots; the plain bot stands still). DC_Coastal_Hammer: the
    /// attackers' base is cut off by ~400 m of water 11 m deep, crossed only in the BMP-2 and BRDM-2; the path map has no way, and the
    /// driver sat in the vehicle with no axes until it got off.
    pub swim: bool,
    /// since when the vehicle path map has had no way to the goal nor to the ring around it (and it does not swim across): the driver
    /// sits with no axes. bots gets off after [`crate::bots::individual::NO_WAY_S`]; the plain bot waits for the jam (25 s)
    pub no_way: Option<f64>,
    /// a point of the path within 12 m is passed only if the next one is in sight over the map (bots; the plain bot passes them all, and
    /// cut the corners of ways out between walls)
    pub sight: bool,
    /// on this tick it drives straight across the water for want of a way ([`Driver::swim`]): bots's jam rule counts it as a way, since
    /// the straight line may run into a far bank it cannot climb (DC_Operation_Bragg's BMP-2 at the foot of the canyon's far side)
    pub swimming: bool,
    /// the path it follows ends at the shore it swims from ([`shore_towards`]), not at the goal
    pub to_shore: bool,
    /// the shore it would swim from cannot be reached over the path map: it does not swim
    pub no_shore: bool,
    /// the goal and the shore whose path search was left waiting on the last tick
    pub shore_wait: Option<([f32; 2], [f32; 2])>,
    /// since when the next point of the path has been out of sight over the map (bots, with [`Driver::sight`]): pushed off its way
    /// (by a fight, a slide down a slope, a collision) it drove at a point beyond a hill. GC_Dantooine's T-3B, going round a hill to
    /// the north, slid back down its south side while shooting, and pushed at the hill toward the point past it for the round
    pub lost_since: Option<f64>,
}

impl Driver {
    /// If it wants to board and there is something nearby: the axes to get there.
    // TODO: a big boat moored under a ledge is not boarded: GC_MonCal's `waveskimmer` (radius 21 m) lies at the water below the
    // attackers' platform, and the leader goes round its hull ~12 m from its centre for 20 s and gives up (the squad's only way to the
    // flags). How: `BYTEFIELD_BOTS_ME=0 tournament3 … --maps GCMOD:GC_MonCal --seeds 1 --minutes 2 --threads 1` (`seat Some((17, 0))`,
    // `go_round`) and `examples/obstacles_near data/game GCMOD GC_MonCal 308 1713 30`: where the driver's entry is and what stands
    // between it and the ledge.
    pub fn go_to_vehicle(&mut self, v: &BotView, profile: &MapProfile, alternate: &mut bool) -> Option<Vec<(usize, f32)>> {
        // (`bots:streetwise`: one that steals goes for a free vehicle of the enemy near even if this life wanted none; then only for
        // such a vehicle. Stealing only when it already wanted a vehicle almost never happened)
        let steal_only = (self.wants_board_at.is_none() || self.only_steal) && self.steals && self.vehicle.is_none_or(|_| self.stealing);
        let when = if steal_only { self.steal_look } else { self.wants_board_at? };
        if v.time < when {
            return None;
        }
        let me = &v.me;
        let seen_ones = v.vehicles();
        // the free wheel, or a passenger seat in one of its own (if full, it tires of waiting)
        let flies = |c: &VehicleSeen| c.class.flies();
        // (from a weapon set up high you only get off by falling)
        let up_high = |c: &VehicleSeen| c.fixed && v.ground(c.pos[0], c.pos[2]).is_some_and(|g| c.pos[1] - g > FIXED_HEIGHT);
        let reserved = &self.reserved;
        // (the enemies it sees near a vehicle of theirs: asked only when it steals)
        let enemies: Vec<[f32; 3]> = if self.steals { v.others().iter().filter(|o| o.team != me.team && dist3(o.pos, me.pos) < STEAL_M + STEAL_CLEAR_M && !v.hidden(me.eye, [o.pos[0], o.pos[1] + 1.2, o.pos[2]])).map(|o| o.pos).collect() } else { Vec::new() };
        let steals = self.steals;
        // (a vehicle left empty has no side: the enemy's is one by an enemy flag)
        let enemy_flags: Vec<[f32; 3]> = if steals { v.flags().iter().filter(|f| f.team != 0 && f.team != me.team).map(|f| f.pos).collect() } else { Vec::new() };
        let seen_driven = self.enemy_seen.as_deref();
        let theirs = |c: &VehicleSeen| {
            c.team != me.team
                && match seen_driven {
                    Some(l) => c.team != 0 || l.contains(&c.vehicle),
                    None => c.team != 0 || enemy_flags.iter().any(|f| flat(*f, [c.pos[0], c.pos[2]]) < STEAL_FLAG_M),
                }
        };
        let stolen = |c: &VehicleSeen| steals && theirs(c) && c.driver_free && !c.fixed && !c.class.flies() && c.health > STEAL_HEALTH && dist3(c.pos, me.pos) < STEAL_M && !enemies.iter().any(|e| dist3(*e, c.pos) < STEAL_CLEAR_M);
        let serves = |c: &VehicleSeen| !up_high(c) && !reserved.contains(&c.vehicle) && (c.team == 0 || c.team == me.team || stolen(c)) && c.health > 0.05 && (c.free_to_ride || (c.fixed && c.driver_free) || (flies(c) && c.driver_free) || (!c.driver_free && !c.fixed && !flies(c) && c.seats > 1 && c.team == me.team));
        let d = |c: &VehicleSeen| dist3(c.pos, me.pos);
        if let Some(j) = self.vehicle.filter(|j| !seen_ones.iter().any(|c| c.vehicle == *j && serves(c))) {
            let _ = j;
            self.vehicle = None;
        }
        if self.vehicle.is_none() {
            // what is really free: not those that already have as many mates ahead as seats. And a vehicle with a driver
            // that is moving is only boarded if it is beside it
            let amigos: Vec<[f32; 3]> = v.others().iter().filter(|o| o.team == me.team && !o.in_vehicle).map(|o| o.pos).collect();
            let (insists, wants_fixed, vetoed) = (profile.insists, self.wants_fixed, self.vetoed.clone());
            let has_room = |c: &VehicleSeen| {
                let plazas = if c.driver_free { c.seats } else { c.seats - 1 };
                !vetoed.contains(&c.vehicle) && !(insists && c.fixed && !(wants_fixed && d(c) < NEAR_FIXED)) && (c.driver_free || d(c) < 25.0) && amigos.iter().filter(|a| dist3(**a, c.pos) < d(c)).count() < plazas
            };
            // whoever wants to fly goes to a plane first, even if there is something closer
            let wants_fly = self.wants_fly;
            let possible: Vec<&VehicleSeen> = seen_ones.iter().filter(|c| serves(c) && (!steal_only || stolen(c)) && d(c) < if wants_fly && flies(c) { PLANE_RADIUS } else { VEHICLE_RADIUS } && has_room(c)).collect();
            let best = |list: &mut dyn Iterator<Item = &&VehicleSeen>| list.min_by(|a, b| d(a).total_cmp(&d(b)).then(a.vehicle.cmp(&b.vehicle))).map(|c| c.vehicle);
            let wants_fixed = self.wants_fixed && profile.insists;
            self.vehicle = if wants_fly { best(&mut possible.iter().filter(|c| flies(c))) } else { None }.or_else(|| if wants_fixed { best(&mut possible.iter().filter(|c| c.fixed)) } else { None }).or_else(|| best(&mut possible.iter()));
            self.vehicle_since = v.time;
            self.approach = (me.pos, v.time, v.time);
            self.stealing = steal_only && self.vehicle.is_some();
            if self.vehicle.is_none() {
                // nothing nearby: later
                if steal_only {
                    self.steal_look = v.time + 2.0;
                } else {
                    self.wants_board_at = Some(v.time + 8.0);
                }
                return None;
            }
        }
        // (counting from a metre moved, or from going for it again after a while doing something else: a fight)
        if flat(me.pos, [self.approach.0[0], self.approach.0[2]]) > 1.0 || v.time - self.approach.2 > 1.0 {
            self.approach = (me.pos, v.time, v.time);
        }
        self.approach.2 = v.time;
        let near = seen_ones.iter().find(|c| Some(c.vehicle) == self.vehicle).is_some_and(|c| flat(me.pos, [c.pos[0], c.pos[2]]) < c.radius + 3.0);
        // (beside it, it may wait a while for its turn at the hatch; not for ever: on DC's Gazala three of a side's soldiers stood
        // pressed round the BMP-2 of their base, each in the others' way to its entry, until the 30 s ran out)
        let still = self.go_round_hull && v.time - self.approach.1 > if near { 2.0 * OWN_STILL_S } else { OWN_STILL_S };
        // too long on the way: gives up, not in this life any more
        if still || v.time - self.vehicle_since > 75.0 || (v.time - self.vehicle_since > 30.0 && !seen_ones.iter().any(|c| Some(c.vehicle) == self.vehicle && flies(c))) {
            self.vetoed.extend(self.vehicle);
            self.vehicle = None;
            self.wants_board_at = profile.insists.then_some(v.time + 8.0);
            return None;
        }
        // (the way to it is a straight line: if it runs off a platform's edge, that vehicle is not for this life; one that has already
        // taken off from GC_Bespin's bases is the common case)
        let j = self.vehicle?;
        if let Some(c) = seen_ones.iter().find(|c| c.vehicle == j)
            && !with_ground(v, me.pos, [c.pos[0], c.pos[2]])
        {
            self.vetoed.push(j);
            self.vehicle = None;
            return None;
        }
        *alternate = !*alternate;
        Some(if self.go_round_hull { v.axes_to_seat_round(j, 0, *alternate) } else { v.axes_to_enter(j, *alternate) })
    }

    /// At the wheel: towards the flag over the tanks' map; stops on arrival and when there is someone to shoot.
    #[allow(clippy::too_many_arguments)]
    pub fn drive(&mut self, v: &BotView, inside: &Inside, axes: &mut Vec<(usize, f32)>, rng: &mut Rng, profile: &MapProfile, maps: &Maps, cam: &mut Walker, has_target: bool, attacks: bool, centre: [f32; 3], radio: f32) {
        let pos = inside.pos;
        self.swimming = false;
        // stops to fire now and then (in a tank, 20 % of the time at El Alamein): far from the flag, less
        let far = flat(pos, [centre[0], centre[2]]) > 150.0;
        if has_target && rng.si(if !profile.insists { 0.02 } else if far { 0.002 } else { 0.006 }) {
            cam.stopped_until = v.time + rng.uniform(2.0, 8.0) as f64;
        }
        if v.time < cam.stopped_until {
            return;
        }
        // whoever attacks it, if there is no need to wait, enters the radius: from outside it is not taken
        let enters = attacks && profile.enter_wait <= 0.0;
        let stop = if enters { (0.6 * radio).min(25.0) } else { 25.0 };
        let meta = [centre[0], centre[2]];
        if flat(pos, meta) < stop {
            cam.anchor_t = v.time;
            self.no_way = None;
            return;
        }
        // the mates have laid mines: the path must be looked at again, since a vehicle sets them off whoever they belong to
        let mines = maps.mines_of(v.me.team);
        let new_mines = mines.len() != self.mines_seen;
        self.mines_seen = mines.len();
        if new_mines && !cam.path.is_empty() {
            cam.point = None;
        }
        // over a map of its own with the water open (`BYTEFIELD_NAV_TANKS=gridswim`), what swims needs none of the shortcuts below.
        // Not on a deck: that map is flat, and from a bridge it led down into the river below (DC's Market_Garden, a BMP-2 pushing
        // at the railing of the bridge at 1181,993, 18 m over the water, for the round)
        let on_deck = v.ground(pos[0], pos[2]).is_some_and(|h| pos[1] - h.max(v.water()) > 5.0);
        let gridswim = self.swim && inside.swims && !on_deck && maps.swim(v).is_some();
        if cam.point != Some(meta) || (cam.path.is_empty() && v.time >= cam.look_path) {
            cam.point = Some(meta);
            cam.look_path = v.time + 10.0;
            // the flag's centre is usually closed to a tank: any point on a ring around it will do
            let de = [pos[0], pos[2]];
            let ring = (0..8).map(|k| k as f32 * std::f32::consts::FRAC_PI_4).map(|a| [meta[0] + (stop - 5.0).max(4.0) * a.cos(), meta[1] + (stop - 5.0).max(4.0) * a.sin()]);
            cam.path.clear();
            // (a search for the shore still going on: asking for anything else first would drop it from the queue)
            let shore_wait = self.shore_wait.take().filter(|w| w.0 == meta).map(|w| w.1);
            // in the water, swimming across: out on a low bank of the goal's side, not straight at the goal (DC_Urban_Siege's BRDM-2
            // swam the river into the far quay, 12 m of wall, and sat there)
            // TODO: a far bank that is free on the path map but too steep to climb out of the water is still taken (DC_Operation_Bragg's
            // BMP-2 at the foot of the canyon's far side, `up` 0.90, until the jam rule); with no low bank near (DC_Urban_Siege: a 12 m
            // quay) the driver gets off in the water. How: accept as `low` only cells whose ground rises gently from the water line (the
            // terrain's slope over the last few metres), and look further along the bank than `exit`'s 64 cells for a ramp.
            // `vehicle_trip data/game DC_Final DC_Operation_Bragg bmp2 32 4 --seed 1` (#11 at 1073,1432 from 80 s).
            let water = v.water();
            let afloat = self.swim && inside.swims && !gridswim && v.ground(pos[0], pos[2]).is_some_and(|h| h < water - 1.0);
            if afloat && shore_wait.is_none() {
                let low = |p: [f32; 2]| v.ground(p[0], p[1]).is_some_and(|h| h < water + LOW_BANK_M);
                match maps.request_tank_ashore(v, de, meta, &mines, &low) {
                    Answer::Path(c) => cam.path = c.into_iter().map(unknown_height).collect(),
                    Answer::Wait => {
                        cam.point = None;
                        return;
                    }
                    Answer::NoPath => {}
                }
            }
            for m in std::iter::once(meta).chain(ring).filter(|_| shore_wait.is_none() && cam.path.is_empty()) {
                match maps.request_vehicle_from(v, gridswim, pos, m, &mines, false) {
                    Answer::Path(c) => {
                        cam.path = c.into_iter().map(unknown_height).collect();
                        break;
                    }
                    Answer::Wait => {
                        // on the next tick it asks again, stopped
                        cam.point = None;
                        return;
                    }
                    Answer::NoPath => {}
                }
            }
            // no way by land and it swims: over the path map to the shore on the straight line to the goal, and across from there;
            // with no way to that shore either, not at all. Straight from where it was, it drove into whatever stood before the shore:
            // DC_Operation_Bragg's BMP-2 into the sandbags round its spawn (on a mesa whose way down to the river is a ramp off the
            // straight line), DC_Medina_Ridge's into the barracks beside its own, for the round
            self.to_shore = false;
            self.no_shore = false;
            if cam.path.is_empty() && self.swim && inside.swims && !gridswim
                && let Some(shore) = shore_wait.or_else(|| shore_towards(v, pos, meta))
                && flat(pos, shore) > SHORE_M
            {
                match maps.request_tank_from(v, pos, shore, &mines, false) {
                    Answer::Path(c) => {
                        cam.path = c.into_iter().map(unknown_height).collect();
                        self.to_shore = true;
                    }
                    Answer::Wait => {
                        self.shore_wait = Some((meta, shore));
                        cam.point = None;
                        return;
                    }
                    Answer::NoPath => self.no_shore = true,
                }
            }
        }
        // a point within reach is passed if the next one is in sight over the map: skipping every point closer than 12 m cut the
        // corners of a way out between walls (GC_Tatooine's AT-ST left the Imperial base by a gap to the south and then west: the
        // first five points were all within 12 m, it made straight for the sixth, 270 m off, and walked into the wall for the round)
        let grid = maps.vehicle(v, gridswim);
        let floor = vehicle_floor(pos);
        while cam.path.len() > 1 && flat(pos, xz(cam.path[0])) < 12.0 && (!self.sight || flat(pos, xz(cam.path[0])) < REACHED_M || grid.is_none_or(|g| g.line_clear_from(floor, xz(cam.path[1]), LINE_SKIP_M))) {
            cam.path.remove(0);
        }
        // off its way, the next point out of sight: after [`LOST_S`], a new path from here (not afloat: the path map closes the water,
        // and the bank it swims for is never in sight over it)
        let afloat = v.ground(pos[0], pos[2]).is_some_and(|h| h < v.water() - 1.0);
        let lost = self.sight && (!afloat || gridswim) && cam.path.first().is_some_and(|&p| grid.is_some_and(|g| !g.line_clear_from(floor, xz(p), LINE_SKIP_M)));
        match (lost, self.lost_since) {
            (false, _) => self.lost_since = None,
            (true, None) => self.lost_since = Some(v.time),
            (true, Some(s)) if v.time - s > LOST_S => {
                self.lost_since = None;
                cam.point = None;
            }
            _ => {}
        }
        // no way by land: across the water, straight, if it swims (the path is asked again every 10 s: on the far shore there is one)
        let swim = (self.swim && cam.path.is_empty() && inside.swims && !gridswim && !self.no_shore && water_between(v, pos, meta)).then_some(meta);
        let Some(p) = cam.path.first().copied().map(xz).or(swim) else {
            self.no_way.get_or_insert(v.time);
            return;
        };
        self.no_way = None;
        self.swimming = cam.path.is_empty();
        // a mate on foot ahead: brakes. At the base, on starting, running people over caused many of the deaths without a shot
        let f = inside.rows[2];
        let fine = self.drive_fine;
        let ahead = v.others().iter().any(|o| {
            let a = [o.pos[0] - pos[0], o.pos[2] - pos[2]];
            let d = a[0].hypot(a[1]);
            let ante = a[0] * f[0] + a[1] * f[2];
            o.team == v.me.team && !o.in_vehicle && d < 12.0 && d > 0.5 && ante / d > 0.7 && (!fine || (d * d - ante * ante).max(0.0).sqrt() < LANE_M)
        });
        if ahead {
            return;
        }
        let g = angle_between(inside.rows[2], [p[0] - pos[0], p[1] - pos[2]]);
        if flat(pos, p) < 8.0 && cam.path.len() == 1 {
            cam.path.clear();
            // (at the shore it goes on across the water, and asks for a way again on the far side)
            cam.look_path = if std::mem::take(&mut self.to_shore) { v.time + 10.0 } else { f64::MAX };
            return;
        }
        let t = cam.vehicle_jam(v, pos);
        if t > 5.0 {
            axes.push((axis::THROTTLE, -1.0));
            axes.push((axis::YAW, -g.signum()));
            if t > 7.0 {
                cam.anchor_t = v.time;
            }
        } else {
            axes.push((axis::YAW, (g / 20.0).clamp(-1.0, 1.0)));
            // TODO: GC's walkers (AT-ST) flip badly when turning at speed in the test harness:
            // turn on the spot and walk straight with gentle throttle, if a person at the controls does not flip
            let fast = inside.velocity.iter().map(|x| x * x).sum::<f32>() > MAX_SPEED * MAX_SPEED;
            // (a tank does not turn on the spot either: a Sherman, T-72, BMP-2 or BRDM-2 held at yaw 1 and gas 0 turns 0–2° in 6 s.
            // A BMP-2 at DC_Operation_Bragg's spawn, facing away from its way, sat there with yaw and no gas for the round)
            let turns_moving = matches!(inside.class, VehicleClass::Car | VehicleClass::Tank);
            let gas = if fast { 0.0 } else if g.abs() < 45.0 { 1.0 } else if self.drive_fine && turns_moving { THROTTLE_TURNING } else { 0.0 };
            axes.push((axis::THROTTLE, gas));
        }
    }
}

/// The last dry point before the first water on the straight line from `pos` to `to`, both sampled every 10 m as [`water_between`] does.
fn shore_towards(v: &BotView, pos: [f32; 3], to: [f32; 2]) -> Option<[f32; 2]> {
    let water = v.water();
    let d = flat(pos, to);
    let n = (d / 10.0).ceil().max(1.0) as usize;
    let at = |f: f32| [pos[0] + f * (to[0] - pos[0]), pos[2] + f * (to[1] - pos[2])];
    let wet = (1..n).find(|&k| v.ground(at(k as f32 / n as f32)[0], at(k as f32 / n as f32)[1]).is_some_and(|h| h < water - 1.5))?;
    Some(at((wet as f32 - 1.0) / n as f32))
}

/// Water on the straight line from `pos` to `to` (the ground under it, every 10 m, below the water line by more than a wading depth).
fn water_between(v: &BotView, pos: [f32; 3], to: [f32; 2]) -> bool {
    let water = v.water();
    let d = flat(pos, to);
    let n = (d / 10.0).ceil().max(1.0) as usize;
    (1..n).any(|k| {
        let f = k as f32 / n as f32;
        v.ground(pos[0] + f * (to[0] - pos[0]), pos[2] + f * (to[1] - pos[2])).is_some_and(|h| h < water - 1.5)
    })
}
