//! Riding a ground vehicle or a fixed weapon: look for a free one nearby (or a passenger seat), get to it, board, and at the
//! wheel head for a zone over the tanks' path map, stopping now and then to fire, braking for a mate on foot and
//! reversing when stuck. Leaving it when it flips, burns or is stuck is decided by whoever drives it (`bots2::Bot`, today).
use super::rng::Rng;
use super::walker::Walker;
use super::paths::Answer;
use super::{Maps, MapProfile, with_ground, dist3, flat, angle_between};
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
/// The throttle of a car turning to a point well off its nose ([`Driver::drive_fine`]).
pub const THROTTLE_TURNING: f32 = 0.5;
/// Half the lane a vehicle brakes in for a mate on foot ahead ([`Driver::drive_fine`]), m.
pub const LANE_M: f32 = 2.5;

#[derive(Debug, Default)]
pub struct Driver {
    /// when it looks for a vehicle, if this life looks for one
    pub wants_board_at: Option<f64>,
    /// the vehicle it is heading to
    pub vehicle: Option<usize>,
    pub vehicle_since: f64,
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
    /// vehicles its team has given to someone else (bots3): it does not take them on its own. Empty in bots2
    pub reserved: Vec<usize>,
    /// bots3's driving (B01: cars 20-30 s at the spawn; false in bots2): (1) a car (not a tank) with its next point well off the
    /// nose turns on the move instead of stopping to turn (stopped, a car does not turn: it only got out by the jam's reversing
    /// every few seconds); (2) it brakes for a mate on foot only in its lane ([`LANE_M`]), not for the squad walking to the same
    /// flag some metres to the side
    pub drive_fine: bool,
    /// goes round the vehicle's body to the driver's entry when it is on the far side (bots3; bots2 walks straight at it)
    pub go_round_hull: bool,
    /// at the wheel of a vehicle that swims ([`crate::bot_api::Inside::swims`]), with no way on the vehicle path map and water on the
    /// straight line to the goal, it drives straight at the goal across the water (bots3; bots2 stands still). DC_Coastal_Hammer: the
    /// attackers' base is cut off by ~400 m of water 11 m deep, crossed only in the BMP-2 and BRDM-2; the path map has no way, and the
    /// driver sat in the vehicle with no axes until it got off.
    pub swim: bool,
    /// since when the vehicle path map has had no way to the goal nor to the ring around it (and it does not swim across): the driver
    /// sits with no axes. bots3 gets off after [`crate::bots3::individual::NO_WAY_S`]; bots2 waits for the jam (25 s)
    pub no_way: Option<f64>,
}

impl Driver {
    /// If it wants to board and there is something nearby: the axes to get there.
    // TODO: a big boat moored under a ledge is not boarded: GC_MonCal's `waveskimmer` (radius 21 m) lies at the water below the
    // attackers' platform, and the leader goes round its hull ~12 m from its centre for 20 s and gives up (the squad's only way to the
    // flags). How: `BYTEFIELD_BOTS3_ME=0 tournament3 … --maps GCMOD:GC_MonCal --seeds 1 --minutes 2 --threads 1` (`seat Some((17, 0))`,
    // `go_round`) and `examples/obstacles_near data/game GCMOD GC_MonCal 308 1713 30`: where the driver's entry is and what stands
    // between it and the ledge.
    pub fn go_to_vehicle(&mut self, v: &BotView, profile: &MapProfile, alternate: &mut bool) -> Option<Vec<(usize, f32)>> {
        let when = self.wants_board_at?;
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
        let serves = |c: &VehicleSeen| !up_high(c) && !reserved.contains(&c.vehicle) && (c.team == 0 || c.team == me.team) && c.health > 0.05 && (c.free_to_ride || (c.fixed && c.driver_free) || (flies(c) && c.driver_free) || (!c.driver_free && !c.fixed && !flies(c) && c.seats > 1 && c.team == me.team));
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
            let possible: Vec<&VehicleSeen> = seen_ones.iter().filter(|c| serves(c) && d(c) < if wants_fly && flies(c) { PLANE_RADIUS } else { VEHICLE_RADIUS } && has_room(c)).collect();
            let best = |list: &mut dyn Iterator<Item = &&VehicleSeen>| list.min_by(|a, b| d(a).total_cmp(&d(b)).then(a.vehicle.cmp(&b.vehicle))).map(|c| c.vehicle);
            let wants_fixed = self.wants_fixed && profile.insists;
            self.vehicle = if wants_fly { best(&mut possible.iter().filter(|c| flies(c))) } else { None }.or_else(|| if wants_fixed { best(&mut possible.iter().filter(|c| c.fixed)) } else { None }).or_else(|| best(&mut possible.iter()));
            self.vehicle_since = v.time;
            if self.vehicle.is_none() {
                // nothing nearby: later
                self.wants_board_at = Some(v.time + 8.0);
                return None;
            }
        }
        // too long on the way: gives up, not in this life any more
        if v.time - self.vehicle_since > 75.0 || (v.time - self.vehicle_since > 30.0 && !seen_ones.iter().any(|c| Some(c.vehicle) == self.vehicle && flies(c))) {
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
        if cam.point != Some(meta) || (cam.path.is_empty() && v.time >= cam.look_path) {
            cam.point = Some(meta);
            cam.look_path = v.time + 10.0;
            // the flag's centre is usually closed to a tank: any point on a ring around it will do
            let de = [pos[0], pos[2]];
            let ring = (0..8).map(|k| k as f32 * std::f32::consts::FRAC_PI_4).map(|a| [meta[0] + (stop - 5.0).max(4.0) * a.cos(), meta[1] + (stop - 5.0).max(4.0) * a.sin()]);
            cam.path.clear();
            for m in std::iter::once(meta).chain(ring) {
                match maps.request_tank(v, de, m, &mines, false) {
                    Answer::Path(c) => {
                        cam.path = c;
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
        }
        while cam.path.len() > 1 && flat(pos, cam.path[0]) < 12.0 {
            cam.path.remove(0);
        }
        // no way by land: across the water, straight, if it swims (the path is asked again every 10 s: on the far shore there is one)
        let swim = (self.swim && cam.path.is_empty() && inside.swims && water_between(v, pos, meta)).then_some(meta);
        let Some(p) = cam.path.first().copied().or(swim) else {
            self.no_way.get_or_insert(v.time);
            return;
        };
        self.no_way = None;
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
            cam.look_path = f64::MAX;
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
            let gas = if fast { 0.0 } else if g.abs() < 45.0 { 1.0 } else if self.drive_fine && inside.class == VehicleClass::Car { THROTTLE_TURNING } else { 0.0 };
            axes.push((axis::THROTTLE, gas));
        }
    }
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
