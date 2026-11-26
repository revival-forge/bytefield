//! Marks of hits and blasts (the `decals` setting; not in the original): a bullet's hole where a projectile stopped on the
//! terrain or a static, and a scorch under an explosion. Bevy's forward decals: a quad along the surface that reads the
//! depth prepass, so it lies on our own terrain shader as on any mesh. Blended by multiplying what is under it, so the
//! baked shade, the sun's shadows and the fog of the surface stay; the textures are made here (dark middle, ragged edge).
//!
//! TODO: a mark on a static that is later destroyed stays floating where its face was.
//! TODO: no marks on vehicles (they move; a child of the vehicle's entity would carry it) nor blood on the ground.

use std::collections::VecDeque;

use bevy::pbr::decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt};
use bevy::prelude::*;
use bytefield_formats::image::Image as Rgba;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Hole,
    Scorch,
}

impl Kind {
    /// seconds it stays (it fades over the last quarter)
    fn life(self) -> f32 {
        match self {
            Kind::Hole => 90.0,
            Kind::Scorch => 240.0,
        }
    }

    /// at most this many at once (the oldest go first): a machine gun's or a barrage's
    fn cap(self) -> usize {
        match self {
            Kind::Hole => 400,
            Kind::Scorch => 60,
        }
    }

    /// how far off the quad the surface may lie, metres, before the mark fades: about its size, so a soldier standing on
    /// it is not painted
    fn depth_fade(self) -> f32 {
        match self {
            Kind::Hole => 0.12,
            Kind::Scorch => 0.9,
        }
    }
}

/// A mark asked for: where (Bevy's axes), the surface's normal and its width in metres.
pub struct Mark {
    pub kind: Kind,
    pub at: Vec3,
    pub normal: Vec3,
    pub size: f32,
}

/// Steps of the fade (one material each)
const FADE_STEPS: usize = 6;

#[derive(Resource, Default)]
pub struct Decals {
    pub queue: Vec<Mark>,
    live: VecDeque<(Entity, Kind, f32, usize)>,
    /// per kind, a material per fade step (0: whole)
    materials: Vec<Vec<Handle<ForwardDecalMaterial<StandardMaterial>>>>,
    seed: u32,
}

impl Decals {
    pub fn push(&mut self, m: Mark) {
        if crate::assets::graphics().decals {
            self.queue.push(m);
        }
    }

    fn rand(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / 16_777_216.0
    }
}

/// The scorch's width for an explosion of this radius, metres (a grenade's 1–2 m, a bomb's up to 9)
pub fn scorch_size(radius: f32) -> f32 {
    (radius * 0.6).clamp(1.2, 9.0)
}

/// A bullet's hole, metres
pub const HOLE_SIZE: f32 = 0.14;

/// Every frame: places what was asked, fades what is old and takes it away; all of it at a new round or with the setting
/// off.
pub fn update(
    g: Option<Res<crate::play::Game>>,
    mut decals: ResMut<Decals>,
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
    mut images: ResMut<Assets<Image>>,
    mut placed: Query<&mut MeshMaterial3d<ForwardDecalMaterial<StandardMaterial>>>,
) {
    let on = crate::assets::graphics().decals;
    let fresh = g.as_ref().is_none_or(|g| g.is_added());
    let dt = time.delta_secs();
    let d = &mut *decals;
    if !on || fresh {
        for (e, ..) in d.live.drain(..) {
            commands.entity(e).try_despawn();
        }
        if !on {
            d.queue.clear();
            return;
        }
    }
    if d.materials.is_empty() {
        d.materials = [Kind::Hole, Kind::Scorch].iter().map(|k| make_materials(*k, &mut materials, &mut images)).collect();
    }
    let index = |k: Kind| k as usize;
    // age and fade
    let mut counts = [0usize; 2];
    for (_, k, ..) in d.live.iter() {
        counts[index(*k)] += 1;
    }
    for m in std::mem::take(&mut d.queue) {
        let k = m.kind;
        // the oldest of its kind first
        if counts[index(k)] >= k.cap()
            && let Some(p) = d.live.iter().position(|(_, kk, ..)| *kk == k)
        {
            let (e, ..) = d.live.remove(p).unwrap();
            commands.entity(e).try_despawn();
            counts[index(k)] -= 1;
        }
        let spin = d.rand() * std::f32::consts::TAU;
        let size = m.size * (0.8 + 0.4 * d.rand());
        let n = m.normal.normalize_or(Vec3::Y);
        let rot = Quat::from_rotation_arc(Vec3::Y, n) * Quat::from_rotation_y(spin);
        // (the quad lies a little off the surface, as Bevy's forward decal looks along its normal both ways)
        let e = commands
            .spawn((ForwardDecal, MeshMaterial3d(d.materials[index(k)][0].clone()), Transform::from_translation(m.at + n * 0.02).with_rotation(rot).with_scale(Vec3::new(size, 1.0, size)), crate::level::world_eye_only(), Name::new("decal")))
            .id();
        d.live.push_back((e, k, 0.0, 0));
        counts[index(k)] += 1;
    }
    let mats = &d.materials;
    d.live.retain_mut(|(e, k, age, step)| {
        *age += dt;
        let left = 1.0 - *age / k.life();
        if left <= 0.0 {
            commands.entity(*e).try_despawn();
            return false;
        }
        // (over the last quarter, in steps)
        let s = (((0.25 - left) / 0.25).max(0.0) * FADE_STEPS as f32) as usize;
        let s = s.min(FADE_STEPS - 1);
        if s != *step
            && let Ok(mut m) = placed.get_mut(*e)
        {
            m.0 = mats[index(*k)][s].clone();
            *step = s;
        }
        true
    });
}

fn make_materials(k: Kind, materials: &mut Assets<ForwardDecalMaterial<StandardMaterial>>, images: &mut Assets<Image>) -> Vec<Handle<ForwardDecalMaterial<StandardMaterial>>> {
    let texture = images.add(crate::assets::rgba_image_mips(&[texture(k)]));
    (0..FADE_STEPS)
        .map(|s| {
            let a = 1.0 - s as f32 / FADE_STEPS as f32;
            materials.add(ForwardDecalMaterial {
                base: StandardMaterial {
                    base_color: Color::srgba(1.0, 1.0, 1.0, a),
                    base_color_texture: Some(texture.clone()),
                    // (what is under it, darkened: its light and shadow stay)
                    alpha_mode: AlphaMode::Multiply,
                    unlit: true,
                    // (the surface under it is fogged already: the fog would tint the factor)
                    fog_enabled: false,
                    ..default()
                },
                extension: ForwardDecalMaterialExt { depth_fade_factor: k.depth_fade() },
            })
        })
        .collect()
}

fn hash(x: i32, y: i32, s: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ s.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_215.0
}

/// Value noise, tiling every `period` cells in x (round the mark's angle)
fn noise(x: f32, y: f32, period: i32, s: u32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (x0, y0) = (ix as i32, iy as i32);
    let w = |x: i32| x.rem_euclid(period);
    let h = |x: i32, y: i32| hash(w(x), y, s);
    let a = h(x0, y0) + (h(x0 + 1, y0) - h(x0, y0)) * ux;
    let b = h(x0, y0 + 1) + (h(x0 + 1, y0 + 1) - h(x0, y0 + 1)) * ux;
    a + (b - a) * uy
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The mark's texture: the colour is the factor that multiplies the surface (sRGB), the alpha how much of it applies.
pub fn texture(k: Kind) -> Rgba {
    let side = match k {
        Kind::Hole => 64,
        Kind::Scorch => 256,
    };
    let mut rgba = vec![0u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let u = (x as f32 + 0.5) / side as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / side as f32 * 2.0 - 1.0;
            let r = (u * u + v * v).sqrt();
            let ang = v.atan2(u) / std::f32::consts::TAU + 0.5;
            let (c, a) = match k {
                Kind::Hole => {
                    // a dark core, a ring of thrown dirt, a ragged edge
                    let edge = r * (1.0 + 0.4 * (noise(ang * 9.0, r * 3.0, 9, 1) - 0.5));
                    let a = smoothstep(0.9, 0.6, edge);
                    let c = 0.2 + 0.5 * smoothstep(0.15, 0.4, edge) + 0.15 * noise(u * 10.0 + 20.0, v * 10.0, 1 << 20, 2);
                    (c, a)
                }
                Kind::Scorch => {
                    // soot thinning outwards with streaks along the blast, the edge torn by noise
                    let streak = noise(ang * 40.0, 0.0, 40, 3);
                    let edge = r * (1.0 + 0.45 * (noise(ang * 7.0, r * 2.0, 7, 4) - 0.5) + 0.3 * (streak - 0.5));
                    let grain = noise(u * 24.0 + 40.0, v * 24.0, 1 << 20, 5);
                    let a = smoothstep(1.0, 0.3, edge) * (0.8 + 0.2 * grain);
                    let c = 0.06 + 0.2 * smoothstep(0.1, 0.35, edge) + 0.45 * smoothstep(0.35, 0.9, edge) + 0.1 * (grain - 0.5);
                    (c, a)
                }
            };
            // (a little warm: burnt earth more than ink)
            let c = c.clamp(0.0, 1.0);
            let px = [c * 1.0, c * 0.93, c * 0.85, a.clamp(0.0, 1.0)];
            for (i, p) in px.iter().enumerate() {
                rgba[(y * side + x) * 4 + i] = (p * 255.0).round() as u8;
            }
        }
    }
    // (the border clear, so the quad's edge never shows)
    for i in 0..side {
        for (x, y) in [(i, 0), (i, side - 1), (0, i), (side - 1, i)] {
            rgba[(y * side + x) * 4 + 3] = 0;
        }
    }
    Rgba { width: side, height: side, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_textures_are_dark_inside_and_clear_outside() {
        for k in [Kind::Hole, Kind::Scorch] {
            let t = texture(k);
            let s = t.width;
            let at = |x: usize, y: usize| &t.rgba[(y * s + x) * 4..(y * s + x) * 4 + 4];
            let mid = at(s / 2, s / 2);
            assert!(mid[0] < 80 && mid[3] > 200, "{k:?} middle {mid:?}");
            assert_eq!(at(0, s / 2)[3], 0, "{k:?} edge");
            assert!(at(s / 16, s / 16)[3] < 10, "{k:?} corner");
        }
    }

    #[test]
    fn scorches_grow_with_the_blast() {
        assert!(scorch_size(2.0) < scorch_size(10.0));
        assert!(scorch_size(100.0) <= 9.0);
    }
}
