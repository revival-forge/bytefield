//! The body: what a bot can do with its hands and feet, without deciding what is advisable (that belongs to the team, the squad and the
//! individual). Each skill keeps its own state and writes to the axes a player would press: walking to a point
//! (`walker`), aiming and firing (`shooter`), driving (`driver`), sailing (`sailor`), flying (`pilot`),
//! laying mines and charges (`sapper`) and healing or repairing (`trade`).
pub mod weapons;
pub mod rng;
pub mod walker;
pub mod paths;
pub mod driver;
pub mod sailor;
pub mod trade;
pub mod pilot;
pub mod dogfight;
pub mod shooter;
pub mod sapper;

use crate::bot_api::{BotView, Nav};
use rng::Rng;
use walker::Walker;
use paths::{Searcher, Grid, Answer};
use driver::Driver;
use shooter::Shooter;
use std::sync::{Arc, Mutex, OnceLock};

/// What changes from one map to another. Only Berlin has targets; Basrah and El Alamein, a reference round. The rest is assumed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapProfile {
    /// foot range, m
    pub foot_range: f32,
    /// range from a vehicle, m
    pub vehicle_range: f32,
    /// lives that want to board a vehicle: not the specification's figure, because not all of them find a free one
    pub p_board: f32,
    /// seconds from respawning until it looks for a vehicle (median)
    pub board_wait: f32,
    /// seconds an attacker takes on average to enter a defended flag: in Berlin it gives the time of the B3 capture;
    /// 0: enters without waiting, for when the flags are hundreds of metres apart
    pub enter_wait: f32,
    /// whoever has fewer flags attacks with this probability ([`flags::choose`])
    pub p_fewer: f32,
    /// seconds without seeing anyone after which a defender chooses a flag again
    pub bored: f64,
    /// if it does not reach a vehicle, it looks for another, does not leave the car halfway nor linger at fixed weapons: walking 800 m is not an option
    pub insists: bool,
    /// whoever attacks goes to the second nearest flag with this probability
    pub p_second: f32,
    /// where a vehicle must be insisted on, lives that stop at a fixed weapon they happen to be near
    pub p_fixed: f32,
    /// the engineer uses mines and charges ([`zapador`])
    pub explosives: bool,
}

/// For open maps, with vehicles and distant flags.
pub(crate) const OPEN: MapProfile = MapProfile { foot_range: 100.0, vehicle_range: 225.0, p_board: 0.90, board_wait: 10.0, enter_wait: 0.0, p_fewer: 0.9, bored: 30.0, insists: true, p_second: 0.15, p_fixed: 0.8, explosives: true };

impl MapProfile {
    pub fn de(map: &str) -> MapProfile {
        match map.to_ascii_lowercase().as_str() {
            "berlin" => MapProfile { foot_range: 55.0, vehicle_range: 55.0, p_board: 0.90, board_wait: 20.0, enter_wait: ENTER_WAIT, p_fewer: 0.1, bored: BORED, insists: false, p_second: 0.0, p_fixed: 0.0, explosives: false },
            "basrah" => MapProfile { vehicle_range: 150.0, p_board: 0.47, board_wait: 14.0, ..OPEN },
            "el_alamein" => MapProfile { p_board: 0.95, ..OPEN },
            // a city, but with fewer walls than Berlin, and without the capture-time adjustments (assumed)
            "stalingrad" => MapProfile { foot_range: 70.0, vehicle_range: 120.0, p_board: 0.40, board_wait: 15.0, ..OPEN },
            // open, like El Alamein: assumed, not validated
            _ => OPEN,
        }
    }
}


/// Seconds without seeing anyone after which a defender chooses a flag again, in Berlin.
pub const BORED: f64 = 90.0;
/// Seconds an attacker takes on average to enter a defended flag, in Berlin: it gives the time of the first capture.
pub const ENTER_WAIT: f32 = 280.0;

/// What the bots of a round share: the path maps, which do not change.
#[derive(Default)]
pub struct Maps {
    infantry: OnceLock<Option<Grid>>,
    tanks: OnceLock<Option<Grid>>,
    /// the path searches, one queue per map
    looks_on_foot: Mutex<Searcher>,
    looks_tank: Mutex<Searcher>,
    /// where each team has laid mines: its players see them; the other's do not
    mines: Mutex<[Vec<[f32; 2]>; 2]>,
    /// the ways out of places the path map leaves closed that somebody has found this round: from where it ran and where it came
    /// out. Whoever spawns in the same place later knows the door, as a player who has played the map does.
    doors: Mutex<Vec<([f32; 2], [f32; 2])>>,
    /// the distance fields on foot to each goal asked so far (its cell and radius): [`Maps::way_m`]
    ways: Mutex<Vec<((i32, i32, u32), Arc<Vec<u16>>)>>,
}

impl Maps {
    /// The path maps of a level, already built: building them mid-game cost a step of up to 45 ms each (DC_No_Fly_Zone).
    pub fn prepare(world: &crate::World) -> Arc<Maps> {
        let m = Maps::default();
        std::thread::scope(|s| {
            s.spawn(|| m.infantry.get_or_init(|| world.nav_map(Nav::Infantry).map(|n| Grid::from_map(&n))));
            s.spawn(|| m.tanks.get_or_init(|| world.nav_map(Nav::Tank).map(|n| Grid::from_map(&n))));
        });
        Arc::new(m)
    }

    pub fn foot(&self, v: &BotView) -> Option<&Grid> {
        self.infantry.get_or_init(|| v.nav(Nav::Infantry).map(|m| Grid::from_map(&m))).as_ref()
    }
    pub fn tank(&self, v: &BotView) -> Option<&Grid> {
        self.tanks.get_or_init(|| v.nav(Nav::Tank).map(|m| Grid::from_map(&m))).as_ref().or_else(|| self.foot(v))
    }
    /// How long the way on foot from `from` to the circle of radius `r` around `to` is, m, over the infantry map's free cells
    /// (`None`: no map, or no way). The field of each goal is built the first time it is asked and kept for the round.
    pub fn way_m(&self, v: &BotView, from: [f32; 2], to: [f32; 2], r: f32) -> Option<f32> {
        let g = self.foot(v)?;
        let key = ((to[0] / paths::CELL).floor() as i32, (to[1] / paths::CELL).floor() as i32, r.round() as u32);
        let field = {
            let mut ways = self.ways.lock().unwrap_or_else(|e| e.into_inner());
            match ways.iter().find(|w| w.0 == key) {
                Some(w) => w.1.clone(),
                None => {
                    let f = Arc::new(g.distances_to(to[0], to[1], r));
                    ways.push((key, f.clone()));
                    f
                }
            }
        };
        g.way_in(&field, from[0], from[1])
    }

    /// The foot path from `de` to `a` for whoever is there, or let it wait.
    pub fn request_foot(&self, v: &BotView, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool) -> Answer {
        let Some(g) = self.foot(v) else { return Answer::NoPath };
        self.looks_on_foot.lock().unwrap_or_else(|e| e.into_inner()).request(g, tic(v), v.me.player, de, a, se_ve, true, &[], false)
    }
    /// The same, without passing near `avoid` if possible (bots3).
    /// `aside`: a goal shut in a small zone of the map is moved beside it ([`paths::Searcher::request`]).
    pub fn request_foot_avoiding(&self, v: &BotView, de: [f32; 2], a: [f32; 2], se_ve: &dyn Fn([f32; 2]) -> bool, avoid: &[[f32; 2]], aside: bool) -> Answer {
        let Some(g) = self.foot(v) else { return Answer::NoPath };
        self.looks_on_foot.lock().unwrap_or_else(|e| e.into_inner()).request(g, tic(v), v.me.player, de, a, se_ve, true, avoid, aside)
    }
    /// The vehicle path, without passing through `avoid`.
    pub fn request_tank(&self, v: &BotView, de: [f32; 2], a: [f32; 2], avoid: &[[f32; 2]], aside: bool) -> Answer {
        let Some(g) = self.tank(v) else { return Answer::NoPath };
        self.looks_tank.lock().unwrap_or_else(|e| e.into_inner()).request(g, tic(v), v.me.player, de, a, &|_| true, false, avoid, aside)
    }
    /// The known way out nearest `p`, within `r` of where it starts.
    pub fn door_near(&self, p: [f32; 2], r: f32) -> Option<([f32; 2], [f32; 2])> {
        let d = |q: &([f32; 2], [f32; 2])| (q.0[0] - p[0]).hypot(q.0[1] - p[1]);
        self.doors.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|q| d(q) < r).min_by(|a, b| d(a).total_cmp(&d(b))).copied()
    }
    /// Somebody got out: from `inside` it ran straight and came out at `outside`.
    pub fn door_found(&self, inside: [f32; 2], outside: [f32; 2]) {
        let mut doors = self.doors.lock().unwrap_or_else(|e| e.into_inner());
        if doors.len() < 64 && !doors.iter().any(|q| (q.0[0] - inside[0]).hypot(q.0[1] - inside[1]) < 2.0) {
            doors.push((inside, outside));
        }
    }
    /// The mines laid by this bot's team, as far as it has been told.
    pub fn mines_of(&self, team: u32) -> Vec<[f32; 2]> {
        self.mines.lock().unwrap_or_else(|e| e.into_inner())[(team as usize).saturating_sub(1).min(1)].clone()
    }
    pub fn laid(&self, team: u32, p: [f32; 2]) {
        self.mines.lock().unwrap_or_else(|e| e.into_inner())[(team as usize).saturating_sub(1).min(1)].push(p);
    }
}

/// The tick the round is at.
pub fn tic(v: &BotView) -> u64 {
    (v.time / v.dt as f64).round() as u64
}

/// The signed angle (degrees, + to the right) from flat direction `f` to flat direction `a`.
pub fn angle_between(f: [f32; 3], a: [f32; 2]) -> f32 {
    (f[2] * a[0] - f[0] * a[1]).atan2(f[0] * a[0] + f[2] * a[1]).to_degrees()
}

pub fn pitch(f: [f32; 3]) -> f32 {
    f[1].clamp(-1.0, 1.0).asin().to_degrees()
}

pub fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

pub fn flat(a: [f32; 3], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[1]).powi(2)).sqrt()
}

/// Chest height over [`crate::bot_api::Other::pos`] (the body is a metre above the ground).
pub const CHEST: f32 = 0.3;
/// What part of the heading error is corrected in one tick: the view arrives a tick late, and correcting it all at once makes it oscillate.
pub const TURN: f32 = 0.4;


/// Maximum the ground drops over two metres to walk there: more is a cliff, and a fall like that kills.
pub const MAX_FALL: f32 = 1.6;

/// Whether walking straight from `a` to `b` does not fall off a cliff.
pub fn no_fall(v: &BotView, a: [f32; 3], b: [f32; 2]) -> bool {
    let d = flat(a, b);
    let n = (d / 2.0).ceil().max(1.0) as usize;
    let high = |i: usize| {
        let t = i as f32 / n as f32;
        v.ground(a[0] + (b[0] - a[0]) * t, a[2] + (b[1] - a[2]) * t)
    };
    (0..n).all(|i| match (high(i), high(i + 1)) {
        (Some(h0), Some(h1)) => h0 - h1 < MAX_FALL,
        _ => true,
    }) && with_ground(v, a, b)
}

/// The highest jump down that [`with_ground`] takes as a way, m (7 m costs a soldier about a third of its health).
pub const SURVIVABLE_JUMP_M: f32 = 10.0;

/// How far ahead [`with_ground`] looks: the walker asks again every step, so the first metres are enough.
pub const FLOOR_AHEAD_M: f32 = 12.0;

/// Standing on an object (a platform, a roof well above the terrain), whether there is floor under every 2 m of the first
/// [`FLOOR_AHEAD_M`] from `a` towards `b`: the terrain test does not see where an object ends, and the path map, flat, does not
/// either (GC_Bespin: each team spawns on a platform 260 m above the ground, and the path to the flags runs off its edge).
pub fn with_ground(v: &BotView, a: [f32; 3], b: [f32; 2]) -> bool {
    // (the body is a metre above the feet)
    let pies = a[1] - 1.0;
    // (over water, a deck or a bridge: a ship is not one of the level's objects, and falling into the water does not kill)
    if v.ground(a[0], a[2]).is_none_or(|h| pies - h < SURVIVABLE_JUMP_M || h < v.water()) {
        return true;
    }
    let d = flat(a, b).min(FLOOR_AHEAD_M);
    if d < 0.5 {
        return true;
    }
    let l = flat(a, b);
    let dir = [(b[0] - a[0]) / l, (b[1] - a[2]) / l];
    let n = (d / 2.0).ceil() as usize;
    (1..=n).all(|i| {
        let r = (i as f32 * 2.0).min(d);
        let (x, z) = (a[0] + dir[0] * r, a[2] + dir[1] * r);
        // (a cone: the floor may go down [`MAX_FALL`] every 2 m, as down a staircase; and a jump that does not kill is a way down:
        // GC_Mos_Eisley's spawn platforms are left jumping 7 m, for a third of the health)
        v.hidden([x, pies + 1.0, z], [x, pies - (MAX_FALL * (i + 1) as f32).max(SURVIVABLE_JUMP_M), z])
    })
}


/// What a bot has decided about its destination zone: which flag and whether it attacks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub flag: usize,
    pub attacks: bool,
}

/// The rest of the bot, lent to a skill that needs to touch several things at once (the engineer with its explosives: walks along
/// the path, boards a vehicle, aims at the detonator and changes flag).
pub struct Surroundings<'a> {
    pub rng: &'a mut Rng,
    pub profile: &'a MapProfile,
    pub maps: &'a Arc<Maps>,
    pub player: usize,
    pub shooter: &'a mut Shooter,
    pub cam: &'a mut Walker,
    pub driver: &'a mut Driver,
    /// the pulse of one tick yes and the next no
    pub alternate: &'a mut bool,
    pub flag: &'a mut Option<Choice>,
    pub owner: &'a mut u32,
    pub centre: Option<[f32; 2]>,
}
