//! The weapon in a soldier's hand: the magazines and the
//! bullet in the chamber, the time between shots and the reload, the deviation (from firing, from
//! moving, turning and jumping, a minimum, by pose) and how it turns the shot, and the recoil it
//! gives the soldier's aim. Read from the original game.
//!
//! What is random is not: the deviation takes two numbers from `random_seeds` (the first 1024
//! of MSVC's `rand` from seed 1) at the game's input index, and the recoil seeds its generator
//! with that index too.

use crate::projectile::{Shot, Weapon};
use std::sync::Arc;

use crate::vecmath::V3;

/// `random_seeds`: MSVC's `rand` from seed 1, its first 1024 values (the entry after them,
/// which index 1024 reads, is 0).
fn random_seed(i: usize) -> i32 {
    static TABLE: std::sync::OnceLock<Vec<i32>> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut s: u32 = 1;
        let mut v: Vec<i32> = (0..1024)
            .map(|_| {
                s = s.wrapping_mul(214013).wrapping_add(2531011);
                ((s >> 16) & 0x7fff) as i32
            })
            .collect();
        v.push(0);
        v
    });
    t[i.min(1024)]
}

/// One multiplication modulo 2³¹ − 1 as `Random` does it (Schrage-like, in halves).
fn mul_mod(x: i32, k: i32) -> i32 {
    let lo = (x & 0xffff).wrapping_mul(k);
    let u = (lo >> 16).wrapping_add((((x as u32) & 0x7fff_ffff) >> 16) as i32 * k);
    (u >> 15).wrapping_add(lo & 0xffff).wrapping_add((u & 0x7fff).wrapping_mul(0x10000))
}

fn fix_up(u: i32) -> i32 {
    let v = u.wrapping_add(0x8000_0001u32 as i32);
    if v < 0 { u } else { v }
}

fn fix_down(u: i32) -> i32 {
    let v = u.wrapping_sub(0x7fff_ffff);
    if v < 0 { u } else { v }
}

/// The random generator's raw lookup: × 24112, then × 26143 (630360016 in all), modulo 2³¹ − 1.
pub fn raw(state: i32) -> i32 {
    fix_down(mul_mod(fix_up(mul_mod(state, 0x5e30)), 0x661f))
}

/// A raw number as a uniform one in (0, 1].
fn unit(r: i32) -> f32 {
    ((r >> 7 | 1) + 1) as f32 * 5.960_464_5e-8
}

/// The recoil's generator: the input index through both multiplications.
fn recoil_seed(input: i32) -> i32 {
    fix_up(mul_mod(fix_up(mul_mod(input, 0x5e30)), 0x661f)) & 0x7fff_ffff
}

/// A `RandomDistribution` with its fourth value (`CRD_UNIFORM/a/b/flag`): kind 0 none (a), 1
/// uniform, 2 exponential, 3 normal; with the flag the value gets a random sign.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Distribution4 {
    pub kind: i32,
    pub a: f32,
    pub b: f32,
    pub symmetric: bool,
}

impl Distribution4 {
    pub fn parse(s: &str) -> Option<Distribution4> {
        let mut it = s.split('/');
        let kind = match it.next()?.trim().to_ascii_uppercase().as_str() {
            "CRD_NONE" => 0,
            "CRD_UNIFORM" => 1,
            "CRD_EXPONENTIAL" => 2,
            "CRD_NORMAL" => 3,
            _ => return None,
        };
        let mut n = || it.next().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
        let (a, b, f) = (n(), n(), n());
        Some(Distribution4 { kind, a, b, symmetric: f != 0.0 })
    }

    /// `getContinuousRandom` (or, symmetric, the same with a random sign after it). The normal
    /// one is not done (no hand weapon uses it): its mean. (pub: `examples/fncall_check`, against the original's, 318)
    pub fn sample(&self, state: &mut i32) -> f32 {
        let draw = |state: &mut i32| {
            *state = raw(*state);
            *state
        };
        let v = match self.kind {
            0 => self.a,
            1 => {
                let r = draw(state);
                // in the FPU's long double: exact here in f64
                let u = ((r >> 7 | 1) + 1) as f64 * 5.960_464_477_539_063e-8;
                (u * (self.b as f64 - self.a as f64) + self.a as f64) as f32
            }
            2 => {
                let r = draw(state);
                (unit(r).ln() as f64 * -(self.a as f64)) as f32
            }
            _ => self.a,
        };
        if self.symmetric {
            let r = draw(state);
            if unit(r) <= 0.5 {
                return -v;
            }
        }
        v
    }
}

/// How many shots until the next tracer: the draw, times the barrels if it has them (the weapon's constructor and
/// `createProjectile`). The draw comes from the weapon's seed, not the original's global `Rand`: only the ZPU-4 has a random
/// one (`CRD_UNIFORM/1/5`), so which of its first shots are tracers is level 2.
// TODO: draw from the original's global `Rand` if a level-1 comparison ever needs which ZPU-4 shots are tracers.
pub fn tracer_count(dist: &Distribution4, barrels: usize, seed: &mut i32) -> i32 {
    let n = dist.sample(seed);
    (if barrels != 0 { n * barrels as f32 } else { n }).round_ties_even() as i32
}

/// `recoilTabel` (going back) and `recoilTabel2`: the share of the recoil the aim gets, by the
/// frames left (it counts down from 20, or 8).
/// The delay of the message that ends the hand weapon's deafness (s). The original times it with `getExactTime`
/// (real time); ours counts simulation time, on purpose: the bench must be deterministic, and the
/// two only differ in a pause or a hitch.
pub const RAISE_DEAF: f32 = 0.1;

const RECOIL_TABLE: [f32; 21] = [0.0, 0.02, 0.03, 0.05, 0.07, 0.09, 0.12, 0.12, 0.09, 0.07, 0.05, 0.03, 0.01, -0.01, -0.03, -0.05, -0.07, -0.1, -0.13, -0.16, -0.2];
const RECOIL_TABLE2: [f32; 9] = [0.0, -0.01, -0.03, -0.05, -0.07, -0.1, -0.13, -0.16, -0.2];

/// The deviation below this (degrees) turns nothing (`fireBarrel`).
const MIN_DEVIATION: f32 = 0.01;
/// An axis moves the deviation past this.
const AXIS_DEAD: f32 = 0.01;

/// The recoil of a weapon (`setRecoilForceUp`, `setRecoilForceLeftRight`, `setGoBackOnRecoil`).
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Recoil {
    pub up: Distribution4,
    pub left_right: Distribution4,
    pub go_back: bool,
}

/// What the soldier's recoil is after a shot (`calcRecoil`): frames left, up and left-right.
pub fn calc_recoil(r: &Recoil, input_index: i32) -> (i32, f32, f32) {
    let mut s = recoil_seed(input_index);
    let up = r.up.sample(&mut s);
    let lr = r.left_right.sample(&mut s);
    (if r.go_back { 20 } else { 8 }, up, lr)
}

/// `yawRecoil` / `pitchRecoil`: what the aim gets this frame (with `count` frames left), in the
/// FPU's long double (`getDevMod` × the f32 size × table).
pub fn recoil_now(r: &Recoil, count: i32, size: f32, dev_mod: f32) -> f64 {
    let t = if r.go_back { RECOIL_TABLE.get(count as usize) } else { RECOIL_TABLE2.get(count as usize) };
    dev_mod as f64 * (size * t.copied().unwrap_or(0.0)) as f64
}

/// The hand weapon's own deviation (`HandFireArmsTemplate`): each `[max, add…, decrease]`.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct HandDeviation {
    /// `setTurnDev max mouseY mouseX decrease`
    pub turn: [f32; 4],
    /// `setSpeedDev max throttle strafe decrease`
    pub speed: [f32; 4],
    /// `setMiscDev max add decrease` (jumping)
    pub misc: [f32; 3],
    /// `setDevMod stand crouch lie`
    pub dev_mod: [f32; 3],
}

/// One accumulator of deviation update: towards `max × mod` while `add` (already
/// × mod) is asked, down by `decrease / mod` a frame otherwise.
fn accumulate(acc: &mut f32, add: f32, max: f32, dec: f32, m: f32, _inv: f32) -> f32 {
    let down = dec as f64 / m as f64;
    if add == 0.0 {
        *acc = (*acc as f64 - down) as f32;
        if *acc < 0.0 {
            *acc = 0.0;
        }
        return *acc;
    }
    let top = max * m;
    if top <= *acc {
        *acc = (*acc as f64 - down) as f32;
        if *acc < top {
            *acc = top;
        }
    } else {
        *acc = ((add * m) as f64 - down + *acc as f64) as f32;
        if *acc > top {
            *acc = top;
        } else if *acc < 0.0 {
            *acc = 0.0;
        }
    }
    *acc
}

/// A weapon's state (`FireArms`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct FireArms {
    #[serde(skip)]
    pub template: Arc<Weapon>,
    /// the rounds in each magazine, the one in use first
    pub mags: Vec<i32>,
    /// a round in the chamber (`fireBarrel` takes it, `fireFinished` puts the next)
    pub chamber: bool,
    /// time to the reload's end, the clip's eject, the next shot, the end of an overheat
    pub reload: f32,
    pub eject: f32,
    pub fire: f32,
    pub overheat: f32,
    /// the deviation: in all, from firing, extra; turning, moving,
    /// jumping
    pub dev: f32,
    pub dev_fire: f32,
    pub dev_extra: f32,
    /// (`setFakeFire`) it fires without projectiles (the AI simulates the hit)
    pub fake: bool,
    pub dev_hand: [f32; 3],
    /// the generator after the last shot's deviation
    pub seed: i32,
    /// its clock, the clock at the last fire message, and `fireOnce` held
    pub time: f32,
    /// `magType` 1: seconds since the hand weapon's disable put it away (there a clock)
    pub away: f32,
    pub fired_at: f32,
    pub latched: bool,
    /// just raised (the hand weapon's enable), it hears no message until its own 0x16, posted
    /// for 0.1 s on: the clock when raised
    pub deaf_since: Option<f32>,
    /// the projectile of its last scout mark (`magType` 2), set off when it marks again
    pub mark: Option<usize>,
    /// `fireDelay`'s countdown and a shot waiting for it
    pub delay: f32,
    pub queued: bool,
    /// the heat, or with `velocityDependentOnHeat` the throw's charge (0..1)
    pub heat: f32,
    /// the alt-fire key was down since the last update (a throw charging)
    pub charging: bool,
    /// the charge the shot leaving now takes (before `Fire` empties it)
    pub charge: f32,
    /// `hideDuringFireTime`'s countdown after a shot
    pub hide: f32,
    /// a reload ended with rounds
    pub loaded: bool,
    /// shots before the next tracer
    pub tracer_in: i32,
    /// the active barrel
    pub barrel: usize,
    /// the zoom (−1 off; on the server 10000 when on: no render view to ask)
    pub zoom: f32,
    /// the rounds left as `fireFinished` counts them down (from `getTotalAmmo`, never
    /// below 0); `destroy_vehicle`: the last one went with `destroyVehicleWhenNoAmmo`, for the
    /// vehicle to destroy its PlayerControlObject
    pub rounds_left: i32,
    pub destroy_vehicle: bool,
    /// the clock at the last alt-fire message, and `altFireOnce` held
    pub alt_at: f32,
    pub alt_latched: bool,
    /// the last message or shot came from a human player (the clip's eject)
    pub human: bool,
    /// the AI's refill left over, a fraction of a round (the bots' refill)
    pub ai_refill: f32,
}

/// What a message or an update asks of the soldier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ask {
    Nothing,
    /// a shot leaves: the soldier fires it (`fireBarrel` from its camera) and, with a recoil,
    /// takes it (`calcRecoil`)
    Fire,
    /// `handleMessage(9)` to the soldier (autoReload, the reload input)
    Reload,
    /// a message for the weapon (the soldier's frame queues the fire input's)
    Message(i32),
    /// The soldier's trigger fire animation: the upper body to `Ub_Fire` (`Ub_LieFire`)
    FireAnimation,
    /// The soldier's select best loaded weapon (the last grenade thrown)
    SelectBest,
}

impl FireArms {
    /// The constructor: `numOfMag` full magazines (one with −1: endless), a round in the chamber.
    pub fn new(template: Arc<Weapon>) -> FireArms {
        let n = template.num_of_mag.max(1) as usize;
        let mags = vec![template.mag_size; n];
        let mut f = FireArms {
            mags,
            chamber: true,
            reload: 0.0,
            eject: 0.0,
            fire: 0.0,
            overheat: 0.0,
            dev: template.min_dev,
            dev_fire: 0.0,
            dev_extra: 0.0,
            fake: false,
            dev_hand: [0.0; 3],
            seed: 0,
            time: 0.0,
            away: 0.0,
            fired_at: -1.0,
            latched: false,
            deaf_since: None,
            mark: None,
            delay: 0.0,
            queued: false,
            heat: 0.0,
            charging: false,
            charge: 0.0,
            hide: 0.0,
            loaded: true,
            tracer_in: template.tracer.as_ref().map_or(0, |t| tracer_count(&t.1, template.barrels.len(), &mut 0)),
            barrel: 0,
            zoom: -1.0,
            rounds_left: 0,
            destroy_vehicle: false,
            alt_at: -1.0,
            alt_latched: false,
            human: false,
            ai_refill: 0.0,
            template,
        };
        f.rounds_left = f.total_ammo().max(0);
        f
    }

    fn rate(&self) -> f32 {
        1.0 / self.template.round_of_fire
    }

    /// The weapon's part of the soldier's can-fire test: the next shot is ready
    /// (`getTimeToFireFinished` ≤ 0) or, with `timeToNotAllowChange`, no more than the time
    /// between shots less that away (`getTimeToNotAllowChange`).
    pub fn not_firing(&self) -> bool {
        let t = self.template.time_to_not_allow_change;
        if t == -1.0 { self.fire <= 0.0 } else { self.fire <= self.rate() - t }
    }

    fn ammo(&self) -> i32 {
        self.mags.first().copied().unwrap_or(0)
    }

    /// The rounds in the clip.
    pub fn clip(&self) -> i32 {
        self.ammo()
    }

    /// `getTotalAmmo` (−1: endless).
    pub fn total_ammo(&self) -> i32 {
        if self.template.num_of_mag == -1 || self.ammo() == -1 { -1 } else { self.mags.iter().sum() }
    }

    /// `reloadAmmo(n)`: `n` rounds more, none with `magSize` 0 or −1 or `numOfMag` −1. With one
    /// magazine they go into it (the one in use); with more, into the spare ones in turn (from
    /// the second), each up to `magSize`. What is left over. (The branch of the template's flag,
    /// whole magazines from the rounds gathered, is not seen: off. Its autoReload message at the
    /// end is the one `handle_update` asks for anyway.)
    pub fn reload_ammo(&mut self, mut n: u32) -> u32 {
        let (size, count) = (self.template.mag_size, self.template.num_of_mag);
        if size == 0 || size == -1 || count == -1 || n == 0 {
            return n;
        }
        if count == 1 {
            if self.mags.is_empty() {
                self.mags.push(0);
            }
            self.mags[0] += n as i32;
            if self.mags[0] > size {
                n = (self.mags[0] - size) as u32;
                self.mags[0] = size;
            } else {
                n = 0;
            }
            return n;
        }
        for k in 0..count.max(0) as usize {
            if k == self.mags.len() {
                self.mags.push(0);
            }
            if self.mags[k] != size && k != 0 {
                self.mags[k] += n as i32;
                if self.mags[k] <= size {
                    return 0;
                }
                n = (self.mags[k] - size) as u32;
                self.mags[k] = size;
                if n == 0 {
                    break;
                }
            }
        }
        n
    }

    /// `isReadyToUseFire`: not latched, ejecting, reloading or overheated, the last shot's time
    /// gone (or just started), and a round in the clip (or no clip, or endless).
    pub fn ready_to_fire(&self) -> bool {
        if self.latched || self.eject > 0.0 || self.reload > 0.0 || self.overheat > 0.0 {
            return false;
        }
        if self.fire > 0.0 && self.fire < self.rate() {
            return false;
        }
        self.mags.is_empty() || self.ammo() >= 1 || self.ammo() == -1
    }

    /// `isAnimationReadyToUse`: whether the fire input reaches the animations.
    pub fn animation_ready(&self) -> bool {
        if self.latched || self.eject > 0.0 {
            return false;
        }
        if self.reload > 0.0 || self.overheat > 0.0 {
            return false;
        }
        let t = &self.template;
        if t.fire_once {
            if self.queued && self.delay < t.fire_delay {
                return false;
            }
            if self.fire == self.rate() {
                return true;
            }
            if self.fire > 0.0 && self.fire < self.rate() {
                return false;
            }
        }
        self.mags.is_empty() || self.ammo() >= 1 || self.ammo() == -1
    }

    /// The weapon's message handling: 6 (fire), 7 (alt fire: the zoom), 8 (eject the clip), 9
    /// (reload). `human`: from a human player (none, a bot or a remote one: not).
    pub fn handle_message(&mut self, msg: i32, human: bool) -> Ask {
        let t = self.template.clone();
        self.human = human;
        if self.deaf_since.is_some() {
            return Ask::Nothing;
        }
        match msg {
            6 => {
                self.fired_at = self.time;
                if self.latched || self.reload > 0.0 || self.overheat > 0.0 {
                    return Ask::Nothing;
                }
                // `magType` 1 (the medic's pack, the wrench): `Fire`, every frame it heals, takes
                // `roundOfFire` out of its one magazine, down to 0, and no more
                if t.mag_type == 1 {
                    if let Some(m) = self.mags.first_mut()
                        && *m >= 1
                    {
                        *m = (*m - t.round_of_fire.round_ties_even() as i32).max(0);
                    }
                }
                // a throw on the fire key goes at full strength
                if t.heat_velocity {
                    self.heat = 1.0;
                }
                self.charging = true;
                if !(self.fire > 0.0) && !t.auto_fire {
                    return self.fire_now(human);
                }
                Ask::Nothing
            }
            7 => {
                self.alt_at = self.time;
                if self.alt_latched {
                    return Ask::Nothing;
                }
                // the alt-fire key charges a throw (`handleUpdate` adds to it, letting go throws)
                if t.heat_velocity {
                    self.charging = true;
                    return Ask::Nothing;
                }
                // (the time to zoom again after a shot, stays −1 on the server)
                if t.zoom_fov >= 0.0 {
                    if self.zoom <= 0.0 {
                        let free = self.reload <= 0.0 && self.overheat <= 0.0;
                        if free && (t.unzoom_between_fire == 0.0 || (t.unzoom_between_fire > 0.0 && self.fire <= 0.0)) {
                            self.set_zoom(true);
                        }
                    } else {
                        self.set_zoom(false);
                    }
                }
                Ask::Nothing
            }
            8 => {
                if self.eject <= 0.0 && self.fire <= 0.0 {
                    // `EjectClip`
                    self.eject = t.eject_clip_time;
                    self.loaded = false;
                }
                Ask::Nothing
            }
            9 => {
                if self.reload <= 0.0 && self.eject <= 0.0 && self.fire <= 0.0 {
                    let more = self.ammo() != t.mag_size && (self.mags.len() > 1 || t.num_of_mag == -1) && self.total_ammo() != 0;
                    if more || !self.chamber {
                        self.start_reload();
                    }
                }
                Ask::Nothing
            }
            _ => Ask::Nothing,
        }
    }

    /// `setZoom`: on (from off: on the server, with no render view, 10000) or off.
    pub fn set_zoom(&mut self, on: bool) {
        if self.template.zoom_fov < 0.0 {
            return;
        }
        if on {
            if self.zoom == -1.0 {
                self.zoom = 10000.0;
            }
        } else {
            self.zoom = -1.0;
        }
    }

    /// `isZoomed`
    pub fn zoomed(&self) -> bool {
        self.zoom >= 0.0
    }

    /// `isAltFireAnimationReadyToUse`: a shot waiting for `fireDelay`, or one just fired.
    pub fn alt_animation_ready(&self) -> bool {
        self.queued || (self.fire > 0.0 && self.fire <= self.rate())
    }

    /// The weapon's fire: a shot now (or once `fireDelay` has passed), with rounds.
    fn fire_now(&mut self, human: bool) -> Ask {
        let t = self.template.clone();
        self.human = human;
        self.charging = false;
        if self.heat >= 1.0 && !t.heat_velocity {
            if self.overheat <= 0.0 {
                self.overheat = t.overheat_delay;
            }
            return Ask::Nothing;
        }
        if self.delay > 0.0 {
            self.queued = true;
            return Ask::Nothing;
        }
        self.delay = t.fire_delay;
        if t.projectile_missing {
            return Ask::Nothing;
        }
        if self.ammo() > 0 || self.ammo() == -1 {
            self.fire = self.rate();
            self.dev_fire = (t.fire_dev[1] + self.dev_fire).min(t.fire_dev[0]);
            self.charge = self.heat;
            self.heat = if t.heat_velocity { 0.0 } else { self.heat + t.heat_add };
            self.hide = t.hide_time;
            Ask::Fire
        } else if t.auto_reload {
            self.hide = t.hide_time;
            Ask::Reload
        } else {
            Ask::Nothing
        }
    }

    /// The weapon's setting of AI deviation: the extra, one `updateDeviation(false)` (a hand
    /// weapon's with its soldier's strafe, throttle, mouse and pose; else the plain one) and the
    /// extra again, for the next update.
    pub fn set_ai_deviation(&mut self, d: f32, hand: Option<(f32, f32, f32, f32, usize)>) {
        self.dev_extra = d;
        match hand {
            Some((strafe, throttle, mx, my, pose)) => self.update_deviation(strafe, throttle, mx, my, pose, false),
            None => self.update_plain_deviation(),
        }
        self.dev_extra = d;
    }

    /// The weapon's deviation update (not a hand weapon's): the firing share comes down by
    /// `setFireDev`'s decrease; the total is it plus `setMinDev` and the extra (used once).
    pub fn update_plain_deviation(&mut self) {
        let t = &self.template;
        self.dev_fire = (self.dev_fire - t.fire_dev[2]).max(0.0);
        self.dev = self.dev_fire + t.min_dev + std::mem::take(&mut self.dev_extra);
    }

    /// The projectile of the next shot. The count is drawn when the weapon is
    /// created and again after a tracer (`tracer_count`). With barrels, the last ones of the count are tracers too: a whole
    /// volley. No barrels, `CRD_NONE/2`: N, N, T, N, N, T...
    pub fn next_projectile(&mut self) -> String {
        let Some((tracer, dist)) = self.template.tracer.clone() else {
            return self.template.projectile.clone();
        };
        let barrels = self.template.barrels.len();
        if self.tracer_in > barrels as i32 {
            self.tracer_in -= 1;
            return self.template.projectile.clone();
        }
        if self.tracer_in > 1 {
            self.tracer_in -= 1;
        } else {
            self.tracer_in = tracer_count(&dist, barrels, &mut self.seed);
        }
        tracer
    }

    /// The shots of fire from `rows`/`pos` (the camera's), at the game's
    /// `input_index`: no barrels, one (`fireBarrel(M, −1)`); one, that one; several, the active
    /// one and then the next with `setAsynchronyFire`, else all at once (a shotgun's pellets), or
    /// only as many as the magazine holds without `blastAmmoCount`. The chamber empties.
    pub fn shots(&mut self, rows: [V3; 3], pos: V3, root_vel: Option<V3>, gravity: f32, input_index: i32) -> Vec<Shot> {
        // (`Fire`: no `fireBarrel` while the AI fakes its fire)
        if self.fake {
            self.chamber = false;
            return vec![];
        }
        let n = self.template.barrels.len();
        let barrels: Vec<i32> = match n {
            0 => vec![-1],
            1 => vec![0],
            _ if self.template.asynchrony_fire => {
                if self.barrel >= n {
                    self.barrel = 0;
                }
                let b = self.barrel;
                self.barrel += 1;
                vec![b as i32]
            }
            _ => {
                let rounds = self.ammo();
                let k = if rounds != -1 && (rounds as usize) < n && !self.template.blast_ammo_count { rounds.max(0) as usize } else { n };
                (0..k as i32).collect()
            }
        };
        let shots = barrels.into_iter().map(|b| self.barrel_shot(rows, pos, root_vel, gravity, input_index, b)).collect();
        self.chamber = false;
        shots
    }

    /// `fireBarrel(M, barrel)`: the barrel's turn on the axes, its position on
    /// `projectilePosition`, and the deviation (two numbers from `random_seeds` at
    /// `input_index + barrel`).
    pub fn barrel_shot(&mut self, rows: [V3; 3], pos: V3, root_vel: Option<V3>, gravity: f32, input_index: i32, barrel: i32) -> Shot {
        let mut p = self.template.position;
        let rows = match usize::try_from(barrel) {
            Ok(i) => {
                let b = self.template.barrels[i];
                p = [p[0] + b[0], p[1] + b[1], p[2] + b[2]];
                let r = crate::ref2::set_rotation(self.template.barrel_rots.get(i).copied().unwrap_or([0.0; 3]));
                std::array::from_fn(|k| std::array::from_fn(|c| r[k][0] * rows[0][c] + r[k][1] * rows[1][c] + r[k][2] * rows[2][c]))
            }
            Err(_) => rows,
        };
        let charge = self.template.heat_velocity.then_some(self.charge);
        let mut shot = self.template.fire_from(rows, pos, p, root_vel, gravity, charge);
        if self.dev > MIN_DEVIATION {
            let idx = (input_index.wrapping_add(barrel) & 0x3ff) as usize;
            let r1 = raw(random_seed(idx).wrapping_add(idx as i32));
            let idx2 = if idx + 0x200 > 0x400 { idx - 0x200 } else { idx + 0x200 };
            let r2 = raw(random_seed(idx2).wrapping_add(idx2 as i32));
            self.seed = r2;
            // the angles and the sums stay in the FPU's precision (f64 here); where the server
            // rounds in between, checked shot by shot against fire_mp18
            let lo = -(self.dev as f64);
            let span = self.dev as f64 - lo;
            let u = |r: i32| ((r >> 7 | 1) + 1) as f64 * 5.960_464_477_539_063e-8;
            let a = u(r1) * span + lo;
            let b = u(r2) * span + lo;
            let k = (self.template.velocity / 100.0) as f64;
            let (x, y) = (rows[0], rows[1]);
            let v = shot.vel;
            let ya = |i: usize| y[i] as f64 * k * a;
            let xb = |i: usize| x[i] as f64 * k * b;
            shot.vel = [
                (v[0] as f64 + ya(0) + xb(0)) as f32,
                (v[1] as f64 + ya(1) + xb(1)) as f32,
                (((v[2] as f64 + ya(2)) as f32) as f64 + xb(2)) as f32,
            ];
        }
        shot
    }

    /// The weapon's update handling: the `fireOnce` latch, the reload, the time to the next
    /// shot (then `fireFinished`), a shot waiting for `fireDelay`, autoReload, the end of
    /// `hideDuringFireTime`, the cooling or (`velocityDependentOnHeat`) the throw's charge.
    pub fn handle_update(&mut self, dt: f32) -> Vec<Ask> {
        let t = self.template.clone();
        let mut asks = Vec::new();
        // `magType` 1 fills again by itself: each update its `reloadTime`, as an int, up to `magSize`
        if t.mag_type == 1
            && let Some(m) = self.mags.first_mut()
        {
            *m = (*m + t.reload_time as i32).min(t.mag_size);
        }
        if t.fire_once {
            let held = self.time == self.fired_at;
            // let go: `unZoomBetweenFireTime` takes the zoom off
            if !held && self.latched && self.zoom > 0.0 && t.unzoom_between_fire > 0.0 {
                self.set_zoom(false);
            }
            self.latched = held;
        }
        if t.alt_fire_once {
            self.alt_latched = self.time == self.alt_at;
        }
        if self.reload > 0.0 {
            self.reload -= dt;
            if self.reload <= 0.0 {
                self.reload_finished();
            }
        }
        if self.eject > 0.0 {
            self.eject -= dt;
            if self.eject <= 0.0 {
                self.eject = 0.0;
            }
        }
        if self.fire > 0.0 {
            self.fire -= dt;
            if self.fire <= 0.0 {
                self.fire_finished();
            }
        }
        if self.overheat > 0.0 {
            self.overheat -= dt;
        }
        self.time += dt;
        // a human's empty clip goes out (message 8 through the soldier, with no player)
        if self.human && t.eject_clip_time > 0.0 && self.loaded && self.eject <= 0.0 && self.fire <= 0.0 && self.ammo() < 1 {
            self.handle_message(8, false);
        }
        if t.auto_reload && self.eject <= 0.0 && self.reload <= 0.0 && self.fire <= 0.0 && self.ammo() == 0 && !self.mags.is_empty() {
            asks.push(Ask::Reload);
        }
        if self.queued && self.delay > 0.0 {
            self.delay -= dt;
            if self.delay <= 0.0 {
                let a = self.fire_now(self.human);
                self.queued = false;
                if a == Ask::Fire {
                    asks.push(Ask::Fire);
                }
            }
        }
        if self.hide > 0.0 {
            self.hide -= dt;
            if self.hide <= 0.0 {
                self.hide = 0.0;
                // the last of a delayed weapon's rounds (a grenade): gone, and another one up
                if t.fire_delay > 0.0 && self.ammo() == 1 {
                    self.fire_finished();
                    asks.push(Ask::SelectBest);
                }
            }
        }
        if !t.heat_velocity {
            if self.fire <= 0.0 && self.overheat <= 0.0 {
                self.heat -= t.cool_down;
                if self.heat < 0.0 {
                    self.heat = 0.0;
                }
            }
        } else if !self.queued && self.fire <= 0.0 && self.reload <= 0.0 {
            if self.charging {
                self.heat += t.heat_add;
                if self.heat > 1.0 {
                    self.heat = 1.0;
                }
                self.charging = false;
            } else if self.heat > MIN_DEVIATION {
                // let go: the throw (after `fireDelay`), and the soldier's fire animation
                let a = self.fire_now(self.human);
                asks.push(a);
                asks.push(Ask::FireAnimation);
            }
        }
        // the 0x16 that `enable` posted, at the end of the objects' update whose clock reaches
        // 0.1 s (the 3 frames from the raise on, at 30 Hz): it hears again, and a `fireOnce` or
        // `altFireOnce` key counts as held from now
        if self.deaf_since.is_some_and(|t0| self.time - t0 >= RAISE_DEAF - 1e-5) {
            self.deaf_since = None;
            if t.fire_once {
                self.latched = true;
                self.fired_at = self.time;
            }
            if t.alt_fire_once {
                self.alt_latched = true;
                self.alt_at = self.time;
            }
        }
        asks
    }

    /// `fireFinished`: the next round into the chamber, out of the magazine.
    fn fire_finished(&mut self) {
        self.chamber = true;
        if self.template.mag_type != 1
            && let Some(m) = self.mags.first_mut()
            && *m > 0
        {
            *m = (*m - 1).max(0);
            self.rounds_left = (self.rounds_left - 1).max(0);
            if self.template.destroy_vehicle_when_empty && self.rounds_left == 0 {
                self.destroy_vehicle = true;
            }
        }
        self.fire = 0.0;
        if self.template.heat_velocity {
            self.heat = 0.0;
            self.charging = false;
        }
    }

    /// The hand weapon's enable: raised, the keys held count as held (`fireOnce`, `altFireOnce`),
    /// a throw's charge and a waiting shot gone, `fireDelay` to wait before the first shot.
    pub fn enable(&mut self) {
        let t = self.template.clone();
        // `magType` 1 caught up with the time it was away: seconds × `reloadTime` × the simulation's 30 fps
        if t.mag_type == 1
            && let Some(m) = self.mags.first_mut()
        {
            *m = (*m + (self.away * t.reload_time * 30.0).round_ties_even() as i32).min(t.mag_size);
        }
        self.away = 0.0;
        if t.alt_fire_once {
            self.alt_latched = true;
            self.alt_at = self.time;
        }
        if t.fire_once {
            self.latched = true;
            self.fired_at = self.time;
        }
        if t.heat_velocity {
            self.heat = 0.0;
        }
        self.charging = false;
        self.queued = false;
        self.hide = 0.0;
        self.delay = t.fire_delay;
        self.deaf_since = Some(self.time);
    }

    /// `Reload`: with magazines (not `magType` 1), unless only an empty one may be reloaded.
    fn start_reload(&mut self) {
        let t = &self.template;
        if t.mag_size != 0 && t.mag_type != 1 && (!t.reload_empty_only || self.ammo() < 1) {
            self.reload = t.reload_time;
            if self.zoom > 0.0 {
                self.set_zoom(false);
            }
        }
    }

    /// `ReloadFinished`: the chamber filled, and the next magazine in use: the one there is
    /// thrown away (endless: just full again).
    fn reload_finished(&mut self) {
        if !self.chamber {
            self.fire_finished();
        }
        let t = self.template.clone();
        if t.mag_type == 1 {
            return;
        }
        if t.num_of_mag == -1 {
            if let Some(m) = self.mags.first_mut() {
                *m = t.mag_size;
            }
        } else if self.mags.len() > 1 {
            // the one in use goes, rounds and all: keeping one with rounds at the back needs
            // the template's flag off, which is 1 from its constructor and no command sets
            self.mags.remove(0);
        }
        if self.ammo() > 0 {
            self.loaded = true;
        }
    }

    /// The deviation's mod for this pose (`getDevMod`: stand, crouch, lie).
    pub fn dev_mod(&self, pose: usize) -> f32 {
        self.template.hand.map_or(1.0, |h| h.dev_mod[pose.min(2)])
    }

    /// The hand weapon's deviation update, every `handlePlayerInput` of its soldier, with
    /// its input's axes (mapped ones; 0 strafe, 3 throttle, 4 and 5 the mouse): the hand
    /// weapon's accumulators, the firing one down, the total.
    pub fn update_deviation(&mut self, strafe: f32, throttle: f32, mouse_x: f32, mouse_y: f32, pose: usize, jumping: bool) {
        let t = self.template.clone();
        let Some(h) = t.hand else {
            self.dev_fire = (self.dev_fire - t.fire_dev[2]).max(0.0);
            self.dev = self.dev_fire + t.min_dev + std::mem::take(&mut self.dev_extra);
            return;
        };
        let m = h.dev_mod[pose.min(2)];
        let inv = 1.0 / m;
        let mut speed = 0.0;
        if throttle.abs() > AXIS_DEAD {
            speed = h.speed[1] * m + 0.0;
        }
        if strafe.abs() > AXIS_DEAD {
            speed += h.speed[2] * m;
        }
        let misc = if jumping { h.misc[1] * m + 0.0 } else { 0.0 };
        let mut turn = 0.0;
        if mouse_y.abs() > AXIS_DEAD {
            turn = mouse_y.abs() * h.turn[1] * m + 0.0;
        }
        if mouse_x.abs() > AXIS_DEAD {
            turn += mouse_x.abs() * h.turn[2] * m;
        }
        let s = accumulate(&mut self.dev_hand[1], speed, h.speed[0], h.speed[3], m, inv);
        let tt = accumulate(&mut self.dev_hand[0], turn, h.turn[0], h.turn[3], m, inv);
        let mm = accumulate(&mut self.dev_hand[2], misc, h.misc[0], h.misc[2], m, inv);
        // 1 / mod stays in the FPU's precision, and so does the sum
        let inv64 = 1.0 / m as f64;
        self.dev_fire = (self.dev_fire as f64 - inv64 * t.fire_dev[2] as f64) as f32;
        if self.dev_fire < 0.0 {
            self.dev_fire = 0.0;
        }
        let extra = std::mem::take(&mut self.dev_extra);
        self.dev = (self.dev_fire as f64 + t.min_dev as f64 + s as f64 + tt as f64 + mm as f64 + extra as f64) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tracer sequence of `createProjectile`, with the count starting as the weapon's constructor draws it
    #[test]
    fn tracers_come_after_the_count_and_by_volleys() {
        let seq = |n: f32, barrels: usize, shots: usize| {
            let mut t = (*weapon(1, 1).template).clone();
            t.tracer = Some(("T".to_string(), Distribution4 { kind: 0, a: n, b: 0.0, symmetric: false }));
            t.projectile = "N".to_string();
            t.barrels = vec![[0.0; 3]; barrels];
            let mut w = FireArms::new(Arc::new(t));
            (0..shots).map(|_| w.next_projectile()).collect::<String>()
        };
        assert_eq!(seq(2.0, 0, 7), "NNTNNTN");
        // two barrels, count 2 x 2 = 4: two shots, then the last two of the count are a volley of tracers
        assert_eq!(seq(2.0, 2, 8), "NNTTNNTT");
    }

    #[test]
    fn the_seeds_are_msvc_rand() {
        assert_eq!([random_seed(0), random_seed(1), random_seed(2), random_seed(3)], [41, 18467, 6334, 26500]);
        assert_eq!(random_seed(1024), 0);
    }

    #[test]
    fn a_deviation_pair_like_the_server() {
        // fire_mp18: input index 757935405, barrel −1, deviation 0.4: (0.16047, 0.30747)
        let idx = (757935405i32 - 1) as usize & 0x3ff;
        let r1 = raw(random_seed(idx) + idx as i32);
        let idx2 = if idx + 0x200 > 0x400 { idx - 0x200 } else { idx + 0x200 };
        let r2 = raw(random_seed(idx2) + idx2 as i32);
        assert_eq!((r1, r2), (1504501119, 1899091322));
        assert_eq!(unit(r1) * 0.8 - 0.4, 0.160_470_4_f32);
    }

    #[test]
    fn accumulators_rise_to_their_max_and_fall() {
        let mut a = 0.0;
        for _ in 0..20 {
            accumulate(&mut a, 0.4, 0.8, 0.1, 1.0, 1.0);
        }
        assert_eq!(a, 0.8);
        accumulate(&mut a, 0.0, 0.8, 0.1, 1.0, 1.0);
        assert!((a - 0.7).abs() < 1e-6);
    }

    /// `setAIDeviation`'s update in between: the firing share comes down once more and the
    /// extra stays for the next update.
    #[test]
    fn ai_deviation_updates_once_in_between() {
        let mut t = (*weapon(10, 1).template).clone();
        t.fire_dev = [0.5, 0.2, 0.1];
        t.min_dev = 0.05;
        let mut w = FireArms::new(Arc::new(t));
        w.dev_fire = 0.3;
        w.set_ai_deviation(1.5, None);
        assert!((w.dev_fire - 0.2).abs() < 1e-6 && (w.dev - 1.75).abs() < 1e-6 && w.dev_extra == 1.5, "{} {} {}", w.dev_fire, w.dev, w.dev_extra);
        w.update_plain_deviation();
        assert!((w.dev_fire - 0.1).abs() < 1e-6 && (w.dev - 1.65).abs() < 1e-6 && w.dev_extra == 0.0);
    }

    fn pack() -> FireArms {
        let mut w = weapon(1800, 1);
        let mut t = (*w.template).clone();
        (t.mag_type, t.mag_size, t.round_of_fire, t.reload_time) = (1, 1800, 10.0, 1.5);
        w.template = std::sync::Arc::new(t);
        w.mags = vec![1800];
        w
    }

    /// The medic's pack as the original has it: 10 out with each
    /// message, 1 back with each update (9 a frame while it heals, 200 frames from full to
    /// empty), to `magSize`; put away it adds nothing until raised: seconds × 1.5 × 30.
    #[test]
    fn a_healing_magazine_drains_and_fills_by_itself() {
        let mut w = pack();
        w.handle_update(1.0 / 30.0);
        w.handle_message(6, false);
        assert_eq!(w.mags[0], 1790);
        for _ in 0..3 {
            w.handle_update(1.0 / 30.0);
            w.handle_message(6, false);
        }
        assert_eq!(w.mags[0], 1763);
        w.handle_update(1.0 / 30.0);
        assert_eq!(w.mags[0], 1764);
        w.mags[0] = 610;
        w.away = 200.0 / 30.0;
        w.enable();
        assert_eq!(w.mags[0], 910);
        assert_eq!(w.away, 0.0);
        w.away = 1000.0;
        w.enable();
        assert_eq!(w.mags[0], 1800);
    }

    fn weapon(mag_size: i32, num_of_mag: i32) -> FireArms {
        FireArms::new(Arc::new(crate::projectile::Weapon {
                name: "Gun".into(),
                projectile: "Shell".into(),
                projectile_missing: false,
                velocity: 100.0,
                position: [0.0; 3],
                add_root_speed: true,
                rotational_speed: [0.0; 3],
                mag_size,
                num_of_mag,
                mag_type: 0,
                reload_time: 1.0,
                round_of_fire: 10.0,
                fire_delay: 0.0,
                fire_once: false,
                auto_reload: false,
                auto_fire: false,
                reload_empty_only: false,
                destroy_vehicle_when_empty: false,
                disable_when_fired: false,
                change_when_empty: false,
                cant_select_empty: false,
                time_to_not_allow_change: -1.0,
                fire_dev: [0.0; 3],
                min_dev: 0.0,
                heat_add: 0.0,
                cool_down: 0.0,
                overheat_delay: 0.0,
                heat_velocity: false,
                hide_time: 0.0,
                hand: None,
                recoil: None,
                item_index: 0,
                input_fire: 8,
                firing_force: 0.0,
                tracer: None,
                fire_in_camera: false,
                barrels: Vec::new(),
                barrel_rots: Vec::new(),
                asynchrony_fire: false,
                blast_ammo_count: false,
                zoom_fov: -1.0,
                alt_fire_once: false,
                unzoom_between_fire: 0.0,
                eject_clip_time: 0.0,
                ammo_type: 0,
            }))
    }

    /// A weapon whose `projectileTemplate` does not exist: the weapon's fire
    /// returns at the null template, so the fire key spends no round and asks for no shot.
    #[test]
    fn a_weapon_without_its_projectile_template_does_not_fire() {
        let mut t = (*weapon(8, 5).template).clone();
        t.projectile_missing = true;
        let mut w = FireArms::new(Arc::new(t));
        for _ in 0..10 {
            assert_eq!(w.handle_message(6, true), Ask::Nothing);
            w.handle_update(1.0 / 30.0);
        }
        assert_eq!(w.mags[0], 8);
    }

    /// The hand weapon's enable posts itself 0x16 for 0.1 s on and hears nothing until then: at 30 Hz
    /// the fire key of the three frames from the raise on does nothing, the fourth's fires. A
    /// `fireOnce` key held through it counts as held when it hears again (it must be let go).
    #[test]
    fn a_weapon_just_raised_hears_nothing_for_a_tenth() {
        let dt = 1.0 / 30.0;
        let mut w = weapon(8, 5);
        w.enable();
        for _ in 0..3 {
            assert_eq!(w.handle_message(6, true), Ask::Nothing);
            w.handle_update(dt);
        }
        assert_eq!(w.mags[0], 8);
        assert_eq!(w.handle_message(6, true), Ask::Fire);
        let mut w = weapon(8, 5);
        std::sync::Arc::make_mut(&mut w.template).fire_once = true;
        w.enable();
        for _ in 0..3 {
            w.handle_message(6, true);
            w.handle_update(dt);
        }
        assert_eq!(w.handle_message(6, true), Ask::Nothing, "still held");
        w.handle_update(dt);
        w.handle_update(dt);
        assert_eq!(w.handle_message(6, true), Ask::Fire, "let go, pressed again");
    }

    #[test]
    fn the_ais_rounds_fill_the_one_magazine_or_the_spare_ones() {
        // one magazine (a vehicle's MG42): into the one in use, up to magSize
        let mut w = weapon(500, 1);
        w.mags[0] = 497;
        assert_eq!(w.reload_ammo(3), 0);
        assert_eq!(w.mags, [500]);
        assert_eq!(w.reload_ammo(3), 3);
        // more: the spare ones in turn, never the one in use
        let mut w = weapon(30, 3);
        w.mags = vec![0, 25, 30];
        assert_eq!(w.reload_ammo(10), 5);
        assert_eq!(w.mags, [0, 30, 30]);
        w.mags = vec![0, 20, 20];
        assert_eq!(w.reload_ammo(15), 0);
        assert_eq!(w.mags, [0, 30, 25]);
        // endless: nothing
        let mut w = weapon(-1, 999);
        assert_eq!(w.reload_ammo(4), 4);
    }

    #[test]
    fn a_shotgun_fires_all_its_barrels_at_once() {
        // DC's Remington: 7 barrels, each turned a little, blastAmmoCount
        let rots = [[0.0, -0.5, 0.0], [-0.5, 0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.02, 0.0], [-0.35, -0.45, 0.0]];
        let mut w = weapon(8, 5);
        let t = std::sync::Arc::make_mut(&mut w.template);
        t.barrels = vec![[0.0; 3]; 7];
        t.barrel_rots = rots.to_vec();
        t.blast_ammo_count = true;
        let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let shots = w.shots(rows, [0.0; 3], None, -14.73, 0);
        assert_eq!(shots.len(), 7);
        // each along its own turned z, half a degree apart
        let z = |s: &crate::projectile::Shot| s.vel.map(|c| c / w.template.velocity);
        let (a, b) = (z(&shots[1]), z(&shots[2]));
        let angle = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0).acos().to_degrees();
        assert!((angle - 1.0).abs() < 0.01, "{angle}");
        // one round left: still all of them (blastAmmoCount); without it, one
        w.mags[0] = 1;
        assert_eq!(w.shots(rows, [0.0; 3], None, -14.73, 0).len(), 7);
        std::sync::Arc::make_mut(&mut w.template).blast_ammo_count = false;
        assert_eq!(w.shots(rows, [0.0; 3], None, -14.73, 0).len(), 1);
        // setAsynchronyFire: one at a time
        std::sync::Arc::make_mut(&mut w.template).asynchrony_fire = true;
        w.mags[0] = 8;
        assert_eq!(w.shots(rows, [0.0; 3], None, -14.73, 0).len(), 1);
    }
}
