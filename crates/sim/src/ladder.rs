//! Ladders. The soldier's collision handling
//! notes the first object it touches with a ladder's material (192–195) while it has none
//!; the next `handlePlayerInput` asks `handleClimbAction`:
//! - with the material of this frame: pressing forward, facing the ladder (its z within 0.8 of
//!   the soldier's, from below its centre; or turned from it, from above: then 2 m down onto it),
//!   and not 0.48 under the terrain, the soldier starts climbing (`Lb_ClimbLadder1`); otherwise it
//!   forgets it;
//! - climbing: it is put on the ladder (`getLadderClosestPosition`, the ladder's axes), moves
//!   along the ladder's y (looking down, forward goes down), without gravity and at the speed
//!   asked (`setVelocity`, not the push), until it jumps, the machine reaches `StopClimbing`,
//!   it passes 0.8 under the top of the geometry's box going up, 1.6 over its bottom going down,
//!   or 0.5 under the terrain going down (`stopClimbing`: off the top, 2 m up and 1 m forward).

use crate::vecmath::V3;
use crate::contact::{Ground, Response};
use crate::rigid::RigidBody;
use crate::soldier::Movement;

/// The materials of a ladder.
pub const MATERIALS: std::ops::RangeInclusive<u32> = 192..=195;
/// How far in front of the ladder's plane the soldier climbs; and how far under the
/// terrain it can be to start.
const OFF: f32 = 0.48;
/// The cosine between the soldier's z and the ladder's that faces it.
const FACING: f32 = 0.8;
/// Off the top: this much under the top of the geometry's box.
const TOP: f32 = 0.8;
/// Off the bottom going down: this much over the bottom of the box.
const BOTTOM: f32 = 1.6;
/// The soldier keeps this far from the box's sides.
const EDGE: f32 = 0.4;
/// Where along the ladder's x the soldier is, not chosen yet.
pub const UNSET: f32 = -4711.0;
/// Onto it from above, and off the top: 2 m.
const STEP: f32 = 2.0;
/// Going down: off when the soldier is this far under the terrain.
const UNDER: f32 = 0.5;
/// `stopClimbing` pushes the soldier down.
const STOP_PUSH: f32 = -100.0;

/// A ladder the soldier touched: its place and axes, the box of its geometry (the `.sm`'s
/// header) and the material of the face.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Ladder {
    pub pos: V3,
    pub rows: [V3; 3],
    pub bounds: [V3; 2],
    pub material: u32,
}

impl Ladder {
    /// A point in the ladder's frame (the inverse of its matrix on `p − pos`).
    fn local(&self, p: V3) -> V3 {
        let d = [p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]];
        let r = &self.rows;
        [
            r[0][0] * d[0] + r[0][1] * d[1] + r[0][2] * d[2],
            r[1][0] * d[0] + r[1][1] * d[1] + r[1][2] * d[2],
            d[0] * r[2][0] + d[1] * r[2][1] + d[2] * r[2][2],
        ]
    }

    /// `getLadderClosestPosition(p, &t, ladder)`: `p` at the soldier's distance in front of the
    /// ladder, at its height along it; across, the first `t` it was given (`UNSET`: `p`'s own),
    /// kept off the box's sides, and remembered in `t`.
    pub fn closest(&self, p: V3, t: &mut f32) -> V3 {
        let d = [p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]];
        let r = &self.rows;
        let y = r[1][0] * d[0] + r[1][1] * d[1] + r[1][2] * d[2];
        let mut x = r[0][2] * d[2] + r[0][1] * d[1] + r[0][0] * d[0];
        if *t != UNSET {
            x = *t;
        }
        let lo = self.bounds[0][0] + EDGE;
        x = if lo <= x {
            let hi = self.bounds[1][0] - EDGE;
            if hi < x { hi } else { x }
        } else {
            lo
        };
        if *t == UNSET {
            *t = x;
        }
        let z = -OFF;
        [
            r[0][0] * x + r[1][0] * y + r[2][0] * z + self.pos[0],
            r[0][1] * x + r[1][1] * y + r[2][1] * z + self.pos[1],
            x * r[0][2] + y * r[1][2] + z * r[2][2] + self.pos[2],
        ]
    }
}

/// An axis of the input as `handleClimbAction` reads it: 0 unmapped or under 0.001.
fn axis_of(inp: &crate::anim::Input, i: usize) -> f32 {
    let v = if inp.mapped(i) { inp.get(i) } else { 0.0 };
    if v.abs() < 0.001 { 0.0 } else { v }
}

impl Movement {
    /// Climbing (`Lb_ClimbLadder*`, `Ub_ClimbLadder*`...).
    pub fn climbing(&self) -> bool {
        self.ladder.is_some()
    }

    /// The soldier's climb action handling: `input` is the frame's copy (its throttle
    /// becomes the climb's: −1 or 1 by the pitch; the jump is dropped when it lets go), `dir`
    /// the soldier's z, the ladder's y while climbing.
    pub(crate) fn handle_climb_action(&mut self, body: &mut RigidBody, r: &mut Response, ground: &dyn Ground, copy: &mut crate::anim::Input, dir: &mut V3) {
        use crate::soldier::axis;
        // the frame's contact (`handleCollision` notes one only without a ladder)
        if let Some(h) = r.ladder.take()
            && self.ladder.is_none()
        {
            self.ladder = Some(h);
            self.ladder_contact = h.material;
        }
        let saved = *dir;
        let mut stop = false;
        let mut action = axis_of(copy, axis::ACTION);
        let m = self.machine.clone();
        let state = |name: &str| m.find(name);
        if let Some(l) = self.ladder {
            let mut thr = axis_of(copy, axis::THROTTLE);
            if self.ladder_contact == 0 {
                stop = self.lower.state.is_some() && self.lower.state == state("StopClimbing");
                if action != 0.0 {
                    stop = true;
                }
            } else {
                if thr > 0.0 {
                    let z = l.rows[2];
                    let f = dir[2] * z[2] + dir[1] * z[1] + dir[0] * z[0];
                    let p = body.pos;
                    let under = ground.height_and_normal(p[0], p[2]).0 as f64 - p[1] as f64;
                    let (y, ly) = (p[1], l.pos[1]);
                    let start = under < OFF as f64 && ((y < ly && f > FACING) || (ly < y && f < -FACING));
                    if start {
                        self.ladder_contact = 0;
                        for (half, name) in [(0, "Lb_ClimbLadder1"), (1, "Ub_ClimbLadder1")] {
                            if let Some(i) = state(name) {
                                if half == 0 { self.lower.set(i) } else { self.upper.set(i) }
                            }
                        }
                        self.head_yaw = 0.0;
                        self.head[1] = 0.0;
                        // from above, turned from it: 2 m down onto it, its way
                        if (f as f64) < -0.8 {
                            let q = l.closest([p[0], p[1] - STEP, p[2]], &mut self.ladder_t);
                            body.rows = l.rows;
                            body.pos = q;
                        }
                    }
                }
                if self.ladder_contact > 0 {
                    self.ladder = None;
                }
            }
            if self.ladder.is_some() && self.ladder_contact == 0 {
                body.pos = l.closest(body.pos, &mut self.ladder_t);
                body.rows = l.rows;
                if thr != 0.0 {
                    if self.pitch < 0.0 {
                        thr = -1.0;
                    }
                    if self.pitch > 0.0 {
                        thr = 1.0;
                    }
                }
                *dir = l.rows[1];
                let y = l.local(body.pos)[1];
                if y < BOTTOM + l.bounds[0][1] && thr < 0.0 {
                    stop = true;
                } else if y > l.bounds[1][1] - TOP {
                    let cur = self.lower.state;
                    let (exit, end) = (state("Lb_ClimbLadderExit"), state("Lb_ClimbLadderEnd1"));
                    if thr > 0.0 && cur != exit && cur != state("StopClimbing") {
                        stop = true;
                    }
                    if cur.is_some() && (cur == exit || cur == end) {
                        thr = 1.0;
                    }
                }
                if thr < 0.0 {
                    let p = body.pos;
                    let under = ground.height_and_normal(p[0], p[2]).0 as f64 - p[1] as f64;
                    if under > UNDER as f64 {
                        stop = true;
                    }
                }
                if copy.mapped(axis::THROTTLE) {
                    copy.set(axis::THROTTLE, thr);
                }
            }
        }
        if stop {
            self.stop_climbing(body);
            action = 0.0;
            *dir = saved;
        }
        if copy.mapped(axis::ACTION) {
            copy.set(axis::ACTION, action);
        }
        self.ladder_contact = 0;
    }

    /// The soldier's stop climbing: off the top, 2 m up and 1 m forward onto it; a push down; the
    /// default states (the template's).
    pub(crate) fn stop_climbing(&mut self, body: &mut RigidBody) {
        if let Some(l) = self.ladder
            && l.local(body.pos)[1] > l.bounds[1][1] - TOP
        {
            let q = l.closest([body.pos[0], body.pos[1] + STEP, body.pos[2]], &mut self.ladder_t);
            let z = body.rows[2];
            body.rows = l.rows;
            body.pos = [z[0] + q[0], z[1] + q[1], z[2] + q[2]];
        }
        body.add_acc_at_rel([0.0; 3], [0.0, STOP_PUSH, 0.0]);
        let m = self.machine.clone();
        if let Some(i) = m.find("Lb_Stand") {
            self.lower.set(i);
        }
        if let Some(i) = m.find("Ub_StandAim") {
            self.upper.set(i);
        }
        self.ladder = None;
        self.ladder_contact = 0;
        self.ladder_t = UNSET;
    }
}

