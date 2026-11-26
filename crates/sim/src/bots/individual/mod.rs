//! The player: at each moment it chooses what to do among the things a person would do (fight, flank, take cover, retreat, heal,
//! repair, board a vehicle, drive, mine, go to the goal), according to its health and ammunition, whom it sees, who walks nearby and what
//! its team has ordered. Urgency weighs more than the token: leaving alone with a vehicle to save a flag is justified.
//! Once the action is chosen, it carries it out with the body's skills. It never knows more than it sees ([`BotView`]).
//!
//! Survival: it chooses by utility ([`Action::utility`], [`choose`]) with hysteresis; urgency rules (with an urgent order it
//! does not take cover, retreat or linger healing). Fighting and going to the goal belong to the body (as in the plain bot
//! with the team's order); taking cover and retreating, looking for the medic and healing, repairing what is worth it and reacting to a
//! shot without seeing where it comes from are new. On its own ([`Role::Free`]): as the plain bot, nothing more.
// TODO: combat (flanking, weapon by target) and difficulty.
use super::body::{Maps, MapProfile};
use super::body::weapons;
use super::body::shooter::Target;
use super::body::pilot::{FlightEnd, FlightOrder, Pilot};
use super::body::{Choice, CHEST, TURN, angle_between, flat, no_fall};
use super::team::{Goal, Order, Role, Kind};
use super::profile::Profile;
use super::params::BotParams;
use crate::bot_api::{BotView, Mind, Other, VehicleClass, VehicleSeen};
use super::life::Bot;
use crate::soldier::axis;
use std::sync::Arc;

mod moves;
mod parked;
mod survival;
mod think;
pub use parked::*;
pub use survival::{enemy_heard, enemy_visible, find_cover, retreat_point};

/// What it can do at a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Fight,
    Flank,
    TakeCover,
    Retreat,
    Heal,
    Repair,
    Board,
    Drive,
    Mine,
    GoToGoal,
    /// wounded, heads for the nearest medic of its side
    FindMedic,
}

/// The actions it chooses among today (ties are broken in this order).
pub const MENU: [Action; 7] = [Action::GoToGoal, Action::Fight, Action::TakeCover, Action::Retreat, Action::FindMedic, Action::Heal, Action::Repair];

/// What it looks at to decide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Situation {
    /// from 0 to 1
    pub health: f32,
    /// magazine and reserve, bullets
    pub ammo: (i32, i32),
    /// enemies in sight
    pub enemies_seen: usize,
    /// mates closer than 30 m
    pub mates_near: usize,
    /// metres to the goal it was given
    pub goal_distance: f32,
    pub order: Order,
    // ---- survival
    /// its own is urgent
    pub urgent: bool,
    /// on foot (taking cover, healing and repairing are on-foot things)
    pub on_foot: bool,
    /// metres to the nearest enemy it has in sight
    pub enemy_m: Option<f32>,
    /// seconds since it last saw an enemy
    pub since_enemy: f32,
    /// it looked for where to take cover and there is no place (then it retreats)
    pub no_cover: bool,
    /// is a medic: the nearest wounded mate (< [`HEALTH_HEAL`], within [`HEAL_RADIUS`]): metres and its health
    pub wounded: Option<(f32, f32)>,
    /// the wounded one is who called "medic!" and the team sent it: it goes unless the enemy is on top
    pub called: bool,
    /// is not a medic: metres to the nearest medic of its side (within [`MEDIC_RADIUS`])
    pub medic_m: Option<f32>,
    /// is an engineer: metres to the nearest vehicle with its own crew and little health (within [`REPAIR_RADIUS`])
    pub damaged_m: Option<f32>,
    // ---- difficulty
    /// below this health it does not take risks (by its profile; [`HEALTH_LOW`] for the normal one)
    pub low_health: f32,
}

impl Situation {
    /// With nothing around: healthy, on foot, no order.
    pub fn calm() -> Situation {
        Situation { health: 1.0, ammo: (30, 90), enemies_seen: 0, mates_near: 0, goal_distance: 100.0, order: Order::free(), urgent: false, on_foot: true, enemy_m: None, since_enemy: 1e3, no_cover: false, wounded: None, called: false, medic_m: None, damaged_m: None, low_health: HEALTH_LOW }
    }
}

/// Below this health it does not take risks.
/// `bots:tank-cover`: where it hides from an enemy tank it cannot hurt ([`Individual::tank_cover`]).
#[derive(Clone, Copy, Debug)]
struct TankCover {
    spot: [f32; 3],
    tank: usize,
    tank_at: [f32; 3],
    seen: f64,
    since: f64,
}
/// `bots:tank-cover`: it gives up hiding when the tank has not been seen near for this long, s; it hides at most this long, s
/// (then it goes about its business for [`TANK_REST_S`] whatever the tank does: a tank parked by its flag would hold it forever);
/// and it looks for another spot once the tank has moved this far from where it was, m.
const TANK_GONE_S: f64 = 3.0;
const TANK_HIDE_S: f64 = 20.0;
const TANK_REST_S: f64 = 10.0;
const TANK_MOVED_M: f32 = 8.0;

pub const HEALTH_LOW: f32 = 0.3;
/// The medic goes to a wounded mate below this health...
pub const HEALTH_HEAL: f32 = 0.5;
/// …at this distance at most, m.
pub const HEAL_RADIUS: f32 = 60.0;
/// With the medic closer than this it goes to them even if it has just had a contact, m.
pub const MEDIC_BESIDE: f32 = 15.0;
/// The wounded goes to a medic closer than this, m.
pub const MEDIC_RADIUS: f32 = 120.0;
/// The engineer goes to a vehicle with a crew below this health...
pub const HEALTH_REPAIR: f32 = 0.6;
/// An engineer in a ground vehicle below this health (and above the jumping one), with no fight for
/// [`REPAIR_CALM_S`], gets off to repair it and then boards again.
pub const HEALTH_GET_OFF_TO_REPAIR: f32 = 0.5;
pub const REPAIR_CALM_S: f64 = 5.0;
/// After getting off to repair, how long it spends on it (and boards again if the vehicle is still free), s.
pub const REPAIR_OWN_S: f64 = 40.0;
/// …at this distance at most, m.
pub const REPAIR_RADIUS: f32 = 60.0;
/// Heal or repair only with no enemy in sight closer than this, m.
pub const CALM_M: f32 = 30.0;
/// Hysteresis: it changes action only for one it likes `util_margin` more ([`BotParams`]), and not more than once every so many s
/// (unless the current one no longer makes sense: utility 0).
pub const CHANGE_S: f64 = 1.0;
/// Where it looks for a hidden spot: how many metres, how many points, and for how long before giving up, s.
pub const COVER_M: [f32; 3] = [10.0, 17.0, 25.0];
pub const COVER_POINTS: usize = 10;
pub const COVER_S: f64 = 10.0;
/// In the hidden spot it stays at most this, s (then it retreats or goes back to its business).
pub const UNDER_COVER_S: f64 = 15.0;
/// How long the reaction to a shot it does not know the origin of lasts, s, and how far it looks for whoever gave it, m.
pub const REACTION_S: f64 = 1.8;
pub const REACTION_SIGHT_M: f32 = 150.0;
/// B14: what a reaction answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// a hit, with the enemy in sight or nobody (then it steps aside)
    Hit,
    /// a hit, with only a shot heard: it turns to it and steps aside
    ShotHeard,
    /// no hit: an enemy shot heard near, it looks that way
    Heard,
}

/// B14: a fixed gun within this is at hand, m; it takes it with an enemy soldier in sight within this, m, or an enemy plane within
/// this of the gun, m; it goes to it for this long at most, s.
pub const FIXED_M: f32 = 20.0;
pub const FIXED_TARGET_M: f32 = 120.0;
pub const FIXED_PLANE_M: f32 = 400.0;
pub const FIXED_GO_S: f64 = 10.0;
/// B14: a gun this near its own flag guards it, m.
pub const FIXED_FLAG_M: f32 = 80.0;
/// B14: the gunner's eye over the gun's origin, m.
pub const FIXED_EYE: f32 = 1.2;
/// B14: it gets off a fixed gun after this without a target, s.
pub const FIXED_NO_TARGET_S: f64 = 10.0;
/// B14: without a target, it turns to an enemy shot heard within this, m, fired in the last this, s; it looks that way for this, s,
/// and not again until this after.
pub const HEARD_TURN_M: f32 = 60.0;
pub const HEARD_RECENT_S: f32 = 0.5;
pub const HEARD_TURN_S: f64 = 1.2;
pub const HEARD_CALM_S: f64 = 4.0;

impl Action {
    /// How much it feels like doing it now, from 0 (nothing, makes no sense) to 1. Urgency rules: with an urgent order, going to the goal
    /// is worth almost the maximum and taking cover, retreating or lingering to heal are worth nothing.
    pub fn utility(&self, s: &Situation, p: &BotParams) -> f32 {
        let near = s.enemy_m.is_some_and(|d| d < p.fight_near_m);
        let seen = s.enemy_m.is_some();
        let calm_one = s.enemy_m.is_none_or(|d| d > CALM_M);
        let cautious = s.on_foot && !s.urgent && !near;
        match self {
            Action::GoToGoal => {
                if s.urgent {
                    0.95
                } else {
                    p.util_goal
                }
            }
            Action::Fight => {
                if near {
                    1.0
                } else if seen {
                    p.util_fight
                } else {
                    0.0
                }
            }
            // with little health and the enemy in sight, but not on top: breaks contact
            // TODO: with full health it fights wherever it stands, so it stops in the middle of a street (a tester's
            // marks in DC_Al_Nas); weigh moving to cover a few metres away (COVER_M) before opening fire, by profile.
            Action::TakeCover if cautious && s.health < s.low_health && seen && !s.no_cover => p.util_cover,
            // with little health after a contact (or with nowhere to take cover): takes cover or moves away towards its own
            // (with the enemy in sight and nowhere to hide, turning its back is worse than fighting)
            Action::Retreat if cautious && s.health < s.low_health && s.since_enemy < p.enemy_memory_s => {
                if seen {
                    p.util_retreat_seen
                } else {
                    p.util_retreat
                }
            }
            // wounded, with a medic near and no fight in sight; with little health and a recent contact, it breaks contact first, unless
            // the medic is at hand
            Action::FindMedic if cautious && s.health < p.health_medic && !seen && s.medic_m.is_some_and(|d| d < MEDIC_BESIDE || s.health >= s.low_health || s.since_enemy >= p.enemy_memory_s) => {
                p.util_medic + (p.health_medic - s.health) * 0.4
            }
            Action::Heal if s.on_foot && !s.urgent && s.called && !near => s.wounded.map_or(0.0, |(d, _)| 0.85 - d / 600.0),
            Action::Heal if s.on_foot && !s.urgent && calm_one => s.wounded.map_or(0.0, |(d, h)| p.util_heal + (1.0 - h) * 0.3 - d / 600.0),
            Action::Repair if s.on_foot && !s.urgent && calm_one && s.damaged_m.is_some() => p.util_repair,
            _ => 0.0,
        }
    }
}

/// The action it is due: the one with most utility in the [`MENU`], unless the current one (`current`, chosen at `since`) still makes sense and the
/// new one does not beat it by `util_margin` or [`CHANGE_S`] has not passed.
pub fn choose(s: &Situation, current: Action, since: f64, t: f64, p: &BotParams) -> Action {
    let mut best = (MENU[0], MENU[0].utility(s, p));
    for a in &MENU[1..] {
        let u = a.utility(s, p);
        if u > best.1 {
            best = (*a, u);
        }
    }
    let now = current.utility(s, p);
    if best.0 == current || (now > 0.0 && (best.1 < now + p.util_margin || t - since < CHANGE_S)) {
        return current;
    }
    best.0
}

/// The mates of a side that carry a kit (as a set of player numbers, up to 128).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ensemble(u128);

impl Ensemble {
    pub fn put(&mut self, j: usize) {
        if j < 128 {
            self.0 |= 1 << j;
        }
    }
    pub fn has(&self, j: usize) -> bool {
        j < 128 && self.0 & (1 << j) != 0
    }
}

/// B26: the side's order to a fighter in flight: after that enemy aircraft, to where it was last reported, before anything on the
/// ground; and whom its mates are already after.

/// B23: with bombs or rockets, attack runs on the target the side gave it (a plane); a helicopter goes to it and stays at firing
/// distance (`attack_run`). A fighter sent after an enemy aircraft ([`engage`]) goes after it first.
fn pass(p: &mut Pilot, command: &Command, v: &BotView) {
    let Some(i) = v.me.inside.as_ref().filter(|i| i.seat == 0 && i.class.flies()) else { return };
    let Some(tg) = command.air_pass else { return };
    // (a fighter sent after an enemy aircraft goes after it first)
    if matches!(p.flight, Some(FlightOrder { end: FlightEnd::Engage { .. }, .. })) {
        return;
    }
    p.imposed_goal = Some([tg[0], tg[2]]);
    if i.class == VehicleClass::Plane {
        p.flight = Some(FlightOrder { to: tg, end: FlightEnd::Pass { target: tg, weapon: None }, hold: false });
    }
}

fn engage(pilot: &mut super::body::pilot::Pilot, command: &Command) {
    pilot.taken = command.air_taken;
    if let Some((player, to)) = command.air_engage {
        pilot.flight = Some(FlightOrder { to, end: FlightEnd::Engage { player }, hold: false });
        pilot.imposed_goal = None;
    }
}

/// What reaches it from the team: its order and what it needs to know to carry it out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Command {
    pub order: Order,
    /// its own is urgent (an own flag being lost): it waits for nobody
    pub urgent: bool,
    /// driving: how many of the passengers it was given have not boarded yet
    passengers_out: usize,
    /// leader with its own on foot to the goal: it does not leave alone in a vehicle (the team gives it one with seats for all)
    accompanied: bool,
    /// the medics and engineers of its side (what anyone knows from the radio and the icons)
    pub medics: Ensemble,
    pub engineers: Ensemble,
    /// where several of its own have just fallen (the side's casualty report)
    pub dangers: [Option<[f32; 2]>; 4],
    /// the vehicles the side has given to others (it does not take them on its own); `usize::MAX`, a gap
    pub reserved: [usize; 8],
    /// at the controls of a plane or helicopter, the enemy the side has reported and which it heads for
    pub air_target: Option<[f32; 3]>,
    /// B23: the target of its attack runs, if it flies with bombs or rockets ([`super::team::Team::air_pass`])
    pub air_pass: Option<[f32; 3]>,
    /// B26: at the controls of a fighter in flight, the enemy aircraft the side sends it after (who flies it and where it was last
    /// reported), and the enemy pilots its mates are already after (`usize::MAX`, a gap)
    pub air_engage: Option<(usize, [f32; 3])>,
    pub air_taken: [usize; 6],
    /// at the wheel of the squad's tank, how near the nearest of its own on foot is (if any is on foot)
    pub escort_m: Option<f32>,
    /// B15: the "medic!" call this medic answers (the player who asked), and the vehicle whose "need repairs" this engineer answers
    pub call_medic: Option<usize>,
    pub call_repairs: Option<usize>,
    /// B25: the "medic!" it answers is a person's: it goes even with an enemy in sight, unless the enemy is on top
    pub call_person: bool,
    /// `bots:flank2`: its side names it its squad's flanker
    pub flank: Option<super::team::Flank>,
    /// `bots:streetwise`: the enemies others of the side are shooting at ([`super::team::Team::busy`])
    pub busy: [usize; 8],
    /// `bots:steal-flank`: the vehicles its side saw driven by an enemy lately ([`super::team::Team::enemy_vehicles`])
    pub enemy_vehicles: [usize; 8],
    /// `bots:cover-flank`: a mate of its squad is at its firing spot; and the flank a mate goes round
    pub covered: bool,
    pub cover: Option<super::team::CoverFlank>,
    /// `bots:streetwise`: where its side's mines go ([`super::team::Team::mine_spot`])
    pub mines_at: Option<([f32; 2], [f32; 2])>,
    /// `bots:streetwise`: the cell where an enemy on foot passes again and again ([`super::intel::Intel::habit`])
    pub habit: Option<[f32; 3]>,
    /// the boat or aircraft of its squad's trip leg ([`super::trip`]): it may be far, and the patience to board it counts from reaching it
    pub leg_vehicle: Option<usize>,
    /// B23: the aircraft the side sent it to fly (its patience to board counts once beside it, as a leg's)
    pub wing_vehicle: Option<usize>,
    /// where the aircraft of its squad's air leg lands ([`super::trip::landing_spot`]): the pilot lands there with its passengers
    pub leg_landing: Option<[f32; 3]>,
    /// the point the plane of its squad's air leg passes over, where everyone in it jumps
    pub leg_drop: Option<[f32; 3]>,
}

impl Command {
    pub fn new(order: Order, urgent: bool, passengers_out: usize, accompanied: bool) -> Command {
        Command { order, urgent, passengers_out, accompanied, medics: Ensemble::default(), engineers: Ensemble::default(), dangers: [None; 4], reserved: [usize::MAX; 8], air_target: None, air_pass: None, air_engage: None, air_taken: [usize::MAX; 6], escort_m: None, busy: [usize::MAX; 8], enemy_vehicles: [usize::MAX; 8], covered: false, cover: None, mines_at: None, habit: None, call_medic: None, call_repairs: None, call_person: false, flank: None, leg_vehicle: None, wing_vehicle: None, leg_landing: None, leg_drop: None }
    }

    /// With the vehicle of its squad's trip leg.
    pub fn with_wing_vehicle(mut self, v: Option<usize>) -> Command {
        self.wing_vehicle = v;
        self
    }

    pub fn with_leg_vehicle(mut self, v: Option<usize>) -> Command {
        self.leg_vehicle = v;
        self
    }

    /// With where the people of its squad's air leg jump.
    pub fn with_leg_drop(mut self, p: Option<[f32; 3]>) -> Command {
        self.leg_drop = p;
        self
    }

    /// With where the aircraft of its squad's air leg lands.
    pub fn with_leg_landing(mut self, p: Option<[f32; 3]>) -> Command {
        self.leg_landing = p;
        self
    }

    /// With the medics and engineers of the side.
    pub fn with_trades(mut self, medics: Ensemble, engineers: Ensemble) -> Command {
        self.medics = medics;
        self.engineers = engineers;
        self
    }

    /// B15: with the radio calls it answers.
    pub fn with_calls(mut self, medic: Option<usize>, repairs: Option<usize>) -> Command {
        self.call_medic = medic;
        self.call_repairs = repairs;
        self
    }

    /// B25: the "medic!" it answers is a person's.
    pub fn with_person_call(mut self, person: bool) -> Command {
        self.call_person = person;
        self
    }

    /// With how near its escort on foot is.
    pub fn with_escort(mut self, e: Option<f32>) -> Command {
        self.escort_m = e;
        self
    }

    /// B26: with the enemy aircraft the side sends this fighter after, and those its mates are after.
    pub fn with_air_engage(mut self, engage: Option<(usize, [f32; 3])>, taken: [usize; 6]) -> Command {
        self.air_engage = engage;
        self.air_taken = taken;
        self
    }

    /// With the target of whoever flies.
    pub fn with_air_pass(mut self, b: Option<[f32; 3]>) -> Command {
        self.air_pass = b;
        self
    }

    pub fn with_air_target(mut self, b: Option<[f32; 3]>) -> Command {
        self.air_target = b;
        self
    }

    /// With the vehicles the side has given to others.
    pub fn with_reserved(mut self, r: [usize; 8]) -> Command {
        self.reserved = r;
        self
    }

    /// With the places where several of its own have just fallen.
    pub fn with_dangers(mut self, p: [Option<[f32; 2]>; 4]) -> Command {
        self.dangers = p;
        self
    }

    pub fn with_flank(mut self, f: Option<super::team::Flank>) -> Command {
        self.flank = f;
        self
    }
    pub fn with_habit(mut self, h: Option<[f32; 3]>) -> Command {
        self.habit = h;
        self
    }
    pub fn with_mines_at(mut self, m: Option<([f32; 2], [f32; 2])>) -> Command {
        self.mines_at = m;
        self
    }
    pub fn with_busy(mut self, b: [usize; 8]) -> Command {
        self.busy = b;
        self
    }

    pub fn with_cover(mut self, covered: bool, cover: Option<super::team::CoverFlank>) -> Command {
        self.covered = covered;
        self.cover = cover;
        self
    }

    pub fn with_enemy_vehicles(mut self, b: [usize; 8]) -> Command {
        self.enemy_vehicles = b;
        self
    }
}

/// For how long after its path ran off an edge it goes on its own and may take any vehicle, s.
pub const EDGE_S: f64 = 20.0;
/// Goes with the leader when it has fallen farther behind than this, m (squad: ±20 m outside combat)...
pub const FOLLOW_FAR: f32 = 14.0;
/// …until it is closer than this, m.
pub const FOLLOW_NEAR: f32 = 6.0;
/// Behind its leader's tank it goes when it has fallen farther behind than this, until it is closer than this, m; the
/// tank waits for its escort on foot if the nearest is farther than [`ESCORT_WAIT_M`] (and closer than [`ESCORT_FAR_M`]: farther
/// away it is on something else), at most [`ESCORT_WAIT_S`] in a row and then not for [`ESCORT_REST_S`].
pub const TANK_FAR: f32 = 25.0;
pub const TANK_NEAR: f32 = 12.0;
pub const ESCORT_WAIT_M: f32 = 35.0;
pub const ESCORT_FAR_M: f32 = 150.0;
pub const ESCORT_WAIT_S: f64 = 15.0;
pub const ESCORT_REST_S: f64 = 30.0;
/// The passenger gets off closer than this to the goal, m.
pub const GET_OFF_NEAR: f32 = 50.0;
/// B21: a ground vehicle standing for this long, s, within this distance of the flag it attacks, m, and farther from it than this part
/// of its radius, is left there.
pub const PARKED_S: f64 = 5.0;
/// At the wheel of a ground vehicle with no way to the goal on the vehicle path map for this long, s, the driver gets off and goes on
/// foot ([`super::body::driver::Driver::no_way`]): sitting there it waited for the jam's 25 s (DC_Al_Nas: a flag in a walled
/// compound; DC_Coastal_Hammer: water in between, in what does not swim).
pub const NO_WAY_S: f64 = 4.0;
/// At the wheel of a ground vehicle with a way to its goal, nobody to shoot at and not waiting, it gets off if it has not got further than
/// this from one spot in this long, m and s.
pub const JAM_M: f32 = 8.0;
pub const JAM_S: f64 = 20.0;
pub const PARKED_M: f32 = 60.0;
pub const PARKED_IN: f32 = 0.8;
/// Whoever is going to board a seat gives up if it does not manage within this time, s.
pub const SEAT_PATIENCE: f64 = 20.0;
/// Closer than this to its vehicle's edge, still and pushing for more than [`GO_ROUND_WAIT_S`], it goes round it; it changes side every
/// [`GO_ROUND_SIDE_S`]; m and s.
pub const GO_ROUND_M: f32 = 2.5;
/// Farther than this from its vehicle's edge it goes to it by the paths, like to any place; closer, to the entrance, m.
pub const SEAT_WALKING_M: f32 = 6.0;
/// Going to its seat by the paths, if it has not taken a step in this long, it goes straight, s.
pub const SEAT_NO_WAY_S: f64 = 2.0;
/// Going to its seat in a straight line, if it has not taken a step in this long, it is shut in and breaks out, s.
pub const SEAT_SHUT_IN_S: f64 = 6.0;
/// B03: what counts as a step in [`SEAT_NO_WAY_S`], m.
pub const SEAT_STEP_M: f32 = 1.0;
/// A helicopter is landed below this over what is below it (the ground, a dock, a deck), m; whoever flies it with people lands at this from the flag, and
/// they get off closer than this to it, m.
pub const LANDED_M: f32 = 3.0;
/// The trip's air leg in a plane: how far over the point it flies, m; the least height over it to jump, m; within what distance of
/// it they jump, m, plus what the plane covers in this long, s.
pub const DROP_UP_M: f32 = 30.0;
pub const DROP_LEAST_UP_M: f32 = 12.0;
/// In the air on foot after leaving an aircraft in flight (it jumped), it steers to its goal if it is this near, m. The jump lasts
/// until it stops coming down, and at least this long, s (out of the door it does not fall yet).
pub const FALL_STEER_M: f32 = 200.0;
pub const FALL_GRACE_S: f64 = 2.0;
pub const DROP_M: f32 = 15.0;
pub const DROP_LEAD_S: f32 = 0.5;
pub const HELI_LAND_M: f32 = 40.0;
pub const HELI_GET_OFF_M: f32 = 110.0;
pub const GO_ROUND_WAIT_S: f64 = 0.8;
/// B03: without a step in this long it counts as stuck against the hull, s.
pub const GO_ROUND_STILL_S: f64 = 0.8;
pub const GO_ROUND_SIDE_S: f64 = 4.0;

/// What can be changed of a player: its difficulty and, to measure what each thing contributes, some combat parts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub profile: Profile,
    /// does not fire if a wall hides the target
    pub line: bool,
    /// chooses the weapon by target
    pub melee_weapon: bool,
    /// avoids the place where its own have just fallen
    pub flanking: bool,
    /// B02: the tank waits for its on-foot escort when it lags behind
    pub escort: bool,
    /// B14: turns to the enemy shots it hears (when hit and nobody is in sight, and to a near one when it has no target)
    pub heard: bool,
    /// B14: takes a fixed gun at hand when an enemy is in its reach
    pub fixed: bool,
    /// B01: the bots' driving: a car turns on the move and brakes only for a mate in its lane ([`super::body::driver::Driver::drive_fine`])
    pub gear: bool,
    /// from a plane or helicopter, sees any part of an enemy's body, not only the chest ([`crate::bot_api::BotView::body_seen`])
    pub body: bool,
    /// B26: at the controls of a plane, fights what flies ([`super::body::dogfight`])
    pub dogfight: bool,
    /// B26: the side shares out the enemy aircraft among its fighters ([`super::team::Team::engage`]); off (the default), each
    /// pilot picks its own. `bots:air-task` turns it on.
    /// TODO: off because it does not pay in rounds. Against bots without it (`tournament3 --a bots:air-task --b bots`, 16 rounds
    /// of 8 min per map, card B26) the side with it downs as many aircraft in DC_No_Fly_Zone (1.25 against
    /// 1.44 per round) and fewer in DC_No_Fly_Zone_Day2 (1.8 against 2.7), whether the order goes before the pilot's own pick, after
    /// it, or the fighters gather on the nearest enemy instead of sharing them out. What is not known is why an order costs hits
    /// in Day2 (26 against 31–35): look at how long a fighter flies towards a sighting that is no longer there.
    pub air_task: bool,
    /// a craft its throttle lifts off the ground (GC's fighters, the Falcon) takes off straight up and slides out of a hangar
    /// ([`super::body::pilot::Pilot::vtol`]); `bots:no-vtol` takes the runway like a plane
    pub vtol: bool,
    /// a ground vehicle that swims crosses the water straight where the vehicle path map has no way ([`super::body::driver::Driver::swim`]);
    /// `bots:no-swim` waits for a way, as on its own
    pub swim: bool,
    /// at the wheel with no way to the goal on the vehicle path map, it gets off after [`NO_WAY_S`]; `bots:no-noway` waits for the
    /// jam, as on its own
    pub no_way: bool,
    /// at the wheel of a ground vehicle that does not get away from one spot for [`JAM_S`], it gets off; `bots:no-jam` keeps trying
    pub jam: bool,
    /// at the wheel, a point of the path within reach is passed only if the next one is in sight over the map
    /// ([`super::body::driver::Driver::sight`]); `bots:no-sight` passes every point closer than 12 m, as on its own
    pub sight: bool,
    /// `bots:cover-fight`: before stopping to fight it goes to a firing spot a few metres away, and takes cover where
    /// [`crate::bots::tactics::COVER_SPOT`] says ([`super::life::Bot::cover_spot`]); off, it fights where it stands
    pub cover_fight: bool,
    /// `bots:cover-points`: its position queries take the level's cover points first ([`crate::bots::tactics::Ctx::cover_points`])
    pub cover_points: bool,
    /// `bots:cover-near`, `cover-keep`, `cover-path` and `prone-fight` ([`super::life::Bot::fight_tweaks`]): how `cover-fight` picks,
    /// keeps and walks to its firing spot, and lying down to fight far
    pub fight_tweaks: super::life::FightTweaks,
    /// `bots:fire-rhythm`, `friend-gate`, `look-all`, `quick-react`, `fire-first` ([`super::body::shooter::FpsTweaks`]): how it fires on foot
    pub fps: super::body::shooter::FpsTweaks,
    /// `bots:flank2`: only the flanker its side names flanks ([`Individual::flank2_point`]); the halves by number do not
    pub flank2: bool,
    /// `bots:flank-shuffle`: every bot that flanks by its number draws its side without streaks ([`super::streetwise::Filtered`]), as
    /// the streetwise do, instead of by its number
    pub flank_shuffle: bool,
    /// `bots:cover-flank`: the flanker waits for a mate of its squad at a firing spot (or [`COVER_WAIT_S`]); whoever is at one fires
    /// at where the enemy was while it goes round, and prefers the enemy nearest its way ([`super::team::CoverFlank`])
    pub cover_flank: bool,
    /// `bots:escort2`: on foot behind its leader's tank, it keeps to a point [`ESCORT_BEHIND_M`] behind it (away from the flag), not to
    /// the tank itself, until the tank is near the flag
    pub escort2: bool,
    /// `bots:streetwise`: its side gives it the streetwise trait (high one in four, [`super::streetwise`]), and its flank side is
    /// drawn without streaks
    pub streetwise: bool,
    /// `bots:dialogue` (with `streetwise`): two-line exchanges on the radio about what it does ([`super::radio::St`])
    pub dialogue: bool,
    /// with `streetwise`: the levers that did not pass alone, kept to be tried together ([`OPPORTUNIST_ON`], [`STEAL_ON`],
    /// [`AMBUSH_ON`] say why): `bots:opportunist`, `bots:steal`, `bots:ambush`
    pub opportunist: bool,
    /// `bots:opportunist-all`: every bot, not only the streetwise, weighs how vulnerable an enemy is (busy, just off a vehicle, or
    /// looking away) when it picks its target, with
    /// [`OPPORTUNIST_ALL_W`] plus its trait times [`OPPORTUNIST_ALL_TRAIT`] ([`super::body::shooter::Opportunist`])
    pub opportunist_all: bool,
    pub steal: bool,
    /// `bots:steal-flank` (with `steal`): only the one on its way round the flank steals, and the enemy's vehicle is one its side saw
    /// an enemy drive ([`super::team::Team::enemy_vehicles`]), not one near an enemy flag
    pub steal_flank: bool,
    /// `bots:traffic-mines-spread` (with `streetwise`): its mines on the enemy's traffic go along it, not across
    pub traffic_spread: bool,
    /// `bots:ambush-own` (with `ambush`): it lies in wait only at a habit nearer its side's flags than the enemy's (the one behind the
    /// enemy's lines died there)
    pub ambush_own: bool,
    /// `bots:retreat-mate` (with `streetwise`): with the trait high, it takes cover by a mate who sees the enemy's way to it
    pub retreat_mate: bool,
    pub ambush: bool,
    /// the named parameters ([`crate::bots::params`]) as they reach this bot: the layers and what the mind's name forces
    pub params: crate::bots::params::BotParams,
    /// the mind's name in the fire trace (`bots:` and its variants), to tell the two sides of a round apart
    pub label: &'static str,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { profile: Profile::Normal, line: true, melee_weapon: true, flanking: true, escort: true, heard: true, fixed: true, gear: true, body: true, dogfight: true, air_task: false, vtol: true, swim: true, no_way: true, jam: true, sight: true, cover_fight: false, cover_points: false, fight_tweaks: super::life::FightTweaks::DEFAULT, fps: super::body::shooter::FpsTweaks::DEFAULT, flank2: false, flank_shuffle: false, cover_flank: false, escort2: false, streetwise: false, dialogue: false, opportunist: false, opportunist_all: false, steal: false, steal_flank: false, traffic_spread: false, ambush_own: false, retreat_mate: false, ambush: false, params: crate::bots::params::BotParams::default(), label: "bots" }
    }
}

/// The anti-tank weapon fixes on a ground vehicle before a soldier closer than this, m.
pub const VEHICLE_AT_M: f32 = 80.0;
/// The probability of throwing a grenade when starting a burst against a vehicle nearby (against a soldier, as usual).
pub const P_GRENADE_VEHICLE: f32 = 0.3;
/// How long the wish for another weapon must last before changing, and how long it keeps the one it has taken, s.
pub const WEAPON_WISH_S: f32 = 0.4;
pub const WEAPON_KEEP_S: f32 = 2.0;

/// A dangerous place closer than this to its goal or to it is not avoided (it cannot be), m.
pub const AVOID_FREE_M: f32 = 20.0;
/// The squad half that flanks starts closer than this to the flag, goes to a point at this distance from it and at these degrees
/// from the leader's side, m and degrees.
pub const FLANK_START_M: f32 = 150.0;
pub const FLANK_M: f32 = 45.0;
pub const FLANK_DEGREES: f32 = 75.0;

/// A bots player.
pub struct Individual {
    profile: Profile,
    settings: Settings,
    body: Bot,
    /// goes behind its leader (between `FOLLOW_FAR` and `FOLLOW_NEAR`)
    following: bool,
    /// the seat it was given and since when it goes to it
    seat_requested: Option<(usize, f64)>,
    /// the vehicle it no longer returns to in this life (it got off or could not board)
    gives_up: Option<usize>,
    /// the vehicle it gave up that it put among the body's vetoed, to take it out when it gives up another
    vetoed_given_up: Option<usize>,
    /// the vehicle it is in and since when
    boarded: Option<(usize, f64)>,
    /// as passenger, since when nobody is at the wheel
    without_driver_since: Option<f64>,
    /// B21: since when its ground vehicle stands beside the flag it attacks, outside its radius
    parked_since: Option<f64>,
    /// at the wheel of a ground vehicle, the spot it has not got away from, the seconds it has tried to near it and the last tick ([`JAM_M`], [`JAM_S`])
    jammed: Option<([f32; 2], f64, f64)>,
    /// the last time it was aboard an aircraft in flight, until it is down again
    aloft_at: Option<f64>,
    // ---- survival: the action it holds and since when
    action: Action,
    action_since: f64,
    /// the last time it saw an enemy or lost health, and where it saw the last one
    contact_t: f64,
    enemy: Option<[f32; 3]>,
    /// the health of the previous tick (to notice a shot)
    health: f32,
    /// what it carries (by its kit): medic, engineer...
    kind: Option<Kind>,
    /// whom it heals, which medic it goes to and which vehicle it repairs: player or vehicle, where and how many metres (and the wounded's health, or the vehicle's radius)
    wounded: Option<(usize, [f32; 3], f32, f32)>,
    /// the one it heals is a caller the team sent it to
    heal_called: bool,
    medic: Option<(usize, [f32; 3], f32)>,
    damaged: Option<(usize, [f32; 3], f32, f32)>,
    /// when it looks again at who is wounded, who is a medic and which vehicle is damaged
    look_t: f64,
    /// the medic it no longer goes to (does not heal it) and until when; since when it has waited beside one
    medic_veto: Option<(usize, f64)>,
    beside_medic: Option<f64>,
    /// the hidden spot it goes to, when it looks for another, since when it is in it, and until when it does not look (none)
    cover: Option<[f32; 3]>,
    cover_search: f64,
    under_cover: Option<f64>,
    no_cover_until: f64,
    /// `bots:tank-cover`: the spot hidden from the enemy tank it cannot hurt, and the tank's player, where he was when it chose the
    /// spot, when it last saw him near and since when it hides; when it looks for a spot next and until when it does not hide
    tank_cover: Option<TankCover>,
    tank_search: f64,
    tank_rest_until: f64,
    /// where it retreats to and where the enemy was when it thought of it
    retreat: Option<([f32; 3], [f32; 3])>,
    /// until when it reacts to a shot with no target, where it looks (the enemy it sees) or, if it sees nobody, to which side it steps away;
    /// and why: [`Reaction`]
    reaction: Option<(f64, Option<[f32; 3]>, f32, Reaction)>,
    /// the last situation it looked at (for the trace)
    last: Option<Situation>,
    /// the vehicle it got off to repair, and when
    repair_own: Option<(usize, f64)>,
    // ---- difficulty
    /// the weapon (slot) the target asks for and since when it asks for it; the one it has asked of the body and since when; until when it does not
    /// count on the pistol (empty)
    weapon_wish: (Option<usize>, f64),
    requested_weapon: (Option<usize>, f64),
    pistol_empty_until: f64,
    /// the flag it attacks by a flank, the flank point and whether it has already passed it
    flank: Option<(usize, [f32; 3], bool)>,
    /// `bots:cover-flank`: when it set off on its flank
    flank_t: f64,
    /// `bots:dialogue`: the vehicle it went to steal; and the enemy it last said "fire!" on
    stolen: Option<usize>,
    finish_said: Option<usize>,
    /// the places it avoids now
    avoiding: [Option<[f32; 2]>; 4],
    // ---- squad movement
    /// going round its vehicle: since when and to which side (0: still checking whether it pushes against the hull)
    detour: Option<(f64, f32)>,
    /// waiting for the escort since when (and whether it has tired of waiting: then, since when it does not wait)
    escort: Option<(f64, bool)>,
    /// going to its seat by the paths, still since when; the vehicle it goes to in a straight line (it did not advance by the paths)
    walking_still: Option<usize>,
    /// where and when it last took a step ([`SEAT_STEP_M`]): how long it has not moved, whatever its speed says
    last_step: (f64, [f32; 3]),
    straight_seat: Option<usize>,
    /// when it next listens for a shot to turn to (four times a second)
    heard_t: f64,
    /// the fixed gun it goes to and since when; when it next looks for one; the guns it gave up on this life
    fixed: Option<(usize, f64)>,
    fixed_look_t: f64,
    fixed_vetoed: Vec<usize>,
    /// `bots:streetwise`: its trait, and the generator and history of its flank side (its own, so the rest of its draws stay as they were)
    sw: Option<(f32, super::body::rng::Rng, super::streetwise::Filtered)>,
    /// `bots:flank-shuffle`: the generator and history of its flank side without the trait
    shuffle: Option<(super::body::rng::Rng, super::streetwise::Filtered)>,
    /// `bots:streetwise`: it has just set off to flank (told once, [`Individual::flank_started`])
    flank_new: bool,
    /// `bots:streetwise`: the ambush it lies in wait in, and when it may set another
    ambush: Option<Ambush>,
    ambush_next: f64,
    /// M2: why it stands still on purpose this tick, if it does ([`Individual::waits`])
    waits: Option<crate::bot_api::Wait>,
}

impl Individual {
    pub fn new(map: MapProfile, maps: Arc<Maps>, player: usize, seed: u64) -> Individual {
        Individual {
            profile: Profile::default(),
            settings: Settings::default(),
            body: { let mut b = Bot::new(map, maps, player, seed); b.go_round_hull = true; b.driver_mut().go_round_hull = true; b },
            following: false,
            seat_requested: None,
            gives_up: None,
            vetoed_given_up: None,
            boarded: None,
            without_driver_since: None,
            parked_since: None,
            jammed: None,
            aloft_at: None,
            action: Action::GoToGoal,
            action_since: -100.0,
            contact_t: -100.0,
            enemy: None,
            health: 1.0,
            kind: None,
            wounded: None,
            heal_called: false,
            medic: None,
            damaged: None,
            look_t: 0.0,
            medic_veto: None,
            beside_medic: None,
            cover: None,
            cover_search: 0.0,
            under_cover: None,
            no_cover_until: 0.0,
            tank_cover: None,
            tank_search: 0.0,
            tank_rest_until: 0.0,
            retreat: None,
            reaction: None,
            last: None,
            repair_own: None,
            weapon_wish: (None, -100.0),
            requested_weapon: (None, -100.0),
            pistol_empty_until: -100.0,
            flank: None,
            flank_t: 0.0,
            stolen: None,
            finish_said: None,
            avoiding: [None; 4],
            detour: None,
            escort: None,
            walking_still: None,
            last_step: (0.0, [0.0; 3]),
            straight_seat: None,
            heard_t: 0.0,
            fixed: None,
            fixed_look_t: 0.0,
            fixed_vetoed: Vec::new(),
            sw: None,
            shuffle: None,
            flank_new: false,
            ambush: None,
            ambush_next: 0.0,
            waits: None,
        }
    }

    /// The map profile chosen from what is seen on starting (DC and GC mods, [`super::mods`]).
    pub fn set_map_profile(&mut self, p: MapProfile) {
        self.body.set_profile(p);
    }

    /// With another difficulty or other parts.
    /// M2: why it stood still on purpose at its last think (lying in wait at its ambush spot, a flanker waiting for cover), if it did.
    pub fn waits(&self) -> Option<crate::bot_api::Wait> {
        self.waits
    }

    pub fn with_settings(mut self, a: Settings) -> Individual {
        self.profile = a.profile;
        self.body.cover_fight = a.cover_fight;
        self.body.cover_points = a.cover_points;
        self.body.fight_tweaks = a.fight_tweaks;
        self.body.set_fps(a.fps, &a.params);
        self.body.trace_label = a.label;
        self.settings = a;
        self
    }











    /// The enemy it has in sight (what it would tell its team).
    pub fn target(&self) -> Option<Target> {
        self.body.target()
    }

    /// Its side tells it that it is not getting anywhere on foot: it runs for a while in a direction that looks free.
    pub fn break_out(&mut self, v: &BotView) -> bool {
        self.body.break_out(v)
    }

    /// B26: at the controls of a fighter in flight (the pilot's seat of a plane with guns that fire ahead and nothing to drop).
    pub fn flies_a_fighter(&self, v: &BotView) -> bool {
        self.settings.dogfight && self.settings.air_task && v.me.inside.as_ref().is_some_and(|i| i.seat == 0 && i.class == VehicleClass::Plane && super::body::dogfight::fighter(i))
    }

    /// B26: the enemy aircraft its pilot has in sight (who flies each, where), and the one it has fixed on.
    pub fn air_picture(&self, t: f64) -> (Vec<(usize, [f32; 3])>, Option<usize>) {
        let p = self.body.pilot();
        (p.enemies_in_sight(t), p.lock.map(|l| l.player))
    }
}
