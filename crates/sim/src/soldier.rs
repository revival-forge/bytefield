//! Soldiers, as the original game moves them.
//!
//! A soldier (`BFSoldier`) is not a point: it has a rigid `PhysicsNode` (mass 100, drag 1) and a
//! `ResponsePhysics` like a vehicle's, with branches of its own (`CID_BFSoldierTemplate`):
//! - its collision mesh is `SkeletonCollisionMesh`'s, built in code: 17 vertices, material 41;
//!   only the first 5 (the feet and four points 0.6 m below the centre) touch the terrain;
//! - each hit with |v|² ≥ 0.1 calls the soldier's collision handling, which keeps the most upright
//!   normal of the frame ([`Response::ground_normal`]): the friction uses it, with limits × n.y⁵;
//! - `solveImpulse` and `addFriction` act at the soldier's centre (no torque);
//! - it never counts down to sleep;
//! - walking is friction: the input sets the speed the ground "should" have
//!   (`setSurfacePositionalSpeed`), and the friction pulls the soldier towards it;
//! - against objects, its vertices always go against the other's faces (LOD 1), and the normal
//!   is the direction from the hit back to the soldier, flat unless the hit is ≥ 0.65 m above or
//!   below it (`object_normal`); the foot (vertex 0) keeps the face's;
//! - two soldiers push each other apart by position alone (`push_apart`).
//!
//! The pieces live where the engine has them (`contact`, `mover`); this module has the soldier's
//! constants and functions, and [`World::soldier`].

use std::sync::Arc;

use crate::World;
use crate::anim::{self, Input, Machine};
use crate::contact::{Contactor, Ground, Response, grip};
use crate::mover::Vehicle;
use crate::obstacle::Hit;
use crate::rigid::RigidBody;

use crate::vecmath::V3;

/// The skeleton collision mesh template's constructor: the soldier's collision
/// vertices, in its frame (y up). `setCollisionOffset` moves them all with the pose.
pub const SKELETON_VERTICES: [V3; 17] = [
    [0.0, -1.0, 0.0],
    [-0.2, -0.6, -0.2],
    [-0.2, -0.6, 0.2],
    [0.2, -0.6, 0.2],
    [0.2, -0.6, -0.2],
    [-0.2, 0.0, -0.4],
    [-0.4, 0.0, -0.2],
    [-0.4, 0.0, 0.2],
    [-0.2, 0.0, 0.4],
    [0.2, 0.0, 0.4],
    [0.4, 0.0, 0.2],
    [0.4, 0.0, -0.2],
    [0.2, 0.0, -0.4],
    [0.0, 0.8, -0.4],
    [0.0, 0.8, 0.4],
    [0.4, 0.8, 0.0],
    [-0.4, 0.8, 0.0],
];
/// The material of every vertex (0x29).
pub const SKELETON_MATERIAL: u32 = 41;
/// The detonator's item index (`Detonator`'s `itemIndex`: `enableItem(11)` in `handleMessage` and
/// `selectBestLoadedWeapon`).
pub const DETONATOR_ITEM: i32 = 11;
/// `checkVsTerrain` looks at this many vertices of a soldier.
pub const TERRAIN_VERTICES: usize = 5;
/// |v|² from which a hit calls `handleCollision` (≥ against the terrain, > against objects).
pub const HIT_SPEED2: f32 = 0.1;
/// `addFriction` of a soldier: static and sliding limits are friction × these × 9.82 / 30 × n.y⁵.
pub const STATIC_FRICTION: f32 = 7.2;
pub const SLIDING_FRICTION: f32 = 4.8;
/// Against objects: a hit at least this far above or below the soldier's centre keeps the
/// vertical part of the normal (and uses how far along the segment it was as depth).
pub const VERTICAL_HIT: f32 = 0.65;
/// Two soldiers closer than this (m, between centres) push apart.
pub const SOLDIER_REACH: f32 = 1.0;
/// The soldier template's directional speed (stand, crouch, lie: forward, back) and `strafeSpeed`
/// (m/s), `walkSpeedFactor`: the speeds the input asks for (`handlePlayerInput`).
pub const DIRECTIONAL_SPEED: [[f32; 2]; 3] = [[6.0, 4.0], [2.0, 2.0], [1.0, 1.0]];
pub const STRAFE_SPEED: [f32; 3] = [4.0, 2.0, 1.0];
pub const WALK_SPEED_FACTOR: f32 = 1.0 / 3.0;

/// The soldier's collision handling, the part that matters to the physics: the first hit of the
/// frame, or one more upright than the kept one, sets the ground normal (and whether it was the
/// terrain's); a kept normal with y > 0.1 means the soldier stands on something.
pub fn handle_collision(r: &mut Response, n: V3, terrain: bool) {
    if r.count == 0 || r.kept_normal[1] < n[1] {
        r.ground_normal = n;
        r.kept_normal = n;
        r.kept_terrain = terrain;
    }
    if r.kept_normal[1] > ON_GROUND_NY {
        r.on_ground = true;
    }
    r.hit = true;
}

/// `handleCollision` with the water (`checkVsTerrain`, its last argument 1): a normal like any
/// other, but not ground.
pub fn water_collision(r: &mut Response) {
    let n = [0.0, 1.0, 0.0];
    if r.count == 0 || r.kept_normal[1] < n[1] {
        r.ground_normal = n;
        r.kept_normal = n;
        r.kept_terrain = true;
    }
    r.hit = true;
}

/// A kept normal more upright than this is ground.
pub const ON_GROUND_NY: f32 = 0.1;

/// The friction limits of a soldier (static, sliding), per frame.
pub fn friction_limits(friction: f32, ny: f32) -> (f32, f32) {
    let s = ny * ny;
    let st = ((friction * STATIC_FRICTION * crate::contact::FRICTION_G) / crate::contact::FPS) * ny * s * s;
    let sl = s * ny * ((crate::contact::FRICTION_G * friction * SLIDING_FRICTION) / crate::contact::FPS) * s;
    (st, sl)
}

/// `checkObjectVsObject` with a soldier's vertices but the first (the foot keeps the face's
/// normal): the normal becomes the direction from the hit point back to the segment's start (the soldier's root, set back one step), flat unless
/// the hit is ≥ 0.65 m above or below the soldier's centre `part`; then the depth is how far
/// before its end the segment met the face.
pub fn object_normal(hit: &mut Hit, start: V3, part: V3) {
    let w = [start[0] - hit.point[0], start[1] - hit.point[1], start[2] - hit.point[2]];
    let mut n = [w[0], 0.0, w[2]];
    if (part[1] - hit.point[1]).abs() >= VERTICAL_HIT {
        hit.depth = hit.along;
        n[1] = w[1];
    }
    let l2 = n[2] * n[2] + n[0] * n[0] + n[1] * n[1];
    if (l2 - 1.0).abs() >= f32::EPSILON {
        if l2.abs() >= f32::EPSILON {
            let k = 1.0 / l2.sqrt();
            n = [n[0] * k, n[1] * k, n[2] * k];
        } else {
            n = [0.0; 3];
        }
    }
    hit.normal = n;
}

/// `checkObjectVsObject` between two soldiers (neither in a vehicle): if their heights overlap
/// and their centres are under 1 m apart, they are pushed apart (in x and z only) by what is
/// missing to 1 m, each by its share: the larger of |push·v| and 0.9·|push| of each, the faster
/// one moving more. Positions only, at once.
pub fn push_apart(a: &mut RigidBody, b: &mut RigidBody) {
    let (lo, hi) = (SKELETON_VERTICES[0][1], 0.8f32);
    let (p, q) = (a.pos, b.pos);
    if q[1] + hi < lo + p[1] || p[1] + hi < lo + q[1] {
        return;
    }
    let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let l2 = d[2] * d[2] + d[1] * d[1] + d[0] * d[0];
    if l2 > SOLDIER_REACH {
        return;
    }
    let l = l2.sqrt();
    let dir = if l >= 0.01 { [d[0] / l, d[1] / l, d[2] / l] } else { [0.0, 0.0, 1.0] };
    let k = 1.0 - l;
    let push = [dir[0] * k, 0.0, dir[2] * k];
    let len = (push[0] * push[0] + push[1] * push[1] + push[2] * push[2]).sqrt() * 0.9;
    let along = |v: V3| (push[2] * v[2] + push[1] * v[1] + push[0] * v[0]).abs();
    let sa = along(a.vel).max(len);
    let sb = along(b.vel).max(len);
    if sa == 0.0 && sb == 0.0 {
        return;
    }
    let f = sa / (sb + sa);
    for i in 0..3 {
        a.pos[i] -= push[i] * f;
        b.pos[i] += push[i] * (1.0 - f);
    }
}

/// The soldier template's move acceleration / `moveDeacceleration`: how much the movement factors
/// (a `char` each, ±127) gain per frame with the input held, and lose without it.
pub const MOVE_ACCELERATION: i32 = 20;
pub const MOVE_DEACCELERATION: i32 = 12;
/// `handlePlayerInput` also pushes the soldier by this share of the speed it asks for, every frame.
pub const INPUT_PUSH: f32 = 0.75;
/// A movement factor beyond ±this reads as the throttle (strafe) held, for the animations.
pub const FACTOR_HELD: i32 = 50;
/// The jump: this × the ground normal's y, upwards, at once (m/s), minus a quarter of the speed
/// asked for (which is not asked that frame).
pub const JUMP_SPEED: f32 = 6.0;
pub const JUMP_KEEP: f32 = 0.25;
/// `g_simulationFps`: the mouse axes are per frame at this rate.
pub const SIM_FPS: f32 = 30.0;
/// Degrees the body turns per unit of `c_PIMouseLookX` (×3, `yaw<float>`).
pub const TURN_DEGREES: f32 = 3.0;
/// `setTurnLeftRightAngle 20 14`: how far the head turns (in mouse units) before the
/// body follows, standing still: up to 20 one way, 14 the other.
pub const TURN_LIMIT: [f32; 2] = [20.0, 14.0];
/// `setPointUpDownAngle 38 38`: the aim's pitch up and down.
pub const LOOK_LIMIT: [f32; 2] = [38.0, 38.0];
/// The head's own pitch and yaw go up to ±30; the yaw back to 0 at 150/s.
pub const HEAD_LIMIT: f32 = 30.0;
pub const HEAD_RETURN: f32 = 150.0;
/// The head's own turn on a seat (`handlePlayerInput` with a parent; twice that in the bone). On foot, [`HEAD_LIMIT`].
pub const SEAT_HEAD_LIMIT: f32 = 18.0;
/// The chest's turn on a seat: `setTurnLeftRightAngle 20 14` × 0.6 in the original. It goes with the head here, for lack of its own count (level 2: the same gesture ends the same).
pub const SEAT_TURN_LIMIT: [f32; 2] = [12.0, 8.4];
const SEAT_TURN_OF_HEAD: f32 = 12.0 / SEAT_HEAD_LIMIT;

/// The head's own turn on a seat after a tick, as on foot (twice the mouse, back by itself when moving without turning, gone when firing) with the seat's limit.
/// Level 2: the rule and the limit are the original's, and it gives what the original does: ∓300, then ±600, then ∓300 of the
/// mouse end near the middle, turned a little the first way. Only the Kubelwagen's driver was watched, and only sideways.
pub fn seat_head_turn(head: f32, mouse_x: f32, moving: bool, firing: bool, dt: f32) -> f32 {
    let turn = mouse_x * dt * SIM_FPS;
    let h = if firing {
        0.0
    } else if !moving || turn != 0.0 {
        head - (turn + turn)
    } else if head > 0.0 {
        (head - dt * HEAD_RETURN).max(0.0)
    } else {
        (head + dt * HEAD_RETURN).min(0.0)
    };
    h.clamp(-SEAT_HEAD_LIMIT, SEAT_HEAD_LIMIT)
}
/// The lean (towards its target at 3/s, in `updateAnimations`): strafing, ±0.8; strafing
/// while moving forward or back, 0.5 to the right, −0.4 to the left (negated going back).
pub const LEAN_RATE: f32 = 3.0;
/// |v|² beyond which a soldier flies as if blown up (`handleUpdate`), within this much world
/// time of the last explosion that hit it (`triggerFallingAnimation`). In pre-game the world
/// time stays 0, so it always holds there (most reference runs).
pub const FLY_SPEED2: f32 = 64.0;
pub const FLY_AFTER_BLAST: f64 = 0.2;
/// The parachute: falling faster than this (m/s down) more than this high over the
/// terrain, `handlePlayerInput` puts the soldier in `Lb_ParachuteFall`; it lands (from the fall,
/// or with the parachute open) once its vertical speed is under this.
pub const PARACHUTE_FALL_SPEED: f32 = -8.0;
pub const PARACHUTE_FALL_HEIGHT: f32 = 10.0;
pub const PARACHUTE_LAND_SPEED: f32 = 2.0;
/// `updateSwimming`: a soldier starts swimming this far (centre) under the water, stops above
/// this, and swims held this far under it.
pub const SWIM_START: f32 = 0.43;
pub const SWIM_STOP: f32 = 0.35;
pub const SWIM_DEPTH: f32 = 0.4;
/// Swimming, the soldier is pushed by this × the speed asked for, besides the 0.75.
pub const SWIM_PUSH: f32 = 5.0;
/// A standing soldier whose y axis is down to this is set straight up at once.
pub const UPRIGHT_LIMIT: f32 = 0.1;
/// Lying: how far the y axis goes to the kept normal each update, and the normal's y below which
/// it goes straight up instead.
pub const LIE_FOLLOW: f32 = 0.1;
pub const LIE_UPRIGHT_LIMIT: f32 = 0.3;
/// `checkPlayerTriggers` sends a trigger's message over this.
pub const TRIGGER: f32 = 0.5;
/// The camera child's turn per degree of the aim's pitch and per unit of the head's yaw.
pub const CAMERA_PITCH: f32 = -2.5;
pub const CAMERA_YAW: f32 = -3.0;

/// `setRotateXDeg` (axis 0) / `setRotateYDeg` (1): rows of a turn about that axis.
fn rotate_deg(axis: usize, degrees: f32) -> [V3; 3] {
    // the FPU's fsin/fcos of the f32 radians
    let r = degrees * (std::f32::consts::PI / 180.0);
    let (s, c) = (r as f64).sin_cos();
    let (s, c) = (s as f32, c as f32);
    if axis == 0 {
        [[1.0, 0.0, 0.0], [0.0, c, s], [0.0, -s, c]]
    } else {
        [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]]
    }
}

/// The soldier template's slow down mod: the speed the frame after touching an `Obstacle`.
pub const SLOW_DOWN: f32 = 0.4;
/// The collision skeleton's first sample: at the first update (the constructor's countdown is 0;
/// with the new state's morph at 0 it keeps the `.ske`'s pose); then `rand % 30 / 30 + 0.5` s.
pub const SAMPLE_FIRST: f32 = 0.0;

/// `c_PI*` axes a soldier reads.
pub mod axis {
    pub const YAW: usize = 0;
    pub const THROTTLE: usize = 3;
    pub const MOUSE_X: usize = 4;
    pub const MOUSE_Y: usize = 5;
    pub const FIRE: usize = 8;
    pub const RELOAD: usize = 24;
    pub const ALT_FIRE: usize = 23;
    pub const WEAPON1: usize = 14;
    pub const ACTION: usize = 9;
    pub const WALK: usize = 12;
    pub const LIE: usize = 28;
    pub const CROUCH: usize = 29;
}

/// A soldier's player and what it drives (`BFSoldier`): the input (`handlePlayerInput` reads it
/// every frame once someone sends one), the movement factors that ramp towards it
/// (`applyMovementFactors`), the head's turn off the body and the lower body's
/// animation state (`anim`), which gives the pose and multiplies the speeds.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Movement {
    pub input: Input,
    /// `handlePlayerInput` runs (a console-made soldier has no player: until it gets an input)
    pub driven: bool,
    /// forward and strafe
    pub factor: [i32; 2],
    pub lower: anim::Instance,
    /// the upper body: its states depend on the item held
    pub upper: anim::Instance,
    /// the active item's template name (the upper body's states are named after it; "" none)
    pub item: String,
    /// the head's yaw off the body, in mouse units
    pub head_yaw: f32,
    /// the aim's pitch, degrees
    pub pitch: f32,
    /// the head's own pitch and yaw
    pub head: [f32; 2],
    /// the lean and where it goes
    pub lean: [f32; 2],
    /// the move bits (1 forward, 2 back, 4 left, 8 right), from the movement factors
    pub bits: u16,
    /// frame updates (`handleFrameUpdate`, the animations' time) to run before the next input,
    /// when they are given (a reference run's); None: one of dt every step
    pub frames: Option<Vec<f32>>,
    #[serde(skip)]
    pub machine: Arc<Machine>,
    /// the collision skeleton and the countdown to its next sample
    #[serde(skip)]
    pub skeleton: Option<crate::skeleton::Skeleton>,
    pub sample_in: f32,
    /// the samples' own `rand` (the server's has no seed we know): a fixed-seed generator
    pub sample_rng: u64,
    /// the weapon in its hand, and the others it carries, by slot (`itemIndex`)
    pub weapon: Option<crate::firearms::FireArms>,
    pub stowed: Vec<crate::firearms::FireArms>,
    /// the kit's items in its order (their slots): `selectBestLoadedWeapon` takes the first
    /// with ammunition
    pub kit_order: Vec<i32>,
    /// the kit it carries (an index in `Sim.items`) and the world time of its player's
    /// last pickup (as a float), `kit.rs`
    pub kit: Option<usize>,
    pub kit_picked_at: f64,
    /// the ladder it climbs, or touched this frame, that frame's material and
    /// where across it (`ladder.rs`)
    pub ladder: Option<crate::ladder::Ladder>,
    pub ladder_contact: u32,
    pub ladder_t: f32,
    /// its kit's active parts (`kitpart.rs`) and whether there is room over it this frame
    /// (`hasRoomForJump`, asked before the input)
    pub kit_parts: Vec<crate::kitpart::KitPart>,
    pub kit_room: bool,
    /// the parachute is open; the template's `setParachuteDrag` and
    /// `drag` (the body's while it is closed), `setParachuteSpeed` (the push along its
    /// z axis, falling or under the parachute); a drag the body is to take (`setDrag`)
    pub parachute: bool,
    /// the last hit on its collision skeleton (`ISkeletonCollisionMesh`'s): the bone's x axis
    /// and whether the bone was `Bip01 Head` (the template's)
    pub last_hit: Option<([f32; 3], bool)>,
    pub parachute_drag: f32,
    pub drag: f32,
    pub parachute_speed: f32,
    pub set_drag: Option<f32>,
    /// the world time (`mWorldTime`: 0 in pre-game) and that of the last explosion that hit it
    /// (`triggerFallingAnimation`)
    pub world_time: f64,
    pub blasted_at: f64,
    /// dead: an explosion no longer sends it flying, so the
    /// corpse keeps its death animation instead of landing on its feet, aiming
    pub dead: bool,
    /// its height over the terrain (the body's y less the terrain's under it), for the fall
    pub height_over_terrain: f32,
    /// the first child (the camera the weapon hangs from): its rotation and position off the
    /// soldier, which `handleUpdate` sets from the aim and moves halfway to the pose's
    /// `setPoseCameraPos` each time
    pub camera_rows: [V3; 3],
    pub camera: V3,
    pub pose_camera: [V3; 3],
    /// the recoil (frames left, up, left-right)
    pub recoil: (i32, f32, f32),
    /// the game's input index (`getCurrentInputIndex`), which seeds the deviation and recoil
    pub input_index: i32,
    /// the projectiles fired since the sim last took them: (projectile template, shot)
    pub shots: Vec<(String, crate::projectile::Shot)>,
    /// the binoculars' `Fire` this frame (`magType` 2, the weapon's place scout camera): where it looks from, the
    /// fire matrix without deviation; the sim casts the ray and places the scout camera
    pub mark: Option<crate::projectile::Shot>,
    /// the fire message this frame with the medic's pack in hand (the soldier's message handling
    /// → `useMedPack`): the frame's caller heals around and, if it healed, tells the pack
    pub medpack: bool,
    /// the same with the engineer's wrench (`RepairPack` → `useRepairPack`)
    pub repairpack: bool,
    /// the fire message with the detonator in hand, ready (the soldier's message handling): the
    /// frame's caller sets off the charges (the weapon's detonate projectiles of item 4's)
    pub detonate: bool,
    /// what `useMedPack` heals: `healDistance` (5), `healFactor` (0.1) to another,
    /// `selfHealFactor` (0.1) to itself (the soldiers' data: 10, 0.25, 0.15)
    pub heal: [f32; 3],
    /// what `useRepairPack` reaches: `repairDistance` (3; the soldiers' data: 2) beyond
    /// a vehicle's radius, `repairFactor` (0.1; 0.15) hit points it gives, and
    /// `disarmMinesDistance` (2) beyond a mine's or charge's radius
    pub repair: [f32; 3],
    /// how whole the last vehicle it repaired is, above its critical damage (the HUD's)
    pub repair_status: f32,
    /// it has a player (`simulatePlayerUpdate`: the triggers, its input, then its and its
    /// weapon's `handleUpdate`; its shots have an owner); false: a console-made soldier, which
    /// `handleUpdate`s with the other objects before its input
    pub has_player: bool,
    /// that player is a human (not a bot): its weapon's messages and shots say so
    pub human: bool,
    /// × the directional speed (1; a bot's soldier gets 0.7 + (`rand` & 31) × 0.3/31
    /// when it enters it, the soldier's enter)
    pub speed_mod: f32,
}

/// `applyMovementFactors(input, acceleration, deacceleration, factor)`.
fn ramp(input: f32, f: &mut i32) {
    *f = if input > 0.0 {
        ((*f).max(0) + MOVE_ACCELERATION).min(127)
    } else if input < 0.0 {
        ((*f).min(0) - MOVE_ACCELERATION).max(-127)
    } else if *f > 0 {
        (*f - MOVE_DEACCELERATION).max(0)
    } else {
        (*f + MOVE_DEACCELERATION).min(0)
    };
}

/// An axis as `handlePlayerInput` cleans it: tiny (< 0.001) or absurd (beyond ±99999) is 0.
fn clean(x: f32) -> f32 {
    if x.abs() < 0.001 || x > 99999.0 || x < -99999.0 { 0.0 } else { x }
}

/// … and the throttle and strafe: beyond ±0.1 they are ±1.
fn quantize(x: f32) -> f32 {
    clean(if x > 0.1 { 1.0 } else if x < -0.1 { -1.0 } else { x })
}

fn held(f: i32) -> f32 {
    if f > FACTOR_HELD { 1.0 } else if f < -FACTOR_HELD { -1.0 } else { 0.0 }
}

/// `yaw<float>(M, degrees)` of an upright body: its rows turned about its own y axis.
/// (`rotateAboutLine(M, its y axis, degrees)`: for an upright body each axis × `RotY`, the
/// sums in the FPU's precision.)
pub fn yaw_rows(rows: &mut [V3; 3], degrees: f32) {
    let r = rotate_deg(1, degrees);
    for row in rows.iter_mut() {
        let a = *row;
        *row = [0, 1, 2].map(|j| (r[2][j] as f64 * a[2] as f64 + r[1][j] as f64 * a[1] as f64 + r[0][j] as f64 * a[0] as f64) as f32);
    }
}

impl Movement {
    pub fn new(machine: Arc<Machine>) -> Movement {
        let mut lower = anim::Instance::new(machine.find("Lb_Stand"));
        lower.pose = lower.state;
        let mut upper = anim::Instance::new(machine.find("Ub_StandAim"));
        upper.pose = upper.state;
        Movement {
            input: Input::default(),
            driven: false,
            factor: [0, 0],
            lower,
            upper,
            item: String::new(),
            head_yaw: 0.0,
            pitch: 0.0,
            head: [0.0; 2],
            lean: [0.0; 2],
            bits: 0,
            frames: None,
            machine,
            skeleton: None,
            sample_in: SAMPLE_FIRST,
            sample_rng: 1,
            weapon: None,
            stowed: Vec::new(),
            kit_order: Vec::new(),
            kit: None,
            kit_picked_at: f64::NEG_INFINITY,
            ladder: None,
            ladder_contact: 0,
            ladder_t: crate::ladder::UNSET,
            kit_parts: Vec::new(),
            kit_room: true,
            parachute: false,
            last_hit: None,
            parachute_drag: 1.0,
            drag: 1.0,
            parachute_speed: 1.0,
            set_drag: None,
            world_time: 0.0,
            blasted_at: 0.0,
            dead: false,
            height_over_terrain: f32::INFINITY,
            camera_rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            camera: [0.0; 3],
            pose_camera: [[0.0, 0.65, 0.0], [0.0, 0.12, 0.0], [0.0, -0.7, 0.0]],
            recoil: (0, 0.0, 0.0),
            input_index: 0,
            shots: Vec::new(),
            mark: None,
            medpack: false,
            repairpack: false,
            detonate: false,
            heal: [5.0, 0.1, 0.1],
            repair: [3.0, 0.1, 2.0],
            repair_status: 0.0,
            has_player: false,
            human: false,
            speed_mod: 1.0,
        }
    }

    /// `handleUpdate`'s collision sample: the countdown runs with the physics' dt; past 0 it is
    /// `rand % 30 / 30 + 0.5` s again and the skeleton is posed.
    pub fn sample_skeleton(&mut self, dt: f32) {
        if self.skeleton.is_none() {
            return;
        }
        self.sample_in -= dt;
        if self.sample_in > 0.0 {
            return;
        }
        self.sample_rng = self.sample_rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.sample_in = ((self.sample_rng >> 33) % 30) as f32 / 30.0 + 0.5;
        self.pose_skeleton(dt);
    }

    /// `updateAnimations(dt)` on the collision skeleton: each body half's animation at its time,
    /// by its morph (the lower body first); the lean moves towards where the move bits ask;
    /// then the aim (pitch, the head's yaw off the body and its own turn, the lean) and the
    /// chain.
    pub fn pose_skeleton(&mut self, dt: f32) {
        if self.skeleton.is_none() {
            return;
        }
        let b = self.bits;
        self.lean[1] = if b & 0xc == 0 {
            0.0
        } else if b & 3 == 0 {
            if b & 4 != 0 { -0.8 } else { 0.8 }
        } else {
            let l = if b & 4 != 0 { -0.4 } else { 0.5 };
            if b & 2 != 0 { -l } else { l }
        };
        let [now, to] = self.lean;
        if now != to {
            let step = dt * LEAN_RATE;
            self.lean[0] = if to > now { (now + step).min(to) } else { (now - step).max(to) };
        }
        let mut sk = self.skeleton.take().unwrap();
        self.pose_on(&mut sk);
        self.skeleton = Some(sk);
    }

    /// A skeleton posed as the soldier is now (its animations at their times, the aim, the lean
    /// as it is), without touching the soldier: what the client draws its skin with.
    pub fn posed_skeleton(&self) -> Option<crate::skeleton::Skeleton> {
        let mut sk = self.skeleton.clone()?;
        self.pose_on(&mut sk);
        Some(sk)
    }

    /// The skeleton as the soldier's own eyes see its arms (the soldier's setting of first person, then
    /// `updateAnimations` in the first person): the `.ske`'s pose with the upper body's
    /// first-person animation (its second; the lower body is not played), without the aim
    /// (the camera carries it). A looping one runs on `clock` (seconds) at its own speed; one
    /// played once goes with the state's time (fire, reload). A client's picture: the dedicated
    /// server is never in the first person.
    pub fn posed_skeleton_1p(&self, clock: f32) -> Option<crate::skeleton::Skeleton> {
        let mut sk = self.skeleton.clone()?;
        sk.reset();
        let m = &self.machine;
        if let Some(i) = self.upper.state
            && let Some((a, speed, looping)) = m.animation_1p(i)
        {
            let t = if looping { (clock * speed).rem_euclid(1.0) } else { self.upper.time };
            sk.apply(a, t, 1.0, looping);
        }
        let aim = crate::skeleton::Aim { pitch: 0.0, yaw: 0.0, head: [0.0; 2], lean: 0.0, locked: true, lying: false };
        let extras = sk.aim_extras(&aim);
        sk.transform_leaning(&extras, 0.0);
        Some(sk)
    }

    /// The skeleton of a soldier shown on a vehicle's seat: the seat's two states, looking ahead; a
    /// state the machine does not have changes nothing (`setAnimationState`), he keeps the one he had. `half_body`
    /// (`c_SeatShowHalfBodySoldier`): `disableBoneTree` of `Bip01 Pelvis`, whose children [`crate::skeleton::Skeleton::transform`] leaves on it,
    /// so the legs, not the spine. A client's picture, and, once as he gets in, the collision skeleton of a soldier on a seat
    /// that shows him ([`crate::entry::SeatShow`]): a soldier in a vehicle is not updated here.
    pub fn posed_skeleton_seated(&self, lower: &str, upper: &str, half_body: bool, head_yaw: f32) -> Option<crate::skeleton::Skeleton> {
        let mut sk = self.skeleton.clone()?;
        sk.reset();
        let m = &self.machine;
        for (name, had) in [(lower, self.lower.state), (upper, self.upper.state)] {
            if let Some(a) = m.find(name).or(had).and_then(|i| m.animation(i)) {
                sk.apply(a, 0.0, 1.0, true);
            }
        }
        let still = crate::skeleton::Aim { pitch: 0.0, yaw: 0.0, head: [0.0; 2], lean: 0.0, locked: true, lying: false };
        let extras = sk.aim_extras(&still);
        sk.transform_leaning(&extras, 0.0);
        if head_yaw != 0.0 {
            // (the arms stay as the animation has them: the original's hands do not leave the wheel, whatever the chest does)
            let arms = sk.abs.clone();
            let aim = crate::skeleton::Aim { yaw: (head_yaw * SEAT_TURN_OF_HEAD).clamp(-SEAT_TURN_LIMIT[1], SEAT_TURN_LIMIT[0]), head: [0.0, head_yaw], locked: false, ..still };
            let extras = sk.aim_extras(&aim);
            sk.transform_leaning(&extras, 0.0);
            for (b, keep) in arms.iter().enumerate() {
                let mut up = Some(b);
                while let Some(q) = up {
                    if sk.file.bones[q].name.ends_with("Clavicle") {
                        sk.abs[b] = *keep;
                        break;
                    }
                    up = sk.file.bones[q].parent;
                }
            }
        }
        if half_body && let Some(pelvis) = sk.file.index("Bip01 Pelvis") {
            for b in 0..sk.abs.len() {
                let mut up = sk.file.bones[b].parent;
                while let Some(q) = up.filter(|&q| q != pelvis) {
                    up = sk.file.bones[q].parent;
                }
                if up.is_some() {
                    sk.abs[b] = sk.abs[pelvis];
                }
            }
        }
        Some(sk)
    }

    /// Each body half's animation at its time, by its morph (the lower body first), then the
    /// aim and the chain.
    fn pose_on(&self, sk: &mut crate::skeleton::Skeleton) {
        let m = &self.machine;
        for half in [&self.lower, &self.upper] {
            if let Some(i) = half.state
                && let (Some(a), Some((_, looping))) = (m.animation(i), half.anim)
            {
                sk.apply(a, half.time, half.morph, looping);
            }
        }
        let flags = self.lower.flags(m);
        let aim = crate::skeleton::Aim { pitch: self.pitch, yaw: self.head_yaw, head: self.head, lean: self.lean[0], locked: flags & anim::LOCK_FREE_LOOK != 0, lying: flags & anim::LYING != 0 };
        let extras = sk.aim_extras(&aim);
        sk.transform_leaning(&extras, aim.lean);
    }

    /// The soldier's enable item: a new item (by its template name) is raised: the upper body goes
    /// to `Ub_StandRaiseWeapon<item>` (`Crouch`, `Lie` by the pose), set up at once (`updateState(0)`)
    /// and its animation applied to the skeleton (its bones' relative matrices only: they show
    /// at the next sample).
    pub fn enable_item(&mut self, name: &str) {
        if self.item == name {
            return;
        }
        self.item = name.to_string();
        let m = self.machine.clone();
        let f = self.lower.flags(&m);
        let pose = if f & anim::LYING != 0 { "Ub_LieRaiseWeapon" } else if f & anim::CROUCHING != 0 { "Ub_CrouchRaiseWeapon" } else { "Ub_StandRaiseWeapon" };
        let Some(i) = m.find(&format!("{pose}{name}")) else { return };
        self.upper.set(i);
        self.upper.update(&m, 0.0, "");
        if let (Some(sk), Some(a), Some((_, looping))) = (self.skeleton.as_mut(), m.animation(i), self.upper.anim) {
            sk.apply(a, self.upper.time, self.upper.morph, looping);
        }
    }

    /// `handleFrameUpdate(dt)`'s animation part: the upper body's pose state from the lower
    /// body's pose (`Ub_StandAim`, `Ub_Crouch`, `Ub_Lie`: the template's states), then each
    /// half's `updateState` with the item's name, the upper body's start time being the lower
    /// body's time.
    pub fn frame_update(&mut self, dt: f32) {
        let m = self.machine.clone();
        let f = self.lower.flags(&m);
        self.upper.pose = m.find(if f & anim::CROUCHING != 0 { "Ub_Crouch" } else if f & anim::LYING != 0 { "Ub_Lie" } else { "Ub_StandAim" });
        self.lower.update(&m, dt, &self.item);
        self.upper.start = self.lower.time;
        self.upper.update(&m, dt, &self.item);
    }

    /// A projectile hit its collision skeleton at this bone, whose x axis is `normal`.
    pub fn hit_bone(&mut self, bone: usize, normal: [f32; 3]) {
        let head = self.skeleton.as_ref().and_then(|sk| sk.file.index("Bip01 Head")) == Some(bone);
        self.last_hit = Some((normal, head));
    }

    /// The soldier's damage handling, the death: in a vehicle `Ub_DieInVehicle` alone; under the
    /// parachute `*_ParachuteDie`; swimming `*_DieSwim`; falling (`Lb_ParachuteFall`)
    /// `*_DieHitGround`; lying `*_DieLie`; crouching `*_DieChestCrouch`/`*_DieBackCrouch`;
    /// standing, a hit on the head `*_DieHead`, else three times in four `*_DieChestStand`/
    /// `*_DieBackStand` and once `*_DieSlow` (`rand & 3`). Back: the last hit's normal along
    /// the body's z axis. (The upper body waits for a weapon's animation in the original: not here.)
    pub fn die(&mut self, rows: &[[f32; 3]; 3], in_vehicle: bool) {
        self.dead = true;
        let m = self.machine.clone();
        let set = |a: &mut anim::Instance, name: &str| {
            if let Some(i) = m.find(name) {
                a.set(i);
            }
        };
        if in_vehicle {
            set(&mut self.upper, "Ub_DieInVehicle");
            return;
        }
        let (back, head) = self.last_hit.map_or((false, false), |(n, head)| (n[0] * rows[2][0] + n[1] * rows[2][1] + n[2] * rows[2][2] > 0.0, head));
        let side = if back { "Back" } else { "Chest" };
        let name = if self.parachute {
            "ParachuteDie".to_string()
        } else if self.lower.flags(&m) & anim::SWIMMING != 0 {
            "DieSwim".to_string()
        } else if self.in_state(&["Lb_ParachuteFall"]) {
            "DieHitGround".to_string()
        } else {
            match self.pose() {
                1 => format!("Die{side}Crouch"),
                2 => "DieLie".to_string(),
                _ if head => "DieHead".to_string(),
                _ if self.lower.rand() & 3 != 0 => format!("Die{side}Stand"),
                _ => "DieSlow".to_string(),
            }
        };
        set(&mut self.lower, &format!("Lb_{name}"));
        set(&mut self.upper, &format!("Ub_{name}"));
    }

    /// Standing again, aiming.
    pub fn revive(&mut self) {
        self.dead = false;
        let m = self.machine.clone();
        if let Some(i) = m.find("Lb_Stand") {
            self.lower.set(i);
        }
        if let Some(j) = m.find("Ub_StandAim") {
            self.upper.set(j);
        }
    }

    /// The soldier's damage handling, a hit that does not kill: the upper body only, by pose and side (the last hit's normal
    /// along the body's z axis), unless it is flying from an explosion. (The upper body waits for a weapon's animation in the original: not here.)
    pub fn hurt(&mut self, rows: &[[f32; 3]; 3]) {
        if self.in_state(&["Lb_ExplosionForward", "Lb_ExplosionBackward"]) {
            return;
        }
        let back = self.last_hit.is_some_and(|(n, _)| n[0] * rows[2][0] + n[1] * rows[2][1] + n[2] * rows[2][2] > 0.0);
        let side = if back { "Back" } else { "Chest" };
        let name = match self.pose() {
            0 => format!("Ub_Hit{side}Stand"),
            1 => format!("Ub_Hit{side}Crouch"),
            2 => "Ub_HitLie".to_string(),
            _ => return,
        };
        if let Some(i) = self.machine.find(&name) {
            self.upper.set(i);
        }
    }

    /// The input from now on: these axes, mapped; the rest unmapped.
    pub fn set_input(&mut self, axes: &[(usize, f32)]) {
        self.input = Input::default();
        for &(i, v) in axes {
            if i < 55 {
                self.input.axes[i] = v;
                self.input.mask |= 1 << i;
            }
        }
        self.driven = true;
    }

    /// The pose the speeds use (`getPose`, from the lower body's state).
    pub fn pose(&self) -> usize {
        self.lower.pose(&self.machine)
    }

    /// The parts of update handling (and of the collision and the frame updates since
    /// the last one) that move the animation state: landing ends a flight; the animations' time
    /// runs; faster than 8 m/s, the soldier flies (`Lb_ExplosionForward`/`Backward`).
    pub fn handle_update(&mut self, body: &mut RigidBody, r: &mut Response, ground: &dyn Ground, dt: f32) {
        let water = ground.water_level();
        self.upright(body, r, ground);
        self.land_from_fall(body);
        let m = self.machine.clone();
        let flying = [m.find("Lb_ExplosionForward"), m.find("Lb_ExplosionBackward")];
        if r.hit && self.lower.state.is_some() && flying.contains(&self.lower.state)
            && let Some(i) = m.find("Lb_Stand")
        {
            // (The soldier's collision handling: both halves)
            self.lower.set(i);
            if let Some(j) = m.find("Ub_StandAim") {
                self.upper.set(j);
            }
        }
        r.hit = false;
        match self.frames.as_mut() {
            Some(f) => {
                for dt in std::mem::take(f) {
                    self.frame_update(dt);
                }
            }
            None => self.frame_update(dt),
        }
        // after the frame updates: a state that ends there (`_POSE_`) is what the pose's camera sees, for one frame
        self.update_camera();
        let v = body.vel;
        let v2 = v[2] * v[2] + v[1] * v[1] + v[0] * v[0];
        let blasted = self.world_time - self.blasted_at < FLY_AFTER_BLAST;
        if blasted && !self.dead && !self.parachute && v2 > FLY_SPEED2 && !flying.contains(&self.lower.state) && !self.in_state(&["Lb_ParachuteFall"]) {
            let l = v2.sqrt();
            let z = body.rows[2];
            let ahead = z[2] * (v[2] / l) + z[1] * (v[1] / l) + z[0] * (v[0] / l) > 0.0;
            if let Some(i) = flying[if ahead { 0 } else { 1 }] {
                self.lower.set(i);
                if let Some(j) = m.find(if ahead { "Ub_ExplosionForward" } else { "Ub_ExplosionBackward" }) {
                    self.upper.set(j);
                }
            }
        }
        self.update_swimming(body, water);
        // falling, it is pushed along its z axis (not up); under the parachute too
        let fall = m.find("Lb_ParachuteFall");
        if self.parachute || (fall.is_some() && self.lower.state == fall) {
            let z = if self.parachute { body.rows[2] } else { self.camera_in_world(body).0[2] };
            let k = self.parachute_speed;
            let mut a = [z[0] * k, z[1] * k, z[2] * k];
            if !self.parachute && a[1] > 0.0 {
                a[1] = 0.0;
            }
            body.add_acc_at_rel([0.0; 3], a);
        }
        self.sample_skeleton(dt);
    }

    /// Whether the lower body is in one of these states.
    /// Falling with or without its parachute open (`Lb_ParachuteFall`).
    pub fn parachuting(&self) -> bool {
        self.parachute || self.in_state(&["Lb_ParachuteFall"])
    }

    pub(crate) fn in_state(&self, names: &[&str]) -> bool {
        self.lower.state.is_some() && names.iter().any(|n| self.machine.find(n) == self.lower.state)
    }

    /// `setIsParachuting`: open, the parachute's drag and `Lb_ParachuteOpen`/`Ub_ParachuteOpen`;
    /// closed (landing), the template's drag and `Lb_ParachuteHitGround`/`Ub_ParachuteHitGround`.
    pub fn set_parachuting(&mut self, open: bool) {
        if self.parachute == open {
            return;
        }
        self.parachute = open;
        self.set_drag = Some(if open { self.parachute_drag } else { self.drag });
        let (lo, up) = if open { ("Lb_ParachuteOpen", "Ub_ParachuteOpen") } else { ("Lb_ParachuteHitGround", "Ub_ParachuteHitGround") };
        let m = self.machine.clone();
        if let Some(i) = m.find(lo) {
            self.lower.set(i);
        }
        if let Some(j) = m.find(up) {
            self.upper.set(j);
        }
    }

    /// The fall's part of update handling (before the forces): landing, under the
    /// parachute (it closes) or from the free fall.
    fn land_from_fall(&mut self, body: &RigidBody) {
        if self.parachute && body.vel[1].abs() < PARACHUTE_LAND_SPEED {
            self.set_parachuting(false);
            return;
        }
        if !self.parachute && self.in_state(&["Lb_ParachuteFall"]) && body.vel[1].abs() < PARACHUTE_LAND_SPEED {
            let m = self.machine.clone();
            if let Some(i) = m.find("Lb_ParachuteHitGround") {
                self.lower.set(i);
            }
            if let Some(j) = m.find("Ub_ParachuteHitGround") {
                self.upper.set(j);
            }
        }
    }

    /// The frame updates given for this step, now (a reference run's; none given: `handle_update` runs
    /// one of dt).
    pub fn run_frames(&mut self) {
        if let Some(f) = self.frames.as_mut() {
            for dt in std::mem::take(f) {
                self.frame_update(dt);
            }
        }
    }

    /// The soldier's update handling: a standing (or crouching) soldier's y axis goes halfway to the
    /// world's up (straight up if it is down to 0.1), and the axes are rebuilt: x = y × z,
    /// z = x × y, y = z × x, each normalised. Lying, it goes a tenth of the way to the kept
    /// normal (the terrain's under the soldier if the terrain gave it; straight up if it is
    /// down to 0.3).
    /// TODO: dead, it does too, after a turn by the skeleton. Not done: the first thing that separates us from the original
    /// after a fatal fall (GC_Podrace, cliff_podrace.jsonl), because the drag goes by the body's axes.
    fn upright(&mut self, body: &mut RigidBody, r: &Response, ground: &dyn Ground) {
        let [_, mut y, mut z] = body.rows;
        let lying = self.lower.flags(&self.machine) & anim::LYING != 0;
        let to = if !lying {
            (y[1] > UPRIGHT_LIMIT).then_some(([0.0, 1.0, 0.0], 0.5))
        } else if r.kept_terrain {
            Some((ground.height_and_normal(body.pos[0], body.pos[2]).1, LIE_FOLLOW))
        } else {
            (r.kept_normal[1] >= LIE_UPRIGHT_LIMIT).then_some((r.kept_normal, LIE_FOLLOW))
        };
        y = match to {
            Some((n, k)) => [y[0] + (n[0] - y[0]) * k, (n[1] - y[1]) * k + y[1], y[2] + (n[2] - y[2]) * k],
            None => [0.0, 1.0, 0.0],
        };
        let norm = |v: &mut V3| -> bool {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if l < 1e-6 {
                return false;
            }
            let k = 1.0 / l;
            *v = v.map(|a| a * k);
            true
        };
        if !(norm(&mut y) && norm(&mut z)) {
            return;
        }
        let mut x = [y[1] * z[2] - z[1] * y[2], y[2] * z[0] - z[2] * y[0], y[0] * z[1] - y[1] * z[0]];
        if norm(&mut x) {
            z = [x[1] * y[2] - y[1] * x[2], x[2] * y[0] - y[2] * x[0], x[0] * y[1] - x[1] * y[0]];
            norm(&mut z);
            y = [z[1] * x[2] - x[1] * z[2], z[2] * x[0] - x[2] * z[0], z[0] * x[1] - z[1] * x[0]];
            norm(&mut y);
        }
        body.rows = [x, y, z];
    }

    /// The soldier's camera in its update: `RotX(−2.5 × pitch) · RotY(−3 × the head's yaw)`, and
    /// halfway from where it is to the pose's camera position.
    fn update_camera(&mut self) {
        let rx = rotate_deg(0, CAMERA_PITCH * self.pitch);
        let ry = rotate_deg(1, CAMERA_YAW * self.head_yaw);
        let mut r = [[0.0f32; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                // summed in the FPU's precision, rounded once
                *v = (rx[i][0] as f64 * ry[0][j] as f64 + rx[i][1] as f64 * ry[1][j] as f64 + rx[i][2] as f64 * ry[2][j] as f64) as f32;
            }
        }
        self.camera_rows = r;
        let to = self.pose_camera[self.pose().min(2)];
        for i in 0..3 {
            self.camera[i] += (to[i] - self.camera[i]) * 0.5;
        }
    }

    /// Where the eyes look from now, with `look` more mouse units (x, y) still to come: the
    /// camera of [`Self::update_camera`] from the aim's pitch and the head's yaw as they are after
    /// this tick's input (the server's camera is a tick behind them) plus `look` (× `zoomFov`
    /// zoomed), the pitch within its limits. A client's view between two ticks; the turn
    /// splits between head and body, but the sum is the same.
    pub fn view(&self, body: &RigidBody, look: [f32; 2]) -> ([V3; 3], V3) {
        let k = self.weapon.as_ref().filter(|w| w.zoomed()).map_or(1.0, |w| w.template.zoom_fov);
        let pitch = (self.pitch - look[1] * k).min(LOOK_LIMIT[0]).max(-LOOK_LIMIT[1]);
        let rx = rotate_deg(0, CAMERA_PITCH * pitch);
        let ry = rotate_deg(1, CAMERA_YAW * (self.head_yaw - look[0] * k));
        let r: [V3; 3] = std::array::from_fn(|i| std::array::from_fn(|j| rx[i][0] * ry[0][j] + rx[i][1] * ry[1][j] + rx[i][2] * ry[2][j]));
        let b = body.rows;
        let world = |v: V3| [0, 1, 2].map(|k| v[0] * b[0][k] + v[1] * b[1][k] + v[2] * b[2][k]);
        (r.map(world), self.camera_in_world(body).1)
    }

    /// The camera's (and the weapon's) axes and position in the world.
    pub fn camera_in_world(&self, body: &RigidBody) -> ([V3; 3], V3) {
        let b = body.rows;
        let world = |v: V3| [0, 1, 2].map(|k| (v[0] as f64 * b[0][k] as f64 + v[1] as f64 * b[1][k] as f64 + v[2] as f64 * b[2][k] as f64) as f32);
        let rows = self.camera_rows.map(world);
        let p = world(self.camera);
        (rows, [body.pos[0] + p[0], body.pos[1] + p[1], body.pos[2] + p[2]])
    }

    /// The weapon's part of the frame: its `handleUpdate`, then what
    /// the server's player triggers check sends for the input (fire: message 6, reload: 9) through
    /// the soldier's message handling; a shot leaves from the camera and gives the recoil.
    pub fn weapon_frame(&mut self, body: &RigidBody, gravity: f32, dt: f32) {
        self.weapon_update(body, gravity, dt);
        self.triggers(body, gravity);
    }

    /// The weapon's `handleUpdate` (and what it asks: a queued shot, autoReload).
    pub fn weapon_update(&mut self, body: &RigidBody, gravity: f32, dt: f32) {
        for w in self.stowed.iter_mut().filter(|w| w.template.mag_type == 1) {
            w.away += dt;
        }
        let Some(w) = self.weapon.as_mut() else { return };
        let asks = w.handle_update(dt);
        self.answer(asks, body, gravity, false);
    }

    /// What `checkPlayerTriggers` sends for the input (fire: message 6, reload: 9, the weapon
    /// keys: 10..).
    pub fn triggers(&mut self, body: &RigidBody, gravity: f32) {
        let mut asks = Vec::new();
        // the triggers' messages come with the player
        let human = self.has_player && self.human;
        self.medpack = false;
        self.repairpack = false;
        self.detonate = false;
        if self.driven {
            if self.input.mapped(axis::FIRE) && self.input.get(axis::FIRE) > TRIGGER {
                // the medic's pack (the wrench) with more in its clip than its reload time
                // (rounded): first `useMedPack` (`useRepairPack`); the pack hears the message
                // only if it healed (repaired or picked up) something
                let full = |w: &crate::firearms::FireArms| w.clip() > (w.template.reload_time as f64).round_ties_even() as i32;
                match self.weapon.as_ref() {
                    Some(w) if w.template.name.eq_ignore_ascii_case("MedPack") && full(w) => self.medpack = true,
                    Some(w) if w.template.name.eq_ignore_ascii_case("RepairPack") && full(w) => self.repairpack = true,
                    _ => asks.push(crate::firearms::Ask::Message(6)),
                }
            }
            if self.input.mapped(axis::ALT_FIRE) && self.input.get(axis::ALT_FIRE) > TRIGGER {
                asks.push(crate::firearms::Ask::Message(7));
            }
            if self.input.mapped(axis::RELOAD) && self.input.get(axis::RELOAD) > TRIGGER {
                asks.push(crate::firearms::Ask::Reload);
            }
            // c_PIWeapon1..: messages 10.. (`enableItem(slot)`), not while firing or recoiling
            for k in 1..=9 {
                let a = axis::WEAPON1 + k - 1;
                if self.input.mapped(a) && self.input.get(a) > TRIGGER {
                    asks.push(crate::firearms::Ask::Message(9 + k as i32));
                }
            }
        }
        self.answer(asks, body, gravity, human);
    }

    /// The medic's pack after `useMedPack` healed someone (the wrench after `useRepairPack`):
    /// the fire message goes on to it.
    pub fn medpack_fired(&mut self, body: &RigidBody, gravity: f32) {
        let human = self.has_player && self.human;
        self.answer(vec![crate::firearms::Ask::Message(6)], body, gravity, human);
    }

    /// What the weapon is asked or asks for: `human` when it comes with a human player (the
    /// triggers; the weapon's own `handleUpdate` sends its messages with none).
    fn answer(&mut self, asks: Vec<crate::firearms::Ask>, body: &RigidBody, gravity: f32, human: bool) {
        for ask in asks {
            // The soldier's message handling: fire with the detonator sets off the charges (and the
            // detonator hears it too); alt fire with the charges, ready, raises the detonator
            let held = |name: &str| self.weapon.as_ref().is_some_and(|w| w.template.name.eq_ignore_ascii_case(name) && w.ready_to_fire());
            match ask {
                crate::firearms::Ask::Message(6) if held("Detonator") => self.detonate = true,
                crate::firearms::Ask::Message(7) if held("ExpPack") => {
                    self.switch_to(DETONATOR_ITEM);
                    continue;
                }
                _ => {}
            }
            let ask = match ask {
                crate::firearms::Ask::Message(m @ (6 | 7)) => match self.weapon.as_mut() {
                    Some(w) => w.handle_message(m, human),
                    None => crate::firearms::Ask::Nothing,
                },
                a => a,
            };
            match ask {
                crate::firearms::Ask::Fire => self.fire(body, gravity),
                crate::firearms::Ask::Reload => self.reload_message(human),
                crate::firearms::Ask::FireAnimation => {
                    let m = self.machine.clone();
                    let lying = self.lower.flags(&m) & anim::LYING != 0;
                    if let Some(i) = m.find(if lying { "Ub_LieFire" } else { "Ub_Fire" }) {
                        self.upper.set(i);
                    }
                }
                crate::firearms::Ask::SelectBest => self.select_best_loaded_weapon(),
                crate::firearms::Ask::Message(18) => {
                    // The soldier's message handling: in the free fall, the parachute opens
                    if self.in_state(&["Lb_ParachuteFall"]) {
                        self.set_parachuting(true);
                    }
                }
                crate::firearms::Ask::Message(m) if m >= 10 => {
                    let free = self.recoil.0 == 0 && self.weapon.as_ref().is_none_or(|w| w.not_firing());
                    if free {
                        self.switch_to((m - 9) as i32);
                    }
                }
                _ => {}
            }
        }
    }

    /// The soldier's enable item: another weapon it carries in that slot is raised; the
    /// one in its hand is put away as it is.
    pub fn switch_to(&mut self, slot: i32) {
        if self.weapon.as_ref().is_some_and(|w| w.template.item_index == slot) {
            return;
        }
        let Some(k) = self.stowed.iter().position(|w| w.template.item_index == slot) else { return };
        // The hand weapon's enable fails for one out of rounds that `cantSelectWhenNoAmmo`: the one
        // in the hand stays
        if self.stowed[k].template.cant_select_empty && self.stowed[k].total_ammo() == 0 {
            return;
        }
        let mut new = self.stowed.remove(k);
        new.enable();
        let name = new.template.name.clone();
        if let Some(mut old) = self.weapon.replace(new) {
            // The hand weapon's disable
            old.set_zoom(false);
            self.stowed.push(old);
        }
        self.enable_item(&name);
    }

    /// The soldier's select best loaded weapon: with the last charge placed, the detonator; else the
    /// kit's first item with ammunition is raised.
    /// The weapon's setting of AI deviation on the weapon in the hand: the extra, one
    /// the hand weapon's deviation update with the soldier's present input, and the extra again
    /// (for the next update).
    pub fn set_ai_deviation(&mut self, d: f32) {
        let raw = |i: usize| if self.input.mapped(i) { self.input.get(i) } else { 0.0 };
        let (a0, a3, a4, a5) = (raw(axis::YAW), raw(axis::THROTTLE), raw(axis::MOUSE_X), raw(axis::MOUSE_Y));
        let pose = self.pose();
        if let Some(w) = self.weapon.as_mut() {
            w.set_ai_deviation(d, Some((a0, a3, a4, a5, pose)));
        }
    }

    /// Its weapons in the kit's order (the one in its hand among them), as they are.
    pub fn weapons_in_kit_order(&self) -> Vec<crate::firearms::FireArms> {
        let mut all: Vec<_> = self.weapon.iter().chain(self.stowed.iter()).cloned().collect();
        all.sort_by_key(|w| self.kit_order.iter().position(|&s| s == w.template.item_index).unwrap_or(usize::MAX));
        all
    }

    /// `dropKit`: its weapons go back to the kit (in the kit's order); its hands are empty.
    pub fn take_weapons(&mut self) -> Vec<crate::firearms::FireArms> {
        let all = self.weapons_in_kit_order();
        self.weapon = None;
        self.stowed.clear();
        self.kit_order.clear();
        all
    }

    /// `pickupKit`: a kit's weapons as they are (`addItem` each, in order), then
    /// `selectBestLoadedWeapon`.
    pub fn hold_weapons(&mut self, mut weapons: Vec<crate::firearms::FireArms>) {
        if weapons.is_empty() {
            return;
        }
        self.kit_order = weapons.iter().map(|w| w.template.item_index).collect();
        let w = weapons.remove(0);
        let name = w.template.name.clone();
        self.weapon = Some(w);
        self.stowed = weapons;
        self.item.clear();
        self.enable_item(&name);
        self.select_best_loaded_weapon();
    }

    fn select_best_loaded_weapon(&mut self) {
        if self.weapon.as_ref().is_some_and(|w| w.template.name.eq_ignore_ascii_case("ExpPack") && w.total_ammo() == 0) {
            self.switch_to(DETONATOR_ITEM);
            return;
        }
        let has = |w: &crate::firearms::FireArms| w.total_ammo() != 0;
        let slot = self.kit_order.iter().copied().find(|&s| {
            self.weapon.iter().chain(self.stowed.iter()).any(|w| w.template.item_index == s && has(w))
        });
        if let Some(s) = slot.or(self.kit_order.first().copied()) {
            self.switch_to(s);
        }
    }

    /// A shot of the weapon from the camera, and the recoil it gives (`calcRecoil`).
    pub(crate) fn fire(&mut self, body: &RigidBody, gravity: f32) {
        let (rows, pos) = self.camera_in_world(body);
        let root = Some(body.vel);
        let index = self.input_index;
        let w = self.weapon.as_mut().unwrap();
        // `magType` 2 (the binoculars): no shot, `placeScoutCamera` from the fire matrix (no deviation)
        if w.template.mag_type == 2 {
            let t = &w.template;
            self.mark = Some(t.fire_from(rows, pos, t.position, None, gravity, None));
            return;
        }
        for shot in w.shots(rows, pos, root, gravity, index) {
            let projectile = w.next_projectile();
            self.shots.push((projectile, shot));
        }
        if let Some(r) = w.template.recoil {
            self.recoil = crate::firearms::calc_recoil(&r, index);
        }
    }

    /// The soldier's message handling: the weapon's, and if it started reloading, the upper body
    /// to `Ub_StandReload` (`Ub_LieReload` lying).
    fn reload_message(&mut self, human: bool) {
        let Some(w) = self.weapon.as_mut() else { return };
        let before = w.reload;
        w.handle_message(9, human);
        if before <= 0.0 && w.reload > 0.0 {
            let m = self.machine.clone();
            let lying = self.lower.flags(&m) & anim::LYING != 0;
            if let Some(i) = m.find(if lying { "Ub_LieReload" } else { "Ub_StandReload" }) {
                self.upper.set(i);
            }
        }
    }

    /// `updateSwimming`: in deep enough water the soldier starts swimming (`Lb_StartSwim`), and
    /// swimming it is held 0.4 m under the surface (its position, not its speed) until the water
    /// is too shallow (`Lb_EndSwim`). Not done: the start's other test (the water over its first
    /// child, the head?), climbing.
    fn update_swimming(&mut self, body: &mut RigidBody, water: f32) {
        let m = self.machine.clone();
        let flags = self.lower.flags(&m);
        if flags & anim::CLIMBING != 0 {
            return;
        }
        let y = body.pos[1];
        let depth = if water - y > 0.0 { water - y } else { 0.0 };
        if flags & anim::SWIMMING == 0 {
            if depth > SWIM_START
                && let Some(i) = m.find("Lb_StartSwim")
            {
                self.lower.set(i);
            }
        } else if depth >= SWIM_STOP {
            if water > y {
                body.pos[1] = water - SWIM_DEPTH;
            }
        } else if let Some(i) = m.find("Lb_EndSwim") {
            self.lower.set(i);
        }
    }

    /// One frame of player input handling: the pose and the animation state from the
    /// input, the jump, the turn, the movement factors and the speed they ask for along the
    /// soldier's axes (z forward, x right): (factor/127)² of `directionalSpeed` (forward or back)
    /// and `strafeSpeed` of the pose, × the state's `setSpeed`, × `walkSpeedFactor` walking
    /// upright, no faster than that directional speed. The ground is set to move at it
    /// (returned), and the soldier is pushed by 0.75 of it, or jumps; all × the soldier's
    /// `speed_mod` (a bot's is random). Not done: sprinting, climbing, swimming,
    /// parachutes, vehicles, the upper body.
    pub fn handle_player_input(&mut self, body: &mut RigidBody, r: &mut Response, ground: &dyn Ground, dt: f32) -> V3 {
        let m = self.machine.clone();
        // falling fast and high: the free fall, where the parachute can open
        // (not with a kit part that overrides the air's inhibitions: `nochute`, the rocket pack)
        let override_air = self.kit_parts.iter().any(|p| p.t.override_air);
        if !self.parachute && !override_air && body.vel[1] < PARACHUTE_FALL_SPEED && !self.in_state(&["Lb_ParachuteFall"]) && self.height_over_terrain > PARACHUTE_FALL_HEIGHT
            && let (Some(i), Some(j)) = (m.find("Lb_ParachuteFall"), m.find("Ub_ParachuteFall"))
        {
            self.lower.set(i);
            self.upper.set(j);
        }
        let flags = self.lower.flags(&m);
        let on_ground = r.on_ground;
        // a ladder (`handleClimbAction`): the way it moves, and the climb's throttle
        let mut copy = self.input.clone();
        let mut dir = body.rows[2];
        self.handle_climb_action(body, r, ground, &mut copy, &mut dir);
        let inp = &self.input;
        // `handleSwimAction`: no jumping while swimming
        if flags & anim::SWIMMING != 0 && copy.mapped(axis::ACTION) {
            copy.set(axis::ACTION, 0.0);
        }
        let mut thr = quantize(copy.get(axis::THROTTLE));
        let mut strafe = quantize(inp.get(axis::YAW));
        let mut mouse_x = clean(inp.get(axis::MOUSE_X));
        let mut mouse_y = clean(inp.get(axis::MOUSE_Y));
        let walking = clean(inp.get(axis::WALK)) != 0.0;
        let mut fire = inp.get(axis::FIRE);
        // the weapon: the fire input reaches the animations only when it can fire
        // (`isAnimationReadyToUse`); the recoil turns the aim for its frames
        let mut change = false;
        if let Some(w) = self.weapon.as_ref() {
            if !w.animation_ready() && copy.mapped(axis::FIRE) {
                copy.set(axis::FIRE, 0.0);
                fire = 0.0;
            }
            if !w.alt_animation_ready() && copy.mapped(axis::ALT_FIRE) {
                copy.set(axis::ALT_FIRE, 0.0);
            }
            // zoomed, the mouse turns by `zoomFov`
            if w.zoomed() {
                mouse_x *= w.template.zoom_fov;
                mouse_y *= w.template.zoom_fov;
            }
            if self.recoil.0 != 0
                && let Some(r) = w.template.recoil
            {
                let k = w.dev_mod(self.lower.pose(&m));
                // `yawRecoil` comes back in the FPU's long double and is added so
                mouse_x = (crate::firearms::recoil_now(&r, self.recoil.0, self.recoil.2, k) + mouse_x as f64) as f32;
                mouse_y = (crate::firearms::recoil_now(&r, self.recoil.0, self.recoil.1, k) + mouse_y as f64) as f32;
                self.recoil.0 -= 1;
            }
            // out of rounds with `changeWeaponWhenNoAmmo` (a grenade, the charges, the mines):
            // another one up (`selectBestLoadedWeapon`)
            change = w.template.change_when_empty && w.total_ammo() == 0;
        }
        // the animations see the factors, not the axes
        if inp.mapped(axis::THROTTLE) {
            copy.set(axis::THROTTLE, held(self.factor[0]));
        }
        if inp.mapped(axis::YAW) {
            copy.set(axis::YAW, held(self.factor[1]));
        }
        // a jump: standing (not crouching or lying), on the ground, not jumping already
        let jump = if flags & (anim::CROUCHING | anim::LYING) == 0 && on_ground && self.lower.sound(&m) != anim::SOUND_JUMP {
            copy.get(axis::ACTION)
        } else {
            0.0
        };
        // the pose's keys start the state that changes it, below and above: `Lb_X`, then `Ub_X` (on foot; in a vehicle,
        // only if the seat's template says so: not done here, nor is this path in one). Before, the upper body kept
        // its weapon's state (DC's dc_fire_ak47_poses: `Ub_StandToCrouch` in the server, `Ub_StandAimAK47` here)
        let to = |name: &str, lower: &mut anim::Instance, upper: &mut anim::Instance| {
            if let Some(i) = m.find(name) {
                lower.set(i);
            }
            if let Some(i) = name.strip_prefix("Lb_").and_then(|rest| m.find(&format!("Ub_{rest}"))) {
                upper.set(i);
            }
        };
        if inp.get(axis::CROUCH) != 0.0 {
            if flags & (anim::SWIMMING | anim::CLIMBING | anim::CROUCHING | anim::LYING) == 0 {
                to("Lb_StandToCrouch", &mut self.lower, &mut self.upper);
            } else if flags & anim::LYING != 0 {
                to("Lb_LieToCrouch", &mut self.lower, &mut self.upper);
            }
        }
        if inp.get(axis::LIE) != 0.0 {
            if flags & anim::CROUCHING != 0 {
                to("Lb_CrouchToLie", &mut self.lower, &mut self.upper);
            } else if flags & anim::LYING != 0 {
                to("Lb_LieToStand", &mut self.lower, &mut self.upper);
            } else if flags & (anim::SWIMMING | anim::CLIMBING) == 0 {
                let f = self.lower.state.map_or(0.0, |i| m.states[i].speed[0]) * thr;
                to(if f > 0.0 { "Lb_RunStandToLie" } else { "Lb_StandToLie" }, &mut self.lower, &mut self.upper);
            }
        }
        if !on_ground && copy.mapped(axis::ACTION) {
            copy.set(axis::ACTION, 0.0);
        }
        // the head turns first, the body once the head is at its limit (or moving)
        let mut turn = mouse_x * dt * SIM_FPS;
        self.head_yaw = (self.head_yaw as f64 - mouse_x as f64 * dt as f64 * SIM_FPS as f64) as f32;
        let moving = thr != 0.0 || strafe != 0.0;
        // the head's own yaw follows twice the turn, not firing; it goes back to 0 otherwise
        let fire = clean(fire);
        if fire == 0.0 && (!moving || turn != 0.0) {
            self.head[1] -= turn + turn;
        } else if self.head[1] != 0.0 {
            if fire == 0.0 {
                let h = self.head[1];
                self.head[1] = if h > 0.0 { (h - dt * HEAD_RETURN).max(0.0) } else { (h + dt * HEAD_RETURN).min(0.0) };
            } else {
                self.head = [0.0; 2];
            }
        }
        self.head[1] = self.head[1].clamp(-HEAD_LIMIT, HEAD_LIMIT);
        if self.head_yaw > TURN_LIMIT[0] {
            self.head_yaw = TURN_LIMIT[0];
        } else if self.head_yaw < -TURN_LIMIT[1] {
            self.head_yaw = -TURN_LIMIT[1];
        } else {
            if copy.mapped(axis::MOUSE_X) {
                copy.set(axis::MOUSE_X, 0.0);
            }
            if !moving {
                turn = 0.0;
            }
        }
        let k = self.lower.check(&m, &copy);
        self.upper.check(&m, &copy);
        // the aim's pitch, and the head's own, twice (not firing)
        let look = mouse_y * dt * SIM_FPS;
        // the product stays in the FPU's precision until the subtraction
        let look64 = mouse_y as f64 * dt as f64 * SIM_FPS as f64;
        self.pitch = ((self.pitch as f64 - look64) as f32).min(LOOK_LIMIT[0]).max(-LOOK_LIMIT[1]);
        if fire == 0.0 {
            self.head[0] -= look + look;
        }
        self.head[0] = self.head[0].clamp(-HEAD_LIMIT, HEAD_LIMIT);
        thr *= k[0];
        turn *= k[1];
        strafe *= k[2];
        if turn != 0.0 {
            yaw_rows(&mut body.rows, turn * TURN_DEGREES);
        }
        if (strafe != 0.0 || thr != 0.0) && self.head_yaw != 0.0 {
            yaw_rows(&mut body.rows, -self.head_yaw * TURN_DEGREES);
            self.head_yaw = 0.0;
        }
        ramp(thr, &mut self.factor[0]);
        ramp(strafe, &mut self.factor[1]);
        let f = self.factor;
        self.bits = (f[0] > 0) as u16 | ((f[0] < 0) as u16) << 1 | ((f[1] < 0) as u16) << 2 | ((f[1] > 0) as u16) << 3;
        if flags & anim::LYING != 0 && f[0] == 0 {
            self.bits = 0;
        }
        let slow = if std::mem::take(&mut r.slowed) { SLOW_DOWN } else { 1.0 };
        let pose = self.pose();
        let kk = 1.0f32 / 127.0;
        let max = self.speed_mod * DIRECTIONAL_SPEED[pose][if self.factor[0] < 1 { 1 } else { 0 }] * k[0];
        let f = self.factor[0] as f32 * kk;
        let mut kf = f.signum() * (f * f) * max * slow;
        let st = self.factor[1] as f32 * kk;
        let mut ks = st * st * STRAFE_SPEED[pose] * st.signum() * slow;
        if walking && pose == 0 {
            kf *= WALK_SPEED_FACTOR;
            ks *= WALK_SPEED_FACTOR;
        }
        let x = body.rows[0];
        let mut v = [dir[0] * kf + x[0] * ks, dir[1] * kf + x[1] * ks, dir[2] * kf + x[2] * ks];
        let l2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
        if max * max < l2 {
            let c = max / l2.sqrt();
            v = v.map(|a| a * c);
        }
        // the kit's active parts see the state's flags, and 0x80 for a
        // jump on the ground
        if !self.kit_parts.is_empty() {
            let mask = flags & !0x80 | if jump != 0.0 { 0x80 } else { 0 };
            let room = self.kit_room;
            for p in self.kit_parts.iter_mut() {
                p.update(&self.input, mask, &mut || room);
            }
        }
        // on a ladder: no gravity, and that speed itself (`setVelocity`), not the push
        body.gravity_modifier = if self.ladder.is_some() { 0.0 } else { 1.0 };
        if self.ladder.is_some() {
            body.vel = v;
            v = [0.0; 3];
        } else if jump != 0.0 {
            let mut d = [v[0], 0.0, v[2]];
            let l = (d[0] * d[0] + d[2] * d[2]).sqrt();
            if l > 0.0 {
                d = [d[0] / l, 0.0, d[2] / l];
            }
            let n = r.kept_normal;
            let up = (d[2] * n[2] + d[1] * n[1] + d[0] * n[0] + 1.0).min(1.0) * n[1] * JUMP_SPEED;
            let a = [(0.0 - v[0] * JUMP_KEEP) * SIM_FPS, (up - v[1] * JUMP_KEEP) * SIM_FPS, (0.0 - v[2] * JUMP_KEEP) * SIM_FPS];
            body.add_acc_at_rel([0.0; 3], a);
            v = [0.0; 3];
        } else if r.count == 0 {
            body.add_acc_at_rel([0.0; 3], v.map(|a| a * INPUT_PUSH));
        }
        // their accelerations, in the soldier's axes (`getAcceleration`, over 1 m/s²)
        if self.ladder.is_none() {
            for p in &self.kit_parts {
                let a = p.acceleration();
                if a[0] * a[0] + a[1] * a[1] + a[2] * a[2] > 1.0 {
                    let rw = body.rows;
                    let w = [0, 1, 2].map(|k| rw[0][k] * a[0] + rw[1][k] * a[1] + rw[2][k] * a[2]);
                    body.add_acc_at_rel([0.0; 3], w);
                }
            }
        }
        if flags & anim::SWIMMING != 0 && self.ladder.is_none() {
            body.add_acc_at_rel([0.0; 3], v.map(|a| a * SWIM_PUSH));
        }
        r.on_ground = false;
        // (the weapon change of the recoil's part, here for the borrow: nothing between reads
        // the weapon)
        if change {
            self.select_best_loaded_weapon();
        }
        // The hand weapon's deviation update with this input's mapped axes
        let raw = |i: usize| if self.input.mapped(i) { self.input.get(i) } else { 0.0 };
        let (a0, a3, a4, a5) = (raw(axis::YAW), raw(axis::THROTTLE), raw(axis::MOUSE_X), raw(axis::MOUSE_Y));
        if let Some(w) = self.weapon.as_mut() {
            w.update_deviation(a0, a3, a4, a5, pose, jump != 0.0);
        }
        v
    }
}

impl Vehicle {
    /// The soldier takes a kit's weapons (`addKitByName`): `active` in its hand, or the first
    /// one (`selectBestLoadedWeapon`: the kit's items in order, the first with ammunition), the
    /// others stowed. False without weapons or movement.
    pub fn give_kit(&mut self, weapons: Vec<crate::projectile::Weapon>, active: Option<&str>) -> bool {
        let Some(m) = self.soldier_mut() else { return false };
        if weapons.is_empty() {
            return false;
        }
        m.kit_order = weapons.iter().map(|w| w.item_index).collect();
        let mut all: Vec<_> = weapons.into_iter().map(|w| crate::firearms::FireArms::new(std::sync::Arc::new(w))).collect();
        let k = active.and_then(|a| all.iter().position(|w| w.template.name.eq_ignore_ascii_case(a))).unwrap_or(0);
        let w = all.remove(k);
        let name = w.template.name.clone();
        m.weapon = Some(w);
        m.stowed = all;
        m.enable_item(&name);
        true
    }

    /// `setIsParachuting` (a paratrooper's spawn point opens it at once).
    pub fn parachute(&mut self, open: bool) {
        if let Some(m) = self.soldier_mut() {
            m.set_parachuting(open);
            if let Some(d) = m.set_drag.take() {
                self.craft.body.drag = d;
            }
        }
    }

    /// What the soldier's player asks for from now on: forward and strafe, the pose's keys
    /// (1 crouch: `c_PICrouch` held; 2 lie: `c_PILie` held, which toggles), the slow walk.
    pub fn set_move(&mut self, forward: f32, strafe: f32, pose: usize, walking: bool) {
        let axes = [
            (axis::THROTTLE, forward),
            (axis::YAW, strafe),
            (axis::WALK, if walking { 1.0 } else { 0.0 }),
            (axis::CROUCH, if pose == 1 { 1.0 } else { 0.0 }),
            (axis::LIE, if pose == 2 { 1.0 } else { 0.0 }),
        ];
        if let Some(m) = self.soldier_mut() {
            m.set_input(&axes);
        }
    }

    /// The velocity the soldier's input asks for (world, m/s); kept until changed (and no
    /// player input runs).
    pub fn set_walk(&mut self, v: V3) {
        if let Some(m) = self.soldier_mut() {
            m.driven = false;
        }
        for c in self.contacts.iter_mut().filter(|c| c.soldier) {
            c.response.walk = v;
        }
    }
}

/// The contactor of a soldier: its skeleton collision mesh at its centre.
pub fn contactor(name: &str) -> Contactor {
    Contactor {
        walked: 0,
        name: name.to_string(),
        vertices: SKELETON_VERTICES.to_vec(),
        materials: vec![SKELETON_MATERIAL; SKELETON_VERTICES.len()],
        bbox: Some(bounds()),
        pos: [0.0; 3],
        rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        grip: grip::CONTACT,
        is_root: true,
        soldier: true,
        projectile: false,
        spring: None,
        response: Response::default(),
        last_impulses: Vec::new(),
        hits: Vec::new(),
        faces: Vec::new(),
        scale: [1.0; 3],
        radius: radius(),
        anchor: None,
        drive: None,
        forward: [0.0, 0.0, 1.0],
        side: (0.0, [1.0, 0.0, 0.0]),
        engine_above: false,
        chain: Vec::new(),
        chain_anchor: Vec::new(),
    }
}

/// `computeBoundingValues` of the skeleton mesh: its box.
pub fn bounds() -> [V3; 2] {
    let mut lo = SKELETON_VERTICES[0];
    let mut hi = lo;
    for v in SKELETON_VERTICES {
        for i in 0..3 {
            lo[i] = lo[i].min(v[i]);
            hi[i] = hi[i].max(v[i]);
        }
    }
    [lo, hi]
}

/// … and its radius (the farthest vertex).
pub fn radius() -> f32 {
    SKELETON_VERTICES.iter().map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()).fold(0.0, f32::max)
}

impl World {
    /// Whether an object template is a soldier (`ObjectTemplate.create BFSoldier`).
    pub fn is_soldier(&self, template: &str) -> bool {
        self.db.object_template(template).is_some_and(|t| t.kind.eq_ignore_ascii_case("bfsoldier"))
    }

    /// A soldier template's collision skeleton: its `createSkeleton` file, the bones of its
    /// `setSkeletonCollisionBone name radius adjustment material` (names with `_` for spaces;
    /// only bones with a parent), `setCharacterHeight` (−0.9 by default) and the pitch's
    /// bounds lying (`setLiePointUpDownAngle a b`: from b to a).
    pub fn collision_skeleton(&self, template: &str) -> Option<crate::skeleton::Skeleton> {
        let t = self.db.object_template(template)?;
        let file = t.get("createSkeleton")?.first()?;
        let file = Arc::new(bytefield_formats::anim::Skeleton::parse(&self.vfs.read(file).ok()?).ok()?);
        let num = |a: &[String], i: usize| a.get(i).and_then(|v| v.parse::<f32>().ok());
        let collision = t
            .all("setskeletoncollisionbone")
            .into_iter()
            .filter_map(|a| {
                let bone = file.index(&a.first()?.replace('_', " "))?;
                file.bones[bone].parent?;
                Some(crate::skeleton::CollisionBone { bone, radius: num(a, 1)?, shift: num(a, 2).unwrap_or(0.0), material: num(a, 3).unwrap_or(0.0) as u32 })
            })
            .collect();
        let height = t.get("setCharacterHeight").and_then(|a| num(a, 0)).unwrap_or(-0.9);
        let mut s = crate::skeleton::Skeleton::new(file, collision, height);
        if let Some(a) = t.get("setLiePointUpDownAngle")
            && let (Some(hi), Some(lo)) = (num(a, 0), num(a, 1))
        {
            s.pitch_bounds[1] = [lo, hi];
        }
        Some(s)
    }

    /// A soldier ready to drop into a [`crate::Sim`]: its rigid body (mass, drag from the template;
    /// the skeleton mesh's box), no components, the skeleton contactor. At the origin, at rest.
    pub fn soldier(&self, template: &str) -> Option<Vehicle> {
        if !self.is_soldier(template) {
            return None;
        }
        let model = crate::vehicle::Component {
            name: template.to_string(),
            kind: "BFSoldier".into(),
            position: [0.0; 3],
            rotation: [0.0; 3],
            params: Default::default(),
            children: Vec::new(),
        };
        let mut body = self.rigid_body(template)?;
        body.bbox = Some(bounds());
        let mut movement = Movement::new(self.animations());
        movement.skeleton = self.collision_skeleton(template);
        if let Some(t) = self.db.object_template(template) {
            let num = |n: &str, d: f32| t.get(n).and_then(|a| a.first()).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
            movement.parachute_drag = num("setparachutedrag", 1.0);
            movement.parachute_speed = num("setparachutespeed", 1.0);
            movement.heal = [num("healdistance", 5.0), num("healfactor", 0.1), num("selfhealfactor", 0.1)];
            movement.repair = [num("repairdistance", 3.0), num("repairfactor", 0.1), num("disarmminesdistance", 2.0)];
        }
        movement.drag = body.drag;
        let movement = Some(movement);
        let body = crate::mover::Body { template: template.to_string(), craft: crate::craft::Craft::new(&model, body), contacts: vec![contactor(template)], radius: radius(), id: 0, solved: false, fixed: false, physics_off: false };
        Some(Vehicle { body, movement, armor: self.db.object_template(template).and_then(crate::damage::ArmorTemplate::from_template).map(crate::damage::Armor::new), removed: false, gone_since: None, born: 0, place_gen: 0, team: 0, entry: Default::default(), weapons: Vec::new(), shots: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_mesh_box_and_radius() {
        assert_eq!(bounds(), [[-0.4, -1.0, -0.4], [0.4, 0.8, 0.4]]);
        assert_eq!(radius(), 1.0);
    }

    #[test]
    fn the_most_upright_normal_wins() {
        let mut r = Response::default();
        handle_collision(&mut r, [0.6, 0.8, 0.0], true);
        assert_eq!(r.ground_normal, [0.6, 0.8, 0.0]);
        r.count = 1;
        handle_collision(&mut r, [0.8, 0.6, 0.0], false);
        assert_eq!(r.ground_normal, [0.6, 0.8, 0.0]);
        assert!(r.kept_terrain);
        handle_collision(&mut r, [0.0, 1.0, 0.0], false);
        assert_eq!(r.ground_normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn friction_limits_fall_with_the_slope() {
        let (st, sl) = friction_limits(1.0, 1.0);
        assert!((st - 7.2 * 9.82 / 30.0).abs() < 1e-5 && (sl - 4.8 * 9.82 / 30.0).abs() < 1e-5);
        let (st2, _) = friction_limits(1.0, 0.8);
        assert!((st2 / st - 0.8f32.powi(5)).abs() < 1e-5);
    }

    #[test]
    fn object_normal_is_flat_at_the_waist_and_upright_at_the_feet() {
        let mut h = Hit { normal: [1.0, 0.0, 0.0], point: [1.0, 10.0, 0.0], depth: -0.1, along: -0.3, material: 0 };
        object_normal(&mut h, [0.0, 10.2, 0.0], [0.0, 10.0, 0.0]);
        assert_eq!(h.normal, [-1.0, 0.0, 0.0]);
        assert_eq!(h.depth, -0.1);
        let mut h = Hit { normal: [0.0, 1.0, 0.0], point: [0.0, 9.0, 0.0], depth: -0.1, along: -0.3, material: 0 };
        object_normal(&mut h, [0.0, 10.0, 0.0], [0.0, 10.0, 0.0]);
        assert_eq!(h.normal, [0.0, 1.0, 0.0]);
        assert_eq!(h.depth, -0.3);
    }

    #[test]
    fn two_soldiers_push_apart_to_a_metre() {
        let mut a = RigidBody::new([0.0, 0.0, 0.0], 100.0, 1.0, None);
        let mut b = RigidBody::new([0.4, 0.0, 0.0], 100.0, 1.0, None);
        push_apart(&mut a, &mut b);
        assert!((b.pos[0] - a.pos[0] - 1.0).abs() < 1e-5, "{:?} {:?}", a.pos, b.pos);
        assert!((a.pos[0] + 0.3).abs() < 1e-5);
        // one standing on the other's head: no push
        let mut c = RigidBody::new([0.0, 2.0, 0.0], 100.0, 1.0, None);
        let before = c.pos;
        push_apart(&mut a, &mut c);
        assert_eq!(c.pos, before);
    }

    struct Flat;
    impl Ground for Flat {
        fn height_and_normal(&self, _: f32, _: f32) -> (f32, V3) {
            (-100.0, [0.0, 1.0, 0.0])
        }
        fn material(&self, _: f32, _: f32) -> u32 {
            0
        }
        fn water_level(&self) -> f32 {
            -100.0
        }
    }

    #[test]
    fn input_speed_runs_forward_at_six() {
        let calls: Vec<(String, Vec<String>)> = ["Lb_Stand", "Lb_Crouch"]
            .iter()
            .flat_map(|n| [("AnimationStateMachine.createState".to_string(), vec![n.to_string()])])
            .chain([("AnimationStateMachine.setFlag".to_string(), vec!["c_AsmIsCrouching".to_string()])])
            .collect();
        let machine = Arc::new(Machine::from_commands(calls.iter().map(|(c, a)| (c.as_str(), a.as_slice())), |_| None));
        let speed = |crouch: bool, forward: f32, strafe: f32| {
            let mut m = Movement::new(machine.clone());
            if crouch {
                m.lower.set(machine.find("Lb_Crouch").unwrap());
            }
            m.set_input(&[(axis::THROTTLE, forward), (axis::YAW, strafe)]);
            let mut body = RigidBody::new([0.0; 3], 100.0, 1.0, None);
            let mut r = Response::default();
            (0..10).map(|_| m.handle_player_input(&mut body, &mut r, &Flat, 1.0 / 30.0)).last().unwrap()
        };
        assert_eq!(speed(false, 1.0, 0.0), [0.0, 0.0, 6.0]);
        assert_eq!(speed(false, -1.0, 0.0), [0.0, 0.0, -4.0]);
        assert_eq!(speed(true, 0.0, 1.0), [2.0, 0.0, 0.0]);
    }

    /// A blast flings a living soldier (`Lb_ExplosionForward`), not a dead one: a corpse thrown
    /// by a tank's shell kept flying, landed on `Lb_Stand`/`Ub_StandAim` and stood there, aiming.
    #[test]
    fn a_blast_does_not_fling_the_dead() {
        let calls: Vec<(String, Vec<String>)> = ["Lb_Stand", "Ub_StandAim", "Lb_ExplosionForward", "Lb_ExplosionBackward", "Ub_ExplosionForward", "Ub_ExplosionBackward", "Lb_DieSlow", "Ub_DieSlow", "Lb_DieChestStand", "Ub_DieChestStand"]
            .iter()
            .map(|n| ("AnimationStateMachine.createState".to_string(), vec![n.to_string()]))
            .collect();
        let machine = Arc::new(Machine::from_commands(calls.iter().map(|(c, a)| (c.as_str(), a.as_slice())), |_| None));
        let after_blast = |dead: bool| {
            let mut m = Movement::new(machine.clone());
            m.lower.set(machine.find("Lb_Stand").unwrap());
            let mut body = RigidBody::new([0.0; 3], 100.0, 1.0, None);
            if dead {
                m.die(&body.rows, false);
            }
            (m.world_time, m.blasted_at) = (10.0, 10.0);
            body.vel = [0.0, 5.0, 12.0];
            m.handle_update(&mut body, &mut Response::default(), &Flat, 1.0 / 30.0);
            m.in_state(&["Lb_ExplosionForward", "Lb_ExplosionBackward"])
        };
        assert!(after_blast(false));
        assert!(!after_blast(true));
    }
}

#[cfg(test)]
mod seat_head_tests {
    use super::*;

    /// What the original does: a big turn each way and half of it back ends near the middle, on the first side; it stays with the mouse still.
    #[test]
    fn the_seats_head() {
        let dt = 1.0 / 30.0;
        let gesture = |h: f32, total: f32| (0..20).fold(h, |h, _| seat_head_turn(h, total / 20.0, false, false, dt));
        let h = gesture(0.0, -12.0);
        assert_eq!(h, SEAT_HEAD_LIMIT);
        assert_eq!(seat_head_turn(h, 0.0, false, false, dt), h, "stays");
        let h = gesture(h, 24.0);
        assert_eq!(h, -SEAT_HEAD_LIMIT);
        let back = gesture(h, -12.0);
        assert!((back - 6.0).abs() < 1e-3, "{back}");
        // (driving off, it goes back by itself; firing, at once)
        assert_eq!(seat_head_turn(4.0, 0.0, true, false, dt), 0.0);
        assert_eq!(seat_head_turn(4.0, 0.0, false, true, dt), 0.0);
    }
}
