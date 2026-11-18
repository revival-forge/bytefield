//! The engine's rigid physics node (`PhysicsNode`), as read from the original game:
//! what moves vehicles and other mobile objects. In `f32`, like the original.
//!
//! One update (`step`), for a root node that is awake:
//! 1. drag, if `drag > 0`: "advanced" for objects with geometry (sections of the geometry's box
//!    along the three body axes), "simple" otherwise
//! 2. positional physics: clamp, v += dt·a, p += dt·v, empty the accumulators
//! 3. rotational physics: the torque accumulator projected on each body axis and divided by that
//!    axis' inertia gives dω (world frame); the body turns about ω by dt·|ω|
//! 4. gravity (× `gravity_modifier`) goes into the accumulator: it acts from the next update
//!
//! The orientation is a row matrix like the engine's: `rows[0..3]` are the body's x, y, z axes
//! in the world (x right, y up, z forward; left-handed).

use crate::vecmath::dot_zyx;
use serde::Serialize;

/// Gravity the physics system returns, m/s² along y.
pub const GRAVITY_Y: f32 = -14.73;
/// The engine's π (the drag sections use 3.14, not π).
pub const ENGINE_PI: f32 = 3.14;
/// Underwater drag multiplier at full immersion.
pub const UNDERWATER_DRAG: f32 = 25.0;
/// |acceleration| limit, m/s².
pub const MAX_ACC: f32 = 1000.0;
/// The second ("friction") accumulator is dropped when its magnitude passes this.
pub const MAX_FRICTION_ACC: f32 = 250.0;
/// The rotational friction accumulator is dropped past this.
pub const MAX_ROT_FRICTION: f32 = 200.0;
/// Inertia used when the object has no geometry.
pub const NO_GEOMETRY_INERTIA: f32 = 0.0314;
/// Sleep thresholds (squared magnitudes): acceleration, speed, angular speed.
pub const WAKE_ACC2: f32 = 2.5;
pub const WAKE_SPEED2: f32 = 0.25;
/// The physics node's m frames before safe to sleep: quiet updates before a node falls asleep.
pub const FRAMES_BEFORE_SLEEP: i32 = 100;

use crate::vecmath::V3;

fn len2(a: V3) -> f32 {
    a[2] * a[2] + a[1] * a[1] + a[0] * a[0]
}

/// `v` projected on the (not necessarily unit) axis `axis`; zero for a degenerate axis.
fn project(v: V3, axis: V3) -> V3 {
    let l = len2(axis);
    if l == 0.0 || !l.is_finite() {
        return [0.0; 3];
    }
    let k = dot_zyx(v, axis) / l;
    [axis[0] * k, axis[1] * k, axis[2] * k]
}

fn finite(v: V3) -> bool {
    v.iter().all(|x| x.is_finite())
}

#[derive(Debug, Clone, Serialize)]
pub struct RigidBody {
    /// rows: body x, y, z axes in the world
    pub rows: [V3; 3],
    pub pos: V3,
    pub vel: V3,
    /// angular velocity, rad/s, world frame
    pub rotvel: V3,
    /// accumulators (emptied every update)
    pub acc: V3,
    pub rotacc: V3,
    pub friction: V3,
    pub rot_friction: V3,
    /// how many frictions the accumulators average
    pub friction_count: f32,
    /// `ObjectTemplate.drag`
    pub drag: f32,
    /// `ObjectTemplate.mass`, kg (default 1)
    pub mass: f32,
    /// `ObjectTemplate.inertiaModifier` (x, y, z; default 1)
    pub inertia_modifier: V3,
    /// `ObjectTemplate.centerOfMassOffset` (subtracted from world offsets, as the engine does)
    pub center_of_mass: V3,
    /// `ObjectTemplate.gravityModifier` (default 1)
    pub gravity_modifier: f32,
    /// depth under water, m (0 in air)
    pub underwater: f32,
    /// the geometry's box (min, max) in body coordinates; None: no geometry
    pub bbox: Option<[V3; 2]>,
    /// objects whose geometry the engine flags as "simple" get the simple drag
    pub simple_drag: bool,
    pub wind: V3,
    /// updates left before it sleeps: reset by a large acceleration or speed, counted
    /// down while quiet; at 0 the node sleeps (no integration, no collision). Negative: never
    /// checked (soldiers).
    pub sleepiness: i32,
}

impl RigidBody {
    pub fn new(pos: V3, mass: f32, drag: f32, bbox: Option<[V3; 2]>) -> RigidBody {
        RigidBody {
            rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            pos,
            vel: [0.0; 3],
            rotvel: [0.0; 3],
            acc: [0.0; 3],
            rotacc: [0.0; 3],
            friction: [0.0; 3],
            rot_friction: [0.0; 3],
            friction_count: 0.0,
            drag,
            mass,
            inertia_modifier: [1.0; 3],
            center_of_mass: [0.0; 3],
            gravity_modifier: 1.0,
            underwater: 0.0,
            bbox,
            simple_drag: false,
            wind: [0.0; 3],
            sleepiness: FRAMES_BEFORE_SLEEP,
        }
    }

    fn dims(&self) -> Option<V3> {
        self.bbox.map(|[lo, hi]| [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]])
    }

    /// Cross sections used by the drag (xy, zx, yz): the ellipses inscribed in the box faces.
    pub fn sections(&self) -> Option<V3> {
        let [dx, dy, dz] = self.dims()?;
        let x = dx * ENGINE_PI * 0.25;
        Some([x * dy, dz * x, dy * ENGINE_PI * 0.25 * dz])
    }

    /// Inertia per body axis (x, y, z) of the geometry's box, before the modifier; no mass.
    pub fn box_inertia(&self) -> Option<V3> {
        let [dx, dy, dz] = self.dims()?;
        let (x2, y2, z2) = (dx * dx, dy * dy, dz * dz);
        let k = 1.0f32 / 3.0;
        Some([(y2 + z2) * k, (z2 + x2) * k, (x2 + y2) * k])
    }

    /// `addAccelerationAtRelativePosition`: `a` at the offset `rel` from the body's position
    /// (world axes): a goes to the linear accumulator and (rel − centre of mass) × a to the
    /// angular one. Accelerations, not forces: the mass is already in them.
    pub fn add_acc_at_rel(&mut self, rel: V3, a: V3) {
        let c = self.center_of_mass;
        for i in 0..3 {
            self.acc[i] += a[i];
        }
        let (x, y, z) = (rel[0] - c[0], rel[1] - c[1], rel[2] - c[2]);
        self.rotacc[0] += y * a[2] - z * a[1];
        self.rotacc[1] += z * a[0] - x * a[2];
        self.rotacc[2] += x * a[1] - y * a[0];
    }

    /// `addAccelerationAtAbsolutePosition`: the same at a world point.
    /// `wakeUp`: the countdown to sleep starts again (a body that never sleeps stays so).
    pub fn wake(&mut self) {
        if self.sleepiness >= 0 {
            self.sleepiness = FRAMES_BEFORE_SLEEP;
        }
    }

    pub fn add_acc_at(&mut self, pos: V3, a: V3) {
        let rel = [pos[0] - self.pos[0], pos[1] - self.pos[1], pos[2] - self.pos[2]];
        self.add_acc_at_rel(rel, a);
    }

    /// `addFrictionAtAbsolutePosition`: the friction accumulators are means, not sums: of the
    /// frictions and of their moments r × f about the body's position.
    pub fn add_friction_at(&mut self, pos: V3, f: V3) {
        let c = self.friction_count;
        let k = 1.0 / (c + 1.0);
        let lin = [self.friction[0] * c + f[0], self.friction[1] * c + f[1], self.friction[2] * c + f[2]];
        self.friction = [lin[0] * k, lin[1] * k, lin[2] * k];
        let r = [pos[0] - self.pos[0], pos[1] - self.pos[1], pos[2] - self.pos[2]];
        let m = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]];
        let k = 1.0 / (c + 1.0);
        self.rot_friction = [
            (self.rot_friction[0] * c + m[0]) * k,
            (self.rot_friction[1] * c + m[1]) * k,
            k * (self.rot_friction[2] * c + m[2]),
        ];
        self.friction_count = c + 1.0;
    }

    /// The world velocity of the body's point `pos` (`getTangentSpeed`): v + ω × (pos − p).
    pub fn velocity_at(&self, pos: V3) -> V3 {
        let r = [pos[0] - self.pos[0], pos[1] - self.pos[1], pos[2] - self.pos[2]];
        let w = self.rotvel;
        [
            w[1] * r[2] - w[2] * r[1] + self.vel[0],
            w[2] * r[0] - r[2] * w[0] + self.vel[1],
            r[1] * w[0] - r[0] * w[1] + self.vel[2],
        ]
    }

    /// An acceleration at the centre.
    pub fn add_acc(&mut self, a: V3) {
        for i in 0..3 {
            self.acc[i] += a[i];
        }
    }

    pub fn add_rotacc(&mut self, a: V3) {
        for i in 0..3 {
            self.rotacc[i] += a[i];
        }
    }

    /// The share of the drag that comes from being under water (×1 in air, ×25 submerged).
    fn water_factor(&self) -> f32 {
        let h = self.dims().map_or(1.0, |d| d[1]);
        let f = (self.underwater / h).min(1.0);
        f * UNDERWATER_DRAG + (1.0 - f)
    }

    fn drag_advanced(&mut self, s: V3, water: f32) {
        // positional
        let vr = [
            self.vel[0] * water - self.wind[0],
            self.vel[1] * water - self.wind[1],
            self.vel[2] * water - self.wind[2],
        ];
        let k = (len2(vr).sqrt() * -1.0 * self.drag) / self.mass;
        let px = project(vr, self.rows[0]);
        let py = project(vr, self.rows[1]);
        let pz = project(vr, self.rows[2]);
        let (kx, ky, kz) = (k * s[2], k * s[1], k * s[0]);
        for i in 0..3 {
            self.acc[i] += pz[i] * kz + py[i] * ky + px[i] * kx + 0.0;
        }
        // rotational
        let w = self.rotvel;
        let kr = (len2(w).sqrt() * -water * self.drag) / self.mass;
        let axes = [(0, s[1] + s[0]), (1, s[0] + s[2]), (2, s[1] + s[2])];
        for (row, sec) in axes {
            let p = project(w, self.rows[row]);
            let f = sec * kr;
            for i in 0..3 {
                self.rotacc[i] += p[i] * f;
            }
        }
    }

    fn drag_simple(&mut self, positional: f32, _rotational: f32) {
        // (0.1, 1.0): only reached for "simple" geometry.
        let k = -positional * self.drag / self.mass;
        for i in 0..3 {
            self.acc[i] += (self.vel[i] - self.wind[i]) * k;
        }
    }

    fn positional(&mut self, dt: f32) {
        if !finite(self.acc) {
            self.acc = [0.0; 3];
        }
        if !finite(self.friction) {
            self.friction = [0.0; 3];
        }
        let a2 = len2(self.acc);
        if a2 > MAX_ACC * MAX_ACC {
            let f = MAX_ACC / a2.sqrt();
            self.acc = self.acc.map(|a| a * f);
        }
        if len2(self.friction) > MAX_FRICTION_ACC * MAX_FRICTION_ACC {
            self.friction = [0.0; 3];
        }
        for i in 0..3 {
            self.vel[i] += dt * self.acc[i];
        }
        for i in 0..3 {
            self.vel[i] += dt * self.friction[i];
        }
        for i in 0..3 {
            self.pos[i] += dt * self.vel[i];
        }
        self.acc = [0.0; 3];
        self.friction = [0.0; 3];
    }

    fn rotational(&mut self, dt: f32) {
        if !finite(self.rot_friction) {
            self.rot_friction = [0.0; 3];
        }
        if !finite(self.rotacc) {
            self.rotacc = [0.0; 3];
        }
        let a2 = len2(self.rotacc);
        if a2 > MAX_ACC * MAX_ACC {
            let f = MAX_ACC / a2.sqrt();
            self.rotacc = self.rotacc.map(|a| a * f);
        }
        let mut dw = [0.0f32; 3];
        match self.box_inertia() {
            Some(inertia) => {
                let torque = [
                    self.rotacc[0] + self.rot_friction[0],
                    self.rotacc[1] + self.rot_friction[1],
                    self.rotacc[2] + self.rot_friction[2],
                ];
                if len2(self.rot_friction) > MAX_ROT_FRICTION * MAX_ROT_FRICTION {
                    self.rot_friction = [0.0; 3];
                }
                for axis in [2usize, 1, 0] {
                    let p = project(torque, self.rows[axis]);
                    let k = dt / (self.inertia_modifier[axis] * inertia[axis]);
                    for i in 0..3 {
                        dw[i] += p[i] * k;
                    }
                }
                if !finite(dw) {
                    dw = [0.0; 3];
                }
            }
            None => {
                let k = dt / NO_GEOMETRY_INERTIA;
                for i in 0..3 {
                    dw[i] = (self.rot_friction[i] + self.rotacc[i]) * k;
                }
                if len2(dw) > MAX_ACC * MAX_ACC || !finite(dw) {
                    dw = [0.0; 3];
                }
            }
        }
        // the turn
        // TODO: the F-15C's takeoff still parts at step 45 (1.16 m): with the turn at the bit, the difference comes in dw. How: our dw
        // (PHYS_MAT=1) against the reference run's `domega_bits`, the first record that differs
        let (rotvel, turn) = turn(self.rotvel, dw, dt);
        self.rotvel = rotvel;
        if let Some((axis, deg)) = turn {
            self.rows = crate::ref2::rotate_about_line(&self.rows, axis, deg);
        }
        self.rotacc = [0.0; 3];
        self.rot_friction = [0.0; 3];
        self.friction_count = 0.0;
    }


    pub fn is_sleeping(&self) -> bool {
        self.sleepiness < 1
    }

    /// One the physics node's physics update of a root node, without contacts. First the sleep
    /// check: an acceleration (as accumulated, gravity included) or a speed above the
    /// thresholds keeps it awake; otherwise it counts down, and asleep it stops dead.
    pub fn step(&mut self, dt: f32) {
        if self.sleepiness >= 0 {
            if len2(self.acc) >= WAKE_ACC2 || len2(self.vel) >= WAKE_SPEED2 || len2(self.rotvel) >= WAKE_SPEED2 {
                self.sleepiness = FRAMES_BEFORE_SLEEP;
            } else if self.sleepiness > 0 {
                self.sleepiness -= 1;
            }
        }
        if self.is_sleeping() {
            self.vel = [0.0; 3];
            self.rotvel = [0.0; 3];
            self.acc = [0.0; 3];
            self.rotacc = [0.0; 3];
            self.friction = [0.0; 3];
            self.rot_friction = [0.0; 3];
            self.friction_count = 0.0;
            return;
        }
        if self.drag > 0.0 {
            if self.simple_drag {
                self.drag_simple(0.1, 1.0);
            } else if let Some(s) = self.sections() {
                let w = self.water_factor();
                self.drag_advanced(s, w);
            }
        }
        self.positional(dt);
        self.rotational(dt);
        self.acc[1] += GRAVITY_Y * self.gravity_modifier;
    }
}

/// The engine's rotational update: ω += Δω, and the turn it gives (axis, degrees) when |ω|² > 1e-6. The
/// x87 keeps the sum unrounded on its stack (the stored ω is its f32): the axis comes from it (squares, sum, sqrt, 1/sqrt in
/// extended; double does the same here), the angle is f32(dt·f32(|ω|)·K), K = 0x42652ee0 (an ulp under f32(180/π)).
/// Over the F-15C's five maneuvers 1195 of 1195 turns at the bit.
pub fn turn(rotvel: V3, dw: V3, dt: f32) -> (V3, Option<(V3, f32)>) {
    let sum: [f64; 3] = std::array::from_fn(|i| rotvel[i] as f64 + dw[i] as f64);
    let stored = sum.map(|c| c as f32);
    let w2 = sum.iter().map(|&c| c * c).sum::<f64>();
    if w2 <= 1e-6 {
        return (stored, None);
    }
    let w = w2.sqrt();
    let inv = 1.0 / w;
    let deg = (dt as f64 * (w as f32) as f64 * f32::from_bits(0x4265_2ee0) as f64) as f32;
    (stored, Some((sum.map(|c| (c * inv) as f32), deg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four turns of the original's F-15C rolling (a reference run hook on
    /// `updateRotationalPhysics`: ω before, Δω, dt, and the axis and degrees it passed to `rotateAboutLine`), to the bit.
    #[test]
    fn the_turn_is_the_originals_to_the_bit() {
        let b = |v: [u32; 3]| v.map(f32::from_bits);
        for (w, dw, dt, axis, deg) in [
            ([0x3cd2a8e7, 0xbc88f23c, 0xbee7fa6e], [0x3ac7de7a, 0xbacf0422, 0xbdeb63a8], 0x3d088889, [0x3d441a54, 0xbd03b783, 0xbf7f92ea], 0x3f8b172b),
            ([0x3e8b0a2f, 0x3dfacdc6, 0xbfdf370c], [0x3ba42da1, 0x3bdafcfd, 0xbc8fb759], 0x3d088889, [0x3e1e6cbe, 0x3d93f408, 0xbf7c3d97], 0x405a8202),
            ([0x3f7d2abc, 0x3f592b3a, 0xc01a2c5b], [0x3bfa4cdc, 0x3c264126, 0xbbe208f7], 0x3d088889, [0x3eb97eb0, 0x3e9fc9cd, 0xbf60d5cc], 0x40a81c9b),
            ([0x3fbee796, 0x3fada4de, 0xc01082b7], [0x3bd4eb4e, 0x3b7aaa5c, 0x3bc3f3cd], 0x3d088889, [0x3efd5276, 0x3ee610bb, 0xbf3e6bfb], 0x40b907be),
        ] {
            let (_, t) = turn(b(w), b(dw), f32::from_bits(dt));
            let (a, d) = t.unwrap();
            assert_eq!((a.map(f32::to_bits), d.to_bits()), (axis, deg));
        }
    }

    #[test]
    fn falls_from_the_next_update() {
        let mut b = RigidBody::new([0.0, 100.0, 0.0], 1000.0, 0.0, None);
        b.step(1.0 / 30.0);
        assert_eq!(b.pos[1], 100.0);
        assert_eq!(b.acc[1], GRAVITY_Y);
        b.step(1.0 / 30.0);
        assert!((b.vel[1] - GRAVITY_Y / 30.0).abs() < 1e-6);
        assert!((b.pos[1] - (100.0 + GRAVITY_Y / 900.0)).abs() < 1e-4);
    }

    #[test]
    fn sections_and_inertia_of_a_box() {
        let b = RigidBody::new([0.0; 3], 1.0, 1.0, Some([[-1.0, 0.0, -2.0], [1.0, 1.0, 2.0]]));
        let s = b.sections().unwrap();
        assert!((s[0] - 2.0 * 1.0 * 0.785).abs() < 1e-5);
        assert!((s[1] - 4.0 * 2.0 * 0.785).abs() < 1e-5);
        assert!((s[2] - 1.0 * 4.0 * 0.785).abs() < 1e-5);
        let i = b.box_inertia().unwrap();
        assert!((i[0] - (1.0 + 16.0) / 3.0).abs() < 1e-5);
        assert!((i[2] - (4.0 + 1.0) / 3.0).abs() < 1e-5);
    }

    #[test]
    fn spins_about_the_angular_velocity() {
        let mut b = RigidBody::new([0.0; 3], 1.0, 0.0, None);
        b.rotvel = [0.0, 1.0, 0.0];
        for _ in 0..30 {
            b.step(1.0 / 30.0);
        }
        // one radian about +y: x axis -> (cos 1, 0, -sin 1)
        assert!((b.rows[0][0] - 1f32.cos()).abs() < 1e-4);
        assert!((b.rows[0][2] + 1f32.sin()).abs() < 1e-4);
    }

    #[test]
    fn drag_opposes_motion() {
        let mut b = RigidBody::new([0.0; 3], 100.0, 1.0, Some([[-1.0; 3], [1.0; 3]]));
        b.gravity_modifier = 0.0;
        b.vel = [10.0, 0.0, 0.0];
        b.step(1.0 / 30.0);
        assert!(b.vel[0] < 10.0 && b.vel[0] > 9.0);
    }
}
