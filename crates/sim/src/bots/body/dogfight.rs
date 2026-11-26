//! Fighting what flies, from a pilot's or a gunner's seat: the aircraft it has fixed on, how that one moves as seen from one look to
//! the next, and where a shot must go to meet it. Only what a player sees and knows: where the other is each time it looks, and how
//! fast its own rounds leave (B26).
//!
//! A pilot flies as it always did while [`super::pilot::Pilot::dogfight`] is off, and while no enemy aircraft is in sight.
// TODO: B26: two pilots of the same skill that turn after each other: in `examples/air_duel`, bots against bots from behind, 16 duels
// of 120 s, one downs the other in 57 % (F-16 against MiG-29) to 94 % (TIE fighter against X-wing), and they no longer collide (they
// pass each other on the right). What is left undecided is two that meet nose to nose again and again until one leaves the map or the
// time runs out. Tried and no better: climbing instead of flying level out of a fight that goes nowhere (57/94/94/87 % decided became
// 63/75/94/56 %). Not tried: slowing down to turn inside the other, and going up over the top after a pass.
use super::rng::Rng;
use crate::bot_api::{AltFire, BotView, Inside, Other};
use crate::soldier::axis;

type V3 = [f32; 3];

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let l = len(a).max(1e-6);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// How fast the nose is asked to come onto the target, 1/s: the turn it asks for is this times how far off it is.
const NOSE_GAIN: f32 = 3.0;
/// How fast the wings are asked to roll to where they should be, 1/s, and the most they are asked for, degrees a second.
const ROLL_GAIN: f32 = 4.0;
const ROLL_MOST: f32 = 150.0;
/// What a pilot expects of a stick before knowing the aircraft, degrees a second with it hard over: the pitch (per m/s of its speed)
/// and the roll. The stick's memory makes up for what the aircraft really gives.
const PITCH_REACH: f32 = 0.27;
/// How fast the stick hard over gets the wings rolling, degrees a second each second, before having watched this aircraft do it (the
/// slowest to roll does 140, the quickest over 600: the pilot learns it in the first rolls).
const ROLL_PUSH: f32 = 300.0;
/// Closer to the nose than this the wings do not chase the target (the stick and the rudder do), and from this on they chase it fully,
/// degrees: chasing it close to the nose, an aircraft rolls from side to side and its guns never settle.
const WINGS_FROM: f32 = 4.0;
const WINGS_FULLY: f32 = 16.0;
/// The world's pull, m/s².
const FALL: f32 = crate::GRAVITY as f32;

/// The hands on the stick: they bring the nose onto a direction and keep it there, watching how the aircraft answers (how fast it
/// pitches, yaws and rolls from one moment to the next) and remembering how much stick it takes to hold the nose where it is. Nothing
/// here knows which aircraft it is.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stick {
    before: Option<([V3; 3], f64)>,
    /// how the nose and the wings turn, degrees a second: nose up, nose right, right wing down
    pub rates: V3,
    /// the stick it has learnt to hold
    trim: f32,
    /// what it has seen the stick hard over do to the roll, degrees a second each second (0: not seen yet)
    roll_push: f32,
    /// the bank stick it held the tick before
    bank_before: f32,
    /// the lean it has learnt to add to keep the nose from lagging to one side, degrees of bank
    lean: f32,
}

impl Stick {
    /// Watches the aircraft turn: `rows` are its axes (right, up, forward) at `t`. Every tick, before [`Stick::point`].
    pub fn watch(&mut self, rows: [V3; 3], t: f64) {
        match self.before {
            Some(([r0, u0, _], t0)) if t > t0 && t - t0 < 0.5 => {
                let dt = (t - t0) as f32;
                let rates = [dot(rows[2], u0), dot(rows[2], r0), -dot(rows[0], u0)].map(|x| x.clamp(-1.0, 1.0).asin().to_degrees() / dt);
                // (how hard the stick gets the wings going shows when it is well over and the wings are not yet rolling fast that way)
                let b = self.bank_before;
                if b.abs() > 0.5 && rates[2] * b.signum() < 40.0 {
                    let seen = ((rates[2] - self.rates[2]) / dt / b).clamp(60.0, 900.0);
                    self.roll_push = if self.roll_push == 0.0 { seen.min(ROLL_PUSH) } else { self.roll_push + 0.1 * (seen - self.roll_push) };
                }
                self.rates = rates;
            }
            Some((_, t0)) if t <= t0 => return,
            // (the first look, or the first after a while: what it held then is no use now)
            _ => {
                self.rates = [0.0; 3];
                self.trim = 0.0;
                self.lean = 0.0;
                self.bank_before = 0.0;
            }
        }
        self.before = Some((rows, t));
    }

    /// The stick that brings the nose onto `dir` (a direction in the world) and keeps it there while that direction turns at `turn`
    /// (rad/s, in the world: the line to a target that crosses turns by itself). The wings roll to where the push the turn needs points
    /// (towards the target, plus what the line itself turns, plus holding the aircraft up) and the stick pulls the nose round; close to
    /// the nose the wings stay and the stick and the rudder do the rest. `hard` (0 to 1): how much of the stick it may use.
    /// Returns the pitch (nose up), the bank (right wing down) and the rudder (nose right), each -1 to 1.
    pub fn point(&mut self, rows: [V3; 3], vel: V3, dir: V3, turn: V3, dt: f32) -> (f32, f32, f32) {
        let [r, u, f] = rows;
        let d = unit(dir);
        let (x, y, z) = (dot(d, r), dot(d, u), dot(d, f));
        let off = x.hypot(y).atan2(z).to_degrees();
        let s = len(vel);
        // what the line itself turns, as the nose would have to: up and to the right, degrees a second
        let (line_up, line_right) = (-dot(turn, r).to_degrees(), dot(turn, u).to_degrees());
        // ---- the wings: where the push points
        let across = unit([d[0] - z * f[0], d[1] - z * f[1], d[2] - z * f[2]]);
        let chase = ((off - WINGS_FROM) / (WINGS_FULLY - WINGS_FROM)).clamp(0.0, 1.0);
        let turning = cross(turn, vel);
        let mut push: V3 = std::array::from_fn(|k| chase * NOSE_GAIN * s * off.to_radians().min(1.5) * across[k] + turning[k]);
        let l = len(push);
        if l > 90.0 {
            push = push.map(|c| c * 90.0 / l);
        }
        let lift = [push[0], push[1] + FALL, push[2]];
        let (el, az) = (y.atan2(z).to_degrees(), x.atan2(z).to_degrees());
        // (close to the nose, a nose that stays to one side of the target asks for a little more or less bank: learnt slowly, the rudder
        // holds it meanwhile)
        if off < WINGS_FULLY {
            self.lean = (self.lean + 1.5 * az * dt).clamp(-20.0, 20.0);
        } else {
            self.lean *= 1.0 - dt;
        }
        let mut roll = dot(lift, r).atan2(dot(lift, u)).to_degrees() + self.lean;
        if roll.abs() > 180.0 {
            roll -= 360.0 * roll.signum();
        }
        // the wings are asked to roll no faster than they can be stopped in what is left to go
        let push = if self.roll_push > 0.0 { self.roll_push } else { ROLL_PUSH };
        let roll_rate = (ROLL_GAIN * roll).clamp(-ROLL_MOST, ROLL_MOST).clamp(-(1.2 * push * roll.abs()).sqrt(), (1.2 * push * roll.abs()).sqrt());
        let bank = ((roll_rate - self.rates[2]) / (0.25 * push)).clamp(-1.0, 1.0);
        self.bank_before = bank;
        // ---- the stick: the nose up to the target, as fast as it is far, and no pulling while the wings are still on their way round
        let in_plane = if off < WINGS_FULLY { 1.0 } else { roll.to_radians().cos().max(0.0) };
        let want_up = ((NOSE_GAIN * el).clamp(-12.0, 60.0) * in_plane + line_up).clamp(-15.0, 60.0);
        let reach = PITCH_REACH * s.max(40.0);
        let lacking = (want_up - self.rates[0]) / reach;
        let pitch = self.trim + 2.0 * lacking;
        // (what it lacked for a while, the stick remembers; not while it is hard against its stop)
        if pitch.abs() < 1.0 || pitch.signum() != lacking.signum() {
            self.trim = (self.trim + 4.0 * lacking * dt).clamp(-0.5, 0.9);
        }
        // ---- the rudder: only for the last few degrees
        let rudder = if off < WINGS_FULLY { 0.12 * az + 0.03 * (line_right - self.rates[1]) } else { 0.0 };
        (pitch.clamp(-1.0, 1.0), bank, rudder.clamp(-0.7, 0.7))
    }
}

/// How much of a new sight of its speed is believed at once, per look: the rest is what it was doing.
const BELIEVED: f32 = 0.5;
/// Two looks closer than this say nothing of its speed, s.
const LOOK_S: f64 = 0.1;

/// The aircraft fixed on for a whole engagement or attack run: it is not changed because another comes nearer or because it is
/// lost from sight for a moment. Whoever holds it lets go when it dies, gets too far to catch, or someone gets on their own tail.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lock {
    /// who flies it
    pub player: usize,
    /// since when it is fixed on, s
    pub since: f64,
    /// where it was last seen, and when
    pub last_seen: ([f32; 3], f64),
    /// how it moves, from where it was seen each time, smoothed, m/s
    pub velocity: [f32; 3],
}

impl Lock {
    pub fn new(player: usize, at: [f32; 3], t: f64) -> Lock {
        Lock { player, since: t, last_seen: (at, t), velocity: [0.0; 3] }
    }

    /// Seen again, at `at`.
    pub fn seen(&mut self, at: [f32; 3], t: f64) {
        let (before, t0) = self.last_seen;
        if t - t0 < LOOK_S {
            return;
        }
        let dt = (t - t0) as f32;
        let first = t0 == self.since;
        for k in 0..3 {
            let now = (at[k] - before[k]) / dt;
            self.velocity[k] = if first { now } else { self.velocity[k] + BELIEVED * (now - self.velocity[k]) };
        }
        self.last_seen = (at, t);
    }

    /// Where it will be at `t` if it goes on as it was going when last seen.
    pub fn at(&self, t: f64) -> [f32; 3] {
        let dt = (t - self.last_seen.1) as f32;
        std::array::from_fn(|k| self.last_seen.0[k] + self.velocity[k] * dt)
    }
}

/// Where to point a gun at `from`, moving at `own`, for its round (leaving at `speed` m/s over the gun's own motion, falling at
/// `fall` m/s²: 0 for one that flies straight) to meet a target now at `target` and moving at `velocity`: the point in the world, and
/// the seconds the round takes. `None`: the round never catches it.
pub fn lead(from: [f32; 3], own: [f32; 3], target: [f32; 3], velocity: [f32; 3], speed: f32, fall: f32) -> Option<([f32; 3], f32)> {
    let r: [f32; 3] = std::array::from_fn(|k| target[k] - from[k]);
    let v: [f32; 3] = std::array::from_fn(|k| velocity[k] - own[k]);
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // |r + v t| = speed t
    let (a, b, c) = (dot(v, v) - speed * speed, 2.0 * dot(r, v), dot(r, r));
    let t = if a.abs() < 1e-3 {
        (b < 0.0).then(|| -c / b)?
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let (t1, t2) = ((-b - disc.sqrt()) / (2.0 * a), (-b + disc.sqrt()) / (2.0 * a));
        [t1, t2].into_iter().filter(|t| *t > 0.0).min_by(f32::total_cmp)?
    };
    // (the round falls on its way: the gun points that much higher)
    Some(([from[0] + r[0] + v[0] * t, from[1] + r[1] + v[1] * t + 0.5 * fall * t * t, from[2] + r[2] + v[2] * t], t))
}

/// A direction no steeper than the aircraft can climb without running out of speed: a slow aircraft that points its nose at a target
/// well above it stalls before getting there; it climbs towards it as its speed lets it.
pub fn climbable(dir: [f32; 3], speed: f32) -> [f32; 3] {
    let most = ((speed - CLIMB_FROM) * 1.0).clamp(0.0, 60.0).to_radians();
    let flat = dir[0].hypot(dir[2]);
    if dir[1] <= 0.0 || dir[1].atan2(flat) <= most {
        return dir;
    }
    [dir[0], flat.max(1e-3) * most.tan(), dir[2]]
}
/// Slower than this an aircraft does not point above the horizon to follow a target, m/s; a degree more for each m/s over it.
const CLIMB_FROM: f32 = 65.0;

const PITCH: usize = 1;
const BANK: usize = 2;
/// How far a pilot picks an aircraft to go after, and how far it lets it get before giving it up, m.
const PICK_M: f32 = 900.0;
const GIVE_UP_M: f32 = 1500.0;
/// An aircraft out of sight for this long is forgotten, s: meanwhile it is where it was going.
const FORGOTTEN_S: f64 = 4.0;
/// What flies lower than this over what is under it is not gone after, m (it is landing, taking off, or about to hit the ground alone).
const FLYING_M: f32 = 25.0;
/// The guns fire from this far, m, and the rockets from this far (they slow down soon).
const GUN_M: f32 = 480.0;
const ROCKET_M: f32 = 450.0;
/// A rocket is fired only at what closes faster than this, m/s (or at what hovers); and the part of its launch speed it is reckoned to
/// keep on its way.
const ROCKET_CLOSING: f32 = 120.0;
const ROCKET_KEEPS: f32 = 0.5;
/// Half the size of what a burst must pass through to be worth firing, m: an aircraft seen from behind is a small thing.
const BURST_M: f32 = 4.0;
/// The distance it tries to keep behind its target, m.
const BEHIND_M: f32 = 200.0;
/// The height it keeps over the ground and the water whatever it is chasing, m; and the pull-out it reckons with, rad/s.
const FLOOR_M: f32 = 55.0;
const PULL_OUT: f32 = 0.3;
/// An enemy this close behind, with its nose this near to pointing at it, is on its tail: m and degrees.
const TAIL_M: f32 = 700.0;
const TAIL_NOSE: f32 = 35.0;
/// Two aircraft that would pass closer than this within this time are about to collide, m and s.
const MISS_M: f32 = 28.0;
const MISS_S: f32 = 1.8;
/// Two aircraft that fly at each other (each within 60 degrees of the line between them) and close faster than this, m/s, and would
/// meet within this time, s, pass each other on the right: each
/// points this far to its own right of the other, m (two that turn into each other meet nose to nose again and again, and where each
/// will be by then cannot be told from how it flies now).
const MERGE_CLOSING: f32 = 90.0;
const MERGE_S: f32 = 1.6;
const PASS_M: f32 = 70.0;
/// Turning after a target closer than this without getting the nose on it is a fight going nowhere, m; and how long it flies straight
/// out of one, s.
const STALL_M: f32 = 600.0;
const EXTEND_S: f64 = 5.0;

/// An aircraft a pilot has in sight, own or enemy.
#[derive(Clone, Copy, Debug)]
struct Seen {
    vehicle: usize,
    lock: Lock,
    enemy: bool,
    hovers: bool,
    /// since when it is on this pilot's tail
    tail_since: Option<f64>,
}

/// A pilot's fight in the air, from one tick to the next: the aircraft in sight and how each moves, the hands on the stick, the
/// wandering of its aim and when it may fire.
#[derive(Clone, Debug, Default)]
pub struct Fight {
    stick: Stick,
    seen: Vec<Seen>,
    /// its aim wanders from one error to the next, degrees (right, up): from, to, and when the next was drawn
    wander: ([f32; 2], [f32; 2], f64),
    burst_until: f64,
    rocket_at: f64,
    /// the obstacle check ahead, and what it said
    looked: f64,
    obstacle: bool,
    /// pulling out of a dive until then
    pull_out_until: f64,
    /// since when it has been turning after its target without getting its nose any nearer, and how long it puts up with it, s
    stalled: Option<(f64, f32)>,
    /// flying straight away from a fight that was going nowhere, until then
    extend_until: f64,
    trace_t: f64,
}

/// What a fighting pilot needs to know of itself beyond what it sees.
pub struct Skill<'a> {
    /// how much its aim wanders, degrees
    pub error_degrees: f32,
    /// how long after fixing on a target it starts firing, s
    pub reaction_s: f32,
    /// the head has sent it after that player's aircraft
    pub ordered: Option<usize>,
    /// the enemy pilots its mates are already after (the radio): with another to go for, it leaves those to them
    pub taken: &'a [usize],
    /// where its own are (the nearest flag of its side): an aircraft with nothing to fight back with runs there when it is chased
    pub refuge: Option<[f32; 3]>,
}

/// An aircraft whose pilot fights what flies: guns for the pilot that fire ahead, and nothing to drop (the rest only shake off whoever
/// chases them).
pub fn fighter(i: &Inside) -> bool {
    i.alt_fire != AltFire::Drops && i.guns.iter().any(|g| g.key == axis::FIRE && !g.drops && g.speed > 0.0 && (g.endless || g.rounds.0 + g.rounds.1 > 0))
}
/// An enemy aircraft a mate is already after counts as this much farther when picking one, m.
const TAKEN_M: f32 = 600.0;
/// Farther than this from where its own are, a chased aircraft with nothing to fight with runs for them, weaving, m; nearer, it turns
/// hard over them. And how long each leg of the weave lasts, s.
const REFUGE_M: f32 = 400.0;
const WEAVE_S: f64 = 2.5;

impl Fight {
    /// The aircraft in sight, each followed from where it was seen the last time.
    fn look(&mut self, v: &BotView, i: &Inside, others: &[Other]) {
        let t = v.time;
        for o in others.iter().filter(|o| o.in_vehicle) {
            // (an enemy's vehicle is known only while it is seen)
            let Some(j) = o.vehicle.filter(|j| *j != i.vehicle) else { continue };
            if let Some(k) = self.seen.iter().position(|a| a.vehicle == j) {
                if self.seen[k].lock.last_seen.1 < t {
                    self.seen[k].lock.seen(o.pos, t);
                }
                continue;
            }
            let Some(x) = v.vehicle(j).filter(|x| x.class.flies()) else { continue };
            self.seen.push(Seen { vehicle: j, lock: Lock::new(o.player, o.pos, t), enemy: o.team != v.me.team, hovers: x.class.hovers(), tail_since: None });
        }
        self.seen.retain(|a| t - a.lock.last_seen.1 < FORGOTTEN_S && others.iter().any(|o| o.player == a.lock.player && o.in_vehicle));
    }

    /// The enemy aircraft it has in sight at `t`: who flies each and where it is (what a pilot calls on the radio).
    pub fn enemies_in_sight(&self, t: f64) -> impl Iterator<Item = (usize, [f32; 3])> + '_ {
        self.seen.iter().filter(move |a| a.enemy && t - a.lock.last_seen.1 < 0.5).map(|a| (a.lock.player, a.lock.last_seen.0))
    }

    /// At the controls of a plane in the air with [`super::pilot::Pilot::dogfight`] on. The axes for this tick, or `None` when there is
    /// nothing to fight (the pilot then flies as it always did). `ground`: the height of the ground or the water under a point.
    #[allow(clippy::too_many_arguments)]
    pub fn fly(&mut self, v: &BotView, i: &Inside, others: &[Other], rng: &mut Rng, lock: &mut Option<Lock>, skill: &Skill, ground: &dyn Fn(f32, f32) -> f32, player: usize) -> Option<Vec<(usize, f32)>> {
        let t = v.time;
        let (pos, vel, [r, u, f]) = (i.pos, i.velocity, i.rows);
        let s = len(vel);
        self.stick.watch(i.rows, t);
        self.look(v, i, others);
        let flies = |p: V3| p[1] - ground(p[0], p[2]) > FLYING_M;
        // ---- who is on its tail: behind it, near, and with the nose on it
        for a in self.seen.iter_mut() {
            let (at, to_me) = (a.lock.at(t), sub(pos, a.lock.at(t)));
            let on_tail = a.enemy && !a.hovers && len(to_me) < TAIL_M && dot(unit(sub(at, pos)), f) < -0.2 && len(a.lock.velocity) > 30.0 && dot(unit(a.lock.velocity), unit(to_me)) > TAIL_NOSE.to_radians().cos();
            a.tail_since = if on_tail { a.tail_since.or(Some(t)) } else { None };
        }
        let threat = self.seen.iter().filter(|a| a.tail_since.is_some_and(|t0| t - t0 > 0.5)).min_by(|a, b| len(sub(a.lock.at(t), pos)).total_cmp(&len(sub(b.lock.at(t), pos)))).copied();
        // ---- whom it goes after: a fighter (guns for the pilot and nothing to drop) picks its own; the rest only shake off who chases them
        let gun = i.guns.iter().find(|g| g.key == axis::FIRE && !g.drops && g.speed > 0.0 && (g.endless || g.rounds.0 + g.rounds.1 > 0));
        let fighter = fighter(i);
        let range_of = |a: &Seen| len(sub(a.lock.at(t), pos));
        if let Some(l) = *lock {
            let still = self.seen.iter().find(|a| a.lock.player == l.player || others.iter().any(|o| o.player == l.player && o.vehicle == Some(a.vehicle)));
            // (about to fire at the one it has: it does not let go for the one behind)
            let on_it = still.is_some_and(|a| range_of(a) < GUN_M && dot(unit(sub(a.lock.at(t), pos)), f) > 0.97);
            let keep = still.is_some_and(|a| a.enemy && range_of(a) < GIVE_UP_M && flies(a.lock.at(t))) && fighter && (threat.is_none_or(|x| Some(x.vehicle) == still.map(|a| a.vehicle)) || on_it);
            if !keep {
                *lock = None;
            }
        }
        if lock.is_none() && fighter {
            let ordered = skill.ordered.and_then(|p| self.seen.iter().find(|a| a.enemy && a.lock.player == p && range_of(a) < GIVE_UP_M && flies(a.lock.at(t))));
            // (the nearest; a helicopter only when no plane is as near: it cannot run away; and one a mate is already after, only when
            // no other is as near)
            let far = |a: &Seen| range_of(a) + if a.hovers { 400.0 } else { 0.0 } + if skill.taken.contains(&a.lock.player) { TAKEN_M } else { 0.0 };
            let nearest = self.seen.iter().filter(|a| a.enemy && range_of(a) < PICK_M && flies(a.lock.at(t))).min_by(|a, b| far(a).total_cmp(&far(b)));
            if let Some(a) = threat.as_ref().or(ordered).or(nearest) {
                *lock = Some(Lock { since: t, ..a.lock });
            }
        }
        let target = lock.and_then(|l| self.seen.iter().find(|a| a.lock.player == l.player).copied());
        if let (Some(l), Some(a)) = (lock.as_mut(), target) {
            (l.last_seen, l.velocity) = (a.lock.last_seen, a.lock.velocity);
        }
        if target.is_none() && threat.is_none() {
            return None;
        }
        // ---- where the nose goes
        let mut axes = Vec::with_capacity(8);
        let mut throttle = 1.0;
        let (mut dir, mut turn) = (f, [0.0f32; 3]);
        let mut fire = (false, false);
        let mut what = "break";
        if let Some(a) = target {
            what = "chase";
            let (at, tv) = (a.lock.at(t), a.lock.velocity);
            let to = sub(at, pos);
            let range = len(to);
            let rel = sub(tv, vel);
            let aim = gun.and_then(|g| lead(pos, vel, at, tv, g.speed, 0.0)).map_or(to, |(p, _)| sub(p, pos));
            // (its aim wanders: a new error every little while, and the hands drift from one to the next)
            if t >= self.wander.2 {
                self.wander = (self.wander.1, [rng.normal() * skill.error_degrees, rng.normal() * skill.error_degrees], t + 0.8);
            }
            let k = (1.0 - (self.wander.2 - t) as f32 / 0.8).clamp(0.0, 1.0);
            let err: [f32; 2] = std::array::from_fn(|n| (self.wander.0[n] + k * (self.wander.1[n] - self.wander.0[n])).to_radians());
            let la = len(aim);
            dir = std::array::from_fn(|n| aim[n] + la * (err[0] * r[n] + err[1] * u[n]));
            turn = cross(to, rel).map(|c| c / (range * range).max(1.0));
            // the throttle: the distance behind it and how fast it closes; turning hard, the speed at which it turns tightest
            let closing = -dot(rel, unit(to));
            let off = dot(unit(dir), f).clamp(-1.0, 1.0).acos().to_degrees();
            throttle = if off > 40.0 { (0.5 + (95.0 - s) / 20.0).clamp(0.3, 1.0) } else { (0.5 + (range - BEHIND_M) / 200.0 - closing / 40.0).clamp(0.0, 1.0) };
            // two that turn after each other get nowhere: after a while one of them gives it up, flies straight out for room and speed, and
            // comes back at it (or is caught from behind for leaving)
            if range < STALL_M && off > 50.0 {
                let (since, patience) = *self.stalled.get_or_insert_with(|| (t, rng.uniform(6.0, 16.0)));
                if t - since > patience as f64 {
                    self.extend_until = t + EXTEND_S;
                    self.stalled = None;
                }
            } else if off < 30.0 || range > STALL_M {
                self.stalled = None;
            }
            if t < self.extend_until {
                what = "extend";
                let flat_v = unit([vel[0], 0.0, vel[2]]);
                (dir, turn, throttle) = ([flat_v[0], -0.05, flat_v[2]], [0.0; 3], 1.0);
            }
            // the trigger: the burst would pass through it, and it has had the time to react
            let reacted = t - lock.map_or(t, |l| l.since) >= skill.reaction_s as f64;
            let through = |d: V3, size: f32| dot(unit(d), f).clamp(-1.0, 1.0).acos() < (size / range.max(1.0)).atan().max(0.004);
            // (nobody of its own between the guns and the target)
            let mate_in_line = self.seen.iter().any(|m| !m.enemy && range_of(m) < range + 80.0 && dot(unit(sub(m.lock.at(t), pos)), f) > 0.998);
            fire.0 = what == "chase" && gun.is_some() && reacted && range < GUN_M && through(dir, BURST_M) && !mate_in_line;
            // what hangs under the wings, if it flies by itself: a rocket leaves fast and soon slows down to less than the aircraft's own
            // speed (fired from behind at a target that runs, it never gets there, and whoever fired it flies into it seconds later): only at
            // what comes to meet it or hangs in the air, from close, and led with the speed it keeps on the way
            let rocket = i.guns.iter().find(|g| g.key == axis::ALT_FIRE && !g.drops && g.speed > 0.0 && (g.endless || g.rounds.0 + g.rounds.1 > 0));
            let big_or_slow = v.vehicle(a.vehicle).is_some_and(|x| missile_worth(x.radius, len(a.lock.velocity), false));
            let meets = what == "chase" && (closing > ROCKET_CLOSING || a.hovers || big_or_slow);
            fire.1 = meets && rocket.and_then(|g| lead(pos, vel, at, tv, ROCKET_KEEPS * g.speed, 0.0)).is_some_and(|(p, _)| reacted && range < ROCKET_M && through(sub(p, pos), BURST_M + 1.5) && !mate_in_line && t >= self.rocket_at);
        } else if let Some(a) = threat {
            let home = skill.refuge.map(|q| [q[0] - pos[0], 0.0, q[2] - pos[2]]).filter(|d| len(*d) > REFUGE_M);
            if let Some(d) = home {
                // nothing to fight it with, and its own are far: it runs for them (their guns and their fighters are there), weaving
                // so that whoever follows does not have a still target, and losing a little height for speed
                what = "run";
                let to = unit(d);
                let side = if (t / WEAVE_S) as i64 % 2 == 0 { 1.0 } else { -1.0 };
                dir = [to[0] + 0.45 * side * to[2], -0.06, to[2] - 0.45 * side * to[0]];
            } else {
                // nothing to fight it with: a hard turn towards the side it comes from, losing a little height for speed
                let side = if dot(sub(a.lock.at(t), pos), r) >= 0.0 { 1.0 } else { -1.0 };
                let flat_f = unit([f[0], 0.0, f[2]]);
                let flat_r = [flat_f[2], 0.0, -flat_f[0]];
                dir = std::array::from_fn(|n| 0.2 * flat_f[n] + side * flat_r[n]);
                dir[1] = -0.08;
            }
        }
        // ---- what rules over the chase
        // not running out of speed
        let up = climbable(dir, s);
        if up != dir {
            (dir, turn, throttle) = (up, [0.0; 3], 1.0);
        }
        // another aircraft about to be hit: away from where it will pass
        for a in &self.seen {
            let (p, w) = (sub(a.lock.at(t), pos), sub(a.lock.velocity, vel));
            // (coming at each other: each keeps to its right, from well before they meet)
            let (range, closing) = (len(p), -dot(w, unit(p)));
            let head_on = dot(unit(p), unit(vel)) > 0.5 && len(a.lock.velocity) > 30.0 && dot(unit(a.lock.velocity), unit(p)) < -0.5;
            if head_on && closing > MERGE_CLOSING && range < closing * MERGE_S {
                let flat = p[0].hypot(p[2]);
                let right = if flat > 0.3 * range { [p[2] / flat, 0.0, -p[0] / flat] } else { r };
                dir = std::array::from_fn(|n| p[n] + PASS_M * right[n]);
                turn = [0.0; 3];
                fire = (false, false);
                what = "pass";
                continue;
            }
            let when = (-dot(p, w) / dot(w, w).max(1e-3)).clamp(0.0, MISS_S);
            let miss: V3 = std::array::from_fn(|n| p[n] + w[n] * when);
            if len(miss) < MISS_M && len(p) < 400.0 && dot(p, w) < 0.0 {
                // (straight at each other, each breaks to its own right: the two go opposite ways)
                let away = if len(miss) > 6.0 { unit(miss.map(|c| -c)) } else { r };
                dir = std::array::from_fn(|n| 0.5 * f[n] + away[n]);
                turn = [0.0; 3];
                fire = (false, false);
                what = "avoid";
            }
        }
        // the ground and the water: with the height a pull-out takes from this dive, or with something ahead, up
        let floor = [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0].iter().map(|k| ground(pos[0] + vel[0] * k, pos[2] + vel[2] * k)).fold(f32::MIN, f32::max) + FLOOR_M;
        let sink = (-vel[1] / s.max(1.0)).clamp(0.0, 1.0);
        let needs = s / PULL_OUT * (1.0 - (1.0 - sink * sink).sqrt()) * 1.5 + 2.0 * (-vel[1]).max(0.0);
        if t >= self.looked {
            self.looked = t + 0.2;
            self.obstacle = v.hidden(pos, [pos[0] + vel[0] * 3.0, pos[1] + vel[1] * 3.0, pos[2] + vel[2] * 3.0]);
        }
        if pos[1] - needs < floor - 20.0 || self.obstacle {
            self.pull_out_until = t + 1.0;
        }
        if t < self.pull_out_until {
            // (too low for anything else: wings level and up)
            let flat_f = unit([f[0], 0.0, f[2]]);
            dir = [flat_f[0], if self.obstacle { 0.6 } else { 0.3 }, flat_f[2]];
            dir = climbable(dir, s + 25.0);
            (turn, throttle, fire, what) = ([0.0; 3], 1.0, (false, false), "pull out");
        } else if pos[1] - needs < floor + 15.0 || pos[1] + dir[1] / len(dir).max(1e-3) * s * 2.0 < floor {
            // (near the floor it goes on turning after its target, but not down: level or climbing a little, as low as it is)
            let flat = dir[0].hypot(dir[2]).max(1e-3);
            let rise = ((floor + 15.0 - (pos[1] - needs)) / 100.0).clamp(0.0, 0.25);
            if dir[1] < flat * rise {
                dir[1] = flat * rise;
                turn[0] = 0.0;
                turn[2] = 0.0;
                throttle = throttle.max(0.7);
                if what == "chase" {
                    what = "chase low";
                }
            }
        }
        if s < 55.0 {
            throttle = 1.0;
        }
        let (p, b, y) = self.stick.point(i.rows, vel, dir, turn, v.dt.max(1e-3));
        axes.push((axis::THROTTLE, throttle));
        axes.push((PITCH, -p));
        axes.push((BANK, b));
        axes.push((axis::YAW, y));
        if fire.0 {
            self.burst_until = t + 0.5;
        }
        if t < self.burst_until && what.starts_with("chase") {
            axes.push((axis::FIRE, 1.0));
        }
        if fire.1 {
            axes.push((axis::ALT_FIRE, 1.0));
            self.rocket_at = t + 1.5;
        }
        if crate::debug::options().bots_air && t >= self.trace_t + 1.0 {
            self.trace_t = t;
            let range = target.map_or(-1.0, |a| range_of(&a));
            eprintln!("air t={t:.0} {player} fight {what} h {:.0} v {s:.0} range {range:.0} off {:.1} throttle {throttle:.1} stick {p:.2} {b:.2} {y:.2} rates {:.0} {:.0} {:.0} seen {} tail {} fire {}{} lock {:?} sent after {:?}", pos[1] - ground(pos[0], pos[2]), dot(unit(dir), f).clamp(-1.0, 1.0).acos().to_degrees(), self.stick.rates[0], self.stick.rates[1], self.stick.rates[2], self.seen.len(), threat.is_some() as u8, fire.0 as u8, fire.1 as u8, lock.map(|l| l.player), skill.ordered);
        }
        Some(axes)
    }
}

/// A round slower than this leaves a gun that is no weapon against an enemy (the droid that mends an X-wing in flight fires at 10 m/s), m/s.
const WEAPON_SPEED: f32 = 30.0;
/// A target at least this big (radius) or no faster than this, a missile is worth: a gunship, the Falcon, a walker; a transport that
/// crawls; m and m/s.
const MISSILE_BIG_M: f32 = 12.0;
const MISSILE_SLOW: f32 = 35.0;
/// An enemy vehicle this big on the ground is a target for a missile too, m.
const MISSILE_GROUND_M: f32 = 6.0;

/// Whether a gun's round is fast enough to be a weapon against an enemy.
pub fn is_weapon(round_speed: f32) -> bool {
    round_speed >= WEAPON_SPEED
}

/// Whether an aircraft of this size (radius, m) flying at this speed (m/s) is worth a missile or torpedo: what a player sees as big
/// (a gunship, the Falcon) or slow (a transport, something hovering).
pub fn missile_worth(radius: f32, speed: f32, hovers: bool) -> bool {
    hovers || radius >= MISSILE_BIG_M || speed < MISSILE_SLOW
}

/// Where a gunner in a turret of an aircraft points and what it fires: what flies before what walks (led, with its own speed taken off),
/// nothing at an enemy from a gun that only mends, and the missiles (key 2) only at what is big or slow.
#[derive(Clone, Debug, Default)]
pub struct Gunner {
    lock: Option<Lock>,
}

/// What a gunner does this tick: where it points, and which keys it may squeeze.
#[derive(Clone, Copy, Debug)]
pub struct SeatPlan {
    pub at: [f32; 3],
    pub fire: bool,
    pub alt: bool,
}

impl Gunner {
    /// `target`: what the seat's eye found (the nearest enemy); an enemy aircraft in sight and within reach is taken before it.
    /// TODO: it fires even when the led point is outside what its gun can reach (the SBD's rear gun goes down 1 degree: ~480 rounds,
    /// no hit, with the target a little below; `AIR_DUEL_DEBUG_GUNNER=1 examples/air_duel data/game bf1942 Midway --a SBD --b Zero
    /// --gunner-a 1 --mind-b drone:straight --aspect rear --range 150 --seeds 4 --seconds 40 --drone-speed 80`, with `--above 20` it
    /// hits); `Inside` would need the limits of the seat's bundles. The lead also leaves out the round's fall (`lead(…, 0.0)`).
    pub fn plan(&mut self, v: &BotView, i: &Inside, others: &[Other], target: Option<crate::bots::body::shooter::Target>) -> Option<SeatPlan> {
        let t = v.time;
        let usable = |key: usize| i.guns.iter().find(|g| g.key == key && !g.drops && is_weapon(g.speed) && (g.endless || g.rounds.0 + g.rounds.1 > 0));
        let (gun, missile) = (usable(axis::FIRE), usable(axis::ALT_FIRE));
        if gun.is_none() && missile.is_none() {
            return None;
        }
        let eye = i.camera_pos;
        // an enemy aircraft in sight
        let air = others
            .iter()
            .filter(|o| o.team != v.me.team && o.in_vehicle)
            .filter_map(|o| Some((o, v.vehicle(o.vehicle?).filter(|x| x.class.flies())?)))
            .filter(|(o, _)| len(sub(o.pos, eye)) < GUN_M && !v.hidden(eye, o.pos))
            .min_by(|a, b| len(sub(a.0.pos, eye)).total_cmp(&len(sub(b.0.pos, eye))).then(a.0.player.cmp(&b.0.player)));
        if let Some((o, x)) = air {
            match self.lock.as_mut() {
                Some(l) if l.player == o.player => l.seen(o.pos, t),
                _ => self.lock = Some(Lock::new(o.player, o.pos, t)),
            }
            let l = self.lock.as_ref()?;
            let at = l.at(t);
            let speed = gun.or(missile)?.speed;
            let led = lead(eye, i.velocity, at, l.velocity, speed, 0.0).map(|(p, _)| p);
            return Some(SeatPlan { at: led.unwrap_or(at), fire: gun.is_some() && led.is_some(), alt: missile.is_some() && missile_worth(x.radius, len(l.velocity), x.class.hovers()) });
        }
        self.lock = None;
        // on the ground: what the eye found, the missiles only for a vehicle of some size
        let (player, at, _) = target?;
        let vehicle = others.iter().find(|o| o.player == player).and_then(|o| o.vehicle).and_then(|j| v.vehicle(j));
        Some(SeatPlan { at, fire: gun.is_some(), alt: missile.is_some() && vehicle.is_some_and(|x| x.radius >= MISSILE_GROUND_M) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B26: a target fixed on is followed from one look to the next, and its speed is what those looks say.
    #[test]
    fn a_lock_learns_how_its_target_moves() {
        let mut l = Lock::new(7, [0.0, 100.0, 0.0], 10.0);
        for k in 1..=20 {
            let t = 10.0 + k as f64 * 0.2;
            l.seen([100.0 * (t - 10.0) as f32, 100.0, 0.0], t);
        }
        assert!((l.velocity[0] - 100.0).abs() < 0.5 && l.velocity[1].abs() < 0.01, "{:?}", l.velocity);
        let p = l.at(15.0);
        assert!((p[0] - 500.0).abs() < 1.0, "{p:?}");
        // two looks in the same instant say nothing
        let before = l.velocity;
        l.seen([0.0; 3], 14.0 + 0.01);
        assert_eq!(l.velocity, before);
        assert_eq!((l.player, l.since), (7, 10.0));
    }

    /// B26: a slow aircraft does not point at a target well above it; a fast one does.
    #[test]
    fn a_slow_aircraft_climbs_as_its_speed_lets_it() {
        let up = [0.0, 1.0, 1.0];
        assert_eq!(climbable(up, 130.0), up);
        let slow = climbable(up, 75.0);
        assert!((slow[1].atan2(slow[2]).to_degrees() - 10.0).abs() < 0.1 && slow[2] == 1.0, "{slow:?}");
        assert_eq!(climbable(up, 50.0)[1], 0.0);
        // downwards, whatever its speed
        assert_eq!(climbable([0.0, -1.0, 1.0], 50.0), [0.0, -1.0, 1.0]);
    }

    /// B26: the hands on the stick. A target above the nose is pulled onto; one far to the right is rolled towards before pulling; one
    /// just right of the nose is left to the rudder, with the wings where they are.
    #[test]
    fn the_stick_rolls_towards_a_target_and_pulls() {
        let level = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let vel = [0.0, 0.0, 100.0];
        let fresh = || {
            let mut s = Stick::default();
            s.watch(level, 0.0);
            s.watch(level, 1.0 / 30.0);
            s
        };
        // 10 degrees above the nose: pulls, wings level
        let (p, b, y) = fresh().point(level, vel, [0.0, 0.176, 1.0], [0.0; 3], 1.0 / 30.0);
        assert!(p > 0.5 && b.abs() < 0.05 && y.abs() < 0.01, "{p} {b} {y}");
        // 60 degrees to the right, level: rolls right first, and no rudder that far out
        let (p, b, y) = fresh().point(level, vel, [1.73, 0.0, 1.0], [0.0; 3], 1.0 / 30.0);
        assert!(b > 0.9 && p < 0.3 && y == 0.0, "{p} {b} {y}");
        // 2 degrees to the right: the rudder, and the wings hardly move
        let (_, b, y) = fresh().point(level, vel, [0.035, 0.0, 1.0], [0.0; 3], 1.0 / 30.0);
        assert!(y > 0.1 && b.abs() < 0.1, "{b} {y}");
        // a line that turns to the right by itself asks for bank even with the nose on it
        let (_, b, _) = fresh().point(level, vel, [0.0, 0.0, 1.0], [0.0, 0.2, 0.0], 1.0 / 30.0);
        assert!(b > 0.5, "{b}");
        // and it sees how the aircraft turns from one look to the next: 3 degrees of roll to the right in a thirtieth of a second
        let mut s = fresh();
        let (sn, c) = 3.0f32.to_radians().sin_cos();
        s.watch([[c, -sn, 0.0], [sn, c, 0.0], [0.0, 0.0, 1.0]], 2.0 / 30.0);
        assert!((s.rates[2] - 90.0).abs() < 1.0 && s.rates[0].abs() < 0.1, "{:?}", s.rates);
    }

    /// B26: the shot goes where the target will be when the round gets there.
    #[test]
    fn the_lead_meets_a_crossing_target() {
        // still, and nothing falls: at it
        let (p, t) = lead([0.0; 3], [0.0; 3], [0.0, 0.0, 300.0], [0.0; 3], 1000.0, 0.0).unwrap();
        assert!((p[2] - 300.0).abs() < 1e-3 && (t - 0.3).abs() < 1e-4);
        // crossing at 100 m/s, 300 m away, a round of 1000 m/s: about 30 m ahead of it
        let (p, t) = lead([0.0; 3], [0.0; 3], [0.0, 0.0, 300.0], [100.0, 0.0, 0.0], 1000.0, 0.0).unwrap();
        assert!((p[0] - 100.0 * t).abs() < 1e-3 && (p[0] - 30.15).abs() < 0.1, "{p:?} {t}");
        assert!(((p[0] * p[0] + p[2] * p[2]).sqrt() - 1000.0 * t).abs() < 0.01);
        // both going the same way at the same speed: as if both were still
        let (p, _) = lead([0.0; 3], [0.0, 0.0, 90.0], [0.0, 0.0, 300.0], [0.0, 0.0, 90.0], 1000.0, 0.0).unwrap();
        assert!((p[2] - 300.0).abs() < 1e-2 && p[0].abs() < 1e-3);
        // a round that falls is aimed higher
        let (p, t) = lead([0.0; 3], [0.0; 3], [0.0, 0.0, 300.0], [0.0; 3], 300.0, 14.73).unwrap();
        assert!((p[1] - 0.5 * 14.73 * t * t).abs() < 1e-3 && p[1] > 7.0);
        // running away faster than the round: never
        assert!(lead([0.0; 3], [0.0; 3], [0.0, 0.0, 300.0], [0.0, 0.0, 400.0], 300.0, 0.0).is_none());
    }

    /// B26: a gun that only mends is no weapon, and a missile is kept for what is big, slow or hanging in the air.
    #[test]
    fn the_gunner_keeps_missiles_for_the_big_and_the_slow() {
        assert!(!is_weapon(10.0), "the droid that mends an X-wing fires at 10 m/s");
        assert!(is_weapon(650.0));
        assert!(!missile_worth(6.0, 95.0, false), "a TIE fighter at full speed");
        assert!(missile_worth(27.0, 95.0, false), "the Falcon");
        assert!(missile_worth(6.0, 20.0, false), "something that crawls");
        assert!(missile_worth(6.0, 95.0, true), "something that hovers");
    }
}
