//! A side's head: what a whole team decides, like the player who directs it over the radio. Which flag each one attacks or defends
//! (by its worth, how far it is and how many are already going), who goes with whom, who drives each vehicle and who rides as
//! passenger, where each one spawns and what mix of kits the side carries.
//!
//! There is one `Team` per side and round, shared by all its bots (`Arc<Mutex<Team>>`): the first to think each tick
//! updates it and each bot reads its [`Order`]. It only knows what a side would know of itself ([`TeamView`]: mates, flags and their
//! owner, vehicles) and, of the enemy, what its own tell it ([`Team::report`]: whom each one sees, with the time, like a
//! radio report). Never the position of an enemy nobody has seen.
//!
//! The split (every `split_period_s` s, [`crate::bots::params::BotParams`]):
//! 1. **urgency**: an own flag the enemy is taking (one has been seen on top of it, or it has just gone neutral) sends
//!    the own units closer than `urgency_m`, at once, even if alone or in a vehicle with empty seats;
//! 2. **defence**: an own flag with enemies seen nearby gets one or more defenders;
//! 3. **guard**: the own front flag keeps a few (never half the side);
//! 4. **attack**: the rest is spread among the takeable flags nearest the front, without bunching up.
//!
//! Then the **squads**: those going to the same flag, in groups of 3 to 5 with a leader; a vehicle with seats near
//! the squad is filled (driver and passengers) and the driver waits for its own.
use super::body::rng::Rng;
use super::squad::{self, Squad, Levers};
use super::params::BotParams;
use super::trip::{self, Trip};
use crate::bot_api::{BotView, DeadView, VehicleClass};
use std::collections::{BTreeMap, BTreeSet};

mod parked;
mod split;
mod think;
#[cfg(test)]
mod tests;
pub use parked::*;
pub use split::*;

/// A pilot goes to an enemy reported less than this ago, s, and closer than this to it, m.
pub const AIR_REPORT_S: f64 = 15.0;
pub const AIR_REPORT_RANGE: f32 = 1500.0;

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
/// nobody takes them, bots or the plain bot: the team with only those to attack sent everyone there and left its own alone.
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

/// The parts of the head that are switched on (to measure what each contributes: `bots:no-assign`...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    /// flag split; without it, each one chooses its flag
    pub split: bool,
    /// shared squads and vehicles
    pub squads: bool,
    /// spawn with the squad and the kit
    pub spawning: bool,
    /// a squad also flies with a way by land when the flight arrives [`trip::AIR_GAIN_S`] sooner ([`trip::choose`]); without it,
    /// only with no way by land (`bots:no-heli`). On: against bots without it, 8 seeds of 8 min, Basrahs_Edge and Al_Nas +0 (the
    /// car wins there), LostVillage +5, Operation_Bragg +5
    pub heli: bool,
    /// map profile from what is seen on starting, in the DC and GC mods ([`super::mods`]); `bots:no-mods` switches it off
    pub mods: bool,
    /// flags that cannot be taken on foot are not attack targets ([`reachable_on_foot`]); off, all are (`bots:no-reach`)
    pub reach: bool,
    /// the side's aircraft that fight get a pilot ([`Team::wing`]): an idle bot near one takes it; off, only a squad's trip or a
    /// bot on its own takes one (`bots:no-wing`)
    pub wing: bool,
    /// a pilot with bombs or rockets makes attack runs on a target the side gives it ([`Team::air_pass`]); off, it flies as before
    /// (`bots:no-pass`)
    pub pass: bool,
    /// `bots:strength`: the attack goes by what the side knows of the enemy ([`super::intel`]): each flag needs more people than it
    /// has defenders, the undefended goes first, and the people are not spread over flags none of whose shares would be enough
    /// ([`attack_by_strength`]); off, fixed shares to the flags nearest the side
    pub strength: bool,
    /// `bots:flank2`: a squad attacking a flag with the enemy seen near it sends one flanker, chosen by the side, round the side the
    /// enemy is not facing ([`Team::flank`]); off, half of every squad flanks by its number
    pub flank2: bool,
    /// `bots:hunt`: with no flag that changes hands, the squads go where the enemy is seen to pass ([`Team::hunt_point`]) instead
    /// of to the enemy's bases
    pub hunt: bool,
    /// `bots:traffic-mines` (with `streetwise`): the side's mines where the enemy's vehicles pass, though it did not pass alone
    /// ([`TRAFFIC_MINES_ON`])
    pub traffic_mines: bool,
    /// `bots:regroup`: the squads sent to an enemy flag wait at an assembly point short of it until enough of them are there, and go
    /// in together ([`Team::regroup`])
    pub regroup: bool,
    /// `bots:regroup-way` (R1, with `regroup`): each squad's assembly point on its own way on foot, the squad's people held at it (not
    /// only its leader) and its flag kept while it waits ([`Team::regroup_way`]).
    /// TODO: parked, off by default. It hardly acts (4 ordered waits in 128 rounds, none in Berlin, El_Alamein or Al_Nas), and
    /// tuned against the default it scores +0.0 ± 4.4 tickets with the still gate red in El_Alamein. Revisit only with squads that
    /// keep their flag across the split, so that more of them reach the point.
    pub regroup_way: bool,
    /// `bots:memory`: the side's paths on foot go round the places where it dies most, learnt before the round
    /// ([`crate::bots::memory`]); without a file for the map, nothing changes. Parked (H0 against anchor 6): see the TODO there.
    pub memory: bool,
}

impl Default for Parts {
    fn default() -> Parts {
        Parts { split: true, squads: true, spawning: true, heli: true, mods: true, reach: true, wing: true, pass: true, strength: false, flank2: false, hunt: false, traffic_mines: false, regroup: false, regroup_way: false, memory: false }
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
    /// the levers forced by the variant (`bots:guard=0`): they override the map's
    pub forced: Vec<(String, f32)>,
    /// the named parameters ([`crate::bots::params`]) the head reads
    pub params: crate::bots::params::BotParams,
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
    /// `bots:streetwise`: who of the side has which enemy as its target lately: (enemy, mate, when)
    targeted: Vec<(usize, usize, f64)>,
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
    /// the team trace (`BYTEFIELD_BOTS_TRACE`): the first time it writes where each flag is
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
    /// what the side knows of the enemy: presence per flag and traffic ([`super::intel`])
    pub intel: super::intel::Intel,
    /// `bots:flank2`: the flanker of each squad (by leader): (the player, the flag, since when)
    flankers: BTreeMap<usize, (usize, usize, f64)>,
    /// `bots:flank2`: the arrivals on foot at flags not the side's: (when, flag, sector of [`FLANK_SECTORS`]); and who was within
    /// [`ARRIVAL_M`] of which flag at the last split
    arrivals: Vec<(f64, usize, usize)>,
    arrived: BTreeSet<(usize, usize)>,
    /// where each flag is (the last split's)
    intel_flags: Vec<[f32; 3]>,
    /// `bots:hunt`: the point the side hunts at
    hunt: Option<[f32; 3]>,
    /// `bots:streetwise`: the trait of each of its bots
    pub traits: super::streetwise::Traits,
    /// `bots:streetwise`: where its engineers lay mines: where the enemy's vehicles pass ([`Team::mine_spot`])
    pub mines_at: Option<([f32; 2], [f32; 2])>,
    /// `bots:regroup`: the assembly point of each enemy flag being attacked, and the squads (by leader) held at it
    rallies: BTreeMap<usize, Rally>,
    held: BTreeMap<usize, usize>,
    /// `bots:regroup-way`: the point each held squad (by leader) waits at
    held_at: BTreeMap<usize, [f32; 3]>,
    /// `bots:steal-flank`: the vehicles the side saw an enemy in, and when last
    enemy_driven: BTreeMap<usize, f64>,
    /// `bots:cover-flank`: what each bot said on its last tick: when, whether it was at its firing spot, and the flag and point of the
    /// flank it was going round, with where it was
    cover: BTreeMap<usize, (f64, bool, Option<(usize, [f32; 3], [f32; 3])>)>,
}

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
        self.intel.death(pos);
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
            if let Some(k) = other.as_ref().and_then(|o| o.vehicle) {
                self.enemy_driven.insert(k, v.time);
            }
            let aircraft = other.as_ref().and_then(|o| o.vehicle).and_then(|k| v.vehicle(k)).is_some_and(|x| x.class.flies());
            if aircraft && pos[1] - v.ground(pos[0], pos[2]).unwrap_or(f32::MIN).max(v.water()) > FLYING_M {
                self.air_seen(v.time, j, pos);
            }
            let p = Report { t: v.time, player: j, pos, vehicle };
            let class = other.as_ref().and_then(|o| o.vehicle).and_then(|k| v.vehicle(k)).map(|x| x.class);
            let what = match class {
                _ if !vehicle => super::intel::Seen::OnFoot,
                Some(VehicleClass::Tank | VehicleClass::Walker) => super::intel::Seen::Armour,
                Some(c) if c.flies() => super::intel::Seen::Aircraft,
                _ => super::intel::Seen::Vehicle,
            };
            self.intel.sighting(v.time, j, pos, what);
            self.targeted.retain(|x| v.time - x.2 < super::body::shooter::BUSY_S && !(x.0 == j && x.1 == me.player));
            self.targeted.push((j, me.player, v.time));
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

    /// `bots:streetwise`: the enemies another of the side (not `player`) has had as its target in the last
    /// [`super::body::shooter::BUSY_S`] (up to 8; `usize::MAX` fills).
    pub fn busy(&self, player: usize, t: f64) -> [usize; 8] {
        let mut out = [usize::MAX; 8];
        let mut n = 0;
        for x in self.targeted.iter().filter(|x| x.1 != player && t - x.2 < super::body::shooter::BUSY_S) {
            if n < 8 && !out[..n].contains(&x.0) {
                out[n] = x.0;
                n += 1;
            }
        }
        out
    }

    fn flag_pos(&self, i: usize) -> Option<[f32; 3]> {
        self.intel_flags.get(i).copied()
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

    /// B15: whether `player` is a bots bot of this team (one that tells the team its kit), not a person.
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
        self.intel.sighting(t, HEARD_ENEMY + speaker, at, super::intel::Seen::Vehicle);
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
