//! Flying a plane: taxiing, taking off, going to the right flag, firing at what it sees and not crashing. Only with what a
//! player sees (where it is, its speed and how it is tilted) and with the usual axes, so it works for any plane: it
//! corrects by watching what it does, not by knowing which plane it is. A propeller fighter or a jet is all the same.
use super::rng::Rng;
use super::shooter::{Target, Shooter};
use super::{Maps, TURN, pitch, angle_between, flat};
use crate::bot_api::{AltFire, BotView, Inside, Other};
use crate::soldier::axis;
use std::sync::Arc;

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
/// bots3: an attacking helicopter stops at this distance from the target, m.
pub const HELI_FIRE_M: f32 = 250.0;
/// bots3: and flies at this height over what is ahead of it (at 40 m, in a city, the houses hide everything on foot from it), m.
pub const HELI_ATTACK_HEIGHT: f32 = 70.0;
/// A helicopter about to land at a point starts descending closer than this to it, m (bots3).
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

/// bots3: what the head orders a pilot (a trip's air leg, [`crate::bots3::trip`]): where, how it ends, and whether it holds where it is
/// (landed, waiting for its passengers). Without an order the pilot flies as it always did (bots2 never gives one).
/// The transport flight's phases live where their state is: boarding in the squad's plan, the landing spot in the trip's air leg
/// (chosen before taking off), holding and landing here.
/// The attack run of a plane is [`FlightEnd::Pass`] (its phases in [`Run`]).
/// TODO: an attack helicopter still goes to the side's target through `imposed_goal` and `attack_run` (and the bots2 body uses
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
    /// bots3: where it goes in the air instead of to a flag (a target reported by its side); without it, as usual
    pub imposed_goal: Option<[f32; 2]>,
    /// bots3: the helicopter attacks like a player: it stays at firing distance, points the nose at the target and also uses the
    /// rockets or missiles against what is in a vehicle; without it, as usual
    pub attack_run: bool,
    /// bots3: the head's order for this flight; without it, as usual
    pub flight: Option<FlightOrder>,
    /// bots3: it fights what flies ([`super::dogfight`]): it fixes on an enemy aircraft and goes after it, and breaks when one is
    /// on its tail; off, as usual
    pub dogfight: bool,
    /// bots3: the aircraft it has fixed on, for an engagement or for an attack run
    pub lock: Option<super::dogfight::Lock>,
    /// bots3: the enemy pilots its mates are already after (what the side tells it)
    pub taken: [usize; 6],
    /// its fight in the air, while `dogfight` is on
    fight: super::dogfight::Fight,
    /// the last line of the air trace (`BYTEFIELD_BOTS3_AIR`)
    trace_t: f64,
    /// bots3: it flies an airship as one ([`Pilot::fly_airship`]); off, as a plane (bots2)
    pub airship: bool,
    /// B23: the attack run in course ([`FlightEnd::Pass`])
    run: Option<Run>,
    /// bots3: a craft its throttle lifts off the ground ([`crate::bot_api::Inside::lift`]) takes off straight up and slides out of
    /// a hangar or off a platform ([`Pilot::vtol_takeoff`]); off, it takes a runway like a plane (bots2)
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

/// `BYTEFIELD_BOTS3_AIR=1`: each pilot in the air writes a line a second: its target and what it lacks to fire, or, with no target,
/// how many enemies are within range and how many in front (stderr).
fn trace_air() -> bool {
    crate::debug::options().bots3_air
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

    /// bots3: on foot and coming down through the air: falling or under the parachute, with nothing within [`FALL_CLEAR_M`] below its
    /// feet. (Whoever walks down a ramp up on a building is also coming down with nothing the sight ray stops at below: this alone
    /// does not tell a jump.)
    pub fn in_the_air(v: &BotView) -> bool {
        let me = &v.me;
        me.velocity[1] <= -0.5 && me.pos[1] - Pilot::ground(v, me.pos[0], me.pos[2]) >= FALL_CLEAR_M && !v.hidden([me.pos[0], me.pos[1] + 0.5, me.pos[2]], [me.pos[0], me.pos[1] - FALL_CLEAR_M, me.pos[2]])
    }

    /// bots3: coming down through the air ([`Pilot::in_the_air`]) with a place to come down at: it faces that place, since the fall and
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

    /// bots3: the enemy aircraft it has in sight while it fights, at `t`: who flies each and where it is.
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

    /// At the controls of a plane. Returns whether it is advisable to jump.
    #[allow(clippy::too_many_arguments)]
    pub fn fly(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, maps: &Arc<Maps>, player: usize) -> bool {
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
        let h = pos[1] - below;
        if self.vtol && i.lift != 0.0 && !self.in_the_air && self.takeoff.is_none_or(|x| x.phase != VtolPhase::Failed) {
            self.vtol_takeoff(v, i, axes, dpit, drol, player);
            return false;
        }
        if self.vtol && i.hangs && self.hover.on {
            self.fly_hover(v, i, axes, others, rng, shooter, dpit, drol, player);
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
            // the runway: wherever you can run without crashing, as close as possible to where it is looking
            // (a craft that lifts off by itself does not come here with bots3: [`Pilot::vtol_takeoff`]; a run out of a hangar does not
            // work: from GC_Hoth's every way out is shut within 40-95 m at the belly's height and the run needs ~45 m to rotate)
            let eye = [pos[0], pos[1] + 2.0, pos[2]];
            if self.runway.is_none() {
                let (c, sn) = (f[2] / f[0].hypot(f[2]).max(1e-3), f[0] / f[0].hypot(f[2]).max(1e-3));
                let seen_ones = v.vehicles();
                let maps = maps.clone();
                // on a deck the map says nothing: what is below is not where it goes
                let under_cover_now = pos[1] - below > 5.0;
                let grid = if under_cover_now { None } else { maps.tank(v) };
                // the metres that can be run in that direction without crashing
                let free = |g: f32| -> f32 {
                    let (gs, gc) = g.to_radians().sin_cos();
                    let dir = [sn * gc + c * gs, c * gc - sn * gs];
                    let side_ = [dir[1], -dir[0]];
                    let mut l = RUNWAY_LENGTH;
                    if let Some(m) = grid {
                        let mut t = 12.0;
                        while t < RUNWAY_LENGTH {
                            let mut with_nothing = true;
                            for w in [-4.0f32, 0.0, 4.0] {
                                let (x, z) = (pos[0] + dir[0] * t + side_[0] * w, pos[2] + dir[1] * t + side_[1] * w);
                                // (what is under water is blocked on the map, but you fly over it)
                                with_nothing &= m.clear(x, z) || Pilot::ground(v, x, z) < pos[1] - 6.0;
                            }
                            if !with_nothing {
                                l = t;
                                break;
                            }
                            t += 4.0;
                        }
                    }
                    // (what the map does not say, the view does; also to the sides, which the wings take up)
                    // (on a deck it leaves straight ahead, and to the sides is the ship itself)
                    let wides: &[f32] = if under_cover_now { &[0.0] } else { &[0.0, -WING_SPAN, WING_SPAN] };
                    for &w in wides {
                        let a = [eye[0] + side_[0] * w, eye[1], eye[2] + side_[1] * w];
                        if v.hidden(a, [a[0] + dir[0] * l, a[1], a[2] + dir[1] * l]) {
                            l = l.min(40.0);
                        }
                    }
                    // neither the map nor other vehicles in between (parked ones are not reported by `hidden`)
                    for o in &seen_ones {
                        let a = [o.pos[0] - pos[0], o.pos[2] - pos[2]];
                        let t = a[0] * dir[0] + a[1] * dir[1];
                        if o.vehicle != i.vehicle && o.radius < 25.0 && (6.0..l).contains(&t) && (a[0] * dir[1] - a[1] * dir[0]).abs() < 4.0 + 0.5 * o.radius.min(15.0) && (o.pos[1] - pos[1]).abs() < 8.0 {
                            l = t;
                        }
                    }
                    l
                };
                let candidates = [0.0f32, 15.0, -15.0, 30.0, -30.0, 45.0, -45.0, 60.0, -60.0, 90.0, -90.0, 120.0, -120.0, 150.0, -150.0, 180.0];
                let mut best = (0.0f32, f32::MIN);
                for g in candidates {
                    let l = free(g);
                    if l >= RUNWAY_LENGTH {
                        best = (g, l);
                        break;
                    }
                    if l > best.1 + 1.0 {
                        best = (g, l);
                    }
                }
                let (gs, gc) = best.0.to_radians().sin_cos();
                self.runway = Some([sn * gc + c * gs, c * gc - sn * gs]);
            }
            let runway = self.runway.unwrap_or([f[0], f[2]]);
            let e = angle_between(f, runway);
            // (some tail wheels turn the opposite way to the rudder: it watches what it does and corrects)
            if v.time >= self.look_wheel {
                if let Some((t, before)) = self.wheel
                    && v.time > t
                    && s > 2.0
                    && e.abs() > before.abs() + 0.5
                    && e.abs() > 3.0
                {
                    self.wheel_sign = -self.wheel_sign;
                }
                self.wheel = Some((v.time, e));
                self.look_wheel = v.time + 0.5;
            }
            let sign = if s < 20.0 { self.wheel_sign } else { 1.0 };
            axes.push((axis::YAW, sign * (e / 15.0).clamp(-1.0, 1.0)));
            // with something ahead it does not manage to take off: better to brake to turn
            if e.abs() > 25.0 {
                axes.retain(|x| x.0 != axis::THROTTLE);
                axes.push((axis::THROTTLE, if s > 8.0 { 0.0 } else { 0.5 }));
            }
            // an obstacle or a dead end: after a while it gives up
            if s < 2.0 {
                let since = *self.stopped_since.get_or_insert(v.time);
                if v.time - since > 25.0 {
                    return true;
                }
            } else {
                self.stopped_since = None;
            }
            let ahead_obstacle = s > 12.0 && v.hidden(eye, [eye[0] + vel[0] * 2.5, eye[1] + vel[1] * 2.5, eye[2] + vel[2] * 2.5]);
            let throws = s > V_ROTATE || (ahead_obstacle && s > V_LOST);
            let cab = if throws { 0.08 * ((if ahead_obstacle { 25.0 } else { 15.0 }) - pit) - 0.03 * dpit } else { 0.0 };
            axes.push((PITCH, PITCH_SIGN * cab.clamp(-1.0, 1.0)));
            axes.push((BANK, (0.03 * kind + 0.01 * drol).clamp(-1.0, 1.0)));
            return false;
        }
        // ---- in the air
        // an enemy aircraft in sight, or one on its tail: the fight rules (not while just off the runway)
        if self.dogfight && h > MIN_HEIGHT {
            // (where its own are matters only to what cannot fight back)
            let refuge = if super::dogfight::fighter(i) { None } else { v.flags().iter().filter(|b| b.team == v.me.team).map(|b| b.pos).min_by(|a, b| flat(pos, [a[0], a[2]]).total_cmp(&flat(pos, [b[0], b[2]]))) };
            let ordered = self.flight.and_then(|o| if let FlightEnd::Engage { player } = o.end { Some(player) } else { None });
            let skill = super::dogfight::Skill { error_degrees: shooter.error_degrees, reaction_s: shooter.median_reaction, ordered, taken: &self.taken, refuge };
            if let Some(fight) = self.fight.fly(v, i, others, rng, &mut self.lock, &skill, &|x, z| Pilot::ground(v, x, z), player) {
                if trace_air() && v.time >= self.trace_t + 1.0 {
                    self.trace_t = v.time;
                    eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} dogfight lock {:?}", v.time, self.lock.map(|l| l.player));
                }
                axes.retain(|x| x.0 != axis::THROTTLE);
                axes.extend(fight);
                return false;
            }
        } else {
            self.lock = None;
        }
        // B23: an attack run: pulling away (straight, climbing) or coming in on the target (what it aims at is fixed for the run)
        let pass = if let Some(FlightOrder { end: FlightEnd::Pass { target, .. }, .. }) = self.flight { Some(target) } else { self.run = None; None };
        let bombs = pass.is_some() && i.alt_fire == AltFire::Drops;
        let mut forced: Option<Option<Target>> = None;
        let mut meta = self.air_goal(v, rng, pos, f);
        if let Some(tg) = pass {
            let aim = others.iter().filter(|o| o.team != v.me.team && flat(o.pos, [tg[0], tg[2]]) < PASS_FIND_M).min_by(|a, b| (!a.in_vehicle).cmp(&!b.in_vehicle).then(flat(a.pos, [tg[0], tg[2]]).total_cmp(&flat(b.pos, [tg[0], tg[2]]))));
            let at = aim.map_or(tg, |o| o.pos);
            let d = flat(pos, [at[0], at[2]]);
            let ahead_deg = angle_between(f, [at[0] - pos[0], at[2] - pos[2]]);
            // where it pulls away to: straight on if that stays inside the play area with room to turn back, else the nearest heading
            // that does (Kursk's planes flew out of it and died)
            let area = v.combat_area();
            let inside = |p: [f32; 2], m: f32| p[0] > area[0] + m && p[0] < area[0] + area[2] - m && p[1] > area[1] + m && p[1] < area[1] + area[3] - m;
            let away = |vel: [f32; 3]| {
                let n = vel[0].hypot(vel[2]).max(1.0);
                let d0 = [vel[0] / n, vel[2] / n];
                let turned = |g: f32| {
                    let (sn, c) = g.to_radians().sin_cos();
                    [d0[0] * c - d0[1] * sn, d0[0] * sn + d0[1] * c]
                };
                let ends = |d: [f32; 2]| [at[0] + d[0] * PASS_OUT_M, at[2] + d[1] * PASS_OUT_M];
                [0.0f32, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0, 120.0, -120.0, 150.0, -150.0, 180.0].iter().map(|&g| turned(g)).find(|&d| inside(ends(d), PASS_EDGE_M)).unwrap_or(d0)
            };
            let run = match self.run {
                Some(r) if flat(r.target, [tg[0], tg[2]]) < 300.0 => self.run.insert(Run { target: tg, ..r }),
                _ => self.run.insert(Run { target: tg, phase: if d > 0.7 * PASS_OUT_M { RunPhase::In } else { RunPhase::Out { dir: away(vel) } }, since: v.time, dropped: f64::MIN, aligned: false, best_miss: f32::MAX }),
            };
            match run.phase {
                RunPhase::Out { dir } => {
                    meta = [pos[0] + dir[0] * 1000.0, pos[2] + dir[1] * 1000.0];
                    forced = Some(None);
                    let gone = v.time - run.since;
                    let edge = !inside([pos[0], pos[2]], PASS_EDGE_M);
                    if (gone > PASS_OUT_LEAST_S && (d > PASS_OUT_M || edge)) || gone > PASS_OUT_MOST_S {
                        *run = Run { phase: RunPhase::In, since: v.time, aligned: false, best_miss: f32::MAX, ..*run };
                    }
                }
                RunPhase::In => {
                    meta = [at[0], at[2]];
                    run.aligned |= ahead_deg.abs() < PASS_ALIGNED;
                    let done = d < PASS_OVER_M || (run.aligned && ahead_deg.abs() > PASS_BEHIND && d < PASS_BEHIND_M) || (run.dropped > run.since && v.time - run.dropped > PASS_AFTER_DROP_S);
                    if done {
                        if trace_air() {
                            eprintln!("air t={:.0} {player} run ends: {} at {d:.0} m off {ahead_deg:.0} h {h:.0}", v.time, if run.dropped > run.since { "dropped" } else if d < PASS_OVER_M { "over" } else { "behind" });
                        }
                        *run = Run { phase: RunPhase::Out { dir: away(vel) }, since: v.time, ..*run };
                        forced = Some(None);
                    } else {
                        let d3 = (0..3).map(|k| (at[k] - pos[k]).powi(2)).sum::<f32>().sqrt();
                        forced = Some(Some((aim.map_or(usize::MAX, |o| o.player), at, d3)));
                    }
                }
            }
            if trace_air() && v.time >= self.trace_t + 1.0 {
                self.trace_t = v.time;
                eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} pass {:?} target {d:.0} m off {ahead_deg:.0} seen {} bombs {bombs}", v.time, run.phase, aim.is_some());
            }
        }
        let in_run = matches!(self.run, Some(Run { phase: RunPhase::In, .. }));
        let d_goal = flat(pos, meta);
        let mut d_xz = [meta[0] - pos[0], meta[1] - pos[2]];
        // over the flag it circles: heading for a point on a ring ahead. Ordered over a point, straight at it, again and again; but
        // while it is too far below it to climb what is left on the way in, it climbs on a wide ring around it (what holds a flag up
        // in the air is in the way of whoever comes in below it)
        let over = if let Some(FlightOrder { to, end: FlightEnd::Over, .. }) = self.flight { Some(to[1] - pos[1]) } else { None };
        let ring = match over {
            None if pass.is_some() => None,
            None => (d_goal < 300.0).then_some(250.0),
            Some(up) => (up > OVER_LOW_M && d_goal < OVER_CLIMB * up).then_some(OVER_RING_M),
        };
        if let Some(r) = ring {
            let t = [-d_xz[1] / d_goal.max(1.0), d_xz[0] / d_goal.max(1.0)];
            let g = if player.is_multiple_of(2) { 1.0 } else { -1.0 };
            d_xz = [t[0] * g * r + d_xz[0] * (d_goal - r) / d_goal.max(1.0), t[1] * g * r + d_xz[1] * (d_goal - r) / d_goal.max(1.0)];
        }
        let mut heading_d = angle_between(f, d_xz);
        // what it sees: the nearest enemy of what is ahead of it
        // (what is behind or right below cannot be attacked: if it looked at it, it would not see what it does have ahead)
        let ahead: Vec<Other> = others.iter().filter(|o| o.team == v.me.team || (angle_between(f, [o.pos[0] - pos[0], o.pos[2] - pos[2]]).abs() < AIR_CONE && flat(o.pos, [pos[0], pos[2]]) > 60.0)).cloned().collect();
        shooter.see(v, rng, [pos[0], pos[1] - 1.0, pos[2]], AIR_RANGE, &ahead);
        if let Some(t) = forced {
            shooter.target = t;
        }
        if trace_air() && shooter.target.is_none() && v.time >= self.trace_t + 1.0 {
            self.trace_t = v.time;
            let in_range = |o: &&Other| o.team != v.me.team && (0..3).map(|k| (o.pos[k] - pos[k]).powi(2)).sum::<f32>() < SHOT_RANGE * SHOT_RANGE;
            eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} no target: enemies in range {}, ahead {}", v.time, others.iter().filter(in_range).count(), ahead.iter().filter(in_range).count());
        }
        let mut goal_alt = below + CRUISE_HEIGHT;
        // (ordered over a point: at its height, if that is higher: a platform up in the air)
        if let Some(FlightOrder { to, end: FlightEnd::Over, .. }) = self.flight {
            goal_alt = goal_alt.max(to[1]);
            // (and slowly as it gets there: whoever jumps over it keeps the plane's speed)
            if flat(pos, [to[0], to[2]]) < OVER_SLOW_M && s > OVER_SLOW * V_LOST {
                axes.retain(|x| x.0 != axis::THROTTLE);
                axes.push((axis::THROTTLE, 0.0));
            }
        }
        // (pulling away it climbs back to its cruise; coming in with bombs, level over the target)
        if bombs && in_run {
            goal_alt = shooter.target.map_or(goal_alt, |b| (b.1[1] + PASS_BOMB_UP).max(below + MIN_HEIGHT + 10.0));
        }
        let mut diving = false;
        // where what it looks at is moving, seen from one time to the next (as anyone sees it); and how far ahead of it the shot must lead
        if let Some((p, at, _)) = shooter.target {
            match self.seen_target {
                Some((q, before, t)) if q == p && v.time - t >= 0.2 => {
                    let dt = (v.time - t) as f32;
                    self.target_speed = std::array::from_fn(|k| (at[k] - before[k]) / dt);
                    self.seen_target = Some((p, at, v.time));
                }
                Some((q, ..)) if q == p => {}
                _ => {
                    self.target_speed = [0.0; 3];
                    self.seen_target = Some((p, at, v.time));
                }
            }
        } else {
            self.seen_target = None;
        }
        let lead: [f32; 3] = shooter.target.map_or([0.0; 3], |b| std::array::from_fn(|k| (self.target_speed[k] - vel[k]) * b.2.min(SHOT_RANGE) / BULLET_SPEED));
        if let Some((_, at, d)) = shooter.target {
            let a: [f32; 3] = std::array::from_fn(|k| at[k] + lead[k] - pos[k]);
            let slope = a[1].atan2((a[0] * a[0] + a[2] * a[2]).sqrt()).to_degrees();
            let g_at = angle_between(f, [a[0], a[2]]);
            // it has it more or less ahead and can descend
            if bombs && in_run {
                // (the bomb keeps the plane's speed: it heads for where the target will be when it lands)
                let fall = ((2.0 * (pos[1] - at[1]).max(1.0)) / crate::GRAVITY as f32).sqrt();
                heading_d = angle_between(f, [at[0] + self.target_speed[0] * fall - pos[0], at[2] + self.target_speed[2] * fall - pos[2]]);
            } else if g_at.abs() < 70.0 && slope > -55.0 && h > MIN_HEIGHT && v.time > self.dive_out {
                heading_d = g_at;
                diving = true;
                goal_alt = at[1];
            }
            // (it fires if the burst passes near it, not if it goes more or less that way)
            let cone_ang = (BURST_RADIUS / d.max(1.0)).atan().to_degrees().max(1.5);
            let cone = g_at.abs() < cone_ang && (slope - pit).abs() < cone_ang + 2.0;
            if trace_air() && v.time >= self.trace_t + 1.0 {
                self.trace_t = v.time;
                eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} target {d:.0} m turn {g_at:.0} pitch {:.0} (slope {slope:.0} nose {pit:.0}) cone {cone_ang:.1} ready {:.1} diving {}", v.time, slope - pit, (shooter.ready_at - v.time).max(0.0), v.time > self.dive_out);
            }
            if cone && d < SHOT_RANGE && v.time >= shooter.ready_at {
                self.burst_until = v.time + 0.4;
            }
            // missiles at what flies, if it carries any: what the secondary key carries is used from afar if it is not a bomb
            let in_the_air_the = Pilot::target_flies(v, others, shooter.target);
            // (only if it fires them: with bombs on that key, a bomber in a dogfight dropped its whole load into the sea)
            let alt = v.me.inside.as_ref().map_or(AltFire::None, |i| i.alt_fire);
            // rockets at what goes by land, from closer and with the nose on it (they fly straight, as the machine guns)
            let reach = if in_the_air_the { 800.0 } else { 500.0 };
            if alt == AltFire::Fires && g_at.abs() < 8.0 && (slope - pit).abs() < 12.0 && d < reach && v.time >= self.missile_at {
                axes.push((axis::ALT_FIRE, 1.0));
                self.missile_at = v.time + if in_run { 0.5 } else { 3.0 };
            }
            // bombs: a bomb keeps moving forward as it falls, so it is dropped when where it would land is where the target is
            let drop = pos[1] - at[1];
            if alt == AltFire::Drops && !in_the_air_the && drop > 30.0 {
                // (the world's gravity, not the Earth's: with 9.81 every bomb fell ~18 % short of the target, 40-60 m)
                let (low, g) = (-vel[1], crate::GRAVITY as f32);
                let t = (low + (low * low + 2.0 * g * drop).sqrt()) / g;
                let impact = [pos[0] + vel[0] * t, pos[2] + vel[2] * t];
                let friend_near = others.iter().any(|o| o.team == v.me.team && flat(o.pos, impact) < FRIEND_BOMB_RADIUS);
                // (where the target will be when the bomb lands: a ship at 10 m/s goes 40 m in a bomb's fall)
                let there = [at[0] + self.target_speed[0] * t, at[1], at[2] + self.target_speed[2] * t];
                let miss = flat(there, impact);
                // (coming in, the nearest it gets, if near enough)
                let passed = match self.run.as_mut().filter(|r| r.phase == RunPhase::In && r.dropped < r.since) {
                    Some(r) => {
                        let gone_by = miss > r.best_miss + 2.0 && r.best_miss < PASS_LOOSE_M;
                        r.best_miss = r.best_miss.min(miss);
                        gone_by
                    }
                    None => false,
                };
                if (miss < BOMB_RADIUS || passed) && !friend_near {
                    axes.push((axis::ALT_FIRE, 1.0));
                    if let Some(r) = self.run.as_mut().filter(|r| r.phase == RunPhase::In) {
                        if r.dropped < r.since {
                            r.dropped = v.time;
                        }
                    }
                }
            }
        }
        if v.time < self.burst_until {
            axes.push((axis::FIRE, 1.0));
        }
        // what is ahead rules over height: the angle needed to clear each point
        let ahead = |t: f32, turn_val: f32| {
            let (gs, gc) = turn_val.to_radians().sin_cos();
            let dir = [vel[0] * gc + vel[2] * gs, vel[2] * gc - vel[0] * gs];
            Pilot::ground(v, pos[0] + dir[0] * t, pos[2] + dir[1] * t)
        };
        let is_needed = |turn_val: f32| {
            [1.0f32, 2.0, 3.0, 5.0, 8.0, 11.0].iter().map(|t| (ahead(*t, turn_val) + MIN_HEIGHT - pos[1]).atan2(s.max(20.0) * t).to_degrees()).fold(f32::MIN, f32::max)
        };
        let mut boards = is_needed(0.0);
        // too steep: better to one side
        if boards > 22.0 {
            let (a, b) = (is_needed(45.0), is_needed(-45.0));
            if a < boards.min(b) {
                heading_d = 45.0;
                boards = a;
            } else if b < boards {
                heading_d = -45.0;
                boards = b;
            }
        }
        // something on the map ahead, seen without more ado
        if v.time >= self.look_ahead {
            self.look_ahead = v.time + 0.2;
            self.obstacle = v.hidden(pos, [pos[0] + vel[0] * 3.0, pos[1] + vel[1] * 3.0, pos[2] + vel[2] * 3.0]);
        }
        let diving = diving && boards < -3.0 && !self.obstacle;
        if !diving && shooter.target.is_some() && !(bombs && in_run) && pos[1] < goal_alt.max(below + MIN_HEIGHT) {
            self.dive_out = v.time + 6.0;
            goal_alt = below + CRUISE_HEIGHT;
        }
        let alt_d = if diving { goal_alt } else { goal_alt.max(pos[1] + 0.0_f32.min(boards)) };
        let err_alt = alt_d - pos[1];
        let mut pit_d = if diving {
            let a = shooter.target.map_or([0.0; 3], |b| std::array::from_fn(|k| b.1[k] + lead[k] - pos[k]));
            a[1].atan2((a[0] * a[0] + a[2] * a[2]).sqrt()).to_degrees().clamp(-45.0, 20.0)
        } else {
            (err_alt * 0.25).clamp(-12.0, 22.0)
        };
        let mut kind_d = -(heading_d * 1.2).clamp(-BANK_MAX, BANK_MAX);
        // hugging what is ahead: up, with the wings level
        if boards > 0.0 || self.obstacle {
            pit_d = pit_d.max((boards + 6.0).min(35.0));
            if boards > 15.0 || self.obstacle {
                pit_d = pit_d.max(30.0);
                kind_d = kind_d.clamp(-20.0, 20.0);
            }
        }
        // near the ground, and more the faster it runs, it does not turn tightly: a tight turn loses height and does not let it climb
        let just = 40.0 + 1.2 * s;
        if h < just {
            let cap = (BANK_MAX * (h / just).max(0.4)).max(20.0);
            kind_d = kind_d.clamp(-cap, cap);
        }
        // low speed: nose down, and wings level
        if s < V_LOST {
            pit_d = pit_d.min(-8.0);
            kind_d = kind_d.clamp(-15.0, 15.0);
        }
        // the controls' authority grows with the square of the speed
        let k = (45.0 / s.max(20.0)).powi(2).clamp(0.3, 3.0);
        // turning, you have to pull more not to lose height
        let extra = 8.0 * (1.0 - kind.to_radians().cos().abs()) * (kind.to_radians().cos() > 0.0) as i32 as f32;
        let cab = k * (0.05 * (pit_d - pit + extra) - 0.02 * dpit);
        let ala = k * (0.02 * (kind - kind_d) + 0.006 * drol);
        axes.push((PITCH, PITCH_SIGN * cab.clamp(-1.0, 1.0)));
        axes.push((BANK, ala.clamp(-1.0, 1.0)));
        axes.push((axis::YAW, (0.03 * heading_d).clamp(-0.4, 0.4)));
        false
    }

    /// At the controls of a helicopter: the throttle raises and lowers the collective little by little, so vertical speed is commanded;
    /// to advance the nose is tilted. First of all, not crashing.
    /// `land`: if present, it goes to that point instead of to a flag, and on arrival (closer than [`LAND_M`]) it descends to what is below
    /// (the ground, or a dock or deck it is over: [`BotView::floor`]) and
    /// stays (bots3: whoever carries passengers drops them near the flag; whoever waits for them does not take off). Without it, as usual.
    #[allow(clippy::too_many_arguments)]
    // (B11/B23: a plane with bombs or rockets attacks what goes by land or sea in runs, [`FlightEnd::Pass`]; `BYTEFIELD_BOTS3_AIR=1`,
    // with a line per run's end, shows it.)
    /// bots3: a VTOL takeoff ([`Pilot::vtol`]), for a craft its throttle lifts straight up ([`crate::bot_api::Inside::lift`]: GC's
    /// fighters, the Falcon). It needs no runway: it rises a few metres off where it stands (`VTOL_SLIDE_HEIGHT`, under the ceiling
    /// if there is one), slides out at that height towards the nearest open sky (sliding, not turning: hanging still, the yaw does not
    /// turn it, while the pitch and roll sticks tilt the lift racks and move it along and across), goes straight up under the open sky
    /// and then puts the throttle forward. The walls are what a craft hits ([`BotView::craft_clear`]: `hidden` does not see a
    /// hangar's walls from inside). What it looks at it looks at once, when it rises (16 ways out, about 500 rays), and then the sky
    /// overhead four times a second. Stuck for `VTOL_GIVE_UP_S`, or with no way out, it takes the runway as a plane does.
    /// TODO: the Falcon in GC_Hoth's hangar (`BYTEFIELD_VTOL_FINE=1 BYTEFIELD_BOTS3_AIR=1 examples/air_aim data/game GCMOD GC_Hoth 52
    /// --seat all`, player 24) gets out in ~35 s scraping the walls all the way (it is 16 m to each side under an 11.8 m ceiling, and
    /// what lifts it pushes it ahead), then banks over in the climb with the roll stick hard over and crashes: its slide and its
    /// level-holding are guesses (`air_envelope … milleniumfalcon --hover`: the pitch stick drives it along, the roll stick barely
    /// moves it); it crashed before this too.
    fn vtol_takeoff(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, dpit: f32, drol: f32, player: usize) {
        let (pos, vel) = (i.pos, i.velocity);
        let (f, u, r) = (i.rows[2], i.rows[1], i.rows[0]);
        let up = i.lift;
        let sky = |p: [f32; 3]| v.craft_clear(p, [p[0], p[1] + VTOL_SKY_M, p[2]]) >= VTOL_SKY_M - 0.5;
        let span = Pilot::vtol_half_span(v, i);
        // (open over all of it: its middle, its wing tips and its tail, as it slides along `dir`)
        let sky_over = |dir: [f32; 2]| {
            let side = [dir[1], -dir[0]];
            sky(pos)
                && sky([pos[0] - dir[0] * VTOL_TAIL_M, pos[1], pos[2] - dir[1] * VTOL_TAIL_M])
                && [-span, span].iter().all(|w| sky([pos[0] + side[0] * w, pos[1], pos[2] + side[1] * w]))
        };
        let st = *self.takeoff.get_or_insert_with(|| {
            let overhead = v.craft_clear([pos[0], pos[1] + 1.0, pos[2]], [pos[0], pos[1] + 1.0 + VTOL_SKY_M, pos[2]]);
            let phase = if overhead >= VTOL_SKY_M - 0.5 { VtolPhase::Climb { dir: [f[0], f[2]] } } else { VtolPhase::Rise };
            if trace_air() {
                eprintln!("air t={:.0} {player} vtol start {} #{} at {:.0},{:.0},{:.0} overhead {overhead:.1} m, half span {:.1}: {phase:?}", v.time, i.template, i.vehicle, pos[0], pos[1], pos[2], Pilot::vtol_half_span(v, i));
            }
            let rest = (pos[1] - v.floor(pos[0], pos[2], pos[1]).unwrap_or(pos[1])).clamp(0.0, 4.0);
            Vtol { phase, floor: pos[1], slide: (0.5 * overhead).clamp(1.5, VTOL_SLIDE_HEIGHT), start: [pos[0], pos[2]], best: 0.0, best_at: v.time, look_sky: 0.0, under: pos[1], rest, blocked_since: None, tries: 0, higher: 0.0 }
        });
        // hanging at a height by the throttle, all or nothing (half of it may not lift at all)
        let hold = |y: f32| if pos[1] + 0.8 * vel[1] < y { up } else { 0.0 };
        // sliding along `dir` at `speed` (0: holding still), as a helicopter: hanging, the pitch stick puts the nose down and it goes
        // ahead, the roll stick banks it and it goes to that side (the racks tilt with them); left alone it pitches up by itself
        let pit = pitch(f);
        let kind = r[1].atan2(u[1]).to_degrees();
        let slide = |axes: &mut Vec<(usize, f32)>, dir: [f32; 2], speed: f32| {
            let n = |x: f32, z: f32| {
                let l = x.hypot(z).max(1e-3);
                [x / l, z / l]
            };
            let (fwd, side) = (n(f[0], f[2]), n(r[0], r[2]));
            let err = [dir[0] * speed - vel[0], dir[1] * speed - vel[2]];
            let (e_fwd, e_side) = (err[0] * fwd[0] + err[1] * fwd[1], err[0] * side[0] + err[1] * side[1]);
            if up > 0.0 {
                // (what the throttle pushed lifts, the Falcon: the pitch stick drives it along without tilting it much; it is held level)
                axes.push((PITCH, (0.3 * e_fwd + PITCH_SIGN * (0.06 * -pit - 0.03 * dpit)).clamp(-1.0, 1.0)));
                axes.push((BANK, (0.3 * e_side + 0.06 * kind + 0.02 * drol).clamp(-1.0, 1.0)));
                return;
            }
            let pit_d = (-1.5 * e_fwd).clamp(-VTOL_TILT, VTOL_TILT);
            let bank_d = (1.5 * e_side).clamp(-VTOL_TILT, VTOL_TILT);
            axes.push((PITCH, PITCH_SIGN * (0.06 * (pit_d - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
            axes.push((BANK, (0.06 * (kind + bank_d) + 0.02 * drol).clamp(-1.0, 1.0)));
        };
        let trace = |what: &str| {
            if trace_air() {
                eprintln!("air t={:.0} {player} vtol {what} {} h {:.1} v {:.0}", v.time, i.template, pos[1] - st.floor, (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt());
            }
        };
        let mut next = st;
        match st.phase {
            VtolPhase::Rise => {
                axes.push((axis::THROTTLE, hold(st.floor + st.slide)));
                slide(axes, [0.0, 0.0], 0.0);
                if pos[1] >= st.floor + st.slide - 0.5 {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, 0, &mut next.tries);
                    (next.start, next.best, next.best_at) = ([pos[0], pos[2]], 0.0, v.time);
                    trace(&format!("{:?}", next.phase));
                } else if v.time - st.best_at > VTOL_GIVE_UP_S {
                    next.phase = VtolPhase::Failed;
                    trace("cannot rise");
                }
            }
            VtolPhase::Out { dir, until, leg } => {
                axes.push((axis::THROTTLE, hold(st.floor.max(st.under) + st.slide)));
                slide(axes, dir, if st.blocked_since.is_some() { 0.0 } else { VTOL_SLIDE });
                let gone = (pos[0] - st.start[0]) * dir[0] + (pos[2] - st.start[1]) * dir[1];
                if gone > st.best + 1.0 {
                    (next.best, next.best_at) = (gone, v.time);
                }
                if v.time >= st.look_sky {
                    next.look_sky = v.time + 0.25;
                    // (what it stands over: the floor as it rose, then what is below it and 4 m ahead, as high as it is)
                    let top = pos[1] + 1.0;
                    let ahead = [pos[0] + dir[0] * 4.0, pos[2] + dir[1] * 4.0];
                    next.under = v.floor(pos[0], pos[2], top).into_iter().chain(v.floor(ahead[0], ahead[1], top)).fold(f32::MIN, f32::max) + st.rest;
                    // a craft with someone at the controls ahead in its way (one parked was left out of the way out)
                    let side = [dir[1], -dir[0]];
                    let in_the_way = v.vehicles().iter().any(|o| {
                        let a = [o.pos[0] - pos[0], o.pos[2] - pos[2]];
                        let (t, w, r) = (a[0] * dir[0] + a[1] * dir[1], (a[0] * side[0] + a[1] * side[1]).abs(), o.radius.min(15.0));
                        // (of two crafts in each other's way, the one with the higher number waits)
                        o.vehicle < i.vehicle && !o.driver_free && !o.fixed && o.class.flies() && t > -r && t < 10.0 + r && w < span + r && (o.pos[1] - pos[1]).abs() < 6.0 + r
                    });
                    next.blocked_since = if in_the_way { st.blocked_since.or(Some(v.time)) } else { None };
                    if in_the_way {
                        next.best_at = v.time;
                    }
                    if sky_over(dir) {
                        next.phase = VtolPhase::Climb { dir };
                        trace("open sky");
                    }
                }
                // waited long enough: another way out from here
                if next.phase == st.phase && next.blocked_since.is_some_and(|b| v.time - b > VTOL_WAIT_S) {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, leg, &mut next.tries);
                    (next.start, next.best, next.best_at, next.blocked_since) = ([pos[0], pos[2]], 0.0, v.time, None);
                    trace(&format!("waited; {:?}", next.phase));
                }
                // the end of a leg that does not reach the sky: the way out again from here
                if next.phase == st.phase && until.is_some_and(|u| gone >= u) {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, leg, &mut next.tries);
                    (next.start, next.best, next.best_at) = ([pos[0], pos[2]], 0.0, v.time);
                    trace(&format!("{:?}", next.phase));
                }
                if next.phase == st.phase && v.time - next.best_at > VTOL_GIVE_UP_S {
                    next.phase = VtolPhase::Failed;
                    trace("stuck sliding out");
                }
            }
            VtolPhase::Climb { dir } => {
                // (over the ground where it is and where the nose will take it once the throttle is forward)
                let ahead = [50.0f32, 100.0, 150.0].iter().map(|d| Pilot::ground(v, pos[0] + f[0] * d, pos[2] + f[2] * d) - 0.2 * d).fold(f32::MIN, f32::max);
                let top = Pilot::ground(v, pos[0], pos[2]).max(st.floor).max(ahead) + VTOL_CLIMB_M;
                axes.push((axis::THROTTLE, hold(top + st.higher)));
                slide(axes, dir, VTOL_SLIDE);
                // (and the way ahead of the nose clear for the run, counting what it sinks before its wings hold it; if not, higher)
                // (a craft that flies hanging flies from here: it never puts the throttle forward for its wings)
                if i.hangs && pos[1] >= top - 2.0 {
                    (self.in_the_air, self.hover.on) = (true, true);
                    trace("hanging");
                } else if pos[1] >= top - 2.0 + st.higher && v.time >= st.look_sky {
                    next.look_sky = v.time + 0.25;
                    let l = f[0].hypot(f[2]).max(1e-3);
                    let run = [pos[0] + f[0] / l * VTOL_RUN_M, pos[1] - VTOL_SINK_M, pos[2] + f[2] / l * VTOL_RUN_M];
                    if v.craft_clear(pos, run) >= VTOL_RUN_M - 1.0 || st.higher >= VTOL_HIGHER_M {
                        (next.phase, next.best_at) = (VtolPhase::Go, v.time);
                        trace("go");
                    } else {
                        next.higher = st.higher + 10.0;
                    }
                }
            }
            VtolPhase::Go => {
                let s = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
                let h = pos[1] - Pilot::ground(v, pos[0], pos[2]).max(st.floor);
                // sinking low before the wings hold it: the lift again for a moment (the throttle back is the lift, or forward is both)
                let sinking = up < 0.0 && h < 15.0 && vel[1] < -2.0;
                axes.push((axis::THROTTLE, if sinking { up } else { 1.0 }));
                // (something in its way in the next 3 s: it pulls up)
                let reach = [pos[0] + vel[0] * 3.0, pos[1] + vel[1] * 3.0, pos[2] + vel[2] * 3.0];
                let nose = if s > 10.0 && v.craft_clear(pos, reach) < 3.0 * s - 1.0 { 25.0 } else { 5.0 };
                axes.push((PITCH, PITCH_SIGN * (0.08 * (nose - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
                axes.push((BANK, (0.03 * kind + 0.01 * drol).clamp(-1.0, 1.0)));
                // (what the throttle pushed lifts climbs as long as it is pushed, and a gunship never goes much faster than 20 m/s)
                let flies = if up > 0.0 { s > 15.0 || v.time - st.best_at > 3.0 } else { s > V_LOST + 10.0 && vel[1] > -2.0 };
                if flies || v.time - st.best_at > 2.0 * VTOL_GIVE_UP_S {
                    self.in_the_air = true;
                    trace("flying");
                }
            }
            VtolPhase::Hold { until } => {
                axes.push((axis::THROTTLE, hold(st.floor.max(st.under) + st.slide)));
                slide(axes, [0.0, 0.0], 0.0);
                if v.time >= until {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, 0, &mut next.tries);
                    (next.start, next.best, next.best_at, next.blocked_since) = ([pos[0], pos[2]], 0.0, v.time, None);
                    trace(&format!("again; {:?}", next.phase));
                }
            }
            VtolPhase::Failed => {}
        }
        if crate::debug::options().bots3_air && std::env::var_os("BYTEFIELD_VTOL_FINE").is_some() {
            let get = |k: usize| axes.iter().rev().find(|x| x.0 == k).map_or(0.0, |x| x.1);
            eprintln!("vtol t={:.2} {player} {} {:?} h {:.1} vy {:+.1} vh {:.1} pitch {:+.0} bank {:+.0} thr {:+.1} p {:+.2} b {:+.2}", v.time, i.template, st.phase, pos[1] - st.floor, vel[1], vel[0].hypot(vel[2]), pit, -kind, get(axis::THROTTLE), get(PITCH), get(BANK));
        }
        self.takeoff = Some(next);
    }

    /// Half the room a craft needs to each side when it slides, m.
    fn vtol_half_span(v: &BotView, i: &Inside) -> f32 {
        v.vehicle(i.vehicle).map_or(VTOL_HALF_SPAN, |x| (VTOL_SPAN_OF_RADIUS * x.radius).max(VTOL_HALF_SPAN))
    }

    /// The next leg out after `leg` legs, at the height `y`: the way out ([`Pilot::vtol_way_out`]); with none, or with all its legs
    /// gone, it hangs a while and looks again from leg 1, and after `VTOL_TRIES` looks it gives up.
    fn vtol_next_leg(&self, v: &BotView, i: &Inside, y: f32, leg: u8, tries: &mut u8) -> VtolPhase {
        *tries += 1;
        match self.vtol_way_out(v, i, y).filter(|_| leg < VTOL_LEGS) {
            Some((dir, until)) => VtolPhase::Out { dir, until, leg: leg + 1 },
            None if *tries < VTOL_TRIES => VtolPhase::Hold { until: v.time + VTOL_HOLD_S },
            None => VtolPhase::Failed,
        }
    }

    /// The way out of where a VTOL craft stands to the nearest open sky, at the height `y` it slides at (x, z): of 16 ways around
    /// it, the one that reaches open sky soonest, with room for its wings and no craft parked in the way. When none reaches it, the
    /// one with most room, as a leg to look again from (with how far to go along it); `None` with nowhere to go.
    fn vtol_way_out(&self, v: &BotView, i: &Inside, y: f32) -> Option<([f32; 2], Option<f32>)> {
        let (pos, f) = (i.pos, i.rows[2]);
        let l = f[0].hypot(f[2]).max(1e-3);
        let (c, sn) = (f[2] / l, f[0] / l);
        let parked = v.vehicles();
        let mut best: Option<(f32, f32, [f32; 2])> = None;
        let mut roomiest: Option<(f32, [f32; 2])> = None;
        let span = Pilot::vtol_half_span(v, i);
        for k in 0..16 {
            let g = (k as f32 * 22.5).to_radians();
            let (gs, gc) = g.sin_cos();
            let dir = [sn * gc + c * gs, c * gc - sn * gs];
            let side = [dir[1], -dir[0]];
            let mut room = VTOL_OUT_M;
            for w in [-span, 0.0, span] {
                let a = [pos[0] + side[0] * w, y, pos[2] + side[1] * w];
                room = room.min(v.craft_clear(a, [a[0] + dir[0] * VTOL_OUT_M, y, a[2] + dir[1] * VTOL_OUT_M]));
            }
            for o in &parked {
                let a = [o.pos[0] - pos[0], o.pos[2] - pos[2]];
                let t = a[0] * dir[0] + a[1] * dir[1];
                if o.vehicle != i.vehicle && o.radius < 25.0 && t > 0.0 && t < room && (a[0] * dir[1] - a[1] * dir[0]).abs() < span + 0.5 * o.radius.min(15.0) && o.pos[1] + 0.4 * o.radius.min(15.0) > y - 2.0 {
                    room = t - 0.5 * o.radius.min(15.0);
                }
            }
            if crate::debug::options().bots3_air && std::env::var_os("BYTEFIELD_VTOL_FINE").is_some() {
                eprintln!("vtol way {} at {:.0},{:.0},{:.0} way {k} dir {:.2},{:.2} room {room:.1}", i.template, pos[0], y, pos[2], dir[0], dir[1]);
            }
            // (a leg back the way it came is no leg: within 45 degrees of where it came from, a craft goes back and forth)
            let back = (i.velocity[0] * dir[0] + i.velocity[2] * dir[1]) < -0.7 * i.velocity[0].hypot(i.velocity[2]) && i.velocity[0].hypot(i.velocity[2]) > 1.0;
            if !back && room >= 2.0 * VTOL_OUT_STEP && roomiest.is_none_or(|b| room > b.0) {
                roomiest = Some((room, dir));
            }
            let mut t = VTOL_OUT_STEP;
            while t < room {
                let p = [pos[0] + dir[0] * t, y, pos[2] + dir[1] * t];
                if v.craft_clear(p, [p[0], p[1] + VTOL_SKY_M, p[2]]) >= VTOL_SKY_M - 0.5 {
                    // (the nearest, and of two as near, the one closer to the nose)
                    let off = (k as f32 * 22.5).min(360.0 - k as f32 * 22.5);
                    if best.is_none_or(|b| t < b.0 || t == b.0 && off < b.1) {
                        best = Some((t, off, dir));
                    }
                    break;
                }
                t += VTOL_OUT_STEP;
            }
        }
        best.map(|b| (b.2, None)).or(roomiest.map(|(room, dir)| (dir, Some(room - VTOL_OUT_STEP))))
    }

    /// bots3: at the controls of a craft that flies hanging by its lift ([`crate::bot_api::Inside::hangs`]: GC_Bespin's cloud car,
    /// the gunships, the skiff), once up ([`Pilot::vtol_takeoff`]). It flies as a helicopter: the throttle holds its height, all or
    /// nothing (half of it may not lift at all); the pitch and roll sticks tilt it, a little, and it goes along or to the side; the yaw
    /// turns it (the cloud car's only while it goes along). It goes from flag to flag at [`HOVER_HEIGHT`] over what is below and
    /// ahead (a platform too: [`BotView::floor`]), climbs and stops before a wall, and with someone in sight faces them, closes to
    /// [`HOVER_FIRE_M`] keeping some speed, holds the target's height when it flies, and fires when the nose is on it.
    /// TODO: in GC_Bespin it fights but hardly downs anything (B26: 5 % of its shots hit, ~0 downed a round), and late in a round
    /// several cloud cars circle one target and crash into each other or the city (3 in 240 s): `AIR_AIM_DAMAGE=1
    /// BYTEFIELD_BOTS3_AIR=1 examples/air_aim data/game GCMOD GC_Bespin 240 --seat all` (`hover … target … blocked` each second, the
    /// crashes as `dmg … source Crash`). The gunships and the skiff climb after fighters to their height (`fly_at`) and never reach
    /// them: what flies fast may be left to what flies fast.
    #[allow(clippy::too_many_arguments)]
    fn fly_hover(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, dpit: f32, drol: f32, player: usize) {
        let (pos, vel) = (i.pos, i.velocity);
        let (f, u, r) = (i.rows[2], i.rows[1], i.rows[0]);
        let pit = pitch(f);
        let kind = r[1].atan2(u[1]).to_degrees();
        let up = i.lift;
        let land = match self.flight {
            Some(FlightOrder { hold: true, .. }) => Some(pos),
            Some(FlightOrder { to, end: FlightEnd::Land, .. }) => Some(to),
            _ => None,
        };
        let meta = match land {
            Some(p) => [p[0], p[2]],
            None => self.air_goal(v, rng, pos, f),
        };
        shooter.see(v, rng, [pos[0], pos[1] - 1.0, pos[2]], AIR_RANGE.min(500.0), others);
        let target = shooter.target.filter(|_| land.is_none());
        // where it goes along the ground, how fast, and where its nose looks
        let (mut dir, mut speed, mut face) = {
            let d = [meta[0] - pos[0], meta[1] - pos[2]];
            let l = d[0].hypot(d[1]).max(1e-3);
            ([d[0] / l, d[1] / l], (0.25 * l).min(HOVER_SPEED), [d[0] / l, d[1] / l])
        };
        let mut fly_at: Option<f32> = None;
        if let Some((who, at, d)) = target {
            let a = [at[0] - pos[0], at[1] - pos[1], at[2] - pos[2]];
            let l = a[0].hypot(a[2]).max(1e-3);
            let to = [a[0] / l, a[2] / l];
            face = to;
            if d > HOVER_FIRE_M {
                (dir, speed) = (to, (0.2 * (d - HOVER_RING_M)).clamp(HOVER_KEEP, HOVER_SPEED));
            } else {
                // within range it circles it at `HOVER_RING_M`, nose on it, and never stops (the cloud car turns only while it goes
                // along) nor rams it: two that closed on each other to a few metres crashed into each other
                let side = if (who + player) % 2 == 0 { 1.0 } else { -1.0 };
                let k = ((l - HOVER_RING_M) / HOVER_RING_M).clamp(-1.0, 1.0) * 1.5;
                let w = [side * -to[1] + k * to[0], side * to[0] + k * to[1]];
                let n = w[0].hypot(w[1]).max(1e-3);
                (dir, speed) = ([w[0] / n, w[1] / n], HOVER_KEEP + 4.0);
            }
            if Pilot::target_flies(v, others, Some((who, at, d))) {
                fly_at = Some(at[1]);
            }
            let slope = a[1].atan2(l).to_degrees();
            if angle_between(f, [a[0], a[2]]).abs() < 6.0 && (slope - pit).abs() < 15.0 && d < SHOT_RANGE && v.time >= shooter.ready_at {
                self.burst_until = v.time + 0.3;
            }
        }
        if v.time < self.burst_until {
            axes.push((axis::FIRE, 1.0));
        }
        // ---- what is below and ahead, four times a second: the height it keeps and whether a wall stands in the way
        if v.time >= self.hover.look_at {
            self.hover.look_at = v.time + 0.25;
            let top = pos[1] + 2.0;
            self.hover.under = v.floor(pos[0], pos[2], top).unwrap_or_else(|| Pilot::ground(v, pos[0], pos[2]));
            let mut high = self.hover.under;
            for t in [1.0f32, 3.0, 6.0] {
                let (x, z) = (pos[0] + vel[0] * t, pos[2] + vel[2] * t);
                high = high.max(v.floor(x, z, top + 10.0).unwrap_or(f32::MIN)).max(Pilot::ground(v, x, z));
            }
            // (and not below where it goes: GC_Bespin's city stands on platforms over a void, and a flight between them
            // keeps to their height)
            high = high.max(v.floor(meta[0], meta[1], pos[1].max(high) + 100.0).unwrap_or(f32::MIN));
            let reach = [pos[0] + vel[0] * HOVER_AHEAD_S, pos[1] + vel[1].min(0.0) * HOVER_AHEAD_S, pos[2] + vel[2] * HOVER_AHEAD_S];
            let s = (vel[0] * vel[0] + vel[2] * vel[2]).sqrt();
            self.hover.blocked = s > 3.0 && v.craft_clear(pos, reach) < s * HOVER_AHEAD_S - 1.0;
            self.hover.alt = high + HOVER_HEIGHT + if self.hover.blocked { 15.0 } else { 0.0 };
            if trace_air() && v.time >= self.trace_t + 1.0 {
                self.trace_t = v.time;
                eprintln!("air t={:.0} {player} hover {} h {:.0} over {:.0} v {s:.0} target {:?} blocked {}", v.time, i.template, pos[1] - high, high, target.map(|t| (t.0, t.2 as i32)), self.hover.blocked);
            }
        }
        // (over what is below it, and up to what it fights in the air)
        let alt = fly_at.map_or(self.hover.alt, |y| y.max(self.hover.alt - HOVER_HEIGHT + 8.0));
        if self.hover.blocked {
            speed = 0.0;
        }
        if land.is_some_and(|p| flat(pos, [p[0], p[2]]) < LAND_M) {
            speed = 0.0;
        }
        axes.push((axis::THROTTLE, if pos[1] + 0.8 * vel[1] < alt { up } else { 0.0 }));
        // ---- the sticks: tilting it along `dir` at `speed`, as a helicopter
        let n = |x: f32, z: f32| {
            let l = x.hypot(z).max(1e-3);
            [x / l, z / l]
        };
        let (fwd, side) = (n(f[0], f[2]), n(r[0], r[2]));
        let err = [dir[0] * speed - vel[0], dir[1] * speed - vel[2]];
        let (e_fwd, e_side) = (err[0] * fwd[0] + err[1] * fwd[1], err[0] * side[0] + err[1] * side[1]);
        let pit_d = (-1.0 * e_fwd).clamp(-HOVER_TILT, HOVER_TILT);
        let bank_d = (1.0 * e_side).clamp(-HOVER_TILT, HOVER_TILT);
        axes.push((PITCH, PITCH_SIGN * (0.06 * (pit_d - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
        axes.push((BANK, (0.06 * (kind + bank_d) + 0.02 * drol).clamp(-1.0, 1.0)));
        // ---- the nose (the turn does not stop by itself: it is braked before arriving)
        let heading_d = angle_between(f, face);
        let rum = f[0].atan2(f[2]).to_degrees();
        let drum = self.previous_heading.map_or(0.0, |(t, q)| if v.time > t { wraps(rum - q) / (v.time - t) as f32 } else { 0.0 });
        self.previous_heading = Some((v.time, rum));
        axes.push((axis::YAW, (heading_d / 50.0 - drum / 40.0).clamp(-1.0, 1.0)));
        // landed (put down, or fallen onto something): it takes off again from there
        if pos[1] - self.hover.under < 4.0 && vel[0].hypot(vel[2]) < 3.0 && land.is_none() {
            (self.in_the_air, self.hover.on) = (false, false);
            self.takeoff = None;
        }
    }

    pub fn fly_heli(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, land: Option<[f32; 3]>) {
        let (pos, vel) = (i.pos, i.velocity);
        let (f, u, r) = (i.rows[2], i.rows[1], i.rows[0]);
        let pit = pitch(f);
        let kind = r[1].atan2(u[1]).to_degrees();
        let (dpit, drol) = match self.previous {
            Some((t, p, q)) if v.time > t => ((pit - p) / (v.time - t) as f32, wraps(kind - q) / (v.time - t) as f32),
            _ => (0.0, 0.0),
        };
        self.previous = Some((v.time, pit, kind));
        let below = Pilot::ground(v, pos[0], pos[2]);
        let h = pos[1] - below;
        // (an ordered landing, or waiting landed where it is)
        let land = land.or(match self.flight {
            Some(FlightOrder { hold: true, .. }) => Some(pos),
            Some(FlightOrder { to, end: FlightEnd::Land, .. }) => Some(to),
            _ => None,
        });
        let meta = match land {
            Some(p) => [p[0], p[2]],
            None => self.air_goal(v, rng, pos, f),
        };
        let d_goal = flat(pos, meta);
        let mut heading_d = angle_between(f, [meta[0] - pos[0], meta[1] - pos[2]]);
        let land_on = land.is_some() && d_goal < LAND_M;
        // what it would stand on, counting the level's objects (a dock, a deck, a platform): where it lands and what it takes off from
        let floor = if land_on || h < 60.0 { v.floor(pos[0], pos[2], pos[1]).unwrap_or(below) } else { below };
        // what is ahead rules over height, and so does a point to land on up on something (it comes in over it, not into it)
        let mut high = below;
        if let Some(p) = land.filter(|_| d_goal < 400.0) {
            high = high.max(p[1]);
        }
        for t in [1.0, 2.0, 4.0, 7.0, 10.0] {
            high = high.max(Pilot::ground(v, pos[0] + vel[0] * t, pos[2] + vel[2] * t));
        }
        for d in [40.0, 80.0] {
            high = high.max(Pilot::ground(v, pos[0] + f[0] * d, pos[2] + f[2] * d));
        }
        // what it sees and fires at: only what is in front of the nose
        shooter.see(v, rng, [pos[0], pos[1] - 1.0, pos[2]], AIR_RANGE.min(500.0), others);
        let mut halt = d_goal < if land.is_some() { 60.0 } else { 120.0 };
        let mut aim_nose: Option<f32> = None;
        if land_on {
            heading_d = 0.0;
        }
        if let Some((_, at, d)) = shooter.target.filter(|_| land.is_none()) {
            let a = [at[0] - pos[0], at[1] - pos[1], at[2] - pos[2]];
            let g_at = angle_between(f, [a[0], a[2]]);
            heading_d = g_at;
            halt = d < 200.0;
            let slope = a[1].atan2((a[0] * a[0] + a[2] * a[2]).sqrt()).to_degrees();
            if self.attack_run {
                // at firing distance it stops and points the nose (without dropping below the safety height: the collective controls that)
                halt = d < HELI_FIRE_M;
                aim_nose = Some(slope.clamp(-25.0, 5.0));
                let aligned = g_at.abs() < 8.0 && (slope - pit).abs() < 12.0 && d < HELI_FIRE_M + 100.0 && v.time >= shooter.ready_at;
                if aligned {
                    self.burst_until = v.time + 0.4;
                    let in_vehicle = others.iter().any(|o| shooter.target.is_some_and(|b| b.0 == o.player) && o.in_vehicle);
                    if in_vehicle && v.time >= self.missile_at {
                        axes.push((axis::ALT_FIRE, 1.0));
                        self.missile_at = v.time + 1.5;
                    }
                }
            } else if g_at.abs() < 6.0 && (slope - pit).abs() < 25.0 && d < SHOT_RANGE && v.time >= shooter.ready_at {
                self.burst_until = v.time + 0.3;
            }
        }
        if v.time < self.burst_until {
            axes.push((axis::FIRE, 1.0));
        }
        // ---- height
        let alt_d = if land_on { floor } else if self.attack_run { high + HELI_ATTACK_HEIGHT } else { high + HELI_HEIGHT };
        let vy_d = ((alt_d - pos[1]) * 0.35).clamp(-5.0, 12.0);
        // just taking off, straight up
        let taking_off = pos[1] - floor < 12.0 && !land_on;
        axes.push((axis::THROTTLE, ((vy_d - vel[1]) * 0.25).clamp(-1.0, 1.0)));
        // ---- actitud
        let va = vel[0].hypot(vel[2]);
        let pit_d = if land_on {
            // landing: brakes completely
            (va * 0.8).clamp(-4.0, 12.0)
        } else if taking_off || vel[1] < -6.0 || heading_d.abs() > 40.0 {
            0.0
        } else if let Some(m) = aim_nose.filter(|_| halt && va < 6.0) {
            // stopped facing the target: nose towards it
            m
        } else if halt {
            // brakes: nose up, according to how fast it runs
            (va * 0.5).clamp(0.0, 12.0)
        } else if va < HELI_SPEED {
            -HELI_NOSE
        } else {
            -2.0
        };
        let kind_d = if taking_off { 0.0 } else { -(heading_d * 0.35).clamp(-18.0, 18.0) };
        axes.push((PITCH, PITCH_SIGN * (0.04 * (pit_d - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
        axes.push((BANK, (0.03 * (kind - kind_d) + 0.02 * drol).clamp(-1.0, 1.0)));
        // (the turn does not stop by itself: it must be braked before arriving)
        let rum = f[0].atan2(f[2]).to_degrees();
        let drum = self.previous_heading.map_or(0.0, |(t, q)| if v.time > t { wraps(rum - q) / (v.time - t) as f32 } else { 0.0 });
        self.previous_heading = Some((v.time, rum));
        axes.push((axis::YAW, (heading_d / 50.0 - drum / 40.0).clamp(-1.0, 1.0)));
    }

    /// bots3: at the controls of an airship ([`crate::bot_api::VehicleClass::Airship`]): it hangs by itself and does not bank. The nose stays level, the
    /// yaw turns it, the throttle pushes it along (backwards too: that is its brake) and the pitch stick takes it up and down
    /// (`examples/air_envelope … cloudbus --step`). It goes to the ordered point or to `land`, else from flag to flag like a plane, at
    /// a height over what is ahead and over the point; it lands on what is below the point ([`BotView::floor`]: a platform). It does not
    /// fire: GC_Bespin's carries a squad.
    pub fn fly_airship(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, rng: &mut Rng, land: Option<[f32; 3]>) {
        let (pos, vel, f) = (i.pos, i.velocity, i.rows[2]);
        let below = Pilot::ground(v, pos[0], pos[2]);
        let land = land.or(match self.flight {
            Some(FlightOrder { hold: true, .. }) => Some(pos),
            Some(FlightOrder { to, end: FlightEnd::Land, .. }) => Some(to),
            _ => None,
        });
        // (a target its side reported is for what fires)
        let imposed = self.imposed_goal.take();
        let meta = match land {
            Some(p) => [p[0], p[2]],
            None => self.air_goal(v, rng, pos, f),
        };
        self.imposed_goal = imposed;
        let d_goal = flat(pos, meta);
        let land_on = land.is_some() && d_goal < AIRSHIP_LAND_M;
        let heading_d = if land_on { 0.0 } else { angle_between(f, [meta[0] - pos[0], meta[1] - pos[2]]) };
        let floor = if land_on || pos[1] - below < 60.0 { v.floor(pos[0], pos[2], pos[1]).unwrap_or(below) } else { below };
        // ---- height: over what is ahead and, near it, over the point it goes to
        let mut high = below;
        for t in [2.0, 5.0, 10.0] {
            high = high.max(Pilot::ground(v, pos[0] + vel[0] * t, pos[2] + vel[2] * t));
        }
        // (what floats over a city high up, with the ground far below, keeps to the height of where it goes, or else to its own)
        high = high.max(land.map_or(pos[1] - AIRSHIP_HEIGHT, |p| p[1]));
        let mut alt_d = if land_on { floor.max(land.map_or(floor, |p| p[1])) } else { high + AIRSHIP_HEIGHT };
        // ---- speed along it: slowing down near the point, and barely while it turns
        let along = vel[0] * f[0] + vel[1] * f[1] + vel[2] * f[2];
        let mut speed_d = if land_on {
            0.0
        } else if heading_d.abs() > AIRSHIP_TURN {
            3.0
        } else if land.is_some() {
            (d_goal * AIRSHIP_BRAKE).min(AIRSHIP_SPEED)
        } else {
            AIRSHIP_SPEED
        };
        // something in the way (a building, the city): it stops and climbs over it
        let flat_f = [f[0], 0.0, f[2]];
        let n = (flat_f[0] * flat_f[0] + flat_f[2] * flat_f[2]).sqrt().max(1e-3);
        let look = (along.max(0.0) * AIRSHIP_LOOK_S).max(AIRSHIP_LOOK_M);
        let at = |m: f32| [pos[0] + flat_f[0] / n * m, pos[1], pos[2] + flat_f[2] / n * m];
        if !land_on && v.hidden(at(AIRSHIP_HULL_M), at(AIRSHIP_HULL_M + look)) {
            speed_d = speed_d.min(2.0);
            alt_d = alt_d.max(pos[1] + 15.0);
        }
        axes.push((axis::THROTTLE, ((speed_d - along) * 0.2).clamp(-1.0, 1.0)));
        // the pitch stick, positive: down
        let vy_d = ((alt_d - pos[1]) * 0.3).clamp(if land_on { -3.0 } else { -6.0 }, 10.0);
        axes.push((PITCH, (-(vy_d - vel[1]) * 0.3).clamp(-1.0, 1.0)));
        // (the turn does not stop by itself: it must be braked before arriving)
        let rum = f[0].atan2(f[2]).to_degrees();
        let drum = self.previous_heading.map_or(0.0, |(t, q)| if v.time > t { wraps(rum - q) / (v.time - t) as f32 } else { 0.0 });
        self.previous_heading = Some((v.time, rum));
        axes.push((axis::YAW, (heading_d / 30.0 - drum / 20.0).clamp(-1.0, 1.0)));
    }
}
