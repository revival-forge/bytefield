//! `BYTEFIELD_FIRE_TRACE=1`: why a bot on foot does not squeeze the trigger with an enemy in reach, to tell where its chances to
//! shoot go (the rhythm of bursts and pauses, the line of fire, aiming, the reaction, walking instead of firing, the choice of
//! target). Lines on stderr, each `fire `, `firelife ` or `fireme ` and a JSON object or key=value pairs, for a tool that adds
//! them up. It only reads the bot (no draw of its random numbers, nothing it acts on): the round is the same with it or without it.
//!
//! A tick counts while the bot is on foot and alive and either some enemy in reach is in sight (its chest seen from the eye, as the
//! bot's own look tests it, but for every enemy in reach rather than the nearest three) or the bot has a target. Each such tick is
//! `fire` (the fire key down) or has one reason, the first gate that held it: from the body's tick, `no_target_look` (an enemy in
//! sight that its look, five times a second, has not taken yet), `no_target_rank` (enemies in sight, none of them among the three
//! nearest in reach, the only ones its look tries), or what it was doing instead (`trade`, `sapper`, `break_contact`, `vehicle`, `unjam`, `walk`,
//! `keeping`, `suppressing`...); from the shooter ([`super::body::shooter::Shooter::shoot_on_foot`]), `reaction`, `aim`, `friend`
//! (a mate within 1.5 m of the line), `wall_eye`, `wall_gun` or `wall_ahead` (which of the three rays was blocked), `changing`
//! (to the weapon the target asks for), `grenade`, `requesting`, `empty`, `pause` (between bursts, or the bolt's interval),
//! `cycle` (the bolt's tick between shots) or `no_class`.
//!
//! - `fire`, per contact (a stretch with an enemy in sight or a target, ended by [`GAP_S`] without either): the mind's name, seed,
//!   side, player; when (`t`, `end`); the most enemies in sight at once (`most`), the nearest at the start (`d0`) and whether it
//!   faced the bot then (`faced0`, within [`FACING_DEG`]); the seconds from the start to its first target, first aimed tick, first
//!   fire key and first round out (`to_target`, `to_aimed`, `to_fire`, `to_shot`); how far it had to turn to its first target
//!   (`turn0`, degrees); rounds fired; seconds by reason (`why`, with `fire`); seconds walking while fighting (`moving_s`) and
//!   firing while walking (`moving_fire_s`); seconds with an enemy in sight and no mate in its line (`clear_s`) and of those not
//!   firing (`clear_idle_s`); seconds with a target that was not the enemy in sight nearest its crosshair, by more than
//!   [`EASIER_DEG`] (`not_easiest_s`); seconds its line was blocked (a mate or a wall) with another enemy in sight and clear
//!   (`blocked_alt_s`); target changes; the weapon at the start.
//! - `firelife`, per life: the same seconds added over its contacts, with its time on foot.
//! - `fireme`, every tick of a contact of the player `BYTEFIELD_BOTS_ME` names: enemies in sight and clear, its target and how far,
//!   the reason, the aim's error, the fire key, its pose and weapon; and the target's vehicle and seat (`in`, `seat`,
//!   `exposed` if that seat shows him and a round can hit him there), and of the enemies in sight in a vehicle, `riders=player:seat`
//!   (`*`: exposed).
//! - `no_target_fov` (with `bots:fov`): enemies in sight, none in its field of view; per life, `lost` and `found`, targets lost from
//!   sight that `bots:recall` watched for and of them taken again, and `recall_s`, the seconds it watched.
//! - Per life, with nobody about (no enemy in sight, no target) on foot: `quiet_s`; `still`, seconds slower than [`STILL_MPS`] by
//!   `action/branch/pushing|idle` (the individual's action, the body's branch, and whether it pressed a walking key; idle, why its
//!   feet stand: `:rest` a rest at a point, `:queued` its path asked and waiting its turn, `:nopath` none, `:turn` turning on the
//!   spot to a step more than 60° off); `stops`,
//!   the walk-stops ([`STOP_S`] still after [`WALKED_S`] walking) by the key they began with ([`FireTrace::after`]).
//! - With each sum, `armour_s` and `armour_shots`: its target inside a vehicle its weapon in hand does not hurt (a rifle at a
//!   tank's driver), and the rounds it fired at him; `grenades`, the grenades it threw.
use crate::bot_api::BotView;
use crate::bots::body::shooter::{Shooter, friend_in_line, in_fov};
use crate::bots::body::{CHEST, angle_between, dist3};
use crate::soldier::axis;
use serde_json::json;
use std::collections::BTreeMap;

/// A contact ends after this long with no enemy in sight and no target, s.
pub const GAP_S: f64 = 1.0;
/// An enemy faces the bot when it looks within this of it, degrees.
pub const FACING_DEG: f32 = 30.0;
/// Its target is not the easiest when another in sight is nearer its crosshair by more than this, degrees.
pub const EASIER_DEG: f32 = 10.0;

/// A pistol in hand against a target farther than this counts as far, m (`bots:pistol-close`'s reach).
pub const PISTOL_FAR_M: f32 = crate::bots::body::shooter::PISTOL_NEAR_M;

/// Whether the trace is on.
pub fn on() -> bool {
    crate::debug::options().bots_fire
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[derive(Default)]
struct Sums {
    s: f64,
    clear_s: f64,
    clear_idle_s: f64,
    moving_s: f64,
    moving_fire_s: f64,
    not_easiest_s: f64,
    blocked_alt_s: f64,
    /// the pistol in hand; and with it, the main gun with rounds (not an anti-tank launcher), or its target farther than [`PISTOL_FAR_M`]
    pistol_s: f64,
    pistol_loaded_s: f64,
    pistol_far_s: f64,
    /// its target riding inside a vehicle (not on a seat that shows him) that the weapon in hand does not hurt
    /// ([`crate::bots::body::shooter::ARMOUR_HURTS`]), and the rounds it fired at him so
    armour_s: f64,
    armour_shots: u32,
    /// grenades thrown (rounds out with a grenade in hand)
    grenades: u32,
    shots: u32,
    why: BTreeMap<&'static str, f64>,
}

impl Sums {
    fn add(&mut self, o: &Sums) {
        self.s += o.s;
        self.clear_s += o.clear_s;
        self.clear_idle_s += o.clear_idle_s;
        self.moving_s += o.moving_s;
        self.moving_fire_s += o.moving_fire_s;
        self.not_easiest_s += o.not_easiest_s;
        self.blocked_alt_s += o.blocked_alt_s;
        self.pistol_s += o.pistol_s;
        self.pistol_loaded_s += o.pistol_loaded_s;
        self.pistol_far_s += o.pistol_far_s;
        self.armour_s += o.armour_s;
        self.armour_shots += o.armour_shots;
        self.grenades += o.grenades;
        self.shots += o.shots;
        for (k, x) in &o.why {
            *self.why.entry(k).or_default() += x;
        }
    }

    fn json(&self) -> serde_json::Value {
        json!({
            "s": r2(self.s), "clear_s": r2(self.clear_s), "clear_idle_s": r2(self.clear_idle_s), "moving_s": r2(self.moving_s),
            "moving_fire_s": r2(self.moving_fire_s), "not_easiest_s": r2(self.not_easiest_s), "blocked_alt_s": r2(self.blocked_alt_s), "shots": self.shots,
            "pistol_s": r2(self.pistol_s), "pistol_loaded_s": r2(self.pistol_loaded_s), "pistol_far_s": r2(self.pistol_far_s),
            "armour_s": r2(self.armour_s), "armour_shots": self.armour_shots, "grenades": self.grenades,
            "why": self.why.iter().map(|(k, x)| (k.to_string(), r2(*x))).collect::<BTreeMap<_, _>>(),
        })
    }
}

struct Contact {
    t0: f64,
    last: f64,
    most: usize,
    d0: Option<f32>,
    faced0: Option<bool>,
    to_target: Option<f64>,
    to_aimed: Option<f64>,
    to_fire: Option<f64>,
    to_shot: Option<f64>,
    turn0: Option<f32>,
    switches: u32,
    target: Option<usize>,
    weapon: Option<String>,
    sums: Sums,
}

/// A bot's fire trace (one per life: the mind is dropped at death).
#[derive(Default)]
pub struct FireTrace {
    seed: u64,
    player: usize,
    team: u32,
    label: &'static str,
    open: Option<Contact>,
    life: Sums,
    contacts: u32,
    life_t0: Option<f64>,
    last_t: f64,
    foot_s: f64,
    /// its cursor in the round's log of shots ([`BotView::my_shots_since`])
    fired_seen: usize,
    /// `bots:recall`: targets lost from sight and watched for, and of them taken again ([`Shooter::recalls`]); seconds watching
    recalls: [u32; 2],
    recall_s: f64,
    /// stops with nobody about ([`FireTrace::after`]): whether this tick had no enemy in sight and no target, and the body's branch
    quiet: bool,
    branch: &'static str,
    quiet_s: f64,
    /// seconds still by `action/branch/pushing|idle`, and stops (still [`STOP_S`] after walking [`WALKED_S`]) by the same key
    still: BTreeMap<String, f64>,
    stops: BTreeMap<String, u32>,
    walked: f64,
    still_since: Option<(f64, String, f64)>,
}

/// A stop with nobody about lasts this long, s (longer is waiting, not the walk-stop of a corridor), after walking this long.
pub const STOP_S: std::ops::Range<f64> = 0.3..3.0;
pub const WALKED_S: f64 = 0.5;
/// Slower than this on foot is still, m/s.
pub const STILL_MPS: f32 = 0.5;

impl FireTrace {
    pub fn new(seed: u64, player: usize) -> FireTrace {
        let mut t = FireTrace::default();
        (t.seed, t.player) = (seed, player);
        t.fired_seen = usize::MAX;
        t
    }

    /// After the bot's tick: its mind's name, its shooter, what its tick on foot did and its reach on foot, and the axes it pressed.
    pub fn tick(&mut self, v: &BotView, label: &'static str, sh: &Shooter, branch: &'static str, reach: f32, axes: &[(usize, f32)]) {
        let me = &v.me;
        let t = v.time;
        if self.life_t0.is_none() {
            self.life_t0 = Some(t);
            self.last_t = t;
            self.team = me.team;
            // (a cursor past the log's end reads it all and moves to its end: what an earlier life fired is not counted)
            v.my_shots_since(&mut self.fired_seen);
        }
        self.label = label;
        let dt = (t - self.last_t).max(0.0);
        self.last_t = t;
        // (the shots the simulation made for its soldier, not what left the magazine: a bolt-action's drops seconds after the
        // shot; a medic's or an engineer's pack makes none; nor a knife, binoculars or a mine)
        let weapon = !matches!(crate::bots::body::weapons::class(me.weapon.as_deref().unwrap_or(""), me.single_shot), crate::bots::body::weapons::WeaponClass::Another);
        let fired = v.my_shots_since(&mut self.fired_seen);
        let shot = if weapon { fired } else { 0 };
        if me.inside.is_some() {
            self.close();
            self.quiet = false;
            return;
        }
        self.foot_s += dt;
        self.recalls = sh.recalls;
        if branch == "recall" {
            self.recall_s += dt;
        }
        let others = v.others();
        let mates: Vec<[f32; 3]> = others.iter().filter(|o| o.team == me.team && o.player != me.player).map(|o| o.pos).collect();
        // the enemies in reach it sees: (player, chest, distance, angle off its crosshair, clear of mates)
        let seen: Vec<(usize, [f32; 3], f32, f32, bool)> = others
            .iter()
            .filter(|o| o.team != me.team)
            .filter_map(|o| {
                let at = [o.pos[0], o.pos[1] + CHEST, o.pos[2]];
                let d = dist3(me.eye, at);
                (d < reach && !v.hidden(me.eye, at)).then(|| {
                    let off = off_crosshair(me.view[2], me.eye, at);
                    (o.player, at, d, off, !friend_in_line(me.eye, at, &mates))
                })
            })
            .collect();
        let target = sh.target;
        self.quiet = seen.is_empty() && target.is_none();
        self.branch = branch;
        if seen.is_empty() && target.is_none() {
            if self.open.as_ref().is_some_and(|c| t - c.last >= GAP_S) {
                self.close();
            }
            return;
        }
        let fire = weapon && axes.iter().any(|e| e.0 == axis::FIRE && e.1 > 0.0);
        let fighting = branch.starts_with("fight");
        let why: &'static str = if fire {
            "fire"
        } else if target.is_none() {
            // (its look tries only the three nearest in reach, seen or not: one in sight further down that list is never taken)
            let mut near: Vec<(f32, usize)> = others
                .iter()
                .filter(|o| o.team != me.team)
                .map(|o| (dist3(me.eye, [o.pos[0], o.pos[1] + CHEST, o.pos[2]]), o.player))
                .filter(|x| x.0 < reach)
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            // (`bots:fov`: those in sight are all out of its field of view)
            if sh.fps.fov && !seen.iter().any(|x| in_fov(me.view[2], me.eye, x.1, x.2)) {
                "no_target_fov"
            } else if near.iter().take(3).any(|x| seen.iter().any(|y| y.0 == x.1)) {
                "no_target_look"
            } else {
                "no_target_rank"
            }
        } else if !fighting {
            branch
        } else {
            sh.why
        };
        let moving = matches!(branch, "fight_walk" | "fight_to_spot");
        let aimed = target.is_some() && fighting && sh.aim_off[0].abs() < 3.0 && sh.aim_off[1].abs() < 4.0;
        let c = self.open.get_or_insert_with(|| {
            let nearest = seen.iter().min_by(|a, b| a.2.total_cmp(&b.2));
            Contact {
                t0: t,
                last: t,
                most: 0,
                d0: nearest.map(|x| x.2),
                faced0: nearest.map(|x| faces(v, x.0, x.1, me.eye)),
                to_target: None,
                to_aimed: None,
                to_fire: None,
                to_shot: None,
                turn0: None,
                switches: 0,
                target: None,
                weapon: me.weapon.clone(),
                sums: Sums::default(),
            }
        });
        if c.d0.is_none()
            && let Some(n) = seen.iter().min_by(|a, b| a.2.total_cmp(&b.2))
        {
            (c.d0, c.faced0) = (Some(n.2), Some(faces(v, n.0, n.1, me.eye)));
        }
        c.last = t;
        c.most = c.most.max(seen.len());
        let since = t - c.t0;
        if let Some(b) = target {
            if c.to_target.is_none() {
                c.to_target = Some(since);
                c.turn0 = Some(off_crosshair(me.view[2], me.eye, b.1));
            }
            if c.target.is_some_and(|p| p != b.0) {
                c.switches += 1;
            }
            c.target = Some(b.0);
        }
        if aimed && c.to_aimed.is_none() {
            c.to_aimed = Some(since);
        }
        if fire && c.to_fire.is_none() {
            c.to_fire = Some(since);
        }
        if shot > 0 && c.to_shot.is_none() {
            c.to_shot = Some(since);
        }
        let clear = seen.iter().any(|x| x.4);
        let s = &mut c.sums;
        s.s += dt;
        s.shots += shot;
        *s.why.entry(why).or_default() += dt;
        if clear {
            s.clear_s += dt;
            if !fire {
                s.clear_idle_s += dt;
            }
        }
        if me.weapon.as_deref().is_some_and(crate::bots::body::weapons::is_pistol) {
            s.pistol_s += dt;
            // (an anti-tank kit's main gun is its launcher: the pistol is its gun against soldiers, not counted)
            let main = me.items.iter().position(|i| i.0 == crate::bots::body::weapons::PRIMARY_SLOT && !crate::bots::body::weapons::is_anti_tank(&i.1)).and_then(|k| me.ammo.get(k));
            if main.is_some_and(|a| a.0 + a.1 > 0) {
                s.pistol_loaded_s += dt;
            }
            if target.is_some_and(|b| b.2 > PISTOL_FAR_M) {
                s.pistol_far_s += dt;
            }
        }
        if let Some(b) = target
            && let Some((j, false)) = v.trace_ride(b.0)
            && let Some(w) = me.weapon.as_deref()
            && v.round_share(w, j).is_some_and(|x| x[0].max(x[2]) < crate::bots::body::shooter::ARMOUR_HURTS)
        {
            s.armour_s += dt;
            s.armour_shots += shot;
        }
        if matches!(crate::bots::body::weapons::class(me.weapon.as_deref().unwrap_or(""), me.single_shot), crate::bots::body::weapons::WeaponClass::Granada) {
            s.grenades += fired;
        }
        if moving {
            s.moving_s += dt;
            if fire {
                s.moving_fire_s += dt;
            }
        }
        if matches!(why, "friend" | "wall_eye" | "wall_gun" | "wall_ahead")
            && let Some(b) = target
            && seen.iter().any(|x| x.0 != b.0 && x.4)
        {
            s.blocked_alt_s += dt;
        }
        if let Some(b) = target
            && let Some(easiest) = seen.iter().min_by(|a, b| a.3.total_cmp(&b.3))
            && easiest.0 != b.0
            && off_crosshair(me.view[2], me.eye, b.1) - easiest.3 > EASIER_DEG
        {
            s.not_easiest_s += dt;
        }
        if crate::debug::options().bots_me == Some(self.player) {
            let d = target.map_or(-1.0, |b| b.2);
            // the target's vehicle and seat, and `exposed` if that seat shows him (a round can hit him there)
            let seat = target.and_then(|b| v.trace_seat(b.0)).map_or_else(String::new, |(car, seat, shown)| format!(" in={car} seat={seat}{}", if shown { " exposed" } else { "" }));
            // and of those in sight in a vehicle, `player:seat`, with `*` if exposed
            let riders: Vec<String> = seen.iter().filter_map(|x| v.trace_seat(x.0).map(|(_, seat, shown)| format!("{}:{seat}{}", x.0, if shown { "*" } else { "" }))).collect();
            let seat = if riders.is_empty() { seat } else { format!("{seat} riders={}", riders.join(",")) };
            eprintln!(
                "fireme seed={} team={} p={} t={t:.2} seen={} clear={} target={:?} d={d:.1} why={why} aim=({:.1},{:.1}) fire={} branch={branch} pose={} weapon={:?} rounds={}{seat}",
                self.seed, self.team, self.player, seen.len(), seen.iter().filter(|x| x.4).count(), target.map(|b| b.0), sh.aim_off[0], sh.aim_off[1], fire as u8, me.pose, me.weapon, me.rounds.0
            );
        }
    }

    /// After the individual's tick (its action, and the axes as it leaves them): on foot with nobody about, whether it stands still
    /// and why (`action/branch`, and whether it pushes a walking key: `pushing` is held by something, `idle` stood of its own),
    /// and the stops of a walk-stop ([`STOP_S`] still after [`WALKED_S`] walking). With `BYTEFIELD_BOTS_ME`, a `stillme` line
    /// each such tick with `detail`.
    pub fn after(&mut self, v: &BotView, action: &str, axes: &[(usize, f32)], walker: &str, detail: impl FnOnce() -> String) {
        let me = &v.me;
        if me.inside.is_some() || !self.quiet || self.life_t0.is_none() {
            self.still_since = None;
            self.walked = 0.0;
            return;
        }
        let t = v.time;
        let dt = v.dt as f64;
        self.quiet_s += dt;
        let speed = me.velocity[0].hypot(me.velocity[2]);
        let pushing = axes.iter().any(|e| matches!(e.0, axis::THROTTLE | axis::YAW) && e.1 != 0.0);
        let key = if pushing { format!("{action}/{}/pushing", self.branch) } else { format!("{action}/{}/idle{walker}", self.branch) };
        if crate::debug::options().bots_me == Some(self.player) {
            eprintln!("stillme seed={} p={} t={t:.2} speed={speed:.1} key={key} {} near={}", self.seed, self.player, detail(), v.trace_bodies_near(self.player, 2.5));
        }
        if speed < STILL_MPS {
            *self.still.entry(key.clone()).or_default() += dt;
            if self.still_since.is_none() {
                self.still_since = Some((t, key, self.walked));
                self.walked = 0.0;
            }
        } else {
            if let Some((t0, k, walked)) = self.still_since.take()
                && walked >= WALKED_S
                && STOP_S.contains(&(t - t0))
            {
                *self.stops.entry(k).or_default() += 1;
            }
            self.walked += dt;
        }
    }

    fn close(&mut self) {
        let Some(c) = self.open.take() else { return };
        self.contacts += 1;
        self.life.add(&c.sums);
        let o = |x: Option<f64>| x.map(r2);
        let mut line = json!({
            "mind": self.label, "seed": self.seed, "team": self.team, "player": self.player, "t": r2(c.t0), "end": r2(c.last), "most": c.most,
            "d0": c.d0.map(|x| r2(x as f64)), "faced0": c.faced0, "to_target": o(c.to_target), "to_aimed": o(c.to_aimed), "to_fire": o(c.to_fire),
            "to_shot": o(c.to_shot), "turn0": c.turn0.map(|x| r2(x as f64)), "switches": c.switches, "weapon": c.weapon,
        });
        merge(&mut line, c.sums.json());
        eprintln!("fire {line}");
    }
}

impl Drop for FireTrace {
    fn drop(&mut self) {
        self.close();
        let Some(t0) = self.life_t0 else { return };
        let mut line = json!({
            "mind": self.label, "seed": self.seed, "team": self.team, "player": self.player, "t": r2(t0), "end": r2(self.last_t),
            "foot_s": r2(self.foot_s), "contacts": self.contacts, "lost": self.recalls[0], "found": self.recalls[1], "recall_s": r2(self.recall_s),
            "quiet_s": r2(self.quiet_s), "still": self.still.iter().map(|(k, x)| (k.clone(), r2(*x))).collect::<BTreeMap<_, _>>(), "stops": self.stops,
        });
        merge(&mut line, self.life.json());
        eprintln!("firelife {line}");
    }
}

fn merge(a: &mut serde_json::Value, b: serde_json::Value) {
    if let (Some(a), serde_json::Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
}

/// How far `at` is off the crosshair of a view looking along `f` from `eye`, degrees (yaw and pitch together).
fn off_crosshair(f: [f32; 3], eye: [f32; 3], at: [f32; 3]) -> f32 {
    let a = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-3);
    let fl = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt().max(1e-3);
    ((f[0] * a[0] + f[1] * a[1] + f[2] * a[2]) / (l * fl)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Whether the enemy `player` (its chest at `at`) looks towards `eye`, within [`FACING_DEG`].
fn faces(v: &BotView, player: usize, at: [f32; 3], eye: [f32; 3]) -> bool {
    v.facing(player).is_some_and(|f| angle_between([f[0], 0.0, f[1]], [eye[0] - at[0], eye[2] - at[2]]).abs() < FACING_DEG)
}
