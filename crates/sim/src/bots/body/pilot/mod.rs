//! Flying a plane: taxiing, taking off, going to the right flag, firing at what it sees and not crashing. Only with what a
//! player sees (where it is, its speed and how it is tilted) and with the usual axes, so it works for any plane: it
//! corrects by watching what it does, not by knowing which plane it is. A propeller fighter or a jet is all the same.
use super::rng::Rng;
use super::shooter::{Target, Shooter};
use super::{Maps, TURN, pitch, angle_between, flat};
use crate::bot_api::{AltFire, BotView, Inside, Other};
use crate::soldier::axis;
use std::sync::Arc;

mod air;
mod rotor;
mod runway;
mod vtol;

const PITCH: usize = 1;
const BANK: usize = 2;
/// The pitch axis, positive: nose down.
const PITCH_SIGN: f32 = -1.0;

/// Speed at which it starts pulling the stick on the runway, m/s.
const V_ROTATE: f32 = 32.0;
/// Below this, already in the air, it does not hold up well: nose down, m/s.
const V_LOST: f32 = 26.0;
/// Height it flies at over whatever is below, m.
const CRUISE_HEIGHT: f32 = 120.0;
/// Height below which it does not descend when attacking, m.
const MIN_HEIGHT: f32 = 35.0;
/// Maximum bank when turning, degrees.
const BANK_MAX: f32 = 50.0;
/// How far it sees from the air, m.
const AIR_RANGE: f32 = 900.0;
/// How far the machine guns fire with any hope, m.
const SHOT_RANGE: f32 = 420.0;
/// Assumed speed of a plane's bullet, over the plane's own, m/s.
const BULLET_SPEED: f32 = 500.0;
/// How many degrees to each side of the nose it looks for something to attack.
const AIR_CONE: f32 = 65.0;
/// How close to the target the burst must pass for it to fire, m.
const BURST_RADIUS: f32 = 40.0;
/// How close to the target the bomb must fall to be dropped, and how close to a mate it is not dropped, m.
const BOMB_RADIUS: f32 = 25.0;
const FRIEND_BOMB_RADIUS: f32 = 40.0;

/// A helicopter's flight height over what is ahead of it, m.
const HELI_HEIGHT: f32 = 40.0;
/// bots: an attacking helicopter stops at this distance from the target, m.
pub const HELI_FIRE_M: f32 = 250.0;
/// bots: and flies at this height over what is ahead of it (at 40 m, in a city, the houses hide everything on foot from it), m.
pub const HELI_ATTACK_HEIGHT: f32 = 70.0;
/// A helicopter about to land at a point starts descending closer than this to it, m (bots).
pub const LAND_M: f32 = 25.0;
/// Nose down to advance, degrees; and at what speed it stops pushing, m/s.
const HELI_NOSE: f32 = 12.0;
const HELI_SPEED: f32 = 35.0;

/// An airship's height over what is ahead of it and over the point it goes to, m.
const AIRSHIP_HEIGHT: f32 = 25.0;
/// The speed it cruises at, m/s (GC_Bespin's `cloudbus` makes 54 at full throttle), and how far from the point it goes to it slows
/// down, m per m/s.
const AIRSHIP_SPEED: f32 = 45.0;
const AIRSHIP_BRAKE: f32 = 0.25;
/// How far ahead it looks for something in its way, s at its speed (at least `AIRSHIP_LOOK_M`, m), and from how far in front of
/// its own centre (its hull), m.
const AIRSHIP_LOOK_S: f32 = 4.0;
const AIRSHIP_LOOK_M: f32 = 30.0;
const AIRSHIP_HULL_M: f32 = 14.0;
/// More than this off the heading it barely moves forward while it turns, degrees.
const AIRSHIP_TURN: f32 = 40.0;
/// It starts coming down this close to the point it lands at, m: it does not slide sideways, so it gets there before (a helicopter's
/// [`LAND_M`] put it beside a platform, on what was lower).
const AIRSHIP_LAND_M: f32 = 8.0;

/// What half a plane takes up on each side when looking at the runway, m.
const WING_SPAN: f32 = 8.0;
/// What must be free ahead to take off, m.
const RUNWAY_LENGTH: f32 = 250.0;

/// A VTOL takeoff ([`Pilot::vtol`]): the height over where it stood that it slides out at, m (over the parked craft beside it).
const VTOL_SLIDE_HEIGHT: f32 = 5.0;
/// Clear overhead this far, m, is open sky: it can go straight up.
const VTOL_SKY_M: f32 = 60.0;
/// How far it looks for the open sky along each way out, m, and in steps of how many.
const VTOL_OUT_M: f32 = 150.0;
const VTOL_OUT_STEP: f32 = 8.0;
/// The speed it slides out at, m/s.
const VTOL_SLIDE: f32 = 4.0;
/// The height over the ground it climbs to under open sky before the throttle goes forward, m: from hanging still a snowspeeder,
/// an X-Wing or a TIE sinks 8-25 m before its wings hold it (`examples/air_envelope … --hover`).
const VTOL_CLIMB_M: f32 = 40.0;
/// The most it tilts the nose or banks to slide, degrees.
const VTOL_TILT: f32 = 10.0;
/// Before the throttle goes forward the way ahead of the nose must be clear this far, m, down to this much lower (what it sinks); if
/// not, it climbs 10 m more, up to `VTOL_HIGHER_M` more.
const VTOL_RUN_M: f32 = 150.0;
const VTOL_SINK_M: f32 = 25.0;
const VTOL_HIGHER_M: f32 = 80.0;
/// Half the room it needs to each side when sliding, m, at least, and for what share of its bounding radius (the Falcon is wide).
const VTOL_HALF_SPAN: f32 = 6.0;
const VTOL_SPAN_OF_RADIUS: f32 = 0.6;
/// How far behind it the sky must be open too before it goes up (its tail), m.
const VTOL_TAIL_M: f32 = 8.0;
/// How long it waits for a craft in its way before it looks for another way out, s.
const VTOL_WAIT_S: f64 = 3.0;
/// Seconds without getting further along its way out (or without flying, once the throttle is forward) before it gives up.
const VTOL_GIVE_UP_S: f64 = 8.0;
/// At most this many straight legs to the open sky.
const VTOL_LEGS: u8 = 3;
/// It looks for the way out at most this many times (hanging still a while between them) before it gives up.
const VTOL_TRIES: u8 = 6;
const VTOL_HOLD_S: f64 = 3.0;

/// Health below which a plane in flight is given up for lost: anti-aircraft damage does not leave much more.
/// A craft that flies hanging ([`Pilot::fly_hover`]): its height over what is below it and ahead, m; the speed it goes at; how far
/// it tilts to go along or to the side, degrees (GC_Bespin's cloud car sinks with its nose more than ~12 degrees down and holds its
/// height at 10: `examples/air_envelope … cloudcar --cruise -1`); where it slows down to fire, m; how far ahead it looks for walls, s.
const HOVER_HEIGHT: f32 = 25.0;
const HOVER_SPEED: f32 = 22.0;
const HOVER_TILT: f32 = 12.0;
const HOVER_FIRE_M: f32 = 150.0;
const HOVER_AHEAD_S: f32 = 3.0;
/// ... the distance it circles what it fights at, m
const HOVER_RING_M: f32 = 100.0;
/// ... and the least speed it keeps while it fights: the cloud car's yaw does not turn it while it hangs still, m/s.
const HOVER_KEEP: f32 = 8.0;
const HEALTH_LOST: f32 = 0.35;
/// Minimum height to jump, m: less, and there is no time for it to open.
const JUMP_HEIGHT: f32 = 40.0;
/// Height from which it opens the parachute, and at what falling speed it is seen to fall, m and m/s.
const OPEN_HEIGHT: f32 = 10.0;
const FALL_SPEED: f32 = -6.0;
/// The key that opens the parachute (`c_PIWeapon9`, message 18).
const PARACHUTE_KEY: usize = 9;
/// With nothing this far below its feet, m, it is in the air.
const FALL_CLEAR_M: f32 = 3.0;
/// Ordered over a point, a plane slows down from this far, m, to this many times the speed below which it is lost.
const OVER_SLOW_M: f32 = 350.0;
const OVER_SLOW: f32 = 1.5;
/// Ordered over a point and more than this far below it, m, it does not come nearer than this many metres per metre left to climb:
/// it climbs on a ring of this radius around it, m.
const OVER_LOW_M: f32 = 10.0;
const OVER_CLIMB: f32 = 15.0;
const OVER_RING_M: f32 = 500.0;

/// B23, the attack run ([`FlightEnd::Pass`]): it pulls away from the target to this distance, m (at 60 m/s its turning circle is some
/// 500 m across: it lines up far away, not over the target), going straight for at least this long, s, and at most this long, s.
const PASS_OUT_M: f32 = 900.0;
const PASS_OUT_LEAST_S: f64 = 5.0;
const PASS_OUT_MOST_S: f64 = 35.0;
/// Pulling away it stays this far inside the play area, m: what it needs to turn back (out of it a pilot dies).
const PASS_EDGE_M: f32 = 600.0;
/// Coming in, the run is over closer than this to the target, m, or once it is this many degrees off the nose within this distance, m.
const PASS_OVER_M: f32 = 120.0;
const PASS_BEHIND: f32 = 100.0;
const PASS_BEHIND_M: f32 = 400.0;
/// Within this many degrees of the nose the target is in front (a run that never had it there has not gone past it).
const PASS_ALIGNED: f32 = 30.0;
/// With bombs it comes in level at this height over the target, m.
const PASS_BOMB_UP: f32 = 70.0;
/// The aim is the enemy nearest the target within this distance of it, m (the target is where the side reported it).
const PASS_FIND_M: f32 = 80.0;
/// Coming in with bombs, if where the bomb would land has passed its nearest to the target within this distance, it drops there, m (a
/// run that ends lined up a little off the target still drops: the next one is a minute away).
const PASS_LOOSE_M: f32 = 50.0;
/// How long after a bomb or a rocket it goes on coming in, s (one drop per run: the next bomb would fall long).
const PASS_AFTER_DROP_S: f64 = 0.6;

/// The phase of an attack run.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RunPhase {
    /// pulling away from the target, straight along `dir`, climbing
    Out { dir: [f32; 2] },
    /// coming in, nose on the target
    In,
}

/// B23: an attack run in course: on which target (where the side reported it), the phase and since when, and when it last dropped.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    target: [f32; 3],
    phase: RunPhase,
    since: f64,
    dropped: f64,
    /// coming in, it has had the target in front of the nose (only then can it go past it)
    aligned: bool,
    /// coming in with bombs, the nearest the bomb would have landed to the target so far, m
    best_miss: f32,
}

/// The phase of a VTOL takeoff ([`Pilot::vtol`]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum VtolPhase {
    /// straight up off the floor to the height it slides out at
    Rise,
    /// sliding along `dir` (x, z) at that height until the sky is open overhead, or, on a leg that does not reach it, until it has
    /// gone `until` m (then it looks for the way out again from there)
    Out { dir: [f32; 2], until: Option<f32>, leg: u8 },
    /// straight up under the open sky, drifting along `dir`
    Climb { dir: [f32; 2] },
    /// the throttle forward until its wings hold it
    Go,
    /// hanging where it is with no way out for now: it looks again at `until`
    Hold { until: f64 },
    /// it found no way out time after time, or got stuck: the runway, as for a plane
    Failed,
}

/// A VTOL takeoff in course: the phase and since when, the height it stood at and the one it slides at over it, and the furthest
/// it has got along its way out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vtol {
    phase: VtolPhase,
    floor: f32,
    slide: f32,
    start: [f32; 2],
    best: f32,
    best_at: f64,
    look_sky: f64,
    /// the highest of what is under it and a little ahead, last looked at ([`BotView::floor`]), plus how high it stood over the floor
    /// it rose from: it slides over a step or a sill as it does over that floor
    under: f32,
    /// how high it stood over the floor it rose from, m
    rest: f32,
    /// since when a craft with someone at its controls has been in its way (it waits for it to go by), last looked at
    blocked_since: Option<f64>,
    /// how many times it has looked for the way out
    tries: u8,
    /// how much higher than `VTOL_CLIMB_M` it climbs because the way ahead of the nose was not clear, m
    higher: f32,
}

/// How an ordered flight ends ([`FlightOrder`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlightEnd {
    /// flies to the point, at its height if that is higher than its cruise over the ground, and passes right over it, slowly, again
    /// and again
    Over,
    /// lands at the point (an aircraft that holds still in the air: [`crate::bot_api::VehicleClass::hovers`])
    Land,
    /// an attack run on a target fixed for the whole run, with that weapon if given
    Pass { target: [f32; 3], weapon: Option<usize> },
    /// goes after that player's aircraft until it is down, lost or out of reach ([`super::dogfight::Lock`]); `to` is where it was
    /// reported
    Engage { player: usize },
}

/// bots: what the head orders a pilot (a trip's air leg, [`crate::bots::trip`]): where, how it ends, and whether it holds where it is
/// (landed, waiting for its passengers). Without an order the pilot flies as it always did (the plain bot never gives one).
/// The transport flight's phases live where their state is: boarding in the squad's plan, the landing spot in the trip's air leg
/// (chosen before taking off), holding and landing here.
/// The attack run of a plane is [`FlightEnd::Pass`] (its phases in [`Run`]).
/// TODO: an attack helicopter still goes to the side's target through `imposed_goal` and `attack_run` (and the plain bot body uses
/// `land_at`); they would become a `Pass` that holds at firing distance, and a `Return` to where it took off with no bombs or
/// rockets left (B24: whether they reload there is not measured).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightOrder {
    pub to: [f32; 3],
    pub end: FlightEnd,
    pub hold: bool,
}

/// What the pilot remembers from one tick to the next.
#[derive(Clone, Debug)]
pub struct Pilot {
    previous: Option<(f64, f32, f32)>,
    /// has left the ground
    in_the_air: bool,
    /// the runway heading, on starting (x, z)
    runway: Option<[f32; 2]>,
    /// since when it has been standing still on the ground
    stopped_since: Option<f64>,
    /// since when it switches to attacking (you cannot dive forever)
    dive_out: f64,
    burst_until: f64,
    wheel_sign: f32,
    wheel: Option<(f64, f32)>,
    look_wheel: f64,
    look_ahead: f64,
    obstacle: bool,
    missile_at: f64,
    meta: Option<usize>,
    change_goal: f64,
    previous_heading: Option<(f64, f32)>,
    /// whom it was looking at, where it was and when: from that you see how it moves
    seen_target: Option<(usize, [f32; 3], f64)>,
    target_speed: [f32; 3],
    /// bots: where it goes in the air instead of to a flag (a target reported by its side); without it, as usual
    pub imposed_goal: Option<[f32; 2]>,
    /// bots: the helicopter attacks like a player: it stays at firing distance, points the nose at the target and also uses the
    /// rockets or missiles against what is in a vehicle; without it, as usual
    pub attack_run: bool,
    /// bots: the head's order for this flight; without it, as usual
    pub flight: Option<FlightOrder>,
    /// bots: it fights what flies ([`super::dogfight`]): it fixes on an enemy aircraft and goes after it, and breaks when one is
    /// on its tail; off, as usual
    pub dogfight: bool,
    /// bots: the aircraft it has fixed on, for an engagement or for an attack run
    pub lock: Option<super::dogfight::Lock>,
    /// bots: the enemy pilots its mates are already after (what the side tells it)
    pub taken: [usize; 6],
    /// its fight in the air, while `dogfight` is on
    fight: super::dogfight::Fight,
    /// the last line of the air trace (`BYTEFIELD_BOTS_AIR`)
    trace_t: f64,
    /// bots: it flies an airship as one ([`Pilot::fly_airship`]); off, as a plane (the plain bot)
    pub airship: bool,
    /// B23: the attack run in course ([`FlightEnd::Pass`])
    run: Option<Run>,
    /// bots: a craft its throttle lifts off the ground ([`crate::bot_api::Inside::lift`]) takes off straight up and slides out of
    /// a hangar or off a platform ([`Pilot::vtol_takeoff`]); off, it takes a runway like a plane (the plain bot)
    pub vtol: bool,
    /// the VTOL takeoff in course
    takeoff: Option<Vtol>,
    /// what a craft that flies hanging looked at last ([`Pilot::fly_hover`])
    hover: Hover,
}

/// What [`Pilot::fly_hover`] looks at four times a second: the height it keeps (over what is below it and ahead) and whether
/// something stands in its way ahead.
#[derive(Clone, Copy, Debug, Default)]
struct Hover {
    /// it rose hanging off the ground ([`Pilot::vtol_takeoff`]) and flies so; a craft whose takeoff failed (DC's Harrier does not
    /// rise at all) takes the runway and flies as a plane
    on: bool,
    look_at: f64,
    alt: f32,
    blocked: bool,
    /// what it stands over (a platform too)
    under: f32,
}

/// `BYTEFIELD_BOTS_AIR=1`: each pilot in the air writes a line a second: its target and what it lacks to fire, or, with no target,
/// how many enemies are within range and how many in front (stderr).
fn trace_air() -> bool {
    crate::debug::options().bots_air
}

/// How the craft is this tick, read from what a player sees ([`Pilot::attitude`]): the same for every way of flying.
#[derive(Clone, Copy, Debug)]
struct Attitude {
    pos: [f32; 3],
    vel: [f32; 3],
    /// its speed, m/s
    s: f32,
    /// its rows: forward and right
    f: [f32; 3],
    r: [f32; 3],
    /// the nose's pitch and the bank, degrees, and how fast each changes, degrees a second
    pit: f32,
    kind: f32,
    dpit: f32,
    drol: f32,
    /// what is below it (the ground or the water) and how high it is over that, m
    below: f32,
    h: f32,
}

impl Default for Pilot {
    fn default() -> Pilot {
        Pilot { previous: None, in_the_air: false, runway: None, stopped_since: None, dive_out: 0.0, burst_until: 0.0, wheel_sign: 1.0, wheel: None, look_wheel: 0.0, look_ahead: 0.0, obstacle: false, missile_at: 0.0, meta: None, change_goal: 0.0, previous_heading: None, seen_target: None, target_speed: [0.0; 3], imposed_goal: None, attack_run: false, flight: None, dogfight: false, lock: None, taken: [usize::MAX; 6], fight: Default::default(), trace_t: f64::MIN, airship: false, run: None, vtol: false, takeoff: None, hover: Hover::default() }
    }
}

fn wraps(a: f32) -> f32 {
    let mut a = a;
    while a > 180.0 {
        a -= 360.0;
    }
    while a < -180.0 {
        a += 360.0;
    }
    a
}

impl Pilot {
    /// Whether the plane it is in, at the controls or as a passenger, is going to crash: bad health and enough height to jump. A plane that
    /// still flies well is not abandoned, even if jumping out were quicker than landing.
    pub fn plane_lost(v: &BotView, i: &Inside, health: f32) -> bool {
        i.class.flies() && health < HEALTH_LOST && i.pos[1] - Pilot::ground(v, i.pos[0], i.pos[2]) > JUMP_HEIGHT
    }

    /// B23: whether the seat it flies from carries something against what goes by land or sea: bombs, or rockets on the secondary key,
    /// with rounds left.
    pub fn ordnance(i: &Inside) -> bool {
        i.alt_fire != AltFire::None && i.guns.iter().any(|g| g.key == axis::ALT_FIRE && (g.endless || g.rounds.0 + g.rounds.1 > 0))
    }

    /// Falling through the air, the parachute opens. What the key does if it does not apply is up to the game: nothing.
    pub fn fall(v: &BotView) -> Option<Vec<(usize, f32)>> {
        let me = &v.me;
        (me.velocity[1] < FALL_SPEED && me.pos[1] - Pilot::ground(v, me.pos[0], me.pos[2]) > OPEN_HEIGHT).then(|| vec![(axis::WEAPON1 + PARACHUTE_KEY - 1, 1.0)])
    }

    /// bots: on foot and coming down through the air: falling or under the parachute, with nothing within [`FALL_CLEAR_M`] below its
    /// feet. (Whoever walks down a ramp up on a building is also coming down with nothing the sight ray stops at below: this alone
    /// does not tell a jump.)
    pub fn in_the_air(v: &BotView) -> bool {
        let me = &v.me;
        me.velocity[1] <= -0.5 && me.pos[1] - Pilot::ground(v, me.pos[0], me.pos[2]) >= FALL_CLEAR_M && !v.hidden([me.pos[0], me.pos[1] + 0.5, me.pos[2]], [me.pos[0], me.pos[1] - FALL_CLEAR_M, me.pos[2]])
    }

    /// bots: coming down through the air ([`Pilot::in_the_air`]) with a place to come down at: it faces that place, since the fall and
    /// the parachute push along where it looks; the parachute opens as in [`Pilot::fall`]. `None` when it is not in the air.
    pub fn fall_towards(v: &BotView, to: [f32; 3]) -> Option<Vec<(usize, f32)>> {
        let me = &v.me;
        if !Pilot::in_the_air(v) {
            return None;
        }
        let mut axes = Pilot::fall(v).unwrap_or_default();
        let d = [to[0] - me.pos[0], to[2] - me.pos[2]];
        let turn = if d[0].hypot(d[1]) > 2.0 { (TURN * angle_between(me.view[2], d) / 3.0).clamp(-10.0, 10.0) } else { 0.0 };
        axes.extend([(axis::MOUSE_X, turn), (axis::MOUSE_Y, 0.0)]);
        Some(axes)
    }

    /// What is below a point: the ground or the water; outside the map, nothing.
    fn ground(v: &BotView, x: f32, z: f32) -> f32 {
        v.ground(x, z).unwrap_or(f32::MIN).max(v.water())
    }

    /// bots: the enemy aircraft it has in sight while it fights, at `t`: who flies each and where it is.
    pub fn enemies_in_sight(&self, t: f64) -> Vec<(usize, [f32; 3])> {
        if self.dogfight { self.fight.enemies_in_sight(t).collect() } else { Vec::new() }
    }

    /// Where it goes in the air: from one enemy flag to another, which is where the enemies are (the base too: the rule of not going to it
    /// is for whoever goes on foot and comes from Berlin, which has no planes); if there are none, to its own. Without flags, straight.
    fn air_goal(&mut self, v: &BotView, rng: &mut Rng, pos: [f32; 3], f: [f32; 3]) -> [f32; 2] {
        if let Some(FlightOrder { end: FlightEnd::Pass { target, .. }, .. }) = self.flight {
            return [target[0], target[2]];
        }
        if let Some(m) = self.imposed_goal {
            return m;
        }
        match self.flight {
            Some(FlightOrder { to, end: FlightEnd::Over | FlightEnd::Engage { .. }, .. }) => return [to[0], to[2]],
            Some(FlightOrder { end: FlightEnd::Pass { target, .. }, .. }) => return [target[0], target[2]],
            _ => {}
        }
        let flags = v.flags();
        let enemy_ones: Vec<usize> = (0..flags.len()).filter(|&k| flags[k].team != v.me.team).collect();
        let list: Vec<usize> = if enemy_ones.is_empty() { (0..flags.len()).collect() } else { enemy_ones };
        let current = self.meta.filter(|k| list.contains(k) && v.time < self.change_goal);
        let k = match current {
            Some(k) => Some(k),
            None => {
                // the nearest of those it does not just see
                let anterior = self.meta;
                let another = list.iter().copied().filter(|k| Some(*k) != anterior || list.len() == 1).min_by(|a, b| flat(pos, [flags[*a].pos[0], flags[*a].pos[2]]).total_cmp(&flat(pos, [flags[*b].pos[0], flags[*b].pos[2]])).then(a.cmp(b)));
                self.meta = another;
                self.change_goal = v.time + rng.uniform(30.0, 60.0) as f64;
                another
            }
        };
        k.map_or([pos[0] + f[0] * 100.0, pos[2] + f[2] * 100.0], |k| [flags[k].pos[0], flags[k].pos[2]])
    }

    /// Whether what is under the crosshair is in the air.
    fn target_flies(v: &BotView, others: &[Other], target: Option<Target>) -> bool {
        target.is_some_and(|b| others.iter().any(|o| o.player == b.0 && o.in_vehicle && o.pos[1] - Pilot::ground(v, o.pos[0], o.pos[2]) > 12.0))
    }

    /// How the craft is this tick; it also keeps the pitch and bank for the next tick's rates of change.
    fn attitude(&mut self, v: &BotView, i: &Inside) -> Attitude {
        let (pos, vel) = (i.pos, i.velocity);
        let s = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
        let (f, u, r) = (i.rows[2], i.rows[1], i.rows[0]);
        let pit = pitch(f);
        let kind = r[1].atan2(u[1]).to_degrees();
        let (dpit, drol) = match self.previous {
            Some((t, p, q)) if v.time > t => ((pit - p) / (v.time - t) as f32, wraps(kind - q) / (v.time - t) as f32),
            _ => (0.0, 0.0),
        };
        self.previous = Some((v.time, pit, kind));
        let below = Pilot::ground(v, pos[0], pos[2]);
        Attitude { pos, vel, s, f, r, pit, kind, dpit, drol, below, h: pos[1] - below }
    }

    /// At the controls of a plane. Returns whether it is advisable to jump.
    #[allow(clippy::too_many_arguments)]
    pub fn fly(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, maps: &Arc<Maps>, player: usize) -> bool {
        let a = self.attitude(v, i);
        let Attitude { vel, s, h, .. } = a;
        if self.vtol && i.lift != 0.0 && !self.in_the_air && self.takeoff.is_none_or(|x| x.phase != VtolPhase::Failed) {
            self.vtol_takeoff(v, i, axes, &a, player);
            return false;
        }
        if self.vtol && i.hangs && self.hover.on {
            self.fly_hover(v, i, axes, others, rng, shooter, &a, player);
            return false;
        }
        // ---- on the ground (or on the deck: height alone does not say, speed does)
        if !self.in_the_air && s > V_LOST && vel[1] > 2.0 && h > 8.0 {
            self.in_the_air = true;
        }
        // has run off an embankment without having taken off: it falls, so it already flies, or tries to (on a deck it is just as high, but does not fall)
        if !self.in_the_air && s > 15.0 && h > 8.0 && vel[1] < -2.0 {
            self.in_the_air = true;
        }
        if self.in_the_air && s < 6.0 && h < 4.0 {
            self.in_the_air = false;
            self.runway = None;
            self.takeoff = None;
        }
        axes.push((axis::THROTTLE, 1.0));
        // where to: the right flag, or where the enemy it sees is
        if !self.in_the_air {
            return self.taxi(v, i, axes, maps, &a);
        }
        self.fly_air(v, i, axes, others, rng, shooter, &a, player);
        false
    }
}
