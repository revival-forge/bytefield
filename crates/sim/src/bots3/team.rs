//! A side's head: what a whole team decides, like the player who directs it over the radio. Which flag each one attacks or defends
//! (by its worth, how far it is and how many are already going), who goes with whom, who drives each vehicle and who rides as
//! passenger, where each one spawns and what mix of kits the side carries.
//!
//! There is one `Team` per side and round, shared by all its bots (`Arc<Mutex<Team>>`): the first to think each tick
//! updates it and each bot reads its [`Order`]. It only knows what a side would know of itself ([`TeamView`]: mates, flags and their
//! owner, vehicles) and, of the enemy, what its own tell it ([`Team::report`]: whom each one sees, with the time, like a
//! radio report). Never the position of an enemy nobody has seen.
//!
//! The split (every [`SPLIT_PERIOD`] s):
//! 1. **urgency**: an own flag the enemy is taking (one has been seen on top of it, or it has just gone neutral) sends
//!    the own units closer than [`URGENCY_M`], at once, even if alone or in a vehicle with empty seats;
//! 2. **defence**: an own flag with enemies seen nearby gets one or more defenders;
//! 3. **guard**: the own front flag keeps a few (never half the side);
//! 4. **attack**: the rest is spread among the takeable flags nearest the front, without bunching up.
//!
//! Then the **squads**: those going to the same flag, in groups of 3 to 5 with a leader; a vehicle with seats near
//! the squad is filled (driver and passengers) and the driver waits for its own.
use super::body::rng::Rng;
use super::squad::{self, Squad, Levers};
use super::trip::{self, Trip};
use crate::bot_api::{BotView, DeadView, VehicleClass};
use std::collections::{BTreeMap, BTreeSet};

/// How often the flags are split up, s.
pub const SPLIT_PERIOD: f64 = 4.0;
/// An own flag being taken calls those closer than this, m.
pub const URGENCY_M: f32 = 150.0;
/// An enemy seen closer than this to an own flag threatens it, m.
pub const THREAT_M: f32 = 60.0;
/// How long a report (a seen enemy) is worth, s.
pub const REPORT_S: f64 = 10.0;
/// A pilot goes to an enemy reported less than this ago, s, and closer than this to it, m.
pub const AIR_REPORT_S: f64 = 15.0;
pub const AIR_REPORT_RANGE: f32 = 1500.0;
/// Share of those left that guard the own front flag when nobody threatens it (never half).
pub const GUARD: f32 = 0.15;

/// Where to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goal {
    /// a flag of the map, by its number
    Flag(usize),
    /// a point of the map (x, z)
    Point([f32; 2]),
}

/// What it does on the side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// leads a squad (or goes alone) to the goal
    Leader,
    /// goes with the player
    Follow(usize),
    /// drives the vehicle
    Drive(usize),
    /// rides as passenger in the vehicle, in the seat
    Passenger(usize, usize),
    /// goes on its own: the goal is ignored (without a team, what it would do alone)
    Free,
}

/// What the team tells a player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Order {
    pub goal: Goal,
    pub role: Role,
    /// the kit it should spawn with
    pub kit: usize,
}

impl Order {
    /// No orders: goes free (the goal is ignored).
    pub fn free() -> Order {
        Order { goal: Goal::Point([0.0, 0.0]), role: Role::Free, kit: 0 }
    }
}

/// A living mate, as the side sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mate {
    pub player: usize,
    pub pos: [f32; 3],
    /// the vehicle it is in, if in one
    pub vehicle: Option<usize>,
}

/// A flag and whose it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlagSighting {
    pub pos: [f32; 3],
    /// 0: nobody's
    pub owner: u32,
    pub takeable: bool,
    pub radio: f32,
    /// taken on foot: from the ground or the water of its circle one gets within its radius ([`reachable_on_foot`]), or a teammate
    /// has been inside it (a roof with stairs, a platform one spawns on)
    pub on_foot: bool,
}

/// Whether a flag can be taken standing on the terrain (or swimming): some point of its circle, at the height of the ground or the
/// water, is within its radius, which is measured in 3D. GC_Mos_Eisley's high ones (20 and 53 m over the ground) are not, and
/// nobody takes them, bots3 or bots2: the team with only those to attack sent everyone there and left its own alone.
// TODO: a ladder is a way on foot that neither this nor the walker knows: DC_Al_Khafji_Docks' flag 2 is on a cargo ship's deck
// (y = 85, the dock at 78.5, the sea bed at 42) with a `ladder_10m_m1` up its side at (618, 652), no aircraft on the map, and no
// flag taken in 4 min. The soldier climbs (`crate::ladder`); a ladder leg would walk to its foot, face it and push forward.
pub fn reachable_on_foot(pos: [f32; 3], radio: f32, water: f32, ground: impl Fn(f32, f32) -> Option<f32>) -> bool {
    // (a soldier's position, a little over its feet: the control point counts with it)
    const ABOVE: f32 = 1.0;
    let step = (radio / 8.0).clamp(1.0, 3.0);
    let n = (radio / step).ceil() as i32;
    (-n..=n).any(|i| {
        (-n..=n).any(|k| {
            let (dx, dz) = (i as f32 * step, k as f32 * step);
            let Some(g) = ground(pos[0] + dx, pos[2] + dz) else { return false };
            let dy = g.max(water) + ABOVE - pos[1];
            dx * dx + dz * dz + dy * dy <= (radio - 0.5).max(0.0).powi(2)
        })
    })
}

/// A vehicle some mate has seen.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleSighting {
    pub vehicle: usize,
    pub pos: [f32; 3],
    pub team: u32,
    pub free_seats: usize,
    pub class: VehicleClass,
    pub health: f32,
    /// an aircraft that hangs in the air by itself (an airship)
    pub floats: bool,
    /// the free seats (0: the driver's)
    pub seats: Vec<usize>,
    /// its size, from its centre, m
    pub radius: f32,
    /// over the water (on a carrier's deck): nobody gets there on foot from land
    pub afloat: bool,
    /// a plane its throttle lifts straight up ([`crate::bot_api::Inside::lift`]): it takes off from where it stands
    pub vtol: bool,
}

impl VehicleSighting {
    /// One on the ground that can be driven with passengers.
    pub fn ground_with_seats(&self) -> bool {
        matches!(self.class, VehicleClass::Car | VehicleClass::Tank | VehicleClass::Walker | VehicleClass::Other) && self.health > 0.3 && self.seats.len() >= 2
    }

    /// A transport helicopter (with three free seats or more, the pilot's among them).
    pub fn transport_helicopter(&self) -> bool {
        self.class.hovers() && self.health > 0.5 && self.seats.len() >= 3
    }
}

/// What a side knows about itself at a moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamView {
    pub team: u32,
    pub mates: Vec<Mate>,
    pub flags: Vec<FlagSighting>,
    pub vehicles: Vec<VehicleSighting>,
}

impl TeamView {
    /// What the side of `v`'s player knows: itself and its mates (shown by the minimap), the flags and the own or
    /// empty vehicles (the enemy's, nothing).
    pub fn de(v: &BotView) -> TeamView {
        let me = &v.me;
        let vehicles: Vec<VehicleSighting> = v
            .vehicles()
            .iter()
            .filter(|c| c.team == 0 || c.team == me.team)
            .map(|c| VehicleSighting { vehicle: c.vehicle, pos: c.pos, team: c.team, free_seats: c.free_seats.len(), class: c.class, health: c.health, floats: c.floats, seats: c.free_seats.clone(), radius: c.radius, afloat: v.ground(c.pos[0], c.pos[2]).is_none_or(|g| g < v.water()), vtol: c.class == VehicleClass::Plane && v.lift(c.vehicle) != 0.0 })
            .collect();
        // (whoever is in a vehicle is where its vehicle is)
        let en = |p: [f32; 3]| vehicles.iter().find(|c| (0..3).all(|k| (c.pos[k] - p[k]).abs() < 0.01)).map(|c| c.vehicle);
        let mut mates = vec![Mate { player: me.player, pos: me.pos, vehicle: me.inside.as_ref().map(|i| i.vehicle) }];
        mates.extend(v.others().iter().filter(|o| o.team == me.team).map(|o| Mate { player: o.player, pos: o.pos, vehicle: if o.in_vehicle { en(o.pos).or(Some(usize::MAX)) } else { None } }));
        mates.sort_by_key(|c| c.player);
        TeamView { team: me.team, mates, flags: v.flags().iter().map(|f| FlagSighting { pos: f.pos, owner: f.team, takeable: f.takeable, radio: f.radius, on_foot: reachable_on_foot(f.pos, f.radius, v.water(), |x, z| v.ground(x, z)) }).collect(), vehicles }
    }
}

/// The parts of the head that are switched on (to measure what each contributes: `bots3:no-assign`...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// flag split; without it, each one chooses its flag
    pub split: bool,
    /// shared squads and vehicles
    pub squads: bool,
    /// spawn with the squad and the kit
    pub spawning: bool,
    /// a squad also flies with a way by land when the flight arrives [`trip::AIR_GAIN_S`] sooner ([`trip::choose`]); without it,
    /// only with no way by land (`bots3:no-heli`). On: against bots3 without it, 8 seeds of 8 min, Basrahs_Edge and Al_Nas +0 (the
    /// car wins there), LostVillage +5, Operation_Bragg +5
    pub heli: bool,
    /// map profile from what is seen on starting, in the DC and GC mods ([`super::mods`]); `bots3:no-mods` switches it off
    pub mods: bool,
    /// flags that cannot be taken on foot are not attack targets ([`reachable_on_foot`]); off, all are (`bots3:no-reach`)
    pub reach: bool,
    /// the side's aircraft that fight get a pilot ([`Team::wing`]): an idle bot near one takes it; off, only a squad's trip or a
    /// bot on its own takes one (`bots3:no-wing`)
    pub wing: bool,
    /// a pilot with bombs or rockets makes attack runs on a target the side gives it ([`Team::air_pass`]); off, it flies as before
    /// (`bots3:no-pass`)
    pub pass: bool,
}

impl Default for Parts {
    fn default() -> Parts {
        Parts { split: true, squads: true, spawning: true, heli: true, mods: true, reach: true, wing: true, pass: true }
    }
}

/// What each kit carries, from what is seen in the carrier's hands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Scout,
    Assault,
    AntiTank,
    Medic,
    Engineer,
}

impl Kind {
    /// By the names of what it carries.
    pub fn de(items: &[(usize, String)]) -> Kind {
        let n: String = items.iter().map(|i| i.1.to_ascii_lowercase()).collect::<Vec<_>>().join(" ");
        let hay = |ks: &[&str]| ks.iter().any(|k| n.contains(k));
        if hay(&["medikit", "medic", "medpack", "bacta"]) {
            Kind::Medic
        } else if hay(&["wrench", "repair", "pliers", "hydrospanner"]) {
            Kind::Engineer
        } else if hay(&["bazooka", "panzerschreck", "piat", "rpg", "at4", "law", "stinger", "smaw", "carl", "launcher", "rocket"]) {
            Kind::AntiTank
        } else if hay(&["sniper", "scope", "k98", "binocular", "kar98", "springfield", "enfield"]) {
            Kind::Scout
        } else {
            Kind::Assault
        }
    }
}

/// An enemy seen by a mate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Report {
    t: f64,
    player: usize,
    pos: [f32; 3],
    /// was in a vehicle
    vehicle: bool,
}

/// B26: an aircraft higher than this over what is under it is in flight, m.
pub const FLYING_M: f32 = 25.0;
/// B26: a fighter is sent after an enemy aircraft reported within this distance of it, m; at most this many fighters after the same one;
/// and one reported this near an own flag that is being taken, or a transport of the side with its people in, goes first, m.
pub const ENGAGE_RANGE: f32 = 3000.0;
pub const ENGAGE_MOST: usize = 2;
pub const ENGAGE_PROTECT_M: f32 = 500.0;
/// For how long after its trip on foot fails a leader is told to get out of where it is, s.
const STUCK_TOLD_S: f64 = 1.0;
/// B26: a fighter that has not asked for this long is down or back on the ground: its target is free again, s.
const ENGAGE_SILENT_S: f64 = 2.0;

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A side's head.
#[derive(Debug, Default)]
pub struct Team {
    /// the last instant it was updated
    last_one: Option<f64>,
    pub squads: Vec<Squad>,
    pub parts: Parts,
    /// the levers that depend on the map type (set by [`Team::set_levers`])
    pub levers: Levers,
    /// the levers forced by the variant (`bots3:guard=0`): they override the map's
    pub forced: Vec<(String, f32)>,
    /// for tests: one order for all
    pub fixed: Option<Order>,
    next: f64,
    orders: BTreeMap<usize, Order>,
    urgent_ones: BTreeSet<usize>,
    /// the leaders who take their own on foot to the goal: they do not leave alone in a vehicle
    accompanied_ones: BTreeSet<usize>,
    /// vehicle → the passengers it was given
    passengers: BTreeMap<usize, Vec<usize>>,
    reports: Vec<Report>,
    /// B26: the enemy aircraft seen in flight (by a pilot of the side, or by whoever was aiming at one), apart from the reports the
    /// ground war is decided with
    air: Vec<Report>,
    /// B26: the fighters in flight that are after an enemy aircraft: our pilot → (the enemy pilot, when ours last asked)
    engaged: BTreeMap<usize, (usize, f64)>,
    /// the leaders whose trip on foot failed for not advancing, and when: a new flag does not unstick a body
    stuck: BTreeMap<usize, f64>,
    owners: Vec<u32>,
    /// when each flag stopped being ours
    lost: Vec<Option<f64>>,
    /// the flags a teammate has been inside this round: they can be taken, if not from the ground
    footsteps: Vec<bool>,
    /// what each kit carries (learned from what its own carry) and each player's kit
    kit_kind: BTreeMap<usize, Kind>,
    kit_de: BTreeMap<usize, usize>,
    /// the side's own rng, seeded with the side
    rng: Option<Rng>,
    /// the casualty report (what the side hears over the radio: «so-and-so has been killed!» and where): when and where each one fell in
    /// the last [`DEATHS_S`]; and the mates alive at the last instant looked at, with where they were
    deaths: Vec<(f64, [f32; 3])>,
    alive_t: f64,
    alive: Vec<(usize, [f32; 3])>,
    /// the team trace (`BYTEFIELD_BOTS3_TRACE`): the first time it writes where each flag is
    last_trace: Option<f64>,
    /// B15: the sightings the team has said on the radio (message, where the enemy was, when), so one is said once
    said_spotted: Vec<(crate::radio::Msg, [f32; 3], f64)>,
    /// B15: a person at the wheel asked to get in: the bots that go, to which vehicle and seat
    get_ins: Vec<GetIn>,
    /// B25: a "medic!" heard: the medic sent to whoever called (caller, medic, until when)
    medic_calls: Vec<(usize, usize, f64)>,
    /// B23: the side's fighting aircraft and who was sent to fly each: vehicle → (pilot, until when it may take to get in)
    wing: BTreeMap<usize, (usize, f64)>,
    /// B23: the fighting aircraft left without a pilot at the last split (vehicle, where), and how many pilots the side may send
    wing_free: Vec<(usize, [f32; 3])>,
    wing_most: usize,
    /// B23: the attack-run target each pilot was given: pilot → (the enemy player, until when it holds it without asking)
    passes: BTreeMap<usize, (usize, f64)>,
}

/// B23: an attack run's target is looked for this far from the aircraft, m; one on foot only inside this distance of a flag of the
/// side being taken, m; a vehicle counts this much nearer than one on foot, and one another pilot has this much farther, m; a pilot
/// that has not asked for this long, s, leaves its target to the others.
pub const PASS_RANGE: f32 = 2500.0;
pub const PASS_FLAG_M: f32 = 40.0;
pub const PASS_FOOT_M: f32 = 500.0;
pub const PASS_TAKEN_M: f32 = 1500.0;
pub const PASS_SILENT_S: f64 = 5.0;
/// B23: over the water, what is reported this high over it is a ship (its centre), m; over land a vehicle is within 8 m of the ground.
pub const PASS_SHIP_UP_M: f32 = 25.0;
/// B23: a bot this near a free fighting aircraft of its side, on foot and with nothing more urgent, is sent to fly it, m.
pub const WING_NEAR_M: f32 = 300.0;
/// B23: on a deck (the aircraft over the water), only one this near: on the same ship, m.
pub const WING_DECK_M: f32 = 60.0;
/// B23: where the path map does not say whether it gets there on foot, only one within this height of the aircraft, m.
pub const WING_STEP_M: f32 = 10.0;
/// B23: a plane standing this high over the ground, not on a ship, is up on a platform: none is sent to it, m.
pub const WING_RAISED_M: f32 = 8.0;
/// B23: and it has this long to get in, s, plus the walk at this speed, m/s; one sent from the spawn screen, this much more.
pub const WING_BOARD_S: f64 = 20.0;
pub const WING_WALK: f32 = 3.5;
pub const WING_SPAWN_S: f64 = 20.0;
/// B23: at most one pilot sent per this many bots of the side.
pub const WING_SHARE: usize = 4;

/// B15: the bots a person's "get in" put in its vehicle: the vehicle, until when the order lasts unless they are in, and who goes in
/// which seat.
#[derive(Clone, Debug, PartialEq)]
pub struct GetIn {
    pub vehicle: usize,
    pub until: f64,
    pub seats: Vec<(usize, usize)>,
    /// those that have got in
    pub boarded: Vec<usize>,
}

/// B25: a medic sent to a caller keeps the order this long, s, unless the caller is healed or gone first.
pub const MEDIC_CALL_S: f64 = 25.0;
/// B15: a "get in" lasts this, s, for a bot that has not got in yet (and while it rides the vehicle).
pub const GET_IN_S: f64 = 25.0;
/// B15: a sighting heard from a person goes into the team's reports as one enemy with this player number plus the speaker's.
const HEARD_ENEMY: usize = 1 << 20;

/// the casualty report remembers this, s; two casualties closer than [`CLUSTER_M`] to each other make a dangerous place.
pub const DEATHS_S: f64 = 20.0;
pub const CLUSTER_M: f32 = 10.0;

impl Team {
    /// With some parts switched off.
    pub fn con(parts: Parts) -> Team {
        Team { parts, ..Default::default() }
    }

    /// The map type's levers, with the forced ones on top.
    pub fn set_levers(&mut self, p: Levers) {
        self.levers = p;
        for (k, v) in &self.forced {
            self.levers.force(k, *v);
        }
    }

    /// Whether it is due to update at `t`: once per instant.
    pub fn due(&mut self, t: f64) -> bool {
        let due = self.last_one != Some(t);
        self.last_one = Some(t);
        due
    }

    /// Notes the side's casualties (the mates that were alive at the previous instant and no longer are), once per instant.
    pub fn casualty_report(&mut self, v: &BotView) {
        if self.alive_t == v.time {
            return;
        }
        let me = &v.me;
        let mut now: Vec<(usize, [f32; 3])> = v.others().into_iter().filter(|o| o.team == me.team).map(|o| (o.player, o.pos)).collect();
        now.push((me.player, me.pos));
        // (a gap of more than a tick is another round or a pause: not casualties)
        if v.time - self.alive_t < 0.1 {
            let fallen: Vec<[f32; 3]> = self.alive.iter().filter(|x| !now.iter().any(|y| y.0 == x.0)).map(|x| x.1).collect();
            for p in fallen {
                self.aim_low(v.time, p);
            }
        }
        self.alive = now;
        self.alive_t = v.time;
    }

    /// A casualty of the side at `pos`, at `t` (forgets those older than [`DEATHS_S`]).
    pub fn aim_low(&mut self, t: f64, pos: [f32; 3]) {
        self.deaths.push((t, pos));
        self.deaths.retain(|m| t - m.0 < DEATHS_S);
    }

    /// The places where two or more of its own have fallen in the last [`DEATHS_S`] (the centre of each cluster; four at most).
    pub fn dangers(&self, t: f64) -> [Option<[f32; 2]>; 4] {
        let mut out: [Option<[f32; 2]>; 4] = [None; 4];
        let mut n = 0;
        let recent: Vec<&(f64, [f32; 3])> = self.deaths.iter().filter(|m| t - m.0 < DEATHS_S).collect();
        for (i, a) in recent.iter().enumerate() {
            let near: Vec<&&(f64, [f32; 3])> = recent.iter().filter(|b| flat(a.1, b.1) < CLUSTER_M).collect();
            if near.len() < 2 || recent[..i].iter().any(|b| flat(a.1, b.1) < CLUSTER_M) {
                continue;
            }
            let k = near.len() as f32;
            let c = [near.iter().map(|b| b.1[0]).sum::<f32>() / k, near.iter().map(|b| b.1[2]).sum::<f32>() / k];
            if n < 4 && !out[..n].iter().flatten().any(|o| (o[0] - c[0]).hypot(o[1] - c[1]) < CLUSTER_M) {
                out[n] = Some(c);
                n += 1;
            }
        }
        out
    }

    /// Whether it is due to split up again (and the side's view is needed).
    pub fn split_due(&self, t: f64) -> bool {
        self.fixed.is_none() && self.parts.split && t >= self.next
    }

    /// What a mate reports on its tick: its kit and what it carries, and the enemy it has in sight.
    pub fn report(&mut self, v: &BotView, target: Option<(usize, [f32; 3])>) {
        let me = &v.me;
        self.kit_de.insert(me.player, me.kit);
        if !me.items.is_empty() && !self.kit_kind.contains_key(&me.kit) {
            self.kit_kind.insert(me.kit, Kind::de(&me.items));
        }
        if let Some((j, pos)) = target {
            let other = v.others().into_iter().find(|o| o.player == j && o.in_vehicle);
            let vehicle = other.is_some();
            let aircraft = other.and_then(|o| o.vehicle).and_then(|k| v.vehicle(k)).is_some_and(|x| x.class.flies());
            if aircraft && pos[1] - v.ground(pos[0], pos[2]).unwrap_or(f32::MIN).max(v.water()) > FLYING_M {
                self.air_seen(v.time, j, pos);
            }
            let p = Report { t: v.time, player: j, pos, vehicle };
            match self.reports.iter_mut().find(|x| x.player == j) {
                Some(x) => *x = p,
                None => self.reports.push(p),
            }
        }
    }

    /// B26: an enemy aircraft someone of the side has in sight, in flight: who flies it and where.
    pub fn air_seen(&mut self, t: f64, player: usize, pos: [f32; 3]) {
        let p = Report { t, player, pos, vehicle: true };
        match self.air.iter_mut().find(|x| x.player == player) {
            Some(x) => *x = p,
            None => self.air.push(p),
        }
    }

    /// B26: the fighter in flight `me` has fixed on that enemy pilot's aircraft by itself (the nearest it saw, or the one on its tail):
    /// the side notes it, so that the others go for another.
    pub fn air_locked(&mut self, t: f64, me: usize, enemy: usize) {
        self.engaged.insert(me, (enemy, t));
    }

    /// B26: the enemy aircraft the fighter in flight `me`, at `pos`, is sent after: who flies it and where it was last reported. The one
    /// it already had while it is still reported (a target is for the whole engagement); if not, of the enemy aircraft seen in
    /// flight in the last [`AIR_REPORT_S`] within [`ENGAGE_RANGE`], first the one that threatens what the side protects (`flags`:
    /// its flags that are being taken; and its transports with people in), then the one with fewer fighters after it (never more than
    /// [`ENGAGE_MOST`]), then the nearest. It is what the radio would tell a fighter pilot: «enemy aircraft over such a flag, yours».
    pub fn engage(&mut self, t: f64, me: usize, pos: [f32; 3], flags: &[[f32; 3]]) -> Option<(usize, [f32; 3])> {
        self.engaged.retain(|k, e| *k == me || t - e.1 < ENGAGE_SILENT_S);
        self.air.retain(|p| t - p.t < AIR_REPORT_S);
        if let Some(e) = self.engaged.get_mut(&me) {
            if let Some(p) = self.air.iter().find(|p| p.player == e.0) {
                e.1 = t;
                return Some((p.player, p.pos));
            }
            self.engaged.remove(&me);
        }
        // (a transport of the side with its people in is where they are)
        let carried: Vec<[f32; 3]> = self.passengers.values().flatten().filter_map(|j| self.alive.iter().find(|x| x.0 == *j)).map(|x| x.1).collect();
        let protects = |p: &Report| flags.iter().chain(&carried).any(|q| flat(*q, p.pos) < ENGAGE_PROTECT_M);
        let after = |p: &Report| self.engaged.values().filter(|e| e.0 == p.player).count();
        let p = self
            .air
            .iter()
            .filter(|p| flat(p.pos, pos) < ENGAGE_RANGE && after(p) < ENGAGE_MOST)
            .min_by(|a, b| protects(b).cmp(&protects(a)).then(after(a).cmp(&after(b))).then(flat(a.pos, pos).total_cmp(&flat(b.pos, pos))).then(a.player.cmp(&b.player)))?;
        let chosen = (p.player, p.pos);
        self.engaged.insert(me, (chosen.0, t));
        Some(chosen)
    }

    /// B23: the target of the attack runs of the pilot `me`, at `pos`, in an aircraft with bombs or rockets: an enemy vehicle on land
    /// or a ship (`surface`: `Some(afloat)` for a spot at the ground or the water, `None` for one in the air) the side reported in the
    /// last [`AIR_REPORT_S`] within [`PASS_RANGE`], or an enemy on foot inside a flag of the side being taken (`taking`). The one it
    /// had while it is still reported (a target is for the whole run); if not, the nearest, a vehicle counting [`PASS_FOOT_M`] nearer
    /// than one on foot and one another pilot already has [`PASS_TAKEN_M`] farther, unless it is a ship. It is what the radio would
    /// tell a pilot: «enemy tank at such a place, yours». The player and where it was reported.
    pub fn air_pass(&mut self, t: f64, me: usize, pos: [f32; 3], taking: &[[f32; 3]], surface: impl Fn([f32; 3]) -> Option<bool>) -> Option<(usize, [f32; 3])> {
        self.passes.retain(|k, e| *k == me || t - e.1 < PASS_SILENT_S);
        let live = |p: &&Report| t - p.t < AIR_REPORT_S && surface(p.pos).is_some() && (p.vehicle || taking.iter().any(|f| flat(*f, p.pos) < PASS_FLAG_M));
        if let Some(e) = self.passes.get_mut(&me) {
            if let Some(p) = self.reports.iter().filter(live).find(|p| p.player == e.0) {
                e.1 = t;
                return Some((p.player, p.pos));
            }
            self.passes.remove(&me);
        }
        let taken = |p: &Report| self.passes.values().any(|e| e.0 == p.player) && surface(p.pos) != Some(true);
        let cost = |p: &Report| flat(p.pos, pos) - if p.vehicle { PASS_FOOT_M } else { 0.0 } + if taken(p) { PASS_TAKEN_M } else { 0.0 };
        let p = self.reports.iter().filter(live).filter(|p| flat(p.pos, pos) < PASS_RANGE).min_by(|a, b| cost(a).total_cmp(&cost(b)).then(a.player.cmp(&b.player)))?;
        let chosen = (p.player, p.pos);
        self.passes.insert(me, (chosen.0, t));
        Some(chosen)
    }

    /// B26: the enemy pilots that the side's fighters other than `me` are after (`usize::MAX`, a gap).
    pub fn air_taken(&self, me: usize) -> [usize; 6] {
        let mut out = [usize::MAX; 6];
        for (slot, e) in out.iter_mut().zip(self.engaged.iter().filter(|(k, _)| **k != me).map(|(_, e)| e.0)) {
            *slot = e;
        }
        out
    }

    /// Decides with what the side knows at instant `t`, on a map it knows nothing of (every trip is one ground leg).
    pub fn think(&mut self, t: f64, view: &TeamView) {
        self.think_on(t, view, &trip::Unknown)
    }

    /// Decides with what the side knows at instant `t`, looking at the map (`land`) for the squads' trips.
    pub fn think_on(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        self.next = t + SPLIT_PERIOD;
        self.rng.get_or_insert_with(|| Rng::new(0xB0_75_3E_00 ^ view.team as u64));
        self.reports.retain(|p| t - p.t < 3.0 * REPORT_S);
        let fl = &view.flags;
        if self.owners.len() != fl.len() {
            self.owners = fl.iter().map(|f| f.owner).collect();
            self.lost = vec![None; fl.len()];
            self.footsteps = vec![false; fl.len()];
        }
        // TODO: a flag up in the air on the deck the side stands on is not reached on foot until a mate has been in it: on GC_Judicator
        // every flag is inside the star destroyer (y = 341, ground ~150) and team 2 spawns 75 m from the neutral flag 3, but stands
        // still (86 % at its spawn at 60 s). A straight line of floor does not join them (the hangar between is open below, nothing
        // down to y = 180; the way is round by the halls), and the walker, with the flat path map, would not find it either: it needs
        // a path map per deck (`navgen` at the deck's height?). See `examples/obstacles_near data/game GCMOD GC_Judicator 540 890 60`.
        for (i, f) in fl.iter().enumerate() {
            let inside = |p: [f32; 3]| (0..3).map(|k| (p[k] - f.pos[k]).powi(2)).sum::<f32>() <= f.radio * f.radio;
            self.footsteps[i] |= view.mates.iter().any(|c| inside(c.pos));
        }
        let with_footsteps;
        let view = if fl.iter().zip(&self.footsteps).any(|(f, &p)| (p || !self.parts.reach) && !f.on_foot) {
            let mut v = view.clone();
            for (f, &p) in v.flags.iter_mut().zip(&self.footsteps) {
                f.on_foot |= p || !self.parts.reach;
            }
            with_footsteps = v;
            &with_footsteps
        } else {
            view
        };
        let fl = &view.flags;
        for (i, f) in fl.iter().enumerate() {
            if self.owners[i] == view.team && f.owner != view.team {
                self.lost[i] = Some(t);
            }
            self.owners[i] = f.owner;
        }
        self.urgent_ones.clear();
        self.accompanied_ones.clear();
        self.passengers.clear();
        let previous = |o: Option<&Order>| o.and_then(|o| if let Goal::Flag(i) = o.goal { Some(i) } else { None });
        if self.parts.squads {
            squad::keep(&mut self.squads, view, self.levers.size);
            let pos = |j: usize| view.mates.iter().find(|c| c.player == j).map(|c| c.pos);
            let units: Vec<Unit> = self
                .squads
                .iter()
                .map(|e| Unit { pos: e.alive(view).into_iter().filter_map(pos).collect(), previous: if let Goal::Flag(i) = e.goal { Some(i) } else { None }, avoid: e.avoid.iter().filter(|a| t < a.1).map(|a| a.0).collect() })
                .collect();
            let destinations = split_up(t, view, &self.reports, &self.lost, &units, &self.levers);
            for (e, d) in self.squads.iter_mut().zip(destinations) {
                let Some(d) = d else { continue };
                // (in the air on a plane its people jump from, it stays with its flag while it is there to take: given another
                // every few seconds, it gets to none)
                let flying_to = e.trip.as_ref().filter(|tr| tr.now().end == trip::End::Drop).filter(|tr| {
                    let v = tr.now().mode.vehicle();
                    v.is_some() && e.all().any(|j| view.mates.iter().any(|c| c.player == j && c.vehicle == v)) && fl.get(tr.flag).is_some_and(|f| f.takeable && f.owner != view.team)
                });
                if let Some(tr) = flying_to {
                    e.goal = Goal::Flag(tr.flag);
                    continue;
                }
                e.goal = Goal::Flag(d.flag);
                if d.urgent {
                    self.urgent_ones.extend(e.alive(view));
                }
            }
            self.trips(t, view, land);
            for e in &self.squads {
                let Some(jp) = view.mates.iter().find(|c| c.player == e.leader && c.vehicle.is_none()).map(|c| c.pos) else { continue };
                if !self.urgent_ones.contains(&e.leader) && e.members.iter().any(|&m| view.mates.iter().any(|c| c.player == m && c.vehicle.is_none() && flat(c.pos, jp) < squad::TOGETHER_M)) {
                    self.accompanied_ones.insert(e.leader);
                }
            }
            let (orders, passengers) = squad::orders(&mut self.squads, view, t, &self.levers);
            self.orders.extend(orders);
            self.passengers = passengers;
            if self.parts.wing {
                self.wing(t, view, land);
            }
        } else {
            let units: Vec<Unit> = view.mates.iter().map(|c| Unit { pos: vec![c.pos], previous: previous(self.orders.get(&c.player)), avoid: Vec::new() }).collect();
            let destinations = split_up(t, view, &self.reports, &self.lost, &units, &self.levers);
            for (c, d) in view.mates.iter().zip(destinations) {
                let Some(d) = d else { continue };
                self.orders.insert(c.player, Order { goal: Goal::Flag(d.flag), role: Role::Leader, kit: 0 });
                if d.urgent {
                    self.urgent_ones.insert(c.player);
                }
            }
        }
        for (j, o) in self.orders.iter_mut() {
            o.kit = self.kit_de.get(j).copied().unwrap_or(0);
        }
        if super::trace_team() {
            let role = |o: &Order| format!("{:?}", o.role);
            if self.last_trace.is_none() {
                eprintln!("t={t:.0} team {} flags {:?}", view.team, fl.iter().map(|f| ([f.pos[0] as i32, f.pos[2] as i32], f.takeable, f.on_foot)).collect::<Vec<_>>());
                eprintln!("t={t:.0} team {} vehicles {:?}", view.team, view.vehicles.iter().map(|c| (c.vehicle, c.class, c.seats.len(), [c.pos[0] as i32, c.pos[1] as i32, c.pos[2] as i32])).collect::<Vec<_>>());
            }
            self.last_trace = Some(t);
            eprintln!("t={t:.0} team {} alive {} owners {:?} sq {:?} pas {:?}", view.team, view.mates.len(), self.owners, self.squads.iter().map(|e| (e.leader, e.members.len(), e.goal)).collect::<Vec<_>>(), self.passengers);
            for e in self.squads.iter().filter(|e| e.trip.as_ref().is_some_and(|tr| !tr.plain()) || !e.avoid.is_empty()) {
                let to = e.trip.as_ref().map(|tr| tr.now().to.map(|x| x as i32));
                let veh = e.leg_vehicle().and_then(|v| view.vehicles.iter().find(|c| c.vehicle == v)).map(|c| c.pos.map(|x| x as i32));
                eprintln!("   trip {} {:?} leg {:?} to {to:?} vehicle at {veh:?} boarded {:?} avoid {:?}", e.leader, e.trip.as_ref().map(|tr| tr.legs.iter().map(|l| l.mode).collect::<Vec<_>>()), e.trip.as_ref().map(|tr| tr.leg), e.trip.as_ref().and_then(|tr| tr.boarded), e.avoid);
            }
            for pl in self.squads.iter().flat_map(|e| e.planes.iter()) {
                eprintln!("   plan {} {:?} driver {} passengers {:?} since {:.0}", pl.vehicle, view.vehicles.iter().find(|c| c.vehicle == pl.vehicle).map(|c| c.class), pl.driver, pl.passengers, pl.since);
            }
            for c in &view.mates {
                eprintln!("   {} {:?} veh {:?} {}", c.player, c.pos.map(|x| x as i32), c.vehicle, self.orders.get(&c.player).map_or("-".into(), role));
            }
        }
    }

    /// Each squad's trip to its flag ([`trip`]): chosen when the flag changes, upgraded from a plain ground leg when the map shows
    /// no way by land, and tested for progress; what fails is not tried again for a while, and with no way left the squad avoids that
    /// flag in the next splits.
    fn trips(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        let avoid = |e: &mut Squad, f: usize, t: f64| {
            e.avoid.retain(|a| a.0 != f);
            e.avoid.push((f, t + trip::RETRY_S));
        };
        let mate = |j: usize| view.mates.iter().find(|c| c.player == j);
        let reports = &self.reports;
        let enemies: Vec<[f32; 3]> = reports.iter().filter(|r| t - r.t < REPORT_S).map(|r| r.pos).collect();
        let air_if_faster = self.parts.heli;
        for k in 0..self.squads.len() {
            let taken: Vec<usize> = self.squads.iter().enumerate().filter(|&(i, _)| i != k).filter_map(|(_, e)| e.leg_vehicle()).collect();
            let e = &mut self.squads[k];
            e.failed.retain(|f| t < f.until);
            e.avoid.retain(|a| t < a.1);
            let (Goal::Flag(f), Some(l)) = (e.goal, mate(e.leader)) else {
                e.trip = None;
                continue;
            };
            let Some(fs) = view.flags.get(f) else { continue };
            let on_foot = l.vehicle.is_none();
            // (in a vehicle already, it goes on by land; the boats and aircraft are chosen on foot)
            let choose = |failed: &[trip::Failed]| {
                let ask = trip::Ask { t, from: l.pos, flag: f, f: fs, vehicles: &view.vehicles, taken: &taken, failed, enemies: &enemies, air_if_faster };
                if on_foot { trip::choose(&ask, land) } else if failed.iter().any(|x| x.flag == f && x.kind == trip::Kind::Ground) { None } else { Some(Trip::ground(f, fs.pos, t)) }
            };
            // (on a plane its people jump from, already aboard, and the side gives it another flag: the same flight, over the new one;
            // a plane does not go back for a new plan)
            let dropping = e.trip.as_ref().is_some_and(|tr| {
                let leg = tr.now();
                tr.flag != f && leg.end == trip::End::Drop && leg.mode.vehicle().is_some_and(|v| e.all().any(|j| mate(j).is_some_and(|c| c.vehicle == Some(v))))
            });
            if dropping && let Some(tr) = e.trip.as_mut() {
                tr.flag = f;
                for leg in tr.legs.iter_mut().skip(tr.leg) {
                    leg.to = fs.pos;
                }
            }
            match &e.trip {
                Some(tr) if tr.flag == f && !(tr.plain() && on_foot) => {}
                Some(tr) if tr.flag == f => {
                    // a plain ground leg that the map now says has no way (the squad respawned across the water): the new trip
                    if let Some(n) = choose(&e.failed).filter(|n| !n.plain()) {
                        e.trip = Some(n);
                    }
                }
                _ => e.trip = choose(&e.failed),
            }
            let Some(leg) = e.trip.as_ref().map(|tr| tr.now()) else {
                avoid(e, f, t);
                continue;
            };
            let vehicle = leg.mode.vehicle().and_then(|v| view.vehicles.iter().find(|c| c.vehicle == v)).filter(|c| c.health > 0.0).map(|c| c.pos);
            let aboard = leg.mode.vehicle().is_some_and(|v| e.all().any(|j| mate(j).is_some_and(|c| c.vehicle == Some(v))));
            let fighting = reports.iter().any(|r| t - r.t < REPORT_S && flat(r.pos, l.pos) < trip::FIGHT_M);
            let sample = trip::Sample { t, leader: e.leader, leader_pos: l.pos, aboard, vehicle, fighting, boarding: !e.planes.is_empty(), below: vehicle.map_or(f32::MIN, |p| land.below(p[0], p[2])) };
            let tr = e.trip.as_mut().unwrap();
            // in the air towards a landing spot with enemies at it: another, once (on the flag's ground, not on a platform)
            if let (trip::Mode::Air(_), Some(p), true) = (leg.mode, vehicle, aboard)
                && leg.end == trip::End::Land
                && !tr.moved
                && fs.on_foot
                && flat(p, leg.to) < trip::LAND_RETHINK_M
                && enemies.iter().any(|q| flat(*q, leg.to) < 0.5 * trip::LAND_ENEMY_M)
            {
                tr.moved = true;
                let to = trip::landing_spot(fs.pos, p, &enemies, land);
                tr.legs[tr.leg].to = to;
                if super::trace_team() {
                    eprintln!("t={t:.0} team {} squad {} lands elsewhere: {:?} -> {:?}", view.team, e.leader, leg.to.map(|x| x as i32), to.map(|x| x as i32));
                }
            }
            if let trip::Step::Failed(kind) = tr.check(&sample) {
                if super::trace_team() {
                    eprintln!("t={t:.0} team {} squad {} trip to {f} failed: {:?} leg {} of {:?}", view.team, e.leader, kind, tr.leg, tr.legs.iter().map(|l| l.mode).collect::<Vec<_>>());
                }
                if kind == trip::Kind::Ground && on_foot {
                    self.stuck.insert(e.leader, t);
                }
                let gone = leg.mode.vehicle();
                e.planes.retain(|p| Some(p.vehicle) != gone);
                e.failed.push(trip::Failed { flag: f, kind, until: t + trip::RETRY_S });
                e.trip = choose(&e.failed);
                if e.trip.is_none() {
                    avoid(e, f, t);
                }
            }
        }
    }

    /// B23: the side's fighting aircraft get a pilot, as players run to the planes at the start: each free one (a plane, or a
    /// helicopter that does not carry a squad), with the pilot's seat free and nobody's trip, goes to the nearest bot on foot within
    /// [`WING_NEAR_M`] that only follows its leader (not a leader, a driver or one with an urgent order), one pilot per [`WING_SHARE`]
    /// bots of the side at most. It keeps it while it flies it; the order to get in lasts [`WING_BOARD_S`]. Once in, it flies as
    /// any pilot of the side (to the target the side reports, else from flag to flag).
    fn wing(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        let mate = |j: usize| view.mates.iter().find(|c| c.player == j);
        let legs: BTreeSet<usize> = self.squads.iter().filter_map(|e| e.leg_vehicle()).collect();
        let planned: BTreeSet<usize> = self.squads.iter().flat_map(|e| e.planes.iter().map(|p| p.vehicle)).collect();
        // what was given and still holds: the pilot alive and in it, or on its way in time and the seat still free
        // (one sent from the spawn screen is not alive yet)
        let kits = &self.kit_de;
        self.wing.retain(|v, (j, until)| {
            let Some(c) = mate(*j) else { return t < *until && kits.contains_key(j) };
            let craft = view.vehicles.iter().find(|x| x.vehicle == *v);
            c.vehicle == Some(*v) || (c.vehicle.is_none() && t < *until && craft.is_some_and(|x| x.seats.contains(&0) && x.health > 0.3))
        });
        let bots = view.mates.iter().filter(|c| self.is_bot(c.player)).count();
        let most = bots.div_ceil(WING_SHARE);
        let fights = |x: &VehicleSighting| (x.class == VehicleClass::Plane || (x.class == VehicleClass::Helicopter && !x.transport_helicopter())) && !x.floats;
        // (a plane up on something that is not a ship does not take off with the bots' pilot: the runway logic of [`Pilot::fly`]
        // looks at the terrain, not at what it stands on; one that lifts straight up does: GC_Bespin's cloud cars on the platforms)
        let raised = |x: &VehicleSighting| {
            let under = land.below(x.pos[0], x.pos[2]);
            x.class == VehicleClass::Plane && !x.vtol && !x.afloat && under > f32::MIN && x.pos[1] - under > WING_RAISED_M
        };
        let mut free: Vec<&VehicleSighting> = view.vehicles.iter().filter(|x| fights(x) && !raised(x) && x.seats.contains(&0) && x.health > 0.5 && !legs.contains(&x.vehicle) && !planned.contains(&x.vehicle) && !self.wing.contains_key(&x.vehicle)).collect();
        free.sort_by_key(|x| x.vehicle);
        self.wing_most = most;
        self.wing_free.clear();
        for x in free {
            if self.wing.len() >= most {
                break;
            }
            let pilot = view
                .mates
                .iter()
                .filter(|c| c.vehicle.is_none() && self.is_bot(c.player) && flat(c.pos, x.pos) < if x.afloat { WING_DECK_M } else { WING_NEAR_M } && !self.urgent_ones.contains(&c.player))
                .filter(|c| matches!(self.orders.get(&c.player).map(|o| o.role), Some(Role::Follow(_))) && !self.wing.values().any(|w| w.0 == c.player))
                // (one it can walk to: with a way on the path map, or, where the map does not say, at about its height; on GC_Bespin's
                // platforms they walked off the edge towards an aircraft on another)
                .filter(|c| match land.reach(c.pos, x.pos, x.radius + 4.0) {
                    trip::Reach::Path => true,
                    trip::Reach::NoPath => false,
                    _ => (c.pos[1] - x.pos[1]).abs() < WING_STEP_M,
                })
                .min_by(|a, b| flat(a.pos, x.pos).total_cmp(&flat(b.pos, x.pos)).then(a.player.cmp(&b.player)));
            if super::trace_team() {
                let near: Vec<(usize, i32, Option<Role>)> = view.mates.iter().filter(|c| c.vehicle.is_none()).map(|c| (c.player, flat(c.pos, x.pos) as i32, self.orders.get(&c.player).map(|o| o.role))).filter(|m| m.1 < 2 * WING_NEAR_M as i32).collect();
                eprintln!("t={t:.0} team {} wing: aircraft {} {:?} at {:?} -> {:?}; on foot near {near:?}", view.team, x.vehicle, x.class, x.pos.map(|q| q as i32), pilot.map(|c| c.player));
            }
            match pilot {
                Some(c) => {
                    self.wing.insert(x.vehicle, (c.player, t + WING_BOARD_S + (flat(c.pos, x.pos) / WING_WALK) as f64));
                }
                None => self.wing_free.push((x.vehicle, x.pos)),
            }
        }
        for (&v, &(j, _)) in &self.wing {
            if let Some(o) = self.orders.get_mut(&j) {
                o.role = Role::Drive(v);
            }
        }
    }

    /// B23: on the spawn screen, the aircraft `d`'s player is sent to fly (the one it had, or a free one beside a group it can spawn at,
    /// if the side may send another pilot and it only follows its leader): where it is.
    fn wing_spawn(&mut self, d: &DeadView, role: Role) -> Option<[f32; 3]> {
        if !self.parts.wing {
            return None;
        }
        let at = |v: usize| d.vehicles.iter().find(|x| x.vehicle == v).map(|x| x.pos);
        if let Some((&v, _)) = self.wing.iter().find(|(_, w)| w.0 == d.player) {
            return at(v);
        }
        if !matches!(role, Role::Follow(_)) || self.wing.len() >= self.wing_most {
            return None;
        }
        // (an empty aircraft is nobody's: the enemy's carrier's too, which none of its groups is near)
        let near_group = |p: [f32; 3]| d.groups.iter().any(|g| g.active && flat(g.pos, p) < WING_NEAR_M);
        let (v, pos) = self.wing_free.iter().copied().find(|&(v, p)| !self.wing.contains_key(&v) && near_group(p))?;
        self.wing.insert(v, (d.player, d.time + WING_BOARD_S + WING_SPAWN_S + (WING_NEAR_M / WING_WALK) as f64));
        Some(pos)
    }

    /// Whether `player` has just been found not advancing on foot (its squad's trip by land failed): it should get out of where it is.
    pub fn stuck_on_foot(&self, player: usize, t: f64) -> bool {
        self.stuck.get(&player).is_some_and(|t0| t - t0 < STUCK_TOLD_S)
    }

    /// B23: the aircraft `player` was sent to fly ([`Team::wing`]): its patience to board counts from reaching it, as a leg's.
    pub fn wing_vehicle(&self, player: usize) -> Option<usize> {
        self.wing.iter().find(|(_, w)| w.0 == player).map(|(&v, _)| v)
    }

    /// The vehicle of the trip's leg of `player`'s squad (a boat or an aircraft it walks to: its patience to board counts from
    /// reaching it).
    pub fn leg_vehicle(&self, player: usize) -> Option<usize> {
        self.squads.iter().find(|e| e.all().any(|j| j == player)).and_then(|e| e.leg_vehicle())
    }

    /// Where the aircraft of the air leg of `player`'s squad lands ([`trip::landing_spot`]), while that leg lasts.
    pub fn leg_landing(&self, player: usize) -> Option<[f32; 3]> {
        let e = self.squads.iter().find(|e| e.all().any(|j| j == player))?;
        let l = e.trip.as_ref()?.now();
        (matches!(l.mode, trip::Mode::Air(_)) && l.end == trip::End::Land).then_some(l.to)
    }

    /// Where the people of the air leg of `player`'s squad jump ([`trip::End::Drop`]), while that leg lasts.
    pub fn leg_drop(&self, player: usize) -> Option<[f32; 3]> {
        let e = self.squads.iter().find(|e| e.all().any(|j| j == player))?;
        let l = e.trip.as_ref()?.now();
        (matches!(l.mode, trip::Mode::Air(_)) && l.end == trip::End::Drop).then_some(l.to)
    }

    /// What is due to `player`.
    pub fn order(&self, player: usize) -> Order {
        if let Some(o) = self.fixed {
            return o;
        }
        let o = self.orders.get(&player).copied().unwrap_or_else(Order::free);
        match self.get_in_seat(player) {
            // B15: a person's "get in" puts it in that vehicle, the rest of its order as it was
            Some((j, s)) if o.role != Role::Free => Order { role: Role::Passenger(j, s), ..o },
            _ => o,
        }
    }

    /// Whether `player`'s is urgent.
    pub fn accompanied(&self, player: usize) -> bool {
        self.accompanied_ones.contains(&player)
    }

    /// Whether `player`'s is urgent.
    pub fn urgent(&self, player: usize) -> bool {
        self.urgent_ones.contains(&player)
    }

    /// What `player` carries according to its kit (what is seen in its hands: the medic, its medkit; the engineer, its wrench), if known.
    /// It is what anyone on the side knows of a mate: the radio says it («I'm a medic»), the icon shows it.
    pub fn kind(&self, player: usize) -> Option<Kind> {
        self.kit_de.get(&player).and_then(|k| self.kit_kind.get(k)).copied()
    }

    /// The vehicles the side has given a squad and that `player` must not take on its own (they are not its own).
    pub fn reserved(&self, player: usize) -> [usize; 8] {
        let mut out = [usize::MAX; 8];
        let planes = self.squads.iter().flat_map(|e| e.planes.iter()).filter(|p| p.driver != player && !p.passengers.iter().any(|x| x.0 == player));
        for (k, p) in planes.take(8).enumerate() {
            out[k] = p.vehicle;
        }
        out
    }

    /// If `player` is a squad leader, how near `pos` the nearest of its own going on foot is (according to `others`).
    pub fn escort(&self, player: usize, pos: [f32; 3], others: &[crate::bot_api::Other]) -> Option<f32> {
        let e = self.squads.iter().find(|e| e.leader == player)?;
        others.iter().filter(|o| !o.in_vehicle && e.members.contains(&o.player)).map(|o| flat(o.pos, pos)).min_by(|a, b| a.total_cmp(b))
    }

    /// B15: whether `player` is a bots3 bot of this team (one that tells the team its kit), not a person.
    pub fn is_bot(&self, player: usize) -> bool {
        self.kit_de.contains_key(&player)
    }

    /// B15: whether the team has not said a sighting of this kind (`m`) near `pos` in the last [`super::radio::SPOT_AGAIN_S`].
    pub fn spotting_new(&self, m: crate::radio::Msg, pos: [f32; 3], t: f64) -> bool {
        !self.said_spotted.iter().any(|s| s.0 == m && t - s.2 < super::radio::SPOT_AGAIN_S && flat(s.1, pos) < super::radio::SPOT_AGAIN_M)
    }

    /// B15: the team said (or heard said) a sighting of this kind near `pos` at `t`.
    pub fn spotting_said(&mut self, m: crate::radio::Msg, pos: [f32; 3], t: f64) {
        self.said_spotted.retain(|s| t - s.2 < super::radio::SPOT_AGAIN_S);
        if !self.said_spotted.iter().any(|s| s.0 == m && (s.2 - t).abs() < 0.5 && flat(s.1, pos) < 1.0) {
            self.said_spotted.push((m, pos, t));
        }
    }

    /// B15: a person (`speaker`, not a bot of the team) said it saw an enemy in a vehicle, at `t`, being at `at`: the team takes it as
    /// a report of an enemy there, as one of its bots seeing it (the pilots go to it, the flag near it is threatened).
    pub fn heard_spotted(&mut self, speaker: usize, at: [f32; 3], t: f64) {
        let p = Report { t, player: HEARD_ENEMY + speaker, pos: at, vehicle: true };
        match self.reports.iter_mut().find(|x| x.player == p.player) {
            Some(x) if x.t < t => *x = p,
            Some(_) => {}
            None => self.reports.push(p),
        }
    }

    /// B15: a person at the wheel of `vehicle` asked to get in, at `t`: the bots on foot in `candidates` (nearest first) take its free
    /// passenger seats (`seats`), unless a "get in" for that vehicle is going on.
    pub fn ask_get_in(&mut self, t: f64, vehicle: usize, seats: &[usize], candidates: &[usize]) -> bool {
        self.get_ins.retain(|g| t < g.until);
        if self.get_ins.iter().any(|g| g.vehicle == vehicle) {
            return false;
        }
        let busy = |j: usize| self.get_ins.iter().any(|g| g.seats.iter().any(|s| s.0 == j));
        let seats: Vec<(usize, usize)> = candidates.iter().copied().filter(|&j| !busy(j)).zip(seats.iter().copied().filter(|&s| s != 0)).collect();
        if seats.is_empty() {
            return false;
        }
        self.get_ins.push(GetIn { vehicle, until: t + GET_IN_S, seats, boarded: Vec::new() });
        true
    }

    /// B25: `caller` shouted "medic!" at `t`: sends the first of `medics` (nearest to the caller first, already without the ones under fire,
    /// in an urgent task or on a call) that is not already on another call, unless one is on its way to `caller`. False if none goes.
    pub fn ask_medic(&mut self, t: f64, caller: usize, medics: &[usize]) -> bool {
        self.medic_calls.retain(|c| t < c.2);
        if self.medic_calls.iter().any(|c| c.0 == caller) {
            return false;
        }
        let Some(&m) = medics.iter().find(|&&m| m != caller && !self.medic_calls.iter().any(|c| c.1 == m)) else { return false };
        self.medic_calls.push((caller, m, t + MEDIC_CALL_S));
        true
    }

    /// B25: the caller `medic` was sent to, if the order still stands at `t`.
    pub fn medic_call(&self, medic: usize, t: f64) -> Option<usize> {
        self.medic_calls.iter().find(|c| c.1 == medic && t < c.2).map(|c| c.0)
    }

    /// B25: the call of `caller` is over (healed, or it died): its medic is free.
    pub fn medic_call_done(&mut self, caller: usize) {
        self.medic_calls.retain(|c| c.0 != caller);
    }

    /// B15: who a "get in" for `vehicle` put in which seat.
    pub fn get_in_seat_all(&self, vehicle: usize) -> Vec<(usize, usize)> {
        self.get_ins.iter().filter(|g| g.vehicle == vehicle).flat_map(|g| g.seats.iter().copied()).collect()
    }

    /// B15: the seat a "get in" gave `player`: vehicle and seat.
    pub fn get_in_seat(&self, player: usize) -> Option<(usize, usize)> {
        self.get_ins.iter().find_map(|g| g.seats.iter().find(|s| s.0 == player).map(|s| (g.vehicle, s.1)))
    }

    /// B15: whether `player`, given a seat by a "get in", rides that vehicle now, at `t`: the order lasts while it does; once it has
    /// got in and out again, its seat goes.
    pub fn get_in_riding(&mut self, player: usize, t: f64, riding: bool) {
        for g in self.get_ins.iter_mut() {
            if !g.seats.iter().any(|s| s.0 == player) {
                continue;
            }
            if riding {
                g.until = g.until.max(t + 1.0);
                if !g.boarded.contains(&player) {
                    g.boarded.push(player);
                }
            } else if g.boarded.contains(&player) {
                g.seats.retain(|s| s.0 != player);
            }
        }
        self.get_ins.retain(|g| t < g.until && !g.seats.is_empty());
    }

    /// The passengers given to the vehicle.
    pub fn passengers(&self, vehicle: usize) -> &[usize] {
        self.passengers.get(&vehicle).map_or(&[], |v| &v[..])
    }

    /// The target for whoever flies at `since`: the enemy vehicle on land (an armoured vehicle, a car; `on_land` says whether a spot
    /// is firm land and at ground level: not a boat or another plane, which move too much to go where they were seen) that the side has
    /// reported in the last [`AIR_REPORT_S`], the nearest within [`AIR_REPORT_RANGE`]. It is what the radio would tell
    /// a pilot: «enemy tank at such a place».
    pub fn air_target2(&self, t: f64, since: [f32; 3], on_land: impl Fn([f32; 3]) -> bool) -> Option<[f32; 3]> {
        self.reports
            .iter()
            .filter(|p| p.vehicle && t - p.t < AIR_REPORT_S && flat(p.pos, since) < AIR_REPORT_RANGE && on_land(p.pos))
            .min_by(|a, b| flat(a.pos, since).total_cmp(&flat(b.pos, since)).then(a.player.cmp(&b.player)))
            .map(|p| p.pos)
    }

    /// Enemy vehicles seen in the last minute.
    fn armoured_seen(&self, t: f64) -> usize {
        self.reports.iter().filter(|p| p.vehicle && t - p.t < 60.0).count()
    }

    /// On the dead screen: the group nearest the living leader of its squad (or its goal) and the kit the squad
    /// lacks (a medic and an engineer per squad; against armour, anti-tank). `None`: as usual.
    pub fn spawning(&mut self, d: &DeadView) -> Option<(Option<i64>, usize)> {
        if !self.parts.spawning {
            return None;
        }
        let current = *self.kit_de.get(&d.player)?;
        let order = self.order(d.player);
        let wing_at = self.wing_spawn(d, order.role);
        let sq = self.squads.iter().find(|e| e.leader == d.player || e.members.contains(&d.player));
        // where to: the living leader (or the living squad mate nearest its goal), else the goal
        let goal = match order.goal {
            Goal::Flag(i) => d.flags.get(i).map(|f| f.pos),
            Goal::Point(p) if order.role != Role::Free => Some([p[0], 0.0, p[1]]),
            Goal::Point(_) => None,
        };
        let leader = sq.and_then(|e| std::iter::once(e.leader).chain(e.members.iter().copied()).filter(|&j| j != d.player).find_map(|j| d.mates.iter().find(|m| m.player == j).map(|m| m.pos)));
        let mut group = leader.or(goal).and_then(|a| d.groups.iter().filter(|g| g.active && !g.on_vehicle).min_by(|x, y| flat(x.pos, a).total_cmp(&flat(y.pos, a)).then(x.id.cmp(&y.id))).map(|g| g.id));
        // B23: a fighting aircraft of the side without a pilot: one who only follows spawns beside it (on a carrier too) and flies it
        if let Some(at) = wing_at {
            if let Some(g) = d.groups.iter().filter(|g| g.active && flat(g.pos, at) < WING_NEAR_M).min_by(|x, y| flat(x.pos, at).total_cmp(&flat(y.pos, at)).then(x.id.cmp(&y.id))) {
                group = Some(g.id);
            }
        }
        // which kit: what the squad lacks
        let kit_of = |j: usize| self.kit_de.get(&j).and_then(|k| self.kit_kind.get(k)).copied();
        let alive: Vec<usize> = sq.map_or(Vec::new(), |e| std::iter::once(e.leader).chain(e.members.iter().copied()).filter(|&j| j != d.player && d.mates.iter().any(|m| m.player == j)).collect());
        let roles: Vec<Kind> = alive.iter().filter_map(|&j| kit_of(j)).collect();
        let mine = self.kit_kind.get(&current).copied();
        let wants = if sq.is_none() {
            None
        } else if !roles.contains(&Kind::Medic) {
            Some(Kind::Medic)
        } else if !roles.contains(&Kind::Engineer) && alive.len() >= 2 {
            Some(Kind::Engineer)
        } else if self.armoured_seen(d.time) >= 2 && roles.iter().filter(|r| **r == Kind::AntiTank).count() * 3 < alive.len() + 1 {
            Some(Kind::AntiTank)
        } else if mine.is_some_and(|r| roles.contains(&r) && matches!(r, Kind::Medic | Kind::Engineer)) {
            Some(Kind::Assault)
        } else {
            None
        };
        let kit = wants.and_then(|r| self.kit_kind.iter().find(|(_, x)| **x == r).map(|(k, _)| *k)).filter(|&k| k < d.kits).unwrap_or(current);
        Some((group, kit))
    }
}

/// Where a unit goes (a squad, or a loose player) after the split.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    pub flag: usize,
    pub urgent: bool,
}

/// What is split: a squad or a loose player, with its living members.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    /// where its living members are (the first, the leader)
    pub pos: Vec<[f32; 3]>,
    /// the flag it had (kept if it is still worth it: it does not change its mind every few seconds)
    pub previous: Option<usize>,
    /// the flags it found no way to ([`super::trip`]): it attacks another if there is one
    pub avoid: Vec<usize>,
}

/// The flag split among units (see the module). Deterministic: same view, same split.
pub fn split_up(t: f64, view: &TeamView, reports: &[Report], lost: &[Option<f64>], units: &[Unit], levers: &Levers) -> Vec<Option<Destination>> {
    let mine = view.team;
    let fl = &view.flags;
    let mut out: Vec<Option<Destination>> = vec![None; units.len()];
    if fl.is_empty() {
        return out;
    }
    let weight = |u: usize| units[u].pos.len();
    let total: usize = (0..units.len()).map(weight).sum();
    let seen_ones = |i: usize, r: f32, s: f64| reports.iter().filter(|p| t - p.t < s && flat(p.pos, fl[i].pos) < r).count();
    // TODO: with no flag that changes hands (DC_No_Fly_Zone_Day2 and DC_Weapon_Bunkers: both flags are bases) the squads are sent to
    // the enemy's base and fail the trip there (Day2: 2.5 km of desert, 30–38 % standing on foot at 8 min); a player goes where the
    // enemy is (the side's reports) or takes an aircraft. How: `BYTEFIELD_BOTS3_TRACE=1 tournament3 … --maps
    // DC_Final:DC_No_Fly_Zone_Day2 --seeds 1 --minutes 4 --threads 1` (`sq` and `failed` lines).
    let takeable_ones: Vec<usize> = (0..fl.len()).filter(|&i| fl[i].takeable).collect();
    let own: Vec<usize> = takeable_ones.iter().copied().filter(|&i| fl[i].owner == mine).collect();
    // (those that cannot be taken on foot are no attack target: going there wastes the people; the enemy's still mark the front)
    // (unless none is, and the side has an aircraft that lands people: a trip by air reaches them, [`super::trip`]. Not before: in
    // DC_Sea_Rigs four of five flags are on platforms and there are two helicopters; with all five attackable the squads with no
    // aircraft went round the platforms and nobody took the one on land)
    let by_air = view.vehicles.iter().any(|c| super::trip::transport_aircraft(c) || super::trip::drop_aircraft(c));
    let on_foot_ones: Vec<usize> = takeable_ones.iter().copied().filter(|&i| fl[i].owner != mine && fl[i].on_foot).collect();
    let attackable: Vec<usize> = if on_foot_ones.is_empty() && by_air { takeable_ones.iter().copied().filter(|&i| fl[i].owner != mine).collect() } else { on_foot_ones };
    let enemy_ones: Vec<usize> = (0..fl.len()).filter(|&k| fl[k].owner != mine && fl[k].owner != 0).collect();
    // how near a unit is: the leader's to go, any of its own's for an urgency
    let d = |u: usize, i: usize| units[u].pos.first().map_or(f32::MAX, |p| flat(*p, fl[i].pos));
    let d_min = |u: usize, i: usize| units[u].pos.iter().map(|p| flat(*p, fl[i].pos)).fold(f32::MAX, f32::min);
    let free_ones = |out: &[Option<Destination>]| (0..units.len()).filter(|&u| out[u].is_none() && weight(u) > 0).collect::<Vec<usize>>();
    let people = |out: &[Option<Destination>], i: usize| (0..units.len()).filter(|&u| out[u].is_some_and(|x| x.flag == i)).map(weight).sum::<usize>();
    // 1. urgency: the own flag being taken (enemy on top, or just lost and still neutral): the nearest, at once
    for &i in &takeable_ones {
        let on_top = seen_ones(i, fl[i].radio + 15.0, 6.0);
        let being_taken = (fl[i].owner == mine && on_top > 0) || (fl[i].owner == 0 && lost.get(i).copied().flatten().is_some_and(|p| t - p < 25.0));
        if !being_taken {
            continue;
        }
        let mut near: Vec<usize> = free_ones(&out).into_iter().filter(|&u| d_min(u, i) < URGENCY_M).collect();
        near.sort_by(|&a, &b| d_min(a, i).total_cmp(&d_min(b, i)).then(a.cmp(&b)));
        for u in near {
            if people(&out, i) >= (1 + on_top).min(3) {
                break;
            }
            out[u] = Some(Destination { flag: i, urgent: true });
        }
    }
    // 2. defence: the own flag with enemies seen nearby; first whoever was already defending it
    for &i in &own {
        let n = seen_ones(i, THREAT_M, REPORT_S);
        if n == 0 {
            continue;
        }
        let mut near: Vec<usize> = free_ones(&out).into_iter().filter(|&u| d(u, i) < 400.0).collect();
        near.sort_by(|&a, &b| (units[a].previous != Some(i)).cmp(&(units[b].previous != Some(i))).then(d(a, i).total_cmp(&d(b, i))).then(a.cmp(&b)));
        for u in near {
            if people(&out, i) >= n.min(3) {
                break;
            }
            out[u] = Some(Destination { flag: i, urgent: false });
        }
    }
    let mut left = free_ones(&out);
    if left.is_empty() {
        return out;
    }
    let remoteness = |i: usize| attackable.iter().map(|&k| flat(fl[i].pos, fl[k].pos)).fold(f32::MAX, f32::min);
    // where to go, with what share of the people: the front ones when there is nothing to attack, else the takeable ones nearest ours
    let (targets, pesos): (Vec<usize>, Vec<f32>) = if attackable.is_empty() {
        // (the front: our flags nearest one of the enemy's, takeable or not; that is where whoever comes for ours comes from)
        // With the enemy holding flags we cannot reach on foot (GC_Mos_Eisley, team 1) the front is under pressure from vehicles and
        // aircraft: cover every flag we own, an equal share each (a decaying share left the third flag empty and it fell at 90 s).
        // Otherwise (Berlin and the like: nothing of theirs is out of reach, they just hold nothing to take) the front-weighted spread stays.
        let to_the_enemy = |i: usize| enemy_ones.iter().map(|&k| flat(fl[i].pos, fl[k].pos)).fold(f32::MAX, f32::min);
        let mut b = own.clone();
        b.sort_by(|&x, &y| to_the_enemy(x).total_cmp(&to_the_enemy(y)).then(x.cmp(&y)));
        let pressed = takeable_ones.iter().any(|&k| fl[k].owner != mine && fl[k].owner != 0 && !fl[k].on_foot);
        b.truncate(if pressed { 4 } else { 3 });
        let w: Vec<f32> = (0..b.len()).map(|r| if pressed { 1.0 } else { 1.0 / (r as f32 + 1.0) }).collect();
        (b, w)
    } else {
        // 3. guard: the own front flag keeps a small unit (never half the side)
        if let Some(&front) = own.iter().min_by(|&&a, &&b| remoteness(a).total_cmp(&remoteness(b)).then(a.cmp(&b)))
            && levers.guard
            && total >= 6
            && people(&out, front) == 0
        {
            let fits = |u: usize| (weight(u) as f32) <= (total as f32 * 0.3).max(1.0);
            let g = left.iter().copied().filter(|&u| fits(u) && d(u, front) < 250.0).min_by(|&a, &b| (units[a].previous != Some(front)).cmp(&(units[b].previous != Some(front))).then(d(a, front).total_cmp(&d(b, front))).then(a.cmp(&b)));
            if let Some(u) = g {
                out[u] = Some(Destination { flag: front, urgent: false });
                left.retain(|&x| x != u);
            }
        }
        // 4. attack
        let anchor: Vec<[f32; 3]> = if own.is_empty() { left.iter().filter_map(|&u| units[u].pos.first().copied()).collect() } else { own.iter().map(|&i| fl[i].pos).collect() };
        let near_the_side = |i: usize| anchor.iter().map(|&p| flat(fl[i].pos, p)).fold(f32::MAX, f32::min);
        let mut b = attackable.clone();
        b.sort_by(|&x, &y| near_the_side(x).total_cmp(&near_the_side(y)).then(x.cmp(&y)));
        let people_q: usize = left.iter().map(|&u| weight(u)).sum();
        b.truncate(if levers.targets > 0 { levers.targets } else if people_q >= 12 && left.len() >= 3 { 3 } else { 2 });
        let w: Vec<f32> = match b.len() {
            1 => vec![1.0],
            2 => vec![0.6, 0.4],
            3 => vec![0.45, 0.35, 0.2],
            n => (0..n).map(|r| 1.0 / (r as f32 + 1.0)).collect(),
        };
        (b, w)
    };
    // 5. nothing to take and nothing of ours to lose (two bases that do not change hands: the round is won by kills): to the enemy's
    // flags that are reached on foot, the nearest first. Waiting at the base is not playing the map.
    let (targets, pesos) = if targets.is_empty() {
        let since: Vec<[f32; 3]> = left.iter().filter_map(|&u| units[u].pos.first().copied()).collect();
        let near_us = |i: usize| since.iter().map(|&p| flat(fl[i].pos, p)).fold(f32::MAX, f32::min);
        let mut b: Vec<usize> = enemy_ones.iter().copied().filter(|&i| fl[i].on_foot).collect();
        b.sort_by(|&x, &y| near_us(x).total_cmp(&near_us(y)).then(x.cmp(&y)));
        b.truncate(2);
        let w = if b.len() == 2 { vec![0.6, 0.4] } else { vec![1.0] };
        (b, w)
    } else {
        (targets, pesos)
    };
    if targets.is_empty() {
        return out;
    }
    let sum: f32 = pesos.iter().sum();
    let people_q: usize = left.iter().map(|&u| weight(u)).sum();
    let quota: Vec<f32> = pesos.iter().map(|p| (p / sum * people_q as f32).max(1.0)).collect();
    let mut how_many = vec![0usize; targets.len()];
    // whoever was already going to one of them keeps going (if it fits); then the nearest choose first
    let near_target = |u: usize| targets.iter().map(|&i| d(u, i)).fold(f32::MAX, f32::min);
    left.sort_by(|&a, &b| units[a].previous.is_none_or(|p| !targets.contains(&p)).cmp(&units[b].previous.is_none_or(|p| !targets.contains(&p))).then(near_target(a).total_cmp(&near_target(b))).then(a.cmp(&b)));
    for u in left {
        let avoided = |r: usize| if units[u].avoid.contains(&targets[r]) { 1.0e6 } else { 0.0 };
        let cost = |r: usize| d(u, targets[r]) + 500.0 * ((how_many[r] + weight(u)) as f32 / quota[r] - 1.0).max(0.0) - if units[u].previous == Some(targets[r]) { 150.0 } else { 0.0 } + avoided(r);
        let r = (0..targets.len()).min_by(|&a, &b| cost(a).total_cmp(&cost(b)).then(a.cmp(&b))).unwrap();
        how_many[r] += weight(u);
        out[u] = Some(Destination { flag: targets[r], urgent: false });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f32, owner: u32) -> FlagSighting {
        FlagSighting { pos: [x, 0.0, 0.0], owner, takeable: true, radio: 10.0, on_foot: true }
    }
    fn c(j: usize, x: f32) -> Mate {
        Mate { player: j, pos: [x, 0.0, 5.0], vehicle: None }
    }

    /// With a calm own flag and two to take, almost all attack; no own flag without an enemy keeps half.
    #[test]
    fn b01_the_side_spreads_out_and_does_not_bunch_up_on_its_own() {
        let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(300.0, 2), b(500.0, 0), b(900.0, 2)], vehicles: vec![] };
        let u: Vec<Unit> = (0..16).map(|j| Unit { pos: vec![c(j, j as f32).pos], previous: None, avoid: Vec::new() }).collect();
        let r = split_up(10.0, &view, &[], &[None; 4], &u, &Levers::default());
        assert!(r.iter().all(|x| x.is_some()));
        let n = |i| r.iter().filter(|d| d.unwrap().flag == i).count();
        assert!(n(0) <= 8, "{}", n(0));
        assert!(n(1) >= 4 && n(2) >= 3, "{} {}", n(1), n(2));
        assert!(n(3) < n(2) && n(2) < n(1), "more to the nearer ones: {} {} {}", n(1), n(2), n(3));
    }

    /// Without the guard lever, the own front flag keeps nobody if nobody threatens it (all attack).
    #[test]
    fn without_guard_all_attack() {
        let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(300.0, 2), b(500.0, 0)], vehicles: vec![] };
        let u: Vec<Unit> = (0..4).map(|j| Unit { pos: (0..4).map(|k| c(4 * j + k, 10.0).pos).collect(), previous: None, avoid: Vec::new() }).collect();
        let to_own = |p: &Levers| split_up(10.0, &view, &[], &[None; 3], &u, p).iter().filter(|d| d.unwrap().flag == 0).count();
        assert_eq!(to_own(&Levers::default()), 1, "with guard, one squad");
        let mut p = Levers::default();
        assert!(p.force("guard", 0.0) && !p.force("no_existe", 1.0));
        assert_eq!(to_own(&p), 0);
    }

    /// The pilot goes to the enemy vehicle on land the side reported a short while ago; not to the soldier, nor the boat, nor the old report.
    #[test]
    fn b11_the_pilot_goes_to_the_reported_armour() {
        let mut e = Team::default();
        e.reports = vec![
            Report { t: 9.0, player: 40, pos: [100.0, 0.0, 0.0], vehicle: false },
            Report { t: 9.0, player: 41, pos: [200.0, 0.0, 0.0], vehicle: true },
            Report { t: 9.0, player: 42, pos: [150.0, -5.0, 0.0], vehicle: true },
            Report { t: -20.0, player: 43, pos: [120.0, 0.0, 0.0], vehicle: true },
        ];
        // (whatever is below the water is a boat)
        let b = e.air_target2(10.0, [0.0, 100.0, 0.0], |p| p[1] >= 0.0);
        assert_eq!(b, Some([200.0, 0.0, 0.0]));
        assert_eq!(e.air_target2(30.0, [0.0, 100.0, 0.0], |p| p[1] >= 0.0), None, "old reports");
    }

    /// B23: each attack pilot gets its own enemy vehicle (one on foot only inside a flag being taken), keeps it while it is reported,
    /// and two pilots share only a ship.
    #[test]
    fn b23_the_attack_pilots_share_out_the_targets() {
        let mut e = Team::default();
        // (y < 0: a ship; y > 50: in the air)
        let surface = |p: [f32; 3]| (p[1] < 50.0).then_some(p[1] < 0.0);
        e.reports = vec![
            Report { t: 9.0, player: 40, pos: [300.0, 0.0, 0.0], vehicle: true },
            Report { t: 9.0, player: 41, pos: [600.0, 0.0, 0.0], vehicle: true },
            Report { t: 9.0, player: 42, pos: [100.0, 0.0, 0.0], vehicle: false },
            Report { t: 9.0, player: 43, pos: [150.0, 200.0, 0.0], vehicle: true },
        ];
        let here = [0.0, 150.0, 0.0];
        assert_eq!(e.air_pass(10.0, 1, here, &[], surface), Some((40, [300.0, 0.0, 0.0])), "the nearest vehicle on the surface");
        assert_eq!(e.air_pass(10.0, 2, here, &[], surface).map(|x| x.0), Some(41), "another pilot, another vehicle");
        assert_eq!(e.air_pass(10.0, 3, here, &[[100.0, 0.0, 10.0]], surface).map(|x| x.0), Some(42), "the one taking our flag");
        assert_eq!(e.air_pass(10.0, 1, here, &[], surface).map(|x| x.0), Some(40), "it keeps its own");
        e.reports.push(Report { t: 9.0, player: 44, pos: [400.0, -2.0, 0.0], vehicle: true });
        e.passes.clear();
        assert_eq!(e.air_pass(10.0, 1, here, &[], surface).map(|x| x.0), Some(40));
        e.reports.retain(|r| r.player != 41);
        assert_eq!(e.air_pass(10.0, 2, here, &[], surface).map(|x| x.0), Some(44), "the ship");
        assert_eq!(e.air_pass(10.0, 3, here, &[], surface).map(|x| x.0), Some(44), "a ship is shared");
        assert_eq!(e.air_pass(40.0, 1, here, &[], surface), None, "old reports");
    }

    /// B23: an idle follower near a free plane of the side is sent to fly it; not the leader, not more than the share, and not one on
    /// land to a plane on a ship's deck.
    #[test]
    fn b23_the_wing_sends_a_follower_to_the_plane() {
        let mut e = Team::default();
        for j in [1, 2, 3, 4, 5, 6, 7, 8] {
            e.kit_de.insert(j, 0);
        }
        let plane = |vehicle: usize, x: f32, afloat: bool| VehicleSighting { vehicle, pos: [x, 0.0, 0.0], team: 0, free_seats: 1, class: VehicleClass::Plane, health: 1.0, floats: false, seats: vec![0], radius: 6.0, afloat, vtol: false };
        let mates: Vec<Mate> = (1..=8).map(|j| Mate { player: j, pos: [j as f32 * 10.0, 0.0, 0.0], vehicle: None }).collect();
        let view = TeamView { team: 1, mates, flags: Vec::new(), vehicles: vec![plane(50, 0.0, false), plane(51, 5.0, false), plane(52, 10.0, false), plane(53, 20.0, true)] };
        e.orders.insert(1, Order { goal: Goal::Flag(0), role: Role::Leader, kit: 0 });
        for j in 2..=8 {
            e.orders.insert(j, Order { goal: Goal::Flag(0), role: Role::Follow(1), kit: 0 });
        }
        e.wing(0.0, &view, &trip::Unknown);
        // 8 bots: 2 pilots; the nearest followers (1 leads)
        assert_eq!(e.wing_vehicle(2), Some(50));
        assert_eq!(e.wing_vehicle(3), Some(51));
        assert_eq!(e.wing_vehicle(1), None);
        assert_eq!(e.order(2).role, Role::Drive(50));
        assert_eq!(e.wing_free, vec![], "the share is full: the rest wait for a pilot from the spawn screen");
        // on a ship's deck only one there gets it
        let mut e2 = Team::default();
        e2.kit_de.insert(1, 0);
        e2.orders.insert(1, Order { goal: Goal::Flag(0), role: Role::Follow(9), kit: 0 });
        let v2 = TeamView { team: 1, mates: vec![Mate { player: 1, pos: [200.0, 0.0, 0.0], vehicle: None }], flags: Vec::new(), vehicles: vec![plane(53, 0.0, true)] };
        e2.wing(0.0, &v2, &trip::Unknown);
        assert_eq!(e2.wing_vehicle(1), None);
        assert_eq!(e2.wing_free.len(), 1);
    }

    /// The leader in its tank knows how near the nearest of its own on foot is (those in a vehicle do not count).
    /// B26: each enemy aircraft gets a fighter before any gets two, never more than two, and a fighter keeps the one it was given.
    #[test]
    fn b26_the_fighters_share_out_the_enemy_aircraft() {
        let mut e = Team::default();
        e.air_seen(10.0, 40, [1000.0, 300.0, 0.0]);
        e.air_seen(10.0, 41, [2000.0, 300.0, 0.0]);
        let here = [0.0, 300.0, 0.0];
        assert_eq!(e.engage(10.0, 1, here, &[]), Some((40, [1000.0, 300.0, 0.0])), "the nearest");
        assert_eq!(e.air_taken(2), [40, usize::MAX, usize::MAX, usize::MAX, usize::MAX, usize::MAX]);
        assert_eq!(e.engage(10.0, 2, here, &[]).map(|x| x.0), Some(41), "the one nobody is after");
        assert_eq!(e.engage(10.0, 3, here, &[]).map(|x| x.0), Some(40), "then a second on the nearest");
        assert_eq!(e.engage(10.0, 4, here, &[]).map(|x| x.0), Some(41));
        assert_eq!(e.engage(10.0, 5, here, &[]), None, "two each is the most");
        // it keeps its own while it is seen, wherever that one goes; and it is free again once its fighter stops asking
        e.air_seen(11.0, 41, [2900.0, 300.0, 0.0]);
        assert_eq!(e.engage(11.0, 2, here, &[]), Some((41, [2900.0, 300.0, 0.0])));
        assert_eq!(e.engage(14.0, 5, here, &[]).map(|x| x.0), Some(40), "those who asked at 10 are gone");
        // out of sight for too long, or too far: nothing to go after
        assert_eq!(e.engage(40.0, 2, here, &[]), None);
        e.air_seen(50.0, 42, [9000.0, 300.0, 0.0]);
        assert_eq!(e.engage(50.0, 2, here, &[]), None);
    }

    /// B26: the enemy aircraft over an own flag that is being taken goes first, though another is nearer; and a fighter that fixed on
    /// one by itself counts as being after it.
    #[test]
    fn b26_what_threatens_an_own_flag_goes_first() {
        let mut e = Team::default();
        e.air_seen(10.0, 40, [500.0, 300.0, 0.0]);
        e.air_seen(10.0, 41, [2000.0, 300.0, 100.0]);
        let flag = [[2000.0, 20.0, 0.0]];
        assert_eq!(e.engage(10.0, 1, [0.0, 300.0, 0.0], &flag).map(|x| x.0), Some(41));
        assert_eq!(e.engage(10.0, 2, [0.0, 300.0, 0.0], &flag).map(|x| x.0), Some(41));
        assert_eq!(e.engage(10.0, 3, [0.0, 300.0, 0.0], &flag).map(|x| x.0), Some(40));
        let mut e = Team::default();
        e.air_seen(10.0, 40, [500.0, 300.0, 0.0]);
        e.air_seen(10.0, 41, [900.0, 300.0, 0.0]);
        e.air_locked(10.0, 7, 40);
        assert_eq!(e.engage(10.0, 1, [0.0, 300.0, 0.0], &[]).map(|x| x.0), Some(41));
    }

    #[test]
    fn b02_the_tank_escort() {
        let mut e = Team::default();
        e.squads.push(Squad { vehicle: Some(5), ..Squad::new(1, vec![2, 3], Goal::Flag(0)) });
        let o = |player: usize, x: f32, in_vehicle: bool| crate::bot_api::Other { player, team: 1, pos: [x, 0.0, 0.0], in_vehicle, vehicle: None, kit: None, health: None };
        assert_eq!(e.escort(1, [0.0; 3], &[o(2, 60.0, false), o(3, 10.0, true), o(9, 5.0, false)]), Some(60.0));
        assert_eq!(e.escort(2, [0.0; 3], &[o(2, 60.0, false)]), None, "not a leader");
    }

    /// GC_Mos_Eisley, team 1: with enemy flags out of reach on foot, every front flag of its own gets a squad (none left empty).
    #[test]
    fn unreachable_enemy_flags_leave_no_front_flag_empty() {
        let high = |x: f32| FlagSighting { on_foot: false, ..b(x, 2) };
        let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(100.0, 1), b(150.0, 1), high(300.0), high(320.0)], vehicles: vec![] };
        let u: Vec<Unit> = (0..3).map(|j| Unit { pos: (0..3).map(|k| c(3 * j + k, 10.0).pos).collect(), previous: Some(0), avoid: Vec::new() }).collect();
        let r = split_up(10.0, &view, &[], &[None; 5], &u, &Levers { guard: false, ..Levers::default() });
        for i in 0..3 {
            assert!(r.iter().any(|d| d.is_some_and(|d| d.flag == i)), "flag {i} empty: {r:?}");
        }
    }

    /// B20, DC_Weapon_Bunkers: two bases that never change hands. Nobody waits at its own: the side goes to the enemy's.
    #[test]
    fn b20_with_nothing_to_take_the_side_goes_to_the_enemy() {
        let fixed = |x: f32, owner: u32| FlagSighting { takeable: false, ..b(x, owner) };
        let view = TeamView { team: 1, mates: vec![], flags: vec![fixed(0.0, 1), fixed(400.0, 2)], vehicles: vec![] };
        let u: Vec<Unit> = (0..4).map(|j| Unit { pos: (0..3).map(|k| c(3 * j + k, 10.0).pos).collect(), previous: None, avoid: Vec::new() }).collect();
        let r = split_up(10.0, &view, &[], &[None; 2], &u, &Levers::default());
        assert!(r.iter().all(|d| d.is_some_and(|d| d.flag == 1)), "{r:?}");
        // out of reach on foot, it is not a destination (that needs a flight)
        let mut v2 = view.clone();
        v2.flags[1].on_foot = false;
        assert!(split_up(10.0, &v2, &[], &[None; 2], &u, &Levers::default()).iter().all(|d| d.is_none()));
    }

    /// An own flag with an enemy on top calls the nearest, even if alone; the threatened one gets a defender.
    /// GC_Mos_Eisley, team 1: both enemy flags are high up; nobody goes there, the front ones of its own are defended.
    #[test]
    fn what_cannot_be_taken_on_foot_is_not_attacked() {
        let high = |x: f32| FlagSighting { on_foot: false, ..b(x, 2) };
        let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(100.0, 1), high(300.0), high(320.0), b(-200.0, 1)], vehicles: vec![] };
        let u: Vec<Unit> = (0..4).map(|j| Unit { pos: (0..3).map(|k| c(3 * j + k, 10.0).pos).collect(), previous: Some(2), avoid: Vec::new() }).collect();
        let r = split_up(10.0, &view, &[], &[None; 5], &u, &Levers { guard: false, ..Levers::default() });
        assert!(r.iter().all(|d| d.is_some_and(|d| d.flag != 2 && d.flag != 3)), "{r:?}");
        assert!(r.iter().any(|d| d.unwrap().flag == 1), "the front one first: {r:?}");
        // within reach, they are attacked
        let mut v2 = view.clone();
        v2.flags[2].on_foot = true;
        assert!(split_up(10.0, &v2, &[], &[None; 5], &u, &Levers::default()).iter().any(|d| d.unwrap().flag == 2));
        // with a transport helicopter: still the one on land first (DC_Sea_Rigs); with none on land, the high ones (GC_Judicator)
        let heli = VehicleSighting { vehicle: 9, pos: [0.0; 3], team: 1, free_seats: 4, class: VehicleClass::Helicopter, health: 1.0, floats: false, seats: vec![0, 1, 2, 3], radius: 6.0, afloat: false, vtol: false };
        let mut v3 = view.clone();
        v3.vehicles.push(heli);
        v3.flags[2].owner = 1;
        let mut extra = v3.clone();
        extra.flags.push(b(200.0, 2));
        assert!(split_up(10.0, &extra, &[], &[None; 6], &u, &Levers { guard: false, ..Levers::default() }).iter().all(|d| d.is_some_and(|d| d.flag == 5 || d.flag == 1)), "the one on land");
        assert!(split_up(10.0, &v3, &[], &[None; 5], &u, &Levers { guard: false, ..Levers::default() }).iter().any(|d| d.unwrap().flag == 3), "by air when nothing is on land");
    }

    /// A squad with a ground leg that does not move for two windows fails it and, with no other way, avoids that flag for a while.
    #[test]
    fn a_squad_that_does_not_move_avoids_its_flag() {
        let mut e = Team::default();
        let mut view = TeamView { team: 1, mates: (0..4).map(|j| c(j, 0.0)).collect(), flags: vec![b(0.0, 1), b(600.0, 2), b(700.0, 2)], vehicles: vec![] };
        for m in view.mates.iter_mut() {
            m.pos[0] = -200.0;
        }
        let mut goals = Vec::new();
        for k in 0..12 {
            e.think(4.0 * k as f64, &view);
            goals.push(e.squads[0].goal);
        }
        assert_eq!(goals[0], Goal::Flag(1));
        assert!(e.squads[0].avoid.iter().any(|a| a.0 == 1), "{:?}", e.squads[0]);
        assert_eq!(*goals.last().unwrap(), Goal::Flag(2), "another flag: {goals:?}");
    }

    /// B20: the leader whose trip on foot fails for not advancing is told so, for a moment: a new flag does not unstick a body.
    #[test]
    fn b20_the_leader_that_does_not_advance_is_told_to_get_out() {
        let mut e = Team::default();
        let mut view = TeamView { team: 1, mates: (0..4).map(|j| c(j, 0.0)).collect(), flags: vec![b(0.0, 1), b(600.0, 2), b(700.0, 2)], vehicles: vec![] };
        for m in view.mates.iter_mut() {
            m.pos[0] = -200.0;
        }
        let mut told = Vec::new();
        for k in 0..12 {
            let t = 4.0 * k as f64;
            e.think(t, &view);
            let leader = e.squads[0].leader;
            if e.stuck_on_foot(leader, t) {
                told.push(t);
            }
            assert!(!e.stuck_on_foot(leader + 1, t), "only the leader");
        }
        assert!(!told.is_empty() && told[0] >= 2.0 * trip::WINDOW_S, "after two windows without moving: {told:?}");
        assert!(!e.stuck_on_foot(e.squads[0].leader, told[0] + 2.0), "and only for a moment");
    }

    #[test]
    fn reach_on_foot_is_measured_in_3d() {
        let flat = |_: f32, _: f32| Some(1.0);
        assert!(reachable_on_foot([0.0, 13.0, 0.0], 18.0, 0.0, flat), "GC_Mos_Eisley 4: 12 m above the ground, radius 18");
        assert!(!reachable_on_foot([0.0, 22.0, 0.0], 18.0, 0.0, flat), "GC_Mos_Eisley 2");
        assert!(reachable_on_foot([0.0, 22.0, 0.0], 18.0, 0.0, |x, _| Some(if x > 10.0 { 10.0 } else { 1.0 })), "a high edge inside the circle");
        assert!(reachable_on_foot([0.0, 3.0, 0.0], 5.0, 2.5, |_, _| Some(-20.0)), "above the water, swimming");
    }

    #[test]
    fn b18_the_flag_being_taken_calls_the_nearest() {
        let mut u: Vec<Unit> = (0..10).map(|j| Unit { pos: vec![c(j, 0.0).pos], previous: Some(1), avoid: Vec::new() }).collect();
        u.push(Unit { pos: vec![c(10, 880.0).pos], previous: Some(1), avoid: Vec::new() });
        let view = TeamView { team: 1, mates: vec![], flags: vec![b(0.0, 1), b(300.0, 2), b(900.0, 1)], vehicles: vec![] };
        let reports = [Report { t: 9.0, player: 40, pos: [905.0, 0.0, 0.0], vehicle: false }];
        let r = split_up(10.0, &view, &reports, &[None; 3], &u, &Levers::default());
        assert_eq!(r[10], Some(Destination { flag: 2, urgent: true }));
        assert!(r.iter().filter(|d| d.unwrap().urgent).count() <= 2);
    }

    #[test]
    fn b25_the_nearest_free_medic_is_sent_once_per_call() {
        let mut e = Team::default();
        assert!(e.ask_medic(10.0, 5, &[7, 8]));
        assert_eq!(e.medic_call(7, 11.0), Some(5));
        assert_eq!(e.medic_call(8, 11.0), None);
        assert!(!e.ask_medic(11.0, 5, &[8]), "one medic per call");
        assert!(e.ask_medic(11.0, 6, &[7, 8]), "7 is busy: the next goes");
        assert_eq!(e.medic_call(8, 12.0), Some(6));
        assert!(!e.ask_medic(12.0, 9, &[]), "no medic to send");
        e.medic_call_done(5);
        assert_eq!(e.medic_call(7, 12.0), None);
        assert_eq!(e.medic_call(8, 10.0 + MEDIC_CALL_S + 2.0), None, "the order lapses");
    }

    #[test]
    fn the_kit_is_recognised_by_what_it_carries() {
        let it = |n: &str| vec![(0, "Colt".to_string()), (1, n.to_string())];
        assert_eq!(Kind::de(&it("Medikit")), Kind::Medic);
        assert_eq!(Kind::de(&it("Wrench")), Kind::Engineer);
        assert_eq!(Kind::de(&it("Bazooka")), Kind::AntiTank);
        assert_eq!(Kind::de(&it("Thompson")), Kind::Assault);
    }
}
