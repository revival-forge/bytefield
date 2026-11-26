//! `BYTEFIELD_SPOT_TRACE=1`: what became of each firing spot a bot with `bots:cover-fight` chose ([`crate::bots::tactics`]), to
//! tell why the level's cover points lose (stuck on the way there, no line of fire from it, held too long, too many stops). Two
//! kinds of line on stderr, each `spot ` or `spotlife ` and a JSON object, for a tool that adds them up. It only reads the bot
//! (no draw of its random numbers, nothing it acts on): the round is the same with it or without it.
//!
//! - `spot`, when a spot is left: the round's seed, the side and player; when it was chosen (`t`), by which query, from which
//!   candidate (`cover`, `mesh` or `ring`), the cover point's sectors (`crouch`, `stand`) and `roof`; how far the spot and the
//!   enemy were then, whether it had an order to attack; the nearest it got (`min_d`), when it got within
//!   [`SPOT_ARRIVED_M`] (`arrive`), the seconds within the slack ([`COVER_SPOT_SLACK_M`], `at_s`) and pushing towards it without
//!   moving (`push_s`), rounds fired there and in all, a sample a second while there and with a target of the rays from where it
//!   really is (standing it sees the enemy, crouched it is hidden from it, its muzzle sees it now), and why it ended: `combat` it
//!   stopped fighting, `reach` or `hidden` its `still_good` failed, `no_target` [`NO_TARGET_S`] with nobody in sight,
//!   `vehicle`, `gone` the mind was dropped (a death or the round's end: the round's events say which).
//! - `spotlife`, per life: whether it has `bots:cover-fight`, seconds alive on foot, with a target, stopped to fight, walking to a
//!   spot (and of the time stopped, lying: `lying_s`; lying at all, on foot: `prone_s`); queries asked and spots found; and its time with a target in stretches by what it did (`S` stopped to fight, `P` on its
//!   way to its spot, `W` walking on, `O` something else, such as healing), with the rounds it fired in each kind.
//!
//! In a `spot` line, `walked` is the way it walked while not yet there, `toward` how well its steps pointed at the spot on its
//! way there (the cosine summed and the ticks, with the spot ahead of its view, to a side, behind), and `samples_in`/`hidden_in`
//! the samples within [`SPOT_ARRIVED_M`] of the spot and how many of them were hidden crouched.
use crate::bot_api::BotView;
use crate::bots::body::{BODY_OVER_FEET, flat};
use crate::bots::life::{COVER_SPOT_SLACK_M, SPOT_ARRIVED_M};
use crate::bots::tactics::{CROUCH_M, STAND_EYE_M, Spot};
use serde_json::json;

/// An episode ends after this long without a target, s (the bot walks on to its flag; the spot is kept, unused, until it fights).
pub const NO_TARGET_S: f64 = 3.0;
/// Slower than this, across (what it really moved), it is not moving, m/s.
const STILL_MPS: f32 = 0.5;

/// Whether the trace is on.
pub fn on() -> bool {
    crate::debug::options().bots_spot
}

fn r1(x: f32) -> f64 {
    (x as f64 * 10.0).round() / 10.0
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

struct Episode {
    spot: Spot,
    query: &'static str,
    resumed: bool,
    t0: f64,
    start_d: f32,
    enemy_d: f32,
    attacks: bool,
    min_d: f32,
    arrive: Option<f64>,
    at_s: f64,
    push_s: f64,
    shots_at: u32,
    shots: u32,
    samples: u32,
    sees_stand: u32,
    hidden_crouch: u32,
    muzzle_sees: u32,
    samples_in: u32,
    hidden_in: u32,
    walked: f32,
    last_pos: [f32; 3],
    /// on its way there and moving: how much its steps point at the spot (cosine), summed, and the ticks, with the spot ahead
    /// of where it looks, to a side or behind
    toward: [(f32, u32); 3],
    next_sample: f64,
    last_t: f64,
    last_d: f32,
    no_target_since: Option<f64>,
}

/// A bot's trace of its firing spots (one per life: the mind is dropped at death).
#[derive(Default)]
pub struct SpotTrace {
    seed: u64,
    team: u32,
    player: usize,
    /// it has `bots:cover-fight` (the side being measured; the rival's bots are traced for their time stopped)
    cover_fight: bool,
    open: Option<Episode>,
    /// why the bot dropped its spot this tick (said where it drops it)
    pub why: Option<&'static str>,
    /// the query that found the spot just chosen
    pub chosen: Option<&'static str>,
    pub asked: u32,
    pub found: u32,
    /// this tick, in a fight on foot: whether it walks, and whether towards its spot
    pub fight: Option<(bool, bool)>,
    weapon: Option<String>,
    rounds: i32,
    life_t0: Option<f64>,
    last_t: f64,
    alive_s: f64,
    target_s: f64,
    stopped_s: f64,
    /// of `stopped_s`, lying (`bots:prone-fight`)
    lying_s: f64,
    /// lying at all, on foot, whatever it does (`prone_s` well over `lying_s`: it does not get up to walk)
    prone_s: f64,
    to_spot_s: f64,
    episodes: u32,
    /// the stretches with a target: start, end, what it did
    stretches: Vec<(f64, f64, char)>,
    shots_by: [u32; 4],
}

const DOING: [char; 4] = ['S', 'P', 'W', 'O'];

impl SpotTrace {
    pub fn new(seed: u64, player: usize) -> SpotTrace {
        let mut t = SpotTrace::default();
        (t.seed, t.player) = (seed, player);
        t
    }

    /// After the bot's tick: whether it has `bots:cover-fight`, its spot, its target's body, whether it has an order to attack, and
    /// the axes it pressed (for `BYTEFIELD_BOTS_ME=<player>`: a `spotme` line every third tick of an episode of that player).
    pub fn tick(&mut self, v: &BotView, cover_fight: bool, spot: Option<Spot>, target: Option<[f32; 3]>, attacks: bool, axes: &[(usize, f32)]) {
        self.cover_fight = cover_fight;
        let me = &v.me;
        let t = v.time;
        if self.life_t0.is_none() {
            self.life_t0 = Some(t);
            self.last_t = t;
            self.team = me.team;
        }
        let dt = (t - self.last_t).max(0.0);
        self.last_t = t;
        // (the rounds that left the magazine of the same weapon: a reload or a change of weapon is no shot; nor what a medic's or
        // an engineer's tool spends healing or mending)
        let weapon = !matches!(crate::bots::body::weapons::class(me.weapon.as_deref().unwrap_or(""), me.single_shot), crate::bots::body::weapons::WeaponClass::Another);
        let shot = if weapon && self.weapon == me.weapon && me.rounds.0 < self.rounds { (self.rounds - me.rounds.0) as u32 } else { 0 };
        if self.weapon != me.weapon {
            self.weapon = me.weapon.clone();
        }
        self.rounds = me.rounds.0;
        let fight = self.fight.take();
        if me.inside.is_some() {
            self.close("vehicle");
            return;
        }
        if target.is_some() {
            let doing = match fight {
                Some((false, _)) => 0,
                Some((true, true)) => 1,
                Some((true, false)) => 2,
                None => 3,
            };
            self.shots_by[doing] += shot;
            match self.stretches.last_mut() {
                Some(x) if x.2 == DOING[doing] && x.1 >= t - dt - 1e-6 => x.1 = t,
                _ => self.stretches.push((t - dt, t, DOING[doing])),
            }
        }
        self.alive_s += dt;
        if me.pose == 2 {
            self.prone_s += dt;
        }
        if target.is_some() {
            self.target_s += dt;
        }
        if let Some((walks, to_spot)) = fight {
            if !walks {
                self.stopped_s += dt;
                if me.pose == 2 {
                    self.lying_s += dt;
                }
            }
            if to_spot {
                self.to_spot_s += dt;
            }
        }
        if self.open.as_ref().is_some_and(|e| spot.is_none_or(|s| s.pos != e.spot.pos)) {
            let why = self.why.take().unwrap_or("dropped");
            self.close(why);
        }
        self.why = None;
        if self.open.is_none()
            && let (Some(s), Some(e)) = (spot, target)
        {
            let chosen = self.chosen.take();
            self.episodes += 1;
            self.open = Some(Episode {
                spot: s,
                query: chosen.unwrap_or("resumed"),
                resumed: chosen.is_none(),
                t0: t,
                start_d: flat(me.pos, [s.pos[0], s.pos[2]]),
                enemy_d: flat(me.pos, [e[0], e[2]]),
                attacks,
                min_d: f32::MAX,
                arrive: None,
                at_s: 0.0,
                push_s: 0.0,
                shots_at: 0,
                shots: 0,
                samples: 0,
                sees_stand: 0,
                hidden_crouch: 0,
                muzzle_sees: 0,
                samples_in: 0,
                hidden_in: 0,
                walked: 0.0,
                last_pos: me.pos,
                toward: [(0.0, 0); 3],
                next_sample: t,
                last_t: t,
                last_d: f32::MAX,
                no_target_since: None,
            });
        }
        let Some(e) = self.open.as_mut() else { return };
        let s = e.spot;
        let d = flat(me.pos, [s.pos[0], s.pos[2]]);
        e.min_d = e.min_d.min(d);
        if d <= SPOT_ARRIVED_M && e.arrive.is_none() {
            e.arrive = Some(t);
        }
        let at = d <= SPOT_ARRIVED_M + COVER_SPOT_SLACK_M;
        // (what it really moved since the last tick: `velocity` is what it asks of its legs, and says 2.5 m/s against a wall)
        let step = [me.pos[0] - e.last_pos[0], me.pos[2] - e.last_pos[2]];
        let moved = step[0].hypot(step[1]);
        if e.arrive.is_none() {
            e.walked += moved;
        }
        e.last_pos = me.pos;
        e.shots += shot;
        if at {
            e.at_s += dt;
            e.shots_at += shot;
        }
        let speed = if dt > 0.0 { moved / dt as f32 } else { STILL_MPS };
        if fight.is_some_and(|f| f.1) && speed < STILL_MPS {
            e.push_s += dt;
        }
        if fight.is_some_and(|f| f.1) && speed >= STILL_MPS && d > SPOT_ARRIVED_M {
            let to = [s.pos[0] - me.pos[0], s.pos[2] - me.pos[2]];
            let c = (step[0] * to[0] + step[1] * to[1]) / (moved * d.max(1e-3));
            let a = crate::bots::body::angle_between(me.view[2], to).abs();
            let k = if a < 45.0 { 0 } else if a < 135.0 { 1 } else { 2 };
            e.toward[k].0 += c;
            e.toward[k].1 += 1;
        }
        if at
            && let Some(x) = target
            && t >= e.next_sample
        {
            e.next_sample = t + 1.0;
            e.samples += 1;
            // (the rays of `tactics::dear` for `SeesStanding` and `HiddenCrouch`, from where it is rather than from the spot)
            let floor = me.pos[1] - BODY_OVER_FEET;
            e.sees_stand += !v.hidden([me.pos[0], floor + STAND_EYE_M, me.pos[2]], [x[0], x[1] + 0.3, x[2]]) as u32;
            let hidden = v.hidden([x[0], x[1] + 0.6, x[2]], [me.pos[0], floor + CROUCH_M, me.pos[2]]);
            e.hidden_crouch += hidden as u32;
            if d <= SPOT_ARRIVED_M {
                e.samples_in += 1;
                e.hidden_in += hidden as u32;
            }
            e.muzzle_sees += !v.hidden(me.muzzle, [x[0], x[1] + 0.3, x[2]]) as u32;
        }
        if crate::debug::options().bots_me == Some(self.player) && (t * 30.0).round() as i64 % 3 == 0 {
            let to = [s.pos[0] - me.pos[0], s.pos[2] - me.pos[2]];
            let a = crate::bots::body::angle_between(me.view[2], to);
            let ax = |k: usize| axes.iter().rev().find(|x| x.0 == k).map_or(0.0, |x| x.1);
            use crate::soldier::axis;
            eprintln!(
                "spotme seed={} team={} p={} t={t:.2} pos=({:.1},{:.1},{:.1}) spot=({:.1},{:.1},{:.1}) d={d:.2} angle={a:.0} vel=({:.1},{:.1}) fight={fight:?} target={:?} thr={} strafe={} crouch={} fire={}",
                self.seed, self.team, self.player, me.pos[0], me.pos[1], me.pos[2], s.pos[0], s.pos[1], s.pos[2], me.velocity[0], me.velocity[2], target.map(|x| x.map(|c| (c * 10.0).round() / 10.0)),
                ax(axis::THROTTLE), ax(axis::YAW), ax(axis::CROUCH), ax(axis::FIRE)
            );
        }
        e.last_t = t;
        e.last_d = d;
        if target.is_some() {
            e.no_target_since = None;
        } else if t - *e.no_target_since.get_or_insert(t) >= NO_TARGET_S {
            self.close("no_target");
        }
    }

    fn close(&mut self, why: &str) {
        let Some(e) = self.open.take() else { return };
        let s = e.spot;
        let cover = s.cover;
        let line = json!({
            "seed": self.seed, "team": self.team, "player": self.player, "t": r2(e.t0), "end": r2(e.last_t), "query": e.query, "resumed": e.resumed,
            "from": s.from, "crouch": cover.map(|c| c.crouch), "stand": cover.map(|c| c.stand), "roof": cover.map(|c| c.roof),
            "pos": [r1(s.pos[0]), r1(s.pos[1]), r1(s.pos[2])], "rays": s.rays, "start_d": r1(e.start_d), "enemy_d": r1(e.enemy_d), "attacks": e.attacks,
            "min_d": r1(e.min_d), "last_d": r1(e.last_d), "arrive": e.arrive.map(|a| r2(a - e.t0)), "at_s": r2(e.at_s), "push_s": r2(e.push_s),
            "shots_at": e.shots_at, "shots": e.shots, "samples": e.samples, "sees_stand": e.sees_stand, "hidden_crouch": e.hidden_crouch, "muzzle_sees": e.muzzle_sees,
            "samples_in": e.samples_in, "hidden_in": e.hidden_in, "walked": r1(e.walked), "toward": e.toward.map(|x| [r2(x.0 as f64), x.1 as f64]), "why": why,
        });
        eprintln!("spot {line}");
    }
}

impl Drop for SpotTrace {
    fn drop(&mut self) {
        self.close("gone");
        let Some(t0) = self.life_t0 else { return };
        let line = json!({
            "seed": self.seed, "team": self.team, "player": self.player, "cover_fight": self.cover_fight, "t": r2(t0), "end": r2(self.last_t), "alive_s": r2(self.alive_s), "target_s": r2(self.target_s),
            "stopped_s": r2(self.stopped_s), "lying_s": r2(self.lying_s), "prone_s": r2(self.prone_s), "to_spot_s": r2(self.to_spot_s), "asked": self.asked, "found": self.found, "episodes": self.episodes,
            "shots_by": DOING.iter().zip(self.shots_by).map(|(c, n)| (c.to_string(), n)).collect::<std::collections::BTreeMap<_, _>>(),
            "stretches": self.stretches.iter().map(|x| json!([r2(x.0), r2(x.1), x.2.to_string()])).collect::<Vec<_>>(),
        });
        eprintln!("spotlife {line}");
    }
}
