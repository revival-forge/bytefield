//! Entering and leaving vehicles.
//!
//! A player's `c_PIUse` (over 0.5) asks the server's toggle entry point, at most once a second
//! of world time. On foot (and not firing or recoiling) it looks for the nearest free entry
//! point of a vehicle of its team (or of none) — `BFfindEntryPoint`: the distance to it, in
//! whole centimetres, under its `setEntryRadius` and at most 3² — and
//! enters its PlayerControlObject: the soldier leaves the physics, rides at the vehicle's
//! matrix, and its input drives the vehicle (the engines run only with a driver, from the
//! update after it got in). Inside, the
//! same key leaves: the soldier comes back at the vehicle's matrix times
//! `setSoldierExitLocation`, with the vehicle's axes and its velocity × `exitSpeedMod`.

use serde::Serialize;

use crate::craft::{IDENTITY, mul, to_parent, ypr};
use crate::vehicle::{Component, Value};

use crate::vecmath::V3;
type M3 = [V3; 3];

/// `c_PIUse`
pub const USE: usize = 10;
/// `c_PIMenuSelect1` (the next eight follow)
pub const MENU_SELECT1: usize = 14;
/// what `checkPlayerTriggers` counts as pressed
pub const PRESSED: f32 = 0.5;
/// the search around the soldier (`BFfindEntryPoint(pos, 3, …)`)
pub const SEARCH_RADIUS: f32 = 3.0;
/// world time between two toggles
pub const TOGGLE_EVERY: f64 = 1.0;
/// what `killPlayer` gives the soldier
pub const KILL_DAMAGE: f32 = 100030.0;

/// What the server gives an object to destroy it outright (1e10: `destroyOnExit`,
/// `destroyVehicleWhenNoAmmo`).
pub const DESTROY_DAMAGE: f32 = 1.0e10;

/// The delay of the message that ends `setExitedFromVehicle` (s).
pub const EXITED_FOR: f64 = 2.0;

/// `setEntryRadius`'s default
pub const ENTRY_RADIUS: f32 = 3.0;
/// how far a bot may be from what it asked for (`validateBFEntryPoint(…, 15, false)`)
pub const BOT_ENTRY_RADIUS: f32 = 15.0;
/// how long a bot asked for its seat lets it go (`calculateVehicleUrgency`: × t/15 for 15 s), and
/// how long a person's soldier waits for it ([`Entry::wants`])
pub const WISH_FOR: f64 = 15.0;

#[derive(Debug, Clone, Serialize)]
pub struct EntryPoint {
    /// in the vehicle's frame
    pub pos: V3,
    pub radius: f32,
}

/// What a PlayerControlObject offers its occupant: the vehicle's own (the driver's) or one in its
/// tree (a passenger's or gunner's), everything in the vehicle's frame.
#[derive(Debug, Clone, Serialize)]
pub struct Seat {
    pub entries: Vec<EntryPoint>,
    /// `setSoldierExitLocation` (its first vector)
    pub exit: V3,
    pub exit_speed_mod: f32,
    /// where the occupant rides: its `SeatObject` (None: the PlayerControlObject's own place,
    /// the T34's)
    pub ride: V3,
    /// that `SeatObject` shows its soldier (its `seatFlags`): he can be shot there ([`SeatShow`])
    #[serde(default)]
    pub show: Option<SeatShow>,
    /// its PlayerControlObject's template (an AI object when it has an `aiTemplate`)
    pub pco: String,
    /// `hasRestrictedExit`: the way out must be clear ([`crate::Sim::exit_place`]); then its
    /// PlayerControlObject's place and axes, and `setSoldierExitLocation` in them
    #[serde(default)]
    pub restricted: Option<(V3, M3, V3)>,
    /// `subPos` (`getSubPos`): the soldier may leave only with its hatch (the exit
    /// location 0.5 m up, here in the vehicle's frame) above the water
    #[serde(default)]
    pub hatch: Option<V3>,
    /// `artPos` (`getArtPos`) of its own PlayerControlObject (the first that
    /// the server's toggle scout camera meets going up from the occupant): it may look from the scouts' marks
    #[serde(default)]
    pub art: bool,
    /// `sonarPos` (`getSonarPos`) of its own PlayerControlObject: its minimap is the vehicle's sonar
    /// (`crate::sonar`), and the sonars ping there as with `subPos`
    #[serde(default)]
    pub sonar: bool,
}

/// How a seat shows its soldier, from its `SeatObject`'s `seatFlags` (a flag name the game does not know adds nothing): half body,
/// whole body, head, standing or outside, and the two animation states he is posed with there (`seatAnimationLowerBody`,
/// `seatAnimationUpperBody`; by default sitting, or standing with `c_SeatShowStandingSoldier`).
///
/// Level 3: a soldier on such a seat can be shot ([`crate::projectile::Projectile::collide`]), posed as the seat shows him
/// ([`crate::soldier::Movement::posed_skeleton_seated`]) at its place with the vehicle's axes. The original server reads no seat
/// flag and takes the soldier out of the physics when he gets in (`disablePhysicsOnSoldier`); how its rounds still reach an
/// exposed gunner has not been traced. Players of the original remember shooting them; a soldier inside the hull stays out of
/// reach, as before.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SeatShow {
    pub lower: String,
    pub upper: String,
    /// `c_SeatShowHalfBodySoldier`: the legs folded onto the pelvis
    pub half_body: bool,
}

impl SeatShow {
    fn of(c: &Component) -> Option<SeatShow> {
        let flags: Vec<String> = match c.param("seatFlags") {
            Some(Value::List(a)) => a.iter().map(|f| f.trim().to_ascii_lowercase()).collect(),
            Some(Value::Text(t)) => vec![t.trim().to_ascii_lowercase()],
            _ => Vec::new(),
        };
        let flag = |n: &str| flags.iter().any(|f| f == n);
        let standing = flag("c_seatshowstandingsoldier");
        let half_body = flag("c_seatshowhalfbodysoldier");
        if !(standing || half_body || flag("c_seatshowfullbodysoldier") || flag("c_seatshowheadofsoldier") || flag("c_seatisoutside")) {
            return None;
        }
        let anim = |key: &str, default: &str| match c.param(key) {
            Some(Value::Text(t)) => t.trim().to_string(),
            _ => default.to_string(),
        };
        Some(SeatShow { lower: anim("seatAnimationLowerBody", if standing { "Lb_StandInVehicle" } else { "Lb_SitInVehicle" }), upper: anim("seatAnimationUpperBody", "Ub_SitInVehicle"), half_body })
    }
}

/// An object its spawner looks after: left
/// far from it (beyond `distance`) with nobody in it or near it, its `timeToLive` runs down, and
/// then it loses `damageWhenLost` a second.
#[derive(Debug, Clone, Serialize)]
pub struct Abandon {
    /// the spawner's place, `distance`, `timeToLive`, `damageWhenLost`
    pub spawner: V3,
    pub distance: f32,
    pub ttl: f32,
    pub damage: f32,
    /// what is left of its time
    pub timer: f32,
}

/// `ObjectTemplate.submarineData <oxygen use> <oxygen fill> <oxygen damage> <periscope min>
/// <periscope max> <max depth> <max depth damage>`: deeper than `max_depth`
/// it takes `max_depth_damage` a second; down to `periscope_max` its oxygen (0..1)
/// fills at `fill` a second, deeper it is used at `usage`, and with none left the soldiers in it
/// take `oxygen_damage` a second (`damageAllAttachedSoldiers`).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Submarine {
    pub usage: f32,
    pub fill: f32,
    pub oxygen_damage: f32,
    pub periscope_min: f32,
    pub periscope_max: f32,
    pub max_depth: f32,
    pub max_depth_damage: f32,
    pub oxygen: f32,
}

impl Submarine {
    pub fn parse(args: &[String]) -> Option<Submarine> {
        let v: Vec<f32> = args.iter().filter_map(|a| a.trim().parse().ok()).collect();
        let &[usage, fill, oxygen_damage, periscope_min, periscope_max, max_depth, max_depth_damage] = v.get(..7)? else { return None };
        Some(Submarine { usage, fill, oxygen_damage, periscope_min, periscope_max, max_depth, max_depth_damage, oxygen: 1.0 })
    }
}

/// A vehicle's seats and who is in them (the first: the driver's), or the vehicle and seat a
/// soldier is in.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Entry {
    pub seats: Vec<Seat>,
    pub occupants: Vec<Option<usize>>,
    /// `occupants[0]`
    pub driver: Option<usize>,
    pub inside: Option<usize>,
    pub position: usize,
    /// the world time of its player's last toggle (None: never, −9999 in the server)
    pub last_toggle: Option<f64>,
    /// a soldier just out of a vehicle: that vehicle and when (the soldier's setting of exited from vehicle,
    ///, cleared by the message 0x16 it posts itself 2 s later): until then its
    /// `handleCollision` with that vehicle stops at once (the impulse, but no ground, no crash)
    pub exited: Option<(usize, f64)>,
    /// made by a spawner: its time when abandoned
    pub abandon: Option<Abandon>,
    /// a submarine's oxygen and depth
    pub submarine: Option<Submarine>,
    /// its sonar (`crate::sonar`: the first `SonarObject` of its tree)
    pub sonar: Option<crate::sonar::Sonar>,
    /// `dontClearTeamOnExit`: the team its last occupant gave it stays when it leaves
    pub keep_team: bool,
    /// `destroyOnExit`: whoever gets out, it is destroyed (the controlled object's exit
    /// gives it 1e10; a guided missile)
    pub destroy_on_exit: bool,
    /// the frame time gathered (`handleFrameUpdate` acts every half second)
    pub frame_acc: f32,
    /// a bot's soldier: the seat its AI asks for, the vehicle and the seat
    pub requested: Option<(usize, usize)>,
    /// a person's soldier in a vehicle: the seat it asked for that a bot had, and when (ours: the
    /// server asks the bot and takes the seat only while the key is held; here a press is kept
    /// for `WISH_FOR` s and the seat taken as soon as the bot leaves it)
    pub wants: Option<(usize, f64)>,
    /// a soldier flying a guided missile: the launcher it was fired from (the launcher's id, set by
    /// the weapon's fire; −1 otherwise). Getting out of the missile, or the missile destroyed, puts it back in that
    /// launcher (the server's exit vehicle, and `giveDamage`'s `exitAndPlaceCameraIfApplicable` before it would kill
    /// it): it does not die with the missile. The launcher and the
    /// seat it fired from: the original enters the launcher's root (`enterVehicle(player, root, 0, 1)`), seat 0 for the
    /// Patriot, the SA-3 and the Wasserfall.
    /// The F-14A's RIO sits in the root (`F14A`, seat 0; the pilot is the `F-14A` under it): back from its Aim-54 it is
    /// in the RIO's seat again, in the original as here (in at record
    /// 90, back into `F14A` at 349, when the missile dies).
    pub return_to: Option<(usize, usize)>,
}

impl Entry {
    /// A vehicle's seats, empty.
    pub fn with_seats(seats: Vec<Seat>) -> Entry {
        Entry { occupants: vec![None; seats.len()], seats, ..Default::default() }
    }
}

/// What a PlayerControlObject's tree has for it: entry points, its `SeatObject`s (depth first), and the
/// PlayerControlObjects below it (their place and axes); not looking into those.
struct Found {
    entries: Vec<EntryPoint>,
    rides: Vec<(V3, Option<SeatShow>)>,
    pcos: Vec<(V3, M3, Component)>,
}

fn walk(c: &Component, at: V3, rows: M3, out: &mut Found) {
    for k in &c.children {
        let p = to_parent([k.position[0] as f32, k.position[1] as f32, k.position[2] as f32], &rows);
        let pos = [at[0] + p[0], at[1] + p[1], at[2] + p[2]];
        let r = mul(&ypr([k.rotation[0] as f32, k.rotation[1] as f32, k.rotation[2] as f32]), &rows);
        // a seat of its own (a passenger's, a gunner's): its entry points are its own
        if k.kind.eq_ignore_ascii_case("PlayerControlObject") {
            out.pcos.push((pos, r, k.clone()));
            continue;
        }
        if k.kind.eq_ignore_ascii_case("EntryPoint") {
            let radius = k.param("setEntryRadius").and_then(Value::number).map_or(ENTRY_RADIUS, |v| v as f32);
            out.entries.push(EntryPoint { pos, radius });
        }
        if k.kind.eq_ignore_ascii_case("SeatObject") {
            out.rides.push((pos, SeatShow::of(k)));
        }
        walk(k, pos, r, out);
    }
}

fn seat_of(c: &Component, at: V3, rows: M3, out: &mut Vec<Seat>) {
    let mut f = Found { entries: Vec::new(), rides: Vec::new(), pcos: Vec::new() };
    walk(c, at, rows, &mut f);
    let exit = match c.param("setSoldierExitLocation") {
        Some(Value::List(a)) => a.first().and_then(|s| bytefield_world::parse_vec(s)),
        Some(Value::Vector(v)) => Some(v.clone()),
        _ => None,
    }
    .filter(|v| v.len() == 3)
    .map_or([0.0; 3], |v| [v[0] as f32, v[1] as f32, v[2] as f32]);
    let local = exit;
    let e = to_parent(exit, &rows);
    let exit = [at[0] + e[0], at[1] + e[1], at[2] + e[2]];
    let exit_speed_mod = c.param("exitSpeedMod").and_then(Value::number).map_or(1.0, |v| v as f32);
    let restricted = c.param("hasRestrictedExit").and_then(Value::number).is_some_and(|v| v != 0.0).then_some((at, rows, local));
    let hatch = c.param("subPos").and_then(Value::number).is_some_and(|v| v != 0.0).then(|| {
        let e = to_parent([local[0], local[1] + 0.5, local[2]], &rows);
        [at[0] + e[0], at[1] + e[1], at[2] + e[2]]
    });
    let art = c.param("artPos").and_then(Value::number).is_some_and(|v| v != 0.0);
    let sonar = c.param("sonarPos").and_then(Value::number).is_some_and(|v| v != 0.0);
    out.push(Seat { entries: f.entries, exit, exit_speed_mod, ride: c.seat_rank().and_then(|k| f.rides.get(k)).map_or(at, |r| r.0), show: c.seat_rank().and_then(|k| f.rides.get(k)).and_then(|r| r.1.clone()), pco: c.name.clone(), restricted, hatch, art, sonar });
    for (p, r, k) in &f.pcos {
        seat_of(k, *p, *r, out);
    }
}

impl Seat {
    /// The seats of a PlayerControlObject's tree, its own first (none for anything else).
    pub fn all_from_model(c: &Component) -> Vec<Seat> {
        let mut out = Vec::new();
        if c.kind.eq_ignore_ascii_case("PlayerControlObject") {
            seat_of(c, [0.0; 3], IDENTITY, &mut out);
        }
        out
    }
}

/// `c_PGFRollGripWhenOccupied` (a plane's wheels): the PlayerControlObject's enter and exit
/// set its responses' grip (`setPermanentGrip`): with a driver they roll (`addFriction` only
/// looks at the roll and engine bits), empty they hold like any contact. (Here the roll bit is
/// added to the flag, and taken off again.)
fn occupied_grip(v: &mut crate::mover::Vehicle, occupied: bool) {
    use crate::contact::grip;
    for c in v.contacts.iter_mut().filter(|c| c.grip & grip::ROLL_WHEN_OCCUPIED != 0) {
        c.grip = if occupied { grip::ROLL_WHEN_OCCUPIED | grip::ROLL } else { grip::ROLL_WHEN_OCCUPIED };
    }
}

/// `BFfindEntryPoint`'s distance: to the centimetre (`(int)(d·100 + 0.5) / 100`).
pub fn rounded(d: f32) -> f32 {
    (d * 100.0).round() / 100.0
}

impl crate::Sim<'_> {
    /// The players' `c_PIUse` (`checkPlayerTriggers` → `toggleEntryPoint`), then each driver's
    /// input to its vehicle (the controlled object's player input handling: its components read the
    /// axes they are mapped to) and every vehicle's weapons.
    pub(crate) fn update_entries(&mut self) {
        // a driver's engines run from the update after it got in (and stop as it gets out)
        for v in self.vehicles.iter_mut().filter(|v| v.entry.driver.is_some()) {
            v.set_engines_on(true);
        }
        for i in 0..self.vehicles.len() {
            self.entry_step(i);
            self.pickup_step(i);
        }
        let dt = self.clock.dt as f32;
        for k in 0..self.vehicles.len() {
            let from = self.feed_seats(k);
            if !self.vehicles[k].weapons.is_empty() && !self.vehicles[k].removed {
                self.vehicle_weapons(k, &from, dt);
            }
        }
    }

    /// A player's soldier's use key in this step (the first part of `update_entries`): in a
    /// vehicle, a seat picked or out; on foot, into the seat found.
    pub fn entry_step(&mut self, i: usize) {
        let now = self.clock.time;
        let v = &self.vehicles[i];
        let Some(m) = v.soldier().filter(|m| m.has_player && m.driven && !v.removed) else { return };
        let pressed = |a: usize| m.input.mapped(a) && m.input.get(a) > PRESSED;
        // in a vehicle, `c_PIMenuSelect1..9` (14..22) pick a seat (the first one held); a seat
        // asked of a bot is taken once it is free
        if let Some(k) = v.entry.inside {
            if let Some(n) = (MENU_SELECT1..MENU_SELECT1 + 9).find(|&a| pressed(a)) {
                self.switch_seat(i, k, n - MENU_SELECT1, true);
                return;
            }
            if let Some((seat, t)) = v.entry.wants {
                if now - t < WISH_FOR {
                    self.switch_seat(i, k, seat, false);
                } else {
                    self.vehicles[i].entry.wants = None;
                }
            }
        }
        let v = &self.vehicles[i];
        let Some(m) = v.soldier() else { return };
        let pressed = |a: usize| m.input.mapped(a) && m.input.get(a) > PRESSED;
        if !pressed(USE) {
            return;
        }
        if v.entry.last_toggle.is_some_and(|t| now - t < TOGGLE_EVERY) {
            return;
        }
        match v.entry.inside {
            // `isAllowedToExit`
            Some(k) => {
                if self.exit_place(k, self.vehicles[i].entry.position).is_some() {
                    self.exit(i, k)
                }
            }
            None => {
                // not while firing or recoiling (`isNotFireingOrHaveRecoil`)
                if m.recoil.0 != 0 || m.weapon.as_ref().is_some_and(|w| !w.not_firing()) {
                    return;
                }
                // a better bot does not look around: the seat it asked for, if it is
                // free and near (the simple bots, `bots`, search as players do)
                let found = if self.better_bots_on() && self.players.iter().any(|p| p.ai && p.soldier == Some(i)) { self.bot_entry(i) } else { self.find_entry(i) };
                if crate::debug::options().trace_entry {
                    eprintln!("use {i} {now:.2} asks {:?} found {found:?}", self.vehicles[i].entry.requested);
                }
                if let Some((k, j)) = found {
                    self.enter(i, k, j);
                }
            }
        }
    }

    /// Each seat's occupant's input to the parts under its PlayerControlObject; the occupants'
    /// inputs.
    pub fn feed_seats(&mut self, k: usize) -> Vec<Option<(crate::anim::Input, i32)>> {
        let from: Vec<Option<(crate::anim::Input, i32)>> = self.vehicles[k]
            .entry
            .occupants
            .iter()
            .map(|o| o.and_then(|d| self.vehicles[d].soldier()).map(|m| (m.input.clone(), m.input_index)))
            .collect();
        let c = &mut self.vehicles[k].craft;
        // (an empty seat keeps what it has: nothing, or a test's forced input)
        for (j, f) in from.iter().enumerate() {
            let Some((input, _)) = f else { continue };
            let target = if j == 0 { Some(&mut c.inputs) } else { c.seat_inputs.get_mut(j - 1) };
            if let Some(t) = target {
                for (a, x) in t.iter_mut().enumerate() {
                    *x = input.get(a);
                }
            }
        }
        from
    }

    /// A vehicle's weapons in its frame: the driver's fire keys (`checkPlayerTriggers`: message
    /// 6 for `c_PIFire`, 7 for `c_PIAltFire`, through the PlayerControlObject to every weapon,
    /// each answering only its `setInputFire`), then each weapon's `handleUpdate`. A shot leaves
    /// from the weapon part's matrix with the vehicle's speed.
    pub fn vehicle_weapons(&mut self, k: usize, occupants: &[Option<(crate::anim::Input, i32)>], dt: f32) {
        let g = crate::rigid::GRAVITY_Y;
        let v = &mut self.vehicles[k];
        let frames = v.craft.frames();
        let seats: Vec<usize> = v.craft.parts.iter().map(|p| p.seat).collect();
        // each seat's camera (`fireInCameraDof`)
        let camera = |seat: usize| v.craft.parts.iter().position(|p| p.seat == seat && matches!(p.kind, crate::craft::Kind::Camera)).map(|k| frames[k]);
        let cameras: Vec<Option<crate::craft::Frame>> = (0..occupants.len().max(1)).map(camera).collect();
        let body_vel = v.craft.body.vel;
        let mut pushes = Vec::new();
        for (part, w) in v.weapons.iter_mut() {
            let f = frames[*part];
            let seat = seats[*part];
            // the weapon's PlayerControlObject's occupant
            let driver = occupants.get(seat).and_then(|o| o.as_ref());
            let index = driver.map_or(0, |d| d.1);
            let camera = cameras.get(seat).copied().flatten();
            let mut act = |w: &mut crate::firearms::FireArms, ask| match ask {
                crate::firearms::Ask::Fire => {
                    let from = if w.template.fire_in_camera { camera.unwrap_or(f) } else { f };
                    for shot in w.shots(from.rows, from.pos, Some(body_vel), g, index) {
                        v.shots.push((w.next_projectile(), shot, seat));
                    }
                    if w.template.firing_force > 0.0 {
                        pushes.push((f.pos, f.rows[2].map(|x| -x * w.template.firing_force)));
                    }
                }
                crate::firearms::Ask::Reload => {
                    w.handle_message(9, false);
                }
                _ => {}
            };
            let key = w.template.input_fire;
            if let Some((input, _)) = driver
                && input.mapped(key)
                && input.get(key) > PRESSED
            {
                let ask = w.handle_message(6, false);
                act(w, ask);
            }
            // The controlled object's player input handling: each weapon's `updateDeviation(false)`,
            // then (every frame) its `handleUpdate`
            if driver.is_some() {
                w.update_plain_deviation();
            }
            for ask in w.handle_update(dt) {
                act(w, ask);
            }
        }
        for (at, a) in pushes {
            v.craft.body.wake();
            v.craft.body.add_acc_at(at, a);
        }
        // `fireFinished` with `destroyVehicleWhenNoAmmo` and the last round: 1e10 to its
        // PlayerControlObject
        let mut destroy = false;
        for (_, w) in v.weapons.iter_mut() {
            destroy |= std::mem::take(&mut w.destroy_vehicle);
        }
        if destroy {
            self.give_damage(k, DESTROY_DAMAGE, crate::damage::Source::Destroy);
        }
    }

    /// Vehicle `k`'s weapons of template `name` fire once, from their part (or their seat's camera with
    /// `fireInCameraDof`) with the vehicle's speed, as they would themselves (for the eye: a client of the original
    /// server is not sent the others' shots, only their weapons' rounds). False with none.
    pub fn fire_vehicle_remote(&mut self, k: usize, name: &str) -> bool {
        let g = crate::rigid::GRAVITY_Y;
        let v = &mut self.vehicles[k];
        let frames = v.craft.frames();
        let body_vel = v.craft.body.vel;
        let mut fired = false;
        for i in 0..v.weapons.len() {
            let part = v.weapons[i].0;
            if !v.weapons[i].1.template.name.eq_ignore_ascii_case(name) {
                continue;
            }
            let seat = v.craft.parts[part].seat;
            let camera = v.craft.parts.iter().position(|p| p.seat == seat && matches!(p.kind, crate::craft::Kind::Camera)).map(|c| frames[c]);
            let w = &mut v.weapons[i].1;
            let from = if w.template.fire_in_camera { camera.unwrap_or(frames[part]) } else { frames[part] };
            for shot in w.shots(from.rows, from.pos, Some(body_vel), g, 0) {
                let p = w.next_projectile();
                v.shots.push((p, shot, seat));
            }
            fired = true;
        }
        if fired {
            self.take_shots();
        }
        fired
    }

    /// `BFfindEntryPoint(soldier, 3, team, true)`: the nearest free entry point (to the
    /// centimetre, under its radius and at most 3²) of a vehicle of the soldier's team or of
    /// none: the vehicle and the seat; on a tie the first (the driver's before a passenger's at
    /// the same place: the Kübelwagen).
    pub fn find_entry(&self, soldier: usize) -> Option<(usize, usize)> {
        let s = &self.vehicles[soldier];
        self.entry_point_near(s.craft.body.pos, s.team, SEARCH_RADIUS, true)
    }

    /// `toggleEntryPoint`'s branch for a bot (`validateBFEntryPoint(object, soldier,
    /// 15, false)`): the seat its AI asked for, when nobody is in it, it has an entry point,
    /// its vehicle (the object asked for: every seat is at the vehicle's place, as the bench's
    /// AI objects) is within 15 m and of the soldier's team or none.
    fn bot_entry(&self, soldier: usize) -> Option<(usize, usize)> {
        let s = &self.vehicles[soldier];
        let (k, j) = s.entry.requested?;
        let v = self.vehicles.get(k)?;
        if !v.in_world() || v.armor.as_ref().is_some_and(|a| a.destroyed) || (v.team != 0 && v.team != s.team) {
            return None;
        }
        if v.entry.occupants.get(j)?.is_some() || v.entry.seats[j].entries.is_empty() {
            return None;
        }
        let (a, b) = (v.craft.body.pos, s.craft.body.pos);
        let d2 = (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
        (d2 <= BOT_ENTRY_RADIUS * BOT_ENTRY_RADIUS).then_some((k, j))
    }

    /// `BFfindEntryPoint(at, r, team, by_radius)`: as above, at a point, within `r` (as the
    /// function: the distance against `r²`). Without `by_radius` (a spawn point's
    /// `getObjectToEnter`, a bot's `toggleEntryPoint` on spawning) the distance is held under `r`
    /// instead of under each entry point's `setEntryRadius`.
    pub(crate) fn entry_point_near(&self, at: V3, team: u32, r: f32, by_radius: bool) -> Option<(usize, usize)> {
        let mut best: Option<(f32, usize, usize)> = None;
        for (k, v) in self.vehicles.iter().enumerate() {
            if !v.in_world() || v.armor.as_ref().is_some_and(|a| a.destroyed) || (v.team != 0 && v.team != team) {
                continue;
            }
            for (j, seat) in v.entry.seats.iter().enumerate() {
                if v.entry.occupants[j].is_some() {
                    continue;
                }
                for e in &seat.entries {
                    let p = to_parent(e.pos, &v.craft.body.rows);
                    let w = [v.craft.body.pos[0] + p[0], v.craft.body.pos[1] + p[1], v.craft.body.pos[2] + p[2]];
                    let d = rounded(((w[0] - at[0]).powi(2) + (w[1] - at[1]).powi(2) + (w[2] - at[2]).powi(2)).sqrt());
                    // (the `d ≤ r²` is the function's own; the object query around the soldier,
                    // of 3 m, is not modeled: the Bf109's point at 3.2 m was found)
                    if d < if by_radius { e.radius } else { r } && d <= r * r && best.is_none_or(|(b, _, _)| d < b) {
                        best = Some((d, k, j));
                    }
                }
            }
        }
        best.map(|(_, k, j)| (k, j))
    }

    /// A soldier just spawned at a `setEnterOnSpawn` point: into the nearest free seat
    /// (`toggleEntryPoint(player, true)`; a human's search is the usual 3 m, a bot's
    /// `BFfindEntryPoint(pos, 15, team, false)`: any free entry point within 15 m, whatever its radius).
    pub(crate) fn enter_nearest(&mut self, soldier: usize, ai: bool) {
        let found = if ai {
            let s = &self.vehicles[soldier];
            self.entry_point_near(s.craft.body.pos, s.team, BOT_ENTRY_RADIUS, false)
        } else {
            self.find_entry(soldier)
        };
        if let Some((k, j)) = found {
            self.enter(soldier, k, j);
        }
    }

    pub fn enter(&mut self, soldier: usize, vehicle: usize, seat: usize) {
        let now = self.clock.time;
        let s = &mut self.vehicles[soldier];
        s.entry.inside = Some(vehicle);
        s.entry.position = seat;
        s.entry.last_toggle = Some(now);
        s.entry.wants = None;
        let team = s.team;
        // The controlled object's enter, first thing, with the template's `setForwardMod` over 0: the
        // root's velocity set to that many m/s along its forward axis with forwardMod · rows[2]
        // of `getRootParent`'s transformation), whoever gets in and however. The guided missiles made by `fireBarrel` get
        // their launch speed here, not from the FireArms' `velocity` (F3: DC's PatriotGuidedRocket and
        // SA-3GuidedRocket 200, the Aim-54 150, the Guided_Tomahawk 30; the original's Patriot rocket is at 175,86 m/s
        // after its first update, drag off 200)
        let forward_mod = self.world.db.object_template(&self.vehicles[vehicle].template).and_then(|t| t.get("setforwardmod")).and_then(|a| a.first()).and_then(|x| x.trim().parse::<f32>().ok()).filter(|&f| f > 0.0);
        let v = &mut self.vehicles[vehicle];
        v.entry.occupants[seat] = Some(soldier);
        // The controlled object's enter: the vehicle takes its occupant's team (`setTeam` of the
        // root); a sleeping vehicle wakes (`setIsAwake`)
        v.team = team;
        if let Some(f) = forward_mod {
            let fwd = v.craft.body.rows[2];
            v.craft.body.vel = [fwd[0] * f, fwd[1] * f, fwd[2] * f];
        }
        if v.craft.body.is_sleeping() {
            v.craft.body.wake();
        }
        if seat == 0 {
            v.entry.driver = Some(soldier);
            occupied_grip(v, true);
            // (a burnt-out engine, runs again with a new driver)
            for p in v.craft.parts.iter_mut() {
                if let crate::craft::Kind::Engine(e) = &mut p.kind {
                    e.burnt = false;
                }
            }
        }
        // on a seat that shows him he can be shot, posed as he is shown (level 3, [`SeatShow`])
        if let Some(show) = self.vehicles[vehicle].entry.seats.get(seat).and_then(|s| s.show.clone())
            && let Some(m) = self.vehicles[soldier].soldier_mut()
            && let Some(sk) = m.posed_skeleton_seated(&show.lower, &show.upper, show.half_body, 0.0)
        {
            m.skeleton = Some(sk);
        }
        self.ride();
    }

    /// Out of a seat (the vehicle's side): without its driver the engines stop and its input
    /// goes.
    pub(crate) fn leave_seat(&mut self, vehicle: usize, seat: usize) {
        let v = &mut self.vehicles[vehicle];
        v.entry.occupants[seat] = None;
        // `clearTeam`: with nobody left in it, no team (unless `dontClearTeamOnExit`)
        if !v.entry.keep_team && v.entry.occupants.iter().all(|o| o.is_none()) {
            v.team = 0;
        }
        if let Some(t) = seat.checked_sub(1).and_then(|j| v.craft.seat_inputs.get_mut(j)) {
            t.iter_mut().for_each(|x| *x = 0.0);
        }
        if seat == 0 {
            v.entry.driver = None;
            v.set_engines_on(false);
            v.craft.inputs.iter_mut().for_each(|x| *x = 0.0);
            occupied_grip(v, false);
        }
    }

    /// `exitVehicle`: the soldier at the vehicle's matrix × its seat's exit location, the
    /// vehicle's axes, its velocity × `exitSpeedMod`.
    pub fn exit(&mut self, soldier: usize, vehicle: usize) {
        let now = self.clock.time;
        let seat = self.vehicles[soldier].entry.position;
        // out of a guided missile: back into its launcher's seat 0, not onto the ground (the server's exit vehicle with
        // the player's launcher id set: the controlled object's exit, its `destroyOnExit`, then `enterVehicle(launcher)` and
        // and the id back to −1). A launcher gone or its seat taken: out as from any vehicle
        if let Some((l, back)) = self.return_seat(soldier, vehicle) {
            self.vehicles[soldier].entry.return_to = None;
            self.leave_seat(vehicle, seat);
            let s = &mut self.vehicles[soldier];
            s.entry.inside = None;
            s.entry.wants = None;
            if self.vehicles[vehicle].entry.destroy_on_exit {
                self.give_damage(vehicle, DESTROY_DAMAGE, crate::damage::Source::Destroy);
            }
            self.enter(soldier, l, back);
            return;
        }
        self.leave_seat(vehicle, seat);
        let exit = self.exit_place(vehicle, seat).unwrap_or(self.vehicles[vehicle].entry.seats[seat].exit);
        let v = &self.vehicles[vehicle];
        let (b, k) = (v.craft.body.clone(), v.entry.seats[seat].exit_speed_mod);
        let s = &mut self.vehicles[soldier];
        s.entry.inside = None;
        s.entry.wants = None;
        s.entry.last_toggle = Some(now);
        s.entry.exited = Some((vehicle, now));
        let p = to_parent(exit, &b.rows);
        s.craft.body.pos = [0, 1, 2].map(|i| b.pos[i] + p[i]);
        s.craft.body.rows = b.rows;
        s.craft.body.vel = b.vel.map(|x| x * k);
        s.craft.body.rotvel = [0.0; 3];
        // its physics comes back with nothing accumulated: the first update moves it at that
        // velocity, gravity from the next
        let body = &mut s.craft.body;
        body.acc = [0.0; 3];
        body.rotacc = [0.0; 3];
        body.friction = [0.0; 3];
        body.rot_friction = [0.0; 3];
        body.friction_count = 0.0;
        if self.vehicles[vehicle].entry.destroy_on_exit {
            self.give_damage(vehicle, DESTROY_DAMAGE, crate::damage::Source::Destroy);
        }
    }

    /// The object manager's messages check with the 0x16 of `setExitedFromVehicle`: posted for 2 s from
    /// the clock at the exit, delivered at the end of the objects' update of the frame whose clock
    /// reaches it, before that frame's collisions (at 30 Hz: the 59 frames from the exit on).
    pub(crate) fn exited_messages(&mut self, dt: f64) {
        let now = self.clock.time;
        for v in self.vehicles.iter_mut() {
            if v.entry.exited.is_some_and(|(_, t)| now - t + dt >= EXITED_FOR - 1e-6) {
                v.entry.exited = None;
            }
        }
    }

    /// The controlled object's allowed to exit test and where `exit` puts the soldier, in the vehicle's
    /// frame. With `hasRestrictedExit`, a segment from the seat's PlayerControlObject to its exit
    /// location raised 0.5 m must not meet another object (the level's or another vehicle; not
    /// soldiers, `PCOPredicator`); if it does, the other side (x the other way, still 0.5 m up)
    /// is tried, and is where the soldier comes out; if both are blocked it cannot leave (None).
    /// A submarine's (`getSubPos`) is first: not with its hatch under the engine's water level
    /// (`getWaterLevel`).
    pub(crate) fn exit_place(&self, vehicle: usize, seat: usize) -> Option<V3> {
        let v = &self.vehicles[vehicle];
        let s = &v.entry.seats[seat];
        if let Some(h) = s.hatch {
            let b = &v.craft.body;
            if (b.pos[1] + to_parent(h, &b.rows)[1]) < self.world.level.terrain.water_level as f32 {
                return None;
            }
        }
        let Some((at, rows, local)) = s.restricted else { return Some(s.exit) };
        let b = &v.craft.body;
        let world = |p: V3| {
            let q = to_parent(p, &b.rows);
            [b.pos[0] + q[0], b.pos[1] + q[1], b.pos[2] + q[2]]
        };
        let start = world(at);
        let clear = |l: V3| {
            let e = to_parent(l, &rows);
            let end = world([at[0] + e[0], at[1] + e[1], at[2] + e[2]]);
            let dir = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
            !self.segment_meets_objects(start, dir, vehicle)
        };
        let up = [local[0], local[1] + 0.5, local[2]];
        if clear(up) {
            return Some(s.exit);
        }
        let other = [-local[0], local[1] + 0.5, local[2]];
        clear(other).then(|| {
            let e = to_parent(other, &rows);
            [at[0] + e[0], at[1] + e[1], at[2] + e[2]]
        })
    }

    /// The level's objects and the vehicles but `skip` (no soldiers) that start → start + dir
    /// goes through.
    fn segment_meets_objects(&self, start: V3, dir: V3, skip: usize) -> bool {
        use crate::projectile::segment_near_sphere;
        let hits_obstacle = self.world.obstacles().iter().any(|o| {
            segment_near_sphere(start, dir, o.center, o.radius + 1.0)
                && crate::obstacle::pick_lod(&o.lods, 1).is_some_and(|m| crate::obstacle::segment_on_skipping(m, &o.rows, o.pos, o.scale, start, dir, false, Some(crate::projectile::NON_SOLID_MATERIAL)).is_some())
        });
        hits_obstacle
            || self.vehicles.iter().enumerate().any(|(j, v)| {
                j != skip && v.in_world() && v.soldier().is_none() && segment_near_sphere(start, dir, v.craft.body.pos, v.radius + 1.0) && !crate::projectile::mover_segment(v, start, dir).is_empty()
            })
    }

    /// `checkPlayerTriggers` in a vehicle: `c_PIMenuSelect<n>` takes seat n − 1 if it is free
    /// (`exitVehicle`, then `enterVehicle` of that PlayerControlObject). A person's soldier
    /// (`asked`: the key, not a wish kept) that finds a bot of its team there asks the bot for it:
    /// the bot's AI lets it go, and the seat is
    /// taken when free ([`Entry::wants`]). A person in it is left there.
    fn switch_seat(&mut self, soldier: usize, vehicle: usize, seat: usize, asked: bool) {
        let v = &self.vehicles[vehicle];
        if seat >= v.entry.seats.len() || self.vehicles[soldier].entry.position == seat {
            self.vehicles[soldier].entry.wants = None;
            return;
        }
        if v.entry.occupants[seat].is_some() {
            if asked {
                let wants = self.ask_bot_for_seat(soldier, vehicle, seat);
                self.vehicles[soldier].entry.wants = wants.then_some((seat, self.clock.time));
            }
            return;
        }
        let from = self.vehicles[soldier].entry.position;
        self.leave_seat(vehicle, from);
        self.enter(soldier, vehicle, seat);
    }

    /// `checkPlayerTriggers` with the AI on, a seat taken: when the soldier is a person's and the
    /// occupant a better bot of its team, the bot is told (the bot manager's player request player control object:
    /// its own AI object and the time, which `calculateVehicleUrgency` reads).
    fn ask_bot_for_seat(&mut self, soldier: usize, vehicle: usize, seat: usize) -> bool {
        let Some(occupant) = self.vehicles[vehicle].entry.occupants[seat] else { return false };
        let player = |s: usize| self.players.iter().position(|p| p.soldier == Some(s));
        let (Some(a), Some(b)) = (player(soldier), player(occupant)) else { return false };
        let (pa, pb) = (&self.players[a], &self.players[b]);
        if pa.ai || !pb.ai || pa.team != pb.team {
            return false;
        }
        let now = self.clock.time as f32;
        self.tell_bot_seat_wanted(b, vehicle, seat, now)
    }

    /// The driver of a destroyed vehicle (`killPlayer`): out (`exitVehicle`), its soldier
    /// destroyed (gone at once: it was inside) and given `KILL_DAMAGE`.
    /// The launcher a soldier flying a guided missile (`vehicle`) goes back to ([`crate::entry::Entry::return_to`]), if it is still
    /// there and its seat 0 free.
    pub(crate) fn return_seat(&self, soldier: usize, vehicle: usize) -> Option<(usize, usize)> {
        let (l, seat) = self.vehicles[soldier].entry.return_to?;
        let v = self.vehicles.get(l)?;
        (l != vehicle && v.in_world() && !v.armor.as_ref().is_some_and(|a| a.destroyed) && v.entry.occupants.get(seat) == Some(&None)).then_some((l, seat))
    }

    pub(crate) fn kill_driver(&mut self, vehicle: usize, soldier: usize) {
        let seat = self.vehicles[soldier].entry.position;
        self.leave_seat(vehicle, seat);
        self.vehicles[soldier].entry.inside = None;
        self.vehicles[soldier].entry.wants = None;
        self.give_damage(soldier, KILL_DAMAGE, crate::damage::Source::Kill);
        self.vehicles[soldier].removed = true;
    }

    /// The controlled object's frame update handling (a frame update, the real clock's): every
    /// half second gathered, a spawned vehicle that is occupied, within its spawner's `distance`
    /// or with a player's soldier within its bounding radius gets its whole `timeToLive` back;
    /// otherwise the time runs down, and once it is 0 it is given the time gathered ×
    /// `damageWhenLost`. Then a submarine's depth and oxygen ([`Submarine`]).
    pub(crate) fn abandoned(&mut self, dt: f32) {
        for k in 0..self.vehicles.len() {
            let v = &mut self.vehicles[k];
            if v.removed || v.entry.inside.is_some() || (v.entry.abandon.is_none() && v.entry.submarine.is_none()) {
                continue;
            }
            v.entry.frame_acc += dt;
            if v.entry.frame_acc < 0.5 {
                continue;
            }
            let acc = std::mem::take(&mut v.entry.frame_acc);
            if let Some(a) = v.entry.abandon.clone() {
                let (pos, r) = (v.craft.body.pos, v.radius);
                let near = |p: V3| (0..3).map(|i| (p[i] - pos[i]).powi(2)).sum::<f32>();
                let occupied = v.entry.occupants.iter().any(|o| o.is_some());
                let player_near = self.vehicles.iter().any(|s| {
                    !s.removed && s.entry.inside.is_none() && s.soldier().is_some_and(|m| m.has_player) && near(s.craft.body.pos) < r * r
                });
                if occupied || player_near || near(a.spawner).sqrt() <= a.distance {
                    self.vehicles[k].entry.abandon.as_mut().unwrap().timer = a.ttl;
                } else if a.timer == 0.0 {
                    self.give_damage(k, acc * a.damage, crate::damage::Source::Lost);
                } else {
                    self.vehicles[k].entry.abandon.as_mut().unwrap().timer = (a.timer - acc).max(0.0);
                }
            }
            let Some(sub) = self.vehicles[k].entry.submarine else { continue };
            let depth = self.vehicles[k].craft.body.underwater;
            if sub.max_depth > 0.0 && depth > sub.max_depth {
                self.give_damage(k, acc * sub.max_depth_damage, crate::damage::Source::Water);
            }
            let oxygen = if depth <= sub.periscope_max { (sub.oxygen + sub.fill * acc).min(1.0) } else { sub.oxygen - acc * sub.usage };
            let out = oxygen < 0.0;
            self.vehicles[k].entry.submarine.as_mut().unwrap().oxygen = oxygen.max(0.0);
            if out {
                for s in self.vehicles[k].entry.occupants.clone().into_iter().flatten() {
                    self.give_damage(s, acc * sub.oxygen_damage, crate::damage::Source::Water);
                }
            }
        }
    }

    /// The soldiers inside ride at their seat, with the vehicle's axes.
    pub(crate) fn ride(&mut self) {
        for i in 0..self.vehicles.len() {
            self.ride_one(i);
        }
    }

    /// A soldier in a vehicle on its seat.
    pub fn ride_one(&mut self, i: usize) {
        if let Some(k) = self.vehicles[i].entry.inside {
            let seat = self.vehicles[i].entry.position;
            let v = &self.vehicles[k].craft.body;
            let (rows, p) = (v.rows, to_parent(self.vehicles[k].entry.seats[seat].ride, &v.rows));
            let pos = [0, 1, 2].map(|j| v.pos[j] + p[j]);
            let b = &mut self.vehicles[i].craft.body;
            b.pos = pos;
            b.rows = rows;
        }
    }
}

#[cfg(test)]
mod fixed_guns {
    /// Berlin coop's `MachinegunSpawner`s make their `Stationary_mg42` (no mobile physics) as
    /// vehicles that do not move: a soldier takes its seat and it stays where it was made.
    #[test]
    fn a_fixed_gun_is_manned_and_stays() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let guns: Vec<usize> = (0..s.vehicles.len()).filter(|&i| s.vehicles[i].fixed).collect();
        assert_eq!(guns.len(), 3, "{:?}", s.vehicles.iter().map(|v| &v.template).collect::<Vec<_>>());
        assert!(guns.iter().all(|&i| s.vehicles[i].template == "Stationary_mg42" && s.vehicles[i].entry.seats.len() == 1));
        let k = s.add_player(1, "GermanSoldier", false);
        for _ in 0..30 * 30 {
            s.step();
            if s.players[k].alive {
                break;
            }
        }
        let soldier = s.players[k].soldier.unwrap();
        let g = guns[0];
        let at = s.vehicles[g].craft.body.pos;
        s.enter(soldier, g, 0);
        for _ in 0..90 {
            s.step();
        }
        assert_eq!(s.vehicles[g].entry.occupants[0], Some(soldier));
        assert_eq!(s.vehicles[g].craft.body.pos, at);
        assert_eq!(s.vehicles[g].craft.body.vel, [0.0; 3]);
    }

    /// `hasRestrictedExit` (the T34's, its exit 2.5 m to the left): free, the soldier leaves on
    /// the left; a tank there, on the right (0.5 m higher); tanks on both sides, not at all.
    #[test]
    fn a_tank_between_two_others_cannot_be_left() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let y = w.height_at(1500.0, 700.0).unwrap() as f32 + 1.0;
        let t = s.spawn_vehicle("T34", [1500.0, y, 700.0], [0.0; 3]).unwrap();
        assert!(s.vehicles[t].entry.seats[0].restricted.is_some());
        assert_eq!(s.exit_place(t, 0), Some([-2.5, 0.0, 0.0]));
        s.spawn_vehicle("T34", [1496.0, y, 700.0], [0.0; 3]).unwrap();
        assert_eq!(s.exit_place(t, 0), Some([2.5, 0.5, 0.0]));
        s.spawn_vehicle("T34", [1504.0, y, 700.0], [0.0; 3]).unwrap();
        assert_eq!(s.exit_place(t, 0), None);
    }

    /// `subPos` (the Gato's: its exit 6 m up its PlayerControlObject, 1.7 m under the hull's
    /// origin): under water nobody gets out; with the hatch (0.5 m above the exit) just over the
    /// water, they do.
    #[test]
    fn a_sunk_submarine_cannot_be_left() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Midway", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let water = w.level.terrain.water_level as f32;
        let g = s.spawn_vehicle("Gato", [1000.0, water - 10.0, 1000.0], [0.0; 3]).unwrap();
        let seat = s.vehicles[g].entry.seats.iter().position(|t| t.hatch.is_some()).unwrap();
        let h = s.vehicles[g].entry.seats[seat].hatch.unwrap()[1];
        assert!((h - 4.8).abs() < 1e-4, "{h}");
        assert_eq!(s.exit_place(g, seat), None);
        s.vehicles[g].craft.body.pos[1] = water - h + 0.1;
        assert!(s.exit_place(g, seat).is_some());
        s.vehicles[g].craft.body.pos[1] = water - h - 0.1;
        assert_eq!(s.exit_place(g, seat), None);
    }

    /// The soldier's setting of exited from vehicle: for the 59 frames after getting out (the message 0x16
    /// 2 s later, at 30 Hz), the soldier's `handleCollision` with that tank stops at once: walked
    /// into its side, he is pushed back but takes no hit from it (no crash, no ground); after,
    /// he does.
    #[test]
    fn a_soldier_just_out_does_not_hit_his_tank() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let a = s.add_player(1, "GermanSoldier", false);
        for _ in 0..30 * 30 {
            s.step();
            if s.players[a].alive {
                break;
            }
        }
        let soldier = s.players[a].soldier.unwrap();
        let y = w.height_at(1500.0, 700.0).unwrap() as f32 + 1.0;
        let t = s.spawn_vehicle("T34", [1500.0, y, 700.0], [0.0; 3]).unwrap();
        for _ in 0..60 {
            s.step();
        }
        s.enter(soldier, t, 0);
        s.step();
        assert_eq!(s.vehicles[soldier].entry.inside, Some(t));
        // walk him into the tank's side from where he got out, and count his hits on it
        let walk_in = |s: &mut crate::Sim, frames: usize| {
            let mut hits = 0;
            for _ in 0..frames {
                s.vehicles[soldier].craft.body.vel = [4.0, 0.0, 0.0];
                s.step();
                hits += s.vehicles[soldier].contacts.iter().flat_map(|c| c.hits.iter()).filter(|h| matches!(h.other, crate::damage::Other::Part(j, _) if j == t)).count();
            }
            hits
        };
        s.exit(soldier, t);
        let out = s.clock.time;
        assert_eq!(s.vehicles[soldier].entry.exited, Some((t, out)));
        assert_eq!(walk_in(&mut s, 30), 0);
        assert!(s.vehicles[soldier].craft.body.pos[0] < s.vehicles[t].craft.body.pos[0], "pushed back, not through");
        for _ in 30..58 {
            s.step();
        }
        assert!(s.vehicles[soldier].entry.exited.is_some(), "58 frames on");
        s.step();
        assert_eq!(s.vehicles[soldier].entry.exited, None, "59 frames on");
        let back = s.exit_place(t, 0).unwrap();
        let b = s.vehicles[t].craft.body.clone();
        s.vehicles[soldier].craft.body.pos = [b.pos[0] + back[0], b.pos[1] + back[1], b.pos[2] + back[2]];
        assert!(walk_in(&mut s, 30) > 0);
    }

    /// The controlled object's enter and `clearTeam`: an empty tank is nobody's; in it, a German
    /// makes it German, so a Briton cannot take its free seat (`BFfindEntryPoint` with his team);
    /// empty again, it is nobody's.
    #[test]
    fn a_tank_takes_the_team_of_who_is_in_it() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let y = w.height_at(1500.0, 700.0).unwrap() as f32 + 1.0;
        let t = s.spawn_vehicle("T34", [1500.0, y, 700.0], [0.0; 3]).unwrap();
        let german = s.spawn_vehicle("GermanSoldier", [1500.0, y, 703.0], [0.0; 3]).unwrap();
        let briton = s.spawn_vehicle("BritishSoldier", [1500.0, y, 697.0], [0.0; 3]).unwrap();
        s.vehicles[german].team = 1;
        s.vehicles[briton].team = 2;
        assert_eq!(s.vehicles[t].team, 0);
        assert_eq!(s.find_entry(briton).map(|e| e.0), Some(t));
        s.enter(german, t, 0);
        assert_eq!(s.vehicles[t].team, 1);
        assert_eq!(s.find_entry(briton), None);
        s.exit(german, t);
        assert_eq!(s.vehicles[t].team, 0);
        assert_eq!(s.find_entry(briton).map(|e| e.0), Some(t));
    }
}

#[cfg(test)]
mod guided {
    use crate::soldier::axis;

    /// XPack2's Wasserfall (`fireBarrel` with a PlayerControlObject for a projectile): the shot
    /// makes a `WasserfallRocket` at the ramp, at rest, its shooter out of the launcher and
    /// driving it; with the throttle its engine heats (`heatIncrement`) and at 1 stops; the
    /// warhead (`WasserFallGuns`, `destroyVehicleWhenNoAmmo`) destroys it; a rocket left
    /// (`destroyOnExit`) is destroyed too.
    #[test]
    fn the_wasserfall_is_flown_and_spent() {
        let game = bytefield_testdata::require!(Base, Xpack2);
        let w = crate::World::load(&game, "XPack2", "Essen", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let (x, z) = (1024.0, 1024.0);
        let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
        let l = s.spawn_vehicle("Wasserfall", [x, y, z], [0.0; 3]).unwrap();
        let soldier = s.spawn_vehicle("GermanSoldier", [x, y, z + 3.0], [0.0; 3]).unwrap();
        s.vehicles[soldier].team = 1;
        s.enter(soldier, l, 0);
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 1.0)]);
        let n = s.vehicles.len();
        for _ in 0..10 {
            s.step();
        }
        assert_eq!(s.vehicles.len(), n + 1, "one rocket");
        let r = n;
        assert_eq!(s.vehicles[r].template, "WasserfallRocket");
        assert_eq!(s.vehicles[soldier].entry.inside, Some(r));
        assert_eq!(s.vehicles[r].entry.driver, Some(soldier));
        assert_eq!(s.vehicles[l].entry.driver, None);
        assert!(s.vehicles[r].entry.destroy_on_exit);
        // full throttle: the fuel runs out (0.00084 a frame at rpm 1)
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0)]);
        let heat = |s: &crate::Sim| s.vehicles[r].craft.parts.iter().find_map(|p| if let crate::craft::Kind::Engine(e) = &p.kind { Some((e.heat, e.burnt)) } else { None }).unwrap();
        s.step();
        assert!(heat(&s).0 > 0.0);
        for _ in 0..30 * 60 {
            if heat(&s).1 || s.vehicles[r].armor.as_ref().unwrap().destroyed {
                break;
            }
            s.step();
        }
        let a = s.vehicles[r].armor.as_ref().unwrap();
        assert!(heat(&s).1 || a.destroyed, "burnt out or crashed: {:?}", heat(&s));
        // a second rocket, set off at once with the alt fire
        let (s2, l2) = {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            let l = s.spawn_vehicle("Wasserfall", [x, y, z], [0.0; 3]).unwrap();
            (s, l)
        };
        let mut s = s2;
        let soldier = s.spawn_vehicle("GermanSoldier", [x, y, z + 3.0], [0.0; 3]).unwrap();
        s.vehicles[soldier].team = 1;
        s.enter(soldier, l2, 0);
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 1.0)]);
        let n = s.vehicles.len();
        for _ in 0..10 {
            s.step();
        }
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::ALT_FIRE, 1.0)]);
        for _ in 0..10 {
            s.step();
        }
        assert!(s.vehicles[n].armor.as_ref().unwrap().destroyed, "the warhead destroys it");
    }

    /// DC's guided missiles take the same path (`Sim::launch`): from each launcher (the F-14A's: its RIO, the
    /// root `F14A`), whoever sits at the guided weapon's seat and fires flies the missile.
    #[test]
    fn dc_guided_missiles_are_flown() {
        let game = bytefield_testdata::require!(Base, Dc);
        let w = crate::World::load(&game, "DC_Final", "DC_No_Fly_Zone", "conquest").unwrap();
        // (at the middle of the map, on the sea: the ships float, the rest is set 300 m up, the F-14A flying)
        for (launcher, missile, up) in [("Patriot", "PatriotGuidedRocket", 300.0), ("SA-3", "SA-3GuidedRocket", 300.0), ("Fletcher", "Guided_Tomahawk", 3.0), ("Gato", "Guided_Tomahawk", 3.0), ("F14A", "Aim-54", 300.0)] {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            let (x, z) = (1024.0, 1024.0);
            let y = w.height_at(x as f64, z as f64).unwrap() as f32 + up;
            let l = s.spawn_vehicle(launcher, [x, y, z], [0.0; 3]).unwrap_or_else(|| panic!("{launcher}"));
            let v = &s.vehicles[l];
            let seat = v.weapons.iter().find(|(_, f)| f.template.projectile.eq_ignore_ascii_case(missile)).map(|(k, _)| v.craft.parts[*k].seat).unwrap_or_else(|| panic!("{launcher}: no {missile}"));
            let soldier = s.spawn_vehicle("USSoldier", [x, y, z + 3.0], [0.0; 3]).unwrap();
            s.vehicles[soldier].team = 2;
            s.enter(soldier, l, seat);
            s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 1.0), (axis::ALT_FIRE, 1.0)]);
            let n = s.vehicles.len();
            for _ in 0..30 {
                s.step();
                if s.vehicles.len() > n {
                    break;
                }
            }
            // (now in it; the keys go, or the missile's own warhead would go off with them)
            let r = (n..s.vehicles.len()).find(|&i| s.vehicles[i].template.eq_ignore_ascii_case(missile));
            assert!(r.is_some_and(|r| s.vehicles[soldier].entry.inside == Some(r)), "{launcher} seat {seat}: {:?}", s.vehicles[n..].iter().map(|v| &v.template).collect::<Vec<_>>());
            // The controlled object's enter: the missile leaves at `setForwardMod` m/s along its forward
            let r = r.unwrap();
            let b = &s.vehicles[r].craft.body;
            let speed = (b.vel[0] * b.vel[0] + b.vel[1] * b.vel[1] + b.vel[2] * b.vel[2]).sqrt();
            let forward_mod = w.db.object_template(missile).and_then(|t| t.get("setforwardmod")).and_then(|a| a.first()).and_then(|x| x.trim().parse::<f32>().ok()).unwrap();
            assert!((speed - forward_mod).abs() < 0.01 && (b.vel[2] - b.rows[2][2] * forward_mod).abs() < 0.01, "{missile}: {:?} at {speed} (forwardMod {forward_mod})", b.vel);
            // the F-14A's Aim-54 never touches its plane (no collision in 300 records)
            if launcher == "F14A" {
                s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 0.0), (axis::ALT_FIRE, 0.0)]);
                for _ in 0..30 {
                    s.step();
                }
                assert!(!s.journal.damage_log.iter().any(|d| d.vehicle == r && d.source == crate::damage::Source::Crash), "{missile}: {:?}", s.journal.damage_log.iter().filter(|d| d.vehicle == r).collect::<Vec<_>>());
                assert!(!s.vehicles[r].armor.as_ref().unwrap().destroyed);
            }
        }
    }

    /// F3: the Patriot's rocket is born at the barrel, 0,4 m past its tube's mouth, with its tubes level just
    /// under the top of `Patriot_Base`'s collision box, at 200 m/s (`setForwardMod`); the frame after, the segment from
    /// where it was to its vertices leaves the box through its top faces (normals (0,1,0) and (0, 0.9999993, −0.00117)
    /// in the original and here) and the crash kills it: 6185,15 of damage in the original
    /// (record 1537; `damageMod` 0,1 × `speedMod` 2 × |v|², and |v| is
    /// 175,86 there, 176,94 here: the frame's drag off 200, 0,6 % apart, so 6262 here), the launcher untouched (no damage
    /// event of its own). With the tubes raised (`c_PIMouseLookY` at −1; the original's rocket leaves at 42°) the box is
    /// cleared and it flies (no collision, 170 m/s after the first frame).
    #[test]
    fn the_patriot_s_rocket_hits_its_launcher_with_the_tubes_level_and_flies_with_them_raised() {
        let game = bytefield_testdata::require!(Base, Dc);
        let w = crate::World::load(&game, "DC_Final", "DC_Basrahs_Edge", "conquest").unwrap();
        for raised in [false, true] {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            let (x, z) = (1000.0, 1000.0);
            let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 0.3;
            let l = s.spawn_vehicle("Patriot", [x, y, z], [0.0; 3]).unwrap();
            let soldier = s.spawn_vehicle("USSoldier", [x - 2.0, y, z + 2.2], [0.0; 3]).unwrap();
            s.vehicles[soldier].team = 2;
            for _ in 0..30 {
                s.step();
            }
            s.enter(soldier, l, 0);
            if raised {
                s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::MOUSE_Y, -1.0)]);
                for _ in 0..60 {
                    s.step();
                }
                s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::MOUSE_Y, 0.0)]);
            }
            s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 1.0)]);
            let n = s.vehicles.len();
            for _ in 0..30 {
                s.step();
                if s.vehicles.len() > n {
                    break;
                }
            }
            let r = n;
            assert_eq!(s.vehicles[r].template, "PatriotGuidedRocket");
            s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 0.0)]);
            let born = s.clock.steps;
            let b = &s.vehicles[r].craft.body;
            assert!((b.vel[2] - b.rows[2][2] * 200.0).abs() < 0.01 && b.vel[1] == b.rows[2][1] * 200.0, "raised {raised}: {:?}", b.vel);
            let hp = s.vehicles[l].armor.as_ref().unwrap().hp;
            for _ in 0..30 {
                s.step();
            }
            let crashes: Vec<_> = s.journal.damage_log.iter().filter(|d| d.vehicle == r && d.source == crate::damage::Source::Crash).collect();
            if raised {
                assert!(crashes.is_empty(), "raised: {crashes:?}");
                let rb = &s.vehicles[r].craft.body;
                assert!(!s.vehicles[r].armor.as_ref().unwrap().destroyed && rb.pos[1] > y + 30.0, "raised: flies, at {:?}", rb.pos);
            } else {
                assert_eq!(crashes.len(), 1, "level: {crashes:?}");
                assert_eq!(crashes[0].step, born + 1);
                assert!((crashes[0].amount / 6185.15 - 1.0).abs() < 0.02, "level: {} of damage", crashes[0].amount);
                assert!(s.vehicles[r].armor.as_ref().unwrap().destroyed);
                assert_eq!(s.vehicles[soldier].entry.inside, Some(l), "level: the pilot is back in the Patriot");
                assert_eq!(s.vehicles[l].armor.as_ref().unwrap().hp, hp, "level: the launcher is untouched");
            }
        }
    }

    /// The one flying a guided missile does not die with it: it goes back to the launcher (a reference run of the original,
    /// a Patriot at rest on DC_Basrahs_Edge fired with its tubes level; the missile is made 10 frames after the
    /// press (`fireDelay 0.35`) and in the original hits its own launcher at once, 6185 of damage; the player is back in the
    /// Patriot's seat the same frame). Getting out of the missile does the same.
    #[test]
    fn the_guided_missile_s_pilot_goes_back_to_its_launcher() {
        let game = bytefield_testdata::require!(Base, Dc);
        let w = crate::World::load(&game, "DC_Final", "DC_Basrahs_Edge", "conquest").unwrap();
        for leave in [false, true] {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            let (x, z) = (1000.0, 1000.0);
            let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 0.3;
            let l = s.spawn_vehicle("Patriot", [x, y, z], [0.0; 3]).unwrap();
            let soldier = s.spawn_vehicle("USSoldier", [x - 2.0, y, z + 2.2], [0.0; 3]).unwrap();
            s.vehicles[soldier].team = 2;
            s.enter(soldier, l, 0);
            s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 1.0)]);
            let n = s.vehicles.len();
            for _ in 0..30 {
                s.step();
                if s.vehicles.len() > n {
                    break;
                }
            }
            let r = n;
            assert_eq!(s.vehicles[r].template, "PatriotGuidedRocket");
            assert_eq!(s.vehicles[soldier].entry.inside, Some(r));
            s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(axis::FIRE, 0.0)]);
            if leave {
                s.exit(soldier, r);
            } else {
                s.give_damage(r, 1000.0, crate::damage::Source::Destroy);
            }
            assert!(s.vehicles[r].armor.as_ref().unwrap().destroyed, "leave {leave}");
            assert_eq!(s.vehicles[soldier].entry.inside, Some(l), "leave {leave}: back in the Patriot");
            assert_eq!(s.vehicles[l].entry.driver, Some(soldier));
            assert!(!s.vehicles[soldier].removed && !s.vehicles[soldier].armor.as_ref().unwrap().destroyed, "leave {leave}: alive");
            assert_eq!(s.vehicles[soldier].entry.return_to, None);
        }
    }

}
