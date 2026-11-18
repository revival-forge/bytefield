//! The squad's trip: how it gets to the flag the team gave it, in legs, like a player who looks at the map before setting off.
//! Is there a way by land? Is the flag taken standing on the ground? What is at hand (a boat, a transport helicopter)? With a
//! way by land and the flag on the ground, the trip is one ground leg: on foot, or in the ground vehicle the squad fills at hand
//! ([`super::squad::orders`], as before trips existed). Without a way by land and with water in between, a boat and then on foot;
//! with no boat, or no water, a helicopter that lands near the flag; a flag up in the air (a platform, a ship's deck), only by air.
//!
//! **Every leg has a deadline and a test of progress**, measured from outside: boarding within [`BOARD_S`] of reaching the vehicle
//! (or later, while the leader gets [`APPROACH_M`] nearer it each window);
//! a vehicle that moves at least [`MOVE_M`] each [`WINDOW_S`]; an aircraft that is [`AIR_UP_M`] above what is below it at
//! [`AIR_UP_S`] after boarding; on foot, the leader moving [`MOVE_M`] each window (two windows in a row, fighting or being near the
//! flag excused). A leg that does not progress fails: the squad does not try that mode for that flag for [`RETRY_S`], and if no
//! other mode is left, the team gives it another flag. No trip dies by the clock while it progresses.
//!
//! Only what a player sees: distances, water, heights, vehicle classes and free seats; nothing by map or template name.
use super::team::{FlagSighting, VehicleSighting};
use crate::bot_api::VehicleClass;

/// Boarding: the driver is in the vehicle within this of reaching it, s (the walk there is added: [`WALK_SPEED`]).
pub const BOARD_S: f64 = 20.0;
/// A walking soldier's speed for the boarding deadline, m/s (a little under a run: paths wind).
pub const WALK_SPEED: f32 = 4.0;
/// The window of the progress test, s, and what must be moved in it, m.
pub const WINDOW_S: f64 = 15.0;
pub const MOVE_M: f32 = 30.0;
/// An aircraft in a leg: this high over what is below it, m, this long after boarding, s.
pub const AIR_UP_M: f32 = 15.0;
pub const AIR_UP_S: f64 = 30.0;
/// Past the boarding deadline, what the leader must get nearer the vehicle in each [`WINDOW_S`] for the leg to go on, m.
pub const APPROACH_M: f32 = 10.0;
/// Windows in a row without progress that fail a ground leg (on foot a squad stops to take cover or wait for its own).
pub const GROUND_WINDOWS: u8 = 2;
/// A failed (flag, mode) is not tried again for this long, s.
pub const RETRY_S: f64 = 60.0;
/// A ground leg closer than this to its flag has arrived: what is done there is fighting for it, not travelling, m.
pub const NEAR_GOAL_M: f32 = 100.0;
/// An enemy reported closer than this to the leader: the squad is fighting, and standing still is not being stuck, m.
pub const FIGHT_M: f32 = 150.0;
/// A boat or a helicopter is at hand for a leg if closer than this to the leader, m (the squad walks to it).
pub const AT_HAND_M: f32 = 300.0;
/// A boat bigger than this does not land its people (it stays off the coast: [`crate::bots3::body::sailor::BIG_SHIP_RADIUS`]), m.
pub const BIG_BOAT_M: f32 = 25.0;
/// A flight is taken with a way by land only if it arrives this much sooner, s (a player does not walk to a helicopter to save
/// a few seconds: boarding, the climb and the landing are where it is shot at).
pub const AIR_GAIN_S: f64 = 20.0;
/// The estimates of how long a trip takes: a squad on foot, m/s; a ground vehicle, m/s, and what boarding it costs, s; a transport
/// helicopter in transit, m/s, and what boarding, climbing and landing cost it, s; how much longer than the straight line a way by
/// land is.
pub const FOOT_SPEED: f32 = 4.5;
pub const CAR_SPEED: f32 = 10.0;
pub const CAR_BOARD_S: f64 = 15.0;
pub const HELI_SPEED: f32 = 28.0;
pub const HELI_OVERHEAD_S: f64 = 55.0;
pub const DETOUR: f32 = 1.3;
/// A ground vehicle this close to the leader takes the squad by land in the estimate ([`super::squad::VEHICLE_NEAR`]), m.
pub const CAR_NEAR_M: f32 = 50.0;
/// Where a helicopter lands to take a flag on the ground: on the ring between these distances from it, m (not on it: on the flag
/// the defenders shoot the helicopter down with all aboard; Basrahs and No_Fly_Zone_Day2, the four dead as it touched down).
pub const LAND_NEAR_M: f32 = 45.0;
pub const LAND_FAR_M: f32 = 75.0;
/// A landing spot is flat if the ground under the rotor (this far around it, m) does not vary more than this, m.
pub const LAND_SPAN_M: f32 = 7.0;
pub const LAND_FLAT_M: f32 = 2.5;
/// An enemy reported this close to a landing spot makes it worse, m; on arrival (the aircraft this close to it, m), it makes the
/// pilot pick another, once.
pub const LAND_ENEMY_M: f32 = 80.0;
pub const LAND_RETHINK_M: f32 = 350.0;

/// How a leg is travelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// by land: on foot, or in the ground vehicle the squad fills at hand (its plan is the squad's, as before trips existed)
    Ground,
    /// in that boat, to the shore nearest the flag; everybody gets off on touching land
    Boat(usize),
    /// in that aircraft
    Air(usize),
}

impl Mode {
    /// The mode without the vehicle: what a failure bans.
    pub fn kind(self) -> Kind {
        match self {
            Mode::Ground => Kind::Ground,
            Mode::Boat(_) => Kind::Boat,
            Mode::Air(_) => Kind::Air,
        }
    }

    pub fn vehicle(self) -> Option<usize> {
        match self {
            Mode::Ground => None,
            Mode::Boat(v) | Mode::Air(v) => Some(v),
        }
    }
}

/// A mode without its vehicle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ground,
    Boat,
    Air,
}

/// How a leg ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum End {
    /// at the point (the last leg: inside the flag's radius)
    Arrive,
    /// the aircraft lands at the point and its passengers get off
    Land,
    /// the aircraft passes over the point, above it, and everyone in it jumps there with the parachute the body already opens
    /// ([`Pilot::fall`]): a plane with passenger seats does not hold still to land ([`drop_aircraft`]). Chosen only for a flag that is
    /// not reached on foot, with no aircraft that lands at hand.
    /// TODO: with a way by land it is never chosen, however long the way (side 2 of DC_Operation_Bragg has a 6-seat `Plane` at
    /// (374, 45, 200) and kilometres to walk): it needs its own flight time in `air_time` (a runway, no landing).
    Drop,
    /// an attack run on a target fixed for the whole run (frente I4; not chosen yet)
    Pass { target: [f32; 3] },
}

/// A leg of the trip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    pub mode: Mode,
    pub to: [f32; 3],
    pub end: End,
    /// in a vehicle leg, the instant by which its driver must be in it
    pub deadline: f64,
}

/// Whether there is a way by land between two points, according to the path map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Path,
    NoPath,
    /// the map does not say (no map, or one end in a place it draws closed: a spawn inside a depot, a flag in a bunker)
    Unknown,
}

/// What the trip asks of the map, as a player who looks at it: a path by land, water in between, the height of what is below.
pub trait Land {
    /// The way by land from `from` to some point of the circle of radius `r` around `to`.
    fn reach(&self, from: [f32; 3], to: [f32; 3], r: f32) -> Reach;
    /// Whether the straight line from `a` to `b` crosses water (a river, a strait).
    fn wet(&self, a: [f32; 3], b: [f32; 3]) -> bool;
    /// The height of what is below a point: the ground or the water.
    fn below(&self, x: f32, z: f32) -> f32;
    /// The height of the water's surface (`f32::MIN` when the map does not say).
    fn water(&self) -> f32 {
        f32::MIN
    }
    /// Nothing stands over the ground at a point up to this height above it (a roof, a tree, a mast): a rotor fits there.
    fn clear(&self, _x: f32, _z: f32, _up: f32) -> bool {
        true
    }
    /// How long the way on foot from `from` to the circle of radius `r` around `to` is, m (`None`: not known).
    fn way_m(&self, _from: [f32; 3], _to: [f32; 3], _r: f32) -> Option<f32> {
        None
    }
}

/// A jump is only for a flag with at least this radius, m: whoever jumps over it is inside it on the way down (GC_Vanguard's, 65
/// to 150 m around its ships).
/// TODO: a narrow flag up in the air asks for coming down on what holds it, and that loses as it is: on GC_Bespin (radius 9 to 20,
/// under the city's roof) the squads that jumped came down on the roof, fell off it or flew into the city, and bots3 against bots2
/// went from +13 tickets and 8 rounds of 8 to −7 and none, with one flag taken in 8 rounds. It needs the place to come down
/// ([`BotView::floor`], open sky over it) chosen before the flight.
pub const DROP_WIDE_M: f32 = 40.0;

/// A map that does not know: everything is reached by land, nothing is wet (the trip is always one ground leg).
pub struct Unknown;

impl Land for Unknown {
    fn reach(&self, _: [f32; 3], _: [f32; 3], _: f32) -> Reach {
        Reach::Unknown
    }
    fn wet(&self, _: [f32; 3], _: [f32; 3]) -> bool {
        false
    }
    fn below(&self, _: f32, _: f32) -> f32 {
        f32::MIN
    }
}

/// What a squad has tried and failed: that mode is not tried for that flag until the instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Failed {
    pub flag: usize,
    pub kind: Kind,
    pub until: f64,
}

/// The trip of a squad to a flag.
#[derive(Clone, Debug, PartialEq)]
pub struct Trip {
    pub flag: usize,
    pub legs: Vec<Leg>,
    /// the leg it is on
    pub leg: usize,
    /// when the leg it is on began
    pub since: f64,
    /// some of the squad has been aboard the leg's vehicle
    pub boarded: Option<f64>,
    /// the progress window: since when, from where (the leader on foot, the vehicle once aboard), who led it, and the windows missed
    window: Option<(f64, [f32; 3], usize)>,
    missed: u8,
    /// the landing spot was changed on arrival (enemies at it): it is changed once
    pub moved: bool,
}

/// What the squad looks like at an instant, for the progress test (measured from outside: positions, who is in what).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub t: f64,
    pub leader: usize,
    pub leader_pos: [f32; 3],
    /// some of the squad is in the leg's vehicle
    pub aboard: bool,
    /// where the leg's vehicle is, if it is still there (not wrecked)
    pub vehicle: Option<[f32; 3]>,
    /// the squad is fighting (an enemy reported near the leader): standing still then is not being stuck
    pub fighting: bool,
    /// a ground vehicle plan of the squad in time (it is boarding: not stuck)
    pub boarding: bool,
    /// the height of what is below the leg's vehicle (ground or water)
    pub below: f32,
}

/// What the progress test says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Going,
    /// the leg failed: what to ban
    Failed(Kind),
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Trip {
    /// One ground leg to the flag: the trip of before trips existed.
    pub fn ground(flag: usize, to: [f32; 3], t: f64) -> Trip {
        Trip { flag, legs: vec![Leg { mode: Mode::Ground, to, end: End::Arrive, deadline: f64::INFINITY }], leg: 0, since: t, boarded: None, window: None, missed: 0, moved: false }
    }

    /// The leg it is on.
    pub fn now(&self) -> Leg {
        self.legs[self.leg.min(self.legs.len() - 1)]
    }

    /// Only one ground leg (what the squad did before trips existed).
    pub fn plain(&self) -> bool {
        self.legs.len() == 1 && self.legs[0].mode == Mode::Ground
    }

    fn next_leg(&mut self, t: f64) {
        self.leg += 1;
        self.since = t;
        self.boarded = None;
        self.window = None;
        self.missed = 0;
    }

    /// The progress test at an instant (called at each split); passes to the next leg when the vehicle's people got off.
    pub fn check(&mut self, s: &Sample) -> Step {
        let leg = self.now();
        let t = s.t;
        // a different leader (the old one fell): the window starts again
        if self.window.is_some_and(|w| w.2 != s.leader) {
            self.window = None;
            self.missed = 0;
        }
        match leg.mode {
            Mode::Ground => {
                if s.fighting || s.boarding || flat(s.leader_pos, leg.to) < NEAR_GOAL_M {
                    self.window = None;
                    self.missed = 0;
                    return Step::Going;
                }
                let (t0, p0, _) = *self.window.get_or_insert((t, s.leader_pos, s.leader));
                if (0..3).map(|k| (s.leader_pos[k] - p0[k]).powi(2)).sum::<f32>().sqrt() >= MOVE_M {
                    self.window = Some((t, s.leader_pos, s.leader));
                    self.missed = 0;
                } else if t - t0 >= WINDOW_S {
                    self.missed += 1;
                    self.window = Some((t, s.leader_pos, s.leader));
                    if self.missed >= GROUND_WINDOWS {
                        return Step::Failed(Kind::Ground);
                    }
                }
                Step::Going
            }
            Mode::Boat(_) | Mode::Air(_) => {
                let kind = leg.mode.kind();
                match (self.boarded, s.aboard, s.vehicle) {
                    // got off (on the shore, landed) or the vehicle is gone with them: the next leg
                    (Some(_), false, _) | (Some(_), _, None) => {
                        if self.leg + 1 < self.legs.len() {
                            self.next_leg(t);
                            Step::Going
                        } else {
                            Step::Failed(kind)
                        }
                    }
                    (None, _, None) => Step::Failed(kind),
                    (None, false, Some(p)) => {
                        if t <= leg.deadline {
                            return Step::Going;
                        }
                        // past the deadline, still going while the leader gets nearer the vehicle each window (the walk to it was
                        // longer than the straight line: GC_MonCal's boat lies under the base's platform, ~50 s round, and the squad
                        // reached it as the leg failed)
                        let (t0, p0, _) = *self.window.get_or_insert((t, s.leader_pos, s.leader));
                        if t - t0 < WINDOW_S {
                            Step::Going
                        } else if flat(s.leader_pos, p) + APPROACH_M <= flat(p0, p) {
                            // (the deadline moves too: the squad's plan for the vehicle lasts until it)
                            self.window = Some((t, s.leader_pos, s.leader));
                            let k = self.leg.min(self.legs.len() - 1);
                            self.legs[k].deadline = t + WINDOW_S;
                            Step::Going
                        } else {
                            Step::Failed(kind)
                        }
                    }
                    (None, true, Some(p)) => {
                        self.boarded = Some(t);
                        self.window = Some((t, p, s.leader));
                        Step::Going
                    }
                    (Some(b), true, Some(p)) => {
                        let (t0, p0, l) = self.window.unwrap_or((t, p, s.leader));
                        if kind == Kind::Air && t - b >= AIR_UP_S && p[1] - s.below < AIR_UP_M && flat(p, leg.to) > NEAR_GOAL_M {
                            return Step::Failed(kind);
                        }
                        // (landed where it was going: the passengers are getting off)
                        if flat(p, leg.to) < NEAR_GOAL_M {
                            return Step::Going;
                        }
                        if flat(p, p0) >= MOVE_M || (p[1] - p0[1]).abs() >= MOVE_M {
                            self.window = Some((t, p, l));
                        } else if t - t0 >= WINDOW_S {
                            return Step::Failed(kind);
                        }
                        Step::Going
                    }
                }
            }
        }
    }
}

/// What [`choose`] knows of the squad and the side.
pub struct Ask<'a> {
    pub t: f64,
    /// where the leader is
    pub from: [f32; 3],
    pub flag: usize,
    pub f: &'a FlagSighting,
    /// the own or empty vehicles the side sees
    pub vehicles: &'a [VehicleSighting],
    /// the vehicles other squads' trips have taken
    pub taken: &'a [usize],
    /// what this squad has tried and failed
    pub failed: &'a [Failed],
    /// where enemies have been reported lately
    pub enemies: &'a [[f32; 3]],
    /// a flight is also taken when it beats the way by land by [`AIR_GAIN_S`] (on by default; `bots3:no-heli` takes it off); without it, only with no way
    pub air_if_faster: bool,
}

/// A boat that lands people: on the water, with the helm and a seat free, not a big ship that stays off the coast.
pub fn landing_boat(c: &VehicleSighting) -> bool {
    c.class == VehicleClass::Ship && c.health > 0.3 && c.seats.contains(&0) && c.seats.len() >= 2 && c.radius <= BIG_BOAT_M
}

/// An aircraft that carries people and lands: one that holds still in the air, with the pilot's seat and two more free.
pub fn transport_aircraft(c: &VehicleSighting) -> bool {
    c.class.hovers() && c.health > 0.5 && c.seats.contains(&0) && c.seats.len() >= 3
}

/// A plane that carries people: it does not hold still, so they jump over the point ([`End::Drop`]). With the pilot's seat and
/// another free. Not what floats: GC's speeder bikes are planes with two seats that hang a metre over the ground (counted, they
/// sent GC_Endor's squads after flags up in the trees, and bots3 against bots2 went from +16 to −1 there).
/// (An airship is not one: it holds still, so it lands, [`transport_aircraft`].)
pub fn drop_aircraft(c: &VehicleSighting) -> bool {
    c.class == VehicleClass::Plane && !c.floats && c.health > 0.5 && c.seats.contains(&0) && c.seats.len() >= 2
}

/// How long the squad takes by land to `to`: in the ground vehicle at hand if there is one (what [`super::squad::orders`] fills),
/// else on foot; the way is the map's ([`Land::way_m`]) or, unknown, [`DETOUR`] longer than the straight line.
pub fn ground_time(a: &Ask, to: [f32; 3], land: &dyn Land) -> f64 {
    // (the way on the map when it is known: DC_LostVillage's attackers walked ~1.5 km round a lake 200 m wide, an estimated 100 s, and
    // never took the helicopter that crosses it)
    let d = land.way_m(a.from, to, a.f.radio).map_or(DETOUR * flat(a.from, to), |w| w.max(flat(a.from, to)));
    let car = a.vehicles.iter().any(|c| c.ground_with_seats() && c.seats.contains(&0) && !a.taken.contains(&c.vehicle) && flat(c.pos, a.from) < CAR_NEAR_M);
    if car { CAR_BOARD_S + (d / CAR_SPEED) as f64 } else { (d / FOOT_SPEED) as f64 }
}

/// How long a flight takes: the walk to the aircraft, boarding, climbing and landing, and the transit in a straight line.
pub fn air_time(from: [f32; 3], aircraft: [f32; 3], to: [f32; 3]) -> f64 {
    (flat(from, aircraft) / WALK_SPEED) as f64 + HELI_OVERHEAD_S + (flat(aircraft, to) / HELI_SPEED) as f64
}

/// Where a helicopter lands to take the flag at `flag`, coming from `from`: on the ring [`LAND_NEAR_M`]–[`LAND_FAR_M`] around it, on
/// flat dry ground with nothing over it, the farther from the enemies reported the better, and on the side it comes from (it does
/// not fly over the flag to land). With no such spot, at the near edge of the ring on its side (the old landing).
pub fn landing_spot(flag: [f32; 3], from: [f32; 3], enemies: &[[f32; 3]], land: &dyn Land) -> [f32; 3] {
    let back = (from[0] - flag[0]).atan2(from[2] - flag[2]);
    let water = land.water();
    let at = |a: f32, r: f32| [flag[0] + r * a.sin(), 0.0, flag[2] + r * a.cos()];
    let mut best: Option<(f32, [f32; 3])> = None;
    for k in 0..16 {
        let a = back + k as f32 * std::f32::consts::TAU / 16.0;
        for r in [LAND_NEAR_M, 0.5 * (LAND_NEAR_M + LAND_FAR_M), LAND_FAR_M] {
            let p = at(a, r);
            let h = land.below(p[0], p[2]);
            if h <= water + 0.5 || h == f32::MIN {
                continue;
            }
            let around = [[LAND_SPAN_M, 0.0], [-LAND_SPAN_M, 0.0], [0.0, LAND_SPAN_M], [0.0, -LAND_SPAN_M]].map(|o| land.below(p[0] + o[0], p[2] + o[1]));
            if around.iter().any(|&g| (g - h).abs() > LAND_FLAT_M || g <= water + 0.5) {
                continue;
            }
            if !land.clear(p[0], p[2], 25.0) || [[LAND_SPAN_M, 0.0], [-LAND_SPAN_M, 0.0], [0.0, LAND_SPAN_M], [0.0, -LAND_SPAN_M]].iter().any(|o| !land.clear(p[0] + o[0], p[2] + o[1], 25.0)) {
                continue;
            }
            let p = [p[0], h, p[2]];
            // the side it comes from (0 to π), how far from the middle of the ring, and the enemies near
            let side = {
                let d = (a - back).rem_euclid(std::f32::consts::TAU);
                d.min(std::f32::consts::TAU - d)
            };
            let threat: f32 = enemies.iter().map(|e| (LAND_ENEMY_M - flat(*e, p)).max(0.0)).sum();
            let cost = 40.0 * side + (r - 0.5 * (LAND_NEAR_M + LAND_FAR_M)).abs() * 0.5 + 3.0 * threat;
            if best.is_none_or(|b| cost < b.0) {
                best = Some((cost, p));
            }
        }
    }
    best.map_or_else(|| { let p = at(back, LAND_NEAR_M); [p[0], flag[1], p[2]] }, |b| b.1)
}

/// The trip for a squad to a flag, or `None` when nothing serves (every way tried and failed: the team gives it another flag).
pub fn choose(a: &Ask, land: &dyn Land) -> Option<Trip> {
    let banned = |k: Kind| a.failed.iter().any(|f| f.flag == a.flag && f.kind == k && a.t < f.until);
    let to = a.f.pos;
    let at_hand = |ok: &dyn Fn(&VehicleSighting) -> bool| {
        a.vehicles.iter().filter(|c| ok(c) && !a.taken.contains(&c.vehicle) && flat(c.pos, a.from) < AT_HAND_M).min_by(|x, y| flat(x.pos, a.from).total_cmp(&flat(y.pos, a.from)).then(x.vehicle.cmp(&y.vehicle)))
    };
    let deadline = |c: &VehicleSighting| a.t + BOARD_S + (flat(c.pos, a.from) / WALK_SPEED) as f64;
    let ground = Leg { mode: Mode::Ground, to, end: End::Arrive, deadline: f64::INFINITY };
    let trip = |legs: Vec<Leg>| Trip { flag: a.flag, legs, leg: 0, since: a.t, boarded: None, window: None, missed: 0, moved: false };
    let by_air = |end: End| {
        if banned(Kind::Air) {
            return None;
        }
        // (a flag on the ground: beside it, where it can land; one up in the air, on it)
        let spot = |c: &VehicleSighting| if a.f.on_foot { landing_spot(to, c.pos, a.enemies, land) } else { to };
        at_hand(&transport_aircraft).map(|c| Leg { mode: Mode::Air(c.vehicle), to: spot(c), end, deadline: deadline(c) })
    };
    // (what does not land: over the flag, and they jump)
    let dropped = || if banned(Kind::Air) || a.f.radio < DROP_WIDE_M { None } else { at_hand(&drop_aircraft).map(|c| Leg { mode: Mode::Air(c.vehicle), to, end: End::Drop, deadline: deadline(c) }) };
    if !a.f.on_foot {
        // up in the air: only an aircraft that lands there (and, inside, on foot if there is a way)
        return by_air(End::Land).or_else(dropped).map(|l| trip(vec![l, ground]));
    }
    let reach = land.reach(a.from, to, a.f.radio);
    if reach != Reach::NoPath && !banned(Kind::Ground) {
        // with a way by land, a flight only if it arrives well sooner
        if a.air_if_faster
            && let Some(l) = by_air(End::Land)
            && let Some(c) = l.mode.vehicle().and_then(|v| a.vehicles.iter().find(|c| c.vehicle == v))
            && air_time(a.from, c.pos, to) + AIR_GAIN_S < ground_time(a, to, land)
        {
            return Some(trip(vec![l, ground]));
        }
        return Some(trip(vec![ground]));
    }
    // no way by land: across the water by boat, if not by air; and on foot as the last resort (swimming, the walker's crossing)
    if land.wet(a.from, to)
        && !banned(Kind::Boat)
        && let Some(c) = at_hand(&landing_boat)
    {
        return Some(trip(vec![Leg { mode: Mode::Boat(c.vehicle), to, end: End::Arrive, deadline: deadline(c) }, ground]));
    }
    if let Some(l) = by_air(End::Land) {
        return Some(trip(vec![l, ground]));
    }
    (!banned(Kind::Ground)).then(|| trip(vec![ground]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map split by a river along x = 100: two zones, water in between.
    struct River;
    impl Land for River {
        fn reach(&self, a: [f32; 3], b: [f32; 3], _: f32) -> Reach {
            if (a[0] < 100.0) == (b[0] < 100.0) { Reach::Path } else { Reach::NoPath }
        }
        fn wet(&self, a: [f32; 3], b: [f32; 3]) -> bool {
            (a[0] < 100.0) != (b[0] < 100.0)
        }
        fn below(&self, _: f32, _: f32) -> f32 {
            0.0
        }
    }

    fn flag(x: f32, on_foot: bool) -> FlagSighting {
        FlagSighting { pos: [x, 0.0, 0.0], owner: 2, takeable: true, radio: 10.0, on_foot }
    }

    fn veh(v: usize, class: VehicleClass, x: f32, seats: usize) -> VehicleSighting {
        VehicleSighting { vehicle: v, pos: [x, 0.0, 20.0], team: 1, free_seats: seats, class, health: 1.0, floats: false, seats: (0..seats).collect(), radius: 5.0, afloat: false, vtol: false }
    }

    fn ask<'a>(f: &'a FlagSighting, vehicles: &'a [VehicleSighting], failed: &'a [Failed]) -> Ask<'a> {
        Ask { t: 10.0, from: [0.0, 0.0, 0.0], flag: 0, f, vehicles, taken: &[], failed, enemies: &[], air_if_faster: false }
    }

    /// With a way by land the trip is one ground leg, whatever is at hand: what the squad did before (the gate of notes).
    #[test]
    fn with_a_way_by_land_one_ground_leg() {
        let f = flag(80.0, true);
        let v = [veh(7, VehicleClass::Ship, 30.0, 4), veh(8, VehicleClass::Helicopter, 30.0, 4)];
        let tr = choose(&ask(&f, &v, &[]), &River).unwrap();
        assert!(tr.plain());
        assert!(choose(&ask(&flag(500.0, true), &v, &[]), &Unknown).unwrap().plain(), "a map that does not know: as before");
    }

    /// Across the river: the boat at hand and then on foot; with no boat, the helicopter; with neither, on foot (swimming).
    #[test]
    fn across_the_water_boat_then_air_then_on_foot() {
        let f = flag(300.0, true);
        let v = [veh(7, VehicleClass::Ship, 30.0, 4), veh(8, VehicleClass::Helicopter, 30.0, 4)];
        let tr = choose(&ask(&f, &v, &[]), &River).unwrap();
        assert_eq!(tr.legs.iter().map(|l| l.mode).collect::<Vec<_>>(), vec![Mode::Boat(7), Mode::Ground]);
        assert!(tr.legs[0].deadline > 10.0 + BOARD_S, "the walk to the boat is added");
        let tr = choose(&ask(&f, &v[1..], &[]), &River).unwrap();
        assert_eq!(tr.legs[0].mode, Mode::Air(8));
        assert_eq!(tr.legs[0].end, End::Land);
        assert!(choose(&ask(&f, &[], &[]), &River).unwrap().plain());
        // a big ship does not land people; a boat far away is not at hand
        let mut big = veh(7, VehicleClass::Ship, 30.0, 4);
        big.radius = 60.0;
        assert!(choose(&ask(&f, &[big], &[]), &River).unwrap().plain());
        assert!(choose(&ask(&f, &[veh(7, VehicleClass::Ship, 900.0, 4)], &[]), &River).unwrap().plain());
    }

    /// A flag up in the air is reached only by an aircraft that lands; without one, nothing serves.
    #[test]
    fn a_flag_up_in_the_air_only_by_air() {
        let f = flag(80.0, false);
        let v = [veh(8, VehicleClass::Helicopter, 30.0, 4)];
        assert_eq!(choose(&ask(&f, &v, &[]), &River).unwrap().legs[0].mode, Mode::Air(8));
        assert_eq!(choose(&ask(&f, &[veh(8, VehicleClass::Tank, 30.0, 4)], &[]), &River), None);
    }

    /// B19: with no aircraft that lands, a plane with a seat to spare flies over a flag up in the air and its people jump; the one
    /// that lands goes first, and a flag on the ground is not jumped on.
    #[test]
    fn b19_a_flag_up_in_the_air_by_plane_jumping() {
        let f = FlagSighting { radio: 60.0, ..flag(80.0, false) };
        let plane = veh(8, VehicleClass::Plane, 30.0, 4);
        let tr = choose(&ask(&f, std::slice::from_ref(&plane), &[]), &River).unwrap();
        assert_eq!((tr.legs[0].mode, tr.legs[0].end, tr.legs[0].to), (Mode::Air(8), End::Drop, f.pos));
        assert_eq!(tr.legs[1].mode, Mode::Ground, "and on foot from where they come down");
        let both = [plane.clone(), veh(9, VehicleClass::Helicopter, 40.0, 4)];
        assert_eq!(choose(&ask(&f, &both, &[]), &River).unwrap().legs[0].end, End::Land);
        assert_eq!(choose(&ask(&f, &[veh(8, VehicleClass::Plane, 30.0, 1)], &[]), &River), None, "a single-seater carries nobody");
        assert_eq!(choose(&ask(&f, &[VehicleSighting { floats: true, ..plane.clone() }], &[]), &River), None, "what floats over the ground does not fly over a flag");
        let on_the_ground = flag(80.0, true);
        assert!(choose(&ask(&on_the_ground, std::slice::from_ref(&plane), &[]), &River).unwrap().legs.iter().all(|l| l.end != End::Drop));
        let narrow = FlagSighting { radio: 15.0, ..f };
        assert_eq!(choose(&ask(&narrow, std::slice::from_ref(&plane), &[]), &River), None, "nobody jumps at a flag a jump does not land in");
    }

    /// What failed is not tried again for `RETRY_S`: the next way, and with none left, nothing (another flag).
    #[test]
    fn what_failed_is_not_retried_for_a_while() {
        let f = flag(300.0, true);
        let v = [veh(7, VehicleClass::Ship, 30.0, 4)];
        let failed = [Failed { flag: 0, kind: Kind::Boat, until: 70.0 }];
        assert!(choose(&ask(&f, &v, &failed), &River).unwrap().plain());
        let failed = [Failed { flag: 0, kind: Kind::Boat, until: 70.0 }, Failed { flag: 0, kind: Kind::Ground, until: 70.0 }];
        assert_eq!(choose(&ask(&f, &v, &failed), &River), None);
        let mut a = ask(&f, &v, &failed);
        a.t = 71.0;
        assert_eq!(choose(&a, &River).unwrap().legs[0].mode, Mode::Boat(7), "a minute later it is tried again");
    }

    fn sample(t: f64, x: f32) -> Sample {
        Sample { t, leader: 1, leader_pos: [x, 0.0, 0.0], aboard: false, vehicle: None, fighting: false, boarding: false, below: 0.0 }
    }

    /// On foot: two windows without moving fail; moving, fighting or being near the flag do not.
    #[test]
    fn on_foot_two_windows_still_fail() {
        let mut tr = Trip::ground(0, [500.0, 0.0, 0.0], 0.0);
        assert_eq!(tr.check(&sample(0.0, 0.0)), Step::Going);
        assert_eq!(tr.check(&sample(16.0, 5.0)), Step::Going, "one window");
        assert_eq!(tr.check(&sample(32.0, 8.0)), Step::Failed(Kind::Ground));
        let mut tr = Trip::ground(0, [500.0, 0.0, 0.0], 0.0);
        for k in 0..20 {
            assert_eq!(tr.check(&sample(k as f64 * 4.0, k as f32 * 9.0)), Step::Going, "walking");
        }
        let mut tr = Trip::ground(0, [500.0, 0.0, 0.0], 0.0);
        for k in 0..20 {
            assert_eq!(tr.check(&Sample { fighting: true, ..sample(k as f64 * 4.0, 0.0) }), Step::Going, "fighting");
            assert_eq!(tr.check(&sample(k as f64 * 4.0, 450.0)), Step::Going, "at the flag");
        }
    }

    /// A boat leg: not boarded by the deadline fails; boarded and moving goes on; everyone off on the shore, the next leg.
    #[test]
    fn a_boat_leg_boards_moves_and_lands() {
        let f = flag(300.0, true);
        let v = [veh(7, VehicleClass::Ship, 30.0, 4)];
        let mut tr = choose(&ask(&f, &v, &[]), &River).unwrap();
        let deadline = tr.legs[0].deadline;
        let on = |t: f64, x: f32, aboard: bool| Sample { aboard, vehicle: Some([x, 0.0, 20.0]), ..sample(t, x) };
        // past the deadline: a window more, and it fails unless the leader gets nearer the boat
        let mut late = tr.clone();
        assert_eq!(late.check(&on(deadline + 1.0, 30.0, false)), Step::Going);
        assert_eq!(late.check(&on(deadline + 1.0 + WINDOW_S, 30.0, false)), Step::Failed(Kind::Boat));
        let mut late = tr.clone();
        let walking = |t: f64, x: f32| Sample { vehicle: Some([30.0, 0.0, 20.0]), ..sample(t, x) };
        assert_eq!(late.check(&walking(deadline + 1.0, -100.0)), Step::Going);
        assert_eq!(late.check(&walking(deadline + 1.0 + WINDOW_S, -60.0)), Step::Going, "walking to it");
        assert!(late.legs[0].deadline > deadline + WINDOW_S, "the deadline moves with it");
        assert_eq!(late.check(&walking(deadline + 1.0 + 3.0 * WINDOW_S, -58.0)), Step::Failed(Kind::Boat), "and stops");
        assert_eq!(tr.check(&on(20.0, 30.0, true)), Step::Going);
        assert_eq!(tr.check(&on(30.0, 70.0, true)), Step::Going);
        assert_eq!(tr.clone().check(&on(50.0, 75.0, true)), Step::Failed(Kind::Boat), "aboard and still for a window");
        assert_eq!(tr.check(&on(44.0, 160.0, true)), Step::Going);
        assert_eq!(tr.check(&on(52.0, 175.0, false)), Step::Going, "off on the shore");
        assert_eq!(tr.leg, 1);
        assert_eq!(tr.now().mode, Mode::Ground);
    }

    /// An aircraft leg that does not lift in `AIR_UP_S` fails (the transport helicopter that never took off, Basrahs).
    #[test]
    fn an_air_leg_that_does_not_lift_fails() {
        let f = flag(300.0, true);
        let v = [veh(8, VehicleClass::Helicopter, 30.0, 4)];
        let mut tr = choose(&ask(&f, &[], &[]), &River).unwrap();
        assert!(tr.plain());
        tr = choose(&ask(&f, &v, &[]), &River).unwrap();
        let on = |t: f64, y: f32, x: f32| Sample { aboard: true, vehicle: Some([x, y, 20.0]), ..sample(t, x) };
        assert_eq!(tr.check(&on(20.0, 1.0, 30.0)), Step::Going);
        assert_eq!(tr.clone().check(&on(52.0, 2.0, 31.0)), Step::Failed(Kind::Air));
        assert_eq!(tr.check(&on(30.0, 40.0, 30.0)), Step::Going, "up: the window starts again");
        assert_eq!(tr.check(&on(52.0, 60.0, 90.0)), Step::Going);
    }

    /// With a way by land, a flight only when it beats the land by `AIR_GAIN_S` (and only with `air_if_faster`): far on foot, yes;
    /// near, or with a car at hand for a middling distance, no.
    #[test]
    fn with_a_way_by_land_a_flight_only_if_much_faster() {
        let heli = veh(8, VehicleClass::Helicopter, 20.0, 4);
        let far = flag(2000.0, true);
        let v = [heli.clone()];
        let mut a = ask(&far, &v, &[]);
        assert!(choose(&a, &Flat).unwrap().plain(), "without the lever, by land as before");
        a.air_if_faster = true;
        let tr = choose(&a, &Flat).unwrap();
        assert_eq!(tr.legs[0].mode, Mode::Air(8));
        assert_eq!(tr.legs[1].to, far.pos, "and the last leg on foot to the flag");
        let near = flag(250.0, true);
        let mut a = ask(&near, &v, &[]);
        a.air_if_faster = true;
        assert!(choose(&a, &Flat).unwrap().plain(), "250 m: on foot");
        let mid = flag(500.0, true);
        let car = [heli, veh(9, VehicleClass::Car, 10.0, 4)];
        let mut a = ask(&mid, &car, &[]);
        a.air_if_faster = true;
        assert!(choose(&a, &Flat).unwrap().plain(), "500 m with a car at hand: the car");
    }

    /// Flat dry land everywhere except a pond west of x = -30 and a house over (40, 0).
    struct Flat;
    impl Land for Flat {
        fn reach(&self, _: [f32; 3], _: [f32; 3], _: f32) -> Reach {
            Reach::Path
        }
        fn wet(&self, _: [f32; 3], _: [f32; 3]) -> bool {
            false
        }
        fn below(&self, x: f32, _: f32) -> f32 {
            if x < -30.0 { -1.0 } else { 10.0 }
        }
        fn water(&self) -> f32 {
            0.0
        }
        fn clear(&self, x: f32, z: f32, _: f32) -> bool {
            (x - 40.0).hypot(z) > 15.0
        }
    }

    /// The landing spot is on the ring around the flag, never on it, on dry ground with nothing over it, on the side the aircraft
    /// comes from, and away from the enemies reported.
    #[test]
    fn the_landing_spot_is_beside_the_flag() {
        let f = [0.0, 10.0, 0.0];
        let ring = |p: [f32; 3]| flat(p, f) >= LAND_NEAR_M - 0.1 && flat(p, f) <= LAND_FAR_M + 0.1;
        // from the south (z < 0): on the south side
        let p = landing_spot(f, [0.0, 0.0, -1000.0], &[], &Flat);
        assert!(ring(p) && p[2] < 0.0, "{p:?}");
        assert_eq!(p[1], 10.0);
        // from the east the house is in the way: beside it, not on it
        let p = landing_spot(f, [1000.0, 0.0, 0.0], &[], &Flat);
        assert!(ring(p) && p[0] > 0.0 && (p[0] - 40.0).hypot(p[2]) > 15.0, "{p:?}");
        // from the west the pond: on dry ground
        let p = landing_spot(f, [-1000.0, 0.0, 0.0], &[], &Flat);
        assert!(ring(p) && p[0] > -30.0, "{p:?}");
        // enemies reported on the south side: elsewhere
        let enemies = [[0.0, 10.0, -60.0], [10.0, 10.0, -55.0]];
        let p = landing_spot(f, [0.0, 0.0, -1000.0], &enemies, &Flat);
        assert!(ring(p) && enemies.iter().all(|e| flat(*e, p) > 40.0), "{p:?}");
    }
}
