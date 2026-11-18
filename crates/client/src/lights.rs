//! Dynamic lights: a shot's flash and an explosion light what is around
//! them for a moment. Bevy's point lights: the lit meshes take them in its PBR, the terrain, the water and the
//! trees (unlit) in their shaders (`flash_light`). An enhancement, not in the original.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

/// One light: where, its colour, its strength at the start (lumens), how far it reaches (m), how long it lasts (s).
#[derive(Clone, Copy, Debug)]
pub struct Flash {
    pub at: Vec3,
    pub color: Color,
    pub lumens: f32,
    pub range: f32,
    pub ttl: f32,
}

impl Flash {
    /// A gun's: warm, a few metres, two or three frames.
    pub fn muzzle(at: Vec3) -> Flash {
        Flash { at, color: Color::srgb(1.0, 0.75, 0.45), lumens: 400.0, range: 7.0, ttl: 0.06 }
    }

    /// An explosion's, by its radius (a grenade's some 5 m, a bomb's 15).
    pub fn explosion(at: Vec3, radius: f32) -> Flash {
        let r = radius.clamp(2.0, 20.0);
        Flash { at: at + Vec3::Y, color: Color::srgb(1.0, 0.65, 0.3), lumens: 160.0 * r * r, range: (r * 3.0).clamp(8.0, 45.0), ttl: 0.15 + 0.015 * r }
    }
}

/// The lights asked for this frame, and those lit (their entity, the light, its age).
#[derive(Resource, Default)]
pub struct Flashes {
    pub queue: Vec<Flash>,
    lit: Vec<(Entity, Flash, f32)>,
}

/// At most this many at once (the oldest go first): a machine gun's or a barrage's.
const MAX_LIT: usize = 24;

impl Flashes {
    pub fn push(&mut self, f: Flash) {
        if crate::assets::graphics().dyn_lights {
            self.queue.push(f);
        }
    }
}

/// Every frame: lights what was asked, fades what is lit (by the square of the time left) and puts it out.
pub fn update(mut flashes: ResMut<Flashes>, time: Res<Time>, mut commands: Commands, mut lights: Query<&mut PointLight>) {
    let dt = time.delta_secs();
    let on = crate::assets::graphics().dyn_lights;
    let Flashes { queue, lit } = &mut *flashes;
    lit.retain_mut(|(e, f, age)| {
        *age += dt;
        let left = 1.0 - *age / f.ttl;
        if !on || left <= 0.0 {
            commands.entity(*e).try_despawn();
            return false;
        }
        if let Ok(mut l) = lights.get_mut(*e) {
            l.intensity = f.lumens * left * left;
        }
        true
    });
    for f in queue.drain(..) {
        if lit.len() >= MAX_LIT {
            let (e, _, _) = lit.remove(0);
            commands.entity(e).try_despawn();
        }
        let e = commands
            .spawn((
                PointLight { color: f.color, intensity: f.lumens, range: f.range, radius: 0.2, shadow_maps_enabled: false, ..default() },
                Transform::from_translation(f.at),
                RenderLayers::from_layers(&[0, 1]),
            ))
            .id();
        lit.push((e, f, 0.0));
    }
}
