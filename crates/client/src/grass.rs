//! Procedural grass (the `grass` setting, not in the original): tufts of blades over the terrain's green near the eye,
//! where `bytefield_world::grass::GrassMap` says it grows. Only drawn: the simulation and the bots do not see it.
//!
//! The blades are drawn by the terrain's own fragment shader (`terrain.wesl`), so each takes the colour tile under its
//! root, the detail texture, the shadows and the fog, times its vertex colour (darker at the root, lighter at the tip).
//! `grass.wesl` sways them in the wind and thins them out with the distance. The field is made in chunks of
//! [`CHUNK`] metres round the eye, the same tufts every time (hashed from where they stand).

use bevy::prelude::*;
use bytefield_formats::image::Orientation;
use bytefield_world::grass::GrassMap;
use bytefield_world::Level;

/// The terrain's material ([`crate::level::Detail`]'s bindings) and the wind.
pub type GrassMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, GrassShade>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct GrassShade {
    #[uniform(100)]
    pub scale: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub texture: Option<Handle<Image>>,
    #[texture(103, sample_type = "float", filterable = false)]
    pub shadow_rows: Option<Handle<Image>>,
    #[texture(104)]
    #[sampler(105)]
    pub atlas: Option<Handle<Image>>,
    #[uniform(106)]
    pub clip: Vec4,
    /// z [`REACH`], w the wind's bend (the clock is Bevy's `globals.time`: changing the material every frame cost a
    /// third of the frame rate)
    #[uniform(107)]
    pub wind: Vec4,
}

impl bevy::pbr::MaterialExtension for GrassShade {
    fn vertex_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/grass.wesl".into()
    }
    fn prepass_vertex_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/grass_prepass.wesl".into()
    }
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/terrain.wesl".into()
    }
    /// `GRASS` for the terrain's shader: no detail texture on the blades (its stones turned them grey)
    fn specialize(_: &bevy::pbr::MaterialExtensionPipeline, descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor, _: &bevy::mesh::MeshVertexBufferLayoutRef, _: bevy::pbr::MaterialExtensionKey<Self>) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader_defs.push("GRASS".into());
        }
        Ok(())
    }
}

/// A chunk's side, metres
const CHUNK: f32 = 16.0;
/// How far the grass reaches (it thins out from a quarter of it and shrinks into the ground from 0.6 of it)
const REACH: f32 = 45.0;
/// The grid the tufts are drawn from, metres (one tuft a cell at full density)
const SPACING: f32 = 0.42;
/// Blades a tuft
const BLADES: usize = 6;
/// The wind's bend at a blade's tip: metres per square metre of its height
const BEND: f32 = 0.35;
/// A blade's colour over the ground's (in linear: about the square root of it on the screen) at its root, halfway up and at its tip (times the tuft's): the tips catch the light,
/// a little warmer
const ROOT: [f32; 3] = [1.0, 1.0, 0.95];
const MID: [f32; 3] = [1.7, 1.75, 1.55];
const TIP: [f32; 3] = [2.4, 2.5, 2.0];
/// New chunks a frame (a level's first frames make the whole field)
const PER_FRAME: usize = 6;

/// The round's grass: where it grows, the ground it stands on (the level's terrain alone), the terrain's material it is
/// drawn like, and the chunks made.
#[derive(Resource)]
pub struct Grass {
    pub map: GrassMap,
    ground: Level,
    pub terrain: Handle<crate::level::TerrainMaterial>,
    material: Option<Handle<GrassMaterial>>,
    chunks: std::collections::HashMap<(i32, i32), Entity>,
    /// the density the chunks were made with
    density: f32,
}

impl Grass {
    pub fn new(level: &Level, map: GrassMap, terrain: Handle<crate::level::TerrainMaterial>) -> Grass {
        let ground = Level { name: level.name.clone(), terrain: level.terrain.clone(), heightmap: level.heightmap.clone(), combat_area: None, placed: Vec::new(), spawners: Vec::new(), material_map: None };
        Grass { map, ground, terrain, material: None, chunks: Default::default(), density: 0.0 }
    }
}

#[derive(Component)]
pub struct GrassChunk;

/// A hash of two integers and a salt to [0, 1).
fn hash(a: i32, b: i32, salt: u32) -> f32 {
    let mut h = (a as u32).wrapping_mul(0x9e37_79b1) ^ (b as u32).wrapping_mul(0x85eb_ca77) ^ salt.wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Chunk (cx, cz)'s mesh (the level's axes: x, z), None without a tuft. `density` the setting's.
fn chunk_mesh(level: &Level, map: &GrassMap, density: f32, (cx, cz): (i32, i32)) -> Option<Mesh> {
    let ws = level.terrain.world_size as f32;
    let n = (CHUNK / SPACING).round() as i32;
    let (mut pos, mut nrm, mut uv, mut uvb, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for j in 0..n {
        for i in 0..n {
            let (gi, gj) = (cx * n + i, cz * n + j);
            let r = |salt: u32| hash(gi, gj, salt);
            let x = (gi as f32 + r(1)) * SPACING;
            let z = (gj as f32 + r(2)) * SPACING;
            let d = map.at(x as f64, z as f64) * density;
            if d <= 0.0 || r(3) >= d {
                continue;
            }
            let Some((ground, normal)) = level.height_and_normal(x as f64, z as f64, Orientation::IDENTITY) else { continue };
            // (Bevy's axes: z is the level's −z; a little into the ground, on slopes too)
            let root = Vec3::new(x, ground as f32 - 0.05, -z);
            let up_n = Vec3::new(normal[0] as f32, normal[1] as f32, -normal[2] as f32);
            let tuft_uv = [x / ws, z / ws];
            let height = (0.25 + 0.45 * d.sqrt()) * (0.7 + 0.3 * r(4));
            // (a little darker or yellower, tuft by tuft)
            let shade = 0.85 + 0.25 * r(5);
            let tint = Vec3::new(shade * (1.0 + 0.08 * (r(6) - 0.5)), shade, shade * 0.95);
            let turn = r(7) * std::f32::consts::TAU;
            for k in 0..BLADES {
                let s = |salt: u32| r(10 + 8 * k as u32 + salt);
                let a = turn + k as f32 * std::f32::consts::TAU / BLADES as f32 + 0.6 * (s(0) - 0.5);
                let off = Vec2::from_angle(a) * 0.18 * s(1).sqrt();
                // (bent outwards, and the more the shorter: a blade that leans is lower)
                let bow = 0.2 + 0.5 * s(3);
                let lean = Vec2::from_angle(a + 0.8 * (s(2) - 0.5)) * height * bow;
                // (across the lean, give or take: seen side on, a blade is still a blade)
                let side = Vec2::from_angle(a + std::f32::consts::FRAC_PI_2 + 0.9 * (s(4) - 0.5)) * (0.012 + 0.012 * s(5));
                let h = height * (0.55 + 0.45 * s(6)) * (1.0 - 0.35 * bow);
                let base = root + Vec3::new(off.x, 0.0, off.y);
                let at = |t: f32, w: f32| base + Vec3::new(lean.x * t * t + side.x * w, h * t, lean.y * t * t + side.y * w);
                let b = pos.len() as u32;
                for (p, f) in [(at(0.0, -1.0), ROOT), (at(0.0, 1.0), ROOT), (at(0.55, -0.8), MID), (at(0.55, 0.8), MID), (at(1.0, 0.0), TIP)] {
                    pos.push(p.to_array());
                    nrm.push(up_n.to_array());
                    uv.push(tuft_uv);
                    uvb.push([root.x, root.z]);
                    let c = tint * Vec3::from(f);
                    col.push([c.x, c.y, c.z, p.y - root.y]);
                }
                idx.extend([b, b + 1, b + 3, b, b + 3, b + 2, b + 2, b + 3, b + 4]);
            }
        }
    }
    (!idx.is_empty()).then(|| {
        Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, Default::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uvb)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
            .with_inserted_indices(bevy::mesh::Indices::U32(idx))
    })
}

/// Makes the chunks near the eye and drops the far ones. With the setting at 0, none.
#[allow(clippy::too_many_arguments)]
pub fn grow(
    mut commands: Commands,
    grass: Option<ResMut<Grass>>,
    eye: Query<&Transform, (With<crate::level::WorldCamera>, Without<crate::level::ReflectionCamera>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    terrain_materials: Res<Assets<crate::level::TerrainMaterial>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let Some(mut grass) = grass else { return };
    let density = crate::assets::graphics().grass;
    if density != grass.density {
        for (_, e) in grass.chunks.drain() {
            commands.entity(e).despawn();
        }
        grass.density = density;
    }
    if density <= 0.0 {
        return;
    }
    let Ok(eye) = eye.single() else { return };
    let material = match grass.material.clone() {
        Some(m) => m,
        None => {
            let Some(t) = terrain_materials.get(&grass.terrain) else { return };
            let d = &t.extension;
            let m = materials.add(GrassMaterial {
                base: StandardMaterial { cull_mode: None, double_sided: true, ..t.base.clone() },
                extension: GrassShade { scale: d.scale, texture: d.texture.clone(), shadow_rows: d.shadow_rows.clone(), atlas: d.atlas.clone(), clip: d.clip, wind: Vec4::new(0.0, 0.0, REACH, BEND) },
            });
            grass.material = Some(m.clone());
            m
        }
    };
    // (the level's axes)
    let at = Vec2::new(eye.translation.x, -eye.translation.z);
    let near = |(cx, cz): (i32, i32), r: f32| {
        let lo = Vec2::new(cx as f32, cz as f32) * CHUNK;
        at.distance(at.clamp(lo, lo + CHUNK)) < r
    };
    let grass = &mut *grass;
    grass.chunks.retain(|c, e| {
        let keep = near(*c, REACH + CHUNK);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    let span = (REACH / CHUNK).ceil() as i32 + 1;
    let (ex, ez) = ((at.x / CHUNK).floor() as i32, (at.y / CHUNK).floor() as i32);
    let mut want: Vec<(i32, i32)> = (-span..=span).flat_map(|dz| (-span..=span).map(move |dx| (ex + dx, ez + dz))).filter(|c| near(*c, REACH) && !grass.chunks.contains_key(c)).collect();
    // (the nearest first)
    want.sort_by_key(|&(cx, cz)| (cx - ex).pow(2) + (cz - ez).pow(2));
    for c in want.into_iter().take(PER_FRAME) {
        let e = match chunk_mesh(&grass.ground, &grass.map, density, c) {
            Some(mesh) => commands.spawn((GrassChunk, Mesh3d(meshes.add(mesh)), MeshMaterial3d(material.clone()), Transform::IDENTITY, bevy::light::NotShadowCaster)).id(),
            // (an empty chunk is remembered as a bare entity)
            None => commands.spawn((GrassChunk, Transform::IDENTITY)).id(),
        };
        grass.chunks.insert(c, e);
    }
}
