//! Aiming and firing: what a player does with the mouse and the trigger. Sees the nearest enemy not hidden by a wall, takes time to
//! react, aims with an aiming error, fires at its weapon's rhythm (bursts for the automatic, one shot and a pause for the
//! bolt-action), reloads, switches to the pistol when out of ammunition, throws a grenade now and then and does not fire if a mate is on
//! the line of fire. It has its own state; the random generator, which belongs to the bot, is lent to it on each call.
use super::weapons::{self, WeaponClass};
use super::rng::Rng;
use super::{TURN, CHEST, pitch, dist3, angle_between};
use crate::bot_api::{BotView, Other};
use crate::soldier::axis;

/// Overall adjustments, measured against the Berlin-8 targets.
pub const PAUSE_SCALE: f32 = 1.0;
/// The pause between bursts from a vehicle, over the one on foot (row 4: 29 % of the shots are from vehicles).
pub const VEHICLE_SCALE: f32 = 1.0;
pub const MEDIAN_REACTION: f32 = 0.6;
/// Seconds that fire lasts without pauses for whoever has just been shot.
pub const HARASSED: f64 = 3.0;
/// The pause between bursts for whoever is being shot at, over the normal one.
pub const HARASSED_SCALE: f32 = 0.3;
/// Aiming error of each burst, degrees (standard deviation).
pub const AIM_ERROR: f32 = 0.55;
/// Probability of throwing a grenade, when starting a burst with the enemy between 10 and 28 m.
pub const P_GRENADE: f32 = 0.06;
/// Probability of using the pistol when starting a fight even with ammunition.
pub const P_PISTOL: f32 = 0.05;
/// Assumed speed of a ship's shell, m/s.
pub const SHELL_SPEED: f32 = 250.0;
/// How long its line of fire may stay busy (a mate or a wall in it) before it stops standing to clear a flag, s.
pub const LINE_BUSY_S: f64 = 3.0;

/// The FPS fundamentals of firing on foot, each its variant (`bots:fire-rhythm`, `friend-gate`, `look-all`, `quick-react`,
/// `fire-first`; `pistol-close` since 1.21.0; `armour`, `close-fight` and `grenade-throw` since 1.22.0; `heard-walk` since 1.23.0; `fov` and `recall` since 1.24.0), all on by default since 1.20.0 (off together with `bots:no-fps`, or one by one with `no-fire-rhythm`...); all off
/// is the bot of 1.19.0, which keeps the slower rhythm it had before.
/// The fire trace said where the chances to shoot go with a clear enemy in sight ([`crate::bots::fire_trace`]): the pause between
/// bursts (46 % of the time in contact), a mate in the line (14 %), a look that tries only the three nearest (9 %), the
/// reaction and aiming (5 % each).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FpsTweaks {
    /// `fire-rhythm`: a person's rhythm, not the slow default: longer bursts and shorter pauses the closer the enemy
    /// ([`weapons::burst_near`], [`weapons::pause_near`]), and a bolt-action, a shotgun or a pistol at its own cycle
    /// ([`weapons::interval_near`]) instead of the default ≥ 2.9 s
    pub rhythm: bool,
    /// `friend-gate`: a mate blocks the line only within [`FRIEND_BODY_M`] (and the spread with distance) of it, short of the
    /// target, not anywhere within 1.5 m; blocked by one a while ([`STEP_ASIDE_S`]), it steps aside
    pub friend: bool,
    /// `look-all`: its look tries up to [`LOOK_ALL`] of the nearest in reach, not three, and prefers one with no mate in the line
    pub look: bool,
    /// `quick-react`: a reaction without the long tail ([`REACT_SIGMA`], at most [`REACT_MAX_S`]; half as long for an enemy
    /// already near its crosshair), and no grenade with a mate in the line or closer than [`GRENADE_MIN_M`] (and back to the gun
    /// if one steps in while it holds it)
    pub react: bool,
    /// `fire-first`: walking with a target aimed and its line clear, it stops for the burst and walks between bursts
    pub stand: bool,
    /// `pistol-close`: the pistol only with the main gun dry (magazine and the rest) and the enemy within [`PISTOL_NEAR_M`], or to
    /// finish one within [`PISTOL_FINISH_M`] with the magazine empty (quicker than reloading); never by the default's coin
    /// ([`P_PISTOL`]); it does not reload the pistol while the main gun has rounds, and goes back to it at once
    pub pistol: bool,
    /// `armour`: on foot, an enemy riding inside a vehicle that nothing it carries hurts (a rifle against a tank: the damage
    /// table of the `.con`, [`BotView::round_share`]) is no target, unless it carries grenades and he is within a throw (then
    /// only grenades); one on a seat that shows him is, at his body ([`Shooter::armour_view`])
    pub armour: bool,
    /// `close-fight`: with a target within [`CLOSE_FIGHT_M`] and its line not blocked for long ([`Shooter::line_long_busy`]),
    /// it fights where it is: no flight to get unstuck ([`super::walker::Walker::flee`], which drops the fire and runs, jumping,
    /// even past or over the enemy) and no count of not getting anywhere (the jump at 1.5 s)
    pub close: bool,
    /// `grenade-throw`: after holding the fire key with a grenade, it lets go for [`GRENADE_LET_GO_S`] before going back to the
    /// gun (a grenade leaves on letting go, at the next update: switching weapon on that tick took it back unthrown)
    pub throw: bool,
    /// `fov`: on foot it takes as a target only an enemy within its field of view ([`Shooter::fov_view`]: a cone
    /// [`FOV_NEAR_DEG`] each side of where it looks near, narrowing to [`FOV_FAR_DEG`] far, as a figure far off is noticed only
    /// near the centre of the screen), one within [`FOV_TOUCH_M`] (heard, brushed against) or the one it already follows; a shot
    /// heard or a hit still turns it ([`crate::bots::individual`]'s reaction)
    pub fov: bool,
    /// `recall`: a target lost from sight (behind a wall, round a corner, past its side) is watched for [`RECALL_S`] where it
    /// was last seen, led by its last motion ([`Shooter::recall`]), while it walks on; if it shows again there it is taken at once
    pub recall: bool,
    /// `heard-walk`: turned to a shot heard with nobody in sight, it walks on along its way (sideways or backwards, as it walks
    /// fighting) while it looks, rather than stand facing the noise: walking, the way more than 60° off where it looks stopped
    /// it, and a shot heard every few seconds made it walk and stop in a street
    pub heard_walk: bool,
    /// `tank-cover`: on foot with no target, an enemy in armour that nothing it carries hurts within [`TANK_NEAR_M`] and in sight
    /// ([`Shooter::armour_near`]): it goes to a spot hidden from him ([`crate::bots::individual::find_cover`]) rather than walk on
    /// in the open beside him
    pub tank_cover: bool,
    /// `flight-aim`: with a target, a flight to get unstuck (round a vehicle's hull, out of a place it cannot leave) moves its legs
    /// only: it keeps aiming and firing, strafing the flight's way, rather than turn its back and jump
    pub flight_aim: bool,
    /// `fire-ready`: with `fire-first`, it stops for a burst only if its last look reached the trigger (`why` is `trigger`,
    /// `pause` or `cycle`): not reloading, switching weapon, asking for one, nor holding a grenade or a weapon with no trigger,
    /// nor facing armour it can only hurt with a grenade. Without it the stop waited on the rhythm alone, and a bot that could
    /// not fire stood still for a burst that never came.
    /// TODO: off: it acts (reloading was about 7 % of `fire-first`'s stops in DC_Weapon_Bunkers) but the short run is level
    /// (-2.1 ± 4.1 tickets) and its foot jam does not move; next, reload heading for cover rather than just walking on.
    pub ready: bool,
}

impl FpsTweaks {
    pub const OFF: FpsTweaks = FpsTweaks { rhythm: false, friend: false, look: false, react: false, stand: false, pistol: false, armour: false, close: false, throw: false, fov: false, recall: false, heard_walk: false, tank_cover: false, flight_aim: false, ready: false };
    /// what the individual bot fires with unless its name says otherwise: the five together beat anchor 1 (bots 1.19.0) by +1.66 in
    /// the SPRT with fresh seeds, and the rhythm alone by +0.41; and since 1.21.0 `pistol-close` (a clear fault, the pistol at 92 m
    /// with a loaded rifle, mended at once: with the main gun loaded 0.37 s a minute on foot to 0.01, the short run level)
    /// and since 1.22.0 `armour`, `close-fight` and `grenade-throw` (clear faults: a rifle emptied at a tank, an enemy at 3 m run
    /// past and jumped over to get unstuck, grenades that never left the hand; the short run +2.3 tickets a round for `armour`
    /// and `grenade-throw` together, `close-fight` level), and since 1.23.0 `heard-walk` (a clear fault: a walk-stop of a second
    /// at each shot heard; with nobody in sight, walk-stops 1.8 a minute to 0.6, the short run level), and since 1.24.0 `fov` and
    /// `recall` (the user's choice, for a bot that does not know more than it should: the first target more than 90° off its
    /// crosshair in 21 % of contacts to 1 %; about 5 tickets a pair lost to a rival that sees all round)
    pub const DEFAULT: FpsTweaks = FpsTweaks { rhythm: true, friend: true, look: true, react: true, stand: true, pistol: true, armour: true, close: true, throw: true, fov: true, recall: true, heard_walk: true, tank_cover: false, flight_aim: false, ready: false };
}

/// `bots:pistol-close`: with the main gun dry, the pistol for an enemy within this, m (a person would not trade a gun for a
/// pistol further out).
pub const PISTOL_NEAR_M: f32 = 15.0;
/// `bots:pistol-close`: with the magazine empty and the enemy within this, the pistol to finish him: drawing it (well under a
/// second) is quicker than reloading a rifle or a sub-machine gun (`reloadTime` 3.8 to 5.6 s in BF1942), m.
pub const PISTOL_FINISH_M: f32 = 5.0;

/// `bots:armour`: a weapon whose round takes less than this share of a vehicle's hit points at its best face does not hurt it.
pub const ARMOUR_HURTS: f32 = 0.01;
/// `bots:armour`: an armoured enemy it can only hurt with a grenade is a target from this near, m (the grenade's reach, as
/// [`Shooter::weapons`] throws one).
pub const ARMOUR_GRENADE_M: f32 = 28.0;
pub const ARMOUR_GRENADE_MIN_M: f32 = 8.0;
/// `bots:tank-cover`: an enemy in armour it cannot hurt this near is a threat it hides from, m (a tank's machine gun cuts down a
/// soldier in the open well beyond; the Berlin take of the `armour` work had it walk by one at 8-24 m).
pub const TANK_NEAR_M: f32 = 30.0;

/// `bots:grenade-throw`: how long it lets go of the fire key with a grenade in hand before going back to the gun, s.
pub const GRENADE_LET_GO_S: f64 = 0.2;

/// `bots:close-fight`: a target this near keeps it fighting rather than getting unstuck, m (25 m lost 1.5 tickets a round in the
/// short run, DC_Al_Nas -9.8: the flight unsticks it after a fight further off).
pub const CLOSE_FIGHT_M: f32 = 8.0;

/// `bots:fov`: half the width of its field of view for an enemy within [`FOV_NEAR_M`], degrees, and for one beyond [`FOV_FAR_M`]
/// (linear in between). BF1942 shows about 90° across the screen; a figure in the corner of the eye is noticed near, not far.
pub const FOV_NEAR_DEG: f32 = 55.0;
pub const FOV_FAR_DEG: f32 = 40.0;
pub const FOV_NEAR_M: f32 = 15.0;
pub const FOV_FAR_M: f32 = 60.0;
/// `bots:fov`: an enemy this near is noticed wherever he is (his steps, his body against it), m.
pub const FOV_TOUCH_M: f32 = 2.5;
/// `bots:recall`: how long a target lost from sight is watched for where it was, s; how far ahead along its last motion it is
/// expected, at most, s; and how far off it may have been lost, m (one lost far away is not watched).
pub const RECALL_S: f64 = 3.0;
pub const RECALL_LEAD_S: f32 = 0.8;
pub const RECALL_M: f32 = 40.0;

/// `bots:fov`: whether `at`, `d` metres away, is within the field of view of one looking along `forward` from `eye`.
pub fn in_fov(forward: [f32; 3], eye: [f32; 3], at: [f32; 3], d: f32) -> bool {
    let k = ((d - FOV_NEAR_M) / (FOV_FAR_M - FOV_NEAR_M)).clamp(0.0, 1.0);
    let half = FOV_NEAR_DEG + k * (FOV_FAR_DEG - FOV_NEAR_DEG);
    d < FOV_TOUCH_M || angle_between(forward, [at[0] - eye[0], at[2] - eye[2]]).abs() <= half
}

/// `bots:recall`: a target lost from sight: who, where it was last seen (its chest), how it was moving (m/s) and when.
#[derive(Clone, Copy, Debug)]
pub struct Lost {
    pub player: usize,
    pub at: [f32; 3],
    pub velocity: [f32; 3],
    pub t: f64,
}

/// `bots:friend-gate`: a mate within this of the line blocks it, m, plus [`FRIEND_SPREAD`] per metre along it.
pub const FRIEND_BODY_M: f32 = 0.6;
pub const FRIEND_SPREAD: f32 = 0.02;
/// `bots:friend-gate`: blocked by a mate this long, it steps aside, s.
pub const STEP_ASIDE_S: f64 = 0.4;
/// `bots:look-all`: how many of the nearest in reach its look tries (one line of sight each, five times a second).
pub const LOOK_ALL: usize = 8;
/// `bots:look-all`: the target it has, if seen and clear, is kept unless a clear one is nearer by this factor.
pub const LOOK_KEEP: f32 = 1.3;
/// `bots:quick-react`: the spread of the reaction (lognormal), its cap, s, and how near its crosshair an enemy is to be reacted to
/// in half the time, degrees.
pub const REACT_SIGMA: f32 = 0.35;
pub const REACT_MAX_S: f32 = 1.5;
pub const REACT_NEAR_DEG: f32 = 20.0;
/// `bots:quick-react`: no grenade at an enemy closer than this, m.
pub const GRENADE_MIN_M: f32 = 15.0;

/// Whether, seen from above, `point` falls in the corridor a shell fired from `eye` to `target` passes through and bursts in.
pub fn in_corridor(eye: [f32; 3], target: [f32; 3], point: [f32; 3]) -> bool {
    let (dx, dz) = (target[0] - eye[0], target[2] - eye[2]);
    let l = (dx * dx + dz * dz).sqrt().max(1.0);
    let (px, pz) = (point[0] - eye[0], point[2] - eye[2]);
    let a = (px * dx + pz * dz) / l;
    let angle_between = (px * dz - pz * dx).abs() / l;
    a > 0.6 * l && a < l + 40.0 && angle_between < 20.0
}

/// Whether a mate is on the line of fire from `eye` to `target`: caution, not something observed.
pub fn friend_in_line(eye: [f32; 3], target: [f32; 3], amigos: &[[f32; 3]]) -> bool {
    let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let l2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if l2 <= 0.0 {
        return false;
    }
    amigos.iter().any(|a| {
        let p = [a[0] - eye[0], a[1] + CHEST - eye[1], a[2] - eye[2]];
        let t = (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) / l2;
        if !(0.0..=1.05).contains(&t) {
            return false;
        }
        let q = [p[0] - t * d[0], p[1] - t * d[1], p[2] - t * d[2]];
        // a metre and a half around the line
        q[0] * q[0] + q[1] * q[1] + q[2] * q[2] < 1.5 * 1.5
    })
}

/// `bots:friend-gate`: the mate nearest the line of fire from `eye` to `target` within [`FRIEND_BODY_M`] plus [`FRIEND_SPREAD`] per
/// metre along it, short of the target, and on which side of it (+1 its right as seen from `eye`, −1 its left); `None` if the
/// line is clear. A bullet is a line, and the aim's spread grows with distance: a mate a metre off it at 5 m is not in the way.
pub fn friend_on_line(eye: [f32; 3], target: [f32; 3], amigos: &[[f32; 3]]) -> Option<f32> {
    let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let l2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if l2 <= 0.0 {
        return None;
    }
    let l = l2.sqrt();
    amigos.iter().find_map(|a| {
        let p = [a[0] - eye[0], a[1] + CHEST - eye[1], a[2] - eye[2]];
        let t = (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) / l2;
        if !(0.0..1.0).contains(&t) {
            return None;
        }
        let q = [p[0] - t * d[0], p[1] - t * d[1], p[2] - t * d[2]];
        let r = FRIEND_BODY_M + FRIEND_SPREAD * t * l;
        (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] < r * r).then(|| if d[2] * q[0] - d[0] * q[2] > 0.0 { 1.0 } else { -1.0 })
    })
}

/// The slot of the weapon in hand (by its name, among what it carries).
pub fn slot_in_hand(me: &crate::bot_api::Me) -> Option<usize> {
    let w = me.weapon.as_deref()?;
    me.items.iter().find(|i| i.1 == w).map(|i| i.0)
}

/// Where a soldier on foot holds the weapon (roughly): at eye height standing, crouched or not.
pub fn weapon_of(pos: [f32; 3]) -> [f32; 3] {
    [pos[0], pos[1] + 0.6, pos[2]]
}

/// The target in sight: who it is, where it is (at the chest) and how many metres away.
pub type Target = (usize, [f32; 3], f32);

#[derive(Debug)]
pub struct Shooter {
    pub look: f64,
    pub target: Option<Target>,
    /// the last time it had someone in sight
    pub target_t: f64,
    pub ready_at: f64,
    pub next_shot: f64,
    pub burst: u32,
    pub bullets: i32,
    pub error: [f32; 2],
    pub still_in_combat: bool,
    pub decide_combat: f64,
    pub requested_weapon: Option<(usize, f64)>,
    pub grenade_until: f64,
    pub no_grenade: bool,
    pub uses_pistol: bool,
    pub line_busy: bool,
    /// since when, aimed, its line of fire has stayed busy (a mate or a wall in it); `None` while it is free
    pub line_busy_since: Option<f64>,
    pub health: f32,
    pub harassed_until: f64,
    /// bots: does not squeeze the trigger if a wall or the terrain hides the target from the eye or from the weapon (the target is chosen
    /// five times a second and in between it can duck behind something; the weapon is higher than the crouched eye). The plain bot: no.
    pub check_line: bool,
    /// bots: sees any part of an enemy's body (head, shoulder) and aims at it, not only the chest ([`BotView::body_seen`])
    pub see_body: bool,
    /// with `see_body`: the part of the target it saw, over its [`crate::bot_api::Other::pos`], which it follows between looks
    report: [f32; 3],
    /// the last time it did not fire for that reason
    pub line_blocked_t: f64,
    /// bots: the anti-tank weapon looks first at ground vehicles closer than this, m (0: the nearest, as on its own)
    pub prefers_vehicle_m: f32,
    /// bots: the weapon (its slot) it wants in hand for the target it has; None: whichever it carries (the plain bot)
    pub wanted_slot: Option<usize>,
    /// bots: the median reaction time to a new enemy, s, and the aiming error, degrees (standard deviation)
    pub median_reaction: f32,
    pub error_degrees: f32,
    /// the probability of throwing a grenade when starting a burst (bots: higher against a vehicle nearby)
    pub p_grenade: f32,
    /// bots: from an aircraft's seat that is not the pilot's, goes for what flies, leads it, never fires a healing tool at the enemy and
    /// keeps missiles for what is big or slow ([`super::dogfight::Gunner`]). The plain bot: None.
    pub gunner: Option<super::dogfight::Gunner>,
    /// `bots:streetwise`: on foot, it prefers the enemy that is busy ([`Opportunist`]); None: the nearest, as always
    pub opportunist: Option<Opportunist>,
    /// `bots:cover-flank`: a mate flanks from the first point to the second: among the enemies it sees, it prefers the one nearest
    /// that way
    pub prefer_way: Option<([f32; 3], [f32; 3])>,
    /// what its last [`Shooter::aim`] was off by, degrees (yaw, pitch; with the error of the burst), and why the trigger was not
    /// squeezed at the last [`Shooter::shoot_on_foot`] ([`crate::bots::fire_trace`]); written, never read by the bot
    pub aim_off: [f32; 2],
    pub why: &'static str,
    line_why: &'static str,
    /// `bots:fire-rhythm`, `friend-gate`, `look-all`, `quick-react`, `fire-first` ([`FpsTweaks`])
    pub fps: FpsTweaks,
    /// `bots:friend-gate`: the side the mate in its line stands on ([`friend_on_line`]), while one does
    pub friend_side: Option<f32>,
    /// `bots:fire-rhythm`: the named parameters `rhythm_pause` and `rhythm_burst` (1 and 1 by default)
    pub rhythm: (f32, f32),
    /// `bots:armour`: the enemies in armour it can only hurt with a grenade, at the last [`Shooter::armour_view`]
    pub grenade_only: Vec<usize>,
    /// `bots:tank-cover`: the nearest enemy in armour it cannot hurt within [`TANK_NEAR_M`] and in sight (player, where), at the
    /// last [`Shooter::armour_view`]
    pub armour_near: Option<(usize, [f32; 3])>,
    /// `bots:recall`: where its target was at the last tick it followed it, and when (for its motion), and the motion
    track: Option<(usize, [f32; 3], f64)>,
    velocity: [f32; 3],
    /// `bots:recall`: the target it lost from sight and still watches for
    pub lost: Option<Lost>,
    /// `bots:recall`: targets lost from sight with the variant, and of them taken again while watched for (the trace reads them)
    pub recalls: [u32; 2],
}

/// `bots:streetwise`: on foot, among the enemies it sees, the one with least `distance × (1 − trait × OPPORTUNIST_W × vulnerable)`,
/// where vulnerable is 1 for one that another of its side has as its target (`busy`, what the side was told in the last
/// [`BUSY_S`]: its tracers are seen) or that it saw in a vehicle less than [`GOT_OFF_S`] ago and now goes on foot. The one shooting
/// at it within [`THREAT_M`] is kept (the threat comes first), and it changes only for one [`SWITCH`] better.
#[derive(Clone, Debug, Default)]
pub struct Opportunist {
    pub streetwise: f32,
    pub busy: Vec<usize>,
    /// `bots:opportunist-all`: an enemy in sight looking away (more than [`BACK_DEGREES`] from it) is vulnerable too
    pub backs: bool,
    /// the enemies it saw riding, and when
    pub riding: Vec<(usize, f64)>,
}

// (0.5 among the 3 nearest moved B27 by 1 to 5 points)
pub const OPPORTUNIST_W: f32 = 0.8;
pub const OPPORTUNIST_LOOKS: usize = 5;
pub const BUSY_S: f64 = 2.0;
pub const GOT_OFF_S: f64 = 3.0;
pub const THREAT_M: f32 = 30.0;
pub const SWITCH: f32 = 0.7;
pub const BACK_DEGREES: f32 = 120.0;

impl Default for Shooter {
    fn default() -> Shooter {
        Shooter {
            look: 0.0,
            target: None,
            target_t: -100.0,
            ready_at: 0.0,
            next_shot: 0.0,
            burst: 0,
            bullets: 0,
            error: [0.0; 2],
            still_in_combat: false,
            decide_combat: 0.0,
            requested_weapon: None,
            grenade_until: 0.0,
            no_grenade: false,
            uses_pistol: false,
            line_busy: false,
            line_busy_since: None,
            health: 1.0,
            harassed_until: 0.0,
            check_line: false,
            see_body: false,
            report: [0.0, CHEST, 0.0],
            line_blocked_t: -100.0,
            prefers_vehicle_m: 0.0,
            wanted_slot: None,
            median_reaction: MEDIAN_REACTION,
            error_degrees: AIM_ERROR,
            p_grenade: P_GRENADE,
            gunner: None,
            opportunist: None,
            prefer_way: None,
            aim_off: [0.0; 2],
            why: "",
            line_why: "",
            fps: FpsTweaks::default(),
            friend_side: None,
            rhythm: (1.0, 1.0),
            grenade_only: Vec::new(),
            armour_near: None,
            track: None,
            velocity: [0.0; 3],
            lost: None,
            recalls: [0; 2],
        }
    }
}

impl Shooter {
    /// On the first tick of the life: when it first looks.
    pub fn spawn(&mut self, v: &BotView, rng: &mut Rng) {
        self.look = v.time + rng.uniform(0.0, 0.2) as f64;
    }

    /// `bots:armour`: `others` as it looks for a target on foot. One riding inside a vehicle that nothing it carries with rounds
    /// hurts ([`ARMOUR_HURTS`]) is left out, unless it carries grenades and he is within [`ARMOUR_GRENADE_MIN_M`]..[`ARMOUR_GRENADE_M`] (then it only throws
    /// them at him, [`Self::grenade_only`]); one on a seat that shows him is in, at his body. Without the variant, `others` as is.
    pub fn armour_view(&mut self, v: &BotView, eye: [f32; 3], others: &[Other]) -> Vec<Other> {
        self.grenade_only.clear();
        self.armour_near = None;
        if !self.fps.armour {
            return others.to_vec();
        }
        let me = &v.me;
        let carried = |pred: &dyn Fn(usize) -> bool| -> Vec<&str> {
            me.items.iter().zip(me.ammo.iter()).filter(|(i, a)| pred(i.0) && a.0 + a.1 > 0).map(|(i, _)| i.1.as_str()).collect()
        };
        let guns = carried(&|slot| slot != weapons::GRENADE_SLOT);
        let grenades = if self.no_grenade { Vec::new() } else { carried(&|slot| slot == weapons::GRENADE_SLOT) };
        let mut out = Vec::with_capacity(others.len());
        let mut near: Option<(usize, [f32; 3], f32)> = None;
        for o in others {
            if o.team == me.team || !o.in_vehicle {
                out.push(o.clone());
                continue;
            }
            if let Some(at) = o.exposed {
                out.push(Other { pos: at, ..o.clone() });
                continue;
            }
            // (one hidden is no target anyway: [`Self::see`] decides)
            let Some(j) = v.ride_of(o.player) else {
                out.push(o.clone());
                continue;
            };
            // (a knife or binoculars have no round: they hurt nothing)
            let hurts = |w: &&str| v.round_share(w, j).is_some_and(|s| s[0].max(s[2]) >= ARMOUR_HURTS);
            if guns.iter().any(hurts) {
                out.push(o.clone());
            } else if grenades.iter().any(hurts) && (ARMOUR_GRENADE_MIN_M..ARMOUR_GRENADE_M).contains(&dist3(eye, o.pos)) {
                self.grenade_only.push(o.player);
                out.push(o.clone());
            } else if self.fps.tank_cover {
                let d = dist3(eye, o.pos);
                if d < TANK_NEAR_M && near.is_none_or(|n| d < n.2) {
                    near = Some((o.player, o.pos, d));
                }
            }
        }
        // (one ray, for the nearest only: a tank behind a house is no reason to hide)
        self.armour_near = near.filter(|n| !v.hidden(eye, [n.1[0], n.1[1] + 1.5, n.1[2]])).map(|n| (n.0, n.1));
        out
    }

    /// `bots:fov`: `others` as it looks for a target on foot: its mates, and of the enemies those within its field of view
    /// ([`in_fov`]: the cone, or within [`FOV_TOUCH_M`]) and the one it already follows. Without the variant, `others` as is.
    pub fn fov_view(&self, v: &BotView, eye: [f32; 3], others: Vec<Other>) -> Vec<Other> {
        if !self.fps.fov {
            return others;
        }
        let me = &v.me;
        let following = self.target.map(|b| b.0);
        others
            .into_iter()
            .filter(|o| {
                let at = [o.pos[0], o.pos[1] + CHEST, o.pos[2]];
                o.team == me.team || following == Some(o.player) || in_fov(me.view[2], eye, at, dist3(eye, at))
            })
            .collect()
    }

    /// `bots:recall`: where to watch for the target it lost from sight (where it was, led by its last motion for up to
    /// [`RECALL_LEAD_S`]), while it is watched for; `None` once [`RECALL_S`] have passed or he is dead.
    pub fn recall(&mut self, v: &BotView, others: &[Other]) -> Option<[f32; 3]> {
        let l = self.lost?;
        let age = v.time - l.t;
        if age >= RECALL_S || !others.iter().any(|o| o.player == l.player) {
            self.lost = None;
            return None;
        }
        let lead = (age as f32).min(RECALL_LEAD_S);
        Some(std::array::from_fn(|k| l.at[k] + if k == 1 { 0.0 } else { l.velocity[k] * lead }))
    }

    /// `bots:recall`: its target's motion, from where it was at the last tick it followed it.
    fn follow(&mut self, now: f64) {
        let Some((p, at, _)) = self.target else {
            self.track = None;
            return;
        };
        if let Some((q, before, t)) = self.track
            && q == p
            && now - t > 0.01
        {
            let dt = (now - t) as f32;
            self.velocity = std::array::from_fn(|k| 0.5 * self.velocity[k] + 0.5 * (at[k] - before[k]) / dt);
        } else if self.track.is_none_or(|x| x.0 != p) {
            self.velocity = [0.0; 3];
        }
        self.track = Some((p, at, now));
    }

    /// The enemy it sees, five times a second: the nearest within range that is not hidden (it sees all round unless `bots:fov`
    /// leaves out what is not in its field of view, [`Self::fov_view`]).
    pub fn see(&mut self, v: &BotView, rng: &mut Rng, eye: [f32; 3], reach: f32, others: &[Other]) {
        if v.time < self.look {
            // follows it by sight wherever it is now
            if let Some(b) = self.target.as_mut() {
                match others.iter().find(|o| o.player == b.0) {
                    Some(o) => {
                        b.1 = if self.see_body { std::array::from_fn(|k| o.pos[k] + self.report[k]) } else { [o.pos[0], o.pos[1] + CHEST, o.pos[2]] };
                        b.2 = dist3(eye, b.1);
                    }
                    None => self.target = None,
                }
            }
            self.follow(v.time);
            return;
        }
        self.look = v.time + 0.2;
        let mut near: Vec<(f32, usize, [f32; 3])> = others
            .iter()
            .filter(|o| o.team != v.me.team)
            .map(|o| {
                let at = [o.pos[0], o.pos[1] + CHEST, o.pos[2]];
                (dist3(eye, at), o.player, at)
            })
            .filter(|x| x.0 < reach)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut seen = None;
        if self.prefers_vehicle_m > 0.0 {
            // the anti-tank weapon: the nearest enemy vehicle in range, before the nearest soldier
            let in_vehicle = |p: usize| others.iter().any(|o| o.player == p && o.in_vehicle);
            seen = near.iter().filter(|x| x.0 < self.prefers_vehicle_m && in_vehicle(x.1)).take(2).find(|x| !v.hidden(eye, x.2)).copied();
        }
        let see_body = self.see_body;
        let se_ve = |x: &(f32, usize, [f32; 3])| {
            if see_body {
                v.body_seen(eye, [x.2[0], x.2[1] - CHEST, x.2[2]]).map(|p| (x.0, x.1, p))
            } else {
                (!v.hidden(eye, x.2)).then_some(*x)
            }
        };
        let seen = match self.opportunist.as_mut().filter(|o| o.streetwise > 0.0 && seen.is_none()) {
            Some(op) => {
                let t = v.time;
                for x in near.iter() {
                    if others.iter().any(|o| o.player == x.1 && o.in_vehicle) {
                        op.riding.retain(|r| r.0 != x.1);
                        op.riding.push((x.1, t));
                    }
                }
                op.riding.retain(|r| t - r.1 < GOT_OFF_S);
                let got_off = |p: usize| op.riding.iter().any(|r| r.0 == p && t - r.1 > 0.1) && others.iter().any(|o| o.player == p && !o.in_vehicle);
                let back = |x: &(f32, usize, [f32; 3])| {
                    op.backs
                        && v.facing(x.1).is_some_and(|f| {
                            let (dx, dz) = (eye[0] - x.2[0], eye[2] - x.2[2]);
                            let l = dx.hypot(dz).max(1e-3);
                            f[0] * dx / l + f[1] * dz / l < BACK_DEGREES.to_radians().cos()
                        })
                };
                let score = |x: &(f32, usize, [f32; 3])| x.0 * (1.0 - op.streetwise * OPPORTUNIST_W * (op.busy.contains(&x.1) || got_off(x.1) || back(x)) as u8 as f32);
                let visible: Vec<(f32, usize, [f32; 3])> = near.iter().take(OPPORTUNIST_LOOKS).filter_map(se_ve).collect();
                let best = visible.iter().min_by(|a, b| score(a).total_cmp(&score(b)).then(a.1.cmp(&b.1))).copied();
                let now = self.target.and_then(|b| visible.iter().find(|x| x.1 == b.0).copied());
                match (now, best) {
                    (Some(n), _) if v.time < self.harassed_until && n.0 < THREAT_M => Some(n),
                    (Some(n), Some(b)) if score(&b) >= SWITCH * score(&n) => Some(n),
                    _ => best,
                }
            }
            None => match self.prefer_way.filter(|_| seen.is_none()) {
                Some((a, b)) => {
                    let off = |x: &(f32, usize, [f32; 3])| crate::bots::tactics::off_way(a, b, x.2);
                    let visible: Vec<(f32, usize, [f32; 3])> = near.iter().take(OPPORTUNIST_LOOKS).filter_map(se_ve).collect();
                    let best = visible.iter().min_by(|x, y| off(x).total_cmp(&off(y)).then(x.1.cmp(&y.1))).copied();
                    let now = self.target.and_then(|t| visible.iter().find(|x| x.1 == t.0).copied());
                    match (now, best) {
                        (Some(n), _) if v.time < self.harassed_until && n.0 < THREAT_M => Some(n),
                        (Some(n), Some(b)) if off(&b) >= SWITCH * off(&n) => Some(n),
                        _ => best,
                    }
                }
                None if self.fps.look && seen.is_none() => {
                    // (`bots:look-all`: more of the nearest, and one with no mate in the line first; the one it has is kept while
                    // seen and clear unless a clear one is much nearer)
                    let mates: Vec<[f32; 3]> = others.iter().filter(|o| o.team == v.me.team).map(|o| o.pos).collect();
                    let visible: Vec<(f32, usize, [f32; 3])> = near.iter().take(LOOK_ALL).filter_map(se_ve).collect();
                    let clear = |x: &(f32, usize, [f32; 3])| friend_on_line(eye, x.2, &mates).is_none();
                    let best = visible.iter().find(|x| clear(x)).copied();
                    let now = self.target.and_then(|b| visible.iter().find(|x| x.1 == b.0).copied()).filter(|x| clear(x));
                    match (now, best) {
                        (Some(n), Some(b)) if n.0 <= LOOK_KEEP * b.0 => Some(n),
                        (_, Some(b)) => Some(b),
                        _ => visible.first().copied(),
                    }
                }
                None => seen.or_else(|| near.iter().take(3).find_map(se_ve)),
            },
        };
        match seen {
            Some((d, p, at)) => {
                // after a while without seeing anyone, it takes time to react
                if self.target.is_none() && v.time - self.target_t > 3.0 {
                    self.ready_at = v.time
                        + if self.fps.react {
                            // (`bots:quick-react`: no long tail, and quicker for one it already nearly faces)
                            let near_crosshair = super::angle_between(v.me.view[2], [at[0] - eye[0], at[2] - eye[2]]).abs() < REACT_NEAR_DEG;
                            let median = if near_crosshair { 0.5 * self.median_reaction } else { self.median_reaction };
                            rng.lognormal(median, REACT_SIGMA).min(REACT_MAX_S) as f64
                        } else {
                            rng.lognormal(self.median_reaction, 0.7) as f64
                        };
                    self.uses_pistol = rng.si(P_PISTOL);
                    self.new_error(rng);
                    self.decide_combat = v.time;
                }
                if self.see_body {
                    self.report = others.iter().find(|o| o.player == p).map_or([0.0, CHEST, 0.0], |o| std::array::from_fn(|k| at[k] - o.pos[k]));
                }
                if self.lost.is_some_and(|l| l.player == p) {
                    self.recalls[1] += 1;
                }
                self.lost = None;
                self.target = Some((p, at, d));
                self.target_t = v.time;
            }
            None => {
                // (`bots:recall`: lost from sight; [`Self::recall`] checks he is still alive)
                if self.fps.recall
                    && let Some(b) = self.target
                    && b.2 < RECALL_M
                {
                    self.lost = Some(Lost { player: b.0, at: b.1, velocity: self.velocity, t: v.time });
                    self.recalls[0] += 1;
                }
                self.target = None;
            }
        }
        self.follow(v.time);
    }

    pub fn new_error(&mut self, rng: &mut Rng) {
        self.error = [rng.normal() * self.error_degrees, rng.normal() * self.error_degrees];
    }

    /// Whoever is shot does not stand thinking (any player sees their health).
    pub fn watch_health(&mut self, v: &BotView) {
        let me = &v.me;
        if me.health < self.health - 0.001 {
            self.ready_at = self.ready_at.min(v.time);
            self.next_shot = self.next_shot.min(v.time);
            self.harassed_until = v.time + HARASSED;
        }
        self.health = me.health;
    }

    /// The axes for aiming at the target, with the view in `rows`; returns whether it has aimed. A turret does not always lower to the target: hence `pitch_margin`.
    pub fn aim(&mut self, axes: &mut Vec<(usize, f32)>, rows: [[f32; 3]; 3], eye: [f32; 3], at: [f32; 3], cap: f32, pitch_margin: f32) -> bool {
        let f = rows[2];
        let a = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
        let flat_map = (a[0] * a[0] + a[2] * a[2]).sqrt();
        let (g, c) = (angle_between(f, [a[0], a[2]]) + self.error[0], a[1].atan2(flat_map).to_degrees() + self.error[1]);
        let c_now = pitch(f);
        axes.push((axis::MOUSE_X, (TURN * g / 3.0).clamp(-cap, cap)));
        axes.push((axis::MOUSE_Y, (TURN * (c_now - c) / 2.5).clamp(-cap, cap)));
        self.aim_off = [g, c - c_now];
        g.abs() < 3.0 && (c - c_now).abs() < pitch_margin
    }

    /// Whether, already aimed, a mate on foot is on the line of fire.
    pub fn look_line(&mut self, v: &BotView, aimed: bool, at: [f32; 3], others: &[Other]) {
        let me = &v.me;
        self.friend_side = None;
        self.line_busy = aimed && {
            let amigos: Vec<[f32; 3]> = others.iter().filter(|o| o.team == me.team).map(|o| o.pos).collect();
            if self.fps.friend {
                self.friend_side = friend_on_line(me.eye, at, &amigos);
                self.friend_side.is_some()
            } else {
                friend_in_line(me.eye, at, &amigos)
            }
        };
        self.line_why = "friend";
        // (and where it really looks, the first metres: aimed is ± 3°, and a corner beside it eats the burst)
        let ahead = |o: [f32; 3]| {
            let f = me.view[2];
            let l = (dist3(o, at) - 1.0).clamp(0.0, 12.0);
            [o[0] + f[0] * l, o[1] + f[1] * l, o[2] + f[2] * l]
        };
        if self.check_line && aimed && !self.line_busy {
            // (the same three rays in the same order, each only if the one before was clear; which one it was, for the trace)
            let blocked = if v.hidden(me.eye, at) {
                Some("wall_eye")
            } else if v.hidden(weapon_of(me.pos), at) {
                Some("wall_gun")
            } else if v.hidden(weapon_of(me.pos), ahead(weapon_of(me.pos))) {
                Some("wall_ahead")
            } else {
                None
            };
            if let Some(w) = blocked {
                self.line_busy = true;
                self.line_blocked_t = v.time;
                self.line_why = w;
            }
        }
        if aimed {
            self.line_busy_since = if self.line_busy { Some(self.line_busy_since.unwrap_or(v.time)) } else { None };
        }
    }

    /// Whether a mate blocked its line at the last [`Shooter::look_line`].
    pub fn friend_blocks(&self) -> bool {
        self.line_busy && self.line_why == "friend"
    }

    /// `bots:fire-first`: whether, aimed with its line clear at the last look, it is in a burst or due to fire (not in a pause).
    pub fn firing_now(&self, now: f64) -> bool {
        let can = !self.fps.ready || matches!(self.why, "trigger" | "pause" | "cycle");
        can && !self.line_busy && now >= self.ready_at && (self.burst > 0 || now >= self.next_shot)
    }

    /// Whether its line of fire has been busy for a while: standing still to shoot makes no sense.
    pub fn line_long_busy(&self, now: f64) -> bool {
        self.line_busy_since.is_some_and(|t| now - t > LINE_BUSY_S)
    }

    /// The trigger for whoever is on foot or in a seat, by weapon class. `alternate` is the bot's shared pulse (one tick yes, the next no).
    pub fn trigger(&mut self, v: &BotView, rng: &mut Rng, alternate: &mut bool, axes: &mut Vec<(usize, f32)>, class: WeaponClass, d: f32, reach: f32) {
        let now = v.time;
        let scale = if now < self.harassed_until { HARASSED_SCALE * PAUSE_SCALE } else { PAUSE_SCALE };
        let bullets = v.me.rounds.0;
        if class == WeaponClass::Automatic && self.burst > 0 {
            if bullets < self.bullets {
                self.burst = self.burst.saturating_sub((self.bullets - bullets) as u32);
                if self.burst == 0 {
                    self.next_shot = now + if self.fps.rhythm { weapons::pause_near(d, reach, scale * self.rhythm.0, rng) } else { weapons::pause(d, reach, scale, rng) } as f64;
                    self.new_error(rng);
                }
            }
            self.bullets = bullets;
            if self.burst > 0 {
                axes.push((axis::FIRE, 1.0));
            } else {
                self.why = "pause";
            }
            return;
        }
        self.bullets = bullets;
        if now < self.next_shot {
            self.why = "pause";
            return;
        }
        match class {
            WeaponClass::Automatic => {
                self.burst = if self.fps.rhythm { weapons::burst_near(d, reach, self.rhythm.1, rng) } else { weapons::burst(rng) };
                axes.push((axis::FIRE, 1.0));
            }
            WeaponClass::Bolt | WeaponClass::Rocket | WeaponClass::Pistol => {
                *alternate = !*alternate;
                if *alternate {
                    axes.push((axis::FIRE, 1.0));
                } else {
                    self.why = "cycle";
                    self.next_shot = now
                        + if self.fps.rhythm && class != WeaponClass::Rocket {
                            weapons::interval_near(class, d, reach, rng)
                        } else {
                            let pause = weapons::pause(d, reach, scale, rng);
                            weapons::interval(class, rng).max(0.5 * pause)
                        } as f64;
                    self.new_error(rng);
                }
            }
            WeaponClass::Granada | WeaponClass::Another => self.why = "no_class",
        }
    }

    /// With the enemy in range and aimed: reloads or switches weapon if needed, and fires at its rhythm.
    pub fn shoot_on_foot(&mut self, v: &BotView, rng: &mut Rng, alternate: &mut bool, axes: &mut Vec<(usize, f32)>, class: WeaponClass, d: f32, aimed: bool, reach: f32) {
        self.weapons(v, rng, axes, class, d);
        // (bots: while switching to the weapon the target asks for, it does not fire the one it carries)
        let changing = self.wanted_slot.is_some_and(|r| slot_in_hand(&v.me) != Some(r) && v.me.items.iter().any(|i| i.0 == r));
        let grenade_only = class != WeaponClass::Granada && self.target.is_some_and(|t| self.grenade_only.contains(&t.0));
        self.why = if v.time < self.ready_at {
            "reaction"
        } else if grenade_only {
            "armour"
        } else if !aimed {
            "aim"
        } else if self.line_busy {
            self.line_why
        } else if changing {
            "changing"
        } else if class == WeaponClass::Granada {
            "grenade"
        } else if self.requesting_weapon(v) {
            "requesting"
        } else if v.me.rounds.0 == 0 {
            "empty"
        } else {
            "trigger"
        };
        if aimed && !self.line_busy && v.time >= self.ready_at && !changing {
            if class == WeaponClass::Granada {
                if v.time >= self.grenade_until - 0.6 && v.time < self.grenade_until {
                    axes.push((axis::FIRE, 1.0));
                }
            } else if !self.requesting_weapon(v) && v.me.rounds.0 != 0 && !grenade_only {
                self.trigger(v, rng, alternate, axes, class, d, reach);
            }
        }
    }

    /// From a seat: half-second bursts with each weapon on its key. `hidden`: a mate or an own shell in between.
    pub fn shoot_from_seat(&mut self, v: &BotView, rng: &mut Rng, axes: &mut Vec<(usize, f32)>, aimed: bool, hidden: bool, reach: f32) {
        self.shoot_from_seat_keys(v, rng, axes, aimed, hidden, reach, (true, true));
    }

    /// As [`Shooter::shoot_from_seat`], with each of the primary and secondary keys allowed or not.
    #[allow(clippy::too_many_arguments)]
    pub fn shoot_from_seat_keys(&mut self, v: &BotView, rng: &mut Rng, axes: &mut Vec<(usize, f32)>, aimed: bool, hidden: bool, reach: f32, keys: (bool, bool)) {
        if aimed && v.time >= self.ready_at && !hidden {
            // half-second bursts
            if v.time < self.next_shot + 0.5 && v.time >= self.next_shot {
                // each weapon answers to its key, and does not choose by target
                if keys.0 {
                    axes.push((axis::FIRE, 1.0));
                }
                if keys.1 {
                    axes.push((axis::ALT_FIRE, 1.0));
                }
            } else if v.time >= self.next_shot + 0.5 {
                self.next_shot = v.time + weapons::pause(self.target.map_or(0.0, |b| b.2), reach, VEHICLE_SCALE * PAUSE_SCALE, rng) as f64;
                self.new_error(rng);
            }
        }
    }

    /// With no enemy in sight: lets go of the trigger, reloads, and goes back to the primary weapon or the one it had asked for.
    pub fn calm(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>, class: WeaponClass) {
        let me = &v.me;
        self.burst = 0;
        self.line_busy = false;
        // (`bots:pistol-close`: not the pistol while the main gun has rounds: back to it, and it is reloaded then)
        let main_has = || slot_ammo(me, weapons::PRIMARY_SLOT).is_some_and(|a| a.0 + a.1 > 0);
        if me.rounds.0 == 0 && me.rounds.1 > 0 && !(self.fps.pistol && class == WeaponClass::Pistol && main_has()) {
            axes.push((axis::RELOAD, 1.0));
        }
        self.back_to_primary(v, axes, class);
        if let Some((r, until)) = self.requested_weapon {
            if v.time < until {
                axes.push((axis::WEAPON1 + r - 1, 1.0));
            } else {
                self.requested_weapon = None;
            }
        }
    }

    pub fn requesting_weapon(&self, v: &BotView) -> bool {
        self.requested_weapon.is_some_and(|(_, until)| v.time < until)
    }

    /// Reloads, switches to the pistol and throws a grenade now and then.
    pub fn weapons(&mut self, v: &BotView, rng: &mut Rng, axes: &mut Vec<(usize, f32)>, class: WeaponClass, d: f32) {
        let me = &v.me;
        if let Some((r, until)) = self.requested_weapon {
            if v.time < until {
                axes.push((axis::WEAPON1 + r - 1, 1.0));
                return;
            }
            self.requested_weapon = None;
        }
        // a grenade that was not one (the engineer carries explosives in that slot)
        if class == WeaponClass::Another {
            // (what the medkit and the wrench are is already known)
            self.no_grenade |= !me.weapon.as_deref().is_some_and(|w| w.eq_ignore_ascii_case("MedPack") || w.eq_ignore_ascii_case("RepairPack"));
            self.request(v, weapons::PRIMARY_SLOT);
            return;
        }
        if class == WeaponClass::Granada {
            // (`bots:quick-react`: a mate stepped into the line: back to the gun, not standing with the grenade in hand)
            let thrown = v.time >= self.grenade_until + if self.fps.throw { GRENADE_LET_GO_S } else { 0.0 };
            if thrown || me.rounds.0 + me.rounds.1 == 0 || (self.fps.react && self.friend_blocks()) {
                self.request(v, weapons::PRIMARY_SLOT);
            }
            return;
        }
        // `bots:pistol-close`: the pistol in hand and the main gun with rounds: back to it, unless finishing one point-blank with its
        // magazine empty
        if self.fps.pistol && class == WeaponClass::Pistol {
            let main = slot_ammo(me, weapons::PRIMARY_SLOT);
            if main.is_some_and(|a| a.0 + a.1 > 0) && !(d < PISTOL_FINISH_M && main.is_some_and(|a| a.0 == 0)) {
                self.request(v, weapons::PRIMARY_SLOT);
                return;
            }
        }
        // ammunition
        if me.rounds.0 == 0 {
            let pistol_loaded = || slot_ammo(me, weapons::PISTOL_SLOT).is_some_and(|a| a.0 > 0);
            if self.fps.pistol && class != WeaponClass::Pistol && me.rounds.1 > 0 && d < PISTOL_FINISH_M && pistol_loaded() {
                self.request(v, weapons::PISTOL_SLOT);
            } else if me.rounds.1 > 0 {
                axes.push((axis::RELOAD, 1.0));
            } else if class != WeaponClass::Pistol && (!self.fps.pistol || d <= PISTOL_NEAR_M) {
                self.request(v, weapons::PISTOL_SLOT);
            }
            return;
        }
        // bots: the weapon the target asks for (decided by whoever drives the body, with hysteresis)
        if let Some(r) = self.wanted_slot
            && slot_in_hand(me) != Some(r)
            && me.items.iter().any(|i| i.0 == r)
        {
            self.request(v, r);
            return;
        }
        if class != WeaponClass::Pistol && self.uses_pistol && !self.fps.pistol {
            self.uses_pistol = false;
            self.request(v, weapons::PISTOL_SLOT);
            return;
        }
        // a grenade, rare and at mid range, when starting a burst (`bots:armour`: at armour it cannot hurt otherwise, from closer)
        let armour = self.target.is_some_and(|t| self.grenade_only.contains(&t.0));
        let lo = if armour { ARMOUR_GRENADE_MIN_M } else if self.fps.react { GRENADE_MIN_M } else { 10.0 };
        if !self.no_grenade
            && (lo..28.0).contains(&d)
            && self.burst == 0
            && v.time >= self.next_shot
            && v.time >= self.ready_at
            && !(self.fps.react && self.friend_blocks())
            && rng.si(self.p_grenade)
        {
            self.request(v, weapons::GRENADE_SLOT);
            self.grenade_until = v.time + 1.5;
        }
    }

    /// Asks to switch to slot `slot` for an instant (sapper and trade use it too).
    pub fn request(&mut self, v: &BotView, slot: usize) {
        self.requested_weapon = Some((slot, v.time + 0.1));
        self.burst = 0;
    }

    /// With no combat, goes back to the primary weapon if ammunition is left.
    pub fn back_to_primary(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>, class: WeaponClass) {
        if matches!(class, WeaponClass::Pistol | WeaponClass::Granada | WeaponClass::Another) && (v.time as u64).is_multiple_of(4) {
            axes.push((axis::WEAPON1 + weapons::PRIMARY_SLOT - 1, 1.0));
        }
    }
}

/// The rounds of what it carries in `slot` (in the magazine, and in the rest), if it carries something there.
fn slot_ammo(me: &crate::bot_api::Me, slot: usize) -> Option<(i32, i32)> {
    me.items.iter().position(|i| i.0 == slot).and_then(|k| me.ammo.get(k).copied())
}
