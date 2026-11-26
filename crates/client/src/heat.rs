//! Heat haze (the `heat` setting; not in the original): the air shimmering over the vehicles' running engines and over
//! blasts, with a quick shock ring when one goes off. A full-screen pass on the world's camera (before the arms and the
//! HUD are drawn over it) shifts each pixel by a rippling offset summed over the sources on screen (`shaders/heat.wesl`).
//! Level 3.
//!
//! Where an engine's exhaust is: the engine part's place says nothing (a tank's sits at the body's origin), so it comes
//! from the box of the vehicle's meshes: a jet's at the tail behind its engines, a land vehicle's on the rear deck, a
//! helicopter's on the roof behind the rotor's mast. Propeller planes and ships, none.
//!
//! TODO: what is in front of a source is only the terrain (a march along the sight); a wall or a vehicle between does not
//! hide it, and the haze shows on it.

use bevy::core_pipeline::fullscreen_material::FullscreenMaterial;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;
use std::collections::HashMap;

/// the sources drawn at once (the strongest)
pub const MAX: usize = 24;

/// The pass's uniform: per source a segment on the screen (uv of its two ends) and its shape (radius in screen heights,
/// strength, kind 0 haze / 1 ring, phase or ring width).
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct HeatHaze {
    pub seg: [Vec4; MAX],
    pub shape: [Vec4; MAX],
    pub count: u32,
    pub time: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}

impl FullscreenMaterial for HeatHaze {
    fn fragment_shader() -> ShaderRef {
        "embedded://bytefield/shaders/heat.wesl".into()
    }
}

/// how long a blast's air shimmers, seconds
const BLAST_LIFE: f32 = 2.5;
/// how long its shock ring runs out, seconds
const RING_LIFE: f32 = 0.35;
/// beyond this, metres, no source
const REACH: f32 = 260.0;
/// every source's shimmer scaled by this (at 1 the haze over an engine looked overdone on video)
const GAIN: f32 = 0.5;

/// The blasts of the last seconds: where, their radius and age.
#[derive(Resource, Default)]
pub struct Blasts(Vec<(Vec3, f32, f32)>);

impl Blasts {
    pub fn push(&mut self, at: Vec3, radius: f32) {
        if crate::assets::graphics().heat {
            self.0.push((at, radius.max(1.0), 0.0));
        }
    }
}

/// Where a vehicle's exhaust is, in its body's frame (Refractor axes: z ahead), and how it blows.
#[derive(Clone, Copy)]
pub enum Exhaust {
    /// rising from the spot, its radius in metres
    Rising(Vec3, f32),
    /// blown back from the spot along −z (a jet), radius and length in metres
    Blown(Vec3, f32, f32),
}

/// A source in the world: a segment and its radius in metres, strength, kind.
struct Source {
    a: Vec3,
    b: Vec3,
    radius: f32,
    strength: f32,
    ring: Option<f32>,
}

/// The exhausts of a template, from its meshes' box in its body's frame and the kinds of its engines.
fn exhausts(db: &bytefield_con::templates::TemplateDb, vfs: &bytefield_vfs::Vfs, v: &bytefield_sim::mover::Vehicle) -> Vec<Exhaust> {
    use bytefield_sim::craft::Kind;
    let frames = v.craft.frames_in_root();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for d in crate::model::draws(db, &v.template, &v.craft.parts) {
        if d.kind != "standardmesh" || d.wreck == Some(true) {
            continue;
        }
        let Ok(bytes) = vfs.read(&format!("standardmesh/{}.sm", d.file)).or_else(|_| vfs.read(&format!("{}.sm", d.file))) else { continue };
        let Ok(sm) = bytefield_formats::sm::parse(&bytes) else { continue };
        let f = match d.part {
            Some(k) => crate::assets::frame_matrix(&frames[k].child_rows, frames[k].pos),
            None => crate::assets::frame_matrix(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3]),
        }
        .mul(&d.local);
        for k in 0..8 {
            let c = [sm.bounds[k & 1][0], sm.bounds[(k >> 1) & 1][1], sm.bounds[(k >> 2) & 1][2]];
            let q = f.transform_point([c[0] as f64, c[1] as f64, c[2] as f64]);
            let q = Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32);
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    if lo.x > hi.x {
        return Vec::new();
    }
    let size = hi - lo;
    let engines: Vec<(u32, [f32; 3], [f32; 3])> = v.craft.parts.iter().zip(&frames).filter_map(|(p, f)| if let Kind::Engine(e) = &p.kind { Some((e.engine_type, f.pos, f.rows[2])) } else { None }).collect();
    let rotor = engines.iter().any(|e| e.0 & 1 != 0 && e.2[1].abs() > 0.7);
    let land = engines.iter().any(|e| e.0 & 6 != 0);
    if rotor {
        return vec![Exhaust::Rising(Vec3::new(0.0, lo.y + size.y * 0.75, lo.z + size.z * 0.4), 0.5)];
    }
    if land {
        // (the rear deck, a little in from the back)
        return vec![Exhaust::Rising(Vec3::new(0.0, lo.y + size.y * 0.55, lo.z + size.z * 0.12), (size.x * 0.18).clamp(0.3, 0.8))];
    }
    // a jet: its thrust engines sit behind the middle (a propeller's are at the nose)
    let jets: Vec<&(u32, [f32; 3], [f32; 3])> = engines.iter().filter(|e| e.0 & 9 == 1 && e.2[2].abs() > 0.7 && e.1[2] < 0.5).collect();
    if jets.is_empty() {
        return Vec::new();
    }
    let x = jets.iter().map(|e| e.1[0]).sum::<f32>() / jets.len() as f32;
    let y = jets.iter().map(|e| e.1[1]).sum::<f32>() / jets.len() as f32;
    vec![Exhaust::Blown(Vec3::new(x, y, lo.z + 0.3), 0.6 * (jets.len() as f32).sqrt(), (size.z * 0.5).clamp(4.0, 10.0))]
}

/// Every frame with `heat`: the sources on the world's camera ([`HeatHaze`]); without it, none.
#[allow(clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    game: Option<Res<crate::play::Game>>,
    mut blasts: ResMut<Blasts>,
    time: Res<Time>,
    cams: Query<(Entity, &Camera, &GlobalTransform, &Projection, Has<HeatHaze>), With<crate::level::WorldCamera>>,
    mut known: Local<HashMap<String, Vec<Exhaust>>>,
) {
    let g = crate::assets::graphics();
    let dt = time.delta_secs();
    blasts.0.retain_mut(|b| {
        b.2 += dt;
        b.2 < BLAST_LIFE
    });
    let Ok((e, cam, at, proj, had)) = cams.single() else { return };
    let Some(game) = game.filter(|_| g.heat) else {
        if had {
            commands.entity(e).remove::<HeatHaze>();
        }
        blasts.0.clear();
        return;
    };
    if game.is_added() {
        known.clear();
    }
    let sim = &game.session.sim;
    let eye = at.translation();
    let mut sources: Vec<Source> = Vec::new();
    for v in sim.vehicles.iter().filter(|v| v.in_world() && !v.has_soldier_contact()) {
        let pos = crate::assets::to_bevy(v.craft.body.pos);
        if pos.distance(eye) > REACH + v.radius {
            continue;
        }
        let rpm = v.craft.parts.iter().filter_map(|p| if let bytefield_sim::craft::Kind::Engine(e) = &p.kind { e.on.then_some(e.rpm.abs()) } else { None }).fold(None, |m: Option<f32>, r| Some(m.unwrap_or(0.0).max(r)));
        let burning = v.armor.as_ref().is_some_and(|a| a.destroyed);
        if rpm.is_none() && !burning {
            continue;
        }
        let ex = known.entry(v.template.to_ascii_lowercase()).or_insert_with(|| exhausts(&sim.world.db, &sim.world.vfs, v));
        let rows = v.craft.body.rows;
        let to_world = |p: Vec3| crate::assets::to_bevy([0, 1, 2].map(|k| v.craft.body.pos[k] + rows[0][k] * p.x + rows[1][k] * p.y + rows[2][k] * p.z));
        if burning {
            // (a wreck's fire: the air over its middle)
            let c = to_world(Vec3::ZERO);
            sources.push(Source { a: c + Vec3::Y * 1.0, b: c + Vec3::Y * 5.0, radius: (v.radius * 0.35).clamp(1.0, 3.0), strength: 0.9, ring: None });
            continue;
        }
        let throttle = rpm.unwrap_or(0.0).min(1.0);
        for x in ex.iter() {
            match *x {
                Exhaust::Rising(p, r) => {
                    let a = to_world(p);
                    sources.push(Source { a, b: a + Vec3::Y * (1.2 + 1.3 * throttle), radius: r, strength: 0.35 + 0.65 * throttle, ring: None });
                }
                Exhaust::Blown(p, r, len) => {
                    let a = to_world(p);
                    let b = to_world(p - Vec3::Z * len * (0.5 + throttle));
                    sources.push(Source { a, b, radius: r, strength: 0.3 + 0.9 * throttle, ring: None });
                }
            }
        }
    }
    for &(c, r, age) in &blasts.0 {
        if age < RING_LIFE {
            // (the ring runs out to twice the radius)
            sources.push(Source { a: c, b: c, radius: r * 2.0 * (age / RING_LIFE).sqrt(), strength: 1.0 - age / RING_LIFE, ring: Some(r * 0.25) });
        }
        let fade = (1.0 - age / BLAST_LIFE).powi(2);
        sources.push(Source { a: c + Vec3::Y * r * 0.2, b: c + Vec3::Y * r * 1.5, radius: r * 0.6, strength: 1.2 * fade, ring: None });
    }
    let Projection::Perspective(p) = proj else { return };
    // screen heights a metre takes at a distance of one metre
    let scale = 1.0 / (2.0 * (p.fov * 0.5).tan());
    let ground = |x: f32, z: f32| sim.world.height_at(x as f64, -z as f64).map(|h| h as f32);
    let mut drawn: Vec<(f32, Vec4, Vec4)> = Vec::new();
    for s in &sources {
        let depth = (s.a - eye).dot(at.forward().as_vec3());
        if depth < 0.5 || depth > REACH {
            continue;
        }
        // (hidden by the terrain: a few points along the sight)
        if (1..12).any(|k| {
            let q = eye + (s.a - eye) * (k as f32 / 12.0);
            ground(q.x, q.z).is_some_and(|h| h > q.y + 0.3)
        }) {
            continue;
        }
        let ndc = |w: Vec3| cam.world_to_ndc(at, w).map(|n| Vec2::new(n.x * 0.5 + 0.5, 0.5 - n.y * 0.5));
        let (Some(a), Some(b)) = (ndc(s.a), ndc(s.b)) else { continue };
        let r = s.radius * scale / depth;
        if r < 0.002 || a.x < -r - 0.2 || a.x > 1.2 + r || a.y < -r - 0.2 || a.y > 1.2 + r {
            continue;
        }
        // (fades out towards its reach, and when it fills the screen)
        let strength = GAIN * s.strength * (1.0 - (depth / REACH).powi(2)) / r.max(1.0);
        let shape = match s.ring {
            Some(w) => Vec4::new(r, strength, 1.0, (w * scale / depth).max(0.002)),
            None => Vec4::new(r, strength, 0.0, (s.a.x * 0.37 + s.a.z * 0.61).rem_euclid(6.283)),
        };
        drawn.push((strength * r, Vec4::new(a.x, a.y, b.x, b.y), shape));
    }
    drawn.sort_by(|x, y| y.0.total_cmp(&x.0));
    let mut h = HeatHaze { count: drawn.len().min(MAX) as u32, time: time.elapsed_secs_wrapped(), ..default() };
    for (k, (_, s, sh)) in drawn.into_iter().take(MAX).enumerate() {
        h.seg[k] = s;
        h.shape[k] = sh;
    }
    commands.entity(e).insert(h);
}
