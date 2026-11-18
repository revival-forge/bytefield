//! A soldier's collision skeleton: the bones the projectiles hit.
//!
//! The server keeps two skeletons per soldier: the animated one and the
//! collision one, which the projectiles test (`SkeletonCollisionMesh`). The collision
//! one is only sampled now and then: the soldier's update handling counts its countdown down and, past 0,
//! sets it to `rand % 30 / 30 + 0.5` s, runs the soldier's visual update (which poses the
//! animated skeleton: `updateAnimations`) and copies the animated bones over. Between samples
//! the bones stay where they were (before the first, in the `.ske`'s own pose).
//!
//! Posing: each body half's animation instance applies its
//! state's animation at its time (`applyOnSkeleton`: the frame from the time's fraction, slerp
//! and lerp to the next one; `setRelativeBoneTransform`, blended by the morph); then the aim
//! turns the spine and the head (the matrices of four bones, rotation only), and
//! [`crate::skeleton::Skeleton::transform`] chains the bones from the root, which goes through `rotate90aroundX`
//! moved by `setCharacterHeight`.
//!
//! The hit: the segment in the soldier's
//! frame against each collision bone's capsule (bone → parent, shifted along itself by the
//! bone's up/down adjustment), in the order `setSkeletonCollisionBone` gave them: the first
//! whose squared distance (`checkCapsuleCollision`) is under its "radius" wins. The point it
//! gives is the bone's own position (in the world), the normal its x axis (not turned into the
//! world), the material the bone's.
//!
//! Matrices are the server's: 4×4 by rows, row vectors (v' = v·M), translation in row 3.

use std::sync::Arc;

use bytefield_formats::anim::{BoneAnimation, Skeleton as SkeletonFile};

use crate::vecmath::V3;

pub type M4 = [f32; 16];

pub const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// The matrix mult: a·b (sums in extended precision, stored as `f32`).
pub fn mult(a: &M4, b: &M4) -> M4 {
    let mut o = [0f32; 16];
    for r in 0..4 {
        for c in 0..3 {
            let mut s = 0f64;
            for k in 0..3 {
                s += b[4 * k + c] as f64 * a[4 * r + k] as f64;
            }
            if r == 3 {
                s += b[12 + c] as f64;
            }
            o[4 * r + c] = s as f32;
        }
    }
    o[15] = 1.0;
    o
}

/// The quaternion to mat (x, y, z, w): the rotation, no translation.
pub fn quat_to_mat(q: [f32; 4]) -> M4 {
    let [x, y, z, w] = q.map(|v| v as f64);
    let s = 2.0 / (w * w + z * z + x * x + y * y);
    let (ys, wxs, zs, xxs) = (y * s, w * x * s, s * z, x * x * s);
    let mut m = IDENTITY;
    m[0] = (1.0 - (ys * y + z * zs)) as f32;
    m[5] = (1.0 - (z * zs + xxs)) as f32;
    m[4] = (x * ys + w * zs) as f32;
    m[1] = (x * ys - w * zs) as f32;
    m[10] = (1.0 - (xxs + ys * y)) as f32;
    m[8] = (x * zs - w * ys) as f32;
    m[2] = (x * zs + w * ys) as f32;
    m[9] = (wxs + y * zs) as f32;
    m[6] = (y * zs - wxs) as f32;
    m
}

/// The quaternion slerp: the shorter way; nearly equal ones (1 − |cos| ≤ 1e-6)
/// linearly.
pub fn slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let d = a[3] * b[3] + d;
    let (d, sign) = if d >= 0.0 { (d, 1.0f32) } else { (-d, -1.0) };
    let (k0, k1) = if 1.0 - d <= 1e-6 {
        (1.0 - t, t)
    } else {
        let om = d.min(1.0).acos();
        let so = om.sin();
        (((1.0 - t) * om).sin() / so, (t * om).sin() / so)
    };
    let k1 = k1 * sign;
    std::array::from_fn(|i| a[i] * k0 + b[i] * k1)
}

/// `setRotateXDeg`, `setRotateYDeg`.
fn rotate_x(deg: f32) -> M4 {
    let (s, c) = ((deg * 0.017_453_292) as f64).sin_cos();
    let mut m = IDENTITY;
    (m[5], m[6], m[9], m[10]) = (c as f32, s as f32, -s as f32, c as f32);
    m
}

fn rotate_y(deg: f32) -> M4 {
    let (s, c) = ((deg * 0.017_453_292) as f64).sin_cos();
    let mut m = IDENTITY;
    (m[0], m[2], m[8], m[10]) = (c as f32, -s as f32, s as f32, c as f32);
    m
}

/// A bone of `setSkeletonCollisionBone name radius adjustment material`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct CollisionBone {
    pub bone: usize,
    /// compared with the squared distance
    pub radius: f32,
    /// the capsule moves along the bone by this many times its length
    pub shift: f32,
    pub material: u32,
}

/// What a segment hit.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Hit {
    pub bone: usize,
    pub material: u32,
    /// the bone's position, in the world
    pub point: V3,
    /// the bone's x axis, in the soldier's frame
    pub normal: V3,
}

/// The aim (pitch, the head's yaw, two more head angles and the lean)
/// and what turns it: whether the state locks the free look and the pose is lying.
#[derive(Debug, Clone, Copy, Default)]
pub struct Aim {
    pub pitch: f32,
    pub yaw: f32,
    pub head: [f32; 2],
    pub lean: f32,
    pub locked: bool,
    pub lying: bool,
}

#[derive(Debug, Clone)]
pub struct Skeleton {
    pub file: Arc<SkeletonFile>,
    /// relative (+8) and absolute matrix of each bone
    pub rel: Vec<M4>,
    pub abs: Vec<M4>,
    pub collision: Vec<CollisionBone>,
    /// `setCharacterHeight`
    pub height: f32,
    /// the template's first spine bone: it and the next two
    /// follow the aim, the fifth after it (the head) the head's own turn
    pub spine: Option<usize>,
    /// `setPointUpDownAngle` / `setLiePointUpDownAngle`: the pitch's bounds standing and lying
    pub pitch_bounds: [[f32; 2]; 2],
}

/// A bone of a `.ske` as a matrix relative to its parent (its rows are the server's columns).
pub fn bone_matrix(b: &bytefield_formats::anim::Bone) -> M4 {
    let r = b.rows;
    [r[0][0], r[1][0], r[2][0], 0.0, r[0][1], r[1][1], r[2][1], 0.0, r[0][2], r[1][2], r[2][2], 0.0, b.pos[0], b.pos[1], b.pos[2], 1.0]
}

impl Skeleton {
    /// In the `.ske`'s own pose (its rows are the server's columns), chained.
    pub fn new(file: Arc<SkeletonFile>, collision: Vec<CollisionBone>, height: f32) -> Skeleton {
        let rel: Vec<M4> = file.bones.iter().map(bone_matrix).collect();
        let spine = file.index("Bip01 Spine");
        let mut s = Skeleton { abs: rel.clone(), rel, file, collision, height, spine, pitch_bounds: [[-20.0, 20.0], [-20.0, 20.0]] };
        s.transform(&[]);
        s
    }

    /// Back to the `.ske`'s pose (the soldier's setting of first person copies the template's).
    pub fn reset(&mut self) {
        self.rel = self.file.bones.iter().map(bone_matrix).collect();
    }

    /// The bone animation's apply on skeleton: the frame from the time's
    /// fraction (a looping one wraps to its first frame; one played once stops at its ends),
    /// interpolated to the next; each of its bones the skeleton has gets it, blended by the
    /// weight (`setRelativeBoneTransform`).
    pub fn apply(&mut self, a: &BoneAnimation, time: f32, weight: f32, looping: bool) {
        let frames = a.frames as usize;
        if frames == 0 {
            return;
        }
        let n = if looping { frames } else { frames - 1 };
        let (f0, f1, cc) = if !looping && time > 1.0 {
            (n.saturating_sub(1), n, 1.0)
        } else if !looping && time < 0.0 {
            (0, 1.min(n), 0.0)
        } else {
            let mut fr = time - time.trunc();
            if fr < 0.0 {
                fr += 1.0;
            }
            let x = n as f32 * fr;
            let i = x.trunc() as i64;
            let f0 = i.rem_euclid(frames as i64) as usize;
            (f0, (f0 + 1) % frames, x - i as f32)
        };
        for (j, name) in a.bones.iter().enumerate() {
            let Some(b) = self.file.index(name) else { continue };
            let (q0, t0) = a.pose(j, f0);
            let (q1, t1) = a.pose(j, f1);
            let q = slerp(q0, q1, cc);
            let t = if t0 == t1 { t0 } else { std::array::from_fn(|k| t0[k] + (t1[k] - t0[k]) * cc) };
            self.set_relative(b, q, t, weight);
        }
    }

    /// `setRelativeBoneTransform(bone, q, t, weight)`: under 1, slerp from the bone's rotation
    /// now and lerp from its translation now.
    fn set_relative(&mut self, b: usize, q: [f32; 4], t: V3, weight: f32) {
        let m = &mut self.rel[b];
        if weight < 1.0 {
            let now = mat_to_quat(m);
            let tr = [m[12], m[13], m[14]];
            *m = quat_to_mat(slerp(now, q, weight));
            for k in 0..3 {
                m[12 + k] = (t[k] - tr[k]) * weight + tr[k];
            }
        } else {
            *m = quat_to_mat(q);
            m[12..15].copy_from_slice(&t);
        }
    }

    /// The aim's matrices (`updateAnimations`): pitch (clamped to the pose's bounds; 0 in a
    /// state that locks the free look) and yaw turn the first spine bone and the next whole,
    /// the third with half the pitch; leaning, the yaw is −30 × the lean instead (the root
    /// turns the other way: [`Skeleton::transform_leaning`]); the head (the fifth after) by
    /// −1×, −2× the head's own angles.
    pub fn aim_extras(&self, aim: &Aim) -> Vec<(usize, M4)> {
        let Some(s) = self.spine else { return Vec::new() };
        let (mut pitch, yaw) = if aim.locked { (0.0, 0.0) } else { (-aim.pitch, -aim.yaw) };
        let [lo, hi] = self.pitch_bounds[aim.lying as usize];
        if pitch > hi {
            pitch = hi;
        } else if pitch < lo {
            pitch = lo;
        }
        let yaw = if aim.lean != 0.0 { aim.lean * -30.0 } else { yaw };
        let both = |p: f32| mult(&rotate_x(p), &rotate_y(yaw));
        let head = mult(&rotate_x(-aim.head[0]), &rotate_y(-2.0 * aim.head[1]));
        [(s, both(pitch)), (s + 1, both(pitch)), (s + 2, both(pitch * 0.5)), (s + 5, head)]
            .into_iter()
            .filter(|(b, _)| *b < self.rel.len())
            .collect()
    }

    /// The bones' transform with `rotate90aroundX` + height as the root: each bone's absolute matrix is its
    /// relative one times its parent's (the root's, times the root matrix); a bone with an aim
    /// matrix then turns its rotation by it (the translation stays).
    pub fn transform(&mut self, extras: &[(usize, M4)]) {
        self.transform_leaning(extras, 0.0);
    }

    /// … leaning: the root matrix is `rotate90aroundX` × a turn of 90 × the lean about y.
    pub fn transform_leaning(&mut self, extras: &[(usize, M4)], lean: f32) {
        let mut root = IDENTITY;
        (root[5], root[6], root[9], root[10]) = (0.0, 1.0, -1.0, 0.0);
        if lean != 0.0 {
            root = mult(&root, &rotate_y(lean * 90.0));
        }
        root[13] = self.height;
        for i in 0..self.rel.len() {
            let parent = self.file.bones[i].parent.map_or(root, |p| self.abs[p]);
            let mut m = mult(&self.rel[i], &parent);
            if let Some((_, e)) = extras.iter().find(|(b, _)| *b == i) {
                let t = [m[12], m[13], m[14]];
                m = mult(&m, &rot_only(e));
                m[12..15].copy_from_slice(&t);
            }
            self.abs[i] = m;
        }
    }

    /// A bone's position in the soldier's frame.
    pub fn bone_pos(&self, b: usize) -> V3 {
        [self.abs[b][12], self.abs[b][13], self.abs[b][14]]
    }

    /// The skeleton collision mesh's distance to geometry lookup for a soldier at `pos` with
    /// these body axes (rows): the first collision bone whose capsule the segment comes close
    /// enough to.
    pub fn segment(&self, rows: &[V3; 3], pos: V3, start: V3, dir: V3) -> Option<Hit> {
        let local = |d: V3| -> V3 { std::array::from_fn(|i| (rows[i][2] as f64 * d[2] as f64 + rows[i][1] as f64 * d[1] as f64 + rows[i][0] as f64 * d[0] as f64) as f32) };
        let p0 = local([start[0] - pos[0], start[1] - pos[1], start[2] - pos[2]]);
        let d = local(dir);
        let p1 = [p0[0] + d[0], p0[1] + d[1], p0[2] + d[2]];
        for c in &self.collision {
            let Some(parent) = self.file.bones[c.bone].parent else { continue };
            let bone = self.bone_pos(c.bone);
            let par = self.bone_pos(parent);
            let (a, b) = if c.shift == 0.0 {
                (par, bone)
            } else {
                let off: V3 = std::array::from_fn(|k| (bone[k] - par[k]) * c.shift);
                (std::array::from_fn(|k| par[k] + off[k]), std::array::from_fn(|k| bone[k] + off[k]))
            };
            if capsule_distance2(a, b, p0, p1) < c.radius {
                let point = std::array::from_fn(|k| {
                    ((rows[2][k] as f64 * bone[2] as f64 + rows[0][k] as f64 * bone[0] as f64 + rows[1][k] as f64 * bone[1] as f64) as f32) + pos[k]
                });
                let m = &self.abs[c.bone];
                return Some(Hit { bone: c.bone, material: c.material, point, normal: [m[0], m[1], m[2]] });
            }
        }
        None
    }
}

impl Skeleton {
    /// Whether [`Skeleton::segment`] would hit, for the AI's lines of sight: a line that does not come near the box of
    /// all the capsules asks none of them.
    /// `capsule_distance2` rounds to floats on the way, as the original, and on a long line that moves the result: the
    /// margin takes that in (some 1e-6 of the line's length squared), four times over.
    pub fn segment_blocked(&self, rows: &[V3; 3], pos: V3, start: V3, dir: V3) -> bool {
        let local = |d: V3| -> V3 { std::array::from_fn(|i| (rows[i][2] as f64 * d[2] as f64 + rows[i][1] as f64 * d[1] as f64 + rows[i][0] as f64 * d[0] as f64) as f32) };
        let p0 = local([start[0] - pos[0], start[1] - pos[1], start[2] - pos[2]]);
        let d = local(dir);
        let (mut lo, mut hi, mut reach) = ([f32::MAX; 3], [f32::MIN; 3], 0f32);
        for c in &self.collision {
            let Some(parent) = self.file.bones[c.bone].parent else { continue };
            let (bone, par) = (self.bone_pos(c.bone), self.bone_pos(parent));
            for k in 0..3 {
                let off = (bone[k] - par[k]) * c.shift;
                let (a, b) = (par[k] + off, bone[k] + off);
                lo[k] = lo[k].min(a.min(b));
                hi[k] = hi[k].max(a.max(b));
            }
            reach = reach.max(c.radius);
        }
        let len2 = |v: V3| v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
        let centre: V3 = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
        let far = len2(d) + len2(std::array::from_fn(|k| p0[k] - centre[k]));
        let margin = (reach + 4e-6 * far).sqrt() + 0.05;
        let (mut t0, mut t1) = (0f32, 1f32);
        for k in 0..3 {
            let (a, b) = (lo[k] - margin, hi[k] + margin);
            if d[k] == 0.0 {
                if p0[k] < a || p0[k] > b {
                    return false;
                }
            } else {
                let (u, v) = ((a - p0[k]) / d[k], (b - p0[k]) / d[k]);
                t0 = t0.max(u.min(v));
                t1 = t1.min(u.max(v));
                if t0 > t1 + 1e-4 {
                    return false;
                }
            }
        }
        self.segment(rows, pos, start, dir).is_some()
    }
}

fn rot_only(e: &M4) -> M4 {
    let mut m = *e;
    (m[3], m[7], m[11], m[12], m[13], m[14], m[15]) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
    m
}

/// The quaternion from mat (only for blending a bone that is mid-morph).
fn mat_to_quat(m: &M4) -> [f32; 4] {
    let at = |r: usize, c: usize| m[4 * r + c] as f64;
    let tr = (m[0] + m[5] + m[10]) as f64;
    let mut q = [0f64; 4];
    if tr > 0.0 {
        let r = (tr + 1.0).sqrt();
        let f = 0.5 / r;
        q[3] = 0.5 * r;
        q[0] = (at(2, 1) - at(1, 2)) * f;
        q[1] = (at(0, 2) - at(2, 0)) * f;
        q[2] = (at(1, 0) - at(0, 1)) * f;
    } else {
        const NEXT: [usize; 3] = [1, 2, 0];
        let mut i = (m[0] < m[5]) as usize;
        if m[5 * i] < m[10] {
            i = 2;
        }
        let (j, k) = (NEXT[i], NEXT[NEXT[i]]);
        let r = (at(i, i) - (at(k, k) + at(j, j)) + 1.0).sqrt();
        let f = 0.5 / r;
        q[i] = 0.5 * r;
        q[3] = (at(k, j) - at(j, k)) * f;
        q[j] = (at(i, j) + at(j, i)) * f;
        q[k] = (at(i, k) + at(k, i)) * f;
    }
    q.map(|v| v as f32)
}

/// `checkCapsuleCollision(a0, a1, b0, b1)`: the squared distance between the segments a0 → a1
/// and b0 → b1 (Eberly's, as the server writes it: extended precision, a few values stored as
/// `f32`).
pub fn capsule_distance2(p1: V3, p2: V3, p3: V3, p4: V3) -> f32 {
    type L = f64;
    let f3 = p4[1] - p3[1];
    let f4 = p4[2] - p3[2];
    let l6 = (p2[0] - p1[0]) as L;
    let l7 = (p2[1] - p1[1]) as L;
    let l8 = (p2[2] - p1[2]) as L;
    let l9 = (p4[0] - p3[0]) as L;
    let f1 = (l8 * l8 + ((l7 * l7 + ((l6 * l6) as f32) as L) as f32) as L) as f32;
    let l10 = -((f4 as L) * l8 + (f3 as L) * l7 + l6 * l9);
    let l11 = (p1[0] - p3[0]) as L;
    let l12 = (p1[1] - p3[1]) as L;
    let f5 = ((f4 as L) * (f4 as L) + (f3 as L) * (f3 as L) + ((l9 * l9) as f32) as L) as f32;
    let l13 = (p1[2] - p3[2]) as L;
    let l7 = l7 * l12 + l11 * l6 + l8 * l13;
    let f2 = (l13 * l13 + ((l11 * l11 + l12 * l12) as f32) as L) as f32;
    let (f1l, f2l, f5l) = (f1 as L, f2 as L, f5 as L);
    let det = (f1l * f5l - l10 * l10).abs();
    let b1 = || -(l11 * l9 + l12 * (f3 as L) + l13 * (f4 as L));
    // each case as a closure: each gives the final value before the abs
    let v: L = if det < 1e-5 {
        // nearly parallel
        if l10 <= 0.0 {
            if f1l <= -l7 {
                l7 + l7 + f1l + f2l
            } else if 0.0 < l7 {
                let l8 = b1();
                if -l10 <= l7 {
                    l8 + l8 + f5l + f2l
                } else {
                    (f5l * (-l7 / l10) + l8 + l8) * (-l7 / l10) + f2l
                }
            } else {
                (-l7 / f1l) * l7 + f2l
            }
        } else if 0.0 <= l7 {
            f2l
        } else if f1l < -l7 {
            let l6 = b1();
            if -(f1l + l7) < l10 {
                let l8 = -(f1l + l7) / l10;
                let l10b = l10 + l6;
                l8 * (l10b + l10b + f5l * l8) + l7 + l7 + f1l + f2l
            } else {
                let l6 = l10 + l7 + l6;
                f1l + f5l + f2l + l6 + l6
            }
        } else {
            (-l7 / f1l) * l7 + f2l
        }
    } else {
        let l8 = b1();
        let s = l10 * l8 - f5l * l7;
        let t = l10 * l7 - f1l * l8;
        // the shared tails
        let at_7e4 = |l7: L| (-l7 / f1l) * l7 + f2l;
        let at_7a3 = || l8 + l8 + f5l + f2l;
        let at_789 = |l10: L, l7: L| (-(l10 + l7) / f1l) * (l10 + l7) + f5l + l8 + l8 + f2l;
        let at_763 = |l10: L, l7: L| {
            if f1l <= -(l10 + l7) {
                let x = l8 + l10 + l7;
                x + x + f1l + f5l + f2l
            } else {
                at_789(l10, l7)
            }
        };
        let at_7c5 = |l7: L| if -l7 < f1l { at_7e4(l7) } else { f1l + l7 + l7 + f2l };
        let at_939 = || {
            if 0.0 <= l8 {
                f2l
            } else if -l8 < f5l {
                (-l8 / f5l) * l8 + f2l
            } else {
                f5l + l8 + l8 + f2l
            }
        };
        if s < 0.0 {
            if t < 0.0 {
                if 0.0 <= l7 { at_939() } else { at_7c5(l7) }
            } else if det < t && l10 + l7 < 0.0 {
                at_763(l10, l7)
            } else {
                at_939()
            }
        } else if det < s {
            let tail = |l10: L, l7: L| {
                let l10 = l10 + l8;
                if 0.0 <= l10 {
                    l7 + l7 + f1l + f2l
                } else if f5l <= -l10 {
                    f1l + f5l + f2l + l7 + l10 + l7 + l10
                } else {
                    (-l10 / f5l) * l10 + f1l + l7 + l7 + f2l
                }
            };
            if t < 0.0 {
                if -l7 < f1l {
                    if l7 < 0.0 { at_7e4(l7) } else { f2l }
                } else {
                    tail(l10, l7)
                }
            } else if det < t && -(l10 + l7) <= f1l {
                if l10 + l7 < 0.0 { at_789(l10, l7) } else { at_7a3() }
            } else {
                tail(l10, l7)
            }
        } else if t < 0.0 {
            if l7 < 0.0 { at_7c5(l7) } else { f2l }
        } else if t <= det {
            let inv = 1.0 / det;
            let (s, t) = (s * inv, t * inv);
            (f1l * s + l10 * t + l7 + l7) * s + t * (l10 * s + f5l * t + l8 + l8) + f2l
        } else if l10 + l7 < 0.0 {
            at_763(l10, l7)
        } else {
            at_7a3()
        }
    };
    v.abs() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The squared distance of two segments, the slow way.
    fn brute(a0: V3, a1: V3, b0: V3, b1: V3) -> f32 {
        let mut best = f64::MAX;
        for i in 0..=400 {
            for j in 0..=400 {
                let (s, t) = (i as f64 / 400.0, j as f64 / 400.0);
                let d: f64 = (0..3).map(|k| {
                    let p = a0[k] as f64 + (a1[k] - a0[k]) as f64 * s;
                    let q = b0[k] as f64 + (b1[k] - b0[k]) as f64 * t;
                    (p - q) * (p - q)
                }).sum();
                best = best.min(d);
            }
        }
        best as f32
    }

    #[test]
    fn capsule_distance_is_the_segments_squared_distance() {
        let cases: [[V3; 4]; 7] = [
            [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.5, 0.3], [1.0, 0.5, 0.3]],
            [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 2.0, 0.3], [1.0, 2.5, 0.3]],
            [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.5, -1.0, 0.0], [0.5, 3.0, 0.0]],
            [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.5, 2.0, 0.0], [0.5, 3.0, 0.0]],
            [[0.1, 0.2, 0.3], [0.4, -0.5, 0.9], [1.0, 1.0, 1.0], [-2.0, 0.5, 0.2]],
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 1.0, 0.0], [3.0, -1.0, 0.5]],
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [-3.0, -1.0, 0.5], [-2.0, 1.0, 0.0]],
        ];
        for [a0, a1, b0, b1] in cases {
            let got = capsule_distance2(a0, a1, b0, b1);
            let want = brute(a0, a1, b0, b1);
            assert!((got - want).abs() < 1e-4, "{a0:?} {a1:?} {b0:?} {b1:?}: {got} vs {want}");
        }
        // and at random, to go through every branch
        let mut x = 12345u64;
        let mut r = || {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        };
        for _ in 0..60 {
            let p: [V3; 4] = std::array::from_fn(|_| [r(), r(), r()]);
            let got = capsule_distance2(p[0], p[1], p[2], p[3]);
            let want = brute(p[0], p[1], p[2], p[3]);
            assert!((got - want).abs() < 1e-4, "{p:?}: {got} vs {want}");
        }
    }

    #[test]
    fn a_quaternion_turns_like_its_matrix() {
        // 90° about z: x → y with row vectors
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let m = quat_to_mat([0.0, 0.0, h, h]);
        assert!((m[1] - -1.0).abs() < 1e-6 && (m[4] - 1.0).abs() < 1e-6, "{m:?}");
        let q = mat_to_quat(&m);
        assert!((q[2].abs() - h).abs() < 1e-6 && (q[3].abs() - h).abs() < 1e-6);
        let s = slerp([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, h, h], 0.5);
        assert!((s[2] - (std::f32::consts::PI / 8.0).sin()).abs() < 1e-6, "{s:?}");
    }
}
