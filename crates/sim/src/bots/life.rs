//! A bot's life: the machine every team bot is built on (and what plays on its own, [`Role::Free`]). Written from the behaviour specification (`Mind`/`BotView`, without touching the rules).
//!
//! Each bot is a simple machine, rebuilt every life:
//! - **destination** ([`flags`]): a flag, attack or defend; inside it goes from point to point and now and then stops;
//! - **sight**: the nearest enemy within the map's reach ([`MapProfile`]) that `hidden` does not cover; nothing else decides who
//!   is near (nor the defence of a flag): `others` is only used passed through `hidden`, to follow the seen target and for mates;
//! - **fire** ([`weapons`]): takes time to react, aims with the mouse, and fires at its weapon's rhythm and by distance;
//!   it does not fire if a mate is on the line of fire;
//! - **vehicle**: in some lives, after a while, it boards a free ground one or a nearby fixed weapon; it gets off if it flips, burns or gets stuck;
//! - **boat**: whoever spawns aboard goes straight to the shore and disembarks ([`sailor`]).
//!
//! Added to `BotView` for this (nothing a player cannot see): [`crate::bot_api::Me::single_shot`], [`crate::bot_api::Inside::camera`]/`camera_pos`, [`crate::bot_api::VehicleSeen::fixed`], [`BotView::water`].
//!
//! Deterministic: everything comes from `BotView` and the bot's own generator, seeded with its number and the spawn instant.
use super::flags;
use crate::bots::body::{Maps, MapProfile};

use crate::bot_api::{BotView, Mind, MindMaker, VehicleClass};
use crate::entry::USE;
use crate::soldier::axis;
use crate::bots::body::{driver, pilot, rng, sailor, sapper, shooter, trade, walker, weapons};
use crate::bots::body::*;
use walker::Walker;
use driver::{STUCK, Driver};
use trade::Trade;
use sapper::Sapper;
use sailor::{Arrival, Sailor};
/// How far ahead of its step a soldier going to a seat looks for the floor, m ([`with_ground`]).
const SEAT_LOOK_M: f32 = 2.5;
use shooter::{Shooter, SHELL_SPEED, friend_in_line, in_corridor};
use rng::Rng;
use std::sync::Arc;

/// Overall settings, measured against the Berlin-8 targets.
/// An enemy closer than this to a flag threatens it, m.
pub const THREAT_RADIUS: f32 = 25.0;
/// The flag it attacks is defended if it has seen an enemy less than these seconds ago.
pub const DEFENCE_MEMORY: f64 = 45.0;
/// Seconds it watches around the flag, after arriving, before taking it for undefended.
pub const ARRIVAL_WAIT: f64 = 30.0;
/// When an attacker goes into the flag and when a defender's flag is threatened: the constants above for the plain bot; bots set
/// them from its named parameters ([`crate::bots::params::BotParams`], `enter_*` and `flag_threat_m`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Enter {
    /// it decides whether to go in once within this of the flag, m
    pub near_m: f32,
    /// [`DEFENCE_MEMORY`]
    pub memory_s: f64,
    /// [`ARRIVAL_WAIT`]
    pub watch_s: f64,
    /// [`THREAT_RADIUS`]
    pub threat_m: f32,
}

impl Default for Enter {
    fn default() -> Enter {
        Enter { near_m: 40.0, memory_s: DEFENCE_MEMORY, watch_s: ARRIVAL_WAIT, threat_m: THREAT_RADIUS }
    }
}

impl Enter {
    pub fn of(p: &crate::bots::params::BotParams) -> Enter {
        Enter { near_m: p.enter_near_m, memory_s: p.enter_memory_s, watch_s: p.enter_watch_s, threat_m: p.flag_threat_m }
    }
}

/// How far a boat fires, m.
pub const SHIP_RANGE: f32 = 700.0;

/// A team's bots on `map`.
pub fn maker(map: &str, seed: u64) -> MindMaker {
    maker_with(map, seed, Arc::new(Maps::default()))
}

/// The same, with the maps already built ([`Maps::prepare`]).
pub fn maker_with(map: &str, seed: u64, maps: Arc<Maps>) -> MindMaker {
    let profile = MapProfile::de(map);
    Box::new(move |player| Box::new(Bot::new(profile, maps.clone(), player, seed)))
}

/// The rest of the bot, lent to a skill (field by field, so the skill itself can be lent at the same time).
macro_rules! surroundings {
    ($s:expr) => {
        Surroundings { rng: &mut $s.rng, profile: &$s.profile, maps: &$s.maps, player: $s.player, shooter: &mut $s.shooter, cam: &mut $s.walker, driver: &mut $s.driver, alternate: &mut $s.alternate, flag: &mut $s.flag, owner: &mut $s.owner, centre: $s.centre, no_own_vehicle: $s.no_own_vehicle }
    };
}

/// A bot's life.
/// `bots:cover-fight`: at its firing spot within this, m.
pub const SPOT_ARRIVED_M: f32 = 1.2;
/// `bots:cover-flank`: it is still at its firing spot within this more, m.
pub const COVER_SPOT_SLACK_M: f32 = 1.5;
/// `bots:cover-fight` also takes cover where the position query says ([`Bot::cover_spot`]).
const COVER_SPOT_ON: bool = false;
/// `bots:cover-near`: no firing spot farther than this, m (on its way it kills about half what it kills stopped).
pub const COVER_NEAR_M: f32 = 4.0;
/// `bots:cover-keep`: the spot is kept this long with nobody in sight, s (it goes on to it, or holds it), and held this long once
/// reached whatever the coin of stopping says, s.
pub const KEEP_S: f64 = 3.0;
pub const HOLD_S: f64 = 3.0;
/// `bots:prone-fight`: stopped to fight an enemy farther than this, it lies down, m; and presses the lie key (a toggle) at most this
/// often, s.
pub const PRONE_M: f32 = 25.0;
const LIE_PRESS_S: f64 = 2.0;

/// The tweaks of `cover-fight`'s firing spot and of the pose fighting, each its variant (`bots:cover-near`, `cover-keep`,
/// `cover-path`, experimental and off; `prone-fight`, on by default since 1.19.0, off with `bots:no-prone-fight`); all off is the bot
/// of 1.18.0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FightTweaks {
    /// no spot farther than [`COVER_NEAR_M`]; already hidden crouched where it stands and seeing standing, it stays (a parapet here)
    pub near: bool,
    /// the spot is not dropped for a target that flickers ([`KEEP_S`]) nor for the coin of stopping on the way or just there ([`HOLD_S`])
    pub keep: bool,
    /// no spot whose straight way from here meets a wall ([`crate::bots::tactics::Ctx::straight`])
    pub path: bool,
    /// stopped to fight with the enemy past [`PRONE_M`], it lies down
    pub prone: bool,
}

impl FightTweaks {
    pub const OFF: FightTweaks = FightTweaks { near: false, keep: false, path: false, prone: false };
    /// what a bot plays with unless its name says otherwise: lying down to fight far (it beat the anchor of 1.18.0 twice, the second
    /// time with fresh seeds, +0.23 tickets per round with all eight maps above)
    pub const DEFAULT: FightTweaks = FightTweaks { prone: true, ..FightTweaks::OFF };
}

pub struct Bot {
    profile: MapProfile,
    maps: Arc<Maps>,
    player: usize,
    seed: u64,
    rng: Rng,
    spawned: Option<f64>,
    // ---- destination
    flag: Option<flags::Choice>,
    owner: u32,
    review: f64,
    centre: Option<[f32; 2]>,
    radio: f32,
    threatened: bool,
    /// the attacker has decided to enter the flag
    enters: bool,
    near_since: Option<f64>,
    /// when it goes in and when its flag is threatened
    pub enter: Enter,
    walker: Walker,
    // ---- sight and fire
    shooter: Shooter,
    /// the pulse of one tick yes and the next no, to press use and the bolt
    alternate: bool,
    // ---- vehicle
    driver: Driver,
    sailor: Sailor,
    pilot: pilot::Pilot,
    // ---- medic and engineer
    pub trade: Trade,
    sapper: Sapper,
    // ---- what is commanded from outside (bots: its team). With nothing (as usual), it chooses
    /// the flag it goes to (and whether it attacks), instead of choosing it
    pub imposed: Option<flags::Choice>,
    /// a point (centre and radius) it goes to instead of the flag: the leader it follows
    pub imposed_point: Option<([f32; 3], f32)>,
    /// the vehicle and the seat it must board
    pub seat_spot: Option<(usize, usize)>,
    /// bots: where it wants to come down when it is in the air with nothing below (it jumped): it faces there, which is where the
    /// fall and the parachute push it
    pub fall_towards: Option<[f32; 3]>,
    /// at the wheel, it does not set off before this: it waits for the passengers
    pub wait_until: f64,
    /// does not board a vehicle on its own (goes with its squad)
    pub no_own_vehicle: bool,
    /// bots: its side has given it a seat to take (it drives or rides), and it starts no chore of its own on the way there; the plain bot never
    pub seat_bound: bool,
    /// goes round the vehicle's body to a seat whose entry is on the far side (bots; the plain bot walks straight at it)
    pub go_round_hull: bool,
    /// get off the vehicle now (the passenger has arrived)
    pub get_off_now: bool,
    /// breaks contact (bots): neither fires nor heals nor boards anything; walks to its point (`imposed_point`) and, once arrived, crouches
    pub break_contact: bool,
    /// at the controls of a helicopter, land at this point instead of going to a flag (bots: carrying passengers, waiting for them)
    pub land_at: Option<[f32; 3]>,
    /// to which side it turns to go to its seat when it has it behind (if not, at ±180° it changes side on every tick and never turns)
    seat_turn: Option<f32>,
    /// bots (`bots:cover-fight`): before stopping to fight, it goes to a firing spot ([`crate::bots::tactics::FIRING_SPOT`]) a few
    /// metres away instead of crouching where it stands
    pub cover_fight: bool,
    /// `bots:cover-points`: its position queries take the level's cover points first
    pub cover_points: bool,
    /// `bots:cover-near`, `cover-keep`, `cover-path`, `prone-fight`
    pub fight_tweaks: FightTweaks,
    /// the firing spot it goes to or holds, and when it last asked for one or checked it
    fight_spot: Option<crate::bots::tactics::Spot>,
    /// `bots:cover-keep`: when it reached its spot (the spot's place and the time), and when and where it last saw its target
    spot_reached: Option<([f32; 3], f64)>,
    last_seen: Option<(f64, [f32; 3])>,
    /// `bots:prone-fight`: when it last pressed the lie key, and whether it wants to lie this tick
    lie_pressed: f64,
    lies: bool,
    /// `bots:fire-first`: it stopped this tick for a burst
    fire_stop: bool,
    spot_asked: f64,
    spot_checked: f64,
    /// `bots:cover-flank`: a mate of its squad flanks ([`crate::bots::team::CoverFlank`]): from its firing spot it fires at where the
    /// enemy was last seen while the flanker goes round; and the most rounds it has seen in its magazine
    pub cover: Option<crate::bots::team::CoverFlank>,
    mag_full: i32,
    /// `bots:retreat-mate` (a streetwise bot with the trait high): its cover is by a teammate who sees the enemy's way to it
    /// ([`crate::bots::tactics::COVER_SPOT_MATE`]), else as always
    pub retreat_mate: bool,
    /// `BYTEFIELD_SPOT_TRACE=1`: what becomes of its firing spots ([`crate::bots::spot_trace`])
    spot_trace: Option<Box<crate::bots::spot_trace::SpotTrace>>,
    /// `BYTEFIELD_FIRE_TRACE=1`: why it does not fire with an enemy in reach ([`crate::bots::fire_trace`]); the mind's name for it
    fire_trace: Option<Box<crate::bots::fire_trace::FireTrace>>,
    pub trace_label: &'static str,
    /// what its last tick on foot did (fight, walk, heal...), for the fire trace; written, never read by the bot
    branch: &'static str,
    /// the way its walk went this tick (flat), if it walked with nobody in sight (`bots:heard-walk` keeps it while it looks at a shot heard)
    pub walk_dir: Option<[f32; 2]>,
}

impl Bot {
    /// Changes what depends on the map (bots, [`crate::bots::mods`]): the plain bot does not call it.
    pub fn set_profile(&mut self, profile: MapProfile) {
        self.profile = profile;
    }

    /// Whether on this map whoever attacks a flag goes into it without waiting outside (at the wheel, it drives into its radius).
    pub fn enters_at_once(&self) -> bool {
        self.profile.enter_wait <= 0.0
    }

    pub fn new(profile: MapProfile, maps: Arc<Maps>, player: usize, seed: u64) -> Bot {
        Bot {
            profile,
            maps,
            player,
            seed,
            rng: Rng::new(0),
            spawned: None,
            flag: None,
            owner: 0,
            review: 0.0,
            centre: None,
            radio: 0.0,
            threatened: false,
            enters: false,
            near_since: None,
            enter: Enter::default(),
            walker: Walker::default(),
            shooter: Shooter::default(),
            alternate: false,
            driver: Driver::default(),
            sailor: Sailor::default(),
            pilot: pilot::Pilot::default(),
            trade: Trade::default(),
            sapper: Sapper::default(),
            imposed: None,
            imposed_point: None,
            seat_spot: None,
            fall_towards: None,
            wait_until: 0.0,
            no_own_vehicle: false,
            seat_bound: false,
            go_round_hull: false,
            get_off_now: false,
            break_contact: false,
            land_at: None,
            seat_turn: None,
            cover_fight: false,
            cover_points: false,
            fight_tweaks: FightTweaks::OFF,
            fight_spot: None,
            spot_reached: None,
            last_seen: None,
            lie_pressed: -10.0,
            lies: false,
            fire_stop: false,
            cover: None,
            mag_full: 0,
            retreat_mate: false,
            spot_asked: -10.0,
            spot_checked: 0.0,
            spot_trace: crate::bots::spot_trace::on().then(|| Box::new(crate::bots::spot_trace::SpotTrace::new(seed, player))),
            fire_trace: crate::bots::fire_trace::on().then(|| Box::new(crate::bots::fire_trace::FireTrace::new(seed, player))),
            trace_label: "bots",
            branch: "",
            walk_dir: None,
        }
    }

    /// On the first tick of the life: the seed, and whether this life will look for a vehicle.
    fn spawn(&mut self, v: &BotView) {
        let tic = (v.time * 30.0).round() as u64;
        self.rng = Rng::new(self.seed.wrapping_mul(0x1000_0000_01B3) ^ ((self.player as u64) << 32) ^ tic);
        self.spawned = Some(v.time);
        self.walker.spawn(v);
        self.shooter.spawn(v, &mut self.rng);
        self.driver.wants_fixed = self.rng.si(self.profile.p_fixed);
        self.driver.wants_fly = self.rng.si(0.5);
        if self.rng.si(self.profile.p_board) {
            self.driver.wants_board_at = Some(v.time + self.rng.lognormal(self.profile.board_wait, 0.6) as f64);
        }
        self.sapper.plan_sapper(v, &mut surroundings!(self));
    }

    /// Chooses a new destination if needed: on spawn and when the flag changes owner.
    fn destination(&mut self, v: &BotView) -> Option<(flags::Choice, [f32; 3], f32)> {
        let mut flags = v.flags();
        // a flag on top of a building is taken from inside its radius, which is 3D: from the ground it is not reached and here there is no
        // way to know the path up (Stalingrad). It is seen from outside: it is not counted on
        for f in &mut flags {
            if v.ground(f.pos[0], f.pos[2]).is_some_and(|h| f.pos[1] - h > f.radius + 2.0) {
                f.takeable = false;
            }
        }
        if let Some(e) = self.imposed.filter(|e| e.flag < flags.len()) {
            // the flag has been given: it does not choose it
            if self.flag != Some(e) {
                self.flag = Some(e);
                self.enters = false;
                self.near_since = None;
                self.owner = flags[e.flag].team;
                self.walker.new_zone();
            }
            return self.zone(&flags);
        }
        // the defender who has gone long without seeing anyone reconsiders where to go
        let bored = self.flag.is_some_and(|b| !b.attacks) && v.time - self.shooter.target_t.max(self.spawned.unwrap_or(0.0)) > self.profile.bored;
        if bored {
            self.shooter.target_t = v.time;
        }
        let still_valid = self.flag.filter(|b| !bored && flags.get(b.flag).is_some_and(|f| f.team == self.owner));
        if still_valid.is_none() || v.time >= self.review {
            let cambia = still_valid.is_none();
            if cambia {
                self.flag = flags::choose(&flags, v.me.team, v.me.pos, self.profile.p_fewer, self.profile.p_second, &mut self.rng);
                self.enters = false;
                self.near_since = None;
                self.owner = self.flag.map_or(0, |b| flags[b.flag].team);
                self.walker.new_zone();
            }
            self.review = v.time + 2.0;
        }
        self.zone(&flags)
    }

    /// The zone it goes to: its flag's or, if commanded, a point.
    fn zone(&mut self, flags: &[crate::bot_api::Flag]) -> Option<(flags::Choice, [f32; 3], f32)> {
        if let Some((c, r)) = self.imposed_point {
            self.centre = Some([c[0], c[2]]);
            self.radio = r;
            return Some((flags::Choice { flag: self.flag.map_or(0, |b| b.flag), attacks: true }, c, r));
        }
        self.centre = self.flag.map(|b| [flags[b.flag].pos[0], flags[b.flag].pos[2]]);
        self.radio = self.flag.map_or(0.0, |b| flags[b.flag].radius);
        self.flag.map(|b| (b, flags[b.flag].pos, flags[b.flag].radius))
    }

    /// The enemy it has in sight now (what it would tell over the radio).
    pub fn target(&self) -> Option<shooter::Target> {
        self.shooter.target
    }

    /// Its hands and eyes, for whoever carries it (bots: checking the line of fire, difficulty, weapon by target).
    pub fn shooter_mut(&mut self) -> &mut Shooter {
        &mut self.shooter
    }

    pub fn shooter(&self) -> &Shooter {
        &self.shooter
    }

    /// Its hands at the controls of a plane or helicopter (bots: where it goes in the air).
    pub fn pilot_mut(&mut self) -> &mut pilot::Pilot {
        &mut self.pilot
    }

    pub fn pilot(&self) -> &pilot::Pilot {
        &self.pilot
    }

    /// The vehicles its team has given to others (bots): it does not take them on its own.
    pub fn reserved_mut(&mut self) -> &mut Vec<usize> {
        &mut self.driver.reserved
    }

    /// Its trade as an engineer, to look at.
    pub fn sapper(&self) -> &sapper::Sapper {
        &self.sapper
    }

    /// Its hands on the wheel, to look at.
    pub fn driver(&self) -> &Driver {
        &self.driver
    }

    /// Its hands on the wheel (bots: [`Driver::drive_fine`]).
    pub fn maps(&self) -> &Arc<Maps> {
        &self.maps
    }
    pub fn driver_mut(&mut self) -> &mut Driver {
        &mut self.driver
    }

    /// bots: its side sees it is not getting anywhere on foot ([`Walker::break_out`]).
    pub fn break_out(&mut self, v: &BotView) -> bool {
        self.walker.break_out(v, &mut self.rng)
    }

    /// Its feet, to look at (the trace).
    pub fn walker(&self) -> &Walker {
        &self.walker
    }

    /// Its feet (bots: the places it avoids).
    pub fn walker_mut(&mut self) -> &mut Walker {
        &mut self.walker
    }

    /// Commanded by its team: a point to go to (the leader it follows); if it moves more than 8 m, it thinks another path.
    pub fn impose_point(&mut self, p: Option<([f32; 3], f32)>) {
        let far = match (self.imposed_point, p) {
            (Some((a, _)), Some((b, _))) => flat(a, [b[0], b[2]]) > 8.0,
            (None, None) => false,
            _ => true,
        };
        if far {
            self.walker.new_zone();
            self.enters = false;
            self.near_since = None;
        }
        self.imposed_point = p;
    }

    /// On foot.
    fn on_foot(&mut self, v: &BotView) -> Vec<(usize, f32)> {
        self.branch = "fall";
        self.walk_dir = None;
        if let Some(p) = self.fall_towards
            && let Some(axes) = pilot::Pilot::fall_towards(v, p)
        {
            return axes;
        }
        if let Some(axes) = pilot::Pilot::fall(v) {
            return axes;
        }
        self.branch = "leave";
        let me = &v.me;
        if let Some((dir, until)) = self.sailor.leave_by {
            if v.time < until {
                let g = angle_between(me.view[2], dir);
                return vec![(axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)), (axis::MOUSE_Y, 0.0), (axis::THROTTLE, 1.0)];
            }
            self.sailor.leave_by = None;
        }
        let others = v.others();
        // (`bots:armour`: not the enemies riding in armour it cannot hurt; the exposed gunners at their bodies)
        let targets = self.shooter.armour_view(v, me.eye, &others);
        // (`bots:fov`: only the enemies in its field of view, near it or the one it follows)
        let targets = self.shooter.fov_view(v, me.eye, targets);
        self.shooter.see(v, &mut self.rng, me.eye, self.profile.foot_range, &targets);
        self.shooter.watch_health(v);
        let mut axes = Vec::with_capacity(8);
        let f = me.view[2];
        self.branch = "break_contact";
        if self.break_contact {
            return self.under_cover(v, axes);
        }
        // (only with what it has seen: `others` also gives those a wall hides)
        let threatened = self.flag.is_some_and(|b| !b.attacks) && self.centre.is_some_and(|c| self.shooter.target.is_some_and(|x| flat(x.1, c) < self.enter.threat_m));
        if threatened && !self.threatened {
            self.walker.replan();
        }
        self.threatened = threatened;
        // whoever attacks enters at once if nobody defends it, and if not, sooner or later
        if self.flag.is_some_and(|b| b.attacks) && !self.enters && self.centre.is_some_and(|c| flat(me.pos, c) < self.enter.near_m) {
            let arrival = *self.near_since.get_or_insert(v.time);
            let defended = v.time - self.shooter.target_t < self.enter.memory_s || v.time - arrival < self.enter.watch_s;
            if !defended || self.profile.enter_wait <= 0.0 || self.rng.si(v.dt / self.profile.enter_wait) {
                self.enters = true;
                self.walker.replan();
            }
        }
        // ---- heal or repair
        self.branch = "trade";
        if let Some(axes) = self.trade.trade(v, &others, &self.profile, &mut self.driver.vetoed, &mut self.walker, &mut self.shooter) {
            return axes;
        }
        // ---- explosives
        self.branch = "sapper";
        let combat = self.shooter.target.is_some();
        // (bots: not on the way to the seat its side gave it: a sapper ordered to fly DC_No_Fly_Zone's Su-25 went for the nearest
        // vehicle to lay mines with instead, in a straight line through the wall of the control tower it spawns in)
        if !self.seat_bound
            && let Some(axes) = self.sapper.sap(v, &mut surroundings!(self), combat)
        {
            return axes;
        }
        // ---- vehicle: the one it was given, or one it looks for itself
        self.branch = "vehicle";
        // (the way to the seat is a straight line: not off a platform's edge, after a vehicle that has already taken off)
        if let Some((j, _)) = self.seat_spot
            && v.vehicle(j).is_some_and(|x| !with_ground(v, v.me.pos, [x.pos[0], x.pos[2]]))
        {
            self.seat_spot = None;
        }
        // (bots: nor the step it is taking, round the hull to a door that hangs over the void: GC_Bespin's airships stand at the
        // platform's edge, 260 m up, and three or four passengers a round walked off it going round to their seat)
        let ahead = {
            let u = v.me.velocity;
            let s = u[0].hypot(u[2]);
            [v.me.pos[0] + u[0] / s.max(0.1) * SEAT_LOOK_M, v.me.pos[2] + u[2] / s.max(0.1) * SEAT_LOOK_M]
        };
        if let Some((j, _)) = self.seat_spot
            && self.go_round_hull
            && v.me.velocity[0].hypot(v.me.velocity[2]) > 0.5
            && !with_ground(v, v.me.pos, ahead)
        {
            self.seat_spot = None;
            self.driver.vetoed.push(j);
            return vec![(axis::THROTTLE, 0.0)];
        }
        if let Some((j, seat)) = self.seat_spot
            && self.shooter.target.is_none()
        {
            self.alternate = !self.alternate;
            let mut axes = if self.go_round_hull { v.axes_to_seat_round(j, seat, self.alternate) } else { v.axes_to_seat(j, seat, self.alternate) };
            if let Some(m) = axes.iter_mut().find(|e| e.0 == axis::MOUSE_X) {
                if m.1.abs() >= 5.0 {
                    m.1 = *self.seat_turn.get_or_insert(m.1.signum()) * m.1.abs();
                } else {
                    self.seat_turn = None;
                }
            }
            return axes;
        }
        if self.shooter.target.is_none()
            && !self.no_own_vehicle
            && v.time >= self.sapper.on_foot_until
            && let Some(axes) = self.driver.go_to_vehicle(v, &self.profile, &mut self.alternate)
        {
            return axes;
        }
        let step = match self.destination(v) {
            Some((e, centre, radio)) => self.walker.next_step(v, &mut self.rng, &self.maps, e.attacks, centre, radio, self.enters || self.imposed_point.is_some(), self.threatened),
            None => None,
        };
        let class = weapons::class(me.weapon.as_deref().unwrap_or(""), me.single_shot);
        let dir = step.map(|p| [p[0] - me.pos[0], p[1] - me.pos[2]]);
        let keeping = self.shooter.target.is_none() && self.keeps_spot(v);
        let recall_at = if self.shooter.target.is_none() && !keeping { self.shooter.recall(v, &others) } else { None };
        match self.shooter.target {
            Some((_, at, d)) => {
                // ---- combat
                if v.time >= self.shooter.decide_combat {
                    // whoever has not yet arrived at its flag stops less
                    let far = self.centre.is_some_and(|c| flat(me.pos, c) > 35.0);
                    self.shooter.still_in_combat = self.rng.si(if far { 0.3 } else { 0.6 });
                    self.shooter.decide_combat = v.time + self.rng.uniform(2.0, 5.0) as f64;
                }
                let aimed = self.shooter.aim(&mut axes, me.view, me.eye, at, 8.0, 4.0);
                // whoever attacks clears the flag before entering it
                let clearing = self.clearing(v);
                self.last_seen = Some((v.time, at));
                let to_spot = if self.cover_fight && !clearing { self.firing_spot(v, at, &others) } else { None };
                let dir = to_spot.or(dir);
                // (`bots:cover-keep`: a spot kept past the coin of stopping is held, not walked away from)
                let held = self.fight_tweaks.keep && self.fight_spot.is_some();
                let walks = ((!self.shooter.still_in_combat && !held) || to_spot.is_some()) && !clearing;
                // (`bots:fire-first`: aimed with its line clear, it stops for the burst and walks between bursts)
                // Its foot jam in DC_Weapon_Bunkers (B16 `stuck` on foot, side against side, seeds 701-706) was mostly the mesh's way
                // out through the railing by the guard tower, now shut; what is left (-0.3 to -0.7 points of alive time without it) is
                // within the noise, and stopping only when it can fire (`bots:fire-ready`) does not move it.
                self.fire_stop = self.shooter.fps.stand && walks && aimed && self.shooter.firing_now(v.time);
                let walks = walks && !self.fire_stop;
                if let Some(t) = self.spot_trace.as_mut() {
                    t.fight = Some((walks, to_spot.is_some()));
                }
                // stopped it crouches: posture is free (specification, §3), and crouched it is a smaller target and aims better; at a
                // parapet it stands up to fire over it
                let parapet = self.fight_spot.is_some_and(|s| s.option == 0) && to_spot.is_none();
                // (stopped only for a burst, it neither lies nor crouches: it walks on after it)
                if self.fight_tweaks.prone && !walks && !parapet && d > PRONE_M && !self.fire_stop {
                    self.lie(v, &mut axes);
                } else if !walks && !(parapet && aimed) && !self.fire_stop {
                    axes.push((axis::CROUCH, 1.0));
                }
                if let (Some(d0), true) = (dir, walks) {
                    strafe_to(f, d0, &mut axes);
                }
                self.branch = if !walks { "fight" } else if to_spot.is_some() { "fight_to_spot" } else { "fight_walk" };
                self.shooter.look_line(v, aimed, at, &others);
                // (`bots:friend-gate`: stopped with a mate in its line a while, it steps aside, away from the mate)
                if let Some(side) = self.shooter.friend_side
                    && !walks
                    && self.shooter.line_busy_since.is_some_and(|t| v.time - t > shooter::STEP_ASIDE_S)
                {
                    axes.retain(|e| e.0 != axis::YAW);
                    axes.push((axis::YAW, -side));
                }
                self.shooter.shoot_on_foot(v, &mut self.rng, &mut self.alternate, &mut axes, class, d, aimed, self.profile.foot_range);
            }
            None if keeping => {
                // ---- `bots:cover-keep`: the target flickered; on to its spot, or holding it crouched, looking where the enemy was
                self.branch = "keeping";
                let s = self.fight_spot.expect("kept spot");
                let at = self.last_seen.expect("seen").1;
                self.shooter.aim(&mut axes, me.view, me.eye, at, 8.0, 4.0);
                let d0 = [s.pos[0] - me.pos[0], s.pos[2] - me.pos[2]];
                if d0[0].hypot(d0[1]) > SPOT_ARRIVED_M {
                    strafe_to(f, d0, &mut axes);
                } else {
                    self.spot_reached.get_or_insert((s.pos, v.time));
                    axes.push((axis::CROUCH, 1.0));
                }
            }
            None if self.suppressing(v) => {
                // ---- `bots:cover-flank`: covering fire at where the enemy was, crouched between bursts, with half the magazine kept
                self.branch = "suppressing";
                let at = self.cover.and_then(|c| c.enemy).unwrap();
                if crate::bots::individual::cover_trace() && (v.time * 30.0).round() as i64 % 30 == 0 {
                    eprintln!("cover: t={:.0} player {} suppresses at {:?}, {} rounds of {}", v.time, self.player, at.map(|x| x as i32), me.rounds.0, self.mag_full);
                }
                let aimed = self.shooter.aim(&mut axes, me.view, me.eye, at, 8.0, 4.0);
                self.mag_full = self.mag_full.max(me.rounds.0);
                if aimed && matches!(class, weapons::WeaponClass::Automatic | weapons::WeaponClass::Bolt) && 2 * me.rounds.0 > self.mag_full {
                    let d = crate::bots::body::dist3(me.eye, at);
                    self.shooter.trigger(v, &mut self.rng, &mut self.alternate, &mut axes, class, d, self.profile.foot_range);
                } else {
                    axes.push((axis::CROUCH, 1.0));
                }
            }
            None if recall_at.is_some() => {
                // ---- `bots:recall`: a target just lost from sight; it walks on looking where he was, to fire if he shows again
                self.branch = "recall";
                let at = recall_at.expect("watched for");
                self.shooter.calm(v, &mut axes, class);
                self.shooter.aim(&mut axes, me.view, me.eye, at, 8.0, 4.0);
                if let Some(d0) = dir {
                    strafe_to(f, d0, &mut axes);
                }
                self.walker.climb(v, &mut axes);
            }
            None => {
                // ---- walking
                self.branch = "walk";
                self.walk_dir = dir;
                self.shooter.calm(v, &mut axes, class);
                let c = self.walker.look(v).unwrap_or((pitch(f) / 2.5).clamp(-4.0, 4.0));
                axes.push((axis::MOUSE_Y, c));
                self.walker.walk(f, dir, &mut axes);
                self.walker.climb(v, &mut axes);
                self.walker.leap(v, &self.maps, &mut axes);
            }
        }
        // ---- jam: jumps, steps aside and after a while looks for another point
        let clearing = self.shooter.target.is_some() && self.clearing(v);
        let to_spot = self.fight_spot.is_some_and(|s| flat(me.pos, [s.pos[0], s.pos[2]]) > SPOT_ARRIVED_M) && (self.shooter.target.is_some() || keeping);
        let held = self.fight_tweaks.keep && self.fight_spot.is_some() && self.shooter.target.is_some();
        let wants = if keeping {
            dir.is_some() && to_spot
        } else {
            dir.is_some() && (!(self.shooter.target.is_some() && (self.shooter.still_in_combat || held)) || to_spot) && !clearing && !(self.shooter.target.is_none() && self.suppressing(v)) && !(self.fire_stop && self.shooter.target.is_some())
        };
        // (`bots:close-fight`: with a target near and a line to it, it fights: no flight, no count of being stuck)
        let close = self.shooter.fps.close && self.shooter.target.is_some_and(|t| t.2 < shooter::CLOSE_FIGHT_M) && !self.shooter.line_long_busy(v.time);
        if close {
            self.walker.flight_until = self.walker.flight_until.min(v.time);
        }
        let wants = wants && !close;
        if let Some(flight) = self.walker.unjam(v, &mut self.rng, self.player, step, wants, self.shooter.target.is_none(), &mut axes) {
            // (`bots:flight-aim`: with a target, the flight moves its legs only; the look and the trigger stay the fight's. Round a
            // tank's hull in Berlin it turned its back to it and jumped, at 8-24 m, rather than fire at its gunner)
            // TODO: parked: short run +1.8, but the SPRT against anchor 6 says H0 (-1.7 +- 2.7 over 128 rounds, Berlin -12.6). Next:
            // only for the flight round a vehicle's hull, and only with the target aimed at and the line clear.
            if self.shooter.fps.flight_aim && self.shooter.target.is_some() {
                self.branch = "unjam_aim";
                axes.retain(|a| !matches!(a.0, axis::THROTTLE | axis::YAW | axis::CROUCH | axis::LIE | axis::ACTION | axis::WALK));
                strafe_to(f, self.walker.flight, &mut axes);
                return axes;
            }
            self.branch = "unjam";
            return flight;
        }
        axes
    }

    /// `bots:cover-flank`: the flank a mate of its squad goes round, if any.
    pub fn set_cover(&mut self, c: Option<crate::bots::team::CoverFlank>) {
        self.cover = c;
        self.shooter.prefer_way = c.map(|c| (c.from, c.to));
    }

    /// `bots:cover-flank`: whether it is at its firing spot (it reached it fighting and has not left it).
    pub fn at_firing_spot(&self, v: &BotView) -> bool {
        self.fight_spot.is_some_and(|s| flat(v.me.pos, [s.pos[0], s.pos[2]]) <= SPOT_ARRIVED_M + COVER_SPOT_SLACK_M)
    }

    /// `bots:cover-flank`: with no enemy in sight, it holds its firing spot and fires at where the enemy was while a mate flanks.
    fn suppressing(&self, v: &BotView) -> bool {
        v.me.inside.is_none() && self.cover.is_some_and(|c| c.enemy.is_some_and(|e| flat(v.me.pos, [e[0], e[2]]) < self.profile.foot_range)) && self.at_firing_spot(v)
    }

    /// `bots:cover-fight`: the way to its firing spot while it fights, or `None` when it has none or is there. It asks for one when it
    /// decides to stop and fight (one query a second at most, [`crate::bots::tactics::due`]); it keeps the one it has while it is
    /// still got to first and still hides it crouched ([`crate::bots::tactics::still_good`]), checked once a second.
    ///
    /// TODO: this loses to anchor 0 (zmargin -0.21, DC_Al_Nas -57 tickets, 64 pairs), and the spot trace says why: of the time with a
    /// target it stops a third less than anchor 0 and walks to spots ~7 m away instead, where it kills less; it reaches a third to a
    /// half of them, as the spot is left within ~3 s (the coin of `still_in_combat`, `still_good`, or a target that flickers and
    /// sends it back along its path, to and fro), and it walks there in a straight line (a spot behind a fence or a wall: it pushes
    /// at it). Try, each behind its variant: not walking to a far spot or when already hidden where it stands, keeping the spot
    /// through a flicker, leaving out a spot whose straight way is blocked (or going by the mesh's path), and lying down when
    /// stopped with the enemy far (it never lies down; a person does, under fire).
    /// It also walks back with no visible reason: the position queries do not weigh a spot behind it against the way it advances,
    /// and a `resumed` spot can be 27–51 m off (none of 67 was reached). If this tactic comes back: a cost for spots behind the
    /// advance and no `resumed` spot farther than ~16 m, measured with BYTEFIELD_SPOT_TRACE (`why`, `resumed`).
    fn firing_spot(&mut self, v: &BotView, at: [f32; 3], others: &[crate::bot_api::Other]) -> Option<[f32; 2]> {
        use crate::bots::tactics;
        let me = &v.me;
        let tw = self.fight_tweaks;
        // (`bots:cover-keep`: the coin of stopping does not take it off a spot it is going to or has just reached)
        let holds = tw.keep && self.fight_spot.is_some_and(|s| self.spot_reached.filter(|r| r.0 == s.pos).is_none_or(|r| v.time - r.1 < HOLD_S));
        if !self.shooter.still_in_combat && !holds {
            if let Some(t) = self.spot_trace.as_mut() {
                t.why = Some("combat");
            }
            self.fight_spot = None;
            return None;
        }
        let attacks = self.flag.is_some_and(|b| b.attacks);
        let ctx = tactics::Ctx {
            enemy: at,
            goal: self.centre.filter(|_| attacks).map(|c| [c[0], me.pos[1], c[1]]),
            claimed: others.iter().filter(|o| o.team == me.team && o.player != me.player && flat(o.pos, [me.pos[0], me.pos[2]]) < 20.0).map(|o| o.pos).collect(),
            mates: Vec::new(),
            cover_points: self.cover_points,
            straight: tw.path,
        };
        if let Some(s) = self.fight_spot
            && v.time - self.spot_checked >= 1.0
        {
            self.spot_checked = v.time;
            if let Some(why) = tactics::still_bad(v, s.pos, &ctx) {
                if let Some(t) = self.spot_trace.as_mut() {
                    t.why = Some(why);
                }
                self.fight_spot = None;
            }
        }
        if self.fight_spot.is_none() && self.shooter.still_in_combat && tactics::due(v.time, self.spot_asked, self.player) {
            self.spot_asked = v.time;
            self.spot_checked = v.time;
            let mut first = if ctx.goal.is_some() { tactics::FIRING_SPOT_ADVANCE } else { tactics::FIRING_SPOT };
            let mut corner = tactics::CORNER_SPOT;
            // (`bots:cover-near`: a parapet where it stands is its spot; else only the near ones)
            let here = if tw.near { tactics::here(v, &ctx) } else { None };
            if tw.near {
                first.r = COVER_NEAR_M;
                corner.r = COVER_NEAR_M;
            }
            self.fight_spot = here.or_else(|| tactics::ask(v, self.maps.foot(v), &[first, corner], &ctx));
            if let Some(t) = self.spot_trace.as_mut() {
                t.asked += 1;
                t.found += self.fight_spot.is_some() as u32;
                t.chosen = self.fight_spot.map(|s| if s.from == "here" { "here" } else if s.option == 0 { first.name } else { corner.name });
            }
        }
        let s = self.fight_spot?;
        let d = flat(me.pos, [s.pos[0], s.pos[2]]);
        if d <= SPOT_ARRIVED_M && self.spot_reached.is_none_or(|r| r.0 != s.pos) {
            self.spot_reached = Some((s.pos, v.time));
        }
        (d > SPOT_ARRIVED_M).then(|| [s.pos[0] - me.pos[0], s.pos[2] - me.pos[2]])
    }

    /// `bots:cover-keep`: whether, with nobody in sight, it still goes to its firing spot or holds it (it saw its target less than
    /// [`KEEP_S`] ago); past that it drops the spot, rather than keeping it for a later fight somewhere else.
    fn keeps_spot(&mut self, v: &BotView) -> bool {
        if !self.cover_fight || !self.fight_tweaks.keep || self.fight_spot.is_none() || v.me.inside.is_some() {
            return false;
        }
        if self.last_seen.is_some_and(|(t, _)| v.time - t < KEEP_S) {
            return true;
        }
        if let Some(t) = self.spot_trace.as_mut() {
            t.why = Some("no_target");
        }
        self.fight_spot = None;
        false
    }

    /// `bots:fire-rhythm`, `friend-gate`, `look-all`, `quick-react`, `fire-first`: how its shooter fires on foot.
    pub fn set_fps(&mut self, t: shooter::FpsTweaks, p: &crate::bots::params::BotParams) {
        self.shooter.fps = t;
        self.shooter.rhythm = (p.rhythm_pause, p.rhythm_burst);
    }

    /// `bots:prone-fight`: lies down (the lie key is a toggle: pressed only while not lying, and not again before the last press
    /// has had time to take).
    fn lie(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>) {
        self.lies = true;
        if v.me.pose != 2 && v.time - self.lie_pressed >= LIE_PRESS_S {
            self.lie_pressed = v.time;
            axes.push((axis::LIE, 1.0));
        }
    }

    /// `bots:cover-fight`: where to take cover from `e` ([`crate::bots::tactics::COVER_SPOT`]), else where [`find_cover`] would.
    /// `bots:streetwise`: it prefers the busy enemy on foot ([`super::body::shooter::Opportunist`]), with this trait.
    pub fn opportunist(&mut self, streetwise: f32, busy: &[usize], backs: bool) {
        let o = self.shooter.opportunist.get_or_insert_with(Default::default);
        o.streetwise = streetwise;
        o.backs = backs;
        o.busy.clear();
        o.busy.extend(busy.iter().copied().filter(|&j| j != usize::MAX));
    }

    /// `bots:streetwise`: where its side says the enemy's vehicles pass, for its mines.
    pub fn mines_at(&mut self, at: Option<([f32; 2], [f32; 2])>, spread: bool) {
        self.sapper.traffic = at;
        self.sapper.spread = spread;
    }

    pub fn cover_spot(&self, v: &BotView, e: [f32; 3]) -> Option<[f32; 3]> {
        use crate::bots::tactics;
        // (off for now: crouched-height cover left the wounded seen and fighting, B05 breaking contact 36 -> 18 % in DC_Al_Nas;
        // TODO: try COVER_SPOT with the chest's height, as `find_cover`, once P1 holds)
        if self.retreat_mate {
            let me = &v.me;
            let mates: Vec<[f32; 3]> = v.others().iter().filter(|o| o.team == me.team && !o.in_vehicle && flat(o.pos, [me.pos[0], me.pos[2]]) < 2.0 * tactics::MATE_NEAR_M).map(|o| o.pos).collect();
            if !mates.is_empty()
                && let Some(s) = tactics::ask(v, self.maps.foot(v), &[tactics::COVER_SPOT_MATE], &tactics::Ctx { enemy: e, mates, cover_points: self.cover_points, ..Default::default() })
            {
                return Some(s.pos);
            }
        }
        if !self.cover_fight || !COVER_SPOT_ON {
            return crate::bots::individual::find_cover(v, e);
        }
        tactics::ask(v, self.maps.foot(v), &[tactics::COVER_SPOT], &tactics::Ctx { enemy: e, cover_points: self.cover_points, ..Default::default() }).map(|s| s.pos).or_else(|| crate::bots::individual::find_cover(v, e))
    }

    /// Whether, attacking, it stands to clear its flag of the enemy it sees: near the flag, unless its line of fire has been busy for
    /// a while (a mate or a wall in it). Omaha_Beach's first flag has a radius of 60 m: five off the same landing craft, on the sand
    /// within it, stood there in each other's line for the rest of the round, without a shot.
    fn clearing(&self, v: &BotView) -> bool {
        self.flag.is_some_and(|b| b.attacks) && self.centre.is_some_and(|c| flat(v.me.pos, c) < self.radio + 12.0) && !self.shooter.line_long_busy(v.time)
    }

    /// Breaks contact: walks to its point without firing (with the weapon calm) and, with no step (arrived or waiting for a path), crouches.
    fn under_cover(&mut self, v: &BotView, mut axes: Vec<(usize, f32)>) -> Vec<(usize, f32)> {
        let me = &v.me;
        let f = me.view[2];
        // (arrived at the point it stays: thinking another point on every tick within a radius of two metres is requesting paths non-stop)
        let arrived = self.imposed_point.is_some_and(|(c, r)| flat(me.pos, [c[0], c[2]]) < r + 1.0);
        let step = match self.destination(v) {
            Some((_, centre, radio)) if !arrived => self.walker.next_step(v, &mut self.rng, &self.maps, true, centre, radio, true, false),
            _ => None,
        };
        let class = weapons::class(me.weapon.as_deref().unwrap_or(""), me.single_shot);
        self.shooter.calm(v, &mut axes, class);
        axes.push((axis::MOUSE_Y, self.walker.look(v).unwrap_or((pitch(f) / 2.5).clamp(-4.0, 4.0))));
        let dir = step.map(|p| [p[0] - me.pos[0], p[1] - me.pos[2]]);
        self.walker.walk(f, dir, &mut axes);
        self.walker.climb(v, &mut axes);
        self.walker.leap(v, &self.maps, &mut axes);
        if dir.is_none() {
            axes.push((axis::CROUCH, 1.0));
        }
        if let Some(flight) = self.walker.unjam(v, &mut self.rng, self.player, step, dir.is_some(), true, &mut axes) {
            return flight;
        }
        axes
    }

    /// Inside a vehicle: fires from its seat, drives if at the wheel, and leaves when due.
    fn inside(&mut self, v: &BotView) -> Vec<(usize, f32)> {
        let inside = v.me.inside.clone().expect("inside");
        let others = v.others();
        // a boat's guns reach much farther than a tank's: a big boat is seen from afar and nobody gets close to it
        let reach = if inside.class == VehicleClass::Ship { self.profile.vehicle_range.max(SHIP_RANGE) } else { self.profile.vehicle_range };
        let mut axes = Vec::with_capacity(8);
        let seen = v.vehicle(inside.vehicle);
        if self.driver.leave_at.is_none() {
            // how long it stays, by what it is
            let fixed_one = seen.as_ref().is_some_and(|c| c.fixed);
            self.driver.fixed_weapon = fixed_one;
            let n = inside.template.to_ascii_lowercase();
            self.sailor.on_board(v, &inside);
            self.driver.vehicle_since = v.time;
            let stay = if fixed_one {
                self.rng.lognormal(if self.profile.insists { 18.0 } else { 9.0 }, 1.1)
            } else if !self.profile.insists && (n.contains("hanomag") || n.contains("halftrack") || n.contains("willy") || n.contains("kubel")) {
                self.rng.lognormal(24.0, 0.9)
            } else {
                1e6
            };
            self.driver.leave_at = Some(v.time + stay as f64);
        }
        // the plane is lost: it jumps, and in the air the parachute opens
        if pilot::Pilot::plane_lost(v, &inside, seen.as_ref().map_or(1.0, |c| c.health)) {
            self.alternate = !self.alternate;
            axes.push((USE, self.alternate as i32 as f32));
            return axes;
        }
        if self.get_off_now {
            self.alternate = !self.alternate;
            axes.push((USE, self.alternate as i32 as f32));
            return axes;
        }
        // flipped, burning or stuck: anyone gets off. Inside a flipped vehicle you die without firing
        let flies = inside.class.flies();
        // (a plane sits tilted and still on the ground: its own pilot looks at that)
        let flipped = !flies && inside.rows[1][1] < 0.25;
        let broken = seen.as_ref().is_some_and(|c| c.health < 0.2) && !flies;
        // TODO: what flies is never "stuck", so a rail vehicle with a flyer's thrust strands its riders: GC_Judicator's tram (a
        // `Plane`) runs to the end of its track and its five sit there the rest of the round, and the uplift (a `Helicopter`) sits at
        // the foot of the cliff with its pilot pressing down. A "still 10 s with no flight order, get off" rule fixed both but was
        // taken out until it shows up often enough to be worth it; to bring it back: `git show 4ea3d4c7 fa508c3f 0619dd93`.
        let stuck = !flies && inside.seat == 0 && !self.driver.fixed_weapon && !self.sailor.boat && self.walker.vehicle_jam(v, inside.pos) > STUCK && self.walker.stopped_until < v.time;
        if flipped || broken || stuck {
            self.alternate = !self.alternate;
            axes.push((USE, self.alternate as i32 as f32));
            return axes;
        }
        // path to lay mines, and that is all
        if self.sapper.dismount_to_mine(v, &mut surroundings!(self), &inside) {
            self.alternate = !self.alternate;
            axes.push((USE, self.alternate as i32 as f32));
            return axes;
        }
        // with nobody at the controls it goes nowhere
        if !self.sailor.boat && inside.seat != 0 && self.seat_spot.is_none() && seen.as_ref().is_some_and(|c| c.driver_free && !c.fixed) {
            axes.push((crate::entry::MENU_SELECT1, 1.0));
            return axes;
        }
        if inside.class == VehicleClass::Airship && inside.seat == 0 && self.pilot.airship {
            self.pilot.fly_airship(v, &inside, &mut axes, &mut self.rng, self.land_at);
            return axes;
        }
        if inside.class == VehicleClass::Helicopter && inside.seat == 0 {
            self.pilot.fly_heli(v, &inside, &mut axes, &others, &mut self.rng, &mut self.shooter, self.land_at);
            return axes;
        }
        if flies && inside.seat == 0 {
            if self.pilot.fly(v, &inside, &mut axes, &others, &mut self.rng, &mut self.shooter, &self.maps, self.player) {
                self.alternate = !self.alternate;
                axes.push((USE, self.alternate as i32 as f32));
            }
            return axes;
        }
        self.shooter.see(v, &mut self.rng, inside.camera_pos, reach, &others);
        if let Some((_, at, d)) = self.shooter.target {
            // at long range the bullet drops: aimed from above, like any gunner
            // TODO: the shell drop uses 9.81, the world's gravity is `crate::GRAVITY` (14.73); left as is because the plain bot is the fixed
            // opponent of the bots tournaments (changing it moves their baselines).
            let at = if inside.class == VehicleClass::Ship && d > 100.0 { [at[0], at[1] + d * d * 0.5 * 9.81 / (SHELL_SPEED * SHELL_SPEED), at[2]] } else { at };
            // (bots, from an aircraft's seat: what flies first, led; no healing tool at an enemy; missiles only for what is big or slow)
            let (at, keys) = match self.shooter.gunner.take() {
                Some(mut g) if inside.class.flies() => {
                    let plan = g.plan(v, &inside, &others, self.shooter.target);
                    self.shooter.gunner = Some(g);
                    match plan {
                        Some(p) => (p.at, (p.fire, p.alt)),
                        None => (at, (false, false)),
                    }
                }
                other => {
                    self.shooter.gunner = other;
                    (at, (true, true))
                }
            };
            let aimed = self.shooter.aim(&mut axes, inside.camera, inside.camera_pos, at, 6.0, 10.0);
            let amigos: Vec<[f32; 3]> = others.iter().filter(|o| o.team == v.me.team && !o.in_vehicle).map(|o| o.pos).collect();
            // (a shell drops in a parabola, with spread, and bursts: none of ours in the firing corridor, before or somewhat after the target)
            let friendly_shell = inside.class == VehicleClass::Ship && d > 100.0 && others.iter().any(|o| o.team == v.me.team && in_corridor(inside.camera_pos, at, o.pos));
            let hidden = friendly_shell || friend_in_line(inside.camera_pos, at, &amigos);
            self.shooter.shoot_from_seat_keys(v, &mut self.rng, &mut axes, aimed, hidden, reach, keys);
        }
        if self.driver.leave_at.is_some_and(|t| v.time >= t) && self.shooter.target.is_none() {
            // from a fixed weapon with nobody to shoot at it leaves never to return
            if self.driver.fixed_weapon && v.time - self.shooter.target_t > 10.0 && !self.driver.vetoed.contains(&inside.vehicle) {
                self.driver.vetoed.push(inside.vehicle);
            }
            self.alternate = !self.alternate;
            axes.push((USE, self.alternate as i32 as f32));
            return axes;
        }
        if self.sailor.boat {
            let low = match self.sailor.arrive(v, &inside, seen.as_ref(), &mut axes, &mut self.driver.vetoed) {
                Arrival::Low => true,
                Arrival::Fin => false,
                Arrival::Follows(shore) => {
                    if let Some((_, centre, _)) = self.destination(v)
                        && !shore
                    {
                        self.sailor.timon(v, &inside, &mut axes, centre, &mut self.walker);
                    }
                    false
                }
            };
            if low {
                self.alternate = !self.alternate;
                axes.push((USE, self.alternate as i32 as f32));
            }
            return axes;
        }
        if inside.seat == 0 && !self.driver.fixed_weapon && v.time < self.wait_until {
            // waits for its own before setting off
            axes.push((axis::THROTTLE, 0.0));
            return axes;
        }
        if inside.seat == 0 && !self.driver.fixed_weapon
            && let Some((e, centre, radio)) = self.destination(v)
        {
            self.driver.drive(v, &inside, &mut axes, &mut self.rng, &self.profile, &self.maps, &mut self.walker, self.shooter.target.is_some(), e.attacks, centre, radio);
        }
        axes
    }
}

/// Walks towards `d0` (flat) whatever way it faces, forwards or back and to either side, to keep its aim while it goes.
pub(crate) fn strafe_to(f: [f32; 3], d0: [f32; 2], axes: &mut Vec<(usize, f32)>) {
    let a = angle_between(f, d0).to_radians();
    let (th, st) = (a.cos(), a.sin());
    axes.push((axis::THROTTLE, if th > 0.38 { 1.0 } else if th < -0.38 { -1.0 } else { 0.0 }));
    axes.push((axis::YAW, if st > 0.38 { 1.0 } else if st < -0.38 { -1.0 } else { 0.0 }));
}

impl Mind for Bot {
    fn think(&mut self, v: &BotView) -> Vec<(usize, f32)> {
        self.lies = false;
        let mut axes = self.think_now(v);
        // (`bots:prone-fight`: lying and not meaning to, it gets up, to a crouch; on foot only, whatever part of it decided)
        if self.fight_tweaks.prone && !self.lies && v.me.pose == 2 && v.me.inside.is_none() {
            axes.push((axis::CROUCH, 1.0));
        }
        if let Some(t) = self.spot_trace.as_mut() {
            t.tick(v, self.cover_fight, self.fight_spot, self.shooter.target.map(|x| x.1), self.flag.is_some_and(|b| b.attacks), &axes);
        }
        if let Some(t) = self.fire_trace.as_mut() {
            t.tick(v, self.trace_label, &self.shooter, self.branch, self.profile.foot_range, &axes);
        }
        axes
    }
}

impl Bot {
    /// `BYTEFIELD_FIRE_TRACE`: after the individual's tick, with its action and the axes it leaves ([`crate::bots::fire_trace::FireTrace::after`]).
    pub fn trace_after(&mut self, v: &BotView, action: &str, axes: &[(usize, f32)]) {
        let Some(t) = self.fire_trace.as_mut() else { return };
        let w = &self.walker;
        let me = &v.me;
        let turning = axes.iter().any(|e| e.0 == axis::MOUSE_X && e.1.abs() > 0.5);
        let walker = if v.time < w.stopped_until {
            ":rest"
        } else if w.requested_point.is_some() {
            ":queued"
        } else if w.path.is_empty() {
            ":nopath"
        } else if turning {
            ":turn"
        } else {
            ""
        };
        t.after(v, action, axes, walker, || {
            format!(
                "pos={:?} point={:?} path={} next={:?} flight={:.1} axes={:?}",
                me.pos.map(|x| (x * 10.0).round() / 10.0),
                w.point.map(|p| (p[0] - me.pos[0]).hypot(p[1] - me.pos[2]) as i32),
                w.path.len(),
                w.path.first().map(|p| p.map(|x| (x * 10.0).round() / 10.0)),
                (w.flight_until - v.time).max(0.0),
                axes
            )
        });
    }

    fn think_now(&mut self, v: &BotView) -> Vec<(usize, f32)> {
        if self.spawned.is_none() {
            self.spawn(v);
        }
        if v.me.inside.is_some() {
            self.driver.vehicle = None;
            self.inside(v)
        } else {
            if self.driver.leave_at.take().is_some() {
                // got off: in this life it does not board again, except where it must insist
                self.driver.wants_board_at = self.profile.insists.then_some(v.time + 15.0);
                self.sailor.on_leaving();
                self.pilot = pilot::Pilot::default();
                self.walker.point = None;
                self.walker.path.clear();
            }
            self.on_foot(v)
        }
    }
}

#[cfg(test)]
#[path = "life_tests.rs"]
mod tests;
