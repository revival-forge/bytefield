//! A vehicle's physics from its component tree, as the original game runs it: the root's
//! rigid body plus the components that push it. For now the ones that fly: `Wing` (lift, with
//! the regulator of air brakes) and `Engine` of the plane kind (thrust), both `RotationalBundle`s
//! whose angles follow the player's input. Springs, landing gear and floating come with contact.
//!
//! One update, in the engine's order: each component reads its input and moves its angles
//! (`handlePlayerInput`, `handleUpdate`), then adds its acceleration to the root at its point
//! (`PhysicsWing`/the physics engine's physics update), then the root integrates (`rigid`).
//!
//! Frames: row matrices like the engine's (rows = the part's x, y, z axes); a child's rows are
//! its local rotation times its parent's. Local rotation from `setRotation yaw/pitch/roll`
//! (degrees): Rz(roll)·Rx(pitch)·Ry(yaw), checked against the server's wing matrices.

use crate::vecmath::dot;
use serde::Serialize;

use crate::rigid::{GRAVITY_Y, RigidBody};
use crate::vehicle::{Component, Value};

use crate::vecmath::V3;
type M3 = [V3; 3];

pub const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
/// The gravity the lift formulas were tuned for: lift scales with g / −9.82.
pub const LIFT_GRAVITY: f32 = -9.82;
/// The height at which the air (lift, propeller) runs out, m (`BasicPhysicsSystem`).
pub const AIR_HEIGHT: f32 = 1000.0;
/// |lift acceleration| limit of one wing.
pub const MAX_LIFT: f32 = 200.0;

/// `c_PI*` player inputs by value (`PlayerInputMap`; "" for the gaps).
pub const INPUTS: &[&str] = &[
    "c_PIYaw", "c_PIPitch", "c_PIRoll", "c_PIThrottle", "c_PIMouseLookX", "c_PIMouseLookY",
    "c_PICameraX", "c_PICameraY", "c_PIFire", "c_PIAction", "c_PIUse", "c_PIMouseLook",
    "c_PIWalk", "c_PIRun", "c_PIMenuSelect1", "c_PIMenuSelect2", "c_PIMenuSelect3", "c_PIMenuSelect4",
    "c_PIMenuSelect5", "c_PIMenuSelect6", "c_PIMenuSelect7", "c_PIMenuSelect8", "c_PIMenuSelect9",
    "c_PIAltFire", "c_PIReload", "c_PIDrop", "c_PIToggleCameraMode", "c_PIToggleCamera",
    "c_PILie", "c_PICrouch", "", "", "", "", "", "", "", "", "", "", "", "", "", "", "c_PISayAll",
    "c_PISayTeam", "c_PINextItem", "c_PIPrevItem", "c_PICommunication", "", "c_PIMap",
    "c_PIZoomMap", "", "", "", "c_PINone",
];
pub const INPUT_COUNT: usize = 56;

pub fn transpose(a: &M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| a[j][i]))
}

pub fn mul(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    r
}

fn rot_x(deg: f32) -> M3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[1.0, 0.0, 0.0], [0.0, c, s], [0.0, -s, c]]
}

fn rot_y(deg: f32) -> M3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]]
}

fn rot_z(deg: f32) -> M3 {
    let (s, c) = deg.to_radians().sin_cos();
    [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
}

/// Local rows of `setRotation yaw/pitch/roll`.
pub fn ypr(r: V3) -> M3 {
    mul(&mul(&rot_z(r[2]), &rot_x(r[1])), &rot_y(r[0]))
}

/// A point in a frame to the parent: p·rows.
pub fn to_parent(p: V3, rows: &M3) -> V3 {
    [0, 1, 2].map(|i| p[0] * rows[0][i] + p[1] * rows[1][i] + p[2] * rows[2][i])
}

/// The lift of an airfoil (`calculateLift`): v the air's velocity relative to the wing, `n` its
/// normal (y axis). The angle of attack α = asin(v̂·n) gives a "stall" term peaking at 22.5°
/// and gone past 45°; lift = (v̂·n/4 + 3/4·stall) · |v|² · coef / 400.
pub fn lift(v: V3, n: V3, coef: f32) -> f32 {
    let s = dot(v, v).sqrt();
    if s <= 0.0 {
        return 0.0;
    }
    let u = v.map(|c| c / s);
    let c = dot(u, n).clamp(-1.0, 1.0);
    let a = c.asin().to_degrees();
    let stall = if a.abs() >= 45.0 { 0.0 } else { ((if a < 0.0 { a + 45.0 } else { 45.0 - a }) * a) / 506.25 };
    (c * 0.25 + stall * 0.75) * s * s * coef * 0.0025
}

/// `c_PIMouseLook`: held, a plane's camera turns.
const MOUSE_LOOK: usize = 11;

/// The axes of a `RotationalBundle` (yaw, pitch, roll) and how input moves them.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Bundle {
    pub min_rotation: V3,
    pub max_rotation: V3,
    pub max_speed: V3,
    pub acceleration: V3,
    pub continuous_speed: V3,
    /// input index per axis (`setInputToYaw/Pitch/Roll`), None: no input
    pub input_to: [Option<usize>; 3],
    pub automatic_reset: bool,
    /// `rememberExcessInput` (the planes' wings): input past ±1 is kept and spent in the next
    /// updates
    pub remember_excess_input: bool,
    // state
    pub angle: V3,
    pub speed: V3,
    pub input: V3,
    /// the input kept, within ±40
    pub excess: V3,
    /// a seat's `Camera` with `toggleMouseLook` (the planes'): it turns only while `c_PIMouseLook` is held, and else
    /// goes back by 0.75 of its angle each update (the camera's player input handling, `_DAT_086ba8cc`)
    #[serde(default)]
    pub spring_back: bool,
}

impl Bundle {
    /// The rotating bundle's calculate and clip angle for one axis.
    pub fn move_axis(&mut self, i: usize, dt: f32) {
        let mut x = self.input[i];
        let e = &mut self.excess[i];
        if !self.remember_excess_input || *e * x < 0.0 {
            *e = 0.0;
        } else {
            // the kept input and this one, spent ±1 at a time
            *e = (*e + x).clamp(-40.0, 40.0);
            x = e.clamp(-1.0, 1.0);
            *e -= x;
        }
        let acc = self.acceleration[i];
        let signed = if acc > 0.0 { x } else if acc < 0.0 { -x } else { 0.0 };
        if !self.automatic_reset {
            let target = signed * self.max_speed[i];
            let s = &mut self.speed[i];
            if target > *s {
                *s = (*s + acc.abs() * dt).min(target);
            } else if target < *s {
                *s = (*s - acc.abs() * dt).max(target);
            }
            self.angle[i] += self.speed[i] * dt;
            self.angle[i] += dt * self.continuous_speed[i];
        } else {
            let target = signed * self.max_rotation[i];
            let a = &mut self.angle[i];
            if target > *a {
                *a = (*a + acc.abs() * dt).min(target);
            } else if target < *a {
                *a = (*a - acc.abs() * dt).max(target);
            }
        }
        let (lo, hi) = (self.min_rotation[i], self.max_rotation[i]);
        if lo == 0.0 && hi == 0.0 {
            if self.angle[i] > 180.0 {
                self.angle[i] -= 360.0;
            }
            if self.angle[i] < -180.0 {
                self.angle[i] += 360.0;
            }
        } else {
            self.angle[i] = self.angle[i].clamp(lo, hi);
        }
    }

    /// The bundle's own rotation from its angles (local rows).
    pub fn rotation(&self) -> M3 {
        ypr(self.angle)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Wing {
    pub wing_lift: f32,
    pub flap_lift: f32,
    pub pitch_offset: f32,
    pub position_offset: V3,
    /// `setRegulateToLift`: air brakes steer their own flap to hold this lift
    pub regulate_to_lift: Option<f32>,
    pub wing_to_regulator_ratio: f32,
    /// the regulator's output, the flap input of the next update
    pub regulator: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Engine {
    pub engine_type: u32,
    pub torque: f32,
    pub differential: f32,
    pub number_of_gears: i32,
    pub gear_up: f32,
    pub gear_down: f32,
    pub gear_change_time: f32,
    pub no_propeller_effect_at_speed: f32,
    /// torque curve over rpm 0..1 and gear ratio curve over gear/gears, 101 points each
    pub torque_curve: Vec<f32>,
    pub gear_curve: Vec<f32>,
    // state (PhysicsEngine)
    pub rpm: f32,
    pub load: f32,
    pub load_prev: f32,
    pub load_n: f32,
    pub gear: i32,
    pub gear_timer: f32,
    pub on: bool,
    /// steering for tracks: the yaw angle over its maximum
    pub steer: f32,
    /// the throttle asks for the other way than the engine turns (braking): the wheels
    /// are not driven
    pub braking: bool,
    /// `heatIncrement` (−1: none) and `coolingFactor` (0): a guided
    /// missile's fuel (XPack2's Wasserfall, DC's Patriot, SA-3, AIM-54 and Tomahawk)
    pub heat_increment: f32,
    pub cooling: f32,
    /// the heat; at 1 the engine stops until someone gets in again
    pub heat: f32,
    pub burnt: bool,
}

/// A piecewise-linear curve of 101 points from its keys (`OverTimeDistribution`).
pub fn curve(keys: &[(usize, f32)]) -> Vec<f32> {
    let mut out = vec![0.0f32; 101];
    let first = keys[0];
    for i in 0..=first.0 {
        out[i] = first.1;
    }
    for w in keys.windows(2) {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        for i in a..=b {
            out[i] = va + (vb - va) * (i - a) as f32 / (b - a) as f32;
        }
    }
    let last = keys[keys.len() - 1];
    for v in out.iter_mut().skip(last.0) {
        *v = last.1;
    }
    out
}

impl Engine {
    fn default_torque_curve() -> Vec<f32> {
        curve(&[(0, 0.7), (10, 0.8), (30, 0.9), (60, 1.0), (85, 0.85), (100, 0.7)])
    }

    fn default_gear_curve() -> Vec<f32> {
        curve(&[(20, 3.5), (40, 2.2), (60, 1.5), (80, 1.1), (100, 0.94)])
    }

    /// `getCurrentTorque`: the torque curve at |rpm| (≤ 1), times `torque`.
    pub fn current_torque(&self) -> f32 {
        let r = self.rpm.abs().min(1.0) * 100.0;
        let i = (r.round() as usize).min(99);
        let f = r - i as f32;
        ((1.0 - f) * self.torque_curve[i] + f * self.torque_curve[(i + 1).min(100)]) * self.torque
    }

    /// `getCurrentRatio`: 3.5 · differential / the gear curve at gear/gears.
    pub fn current_ratio(&self) -> f32 {
        let r = self.gear as f32 / self.number_of_gears as f32 * 100.0;
        let i = (r.round() as usize).min(100);
        let f = r - i as f32;
        let next = self.gear_curve[(i + 1).min(100)];
        3.5 * self.differential / ((1.0 - f) * self.gear_curve[i] + f * next)
    }

    /// The physics engine's feedback loop: the load the thrust puts on the engine.
    pub fn feedback(&mut self, thrust: V3, axis: V3) {
        let mut x = dot(thrust, axis) * self.current_ratio() / self.current_torque();
        if self.engine_type & 2 != 0 {
            x = x.clamp(-1.0, 1.0);
        }
        if self.engine_type & 4 == 0 {
            let n = self.load_n;
            self.load_n = n + 1.0;
            self.load = ((self.load * n + x) * 0.99) / self.load_n;
        } else if (self.rpm >= 0.0 && x > self.load) || (self.rpm < 0.0 && x < self.load) {
            self.load = x;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum Kind {
    Wing(Wing),
    Engine(Engine),
    Floating(Floating),
    /// a `FireArms`: where its barrel is
    Weapon(String),
    /// a `Camera` (weapons with `fireInCameraDof` fire from it)
    Camera,
    LandingGear(LandingGear),
    Other,
}

/// `LandingGear`: a `RotationalBundle` whose input on
/// every axis is +1 (up) or −1 (down), from its height over the ground or the water, whichever is
/// nearer, and the rpm of the first engine among its ancestors (DC hangs it under the engine;
/// without one, `gearDownEngineInput`). Its wheels turn with it into the body.
#[derive(Debug, Clone, Serialize)]
pub struct LandingGear {
    /// `gearDownHeight` (30) and `gearDownEngineInput` (0.65): down under that
    /// height with at most that rpm
    pub down_height: f32,
    pub down_input: f32,
    /// `gearUpHeight` (20) and `gearUpEngineInput` (0.85): up over that height
    /// with at least that rpm
    pub up_height: f32,
    pub up_input: f32,
    /// whether it is on (down when built)
    pub up: bool,
}

impl LandingGear {
    /// The state after one update, given the height over the ground or water and the engine's rpm.
    pub fn decide(&mut self, height: f32, rpm: f32) {
        if height < self.down_height && rpm <= self.down_input {
            self.up = false;
        }
        if self.up_height < height && self.up_input <= rpm {
            self.up = true;
        }
    }
}

/// `FloatingBundle`: a point of the hull that
/// floats.
#[derive(Debug, Clone, Serialize)]
pub struct Floating {
    pub hull_height: f32,
    pub max_lift: f32,
    pub min_lift: f32,
    /// added to the height of the water it sees (sinking grows it)
    pub water_offset: f32,
    /// the node it hangs from when that is not the root: the damping uses its drag, mass and box
    /// (`updatePhysics` asks its parent node; GC's podracer floats under a RotationalBundle
    /// with drag 0, so nothing damps it: a reference run of the original)
    pub node: Option<FloatNode>,
}

/// A floater's parent node: its template's `drag` (0 unless set) and `mass` (1), and the x and z
/// sides of its geometry's box (`World::craft` fills them; None: no geometry, no push).
#[derive(Debug, Clone, Serialize)]
pub struct FloatNode {
    pub template: String,
    pub drag: f32,
    pub mass: f32,
    pub sides: Option<[f32; 2]>,
}

impl Floating {
    /// The vertical acceleration on the root at the floater, given its height `y`, the water
    /// level, the vertical speed there and the root's drag, mass and box (x and z sides).
    pub fn push(&self, y: f32, water: f32, vy: f32, drag: f32, mass: f32, dx: f32, dz: f32, input: f32) -> Option<f32> {
        let h = self.hull_height as f64;
        let s = ((y as f64 - h - water as f64) + self.water_offset as f64) / h;
        let k = (-(s * h - input as f64)).clamp(0.0, 1.0);
        let s = s as f32;
        if s >= 0.0 {
            return None;
        }
        let s = s.max(-1.0);
        let fv = ((1.0 - s) + s * 25.0) * 100.0;
        let lift = (crate::rigid::GRAVITY_Y as f64 * (-s * ((1.0 - k) * self.min_lift as f64 + k * self.max_lift as f64) as f32) as f64) / -9.82f32 as f64;
        let damp = (drag as f64 * (fv * dz * dx) as f64) as f32 as f64 / mass as f64;
        Some((damp * vy as f64 + lift as f32 as f64) as f32)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Part {
    pub name: String,
    /// the nearest moving ancestor (a part), None: the body
    pub parent: Option<usize>,
    /// placement in that ancestor's frame (static placements composed down the tree)
    pub pos: V3,
    pub rows: M3,
    /// `PivotPosition` of a RotationalBundle: it turns about this point
    pub pivot: V3,
    pub bundle: Option<Bundle>,
    pub kind: Kind,
    /// the PlayerControlObject it belongs to, as the vehicle's seats count them (0: the root's;
    /// a gunner's bundles and weapons read their occupant's input)
    pub seat: usize,
}

/// Where a part is in the world at one instant.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// the axes its physics uses (a wing: turned by its effective flap angle)
    pub rows: M3,
    /// the axes its children hang from
    pub child_rows: M3,
    pub pos: V3,
}

#[derive(Debug, Clone, Serialize)]
pub struct Craft {
    pub body: RigidBody,
    pub parts: Vec<Part>,
    /// player inputs by `c_PI*` index (the driver's), and those of the other seats (1..)
    pub inputs: Vec<f32>,
    pub seat_inputs: Vec<Vec<f32>>,
    /// what each part added in the last update: (part, world point, acceleration)
    pub last_adds: Vec<(String, V3, V3)>,
    /// the level's water height (floaters need it)
    pub water_level: Option<f32>,
    /// a floater lifted it in the last update (touching the water)
    pub afloat: bool,
}

fn num(c: &Component, n: &str, d: f32) -> f32 {
    c.param(n).and_then(Value::number).map_or(d, |v| v as f32)
}

fn vec3(c: &Component, n: &str, d: V3) -> V3 {
    match c.param(n) {
        Some(Value::Vector(v)) if v.len() == 3 => [v[0] as f32, v[1] as f32, v[2] as f32],
        Some(Value::Number(x)) => [*x as f32, d[1], d[2]],
        _ => d,
    }
}

fn input_index(c: &Component, n: &str) -> Option<usize> {
    match c.param(n)? {
        Value::Text(t) if !t.is_empty() => INPUTS.iter().position(|i| i.eq_ignore_ascii_case(t)),
        Value::Number(x) => Some(*x as usize),
        _ => None,
    }
}

fn engine_type(c: &Component) -> u32 {
    match c.param("engineType") {
        Some(Value::Text(t)) => match t.to_ascii_lowercase().as_str() {
            "c_etplane" => 1,
            "c_etcar" => 2,
            "c_ettank" => 6,
            "c_etship" => 9,
            "c_etrocket" => 17,
            "c_ettorpedo" => 25,
            _ => 0,
        },
        Some(Value::Number(x)) => *x as u32,
        _ => 0,
    }
}

fn bundle(c: &Component) -> Bundle {
    Bundle {
        min_rotation: vec3(c, "minRotation", [0.0; 3]),
        max_rotation: vec3(c, "maxRotation", [0.0; 3]),
        max_speed: vec3(c, "maxSpeed", [1.0, 0.0, 0.0]),
        acceleration: vec3(c, "acceleration", [0.1, 0.0, 0.0]),
        continuous_speed: vec3(c, "continousRotationSpeed", [0.0; 3]),
        input_to: [input_index(c, "inputToYaw"), input_index(c, "inputToPitch"), input_index(c, "inputToRoll")],
        automatic_reset: num(c, "automaticReset", 0.0) != 0.0,
        remember_excess_input: num(c, "rememberExcessInput", 0.0) != 0.0,
        ..Default::default()
    }
}

/// `seat`: the PlayerControlObject the part is under; `seats`: how many there are so far (in
/// the tree's order, as `entry::Seat::all_from_model` counts them).
#[allow(clippy::too_many_arguments)]
/// Whether a component (not the root) is one of the craft's parts ([`crate::craft::Craft::parts`], in the
/// order of a walk of the tree, parents first).
pub fn is_part(c: &Component) -> bool {
    matches!(c.kind.to_ascii_lowercase().as_str(), "wing" | "engine" | "rotationalbundle" | "landinggear" | "floatingbundle" | "firearms" | "camera")
}

/// `parent`: the component `c` hangs from, None under the root.
#[allow(clippy::too_many_arguments)]
fn collect(c: &Component, parent: Option<&Component>, anchor: Option<usize>, pos: V3, rows: M3, out: &mut Vec<Part>, root: bool, seat: usize, seats: &mut usize) {
    let seat = if !root && c.kind.eq_ignore_ascii_case("playercontrolobject") {
        *seats += 1;
        *seats - 1
    } else {
        seat
    };
    let kind_l = c.kind.to_ascii_lowercase();
    let (b, kind) = match kind_l.as_str() {
        "wing" => (
            Some(bundle(c)),
            Kind::Wing(Wing {
                wing_lift: num(c, "wingLift", 1.0),
                flap_lift: num(c, "flapLift", 0.1),
                pitch_offset: num(c, "pitchOffset", 0.0),
                position_offset: vec3(c, "positionOffset", [0.0; 3]),
                regulate_to_lift: c.param("regulateToLift").and_then(Value::number).map(|v| v as f32),
                wing_to_regulator_ratio: num(c, "wingToRegulatorRatio", 0.0),
                regulator: 0.0,
            }),
        ),
        "engine" => (
            Some(bundle(c)),
            Kind::Engine(Engine {
                engine_type: engine_type(c),
                torque: num(c, "torque", 60.0),
                differential: num(c, "differential", 10.0),
                number_of_gears: (num(c, "numberOfGears", 1.0) as i32).clamp(1, 5),
                gear_up: num(c, "gearUp", 0.7),
                gear_down: num(c, "gearDown", 0.3),
                gear_change_time: num(c, "gearChangeTime", 1.0),
                no_propeller_effect_at_speed: num(c, "noPropellerEffectAtSpeed", 100.0),
                torque_curve: Engine::default_torque_curve(),
                gear_curve: Engine::default_gear_curve(),
                rpm: 0.0,
                load: 0.0,
                load_prev: 0.0,
                load_n: 0.0,
                gear: 1,
                gear_timer: 1.0,
                // off until a player enters (`handleMessage` 4 or 0x13 turn it on, 5, 0x14 and 0x15 off): rpm 0,
                // and its wheels roll free; an empty jeep pushed to 5 m/s rolls as the original's only so (
                // `diff_phys.py --coast`: 6 of 6 at 0.00 m, the Technical 4.8 m with it on)
                on: false,
                steer: 0.0,
                braking: false,
                heat_increment: num(c, "heatIncrement", -1.0),
                cooling: num(c, "coolingFactor", 0.0),
                heat: 0.0,
                burnt: false,
            }),
        ),
        "rotationalbundle" => (Some(bundle(c)), Kind::Other),
        "landinggear" => (
            Some(bundle(c)),
            Kind::LandingGear(LandingGear {
                down_height: num(c, "gearDownHeight", 30.0),
                down_input: num(c, "gearDownEngineInput", 0.65),
                up_height: num(c, "gearUpHeight", 20.0),
                up_input: num(c, "gearUpEngineInput", 0.85),
                up: false,
            }),
        ),
        "floatingbundle" => (
            Some(bundle(c)),
            Kind::Floating(Floating {
                hull_height: num(c, "hullHeight", 0.0),
                max_lift: num(c, "floatMaxLift", 0.0),
                min_lift: num(c, "floatMinLift", 0.0),
                water_offset: 0.0,
                node: parent.map(|q| FloatNode { template: q.name.clone(), drag: num(q, "drag", 0.0), mass: num(q, "mass", 1.0), sides: None }),
            }),
        ),
        "firearms" => (None, Kind::Weapon(c.name.clone())),
        // (a `CameraTemplate` is a `RotationalBundleTemplate`: the seat's mouse look turns it)
        "camera" => (Some(Bundle { spring_back: num(c, "toggleMouseLook", 0.0) != 0.0, ..bundle(c) }), Kind::Camera),
        _ => (None, Kind::Other),
    };
    let (anchor, pos, rows) = if !root && (b.is_some() || !matches!(kind, Kind::Other)) {
        let pivot = vec3(c, "PivotPosition", [0.0; 3]);
        out.push(Part { name: c.name.clone(), parent: anchor, pos, rows, pivot, bundle: b, kind, seat });
        (Some(out.len() - 1), [0.0; 3], IDENTITY)
    } else {
        (anchor, pos, rows)
    };
    // A RotationalBundle right under a LodObject sits at the LodObject's place for the physics: its `setPosition` moves
    // neither its parts nor its collision. GC's gasgano (`GasganoComplex`, 0.5616 m up in its LodObject): the original's
    // floats, engines and wings all push from 0.5616 m lower (`diff_phys` on GC_Podrace, 0.52 m → 0.00), and its first
    // contact jumps come 20–150 steps later. A plain Bundle there keeps it: the AT-AT's `atatdecomplex` (−25.5 m, its
    // LodObject +25.9 m) is hit by the original at its hips, 25 m up, and starts where ours does only with both counted.
    // TODO: why (the RotationalBundle's matrix rebuilt from its angles and pivot under a node that is not a physics
    // node?): the rotating bundle's update handling and how a LodObject's child gets its physics node. No
    // other data has the case; the drawing keeps `setPosition` (not checked against the original)
    let lod = c.kind.eq_ignore_ascii_case("lodobject");
    for ch in &c.children {
        let p = if lod && ch.kind.eq_ignore_ascii_case("rotationalbundle") { [0.0; 3] } else { to_parent(ch.position.map(|x| x as f32), &rows) };
        let local = ypr(ch.rotation.map(|x| x as f32));
        collect(ch, (!root).then_some(c), anchor, [pos[0] + p[0], pos[1] + p[1], pos[2] + p[2]], mul(&local, &rows), out, false, seat, seats);
    }
}

impl Craft {
    /// From a vehicle's component tree and the root's rigid body.
    pub fn new(model: &Component, body: RigidBody) -> Craft {
        let mut parts = Vec::new();
        let mut seats = 1;
        collect(model, None, None, [0.0; 3], IDENTITY, &mut parts, true, 0, &mut seats);
        Craft { body, parts, inputs: vec![0.0; INPUT_COUNT], seat_inputs: vec![vec![0.0; INPUT_COUNT]; seats - 1], last_adds: Vec::new(), water_level: None, afloat: false }
    }

    pub fn set_input(&mut self, name: &str, v: f32) -> bool {
        match INPUTS.iter().position(|i| !name.is_empty() && i.eq_ignore_ascii_case(name)) {
            Some(i) => {
                self.inputs[i] = v;
                true
            }
            None => false,
        }
    }

    /// Every part's frame in the world, parents before children (the order of `parts`).
    pub fn frames(&self) -> Vec<Frame> {
        self.frames_from(self.body.rows, self.body.pos)
    }

    /// Every part's frame in the root's (the body's own axes and origin), as the physics sees
    /// them: an engine's turning (its roll with the throttle) does not carry the wheels under it.
    pub fn frames_in_root(&self) -> Vec<Frame> {
        self.frames_from_with(IDENTITY, [0.0; 3], false, true)
    }

    /// [`Craft::frames`] with the seats' cameras as their turret holds them, without their own mouse look (the
    /// Kubelwagen's driver's, the Lcvp's): what our bots aim and drive from. A bot's look input turned the Lcvp's
    /// driver's camera to 170° and it drove from there.
    // TODO: a seat whose `fireInCameraDof` weapon fires from a camera that turns by itself (an axis with an input and a speed)
    // shoots off where the bot aims: 57 of 290 seats, all DC and
    // all `CallArtillary`/`AltCallArtillary` (binoculars, which our bots do not use) but the BRDM2 Spandrel's and SA-9 Gaskin's
    // launchers (seat 1). None in vanilla (the Daihatsu's MG42 camera has `setMaxSpeed 0/0/0`). When DC's bots get there:
    // aim from `frames`'s camera, or hold its look axes.
    pub fn frames_still_cameras(&self) -> Vec<Frame> {
        self.frames_from_with(self.body.rows, self.body.pos, true, false)
    }

    fn frames_from(&self, root_rows: M3, root_pos: V3) -> Vec<Frame> {
        self.frames_from_with(root_rows, root_pos, true, true)
    }

    fn frames_from_with(&self, root_rows: M3, root_pos: V3, engines_turn: bool, cameras_turn: bool) -> Vec<Frame> {
        let mut out: Vec<Frame> = Vec::with_capacity(self.parts.len());
        // what each part's child parts hang from: a plane's engine turns its own geometry and
        // what is no part (the propeller), not the parts below it (the gear, its hatches, the
        // F-14B's wings), which the original shows still
        let mut hangs: Vec<(M3, V3)> = Vec::with_capacity(self.parts.len());
        for p in &self.parts {
            let (prows, ppos) = match p.parent {
                Some(k) => hangs[k],
                None => (root_rows, root_pos),
            };
            let placed = mul(&p.rows, &prows);
            let (rows, child_rows, origin) = match (&p.bundle, &p.kind) {
                (Some(b), Kind::Wing(w)) => {
                    let eff = (b.angle[1] * w.flap_lift) / (w.flap_lift + w.wing_lift) - w.pitch_offset;
                    (mul(&rot_x(eff), &placed), mul(&rot_x(b.angle[1]), &placed), p.pos)
                }
                (Some(_), Kind::Engine(_)) if !engines_turn => (placed, placed, p.pos),
                // a car's or a tank's engine (`c_ETCar` 2, `c_ETTank` 6) rolls about the body's
                // length: its wheels hang from it and would circle the hood
                (Some(_), Kind::Engine(e)) if e.engine_type & 2 != 0 => (placed, placed, p.pos),
                (Some(_), Kind::Camera) if !cameras_turn => {
                    let pv = to_parent(p.pivot, &p.rows);
                    (placed, placed, [p.pos[0] + pv[0], p.pos[1] + pv[1], p.pos[2] + pv[2]])
                }
                (Some(b), _) => {
                    let r = b.rotation();
                    let pv = to_parent(to_parent(p.pivot, &r), &p.rows);
                    let rows = mul(&r, &placed);
                    (rows, rows, [p.pos[0] + pv[0], p.pos[1] + pv[1], p.pos[2] + pv[2]])
                }
                _ => (placed, placed, p.pos),
            };
            let w = to_parent(origin, &prows);
            let pos = [ppos[0] + w[0], ppos[1] + w[1], ppos[2] + w[2]];
            hangs.push(match p.kind {
                Kind::Engine(_) => {
                    let w = to_parent(p.pos, &prows);
                    (placed, [ppos[0] + w[0], ppos[1] + w[1], ppos[2] + w[2]])
                }
                _ => (child_rows, pos),
            });
            out.push(Frame { rows, child_rows, pos });
        }
        out
    }

    /// A point given in a part's parent frame, offset from the part's placement, in the world.
    fn point_near(&self, frames: &[Frame], p: &Part, offset: V3) -> V3 {
        let (prows, ppos) = match p.parent {
            Some(k) => (frames[k].child_rows, frames[k].pos),
            None => (self.body.rows, self.body.pos),
        };
        let w = to_parent([p.pos[0] + offset[0], p.pos[1] + offset[1], p.pos[2] + offset[2]], &prows);
        [ppos[0] + w[0], ppos[1] + w[1], ppos[2] + w[2]]
    }

    /// One update. `ground(x, z)`: the terrain height (None: no terrain).
    pub fn step(&mut self, dt: f32, ground: &dyn Fn(f32, f32) -> Option<f32>) {
        self.update_parts(dt, ground);
        // 3. the root moves
        self.body.step(dt);
    }

    /// The parts' `handleUpdate`: inputs, angles, engines, and what they add to the root.
    pub fn update_parts(&mut self, dt: f32, ground: &dyn Fn(f32, f32) -> Option<f32>) {
        self.handle_parts(dt, ground);
        self.push_parts(ground);
    }

    /// 1. input and angles (handlePlayerInput + handleUpdate): the bundles move, the engines' rpm and
    /// gears; no forces. (An object made in the middle of a frame gets its engines' update with them off,
    /// [`crate::Sim::creation_frame`]: the UH-60's rotor at 0.028 in the original's first record, one rpm update on a
    /// throttle bundle moved twice.)
    pub fn handle_parts(&mut self, dt: f32, ground: &dyn Fn(f32, f32) -> Option<f32>) {
        let inputs = self.inputs.clone();
        let seat_inputs = self.seat_inputs.clone();
        // a landing gear's height (where it was after the last update) and its engine (the first among its ancestors)
        let gears: Vec<(usize, f32, Option<usize>)> = if self.parts.iter().any(|p| matches!(p.kind, Kind::LandingGear(_))) {
            // (as the physics places them: an engine's turning does not carry the gear under it)
            let frames = self.frames_from_with(self.body.rows, self.body.pos, false, true);
            let parts = &self.parts;
            parts
                .iter()
                .enumerate()
                .filter(|(_, p)| matches!(p.kind, Kind::LandingGear(_)))
                .map(|(k, p)| {
                    let at = frames[k].pos;
                    let over = |h: Option<f32>| h.map_or(f32::INFINITY, |h| at[1] - h);
                    let height = over(ground(at[0], at[2])).min(over(self.water_level));
                    let mut up = p.parent;
                    let engine = loop {
                        match up.map(|a| (a, &parts[a])) {
                            Some((a, Part { kind: Kind::Engine(_), .. })) => break Some(a),
                            Some((_, q)) => up = q.parent,
                            None => break None,
                        }
                    };
                    (k, height, engine)
                })
                .collect()
        } else {
            Vec::new()
        };
        // the engines' rpm as this pass leaves them: a part's handleUpdate comes after its ancestors', so a gear reads
        // its engine's rpm of this frame (`getCurrentDifferentialRPM(0)`): the F-15C's and Su-25's gear
        // folded a frame late in the air, where the rpm and not the height decides)
        let mut rpm_now: Vec<f32> = self.parts.iter().map(|p| if let Kind::Engine(e) = &p.kind { e.rpm } else { 0.0 }).collect();
        for (k, p) in self.parts.iter_mut().enumerate() {
            let Some(b) = p.bundle.as_mut() else { continue };
            let inputs = if p.seat == 0 { &inputs } else { &seat_inputs[p.seat - 1] };
            for i in 0..3 {
                b.input[i] = b.input_to[i].map_or(0.0, |k| inputs.get(k).copied().unwrap_or(0.0));
            }
            match &mut p.kind {
                Kind::Wing(w) => {
                    if w.regulate_to_lift.is_some() {
                        b.input[1] = w.regulator;
                    }
                    b.move_axis(1, dt);
                }
                Kind::Engine(e) => {
                    if e.engine_type & 16 != 0 {
                        b.input[2] = 1.0;
                    }
                    for i in 0..3 {
                        if b.max_speed[i] != 0.0 {
                            b.move_axis(i, dt);
                        }
                    }
                    e.gear_timer = if e.gear_timer > 0.0 { (e.gear_timer - dt / e.gear_change_time).max(0.0) } else { e.gear_timer };
                    // braking: throttle against the turning (±0.1 dead zone), on the rpm before this update
                    let throttle = b.input[2];
                    e.braking = (throttle < -0.1 && e.rpm > 0.0) || (throttle > 0.1 && e.rpm < 0.0);
                    e.steer = if b.max_rotation[0] > 0.0 { b.angle[0] / b.max_rotation[0] } else { 0.0 };
                    let target = b.angle[2] / b.max_rotation[2];
                    let mut rpm = ((target - e.load) - e.rpm * 0.5) * 0.05 + e.rpm;
                    if !e.on || e.burnt {
                        rpm = 0.0;
                    }
                    rpm = rpm.clamp(-1.0, 1.2);
                    // The engine's heat: |rpm| × heatIncrement in, coolingFactor out;
                    // at 1 the engine stops (and the rpm with it)
                    if e.heat_increment > 0.0 {
                        e.heat += rpm.abs() * e.heat_increment - e.cooling;
                        if e.heat < 1.0 {
                            e.heat = e.heat.max(0.0);
                        } else {
                            e.burnt = true;
                            e.heat = 1.0;
                            rpm = 0.0;
                        }
                    }
                    e.rpm = rpm;
                    rpm_now[k] = rpm;
                    if rpm > e.gear_up && e.gear_timer == 0.0 && e.gear < e.number_of_gears {
                        e.gear += 1;
                    }
                    if e.rpm < e.gear_down && e.gear > 1 {
                        e.gear -= 1;
                    }
                    e.load_prev = e.load;
                    e.load = 0.0;
                    e.load_n = 0.0;
                }
                Kind::Floating(_) => {
                    // a floating bundle: only the pitch axis (diving), if it moves
                    if b.max_speed[1] > 0.0 {
                        b.move_axis(1, dt);
                    }
                }
                Kind::Camera => {
                    // (its pivot, as any bundle's: without it the Kubelwagen's driver looked from 0,25 m lower and
                    // 0,3 m back than the original)
                    if b.spring_back && inputs.get(MOUSE_LOOK).copied().unwrap_or(0.0) <= 0.5 {
                        b.angle = b.angle.map(|a| a * 0.75);
                        b.speed = [0.0; 3];
                    } else {
                        for i in 0..3 {
                            if b.input_to[i].is_some() && b.max_speed[i] != 0.0 {
                                b.move_axis(i, dt);
                            }
                        }
                    }
                }
                Kind::Weapon(_) => {}
                Kind::LandingGear(g) => {
                    if let Some(&(_, height, engine)) = gears.iter().find(|x| x.0 == k) {
                        g.decide(height, engine.map_or(g.down_input, |e| rpm_now[e]));
                    }
                    b.input = [if g.up { 1.0 } else { -1.0 }; 3];
                    for i in 0..3 {
                        b.move_axis(i, dt);
                    }
                }
                Kind::Other => {
                    for i in 0..3 {
                        b.move_axis(i, dt);
                    }
                }
            }
        }
    }

    /// 2. the components push the root (`updatePhysics` of each part).
    fn push_parts(&mut self, ground: &dyn Fn(f32, f32) -> Option<f32>) {
        self.push_parts_of(ground, false);
    }

    /// Only the floaters (`FloatingBundle`) push the root: the original's first update of an object made inside another
    /// body's integration (the reference harness) has its floats and none of its other forces, gravity included (GC's
    /// floats that cancel gravity exactly kept that step for good: the capital ships of GC_Vanguard 0.8 m apart).
    pub fn push_floats(&mut self) {
        self.push_parts_of(&|_, _| None, true);
    }

    fn push_parts_of(&mut self, ground: &dyn Fn(f32, f32) -> Option<f32>, only_floats: bool) {
        let g_scale = GRAVITY_Y / LIFT_GRAVITY;
        let mut adds: Vec<(usize, V3, V3)> = Vec::new();
        let mut afloat = false;
        let mut regulators: Vec<(usize, f32)> = Vec::new();
        let mut rpms: Vec<(usize, f32)> = Vec::new();
        let mut feedback: Vec<(usize, V3, V3)> = Vec::new();
        let frames = self.frames();
        for (k, p) in self.parts.iter().enumerate() {
            if only_floats && !matches!(p.kind, Kind::Floating(_)) {
                continue;
            }
            match &p.kind {
                Kind::Wing(w) => {
                    let rows = frames[k].rows;
                    let pos = self.point_near(&frames, p, w.position_offset);
                    // over or under the water (`getWaterLevel`, not the terrain)
                    let above = match self.water_level {
                        Some(w) => w < pos[1],
                        None => ground(pos[0], pos[2]).is_none_or(|h| h < pos[1]),
                    };
                    let mut v = self.body.velocity_at(pos);
                    if above {
                        v = [v[0] - self.body.wind[0], v[1] - self.body.wind[1], v[2] - self.body.wind[2]];
                    }
                    let coef = (w.flap_lift + w.wing_lift) * g_scale;
                    let mut l = lift(v, rows[1], coef);
                    l = if above { l * (1.0 - (pos[1] / AIR_HEIGHT).clamp(0.0, 1.0)) } else { l * 10.0 };
                    let l = -l.clamp(-MAX_LIFT, MAX_LIFT);
                    adds.push((k, pos, rows[1].map(|c| c * l)));
                    if let Some(target) = w.regulate_to_lift {
                        // the lift it would have flying straight at this speed, against the target
                        let s = dot(v, v).sqrt();
                        let vz = self.body.rows[2].map(|c| c * s);
                        let l2 = lift(vz, rows[1], coef);
                        let prows = p.parent.map_or(self.body.rows, |q| frames[q].child_rows);
                        let neutral_rows = mul(&rot_x(-w.pitch_offset), &mul(&p.rows, &prows));
                        let neutral = lift(vz, neutral_rows[1], coef);
                        // TODO: equal to 1e-4 while the state is, but a high-gain loop swinging with period 2: the Mirage's
                        // wings part from the original's at step 14 with no contact (1.6 m at 8 s), the Bf109's after a
                        // bounce (its 4 mm). Floating point (the original's x87)? How: this in f64 against
                        // the Mirage's air reference run
                        let r = (-(neutral * w.wing_to_regulator_ratio + l2 + target) * g_scale).clamp(-1.0, 1.0);
                        regulators.push((k, r));
                    }
                }
                Kind::Floating(f) => {
                    let Some(water) = self.water_level else { continue };
                    let pos = frames[k].pos;
                    let (drag, mass, [dx, dz]) = match &f.node {
                        Some(n) => {
                            let Some(sides) = n.sides else { continue };
                            (n.drag, n.mass, sides)
                        }
                        None => {
                            let Some([lo, hi]) = self.body.bbox else { continue };
                            (self.body.drag, self.body.mass, [hi[0] - lo[0], hi[2] - lo[2]])
                        }
                    };
                    let input = -p.bundle.as_ref().map_or(0.0, |b| b.angle[1]);
                    let vy = self.body.velocity_at(pos)[1];
                    if let Some(a) = f.push(pos[1], water, vy, drag, mass, dx, dz, input) {
                        adds.push((k, pos, [0.0, a, 0.0]));
                        afloat = true;
                    }
                }
                Kind::Engine(e) if e.engine_type & 1 != 0 => {
                    let rows = frames[k].rows;
                    let pos = frames[k].pos;
                    // The physics engine's physics update and the water (getWaterLevel at the engine): under
                    // it an engine that is not a ship's (8) stops dead; above it a ship's propeller
                    // turning (|rpm| > 0.02) races at rpm 1 and pushes nothing (a torpedo leaping)
                    let below = self.water_level.is_some_and(|w| pos[1] < w);
                    if e.engine_type & 8 == 0 && below {
                        rpms.push((k, 0.0));
                        continue;
                    }
                    if e.engine_type & 8 != 0 && !below && e.rpm.abs() as f64 > 0.02 {
                        rpms.push((k, 1.0));
                        continue;
                    }
                    // propeller: thrust along the engine's z axis, less as the plane outruns it
                    let axis = rows[2];
                    let h = (pos[1] / AIR_HEIGHT).clamp(0.0, 1.0);
                    let f = e.rpm - (dot(self.body.vel, axis) * (1.0 - h)) / e.no_propeller_effect_at_speed;
                    let thrust = e.rpm.abs() * 0.1 + f.abs() * f;
                    feedback.push((k, axis.map(|c| c * thrust), axis));
                    adds.push((k, pos, axis.map(|c| c * thrust * e.current_ratio())));
                }
                _ => {}
            }
        }
        for (k, r) in regulators {
            if let Kind::Wing(w) = &mut self.parts[k].kind {
                w.regulator = r;
            }
        }
        for (k, rpm) in rpms {
            if let Kind::Engine(e) = &mut self.parts[k].kind {
                e.rpm = rpm;
            }
        }
        for (k, thrust, axis) in feedback {
            if let Kind::Engine(e) = &mut self.parts[k].kind {
                e.feedback(thrust, axis);
            }
        }
        self.afloat = afloat;
        self.last_adds.clear();
        // The physics engine's physics update: an engine turning (|rpm| ≥ 0.05) wakes its root (a
        // driver's throttle gets a parked tank going)
        if self.parts.iter().any(|p| matches!(&p.kind, Kind::Engine(e) if e.rpm.abs() as f64 >= ENGINE_WAKE_RPM)) {
            self.body.wake();
        }
        if self.body.is_sleeping() {
            return; // the child nodes of a sleeping root do not push it (they check isSleeping)
        }
        for (k, pos, a) in adds {
            self.body.add_acc_at(pos, a);
            self.last_adds.push((self.parts[k].name.clone(), pos, a));
        }
    }
}

/// The engine's rpm from which it wakes its root (the physics engine's physics update, a double 0.05).
const ENGINE_WAKE_RPM: f64 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lift_peaks_near_22_degrees() {
        let n = [0.0, 1.0, 0.0];
        let at = |deg: f32| lift([0.0, deg.to_radians().sin() * 100.0, deg.to_radians().cos() * 100.0], n, 1.0);
        assert_eq!(at(0.0), 0.0);
        assert!(at(22.5) > at(10.0) && at(22.5) > at(40.0));
        // past 45° only the v̂·n/4 share is left
        assert!((at(60.0) - 60f32.to_radians().sin() * 0.25 * 10000.0 * 0.0025).abs() < 1e-3);
    }

    #[test]
    fn default_curves() {
        let e = Engine::default_torque_curve();
        assert_eq!(e[0], 0.7);
        assert!((e[60] - 1.0).abs() < 1e-6 && (e[100] - 0.7).abs() < 1e-6);
        let g = Engine::default_gear_curve();
        assert!((g[100] - 0.94).abs() < 1e-6 && (g[0] - 3.5).abs() < 1e-6);
    }

    #[test]
    fn bundle_with_reset_goes_to_input_times_max() {
        let mut b = Bundle { max_rotation: [0.0, 0.0, 5000.0], min_rotation: [0.0, 0.0, -3000.0], acceleration: [0.0, 0.0, 1000.0], automatic_reset: true, ..Default::default() };
        b.input[2] = 1.0;
        for _ in 0..30 {
            b.move_axis(2, 1.0 / 30.0);
        }
        assert!((b.angle[2] - 1000.0).abs() < 1e-2);
    }

    /// `rememberExcessInput`: a pull of 2.5 moves the wing as 1, 1 and 0.5 over three updates;
    /// without it, as 2.5 once; the other way, the kept input is dropped.
    #[test]
    fn excess_input_is_kept_and_spent() {
        let mut b = Bundle { max_speed: [0.0, 10.0, 0.0], acceleration: [0.0, 1e6, 0.0], max_rotation: [0.0, 90.0, 0.0], min_rotation: [0.0, -90.0, 0.0], remember_excess_input: true, ..Default::default() };
        let mut speeds = Vec::new();
        for x in [2.5, 0.0, 0.0, 0.0] {
            b.input[1] = x;
            b.move_axis(1, 0.1);
            speeds.push(b.speed[1]);
        }
        assert_eq!(speeds, [10.0, 10.0, 5.0, 0.0]);
        b.input[1] = 3.0;
        b.move_axis(1, 0.1);
        b.input[1] = -0.5;
        b.move_axis(1, 0.1);
        assert_eq!((b.speed[1], b.excess[1]), (-5.0, 0.0));
        b.remember_excess_input = false;
        b.input[1] = 2.5;
        b.move_axis(1, 0.1);
        assert_eq!(b.speed[1], 25.0);
    }

}
