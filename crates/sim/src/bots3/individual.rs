//! The player: at each moment it chooses what to do among the things a person would do (fight, flank, take cover, retreat, heal,
//! repair, board a vehicle, drive, mine, go to the goal), according to its health and ammunition, whom it sees, who walks nearby and what
//! its team has ordered. Urgency weighs more than the token: leaving alone with a vehicle to save a flag is justified.
//! Once the action is chosen, it carries it out with the body's skills. It never knows more than it sees ([`BotView`]).
//!
//! Survival: it chooses by utility ([`Action::utility`], [`choose`]) with hysteresis; urgency rules (with an urgent order it
//! does not take cover, retreat or linger healing). Fighting and going to the goal belong to the body (as in `bots2`
//! with the team's order); taking cover and retreating, looking for the medic and healing, repairing what is worth it and reacting to a
//! shot without seeing where it comes from are new. On its own ([`Role::Free`]): as `bots2`, nothing more.
// TODO: combat (flanking, weapon by target) and difficulty.
use super::body::{Maps, MapProfile};
use super::body::weapons;
use super::body::shooter::Target;
use super::body::pilot::{FlightEnd, FlightOrder, Pilot};
use super::body::{Choice, CHEST, TURN, angle_between, flat, no_fall};
use super::team::{Goal, Order, Role, Kind};
use super::profile::Profile;
use crate::bot_api::{BotView, Mind, Other, VehicleClass, VehicleSeen};
use crate::bots2::Bot;
use crate::soldier::axis;
use std::sync::Arc;

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
pub const HEALTH_LOW: f32 = 0.3;
/// With the enemy closer than this it fights (fleeing with its back turned at 15 m is dying), m.
pub const FIGHT_NEAR: f32 = 20.0;
/// Below this health it goes to the medic if there is one near.
pub const HEALTH_MEDIC: f32 = 0.5;
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
/// Whoever has broken contact goes back to its business after this long without seeing anyone, s.
pub const ENEMY_MEMORY: f32 = 12.0;
/// Hysteresis: it changes action only for one it likes this much more...
pub const MARGIN: f32 = 0.1;
/// …and not more than once every so many s (unless the current one no longer makes sense: utility 0).
pub const CHANGE_S: f64 = 1.0;
/// Where it looks for a hidden spot: how many metres, how many points, and for how long before giving up, s.
pub const COVER_M: [f32; 3] = [10.0, 17.0, 25.0];
pub const COVER_POINTS: usize = 10;
pub const COVER_S: f64 = 10.0;
/// In the hidden spot it stays at most this, s (then it retreats or goes back to its business).
pub const UNDER_COVER_S: f64 = 15.0;
/// It moves away from the enemy until this, m.
pub const RETREAT_M: f32 = 70.0;
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
    pub fn utility(&self, s: &Situation) -> f32 {
        let near = s.enemy_m.is_some_and(|d| d < FIGHT_NEAR);
        let seen = s.enemy_m.is_some();
        let calm_one = s.enemy_m.is_none_or(|d| d > CALM_M);
        let cautious = s.on_foot && !s.urgent && !near;
        match self {
            Action::GoToGoal => {
                if s.urgent {
                    0.95
                } else {
                    0.5
                }
            }
            Action::Fight => {
                if near {
                    1.0
                } else if seen {
                    0.7
                } else {
                    0.0
                }
            }
            // with little health and the enemy in sight, but not on top: breaks contact
            // TODO: with full health it fights wherever it stands, so it stops in the middle of a street (a tester's
            // marks in DC_Al_Nas); weigh moving to cover a few metres away (COVER_M) before opening fire, by profile.
            Action::TakeCover if cautious && s.health < s.low_health && seen && !s.no_cover => 0.9,
            // with little health after a contact (or with nowhere to take cover): takes cover or moves away towards its own
            // (with the enemy in sight and nowhere to hide, turning its back is worse than fighting)
            Action::Retreat if cautious && s.health < s.low_health && s.since_enemy < ENEMY_MEMORY => {
                if seen {
                    0.6
                } else {
                    0.8
                }
            }
            // wounded, with a medic near and no fight in sight; with little health and a recent contact, it breaks contact first, unless
            // the medic is at hand
            Action::FindMedic if cautious && s.health < HEALTH_MEDIC && !seen && s.medic_m.is_some_and(|d| d < MEDIC_BESIDE || s.health >= s.low_health || s.since_enemy >= ENEMY_MEMORY) => {
                0.75 + (HEALTH_MEDIC - s.health) * 0.4
            }
            Action::Heal if s.on_foot && !s.urgent && s.called && !near => s.wounded.map_or(0.0, |(d, _)| 0.85 - d / 600.0),
            Action::Heal if s.on_foot && !s.urgent && calm_one => s.wounded.map_or(0.0, |(d, h)| 0.6 + (1.0 - h) * 0.3 - d / 600.0),
            Action::Repair if s.on_foot && !s.urgent && calm_one && s.damaged_m.is_some() => 0.75,
            _ => 0.0,
        }
    }
}

/// The action it is due: the one with most utility in the [`MENU`], unless the current one (`current`, chosen at `since`) still makes sense and the
/// new one does not beat it by [`MARGIN`] or [`CHANGE_S`] has not passed.
pub fn choose(s: &Situation, current: Action, since: f64, t: f64) -> Action {
    let mut best = (MENU[0], MENU[0].utility(s));
    for a in &MENU[1..] {
        let u = a.utility(s);
        if u > best.1 {
            best = (*a, u);
        }
    }
    let now = current.utility(s);
    if best.0 == current || (now > 0.0 && (best.1 < now + MARGIN || t - since < CHANGE_S)) {
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
        Command { order, urgent, passengers_out, accompanied, medics: Ensemble::default(), engineers: Ensemble::default(), dangers: [None; 4], reserved: [usize::MAX; 8], air_target: None, air_pass: None, air_engage: None, air_taken: [usize::MAX; 6], escort_m: None, call_medic: None, call_repairs: None, call_person: false, leg_vehicle: None, wing_vehicle: None, leg_landing: None, leg_drop: None }
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
    /// B01: bots3's driving: a car turns on the move and brakes only for a mate in its lane ([`super::body::driver::Driver::drive_fine`])
    pub gear: bool,
    /// from a plane or helicopter, sees any part of an enemy's body, not only the chest ([`crate::bot_api::BotView::body_seen`])
    pub body: bool,
    /// B26: at the controls of a plane, fights what flies ([`super::body::dogfight`])
    pub dogfight: bool,
    /// B26: the side shares out the enemy aircraft among its fighters ([`super::team::Team::engage`]); off (the default), each
    /// pilot picks its own. `bots3:air-task` turns it on.
    /// TODO: off because it does not pay in rounds. Against bots3 without it (`tournament3 --a bots3:air-task --b bots3`, 16 rounds
    /// of 8 min per map, card B26) the side with it downs as many aircraft in DC_No_Fly_Zone (1.25 against
    /// 1.44 per round) and fewer in DC_No_Fly_Zone_Day2 (1.8 against 2.7), whether the order goes before the pilot's own pick, after
    /// it, or the fighters gather on the nearest enemy instead of sharing them out. What is not known is why an order costs hits
    /// in Day2 (26 against 31–35): look at how long a fighter flies towards a sighting that is no longer there.
    pub air_task: bool,
    /// a craft its throttle lifts off the ground (GC's fighters, the Falcon) takes off straight up and slides out of a hangar
    /// ([`super::body::pilot::Pilot::vtol`]); `bots3:no-vtol` takes the runway like a plane
    pub vtol: bool,
    /// a ground vehicle that swims crosses the water straight where the vehicle path map has no way ([`super::body::driver::Driver::swim`]);
    /// `bots3:no-swim` waits for a way, as bots2
    pub swim: bool,
    /// at the wheel with no way to the goal on the vehicle path map, it gets off after [`NO_WAY_S`]; `bots3:no-noway` waits for the
    /// jam, as bots2
    pub no_way: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { profile: Profile::Normal, line: true, melee_weapon: true, flanking: true, escort: true, heard: true, fixed: true, gear: true, body: true, dogfight: true, air_task: false, vtol: true, swim: true, no_way: true }
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

/// A bots3 player.
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
    /// the vehicle it is in and since when
    boarded: Option<(usize, f64)>,
    /// as passenger, since when nobody is at the wheel
    without_driver_since: Option<f64>,
    /// B21: since when its ground vehicle stands beside the flag it attacks, outside its radius
    parked_since: Option<f64>,
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
            boarded: None,
            without_driver_since: None,
            parked_since: None,
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
            retreat: None,
            reaction: None,
            last: None,
            repair_own: None,
            weapon_wish: (None, -100.0),
            requested_weapon: (None, -100.0),
            pistol_empty_until: -100.0,
            flank: None,
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
        }
    }

    /// The map profile chosen from what is seen on starting (DC and GC mods, [`super::mods`]).
    pub fn set_map_profile(&mut self, p: MapProfile) {
        self.body.set_profile(p);
    }

    /// With another difficulty or other parts.
    pub fn with_settings(mut self, a: Settings) -> Individual {
        self.profile = a.profile;
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

    /// One tick: the axes it presses. The order is taken to the body (where to, with whom, in which vehicle); the how is the body's.
    /// [`Role::Free`]: goes on its own, like a player with no squad (as bots2).
    pub fn think(&mut self, v: &BotView, command: &Command) -> Vec<(usize, f32)> {
        let o = command.order;
        if v.me.inside.is_some() || flat(v.me.pos, [self.last_step.1[0], self.last_step.1[2]]) > SEAT_STEP_M {
            self.last_step = (v.time, v.me.pos);
        }
        if self.kind.is_none() && !v.me.items.is_empty() {
            self.kind = Some(Kind::de(&v.me.items));
        }
        let c = &mut self.body;
        // does not fire at a wall (on its own: as bots2, nothing more)
        c.shooter_mut().check_line = o.role != Role::Free && self.settings.line;
        // from the air it sees a head or a shoulder, as a person does, not only the chest
        c.shooter_mut().see_body = o.role != Role::Free && self.settings.body && v.me.inside.as_ref().is_some_and(|i| i.class.flies());
        // from a gunner's seat of an aircraft: what flies, led (not on its own: as bots2)
        c.shooter_mut().gunner = (o.role != Role::Free && self.settings.dogfight && v.me.inside.as_ref().is_some_and(|i| i.class.flies() && i.seat != 0)).then(|| c.shooter_mut().gunner.take().unwrap_or_default());
        // difficulty (on its own: as bots2)
        let (reaction, error) = if o.role == Role::Free { (super::body::shooter::MEDIAN_REACTION, super::body::shooter::AIM_ERROR) } else { (self.profile.reaction_s(), self.profile.error_degrees()) };
        c.shooter_mut().median_reaction = reaction;
        c.shooter_mut().error_degrees = error;
        c.break_contact = false;
        c.driver_mut().drive_fine = self.settings.gear && o.role != Role::Free;
        c.driver_mut().swim = self.settings.swim && o.role != Role::Free;
        // goes round the hull to a seat (bots2, which a free player plays as, walks straight at it)
        c.go_round_hull = o.role != Role::Free;
        c.driver_mut().go_round_hull = o.role != Role::Free;
        // B20: shut in by the path map (a depot with one door), it looks for the way out
        c.walker_mut().seeks_way_out = o.role != Role::Free;
        c.trade.heal = None;
        c.trade.repair = None;
        if o.role == Role::Free {
            let shooter = c.shooter_mut();
            shooter.prefers_vehicle_m = 0.0;
            shooter.wanted_slot = None;
            shooter.p_grenade = super::body::shooter::P_GRENADE;
            c.imposed = None;
            c.impose_point(None);
            c.seat_spot = None;
            c.wait_until = 0.0;
            c.no_own_vehicle = false;
            c.get_off_now = false;
            c.land_at = None;
            c.pilot_mut().flight = None;
            c.reserved_mut().clear();
            // even with no orders (maps with no flags to take, like the air-only or sea-only ones), whoever flies goes where the radio tells it
            let flies = v.me.inside.as_ref().is_some_and(|i| i.seat == 0 && i.class.flies());
            c.pilot_mut().imposed_goal = command.air_target.filter(|_| flies).map(|b| [b[0], b[2]]);
            c.pilot_mut().attack_run = true;
            c.pilot_mut().dogfight = self.settings.dogfight;
            c.pilot_mut().vtol = self.settings.vtol;
            engage(c.pilot_mut(), command);
            pass(c.pilot_mut(), command, v);
            return c.think(v);
        }
        // what the side has given to others it does not take
        let r = c.reserved_mut();
        r.clear();
        r.extend(command.reserved.iter().copied().filter(|&x| x != usize::MAX));
        let me = &v.me;
        let t = v.time;
        let flags = v.flags();
        // ---- where to
        let mut point = None;
        let meta: Option<[f32; 3]> = match o.goal {
            Goal::Flag(i) if i < flags.len() => {
                c.imposed = Some(Choice { flag: i, attacks: flags[i].team != me.team });
                Some(flags[i].pos)
            }
            Goal::Flag(_) => {
                c.imposed = None;
                None
            }
            Goal::Point(p) => {
                c.imposed = None;
                let y = v.ground(p[0], p[1]).unwrap_or(me.pos[1]);
                point = Some(([p[0], y, p[1]], 12.0));
                point.map(|x| x.0)
            }
        };
        // (in the air on foot out of an aircraft in flight, it comes down towards its goal if it is near: it jumped over it)
        let jumped = me.inside.is_none() && self.aloft_at.is_some_and(|a| t - a < FALL_GRACE_S || Pilot::in_the_air(v));
        if me.inside.is_none() && !jumped {
            self.aloft_at = None;
        }
        c.fall_towards = meta.filter(|p| jumped && flat(me.pos, [p[0], p[2]]) < FALL_STEER_M);
        // ---- with whom: on foot, behind the leader who goes on foot
        // (unless it cannot get there on foot: its path has just run off the edge of what it stands on, a platform far above the
        // ground like GC_Bespin's bases; then it takes a vehicle of its own, one that flies too, and does not stay with the leader)
        let at_edge_on_foot = me.inside.is_none() && v.time - c.walker_mut().edge_t < EDGE_S;
        c.no_own_vehicle = o.role == Role::Leader && command.accompanied && !at_edge_on_foot;
        // half the squad enters the flag it attacks from another side than the leader
        let flank = self.flank_move(v, command);
        let c = &mut self.body;
        if let Some(p) = flank {
            self.following = false;
            c.no_own_vehicle = true;
            point = Some((p, 8.0));
        } else if at_edge_on_foot {
            self.following = false;
        } else if let Role::Follow(j) = o.role
            && !self.flank.is_some_and(|f| f.2)
        {
            match v.others().into_iter().find(|x| x.player == j && x.team == me.team) {
                Some(x) if me.inside.is_none() && (!x.in_vehicle || x.vehicle.and_then(|k| v.vehicle(k)).is_some_and(|k| matches!(k.class, VehicleClass::Tank | VehicleClass::Walker))) => {
                    // (behind the leader's tank, somewhat farther: so it does not run it over and covers it)
                    let (far, near, r) = if x.in_vehicle { (TANK_FAR, TANK_NEAR, 10.0) } else { (FOLLOW_FAR, FOLLOW_NEAR, 6.0) };
                    let d = flat(me.pos, [x.pos[0], x.pos[2]]);
                    if d > far {
                        self.following = true;
                    } else if d < near {
                        self.following = false;
                    }
                    c.no_own_vehicle = true;
                    if self.following {
                        point = Some((x.pos, r));
                    }
                }
                _ => self.following = false,
            }
        }
        // ---- what to do (survival): by utility, and what is neither going to the goal nor fighting changes where it goes
        let action = if me.inside.is_none() { self.decide(v, command, meta) } else { Action::GoToGoal };
        if me.inside.is_some() {
            self.action = Action::GoToGoal;
            self.reaction = None;
        }
        let about_its_business = matches!(action, Action::GoToGoal | Action::Fight);
        if let Some(p) = self.execute(v, action) {
            point = Some(p);
        }
        let c = &mut self.body;
        c.impose_point(point);
        // ---- in which vehicle
        let seat_spot = match o.role {
            Role::Drive(j) => Some((j, 0)),
            Role::Passenger(j, s) => Some((j, s)),
            _ => None,
        }
        .filter(|(j, _)| self.gives_up != Some(*j));
        match (&me.inside, seat_spot) {
            (None, Some((j, s))) => {
                // (the trip's boat or aircraft may be hundreds of metres away: the patience counts once beside it)
                let far_leg = (command.leg_vehicle == Some(j) || command.wing_vehicle == Some(j)) && v.vehicle(j).is_some_and(|x| flat(me.pos, [x.pos[0], x.pos[2]]) > x.radius + SEAT_WALKING_M);
                if self.seat_requested.is_none_or(|p| p.0 != j) || far_leg {
                    self.seat_requested = Some((j, t));
                }
                // (its seat taken by one of its own, the other left free: it goes anyway; on boarding it falls into whichever remains and swaps
                // to its own when the other moves to the one it is due, like the passenger who has fallen to the wheel)
                let seat = v.vehicle(j).filter(|x| x.health > 0.1 && (x.team == 0 || x.team == me.team)).and_then(|x| {
                    if x.free_seats.contains(&s) {
                        Some(s)
                    } else if s != 0 {
                        x.free_seats.iter().copied().find(|&q| q != 0)
                    } else if !x.free_seats.is_empty() {
                        Some(s)
                    } else {
                        None
                    }
                });
                if seat.is_none() || self.seat_requested.is_some_and(|p| t - p.1 > SEAT_PATIENCE) {
                    self.gives_up = Some(j);
                    c.seat_spot = None;
                } else {
                    c.seat_spot = seat.map(|s| (j, s));
                    c.no_own_vehicle = true;
                }
            }
            (Some(ins), _) => c.seat_spot = seat_spot.filter(|p| p.0 == ins.vehicle),
            (None, None) => c.seat_spot = None,
        }
        if !about_its_business && me.inside.is_none() {
            // (healing, going to the medic or under cover it does not go to its seat)
            c.seat_spot = None;
        }
        // B14: a fixed gun at hand with an enemy in its reach: it takes it (the body fires from it and gets out after a while)
        let own: Vec<[f32; 3]> = flags.iter().filter(|f| f.team == me.team).map(|f| f.pos).collect();
        let fixed = if me.inside.is_none() && about_its_business && self.body.seat_spot.is_none() && !command.urgent && self.settings.fixed { self.fixed_weapon(v, &own) } else { None };
        let c = &mut self.body;
        if let Some(g) = fixed {
            c.seat_spot = Some((g, 0));
            c.no_own_vehicle = true;
        }
        // repaired (or time up) the vehicle it got off, it boards again if still free
        if let Some((j, since)) = self.repair_own {
            match &me.inside {
                Some(_) => self.repair_own = None,
                None if t - since > REPAIR_OWN_S + SEAT_PATIENCE => self.repair_own = None,
                None if about_its_business => {
                    if v.vehicle(j).is_some_and(|x| x.driver_free && x.health > 0.2 && (x.team == 0 || x.team == me.team)) {
                        c.seat_spot = Some((j, 0));
                        c.no_own_vehicle = true;
                    }
                }
                None => {}
            }
        }
        // far from its seat, it walks by the paths to the vehicle (in a straight line it stays against a wall or a house); on the
        // side, to the entrance
        // (if by the paths it takes no step in a while, because there is no path to the vehicle's spot, it goes straight)
        // (it is the distance moved that counts, not the speed: pushing against something or turning on the spot the speed jitters
        // over and under any threshold and reset the count, so it stood 14 s at 12 m from the vehicle)
        // (and if straight it takes no step either, it is shut in: on DC_Coastal_Hammer the helicopter's pilot appears in a barracks
        // beside it and pushed against its wall until a passenger took the controls; it gets out as from a place the map shuts)
        if me.inside.is_none()
            && let Some((j, _)) = c.seat_spot
            && self.straight_seat == Some(j)
            && t - self.last_step.0 > SEAT_SHUT_IN_S
            && v.vehicle(j).is_some_and(|x| flat(me.pos, [x.pos[0], x.pos[2]]) > x.radius + SEAT_WALKING_M)
        {
            self.straight_seat = None;
            self.walking_still = None;
            c.break_out(v);
        }
        if me.inside.is_none()
            && let Some((j, _)) = c.seat_spot
            && let Some(x) = v.vehicle(j)
            && flat(me.pos, [x.pos[0], x.pos[2]]) > x.radius + SEAT_WALKING_M
            && self.straight_seat != Some(j)
        {
            match self.walking_still {
                Some(k) if k == j => {
                    if t - self.last_step.0 > SEAT_NO_WAY_S {
                        self.straight_seat = Some(j);
                    }
                }
                _ => {
                    self.walking_still = Some(j);
                    self.last_step = (t, me.pos);
                }
            }
            if self.straight_seat != Some(j) {
                c.seat_spot = None;
                c.no_own_vehicle = true;
                c.impose_point(Some((x.pos, x.radius + 2.0)));
            }
        }
        // ---- inside: the driver waits for its own; the passenger gets off on arrival
        c.wait_until = 0.0;
        c.get_off_now = false;
        if me.inside.is_none() {
            self.parked_since = None;
        }
        match &me.inside {
            Some(ins) => {
                if self.boarded.is_none_or(|s| s.0 != ins.vehicle) {
                    self.boarded = Some((ins.vehicle, t));
                    self.without_driver_since = None;
                }
                let since = self.boarded.map_or(t, |s| s.1);
                // (B01, the first flag: the cars that stood 20-30 s at the spawn were not waiting but jammed: stopped to turn, which
                // a car cannot do, and braking for the squad walking beside it; [`Driver::drive_fine`]. What is left is no jam:
                // El_Alamein's nearest flag is ~680 m away, ~75 s at the driver's top speed with boarding and taking it; in Kursk and
                // Basrahs both teams meet at the middle flag and fight there; Medina_Ridge has no neutral flag)
                // TODO: passengers that cannot reach their seat of an airship (its door over the void at the platform's edge) stand on
                // its hull while the pilot waits; when the wait runs out it lifts off and they fall 260 m (GC_Bespin, 1–2 a round of the
                // 2.3 left). The pilot should not lift off with a mate standing on it, or the passenger step off to the floor. How:
                // `BYTEFIELD_BOTS3_TRACE=1 examples/deaths data/game GCMOD GC_Bespin --a bots3 --b bots2 --minutes 2 --seed 3`
                // (players 23 and 31: `Passenger(7, …)` at the airship's own position, y 282, then `Follow` and the fall).
                if ins.seat == 0 && command.passengers_out > 0 && !command.urgent {
                    c.wait_until = since + self.profile.passengers_wait_s() as f64;
                }
                if c.target().is_some() {
                    self.contact_t = t;
                }
                // the engineer gets off to repair its damaged vehicle if nobody shoots at it (and stops it itself, if driving)
                let on_ground = matches!(ins.class, VehicleClass::Tank | VehicleClass::Car | VehicleClass::Walker);
                let stopped = (ins.velocity[0].powi(2) + ins.velocity[2].powi(2)).sqrt() < 2.0;
                if self.kind == Some(Kind::Engineer) && on_ground && !command.urgent && t - self.contact_t > REPAIR_CALM_S && (ins.seat == 0 || stopped) && v.vehicle(ins.vehicle).is_some_and(|x| x.health > 0.2 && x.health < HEALTH_GET_OFF_TO_REPAIR && !x.fixed) {
                    self.repair_own = Some((ins.vehicle, t));
                }
                if self.repair_own.is_some_and(|r| r.0 == ins.vehicle) {
                    c.get_off_now = true;
                }
                // B21: its ground vehicle stands beside the flag it attacks and not inside it, with nobody to shoot at (what is in
                // the way of a vehicle, a camp or a wreck around the flag, is not in the way of a soldier): on foot from there.
                // Not where the driver waits outside the flag on purpose: on Berlin it cost the side its tanks' guns (bots3
                // against bots2, four rounds of four lower, 11 to 39 tickets)
                let attacked = if let Goal::Flag(i) = o.goal { flags.get(i).filter(|f| f.team != me.team) } else { None };
                let parked = on_ground && stopped && c.enters_at_once() && c.target().is_none() && attacked.is_some_and(|f| (PARKED_IN * f.radius..PARKED_M).contains(&flat(ins.pos, [f.pos[0], f.pos[2]])));
                self.parked_since = if parked { self.parked_since.or(Some(t)) } else { None };
                if self.parked_since.is_some_and(|s| t - s > PARKED_S) {
                    c.get_off_now = true;
                    self.gives_up = Some(ins.vehicle);
                }
                if ins.seat == 0 && on_ground && self.settings.no_way && o.role != Role::Free && c.driver_mut().no_way.is_some_and(|s| t - s > NO_WAY_S) {
                    c.driver_mut().no_way = None;
                    c.get_off_now = true;
                    self.gives_up = Some(ins.vehicle);
                }
                // B14/B16: at a fixed gun with nobody to shoot at for a while, it gets off and goes on (sitting there is standing still)
                if ins.seat == 0 && ins.class == VehicleClass::Fixed && t - since > FIXED_NO_TARGET_S && t - self.contact_t > FIXED_NO_TARGET_S {
                    c.get_off_now = true;
                }
                // at the wheel of a tank, outside combat, it does not leave its escort on foot behind
                let escort_wait = ins.seat == 0 && matches!(ins.class, VehicleClass::Tank | VehicleClass::Walker) && self.settings.escort && !command.urgent && c.target().is_none() && command.escort_m.is_some_and(|d| d > ESCORT_WAIT_M && d < ESCORT_FAR_M);
                match (escort_wait, self.escort) {
                    (true, None) => self.escort = Some((t, false)),
                    (true, Some((since, false))) if t - since > ESCORT_WAIT_S => self.escort = Some((t, true)),
                    (_, Some((since, true))) if t - since > ESCORT_REST_S => self.escort = None,
                    (false, Some((_, false))) => self.escort = None,
                    _ => {}
                }
                if escort_wait && self.escort.is_some_and(|e| !e.1) {
                    c.wait_until = c.wait_until.max(t + 0.5);
                }
                // with the vehicle damaged and an engineer of its own at hand, it stops for it to be fixed
                if ins.seat == 0 && !command.urgent && c.target().is_none() {
                    let touched = v.vehicle(ins.vehicle).is_some_and(|x| x.health > 0.2 && x.health < HEALTH_REPAIR && !x.fixed);
                    if touched && v.others().iter().any(|x| x.team == me.team && !x.in_vehicle && command.engineers.has(x.player) && flat(ins.pos, [x.pos[0], x.pos[2]]) < 40.0) {
                        c.wait_until = c.wait_until.max(t + 0.5);
                    }
                }
                // in a helicopter, landed (near the ground and almost still)
                let flies = ins.class.flies();
                let height = ins.pos[1] - v.ground(ins.pos[0], ins.pos[2]).unwrap_or(f32::MIN).max(v.water());
                // (still and high over the ground: perhaps on a dock or a deck; only then the line tests of `floor`)
                let still = ins.velocity.iter().map(|x| x * x).sum::<f32>() < 9.0;
                let landed = still && (height < LANDED_M || (flies && v.floor(ins.pos[0], ins.pos[2], ins.pos[1]).is_some_and(|f| ins.pos[1] - f < LANDED_M)));
                if flies && height > DROP_LEAST_UP_M {
                    self.aloft_at = Some(t);
                }
                // in command of a transport helicopter: waits landed for its own, carries them and lands near the flag; once
                // empty and landed there, it gets off too and follows on foot with them
                c.land_at = None;
                c.pilot_mut().flight = None;
                // at the controls of something that flies, to the enemy the side has reported (if not, from flag to flag)
                c.pilot_mut().imposed_goal = command.air_target.filter(|_| flies && ins.seat == 0).map(|b| [b[0], b[2]]);
                c.pilot_mut().attack_run = true;
                c.pilot_mut().dogfight = self.settings.dogfight;
                c.pilot_mut().airship = true;
                c.pilot_mut().vtol = self.settings.vtol;
                engage(c.pilot_mut(), command);
                pass(c.pilot_mut(), command, v);
                // (also a follower that took the controls of its squad's leg aircraft: flying on its own it went off to another flag)
                if ins.class.hovers() && ins.seat == 0 && (matches!(o.role, Role::Drive(_) | Role::Leader) || command.leg_vehicle == Some(ins.vehicle)) && !command.urgent {
                    let x = v.vehicle(ins.vehicle);
                    let carrying = x.as_ref().is_some_and(|x| x.free_seats.len() + 1 < x.seats);
                    if t < c.wait_until {
                        c.pilot_mut().flight = Some(FlightOrder { to: ins.pos, end: FlightEnd::Land, hold: true });
                    } else if let Some(m) = meta {
                        let d = flat(ins.pos, [m[0], m[2]]);
                        // the trip's air leg lands where it was chosen (before taking off: beside the flag, not on it), with or without
                        // people; without a trip, whoever carries people lands on the straight line, short of the flag
                        let leg = command.leg_landing.filter(|_| command.leg_vehicle == Some(ins.vehicle));
                        if carrying || leg.is_some() {
                            let k = (HELI_LAND_M / d.max(1.0)).min(1.0);
                            let to = leg.unwrap_or([m[0] + (ins.pos[0] - m[0]) * k, m[1], m[2] + (ins.pos[2] - m[2]) * k]);
                            c.pilot_mut().flight = Some(FlightOrder { to, end: FlightEnd::Land, hold: false });
                        }
                        // TODO: once its people are off the pilot gets off too and follows them on foot; a player would fly back for
                        // another squad or stay over them in support (the helicopter waits by the flag with nobody to use it)
                        if !carrying && landed && d < HELI_GET_OFF_M && self.boarded.is_some_and(|s| t - s.1 > 20.0) {
                            c.get_off_now = true;
                            self.gives_up = Some(ins.vehicle);
                        }
                    }
                }
                // the trip's air leg in a plane: it flies over the point, above it, and everyone in it jumps there (the pilot too: its
                // squad goes on on foot, and a plane does not wait)
                if let Some(to) = command.leg_drop.filter(|_| ins.class == VehicleClass::Plane && command.leg_vehicle == Some(ins.vehicle)) {
                    if ins.seat == 0 {
                        let p = c.pilot_mut();
                        p.flight = Some(FlightOrder { to: [to[0], to[1] + DROP_UP_M, to[2]], end: FlightEnd::Over, hold: false });
                        p.imposed_goal = None;
                        p.attack_run = false;
                        p.dogfight = false;
                    }
                    let speed = ins.velocity[0].hypot(ins.velocity[2]);
                    if flat(ins.pos, [to[0], to[2]]) < DROP_M + DROP_LEAD_S * speed && ins.pos[1] - to[1] > DROP_LEAST_UP_M {
                        c.get_off_now = true;
                        self.gives_up = Some(ins.vehicle);
                    }
                }
                // from a helicopter landed near its goal anyone who does not pilot it gets off (even outside its seat)
                if ins.class.hovers() && ins.seat != 0 && landed && meta.is_some_and(|p| flat(ins.pos, [p[0], p[2]]) < HELI_GET_OFF_M) {
                    c.get_off_now = true;
                    self.gives_up = Some(ins.vehicle);
                }
                if ins.seat != 0 && c.seat_spot.is_some() {
                    let sin = v.vehicle(ins.vehicle).is_some_and(|x| x.driver_free);
                    if !sin {
                        self.without_driver_since = None;
                    } else if self.without_driver_since.is_none() {
                        self.without_driver_since = Some(t);
                    }
                    // (from a helicopter it gets off when landed, near: in the air, jumping is killing yourself)
                    let arrived = if flies { landed && meta.is_some_and(|p| flat(ins.pos, [p[0], p[2]]) < HELI_GET_OFF_M) } else { meta.is_some_and(|p| flat(ins.pos, [p[0], p[2]]) < GET_OFF_NEAR) };
                    // (whoever boards without a driver waits for one to arrive, but not forever)
                    let alone = self.without_driver_since.is_some_and(|s| t - s > 8.0) && (!flies || landed);
                    if arrived || alone {
                        c.get_off_now = true;
                        self.gives_up = Some(ins.vehicle);
                    }
                }
            }
            None => self.boarded = None,
        }
        if me.inside.is_none() {
            self.choose_weapon(v);
            self.avoid(v, command, meta);
        }
        // boarded in another seat than its own (on entering you take the one nearest to hand): the passenger who has fallen to the wheel does not
        // set off and moves to its seat; the driver who has fallen in the back moves to the wheel
        let c = &mut self.body;
        let change = match (&me.inside, c.seat_spot) {
            (Some(ins), Some((j, s))) if j == ins.vehicle && s != ins.seat && (ins.seat == 0 || s == 0) => {
                // (at the wheel without being the driver: to its seat or, if taken, to another passenger one; if none is left, it drives itself)
                v.vehicle(j).and_then(|x| if x.free_seats.contains(&s) { Some(s) } else if s != 0 { x.free_seats.iter().copied().find(|&q| q != 0) } else { None })
            }
            _ => None,
        };
        if change.is_some_and(|s| s != 0) {
            c.wait_until = c.wait_until.max(t + 0.5);
        }
        let mut axes = c.think(v);
        if let Some(s) = change {
            axes.retain(|e| !(crate::entry::MENU_SELECT1..crate::entry::MENU_SELECT1 + 9).contains(&e.0) && e.0 != crate::entry::USE);
            axes.push((crate::entry::MENU_SELECT1 + s, 1.0));
        }
        self.go_round(v, &mut axes);
        self.react(v, &mut axes);
        if super::trace_shots() && me.inside.is_none() && axes.iter().any(|e| e.0 == axis::FIRE && e.1 > 0.0) {
            let b = self.body.target();
            let crouched = axes.iter().any(|e| e.0 == axis::CROUCH && e.1 > 0.0);
            let h = b.map(|b| (v.hidden(me.eye, b.1), v.hidden([me.eye[0], me.eye[1] - 0.4, me.eye[2]], b.1)));
            eprintln!("shot t={t:.2} p={} {:?} crouched {crouched} weapon {:?} d {:?} eye {:?} pos {:?} hidden {:?} cover {:?} following {} carries {:?} target_in_vehicle {:?} bullets {:?}", me.player, self.action, me.weapon, b.map(|b| b.2 as i32), me.eye.map(|x| (x * 10.0).round() / 10.0), me.pos.map(|x| (x * 10.0).round() / 10.0), h, self.under_cover.is_some(), self.following, me.items.iter().map(|i| i.1.as_str()).collect::<Vec<_>>(), b.and_then(|b| v.others().into_iter().find(|o| o.player == b.0)).map(|o| o.in_vehicle), me.rounds);
        }
        let c = &self.body;
        if super::trace_wounded() && me.health < HEALTH_LOW && (t * 30.0).round() as i64 % 30 == 0
            && let Some(s) = self.last
        {
            eprintln!("wounded t={t:.0} p={} {:?} health {:.2} urgent {} enemy {:?} since {:.0} medic {:?} cover {:?} break {} inside {}", me.player, self.action, me.health, s.urgent, s.enemy_m.map(|x| x as i32), s.since_enemy, s.medic_m.map(|x| x as i32), self.cover.map(|p| flat(me.pos, [p[0], p[2]]) as i32), c.break_contact, me.inside.is_some());
        }
        if super::trace_me() == Some(me.player) && (t * 30.0).round() as i64 % 15 == 0 {
            let pv = match o.role { Role::Drive(j) | Role::Passenger(j, _) => v.vehicle(j).map(|x| (x.template, x.pos.map(|q| q as i32), x.radius as i32, x.free_seats)), _ => None };
            eprintln!("me t={t:.1} pos {:?} {:?} {:?} health {:.2} seat {:?} inside {:?} gives_up {:?} go_round {:?} veh {:?} wait {:.1} outside {} escort {:?} step {:?} path {} flight {:?} axes {:?}", me.pos.map(|x| x as i32), o.role, self.action, me.health, c.seat_spot, me.inside.as_ref().map(|i| (i.vehicle, i.seat)), self.gives_up, self.detour, pv, (c.wait_until - t).max(0.0), command.passengers_out, self.escort, c.walker().point.map(|p| p.map(|x| x as i32)), c.walker().path.len(), c.pilot().flight.map(|f| (f.to.map(|x| x as i32), f.end)), axes);
            if let Some(s) = self.last.filter(|s| s.health < HEALTH_MEDIC) {
                eprintln!("   sit urgent {} enemy {:?} since {:.1} no_cover {} medic {:?} cover {:?} retreat {:?}", s.urgent, s.enemy_m, s.since_enemy, s.no_cover, s.medic_m, self.cover.map(|p| p.map(|x| x as i32)), self.retreat.map(|p| p.0.map(|x| x as i32)));
            }
        }
        axes
    }

    /// What it sees and what the team told it, in a [`Situation`]; chooses the action ([`choose`]).
    fn decide(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>) -> Action {
        let me = &v.me;
        let t = v.time;
        let target = self.body.target();
        if let Some(b) = target {
            self.contact_t = t;
            self.enemy = Some(b.1);
        }
        // a shot is also a contact (and it reacts)
        if me.health < self.health - 0.001 {
            self.contact_t = t;
            // (a hit overrides turning to a shot heard)
            if target.is_none() && self.reaction.is_none_or(|r| t >= r.0 || r.3 == Reaction::Heard) {
                let seen = enemy_visible(v, REACTION_SIGHT_M);
                let heard = if seen.is_none() && self.settings.heard { enemy_heard(v, REACTION_SIGHT_M, 0.0) } else { None };
                let look_at = seen.or(heard);
                if let Some(e) = look_at {
                    self.enemy = Some(e);
                }
                let side_ = if (me.player + (t * 2.0) as usize) % 2 == 0 { 1.0 } else { -1.0 };
                self.reaction = Some((t + REACTION_S, look_at, side_, if heard.is_some() { Reaction::ShotHeard } else { Reaction::Hit }));
            }
            // the hidden spot was not there: another place
            if self.under_cover.is_some() {
                self.cover = None;
                self.under_cover = None;
            }
        }
        self.health = me.health;
        // B14: with no target and nothing to react to, it turns to an enemy shot heard near (a person looks where the shooting is)
        if self.settings.heard && target.is_none() && self.reaction.is_none_or(|r| t >= r.0 + HEARD_CALM_S) && !command.urgent && t >= self.heard_t
            && let Some(e) = { self.heard_t = t + 0.25; enemy_heard(v, HEARD_TURN_M, HEARD_RECENT_S) }
        {
            self.reaction = Some((t + HEARD_TURN_S, Some(e), 1.0, Reaction::Heard));
        }
        let others = v.others();
        self.look(v, &others, command);
        let s = Situation {
            health: me.health,
            ammo: me.rounds,
            enemies_seen: target.is_some() as usize,
            mates_near: others.iter().filter(|x| x.team == me.team && flat(me.pos, [x.pos[0], x.pos[2]]) < 30.0).count(),
            goal_distance: meta.map_or(0.0, |p| flat(me.pos, [p[0], p[2]])),
            order: command.order,
            urgent: command.urgent,
            on_foot: me.inside.is_none(),
            enemy_m: target.map(|b| b.2),
            since_enemy: (t - self.contact_t) as f32,
            no_cover: t < self.no_cover_until,
            wounded: self.wounded.map(|h| (h.2, h.3)),
            called: self.heal_called,
            medic_m: self.medic.map(|m| m.2),
            damaged_m: self.damaged.map(|d| d.2),
            low_health: self.profile.low_health(),
        };
        let a = choose(&s, self.action, self.action_since, t);
        self.last = Some(s);
        if a != self.action {
            if super::trace_actions() {
                eprintln!("action t={t:.1} p={} {:?} -> {:?} health {:.2} enemy {:?} wounded {:?} medic {:?} damaged {:?}", me.player, self.action, a, me.health, s.enemy_m.map(|x| x as i32), s.wounded.map(|h| (h.0 as i32, h.1)), s.medic_m.map(|x| x as i32), s.damaged_m.map(|x| x as i32));
            }
            self.action = a;
            self.action_since = t;
            self.cover = None;
            self.under_cover = None;
            self.retreat = None;
            self.beside_medic = None;
        }
        a
    }

    /// Who is wounded (if a medic), which medic it has nearby (if not) and which vehicle with its own inside is damaged (if
    /// an engineer). The list is rebuilt twice a second; where each one is, on every tick.
    fn look(&mut self, v: &BotView, others: &[Other], command: &Command) {
        let me = &v.me;
        let t = v.time;
        let d = |p: [f32; 3]| flat(me.pos, [p[0], p[2]]);
        let mate = |j: usize| others.iter().find(|x| x.player == j && x.team == me.team && !x.in_vehicle);
        // where they are now
        self.wounded = self.wounded.and_then(|h| mate(h.0).and_then(|x| Some((h.0, x.pos, d(x.pos), x.health?)))).filter(|h| h.3 < HEALTH_WOUNDED_HIGH);
        self.medic = self.medic.and_then(|m| mate(m.0).map(|x| (m.0, x.pos, d(x.pos))));
        if t < self.look_t {
            return;
        }
        self.look_t = t + 0.5;
        let medic = self.kind == Some(Kind::Medic);
        // B15: who called "medic!" goes first, if still hurt (as a mate with little health, a call weighs no more)
        let call = command.call_medic.filter(|_| medic).and_then(mate).filter(|x| x.health.is_some_and(|h| h < HEALTH_WOUNDED_HIGH) && d(x.pos) < super::radio::DISPATCH_M);
        // (only a person's call: between bots, going with an enemy in sight cost reactions to fire (B14, 86 -> 76 %) and the
        // callers that live 20 s were already attended 93 % of the time)
        self.heal_called = call.is_some() && command.call_person;
        self.wounded = if let Some(x) = call {
            Some((x.player, x.pos, d(x.pos), x.health.unwrap_or(1.0)))
        } else if medic {
            others
                .iter()
                .filter(|x| x.team == me.team && !x.in_vehicle && x.health.is_some_and(|h| h < HEALTH_HEAL) && d(x.pos) < HEAL_RADIUS)
                .min_by(|a, b| d(a.pos).total_cmp(&d(b.pos)).then(a.player.cmp(&b.player)))
                .map(|x| (x.player, x.pos, d(x.pos), x.health.unwrap_or(1.0)))
        } else {
            None
        };
        let veto = self.medic_veto.filter(|m| t < m.1).map(|m| m.0);
        self.medic = if !medic && me.health < HEALTH_MEDIC {
            others
                .iter()
                .filter(|x| x.team == me.team && !x.in_vehicle && command.medics.has(x.player) && Some(x.player) != veto && d(x.pos) < MEDIC_RADIUS)
                .min_by(|a, b| d(a.pos).total_cmp(&d(b.pos)).then(a.player.cmp(&b.player)))
                .map(|x| (x.player, x.pos, d(x.pos)))
        } else {
            None
        };
        let own_one = self.repair_own.filter(|r| t - r.1 < REPAIR_OWN_S).map(|r| r.0);
        // B15: the vehicle whose crew called "need repairs" goes first, if still damaged
        let call = command.call_repairs.filter(|_| self.kind == Some(Kind::Engineer)).and_then(|k| v.vehicle(k)).filter(|x| x.team == me.team && x.health > 0.0 && x.health < HEALTH_REPAIRED && x.radius < 25.0 && d(x.pos) < super::radio::ANSWER_M);
        self.damaged = if let Some(x) = call {
            Some((x.vehicle, x.pos, d(x.pos), x.radius))
        } else if self.kind == Some(Kind::Engineer) {
            v.vehicles()
                .into_iter()
                .filter(|x| (x.team == me.team || (x.team == 0 && Some(x.vehicle) == own_one)) && x.health > 0.0 && x.radius < 25.0 && d(x.pos) < REPAIR_RADIUS)
                .filter(|x| if Some(x.vehicle) == own_one { x.health < 0.9 } else { !x.driver_free && x.health < HEALTH_REPAIR })
                .min_by(|a, b| d(a.pos).total_cmp(&d(b.pos)).then(a.vehicle.cmp(&b.vehicle)))
                .map(|x| (x.vehicle, x.pos, d(x.pos), x.radius))
        } else {
            None
        };
    }

    /// What the action does with the body; returns the point it goes to (centre and radius) if not the order's.
    fn execute(&mut self, v: &BotView, a: Action) -> Option<([f32; 3], f32)> {
        let me = &v.me;
        let t = v.time;
        match a {
            Action::TakeCover => {
                let e = self.enemy?;
                if self.cover.is_none() && t >= self.cover_search {
                    self.cover_search = t + 1.0;
                    self.cover = find_cover(v, e);
                    if self.cover.is_none() {
                        // nowhere: moves away (Retreat) for a while
                        self.no_cover_until = t + 5.0;
                    }
                }
                self.under_cover(v)
            }
            Action::Retreat => {
                // first under cover from the last enemy it saw, if it has it nearby; if not, far, towards its own
                let e = self.enemy.filter(|e| flat(me.pos, [e[0], e[2]]) < 80.0);
                if self.cover.is_none()
                    && self.retreat.is_none()
                    && let Some(e) = e
                {
                    self.cover = find_cover(v, e);
                }
                if self.cover.is_some() {
                    return self.under_cover(v);
                }
                let still_valid = self.retreat.is_some_and(|r| e.is_none_or(|e| flat(r.1, [e[0], e[2]]) < 20.0));
                if !still_valid {
                    self.retreat = retreat_point(v, e).map(|p| (p, e.unwrap_or(me.pos)));
                }
                let r = self.retreat?;
                self.body.break_contact = true;
                Some((r.0, 6.0))
            }
            Action::FindMedic => {
                let m = self.medic?;
                // beside it: waits for it to heal, but not forever
                if m.2 < 8.0 {
                    let since = *self.beside_medic.get_or_insert(t);
                    if t - since > 20.0 {
                        self.medic_veto = Some((m.0, t + 40.0));
                        self.medic = None;
                        self.beside_medic = None;
                    }
                } else {
                    self.beside_medic = None;
                }
                Some((m.1, 3.0))
            }
            Action::Heal => {
                let h = self.wounded?;
                self.body.trade.heal = Some(h.0);
                Some((h.1, 3.0))
            }
            Action::Repair => {
                let r = self.damaged?;
                self.body.trade.repair = Some(r.0);
                Some((r.1, r.3 + 2.0))
            }
            _ => None,
        }
    }

    /// B14: the fixed gun it goes to now, if any: a free one of its team (or nobody's) within [`FIXED_M`], not up high, with an enemy
    /// plane within [`FIXED_PLANE_M`] of it, or, the gun guarding one of its team's flags (`own`, within [`FIXED_FLAG_M`]), an
    /// enemy soldier in sight within [`FIXED_TARGET_M`]. An attacker does not stop at a machine gun on the way: it slows the attack down (the
    /// tournament lost ~8 tickets with any gun at hand). It keeps going to it for [`FIXED_GO_S`]; one it did not get into is not for this life.
    // TODO: bot_caps counts 33 boardings and 3 kills on the base-game census (its gate asks 50 boardings). Ways: an AA gun with an
    // enemy plane in sight farther than FIXED_PLANE_M, or a guard that waits at the gun without seeing anyone (B16 «still» would show it).
    fn fixed_weapon(&mut self, v: &BotView, own: &[[f32; 3]]) -> Option<usize> {
        let me = &v.me;
        let t = v.time;
        let free = |j: usize| v.vehicle(j).filter(|x| x.fixed && x.driver_free && (x.team == 0 || x.team == me.team) && x.health > 0.3);
        if let Some((j, since)) = self.fixed {
            if t - since < FIXED_GO_S && free(j).is_some() {
                return Some(j);
            }
            self.fixed = None;
            self.fixed_vetoed.push(j);
        }
        if t < self.fixed_look_t {
            return None;
        }
        self.fixed_look_t = t + 1.0;
        let high = |x: &crate::bot_api::VehicleSeen| v.ground(x.pos[0], x.pos[2]).is_none_or(|g| x.pos[1] - g > super::body::driver::FIXED_HEIGHT);
        let weapons: Vec<crate::bot_api::VehicleSeen> = v.vehicles().into_iter().filter(|x| x.fixed && x.driver_free && (x.team == 0 || x.team == me.team) && x.health > 0.3 && !self.fixed_vetoed.contains(&x.vehicle) && flat(me.pos, [x.pos[0], x.pos[2]]) < FIXED_M && !high(x)).collect();
        let g = weapons.iter().min_by(|a, b| flat(me.pos, [a.pos[0], a.pos[2]]).total_cmp(&flat(me.pos, [b.pos[0], b.pos[2]])).then(a.vehicle.cmp(&b.vehicle)))?;
        let plane = v.others().iter().filter(|o| o.team != me.team).filter_map(|o| o.vehicle).filter_map(|k| v.vehicle(k)).any(|x| x.class.flies() && flat(g.pos, [x.pos[0], x.pos[2]]) < FIXED_PLANE_M);
        let keeps = own.iter().any(|f| flat(g.pos, [f[0], f[2]]) < FIXED_FLAG_M);
        // (the enemy in sight from the gun too, not only from where it stands: from a nest behind a wall it would shoot the wall)
        let from_weapon = [g.pos[0], g.pos[1] + FIXED_EYE, g.pos[2]];
        if !plane && !(keeps && enemy_visible(v, FIXED_TARGET_M).is_some_and(|e| !v.hidden(from_weapon, [e[0], e[1] + CHEST, e[2]]))) {
            return None;
        }
        self.fixed = Some((g.vehicle, t));
        Some(g.vehicle)
    }

    /// Goes to its hidden spot and stays; if there they keep seeing it (the enemy in sight for more than a second) or it has been long, another spot
    /// or retreat.
    fn under_cover(&mut self, v: &BotView) -> Option<([f32; 3], f32)> {
        let me = &v.me;
        let t = v.time;
        let c = self.cover?;
        self.body.break_contact = true;
        if flat(me.pos, [c[0], c[2]]) < 2.5 {
            let since = *self.under_cover.get_or_insert(t);
            let seen = self.body.target().is_some() && t - since > 1.0;
            if seen {
                // does not hide: another (one per second at most)
                self.cover = None;
                self.under_cover = None;
                self.cover_search = t + 1.0;
                if let Some(e) = self.body.target().map(|b| b.1) {
                    self.cover = find_cover(v, e).filter(|p| flat(*p, [c[0], c[2]]) > 3.0);
                }
                if self.cover.is_none() {
                    self.no_cover_until = t + 5.0;
                }
                return self.cover.map(|p| (p, 1.5));
            }
            if t - since > self.profile.under_cover_s() as f64 {
                self.no_cover_until = t + 5.0;
            }
        }
        Some((c, 1.5))
    }

    /// The weapon according to target. The anti-tank looks first at the ground vehicles in range and fires rockets at them; at a
    /// soldier on foot it fires with the pistol (the rocket, only if nothing else is left). Whoever carries grenades throws them more often at a
    /// vehicle nearby. Changing weapon costs: the wish must last [`WEAPON_WISH_S`] and what was asked is kept [`WEAPON_KEEP_S`].
    // TODO(perf, low value): the weapon classes come from string searches on every call (here, `is_anti_tank`, `team::kind`);
    // keeping them per kit template would save 3-5 % of a step on GC_Endor (~0.06 ms), not worth it unless they grow
    fn choose_weapon(&mut self, v: &BotView) {
        let me = &v.me;
        let t = v.time;
        let shooter = self.body.shooter_mut();
        if !self.settings.melee_weapon {
            shooter.prefers_vehicle_m = 0.0;
            shooter.wanted_slot = None;
            shooter.p_grenade = super::body::shooter::P_GRENADE;
            return;
        }
        let at = me.items.iter().find(|i| weapons::is_anti_tank(&i.1)).map(|i| i.0);
        let pistol = me.items.iter().find(|i| weapons::is_pistol(&i.1)).map(|i| i.0);
        let in_hand = super::body::shooter::slot_in_hand(me);
        if in_hand.is_some() && in_hand == pistol && me.rounds.0 + me.rounds.1 <= 0 {
            self.pistol_empty_until = t + 60.0;
        }
        shooter.prefers_vehicle_m = if at.is_some() { VEHICLE_AT_M } else { 0.0 };
        let target = shooter.target;
        let on_ground = |o: &Other| v.ground(o.pos[0], o.pos[2]).is_none_or(|h| o.pos[1] - h < 6.0);
        let vehicle = target.and_then(|b| v.others().into_iter().find(|o| o.player == b.0)).is_some_and(|o| o.in_vehicle && on_ground(&o));
        shooter.p_grenade = if vehicle && at.is_none() { P_GRENADE_VEHICLE } else { super::body::shooter::P_GRENADE };
        // (from the rocket to the pistol against a soldier, at once: it is the first thing whoever sees a soldier with a rocket in hand does)
        let wish = match target {
            Some(_) if vehicle => at,
            Some(_) if at.is_some() && in_hand == at && t >= self.pistol_empty_until => pistol,
            // (with the pistol in hand and a soldier ahead, it carries on with it)
            Some(_) if at.is_some() && in_hand == pistol && t >= self.pistol_empty_until => pistol,
            _ => None,
        };
        if wish != self.weapon_wish.0 {
            self.weapon_wish = (wish, t);
        }
        let firm = t - self.weapon_wish.1 >= WEAPON_WISH_S as f64 || (self.weapon_wish.0.is_some() && self.weapon_wish.0 == pistol && in_hand == at);
        if firm && self.weapon_wish.0 != self.requested_weapon.0 && (self.requested_weapon.0.is_none() || t - self.requested_weapon.1 >= WEAPON_KEEP_S as f64) {
            self.requested_weapon = (self.weapon_wish.0, t);
        }
        shooter.wanted_slot = self.requested_weapon.0;
        // with nobody ahead, back to the rocket (DC's pistol is not known by the body as a pistol)
        if target.is_none() && self.requested_weapon.0.is_none() && at.is_some() && in_hand.is_some() && in_hand == pistol && !shooter.requesting_weapon(v) && (t * 2.0).fract() < 0.05 {
            shooter.request(v, at.unwrap_or(weapons::PRIMARY_SLOT));
        }
    }

    /// Does not enter where two or more of its own have just fallen: the path avoids those places (except one at its goal or
    /// where it already is). If one appears near the path it follows, it thinks it again.
    fn avoid(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>) {
        let me = &v.me;
        let mut now: [Option<[f32; 2]>; 4] = [None; 4];
        if self.settings.flanking {
            let mut n = 0;
            for p in command.dangers.iter().flatten() {
                let at_goal = meta.is_some_and(|m| (m[0] - p[0]).hypot(m[2] - p[1]) < AVOID_FREE_M);
                if !at_goal && flat(me.pos, *p) > AVOID_FREE_M {
                    now[n] = Some(*p);
                    n += 1;
                }
            }
        }
        if now == self.avoiding {
            return;
        }
        let cam = self.body.walker_mut();
        let new_on_the_way = now.iter().flatten().any(|p| !self.avoiding.contains(&Some(*p)) && cam.path.iter().any(|c| (c[0] - p[0]).hypot(c[1] - p[1]) < 12.0));
        cam.avoid = now.iter().flatten().copied().collect();
        if new_on_the_way {
            cam.replan();
        }
        self.avoiding = now;
    }

    /// If it is one of those who follow the leader (half the squad, by number), and the squad goes for an enemy flag closer than
    /// [`FLANK_START_M`], it goes first to a point [`FLANK_M`] from the flag, at ±[`FLANK_DEGREES`] from the side the
    /// leader arrives by; once past it, it enters the flag. Returns the point while it goes to it.
    fn flank_move(&mut self, v: &BotView, command: &Command) -> Option<[f32; 3]> {
        let me = &v.me;
        let (Role::Follow(j), Goal::Flag(i)) = (command.order.role, command.order.goal) else {
            self.flank = None;
            return None;
        };
        let flags = v.flags();
        let f = flags.get(i)?;
        let far = flat(me.pos, [f.pos[0], f.pos[2]]);
        if !self.settings.flanking || f.team == me.team || me.inside.is_some() || me.player % 2 == 0 || command.urgent || far > FLANK_START_M + 60.0 {
            self.flank = None;
            return None;
        }
        if self.flank.is_none_or(|x| x.0 != i) {
            if far > FLANK_START_M {
                return None;
            }
            let leader = v.others().into_iter().find(|x| x.player == j && x.team == me.team)?;
            let a = (leader.pos[2] - f.pos[2]).atan2(leader.pos[0] - f.pos[0]);
            let side_ = if (me.player / 2) % 2 == 0 { 1.0 } else { -1.0 };
            let water = v.water();
            let point = [1.0f32, 0.7].iter().find_map(|k| {
                let b = a + side_ * k * FLANK_DEGREES.to_radians();
                let (x, z) = (f.pos[0] + FLANK_M * b.cos(), f.pos[2] + FLANK_M * b.sin());
                v.ground(x, z).filter(|h| *h > water + 0.3).map(|h| [x, h, z])
            });
            let Some(p) = point else {
                // (no room on that side: with the leader)
                self.flank = Some((i, f.pos, true));
                return None;
            };
            self.flank = Some((i, p, false));
        }
        let (_, p, done) = self.flank?;
        if done {
            return None;
        }
        if flat(me.pos, [p[0], p[2]]) < 10.0 || self.body.target().is_some_and(|b| b.2 < 25.0) {
            self.flank = Some((i, p, true));
            return None;
        }
        Some(p)
    }

    /// A shot with no target: looks towards the enemy it sees or, if it sees nobody, steps away running.
    /// Going to its seat, stuck close to the vehicle and not advancing (the entrance is on the other side and it pushes against the hull): slides
    /// sideways without ceasing to look at the entrance, away from the vehicle's centre, and if in a few seconds it does not manage, to the other side.
    /// The same against another vehicle in its way.
    fn go_round(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>) {
        let me = &v.me;
        let seat_spot = self.body.seat_spot.filter(|_| me.inside.is_none() && self.body.target().is_none());
        let Some(x) = seat_spot.and_then(|(j, _)| v.vehicle(j)) else {
            self.detour = None;
            return;
        };
        let still = v.time - self.last_step.0 > GO_ROUND_STILL_S;
        let pushes = axes.iter().any(|e| e.0 == axis::THROTTLE && e.1 > 0.0);
        // what it pushes against: its own vehicle or, on the way to it, another one in front (tanks parked side by side at a spawn)
        let near = |y: &VehicleSeen| flat(me.pos, [y.pos[0], y.pos[2]]) < y.radius + GO_ROUND_M;
        let obstacle = if near(&x) {
            Some(x.pos)
        } else if (still && pushes) || self.detour.is_some() {
            let heading = [x.pos[0] - me.pos[0], x.pos[2] - me.pos[2]];
            v.vehicles()
                .into_iter()
                .filter(|y| y.vehicle != x.vehicle && near(y) && (y.pos[0] - me.pos[0]) * heading[0] + (y.pos[2] - me.pos[2]) * heading[1] > 0.0)
                .min_by(|a, b| flat(me.pos, [a.pos[0], a.pos[2]]).total_cmp(&flat(me.pos, [b.pos[0], b.pos[2]])))
                .map(|y| y.pos)
        } else {
            None
        };
        let stuck_close = obstacle.is_some();
        let t = v.time;
        match self.detour {
            None if stuck_close && still && pushes => self.detour = Some((t, 0.0)),
            Some((since, 0.0)) if t - since > GO_ROUND_WAIT_S => {
                let e = obstacle.unwrap_or(x.pos);
                let centre = angle_between(me.view[2], [e[0] - me.pos[0], e[2] - me.pos[2]]);
                self.detour = Some((t, if centre > 0.0 { -1.0 } else { 1.0 }));
            }
            Some((since, l)) if l != 0.0 && t - since > GO_ROUND_SIDE_S => self.detour = Some((t, -l)),
            Some(_) if !stuck_close => self.detour = None,
            _ => {}
        }
        if let Some((_, l)) = self.detour.filter(|r| r.1 != 0.0) {
            axes.retain(|e| e.0 != axis::YAW);
            axes.push((axis::YAW, l));
        }
    }

    fn react(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>) {
        let Some((until, look_at, side_, cause)) = self.reaction else { return };
        // (whoever is already breaking contact is already moving: it does not turn to the enemy)
        // TODO: most bots that do not react in B14 (Berlin) do react, but turn towards another shot than the nearest enemy one the
        // meter hears (95-175° off: `angle=` in the `BYTEFIELD_QUALITY_B14` trace), and move only 2-3 m in the 2 s. Tried without gain
        // (12 seeds, full B14 88.2 %): the newest shot heard first on a hit (86.9 %), running on while turning (87.0 %). Next: print
        // the point the bot picks next to the meter's. See `tournament3 --first-seed`.
        if v.time >= until || self.body.target().is_some() || v.me.inside.is_some() || self.body.break_contact {
            self.reaction = None;
            return;
        }
        let me = &v.me;
        match look_at {
            Some(e) => {
                let g = angle_between(me.view[2], [e[0] - me.pos[0], e[2] - me.pos[2]]);
                axes.retain(|x| x.0 != axis::MOUSE_X);
                axes.push((axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)));
                // hit from where it only heard a shot: it turns to it and steps aside, out of the line it was hit along
                if cause == Reaction::ShotHeard {
                    axes.retain(|x| !matches!(x.0, axis::YAW | axis::CROUCH));
                    axes.push((axis::YAW, side_));
                }
            }
            None => {
                axes.retain(|x| !matches!(x.0, axis::THROTTLE | axis::YAW | axis::CROUCH));
                axes.push((axis::THROTTLE, 1.0));
                axes.push((axis::YAW, side_));
            }
        }
    }
}

/// A wounded whom the medic goes to stops being one with this health (the medkit heals them all the way up anyway).
const HEALTH_WOUNDED_HIGH: f32 = 0.8;
/// B15: a vehicle whose crew called for repairs is repaired up to this.
const HEALTH_REPAIRED: f32 = 0.9;

/// The nearest enemy it sees (of the four nearest within `r` m, the first with no wall between its eyes and its chest).
pub fn enemy_visible(v: &BotView, r: f32) -> Option<[f32; 3]> {
    let me = &v.me;
    let mut near: Vec<(f32, [f32; 3])> = v.others().into_iter().filter(|o| o.team != me.team).map(|o| (flat(me.pos, [o.pos[0], o.pos[2]]), o.pos)).filter(|x| x.0 < r).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.into_iter().take(4).find(|x| !v.hidden(me.eye, [x.1[0], x.1[1] + CHEST, x.1[2]])).map(|x| x.1)
}

/// B14: where the nearest enemy shot heard within `r` m came from (fired at most `age` s ago; 0: any it still hears). Who fired is
/// not heard: a shot from where a mate of its own team stands (within 3 m) is taken for his.
pub fn enemy_heard(v: &BotView, r: f32, age: f32) -> Option<[f32; 3]> {
    let me = &v.me;
    let heard_ones = v.heard(r);
    if heard_ones.is_empty() {
        return None;
    }
    let others = v.others();
    let own_one = |p: [f32; 3]| others.iter().any(|o| o.team == me.team && (0..3).map(|k| (o.pos[k] - p[k]).powi(2)).sum::<f32>() < 9.0);
    heard_ones.into_iter().filter(|h| (age <= 0.0 || h.age <= age) && flat(me.pos, [h.pos[0], h.pos[2]]) > 3.0 && !own_one(h.pos)).min_by(|a, b| flat(me.pos, [a.pos[0], a.pos[2]]).total_cmp(&flat(me.pos, [b.pos[0], b.pos[2]]))).map(|h| h.pos)
}

/// A spot a few metres away that a wall or the terrain hides from the enemy at `e` (its body): about ten points at three
/// distances, starting with those that leave its back to the enemy; not in the water nor behind a cliff. The first that is good.
pub fn find_cover(v: &BotView, e: [f32; 3]) -> Option<[f32; 3]> {
    let me = &v.me;
    let eye = [e[0], e[1] + 0.6, e[2]];
    let flees = (me.pos[2] - e[2]).atan2(me.pos[0] - e[0]);
    let water = v.water();
    for k in 0..COVER_POINTS {
        // 0, +36°, -36°, +72°... around the direction of flight
        let step = k.div_ceil(2) as f32 * std::f32::consts::TAU / COVER_POINTS as f32;
        let a = flees + if k % 2 == 1 { step } else { -step };
        let r = COVER_M[k % COVER_M.len()];
        let (x, z) = (me.pos[0] + r * a.cos(), me.pos[2] + r * a.sin());
        let Some(h) = v.ground(x, z) else { continue };
        if h < water + 0.3 || !no_fall(v, me.pos, [x, z]) {
            continue;
        }
        let p = [x, h + 1.0, z];
        if v.hidden(eye, [x, p[1] + CHEST, z]) {
            return Some(p);
        }
    }
    None
}

/// Where it retreats to: the nearest own flag that is farther from the enemy than it is (within 400 m) or, if none,
/// to [`RETREAT_M`] in the direction opposite the enemy (or somewhat sideways, if there is water that way).
pub fn retreat_point(v: &BotView, e: Option<[f32; 3]>) -> Option<[f32; 3]> {
    let me = &v.me;
    let de_e = |p: [f32; 3]| e.map_or(0.0, |e| flat(p, [e[0], e[2]]));
    let yo = de_e(me.pos);
    let far = |p: [f32; 3]| flat(me.pos, [p[0], p[2]]);
    let flag = v.flags().into_iter().filter(|f| f.team == me.team && far(f.pos) < 400.0 && far(f.pos) > 15.0 && (e.is_none() || de_e(f.pos) > yo + 20.0)).min_by(|a, b| far(a.pos).total_cmp(&far(b.pos)));
    if let Some(f) = flag {
        return Some(f.pos);
    }
    let e = e?;
    let flees = (me.pos[2] - e[2]).atan2(me.pos[0] - e[0]);
    let water = v.water();
    for k in [0.0f32, 0.5, -0.5, 1.0, -1.0] {
        let a = flees + k * std::f32::consts::FRAC_PI_4 * 1.5;
        let (x, z) = (me.pos[0] + RETREAT_M * a.cos(), me.pos[2] + RETREAT_M * a.sin());
        if let Some(h) = v.ground(x, z).filter(|h| *h > water + 0.3) {
            return Some([x, h, z]);
        }
    }
    None
}
