//! How a bot plays: a [`Mind`] sees what a player could see ([`BotView`]) and answers with the axes a player presses. It cannot
//! touch the simulation or change a rule of the game; nor can it see through walls, unless it chooses to ignore [`BotView::hidden`].
//!
//! A bot's axes are those of [`crate::soldier::axis`], [`crate::entry::USE`] and the rest of `crate::input::player_input`; inside
//! a vehicle the same axes reach the vehicle. Set a [`Mind`] for a team with [`crate::Sim::set_minds`]; players of that team then
//! are played by it and not by anything else.
//!
//! The same state must give the same axes: a mind keeps its own random generator, seeded with what it is given at birth.
use bytefield_formats::pathmap::PathMap;
use std::collections::BTreeMap;

/// A bot's way of playing.
pub trait Mind: Send + Sync {
    /// Once a tick while its player is alive. A soldier's axes as `(axis, value)`.
    fn think(&mut self, view: &BotView) -> Vec<(usize, f32)>;

    /// What a dead player picks on the spawn screen, each time its turn to spawn comes: a spawn group of its team
    /// (`None`: the first open to it) and a kit. `None` (the default): what it had, as the simple choice does.
    fn choose_spawn(&mut self, _view: &DeadView) -> Option<(Option<i64>, usize)> {
        None
    }

    /// Once a tick after [`Self::think`]: what it says on the radio, as a person picks it from the F1–F8 menus (a message, and the
    /// control point it names). `None` (the default): nothing. It goes through [`crate::Sim::say`], so the others hear it as they
    /// hear a person.
    fn speak(&mut self, _view: &BotView) -> Option<(crate::radio::Msg, Option<usize>)> {
        None
    }
}

/// What makes the mind of a team's new bot: the player's number goes in, for the seed.
pub type MindMaker = Box<dyn Fn(usize) -> Box<dyn Mind> + Send + Sync>;

/// The minds in a round, by team and by player.
#[derive(Default)]
pub struct Minds {
    makers: BTreeMap<u32, MindMaker>,
    pub(crate) live: BTreeMap<usize, Box<dyn Mind>>,
}

impl std::fmt::Debug for Minds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Minds for teams {:?}, {} live", self.makers.keys().collect::<Vec<_>>(), self.live.len())
    }
}

impl Minds {
    pub fn plays(&self, team: u32) -> bool {
        self.makers.contains_key(&team)
    }
}

/// The two pathfinding maps a level may ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Infantry,
    Tank,
}

/// What sort of thing a vehicle is, as a player sees at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VehicleClass {
    /// a gun fixed to the ground: it cannot be driven
    Fixed,
    Plane,
    Helicopter,
    /// hangs in the air by itself and goes where it is pointed, without banking (GC_Bespin's `cloudbus`): [`VehicleClass::floats`]
    Airship,
    Ship,
    Submarine,
    Tank,
    Car,
    /// legs instead of wheels or tracks (AT-AT, AT-ST): many moving limbs touching the ground without a spring
    Walker,
    Other,
}

/// Fewest jointed parts and unsprung limbs of a walker (an AT-ST has 27 and 11; the most of any wheeled or tracked vehicle is
/// 19 and 3).
const WALKER_JOINTS: usize = 22;
const WALKER_LEGS: usize = 6;

/// A floater's hull height, m, over which it floats at any height a map has.
pub const AIRSHIP_HULL_M: f32 = 1000.0;
/// What an airship's engines push along it, at most, for what they lift (`cloudbus` 1.7, Vanguard's `lancer` and `runner2` 1.8 and
/// 3.9; GC_Endor's speeder bike, which floats a metre over the ground, 10).
const AIRSHIP_PUSH: f32 = 5.0;
/// A craft lifted by its throttle pulled back whose engines push along it less than this many times what they lift never flies on
/// its wings: it falls with the throttle forward (GC_Bespin's cloud car pushes 1.4 times; the snowspeeder, X-Wing and TIEs 3-5 times
/// and fly as planes; `examples/air_envelope … --hover`).
const HANG_PUSH: f32 = 2.0;

impl VehicleClass {
    pub fn of(v: &crate::mover::Vehicle) -> Self {
        use crate::craft::Kind;
        if v.fixed {
            return Self::Fixed;
        }
        if Self::walks(v) {
            return Self::Walker;
        }
        let (mut land, mut tracks, mut water) = (false, false, false);
        for p in &v.craft.parts {
            let Kind::Engine(e) = &p.kind else { continue };
            land |= e.engine_type & 2 != 0;
            tracks |= e.engine_type & 4 != 0;
            water |= e.engine_type & 8 != 0;
        }
        let (lifts, pushes) = Self::thrust(v);
        // (a plane may have wheels with an engine; a Harrier lifts more than it pushes, but it pushes; a helicopter's others are dummies)
        if lifts + pushes > 0.0 {
            if pushes < lifts * 0.25 {
                return Self::Helicopter;
            }
            // (what hangs and pushes little: a speeder bike floats too, but pushes ten times what it lifts)
            return if Self::floats(v) && pushes < lifts * AIRSHIP_PUSH { Self::Airship } else { Self::Plane };
        }
        // (DC gives `submarineData` and a ship's engine to what is on land too: an amphibian is a land vehicle)
        match (tracks, land, water) {
            (true, ..) => Self::Tank,
            (_, true, _) => Self::Car,
            (_, _, true) if v.entry.submarine.is_some() => Self::Submarine,
            (_, _, true) => Self::Ship,
            _ => Self::Other,
        }
    }

    /// An aircraft that hangs in the air by itself: one of its floaters has a hull taller than any flight ([`AIRSHIP_HULL_M`]) and a
    /// lift that never goes to nothing, so the sky is water to it (GC_Bespin's `cloudbus`).
    /// A ground vehicle with a ship's engine (engine type 8: a propeller that pushes under the water line) crosses water by itself, as a
    /// player sees when driving it in (DC's BMP-2 and BRDM-2: ~5 and ~10 m/s afloat).
    pub fn swims(v: &crate::mover::Vehicle, class: VehicleClass) -> bool {
        matches!(class, VehicleClass::Car | VehicleClass::Tank | VehicleClass::Walker | VehicleClass::Other) && v.craft.parts.iter().any(|p| matches!(&p.kind, crate::craft::Kind::Engine(e) if e.engine_type & 8 != 0))
    }

    pub fn floats(v: &crate::mover::Vehicle) -> bool {
        v.craft.parts.iter().any(|p| matches!(&p.kind, crate::craft::Kind::Floating(f) if f.hull_height > AIRSHIP_HULL_M && f.min_lift > 0.0))
    }

    /// What its air engines give upwards and what they give along it (a rotor's thrust goes up its z axis), summed.
    pub fn thrust(v: &crate::mover::Vehicle) -> (f32, f32) {
        use crate::craft::Kind;
        let (mut lifts, mut pushes) = (0.0, 0.0);
        for (p, f) in v.craft.parts.iter().zip(&v.craft.frames_in_root()) {
            let Kind::Engine(e) = &p.kind else { continue };
            if e.engine_type & 1 != 0 && e.engine_type & 8 == 0 {
                if f.rows[2][1].abs() > 0.7 { lifts += e.torque } else { pushes += e.torque }
            }
        }
        (lifts, pushes)
    }

    /// The throttle that turns its lifting engines up ([`VehicleClass::thrust`]: an air engine whose thrust is mostly vertical,
    /// driven by `c_PIThrottle`): -1 when they speed up with the throttle pulled back (`acceleration` < 0 on the axis it drives:
    /// GC's VTOL racks), 1 when pushed, 0 with none (a plane, or a helicopter, whose rotor the throttle does not drive this way:
    /// its collective is a lever of its own). The torque of each decides between both (`examples/air_envelope … --hover`).
    pub fn lift_throttle(v: &crate::mover::Vehicle) -> f32 {
        Self::lift_throttle_of(v, Self::of(v))
    }

    /// Whether a craft its throttle lifts ([`VehicleClass::lift_throttle`], `lift`) flies hanging by that lift, as a helicopter, and
    /// not on its wings: what the throttle pushed lifts (the gunships, the skiff, the Lambda, the Falcon: the throttle is their
    /// collective, and they climb as long as it is pushed) and what pushes little along it ([`HANG_PUSH`]: the cloud car).
    /// TODO: the Lambda and the Falcon fly on their wings once going (55-65 m/s at full throttle, climbing all the while: the throttle
    /// lifts them too) and here they hang; a plane's pilot for them would hold its height with the throttle. DC's Harrier gives
    /// `lift` -1 and pushes half what it lifts, but does not rise at all here (`examples/air_envelope … AV-8H --hover`): it takes the
    /// runway, and only what rose hanging flies so ([`Pilot::fly_hover`]).
    pub fn hangs_of(v: &crate::mover::Vehicle, lift: f32) -> bool {
        if lift > 0.0 {
            return true;
        }
        let (lifts, pushes) = Self::thrust(v);
        lift < 0.0 && pushes < lifts * HANG_PUSH
    }

    /// [`VehicleClass::lift_throttle`] knowing its class already (only a plane is looked at).
    pub fn lift_throttle_of(v: &crate::mover::Vehicle, class: Self) -> f32 {
        use crate::craft::Kind;
        const THROTTLE: usize = 3;
        // (a speeder bike floats a metre over the ground: it is not lifted off it)
        if class != Self::Plane || Self::floats(v) {
            return 0.0;
        }
        let mut sum = 0.0f32;
        for (p, f) in v.craft.parts.iter().zip(&v.craft.frames_in_root()) {
            let (Kind::Engine(e), Some(b)) = (&p.kind, &p.bundle) else { continue };
            if e.engine_type & 1 == 0 || e.engine_type & 8 != 0 || f.rows[2][1].abs() <= 0.7 {
                continue;
            }
            for k in 0..3 {
                if b.input_to[k] == Some(THROTTLE) && b.acceleration[k] != 0.0 {
                    sum += e.torque * b.acceleration[k].signum();
                }
            }
        }
        if sum == 0.0 { 0.0 } else { sum.signum() }
    }

    /// A land engine over many jointed limbs that touch the ground without a spring: a wheel is sprung, a leg is not. A tank
    /// or a car has at most a couple of unsprung contacts and few joints (a sandcrawler has many contacts but few joints).
    fn walks(v: &crate::mover::Vehicle) -> bool {
        use crate::craft::Kind;
        let land = v.craft.parts.iter().any(|p| matches!(&p.kind, Kind::Engine(e) if e.engine_type & 6 != 0));
        let joints = v.craft.parts.iter().filter(|p| p.bundle.is_some()).count();
        let legs = v.contacts.iter().filter(|c| !c.is_root && c.anchor.is_some() && c.spring.is_none()).count();
        land && joints >= WALKER_JOINTS && legs >= WALKER_LEGS
    }

    /// Whether it holds still in the air (it can land on a point and wait there for its passengers), as a player tells by looking at
    /// it: a helicopter or an airship.
    pub fn hovers(self) -> bool {
        matches!(self, Self::Helicopter | Self::Airship)
    }

    /// Whether it is an aircraft.
    pub fn flies(self) -> bool {
        matches!(self, Self::Plane | Self::Helicopter | Self::Airship)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Walker => "walker",
            Self::Fixed => "fixed",
            Self::Plane => "plane",
            Self::Helicopter => "helicopter",
            Self::Airship => "airship",
            Self::Ship => "ship",
            Self::Submarine => "sub",
            Self::Tank => "tank",
            Self::Car => "car",
            Self::Other => "other",
        }
    }
}

/// The vehicle a bot is in.
#[derive(Clone, Debug)]
pub struct Inside {
    pub vehicle: usize,
    pub seat: usize,
    pub template: String,
    pub class: VehicleClass,
    pub pos: [f32; 3],
    /// the vehicle's axes as rows: right, up, forward
    pub rows: [[f32; 3]; 3],
    pub velocity: [f32; 3],
    /// where its seat's camera looks, right, up, forward (it turns with the turret), and from where: what a player in that seat sees
    pub camera: [[f32; 3]; 3],
    pub camera_pos: [f32; 3],
    /// what its seat's secondary key (`c_PIAltFire`) carries: nothing, things it drops (bombs: the weapon gives them no speed of
    /// their own) or things it fires (rockets, missiles). A pilot sees what hangs under its wings.
    pub alt_fire: AltFire,
    /// the weapons of its seat, in the vehicle's order: what a player learns of them by using them
    pub guns: Vec<SeatGun>,
    /// the throttle that lifts it straight up from the ground, as its pilot knows: -1 pulled back (GC's snowspeeder, X-Wing, TIE,
    /// cloud car), 1 pushed (the Falcon, the gunships), 0 none ([`VehicleClass::lift_throttle`])
    pub lift: f32,
    /// it flies hanging by that lift, as a helicopter, not on its wings ([`VehicleClass::hangs_of`])
    pub hangs: bool,
    /// a ground vehicle that crosses water by itself ([`VehicleClass::swims`])
    pub swims: bool,
}

/// A weapon of the seat a bot is in, as whoever sits there knows it from using it: the key that fires it, how fast its round leaves
/// (over the vehicle's own speed; a rocket's engine may push it on from there), whether the round is just let go (a bomb: the weapon
/// gives it no speed of its own), how many it fires a second, and the rounds left: in the magazine in use and in the rest.
#[derive(Clone, Debug, PartialEq)]
pub struct SeatGun {
    pub key: usize,
    pub speed: f32,
    pub drops: bool,
    pub rate: f32,
    pub rounds: (i32, i32),
    /// its magazine never runs out
    pub endless: bool,
}

/// What a seat's secondary fire key carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AltFire {
    #[default]
    None,
    Drops,
    Fires,
}

/// The bot itself.
#[derive(Clone, Debug)]
pub struct Me {
    pub player: usize,
    pub team: u32,
    pub pos: [f32; 3],
    pub eye: [f32; 3],
    /// where it looks, right, up, forward
    pub view: [[f32; 3]; 3],
    pub velocity: [f32; 3],
    /// hit points, from 0 to 1
    pub health: f32,
    pub kit: usize,
    /// what it carries, its own kit: the key of each thing and its name
    pub items: Vec<(usize, String)>,
    /// the rounds of each thing in `items`, in its order: in the magazine, and in the rest
    pub ammo: Vec<(i32, i32)>,
    /// where its shots leave from on foot: its eyes, moved by the weapon's own offset (a player knows its gun is not its eye)
    pub muzzle: [f32; 3],
    pub weapon: Option<String>,
    /// the weapon in hand gives one shot a press (not automatic)
    pub single_shot: bool,
    /// the rounds in the magazine in use, and in the rest
    pub rounds: (i32, i32),
    pub inside: Option<Inside>,
}

/// Another player's soldier.
#[derive(Clone, Debug)]
pub struct Other {
    pub player: usize,
    pub team: u32,
    pub pos: [f32; 3],
    pub in_vehicle: bool,
    /// the vehicle a soldier of its own team is in (the map shows one's mates on their vehicles); of an enemy, only while one sees
    /// the vehicle (no terrain or object between this bot's eye and it): a person sees who drives what, but not through a hill
    pub vehicle: Option<usize>,
    /// the kit of a soldier of its own team (what it carries shows); of an enemy, nothing
    pub kit: Option<usize>,
    /// the hit points of a soldier of its own team, from 0 to 1, as the bar on a comrade's name tag; of an enemy, nothing
    pub health: Option<f32>,
}

/// A way into a seat, in the world: where a person stands to get in (the doors of a jeep, the hatch of a tank).
#[derive(Clone, Copy, Debug)]
pub struct SeatEntry {
    pub pos: [f32; 3],
    /// how near to `pos` the use key works, m
    pub radius: f32,
}

/// A flag of the round.
#[derive(Clone, Debug)]
pub struct Flag {
    pub pos: [f32; 3],
    pub radius: f32,
    /// its team now, 0: nobody's
    pub team: u32,
    pub takeable: bool,
    /// it is changing hands (the flag on the map lowers or rises): its team is losing it, or a team is taking it
    pub contested: bool,
    /// who is taking it now, 0: nobody
    pub taker: u32,
    /// how far the change is, 0 (just begun) to 1 (about to change): the bar every player sees
    pub bar: f32,
}

/// A spawn group as the dead player's map shows it.
#[derive(Clone, Debug)]
pub struct SpawnGroupSeen {
    pub id: i64,
    /// the middle of its points open now (or of all of them, when none is)
    pub pos: [f32; 3],
    /// it can be chosen now: it has a point open to this player
    pub active: bool,
    pub points: usize,
    /// its points ride a vehicle (a carrier's, a landing craft's): one spawns aboard
    pub on_vehicle: bool,
}

/// What a dead player sees on the spawn screen: its team's groups on the map, the flags, its mates and the vehicles.
#[derive(Clone, Debug)]
pub struct DeadView {
    pub player: usize,
    pub team: u32,
    /// seconds since the round began
    pub time: f64,
    /// the kits its team offers
    pub kits: usize,
    pub groups: Vec<SpawnGroupSeen>,
    pub flags: Vec<Flag>,
    /// its team's live soldiers
    pub mates: Vec<Other>,
    pub vehicles: Vec<VehicleSeen>,
}

/// A vehicle on the map.
#[derive(Clone, Debug)]
pub struct VehicleSeen {
    pub vehicle: usize,
    pub template: String,
    pub class: VehicleClass,
    pub pos: [f32; 3],
    /// its team, 0: nobody's
    pub team: u32,
    pub driver_free: bool,
    pub seats: usize,
    /// hit points, from 0 to 1
    pub health: f32,
    /// its size: from its centre, m
    pub radius: f32,
    /// a land vehicle with a free driver's seat, not wrecked
    pub free_to_ride: bool,
    /// a gun fixed to the ground: it cannot be driven
    pub fixed: bool,
    /// an aircraft that hangs in the air by itself (an airship: floaters that lift it out of the water), as anyone sees it parked
    pub floats: bool,
    /// the seats nobody is in that can be got into (0: the driver's), in the vehicle's order
    pub free_seats: Vec<usize>,
}

/// A shot heard: where it was fired from, how long ago (s), and the gun (a player tells one gun from another by ear).
#[derive(Clone, Debug)]
pub struct Heard {
    pub pos: [f32; 3],
    pub age: f32,
    pub weapon: String,
}

/// Seconds a shot stays in the ear.
pub const HEARD_S: f64 = 3.0;

/// Something said on the radio and heard: what, by whom (a bot or a person), its team, where the speaker stood, how long ago (s)
/// and the control point it names.
#[derive(Clone, Debug)]
pub struct RadioHeard {
    pub msg: crate::radio::Msg,
    pub player: usize,
    pub team: u32,
    pub at: [f32; 3],
    pub age: f32,
    pub point: Option<usize>,
}

/// How far a message said out loud carries, m: a shout, heard by those around (the team's messages reach the whole team).
pub const VOICE_M: f32 = 50.0;

/// Where [`BotView::body_seen`] looks on a soldier standing, over [`crate::bot_api::Other::pos`] (the body, a metre over the ground), m: the chest,
/// the head, the shoulders and half the width between them.
pub const BODY_CHEST: f32 = 0.3;
pub const BODY_HEAD: f32 = 0.7;
pub const BODY_SHOULDER: f32 = 0.45;
pub const BODY_SHOULDER_HALF: f32 = 0.22;

/// What a bot can look at, this tick.
pub struct BotView<'a, 'w> {
    sim: &'a crate::Sim<'w>,
    pub me: Me,
    pub dt: f32,
    /// seconds since the round began
    pub time: f64,
    /// [`Self::others`] of this view, kept: each call draws a line of sight to every enemy in a vehicle, and a mind asks 3–4
    /// times per tick. Keyed by who looks and from where, in case `me` is changed after the first call.
    others: std::cell::RefCell<Option<((usize, u32, [u32; 3]), Vec<Other>)>>,
}

impl BotView<'_, '_> {
    /// Everyone else's soldier that is alive, of both teams. Whether this bot can see them is for [`Self::hidden`] to say.
    pub fn others(&self) -> Vec<Other> {
        let key = (self.me.player, self.me.team, self.me.eye.map(f32::to_bits));
        if let Some((k, v)) = &*self.others.borrow()
            && *k == key
        {
            return v.clone();
        }
        let v = self.others_uncached();
        *self.others.borrow_mut() = Some((key, v.clone()));
        v
    }

    fn others_uncached(&self) -> Vec<Other> {
        let s = self.sim;
        s.players
            .iter()
            .enumerate()
            .filter(|(k, p)| *k != self.me.player && p.alive)
            .filter_map(|(k, p)| {
                let v = s.vehicles.get(p.soldier?).filter(|v| !v.removed)?;
                let inside = v.entry.inside.and_then(|i| s.vehicles.get(i));
                let health = (p.team == self.me.team).then(|| v.armor.as_ref().map_or(1.0, |a| if a.max_hp > 0.0 { (a.hp / a.max_hp).clamp(0.0, 1.0) } else { 1.0 }));
                let seen = |c: &crate::mover::Vehicle| p.team == self.me.team || !s.hidden(self.me.eye, [c.craft.body.pos[0], c.craft.body.pos[1] + 1.5, c.craft.body.pos[2]]);
                let vehicle = v.entry.inside.filter(|_| inside.is_some_and(seen));
                Some(Other { player: k, team: p.team, pos: inside.map_or(v.craft.body.pos, |c| c.craft.body.pos), in_vehicle: inside.is_some(), vehicle, kit: (p.team == self.me.team).then_some(p.kit), health })
            })
            .collect()
    }

    /// The flags of the round.
    pub fn flags(&self) -> Vec<Flag> {
        self.sim.modes.conquest.as_ref().map_or(Vec::new(), |c| {
            c.points
                .iter()
                .map(|p| {
                    use crate::conquest::CpStatus;
                    let (contested, bar) = match p.status {
                        CpStatus::Losing => (true, 1.0 - p.lose / p.template.time_to_lose.max(1e-3)),
                        CpStatus::Taking => (true, 1.0 - p.get / p.template.time_to_get.max(1e-3)),
                        _ => (false, 0.0),
                    };
                    Flag { pos: p.pos, radius: p.template.radius, team: p.team, takeable: !p.template.unable_to_change_team, contested, taker: if contested { p.taker } else { 0 }, bar: bar.clamp(0.0, 1.0) }
                })
                .collect()
        })
    }

    /// The vehicles in the world.
    pub fn vehicles(&self) -> Vec<VehicleSeen> {
        (0..self.sim.vehicles.len()).filter_map(|j| self.vehicle(j)).collect()
    }

    /// The throttle that lifts vehicle `j` straight up, as whoever flies it learns ([`VehicleClass::lift_throttle`]); 0 for none.
    pub fn lift(&self, j: usize) -> f32 {
        self.sim.vehicles.get(j).map_or(0.0, VehicleClass::lift_throttle)
    }

    /// One of them, if it is in the world (what [`Self::vehicles`] says of it, without making the whole list).
    pub fn vehicle(&self, j: usize) -> Option<VehicleSeen> {
        let s = self.sim;
        let v = s.vehicles.get(j).filter(|v| v.in_world() && !v.is_soldier() && !v.entry.seats.is_empty())?;
        let class = VehicleClass::of(v);
        Some(VehicleSeen {
            vehicle: j,
            template: v.template.clone(),
            class,
            pos: v.craft.body.pos,
            team: v.team,
            driver_free: v.entry.driver.is_none(),
            seats: v.entry.seats.len(),
            health: v.armor.as_ref().map_or(1.0, |a| if a.max_hp > 0.0 { (a.hp / a.max_hp).clamp(0.0, 1.0) } else { 1.0 }),
            floats: matches!(class, VehicleClass::Plane | VehicleClass::Airship) && VehicleClass::floats(v),
            radius: v.radius,
            free_to_ride: s.free_to_ride(j),
            fixed: v.fixed,
            free_seats: v.entry.seats.iter().enumerate().filter(|(n, seat)| !seat.entries.is_empty() && v.entry.occupants.get(*n).is_none_or(|o| o.is_none())).map(|(n, _)| n).collect(),
        })
    }

    /// The shots of the last [`HEARD_S`] seconds fired within `radius` m of this bot by anyone else (own, and from its own
    /// vehicle, left out). Not who fired: only where, when and with what.
    pub fn heard(&self, radius: f32) -> Vec<Heard> {
        let s = self.sim;
        let me = &self.me;
        let mine = s.players[me.player].soldier;
        let my_car = me.inside.as_ref().map(|i| i.vehicle);
        let first = s.clock.steps.saturating_sub((HEARD_S / s.clock.dt) as u64);
        let r2 = radius * radius;
        s.journal.fired
            .iter()
            .rev()
            .take_while(|f| f.0 >= first)
            .filter(|f| Some(f.1) != mine && Some(f.1) != my_car)
            .filter(|f| (0..3).map(|k| (f.2.pos[k] - me.pos[k]).powi(2)).sum::<f32>() <= r2)
            .map(|f| Heard { pos: f.2.pos, age: ((s.clock.steps - f.0) as f64 * s.clock.dt) as f32, weapon: f.3.and_then(|p| s.projectiles.get(p)).map_or(String::new(), |p| p.template.name.clone()) })
            .collect()
    }

    /// What was said on the radio in the last `secs` seconds that this bot heard, newest first: its own team's messages, wherever
    /// it is, and what anyone said out loud within [`VOICE_M`] m (an enemy's shout too); never the other team's radio, nor what it
    /// said itself.
    pub fn radio(&self, secs: f32) -> Vec<RadioHeard> {
        let s = self.sim;
        let me = &self.me;
        let first = s.clock.steps.saturating_sub((secs as f64 / s.clock.dt) as u64);
        let r2 = VOICE_M * VOICE_M;
        s.journal.radio_log
            .iter()
            .rev()
            .take_while(|e| e.step >= first)
            .filter(|e| e.player != me.player)
            .filter(|e| if e.msg.local { (0..3).map(|k| (e.at[k] - me.pos[k]).powi(2)).sum::<f32>() <= r2 } else { e.team == me.team })
            .map(|e| RadioHeard { msg: e.msg, player: e.player, team: e.team, at: e.at, age: ((s.clock.steps - e.step) as f64 * s.clock.dt) as f32, point: e.point })
            .collect()
    }

    /// Whether the terrain or one of the level's objects that stop bullets is between two points. One point of a soldier is
    /// what a bullet sees; what a person sees of him is [`BotView::body_seen`].
    pub fn hidden(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        self.sim.hidden(a, b)
    }

    /// How far a vehicle may go from `a` towards `b` before it meets the terrain or one of the level's objects, m (the whole way
    /// when nothing is in between). What a driver sees of the walls around it: unlike [`BotView::hidden`] it counts what bullets go
    /// through, the faces seen from behind (a hangar's walls from inside) and the coarse mesh a big vehicle hits.
    pub fn craft_clear(&self, a: [f32; 3], b: [f32; 3]) -> f32 {
        self.sim.craft_clear(a, b)
    }

    /// The first part of a soldier's body that `eye` sees, chest, head, then each shoulder, or `None` when all four are hidden.
    /// `pos` is [`crate::bot_api::Other::pos`] (the body, a metre over the ground). A person sees a head over a wall or a shoulder past a corner;
    /// from a helicopter over a town, 40 m up, the chest alone is hidden 5-25 % of the time straight above and 50-80 % at a
    /// shallow angle (examples/air_sight), and the whole body less often.
    pub fn body_seen(&self, eye: [f32; 3], pos: [f32; 3]) -> Option<[f32; 3]> {
        let chest = [pos[0], pos[1] + BODY_CHEST, pos[2]];
        if !self.sim.hidden(eye, chest) {
            return Some(chest);
        }
        let head = [pos[0], pos[1] + BODY_HEAD, pos[2]];
        if !self.sim.hidden(eye, head) {
            return Some(head);
        }
        // the shoulders, across the line of sight
        let (dx, dz) = (pos[0] - eye[0], pos[2] - eye[2]);
        let l = (dx * dx + dz * dz).sqrt();
        if l < 1e-3 {
            return None;
        }
        let side = [-dz / l * BODY_SHOULDER_HALF, dx / l * BODY_SHOULDER_HALF];
        [1.0f32, -1.0].into_iter().map(|s| [pos[0] + s * side[0], pos[1] + BODY_SHOULDER, pos[2] + s * side[1]]).find(|p| !self.sim.hidden(eye, *p))
    }

    /// The ground's height, if on the map.
    pub fn ground(&self, x: f32, z: f32) -> Option<f32> {
        self.sim.world.height_at(x as f64, z as f64).map(|h| h as f32)
    }

    /// The height of the first thing below `(x, top, z)`, counting the level's objects (a platform, a deck, a roof), to 0.1 m: what
    /// one would stand on if dropped there. Only the ground (or the water) when nothing is in between; `None` outside the map.
    /// About a dozen line tests: for the head of a side, not for every bot every tick.
    pub fn floor(&self, x: f32, z: f32, top: f32) -> Option<f32> {
        let ground = self.ground(x, z)?.max(self.water());
        if top <= ground {
            return Some(ground);
        }
        let blocked = |y: f32| self.sim.hidden([x, top, z], [x, y, z]);
        // (the terrain test of `hidden` counts what is within 0.2 m of the ground)
        if !blocked(ground + 0.3) {
            return Some(ground);
        }
        let (mut free, mut hit) = (top, ground + 0.3);
        while free - hit > 0.1 {
            let m = 0.5 * (free + hit);
            if blocked(m) { hit = m } else { free = m }
        }
        Some(free)
    }

    /// The play area, as the minimap shows it (and the warning when one leaves it): x, z of its corner, width and depth, m.
    pub fn combat_area(&self) -> [f32; 4] {
        let level = &self.sim.world.level;
        let ws = level.terrain.world_size;
        level.combat_area.unwrap_or([0.0, 0.0, ws, ws]).map(|v| v as f32)
    }

    /// The height of the water's surface, which anyone can see.
    pub fn water(&self) -> f32 {
        self.sim.world.level.terrain.water_level as f32
    }

    /// The level's pathfinding map, or the one made for it when it ships none: 1 m a pixel, not 0 for blocked.
    pub fn nav(&self, kind: Nav) -> Option<PathMap> {
        self.sim.world.nav_map(kind).map(|m| (*m).clone())
    }

    /// Where one gets into a seat of a vehicle, in the world and as the vehicle stands now (its doors move with it): every entry
    /// of the seat, a seat it has not being the driver's. Empty for what is not in the world or has no seat.
    pub fn seat_entries(&self, vehicle: usize, seat: usize) -> Vec<SeatEntry> {
        self.sim.seat_entries(vehicle, seat)
    }

    /// The axes that take this bot on foot to a vehicle's driver's seat, pressing `use` when `press` (press it one tick in two).
    pub fn axes_to_enter(&self, vehicle: usize, press: bool) -> Vec<(usize, f32)> {
        self.sim.players[self.me.player].soldier.map_or(Vec::new(), |i| self.sim.seek_axes(i, vehicle, press))
    }

    /// The same, to one of its seats (a passenger's, a gunner's: [`VehicleSeen::free_seats`]); a seat it has not, the driver's.
    /// Straight at the entry, through the hull if it is on the far side: [`BotView::axes_to_seat_round`] goes round it.
    pub fn axes_to_seat(&self, vehicle: usize, seat: usize, press: bool) -> Vec<(usize, f32)> {
        self.sim.players[self.me.player].soldier.map_or(Vec::new(), |i| self.sim.seek_seat_axes(i, vehicle, seat, press, false))
    }

    /// [`BotView::axes_to_seat`], going round the vehicle's body by the entry's side when the entry is on its far side.
    pub fn axes_to_seat_round(&self, vehicle: usize, seat: usize, press: bool) -> Vec<(usize, f32)> {
        self.sim.players[self.me.player].soldier.map_or(Vec::new(), |i| self.sim.seek_seat_axes(i, vehicle, seat, press, true))
    }
}

impl<'w> crate::Sim<'w> {
    /// From now on, the bots of `team` are played by what `maker` makes.
    pub fn set_minds(&mut self, team: u32, maker: MindMaker) {
        self.drivers.minds.makers.insert(team, maker);
    }

    /// The computer's players get bots: `bots2`, written from what is seen from outside, on both teams.
    pub fn enable_bots(&mut self) {
        // (`--bot-ai bots3`)
        if crate::bots3::is_default() {
            return self.enable_bots3();
        }
        // The path maps, now and not when the first bot asks for them: making them costs up to 2.9 s per map (GC_Endor),
        // and in a round that was a simulation step that long.
        let maps = crate::bots2::Maps::prepare(self.world);
        let level = self.world.level.name.clone();
        for team in [1, 2] {
            self.set_minds(team, crate::bots2::maker_with(&level, 0, maps.clone()));
        }
    }

    /// A land vehicle with a driver's seat free and not wrecked: one a bot (or a script's `seek`) goes for.
    pub fn free_to_ride(&self, j: usize) -> bool {
        self.vehicles.get(j).is_some_and(|c| !c.removed && !c.is_soldier() && c.entry.driver.is_none() && !c.entry.seats.is_empty() && land_vehicle(c) && !c.armor.as_ref().is_some_and(|a| a.destroyed || a.hp <= 0.0))
    }

    /// The world positions of the entries of seat `seat` of vehicle `j` (the driver's when it has no such seat).
    pub fn seat_entries(&self, j: usize, seat: usize) -> Vec<SeatEntry> {
        let Some(c) = self.vehicles.get(j).filter(|c| !c.removed && !c.is_soldier()) else { return Vec::new() };
        let b = &c.craft.body;
        c.entry.seats.get(seat).or(c.entry.seats.first()).map_or(&[][..], |s| &s.entries[..])
            .iter()
            .map(|e| SeatEntry { pos: std::array::from_fn(|n| b.pos[n] + e.pos[0] * b.rows[0][n] + e.pos[1] * b.rows[1][n] + e.pos[2] * b.rows[2][n]), radius: e.radius })
            .collect()
    }

    /// The axes that take soldier `i` to vehicle `j`'s driver's nearest entry point (in the vehicle's frame) and in
    /// within its radius, the use pressed only when `press` (the simple bots press every other tick).
    pub fn seek_axes(&self, i: usize, j: usize, press: bool) -> Vec<(usize, f32)> {
        self.seek_seat_axes(i, j, 0, press, false)
    }

    /// The same, to seat `seat` of vehicle `j` (the driver's when it has no such seat); with `round`, round the body when the
    /// entry is on its far side.
    pub fn seek_seat_axes(&self, i: usize, j: usize, seat: usize, press: bool, round: bool) -> Vec<(usize, f32)> {
        let v = &self.vehicles[i];
        let pos = v.craft.body.pos;
        let fwd = v.movement.as_ref().map_or(v.craft.body.rows[2], |m| m.view(&v.craft.body, [0.0; 2]).0[2]);
        let d2 = |q: [f32; 3]| (q[0] - pos[0]).powi(2) + (q[2] - pos[2]).powi(2);
        let c = &self.vehicles[j];
        let b = &c.craft.body;
        let (at, r) = self.seat_entries(j, seat).into_iter().map(|e| (e.pos, e.radius)).min_by(|x, y| d2(x.0).total_cmp(&d2(y.0))).unwrap_or((b.pos, 4.0));
        let near = d2(at) < (r * 0.7).max(1.0).powi(2);
        // (the body is in the way when the entry is on its far side: go round it by the side the entry is on)
        let to = if near || !round { at } else { round_body([pos[0], pos[2]], [b.pos[0], b.pos[2]], [at[0], at[2]], c.radius).map_or(at, |w| [w[0], at[1], w[1]]) };
        let yaw = side(fwd, [to[0] - pos[0], to[2] - pos[2]]);
        let d2t = d2(to);
        vec![
            (crate::soldier::axis::YAW, 0.0),
            (crate::soldier::axis::THROTTLE, if near || yaw.abs() > 60.0 { 0.0 } else if d2t < 25.0 { 0.5 } else { 1.0 }),
            (crate::soldier::axis::MOUSE_X, (yaw / TURN).clamp(-10.0, 10.0)),
            (crate::soldier::axis::MOUSE_Y, 0.0),
            (crate::soldier::axis::FIRE, 0.0),
            (crate::soldier::axis::ACTION, 0.0),
            (crate::entry::USE, (near && press) as i32 as f32),
        ]
    }

    /// [`BotView::craft_clear`]: the terrain in samples every 2 m, then every object along the line, against the collision mesh a big
    /// body hits (LOD 0, [`crate::contact::faces_lod`]) and from both sides.
    pub(crate) fn craft_clear(&self, a: [f32; 3], b: [f32; 3]) -> f32 {
        let d: f32 = (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>().sqrt();
        let n = (d / 2.0).ceil().max(1.0) as usize;
        let mut free = d;
        if let Some(i) = (1..=n).find(|&i| {
            let t = i as f32 / n as f32;
            let p: [f32; 3] = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
            self.world.height_at(p[0] as f64, p[2] as f64).is_some_and(|h| h as f32 > p[1])
        }) {
            free = d * (i - 1) as f32 / n as f32;
        }
        let dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let obstacles = self.world.obstacles();
        let mut along = Vec::new();
        self.world.obstacle_grid().along(a, dir, &mut along);
        for &i in &along {
            let o = &obstacles[i as usize];
            if !crate::projectile::segment_near_sphere(a, dir, o.center, o.radius + 1.0) {
                continue;
            }
            if let Some(hit) = crate::obstacle::pick_lod(&o.lods, 0).and_then(|mesh| crate::obstacle::segment_on(mesh, &o.rows, o.pos, o.scale, a, dir, true)) {
                free = free.min((d + hit.along).max(0.0));
            }
        }
        free
    }

    /// Whether the terrain (samples every 2 m, a little over the ground) or one of the level's
    /// objects that stop bullets hides `b` from `a`.
    // TODO(perf): one `height_at` every 2 m makes long rays dear (a 400 m one over Coral_Sea's sea is 200); a distance cap
    // (nothing past the view distance is used) or one sample per terrain cell would make it 2-3x cheaper, but either may
    // change what a bot sees: only if a map goes over the bots' budget, and checked against the line-of-sight tests
    pub(crate) fn hidden(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let d: f32 = (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>().sqrt();
        let n = (d / 2.0).ceil().max(1.0) as usize;
        let terrain = (1..n).any(|i| {
            let t = i as f32 / n as f32;
            let p: [f32; 3] = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
            self.world.height_at(p[0] as f64, p[2] as f64).is_some_and(|h| h as f32 > p[1] - 0.2)
        });
        if terrain {
            return true;
        }
        let dir = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        // (only the obstacles along the line: the same ones in the same order)
        thread_local!(static ALONG: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) });
        let obstacles = self.world.obstacles();
        ALONG.with_borrow_mut(|along| {
            self.world.obstacle_grid().along(a, dir, along);
            along.iter().map(|&i| &obstacles[i as usize]).filter(|o| !o.projectiles_pass && crate::projectile::segment_near_sphere(a, dir, o.center, o.radius + 1.0)).any(|o| {
                crate::obstacle::pick_lod(&o.lods, 1).is_some_and(|mesh| crate::obstacle::segment_blocked_skipping(mesh, &o.rows, o.pos, o.scale, a, dir, false, Some(crate::projectile::NON_SOLID_MATERIAL)))
            })
        })
    }

    /// What the dead player `k` sees on the spawn screen, if its team has a mind to ask.
    fn dead_view(&self, k: usize) -> Option<DeadView> {
        let p = &self.players[k];
        let c = self.modes.conquest.as_ref()?;
        let team = p.team;
        let kits = c.teams.kits.get(team as usize - 1).map_or(1, |ks| ks.len().max(1));
        let groups = c
            .groups
            .iter()
            .filter(|(_, g)| g.team == team as i64 && !g.spawns.is_empty() && g.open_to(p.ai))
            .map(|(id, g)| {
                let open: Vec<&crate::conquest::SpawnPoint> = g.spawns.iter().filter(|sp| self.spawn_point_active(sp, team, p.ai)).collect();
                let of = if open.is_empty() { g.spawns.iter().collect() } else { open.clone() };
                let n = of.len() as f32;
                let pos: [f32; 3] = std::array::from_fn(|a| of.iter().map(|sp| sp.pos[a]).sum::<f32>() / n);
                SpawnGroupSeen { id: *id, pos, active: !open.is_empty(), points: g.spawns.len(), on_vehicle: g.spawns.iter().any(|sp| sp.on.is_some()) }
            })
            .collect();
        let mates = self
            .players
            .iter()
            .enumerate()
            .filter(|(q, m)| *q != k && m.alive && m.team == team)
            .filter_map(|(q, m)| {
                let v = self.vehicles.get(m.soldier?).filter(|v| !v.removed)?;
                let inside = v.entry.inside.and_then(|i| self.vehicles.get(i));
                let health = v.armor.as_ref().map_or(1.0, |a| if a.max_hp > 0.0 { (a.hp / a.max_hp).clamp(0.0, 1.0) } else { 1.0 });
                Some(Other { player: q, team, pos: inside.map_or(v.craft.body.pos, |c| c.craft.body.pos), in_vehicle: inside.is_some(), vehicle: v.entry.inside, kit: Some(m.kit), health: Some(health) })
            })
            .collect();
        let view = BotView { sim: self, me: Me { player: k, team, pos: [0.0; 3], eye: [0.0; 3], view: [[0.0; 3]; 3], velocity: [0.0; 3], health: 0.0, kit: p.kit, items: Vec::new(), ammo: Vec::new(), muzzle: [0.0; 3], weapon: None, single_shot: false, rounds: (0, 0), inside: None }, dt: self.clock.dt as f32, time: self.clock.time, others: Default::default() };
        Some(DeadView { player: k, team, time: self.clock.time, kits, groups, flags: view.flags(), mates, vehicles: view.vehicles() })
    }

    /// Before bot `k` spawns: its mind, if its team has one, picks the group and the kit ([`Mind::choose_spawn`]). The mind
    /// is kept for its life only when it chose: one that leaves it to the game is born, as before, at its first tick.
    pub(crate) fn ask_mind_spawn(&mut self, k: usize) {
        let team = self.players[k].team;
        if !self.players[k].ai || !self.drivers.minds.makers.contains_key(&team) {
            return;
        }
        let Some(view) = self.dead_view(k) else { return };
        let mut minds = std::mem::take(&mut self.drivers.minds);
        let make = &minds.makers[&team];
        let mut fresh = minds.live.remove(&k).unwrap_or_else(|| make(k));
        if let Some((group, kit)) = fresh.choose_spawn(&view) {
            let group = group.filter(|g| view.groups.iter().any(|s| s.id == *g));
            let p = &mut self.players[k];
            p.group = group;
            p.kit = kit.min(view.kits - 1);
            minds.live.insert(k, fresh);
        }
        self.drivers.minds = minds;
    }

    fn me(&self, k: usize) -> Option<Me> {
        let p = &self.players[k];
        let i = p.soldier?;
        let v = self.vehicles.get(i).filter(|v| !v.removed)?;
        let m = v.movement.as_ref()?;
        let (view, eye) = m.camera_in_world(&v.craft.body);
        let inside = v.entry.inside.and_then(|c| {
            let car = self.vehicles.get(c)?;
            let class = VehicleClass::of(car);
            let body = &car.craft.body;
            let frames = car.craft.frames_still_cameras();
            let (camera, camera_pos) = car.craft.parts.iter().position(|q| matches!(q.kind, crate::craft::Kind::Camera) && q.seat == v.entry.position).map_or((body.rows, body.pos), |c| (frames[c].child_rows, frames[c].pos));
            // (`c_PIAltFire` is key 23; a weapon that gives its round no speed lets it fall)
            let seat_alt = car.weapons.iter().filter(|(part, w)| car.craft.parts[*part].seat == v.entry.position && w.template.input_fire == 23);
            let alt_fire = seat_alt.fold(AltFire::None, |a, (_, w)| if w.template.velocity > 0.0 { AltFire::Fires } else if a == AltFire::None { AltFire::Drops } else { a });
            let guns = car
                .weapons
                .iter()
                .filter(|(part, _)| car.craft.parts[*part].seat == v.entry.position)
                .map(|(_, w)| {
                    let t = &w.template;
                    SeatGun { key: t.input_fire as usize, speed: t.velocity, drops: t.velocity <= 0.0, rate: t.round_of_fire, rounds: (w.mags.first().copied().unwrap_or(0), w.mags.iter().skip(1).sum()), endless: t.mag_size < 0 }
                })
                .collect();
            let lift = VehicleClass::lift_throttle_of(car, class);
            Some(Inside { vehicle: c, seat: v.entry.position, template: car.template.clone(), class, pos: body.pos, rows: body.rows, velocity: body.vel, camera, camera_pos, alt_fire, guns, lift, hangs: lift != 0.0 && VehicleClass::hangs_of(car, lift), swims: VehicleClass::swims(car, class) })
        });
        let w = m.weapon.as_ref();
        Some(Me {
            player: k,
            team: p.team,
            pos: v.craft.body.pos,
            eye,
            view,
            velocity: v.craft.body.vel,
            health: v.armor.as_ref().map_or(1.0, |a| if a.max_hp > 0.0 { (a.hp / a.max_hp).clamp(0.0, 1.0) } else { 1.0 }),
            kit: p.kit,
            items: m.weapon.iter().chain(m.stowed.iter()).map(|w| (w.template.item_index.max(0) as usize, w.template.name.clone())).collect(),
            ammo: m.weapon.iter().chain(m.stowed.iter()).map(|w| (w.mags.first().copied().unwrap_or(0), w.mags.iter().skip(1).sum())).collect(),
            muzzle: w.map_or(eye, |w| { let o = w.template.position; std::array::from_fn(|n| eye[n] + o[0] * view[0][n] + o[1] * view[1][n] + o[2] * view[2][n]) }),
            weapon: w.map(|w| w.template.name.clone()),
            single_shot: w.is_some_and(|w| w.template.fire_once),
            rounds: w.map_or((0, 0), |w| (w.mags.first().copied().unwrap_or(0), w.mags.iter().skip(1).sum())),
            inside,
        })
    }

    /// Before the simple bots, each tick.
    pub(crate) fn think_minds(&mut self) {
        if self.drivers.minds.makers.is_empty() {
            return;
        }
        let mut minds = std::mem::take(&mut self.drivers.minds);
        let mut orders = Vec::new();
        for k in 0..self.players.len() {
            let p = &self.players[k];
            if !p.ai || !p.alive {
                minds.live.remove(&k);
                continue;
            }
            let Some(make) = minds.makers.get(&p.team) else { continue };
            let Some(me) = self.me(k) else { continue };
            let mind = minds.live.entry(k).or_insert_with(|| make(k));
            let view = BotView { sim: &*self, me, dt: self.clock.dt as f32, time: self.clock.time, others: Default::default() };
            let axes = mind.think(&view);
            let said = mind.speak(&view);
            orders.push((k, self.players[k].soldier, axes, said));
        }
        // (what they say goes into the log after all have thought: in this tick no bot hears another, whatever their order)
        for (k, soldier, axes, said) in orders {
            if let Some(m) = soldier.and_then(|i| self.vehicles.get_mut(i)).and_then(|v| v.movement.as_mut()) {
                m.set_input(&crate::input::through_actions(&axes));
            }
            if let Some((msg, point)) = said {
                self.say(k, msg, point);
            }
        }
        self.drivers.minds = minds;
    }
}


/// Who drives the bots: a [`Mind`] for each team that has one. The bots' actions come from here; what they act on is
/// the rest of the `Sim`.
#[derive(Default)]
pub struct Drivers {
    /// the bots of the teams that have a [`Mind`]
    pub minds: Minds,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest mind there is: it stands and looks.
    struct Still(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Mind for Still {
        fn think(&mut self, view: &BotView) -> Vec<(usize, f32)> {
            assert!(view.me.health > 0.0);
            let _ = (view.others(), view.flags(), view.vehicles(), view.ground(view.me.pos[0], view.me.pos[2]));
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Vec::new()
        }
    }

    #[test]
    fn a_team_can_be_given_a_mind() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        let thoughts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let t = thoughts.clone();
        s.set_minds(1, Box::new(move |_| Box::new(Still(t.clone()))));
        for _ in 0..4 {
            s.join(0, true);
        }
        for _ in 0..300 {
            s.step();
        }
        assert!(thoughts.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }

    /// Picks the last group of its team and kit 3, and walks: what a person does on the spawn screen.
    struct Chooser(i64);
    impl Mind for Chooser {
        fn think(&mut self, _: &BotView) -> Vec<(usize, f32)> {
            vec![(crate::soldier::axis::THROTTLE, 1.0)]
        }
        fn choose_spawn(&mut self, view: &DeadView) -> Option<(Option<i64>, usize)> {
            assert!(!view.groups.is_empty() && view.kits > 1);
            let last = view.groups.iter().filter(|g| g.active).map(|g| g.id).max()?;
            self.0 = last;
            Some((Some(last), 3))
        }
    }

    /// A mind picks where and as what it spawns; one that does not (bots2) spawns as before.
    #[test]
    fn a_mind_chooses_its_spawn_group_and_kit() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        let chosen = std::sync::Arc::new(std::sync::Mutex::new(0i64));
        let c = chosen.clone();
        s.set_minds(1, Box::new(move |_| Box::new(Chooser(0)) as Box<dyn Mind>));
        let _ = c;
        let k = s.join(1, true);
        let mut guard = 0;
        while !(s.players[k].alive) && guard < 900 {
            s.step();
            guard += 1;
        }
        assert!(s.players[k].alive, "it spawned");
        assert_eq!(s.players[k].kit, 3, "the kit it chose");
        let groups = s.spawn_groups(k);
        let last = *groups.iter().max().unwrap();
        assert_eq!(s.players[k].group, Some(last), "the group it chose");
        let i = s.players[k].soldier.unwrap();
        let pos = s.vehicles[i].craft.body.pos;
        let c = s.modes.conquest.as_ref().unwrap();
        let near = c.groups[&last].spawns.iter().any(|sp| ((sp.pos[0] - pos[0]).powi(2) + (sp.pos[2] - pos[2]).powi(2)).sqrt() < 60.0);
        assert!(near, "born at one of its points: {pos:?}");
        assert!(s.drivers.minds.live.contains_key(&k), "the mind that chose lives on into its life");
        // a seat other than the driver's has its own entry: the axes aim somewhere else than seat 0's when the vehicle has one
        let j = (0..s.vehicles.len()).find(|&j| s.vehicles[j].in_world() && !s.vehicles[j].is_soldier() && s.vehicles[j].entry.seats.len() >= 2 && s.vehicles[j].entry.seats[1].entries.iter().any(|e| e.pos != s.vehicles[j].entry.seats[0].entries[0].pos)).expect("a vehicle with two seats");
        let a = s.seek_seat_axes(i, j, 0, false, true);
        let b = s.seek_seat_axes(i, j, 1, false, true);
        assert_eq!(a.len(), b.len());
        let view = BotView { sim: &s, me: s.me(k).unwrap(), dt: 1.0 / 30.0, time: s.clock.time, others: Default::default() };
        let seen = view.vehicle(j).unwrap();
        assert_eq!(seen.free_seats, (0..seen.seats).collect::<Vec<_>>(), "an empty vehicle's seats are all free");
    }

    /// A flag being taken shows it: the bar rises while the taker stays inside.
    #[test]
    fn a_flag_being_taken_shows_its_bar() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let quiet: Vec<Flag> = {
            let k = s.join(1, true);
            let view = BotView { sim: &s, me: Me { player: k, team: 1, pos: [0.0; 3], eye: [0.0; 3], view: [[0.0; 3]; 3], velocity: [0.0; 3], health: 1.0, kit: 0, items: Vec::new(), ammo: Vec::new(), muzzle: [0.0; 3], weapon: None, single_shot: false, rounds: (0, 0), inside: None }, dt: 1.0 / 30.0, time: 0.0, others: Default::default() };
            view.flags()
        };
        assert!(!quiet.is_empty() && quiet.iter().all(|f| !f.contested && f.taker == 0 && f.bar == 0.0), "nothing happens at the start");
        // a flag of team 1 that team 2 may take: put it in the losing state as a taker inside would
        let c = s.modes.conquest.as_mut().unwrap();
        let Some(p) = c.points.iter_mut().find(|p| p.team == 1 && !p.template.unable_to_change_team) else { return };
        p.status = crate::conquest::CpStatus::Losing;
        p.taker = 2;
        p.lose = p.template.time_to_lose * 0.25;
        let k = 0;
        let view = BotView { sim: &s, me: Me { player: k, team: 1, pos: [0.0; 3], eye: [0.0; 3], view: [[0.0; 3]; 3], velocity: [0.0; 3], health: 1.0, kit: 0, items: Vec::new(), ammo: Vec::new(), muzzle: [0.0; 3], weapon: None, single_shot: false, rounds: (0, 0), inside: None }, dt: 1.0 / 30.0, time: 0.0, others: Default::default() };
        let f = view.flags().into_iter().find(|f| f.contested).expect("the flag is contested");
        assert_eq!(f.taker, 2);
        assert!((f.bar - 0.75).abs() < 1e-3, "three quarters of the way: {}", f.bar);
    }

    /// Says "medic!" once, then listens: what a bot says goes through `Sim::say` as a person's would.
    struct Shouter(bool);
    impl Mind for Shouter {
        fn think(&mut self, _: &BotView) -> Vec<(usize, f32)> {
            Vec::new()
        }
        fn speak(&mut self, _: &BotView) -> Option<(crate::radio::Msg, Option<usize>)> {
            (!std::mem::replace(&mut self.0, true)).then_some((crate::radio::LOCAL_MEDIC, None))
        }
    }

    /// A bot speaks; its team hears the team's radio wherever it is, anyone hears a shout within `VOICE_M`, the other team never
    /// hears the team's radio, and nobody hears itself.
    #[test]
    fn a_bot_speaks_and_its_team_hears() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        s.set_minds(1, Box::new(|_| Box::new(Shouter(false))));
        for _ in 0..2 {
            s.join(1, true);
        }
        for _ in 0..300 {
            s.step();
        }
        let medic: Vec<_> = s.journal.radio_log.iter().filter(|e| e.msg == crate::radio::LOCAL_MEDIC).collect();
        assert!(!medic.is_empty() && medic.iter().all(|e| e.team == 1 && s.players[e.player].ai), "the bots said it: {medic:?}");
        assert!(medic.len() <= 2, "once each per life: {}", medic.len());

        // what each one hears, with the speakers placed by hand
        s.journal.radio_log.clear();
        let o = [0.0f32, 0.0, 0.0];
        let ev = |player: usize, team: u32, msg: crate::radio::Msg, at: [f32; 3]| crate::radio::RadioEvent { step: s.clock.steps, player, soldier: None, team, msg, at, point: None };
        let evs = [
            ev(10, 1, crate::radio::ARMOR_SPOTTED, [900.0, 0.0, 0.0]), // team 1's radio, far away
            ev(11, 2, crate::radio::ATTACK, [1.0, 0.0, 0.0]),          // team 2's radio, next to us
            ev(12, 2, crate::radio::LOCAL_COVER_ME, [30.0, 0.0, 0.0]), // an enemy's shout, near
            ev(13, 1, crate::radio::LOCAL_MEDIC, [100.0, 0.0, 0.0]),   // a mate's shout, too far
            ev(0, 1, crate::radio::PICKUP, o),                         // our own
        ];
        s.journal.radio_log.extend(evs);
        let me = |player: usize, team: u32| Me { player, team, pos: o, eye: o, view: [[0.0; 3]; 3], velocity: [0.0; 3], health: 1.0, kit: 0, items: Vec::new(), ammo: Vec::new(), muzzle: o, weapon: None, single_shot: false, rounds: (0, 0), inside: None };
        let heard = |s: &crate::Sim, player: usize, team: u32| -> Vec<usize> { BotView { sim: s, me: me(player, team), dt: 1.0 / 30.0, time: 0.0, others: Default::default() }.radio(5.0).iter().map(|h| h.player).collect() };
        assert_eq!(heard(&s, 0, 1), vec![12, 10], "team 1: its radio and the enemy's shout, not its own");
        assert_eq!(heard(&s, 1, 2), vec![12, 11], "team 2: its radio and its shout, not team 1's radio nor the far shout");
        // and it forgets: 6 s later nothing of it is within 5 s
        s.clock.steps += 180;
        assert!(heard(&s, 0, 1).is_empty());
    }

    /// Legs, not wheels or tracks, make a walker: the AT-AT and AT-ST of GC are, no wheeled or tracked vehicle of the base game is.
    #[test]
    fn walkers_are_told_by_their_legs() {
        let game = bytefield_testdata::require!(Base, Gc);
        let classes = |mod_: &str, level: &str| -> Vec<(String, VehicleClass)> {
            let Ok(w) = crate::World::load(&game, mod_, level, "conquest") else { return Vec::new() };
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            s.vehicles.iter().filter(|v| v.in_world() && !v.is_soldier() && !v.entry.seats.is_empty()).map(|v| (v.template.to_ascii_lowercase(), VehicleClass::of(v))).collect()
        };
        for (t, c) in classes("GCMOD", "GC_Hoth") {
            assert_eq!(c == VehicleClass::Walker, t.starts_with("atat") || t.starts_with("atst"), "{t}: {c:?}");
        }
        for level in ["Berlin", "Kursk", "El_Alamein"] {
            assert!(classes("bf1942", level).iter().all(|(_, c)| *c != VehicleClass::Walker), "{level}");
        }
    }

    /// What vehicle an enemy is in shows only while it is in sight; a mate's always (the map).
    #[test]
    fn an_enemy_vehicle_shows_when_seen() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        s.set_minds(1, Box::new(|_| Box::new(Walker)));
        s.set_minds(2, Box::new(|_| Box::new(Walker)));
        let (a, b) = (s.join(1, true), s.join(2, true));
        let mut guard = 0;
        while !(s.players[a].alive && s.players[b].alive) && guard < 900 {
            s.step();
            guard += 1;
        }
        let ride = (0..s.vehicles.len()).find(|&j| s.free_to_ride(j)).expect("a vehicle to ride");
        let soldier = s.players[b].soldier.unwrap();
        s.enter(soldier, ride, 0);
        let at = s.vehicles[ride].craft.body.pos;
        let mut me = s.me(a).unwrap();
        // above it: in sight; under the ground: the terrain is in between
        me.eye = [at[0], at[1] + 30.0, at[2]];
        let view = BotView { sim: &s, me: me.clone(), dt: 1.0 / 30.0, time: s.clock.time, others: Default::default() };
        let x = view.others().into_iter().find(|o| o.player == b).unwrap();
        assert!(x.in_vehicle && x.vehicle == Some(ride), "{x:?}");
        me.eye = [at[0], at[1] - 30.0, at[2]];
        let view = BotView { sim: &s, me, dt: 1.0 / 30.0, time: s.clock.time, others: Default::default() };
        let x = view.others().into_iter().find(|o| o.player == b).unwrap();
        assert!(x.in_vehicle && x.vehicle.is_none(), "{x:?}");
    }

    /// Going round a vehicle: straight when the entry is on this side, a waypoint on the entry's side when the body is in the way.
    #[test]
    fn the_way_to_a_far_entry_goes_round() {
        let c = [0.0, 0.0];
        // the entry is on the left side (x = -2); the bot is further out on that side: straight
        assert!(round_body([-8.0, 0.0], c, [-2.0, 0.0], 3.0).is_none(), "same side");
        // the bot is on the right side (x = 8): the body is in the way
        let w = round_body([8.0, 0.0], c, [-2.0, 0.0], 3.0).expect("the body is in the way");
        // the waypoint is a quarter turn round, on the ring outside the body, on the entry's side of the line
        assert!(w[0].abs() < 1.0 && w[1].abs() > 3.0, "{w:?}");
        // from the waypoint (z = 4), the entry is within 100 degrees: straight
        assert!(round_body([0.0, w[1]], c, [-2.0, 0.0], 3.0).is_none());
        // the side is the entry's: an entry at z = +2 sends it to +z, one at z = -2 to -z
        assert!(round_body([8.0, 0.0], c, [-1.0, 2.0], 3.0).unwrap()[1] > 0.0);
        assert!(round_body([8.0, 0.0], c, [-1.0, -2.0], 3.0).unwrap()[1] < 0.0);
    }

    /// The entries of a seat come in world coordinates, next to the vehicle.
    #[test]
    fn seat_entries_are_in_the_world() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let ride = (0..s.vehicles.len()).find(|&j| s.free_to_ride(j) && s.vehicles[j].entry.seats[0].entries.len() >= 2).expect("a vehicle with two entries");
        let at = s.vehicles[ride].craft.body.pos;
        let es = s.seat_entries(ride, 0);
        assert!(es.len() >= 2 && es.iter().all(|e| (0..3).all(|n| (e.pos[n] - at[n]).abs() < 15.0) && e.radius > 0.0), "{es:?}");
        assert!(s.seat_entries(usize::MAX, 0).is_empty());
    }

    /// A shot nearby is heard for a few seconds, one's own is not.
    #[test]
    fn a_shot_nearby_is_heard() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        s.set_minds(1, Box::new(|_| Box::new(Walker)));
        s.set_minds(2, Box::new(|_| Box::new(Walker)));
        let (a, b) = (s.join(1, true), s.join(1, true));
        let mut guard = 0;
        while !(s.players[a].alive && s.players[b].alive) && guard < 900 {
            s.step();
            guard += 1;
        }
        let (ia, ib) = (s.players[a].soldier.unwrap(), s.players[b].soldier.unwrap());
        let at = s.vehicles[ib].craft.body.pos;
        let fake = crate::projectile::Shot { pos: at, rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], vel: [0.0; 3], rotvel: [0.0; 3] };
        s.journal.fired.push((s.clock.steps, ib, fake.clone(), None));
        s.journal.fired.push((s.clock.steps, ia, fake, None));
        let view = BotView { sim: &s, me: s.me(a).unwrap(), dt: 1.0 / 30.0, time: s.clock.time, others: Default::default() };
        let d = ((at[0] - view.me.pos[0]).powi(2) + (at[2] - view.me.pos[2]).powi(2)).sqrt();
        let heard = view.heard(d + 5.0);
        assert_eq!(heard.len(), 1, "the other's shot, not one's own: {heard:?}");
        assert!(heard[0].age < 0.1);
        assert!(view.heard((d - 5.0).max(0.0)).is_empty() || d < 5.0, "out of earshot");
        for _ in 0..(HEARD_S * 30.0) as usize + 2 {
            s.step();
        }
        let view = BotView { sim: &s, me: s.me(a).unwrap(), dt: 1.0 / 30.0, time: s.clock.time, others: Default::default() };
        assert!(view.heard(1000.0).iter().all(|h| h.age <= HEARD_S as f32 + 0.1), "old shots fade");
    }

    /// Walks forward for ever.
    struct Walker;
    impl Mind for Walker {
        fn think(&mut self, _: &BotView) -> Vec<(usize, f32)> {
            vec![(crate::soldier::axis::THROTTLE, 1.0)]
        }
    }

    /// A dead bot's last order must not stay on its body (the user, 3-nov: one killed in El_Alamein slid along the ground).
    #[test]
    fn a_dead_bot_stops_walking() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        s.set_minds(1, Box::new(|_| Box::new(Walker)));
        let k = s.join(0, true);
        let alive = |s: &crate::Sim| s.players[k].soldier.filter(|_| s.players[k].alive);
        let mut guard = 0;
        while alive(&s).is_none() && guard < 900 {
            s.step();
            guard += 1;
        }
        let i = alive(&s).expect("it spawned");
        let start = s.vehicles[i].craft.body.pos;
        for _ in 0..90 {
            s.step();
        }
        let a = s.vehicles[i].craft.body.pos;
        let flat = |p: [f32; 3], q: [f32; 3]| ((p[0] - q[0]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        assert!(flat(start, a) > 1.0, "the walker walks: {}", flat(start, a));
        s.give_damage(i, 1000.0, crate::damage::Source::Hit);
        // (a second for what it carried to run out)
        for _ in 0..30 {
            s.step();
        }
        let b = s.vehicles[i].craft.body.pos;
        for _ in 0..30 {
            s.step();
        }
        let moved = flat(b, s.vehicles[i].craft.body.pos);
        assert!(moved < 0.2, "the body moved {moved} m in a second, dead");
    }
}

/// A car or a tank (its engine `c_ETCar`/`c_ETTank`: bit 2).
fn land_vehicle(v: &crate::mover::Vehicle) -> bool {
    v.craft.parts.iter().any(|p| matches!(&p.kind, crate::craft::Kind::Engine(e) if e.engine_type & 2 != 0))
}

/// A waypoint (x, z) to walk round a vehicle that is in the way to its entry, or None when the straight line is free: `me` is on
/// the far side of `center` from `entry` (more than 100 degrees round it) and the line passes through the body, whose reach is
/// the entry's distance from the center or four fifths of the vehicle's `radius`, whichever is more. The waypoint is on a circle
/// a metre and a half outside that, a quarter turn from `me` towards the entry's side; as the bot goes round, the entry's side
/// comes into reach and the line to it frees.
pub(crate) fn round_body(me: [f32; 2], center: [f32; 2], entry: [f32; 2], radius: f32) -> Option<[f32; 2]> {
    let m = [me[0] - center[0], me[1] - center[1]];
    let e = [entry[0] - center[0], entry[1] - center[1]];
    let (rm, re) = (m[0].hypot(m[1]), e[0].hypot(e[1]));
    if rm < 1e-3 || re < 1e-3 || m[0] * e[0] + m[1] * e[1] > -0.17 * rm * re {
        return None;
    }
    let reach = re.max(radius * 0.8);
    // distance from the center to the line me -> entry
    let l = [e[0] - m[0], e[1] - m[1]];
    let len = l[0].hypot(l[1]);
    if len < 1e-3 || (m[0] * l[1] - m[1] * l[0]).abs() / len > reach {
        return None;
    }
    let turn = if m[0] * e[1] - m[1] * e[0] >= 0.0 { 1.0 } else { -1.0 };
    let ring = reach + 1.5;
    let dir = [-turn * m[1] / rm, turn * m[0] / rm];
    Some([center[0] + dir[0] * ring, center[1] + dir[1] * ring])
}

/// The signed angle (degrees, + to the right) from the flat direction `f` to the flat `to`.
pub(crate) fn side(f: [f32; 3], to: [f32; 2]) -> f32 {
    (f[2] * to[0] - f[0] * to[1]).atan2(f[0] * to[0] + f[2] * to[1]).to_degrees()
}

/// Degrees of turn a mouse unit gives (`c_PIMouseLookX`: 3°).
pub(crate) const TURN: f32 = 3.0;
