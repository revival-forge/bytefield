//! New bots, written from the behaviour specification (`Mind`/`BotView`, without touching the rules).
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
pub mod flags;
pub use crate::bots3::body::{BORED, ENTER_WAIT, Maps, MapProfile};

use crate::bot_api::{BotView, Mind, MindMaker, VehicleClass};
use crate::entry::USE;
use crate::soldier::axis;
pub use crate::bots3::body::{driver, paths, pilot, rng, sailor, sapper, shooter, trade, walker, weapons};
use crate::bots3::body::*;
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
        Surroundings { rng: &mut $s.rng, profile: &$s.profile, maps: &$s.maps, player: $s.player, shooter: &mut $s.shooter, cam: &mut $s.walker, driver: &mut $s.driver, alternate: &mut $s.alternate, flag: &mut $s.flag, owner: &mut $s.owner, centre: $s.centre }
    };
}

/// A bot's life.
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
    // ---- what is commanded from outside (bots3: its team). With nothing (as usual), it chooses
    /// the flag it goes to (and whether it attacks), instead of choosing it
    pub imposed: Option<flags::Choice>,
    /// a point (centre and radius) it goes to instead of the flag: the leader it follows
    pub imposed_point: Option<([f32; 3], f32)>,
    /// the vehicle and the seat it must board
    pub seat_spot: Option<(usize, usize)>,
    /// bots3: where it wants to come down when it is in the air with nothing below (it jumped): it faces there, which is where the
    /// fall and the parachute push it
    pub fall_towards: Option<[f32; 3]>,
    /// at the wheel, it does not set off before this: it waits for the passengers
    pub wait_until: f64,
    /// does not board a vehicle on its own (goes with its squad)
    pub no_own_vehicle: bool,
    /// goes round the vehicle's body to a seat whose entry is on the far side (bots3; bots2 walks straight at it)
    pub go_round_hull: bool,
    /// get off the vehicle now (the passenger has arrived)
    pub get_off_now: bool,
    /// breaks contact (bots3): neither fires nor heals nor boards anything; walks to its point (`imposed_point`) and, once arrived, crouches
    pub break_contact: bool,
    /// at the controls of a helicopter, land at this point instead of going to a flag (bots3: carrying passengers, waiting for them)
    pub land_at: Option<[f32; 3]>,
    /// to which side it turns to go to its seat when it has it behind (if not, at ±180° it changes side on every tick and never turns)
    seat_turn: Option<f32>,
}

impl Bot {
    /// Changes what depends on the map (bots3, [`crate::bots3::mods`]): bots2 does not call it.
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
            go_round_hull: false,
            get_off_now: false,
            break_contact: false,
            land_at: None,
            seat_turn: None,
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

    /// Its hands and eyes, for whoever carries it (bots3: checking the line of fire, difficulty, weapon by target).
    pub fn shooter_mut(&mut self) -> &mut Shooter {
        &mut self.shooter
    }

    pub fn shooter(&self) -> &Shooter {
        &self.shooter
    }

    /// Its hands at the controls of a plane or helicopter (bots3: where it goes in the air).
    pub fn pilot_mut(&mut self) -> &mut pilot::Pilot {
        &mut self.pilot
    }

    pub fn pilot(&self) -> &pilot::Pilot {
        &self.pilot
    }

    /// The vehicles its team has given to others (bots3): it does not take them on its own.
    pub fn reserved_mut(&mut self) -> &mut Vec<usize> {
        &mut self.driver.reserved
    }

    /// Its hands on the wheel (bots3: [`Driver::drive_fine`]).
    pub fn driver_mut(&mut self) -> &mut Driver {
        &mut self.driver
    }

    /// bots3: its side sees it is not getting anywhere on foot ([`Walker::break_out`]).
    pub fn break_out(&mut self, v: &BotView) -> bool {
        self.walker.break_out(v, &mut self.rng)
    }

    /// Its feet, to look at (the trace).
    pub fn walker(&self) -> &Walker {
        &self.walker
    }

    /// Its feet (bots3: the places it avoids).
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
        if let Some(p) = self.fall_towards
            && let Some(axes) = pilot::Pilot::fall_towards(v, p)
        {
            return axes;
        }
        if let Some(axes) = pilot::Pilot::fall(v) {
            return axes;
        }
        let me = &v.me;
        if let Some((dir, until)) = self.sailor.leave_by {
            if v.time < until {
                let g = angle_between(me.view[2], dir);
                return vec![(axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)), (axis::MOUSE_Y, 0.0), (axis::THROTTLE, 1.0)];
            }
            self.sailor.leave_by = None;
        }
        let others = v.others();
        self.shooter.see(v, &mut self.rng, me.eye, self.profile.foot_range, &others);
        self.shooter.watch_health(v);
        let mut axes = Vec::with_capacity(8);
        let f = me.view[2];
        if self.break_contact {
            return self.under_cover(v, axes);
        }
        // (only with what it has seen: `others` also gives those a wall hides)
        let threatened = self.flag.is_some_and(|b| !b.attacks) && self.centre.is_some_and(|c| self.shooter.target.is_some_and(|x| flat(x.1, c) < THREAT_RADIUS));
        if threatened && !self.threatened {
            self.walker.replan();
        }
        self.threatened = threatened;
        // whoever attacks enters at once if nobody defends it, and if not, sooner or later
        if self.flag.is_some_and(|b| b.attacks) && !self.enters && self.centre.is_some_and(|c| flat(me.pos, c) < 40.0) {
            let arrival = *self.near_since.get_or_insert(v.time);
            let defended = v.time - self.shooter.target_t < DEFENCE_MEMORY || v.time - arrival < ARRIVAL_WAIT;
            if !defended || self.profile.enter_wait <= 0.0 || self.rng.si(v.dt / self.profile.enter_wait) {
                self.enters = true;
                self.walker.replan();
            }
        }
        // ---- heal or repair
        if let Some(axes) = self.trade.trade(v, &others, &self.profile, &mut self.driver.vetoed, &mut self.walker, &mut self.shooter) {
            return axes;
        }
        // ---- explosives
        let combat = self.shooter.target.is_some();
        if let Some(axes) = self.sapper.sap(v, &mut surroundings!(self), combat) {
            return axes;
        }
        // ---- vehicle: the one it was given, or one it looks for itself
        // (the way to the seat is a straight line: not off a platform's edge, after a vehicle that has already taken off)
        if let Some((j, _)) = self.seat_spot
            && v.vehicle(j).is_some_and(|x| !with_ground(v, v.me.pos, [x.pos[0], x.pos[2]]))
        {
            self.seat_spot = None;
        }
        // (bots3: nor the step it is taking, round the hull to a door that hangs over the void: GC_Bespin's airships stand at the
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
                let clearing = self.flag.is_some_and(|b| b.attacks) && self.centre.is_some_and(|c| flat(me.pos, c) < self.radio + 12.0);
                let walks = !self.shooter.still_in_combat && !clearing;
                // stopped it crouches: posture is free (specification, §3), and crouched it is a smaller target and aims better
                if !walks {
                    axes.push((axis::CROUCH, 1.0));
                }
                if let (Some(d0), true) = (dir, walks) {
                    let a = angle_between(f, d0).to_radians();
                    let (th, st) = (a.cos(), a.sin());
                    axes.push((axis::THROTTLE, if th > 0.38 { 1.0 } else if th < -0.38 { -1.0 } else { 0.0 }));
                    axes.push((axis::YAW, if st > 0.38 { 1.0 } else if st < -0.38 { -1.0 } else { 0.0 }));
                }
                self.shooter.look_line(v, aimed, at, &others);
                self.shooter.shoot_on_foot(v, &mut self.rng, &mut self.alternate, &mut axes, class, d, aimed, self.profile.foot_range);
            }
            None => {
                // ---- walking
                self.shooter.calm(v, &mut axes, class);
                let c = (pitch(f) / 2.5).clamp(-4.0, 4.0);
                axes.push((axis::MOUSE_Y, c));
                self.walker.walk(f, dir, &mut axes);
            }
        }
        // ---- jam: jumps, steps aside and after a while looks for another point
        let clearing = self.shooter.target.is_some() && self.flag.is_some_and(|b| b.attacks) && self.centre.is_some_and(|c| flat(me.pos, c) < self.radio + 12.0);
        let wants = dir.is_some() && !(self.shooter.target.is_some() && self.shooter.still_in_combat) && !clearing;
        if let Some(flight) = self.walker.unjam(v, &mut self.rng, self.player, step, wants, self.shooter.target.is_none(), &mut axes) {
            return flight;
        }
        axes
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
        axes.push((axis::MOUSE_Y, (pitch(f) / 2.5).clamp(-4.0, 4.0)));
        let dir = step.map(|p| [p[0] - me.pos[0], p[1] - me.pos[2]]);
        self.walker.walk(f, dir, &mut axes);
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
            // TODO: the shell drop uses 9.81, the world's gravity is `crate::GRAVITY` (14.73); left as is because bots2 is the fixed
            // opponent of the bots3 tournaments (changing it moves their baselines).
            let at = if inside.class == VehicleClass::Ship && d > 100.0 { [at[0], at[1] + d * d * 0.5 * 9.81 / (SHELL_SPEED * SHELL_SPEED), at[2]] } else { at };
            // (bots3, from an aircraft's seat: what flies first, led; no healing tool at an enemy; missiles only for what is big or slow)
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

impl Mind for Bot {
    fn think(&mut self, v: &BotView) -> Vec<(usize, f32)> {
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
mod tests;
