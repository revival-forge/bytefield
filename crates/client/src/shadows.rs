//! The original's shadows of soldiers and vehicles. From
//! its rendering: each object with `hasDynamicShadow 1` near the camera is drawn black into a 128² target of a pool of
//! 20, seen from the sun, and the terrain under it drawn again with that target projected (camera-space position ×
//! a texture matrix, clamped), blended by its alpha × the texture factor (0x89–0x8f in El Alamein, whose
//! `shadow.shadowColor` is 0.6). Only on the terrain; none past some 45 m (between 40.9 and 45.8 in El Alamein). Where
//! from and how dark: [`crate::level::Atmosphere::object_shadow`]. Here the silhouette is drawn on the CPU (the meshes as
//! they are drawn, the soldiers' skins too) into one row of an atlas, and `shaders/terrain.wesl` darkens by it.
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// A silhouette's side in texels (the original's targets)
pub const SIZE: usize = 128;
/// How many at once (the original's pool of targets)
pub const MAX: usize = 20;
/// How far from the camera an object still casts one (metres)
pub const RANGE: f32 = 45.0;

/// A mesh drawn for the object `0` (its index in the simulation): what makes up its silhouette.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Caster(pub usize);

/// What a [`Caster`] piece puts in the silhouette instead of the mesh drawn: its `.sm`'s shadow LOD ([`crate::assets::Cache::shadow`]).
#[derive(Component, Clone)]
pub struct ShadowMesh(pub Handle<Mesh>);

/// One object's silhouette from the sun: the texels (0 or 255, row by row) and, for a point `p` of the world,
/// `rows[0]·(p, 1)` and `rows[1]·(p, 1)` its coordinates in them (0–1); `rows[2]·(p, 1)` above 0 is above the object
/// (it casts nothing upwards).
pub struct Silhouette {
    pub rows: [Vec4; 3],
    pub mask: Vec<u8>,
}

/// The silhouette of these triangles (world space) with the light coming from `to_sun` (towards the sun), on a square
/// about their extent seen from it, two texels clear at each side (the texture is clamped). With the object's bounding
/// sphere (its position and `World::bounding_radius`), the original's square instead (on the
/// Patriot: `TEXTURE0` spans 21,0 m for a radius of 10,33, so 2r and a texel clear at each side), when it holds them all.
// TODO: the soldiers: in Husky the Kübelwagen's square is 4,679 m (radius 2,294: it fits)
// and another one 5,366 m (radius 2,64), more than a soldier's 1,876: the Kübelwagen with its driver in one sphere? How: the Husky capture above.
// A vehicle's triangles are its pieces' shadow LODs ([`ShadowMesh`]), not the meshes drawn: the original's pass draws,
// for each piece whose `GeometryTemplate.hasDynamicShadow 1`, its `.sm`'s shadow LOD (the Stryker's, one draw of 249 vertices and 83
// triangles, `STRYKER_hull.sm`'s; the AC-130's, 750/250). A piece without the flag (the ZPU-4's three, whose files have one) or whose file
// has no shadow LOD (DC's `PatriotBase`, `SA3_base_m1`) draws nothing: those passes come out empty, at 6 m as at 60. A soldier's is his skin (the client's `AnimatedMesh` shadow index buffer): the mesh drawn, as before.
pub fn silhouette(tris: &[[Vec3; 3]], to_sun: Vec3, sphere: Option<(Vec3, f32)>) -> Option<Silhouette> {
    let l = to_sun.try_normalize()?;
    let u = Vec3::Y.cross(l).try_normalize().unwrap_or(Vec3::X);
    let v = l.cross(u);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    // (each vertex seen from the sun once: this is what most of the time goes on)
    let seen: Vec<[Vec2; 3]> = tris
        .iter()
        .map(|t| {
            t.map(|p| {
                let q = Vec3::new(p.dot(u), p.dot(v), p.dot(l));
                lo = lo.min(q);
                hi = hi.max(q);
                q.truncate()
            })
        })
        .collect();
    if tris.is_empty() || !(hi - lo).is_finite() {
        return None;
    }
    let mut mid = (lo + hi) * 0.5;
    let mut half = (hi.x - lo.x).max(hi.y - lo.y).max(0.01) * 0.5 * SIZE as f32 / (SIZE - 4) as f32;
    if let Some((c, r)) = sphere {
        let q = Vec2::new(c.dot(u), c.dot(v));
        let reach = (q - lo.truncate()).abs().max((hi.truncate() - q).abs());
        if r > 0.0 && reach.x <= r && reach.y <= r {
            (mid.x, mid.y) = (q.x, q.y);
            half = r * SIZE as f32 / (SIZE - 2) as f32;
        }
    }
    let s = 0.5 / half;
    let rows = [u.extend(0.5 - mid.x * s) * Vec4::new(s, s, s, 1.0), v.extend(0.5 - mid.y * s) * Vec4::new(s, s, s, 1.0), l.extend(-hi.z)];
    let mut mask = vec![0u8; SIZE * SIZE];
    let n = SIZE as f32;
    for t in &seen {
        let p = t.map(|q| Vec2::new((q.x - mid.x) * s + 0.5, (q.y - mid.y) * s + 0.5) * n);
        let area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
        if area.abs() < 1e-9 {
            continue;
        }
        let (a, b) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        let span = |lo: f32, hi: f32| ((lo - 0.5).ceil().max(0.0) as isize)..=((hi - 0.5).floor() as isize).min(SIZE as isize - 1);
        for y in span(a.y, b.y) {
            for x in span(a.x, b.x) {
                if mask[y as usize * SIZE + x as usize] != 0 {
                    continue;
                }
                let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w = [(p[1] - p[0]).perp_dot(c - p[0]), (p[2] - p[1]).perp_dot(c - p[1]), (p[0] - p[2]).perp_dot(c - p[2])];
                if w.iter().all(|w| w * area.signum() >= 0.0) {
                    mask[y as usize * SIZE + x as usize] = 255;
                }
            }
        }
    }
    Some(Silhouette { rows, mask })
}

/// The atlas the terrain samples: [`MAX`] silhouettes one under the other (R8).
pub fn atlas() -> Image {
    let mut i = Image::new_fill(Extent3d { width: SIZE as u32, height: (SIZE * MAX) as u32, depth_or_array_layers: 1 }, TextureDimension::D2, &[0], TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
    i.sampler = bevy::image::ImageSampler::linear();
    i
}

/// One row of the table of [`rows_image`]: three texels of four floats.
const ROW_BYTES: usize = 3 * 16;

/// The table the terrain reads: each silhouette's three [`Silhouette::rows`], one line each; and in a last line (x how
/// many, y what each takes).
pub fn rows_image() -> Image {
    Image::new_fill(Extent3d { width: 3, height: MAX as u32 + 1, depth_or_array_layers: 1 }, TextureDimension::D2, &[0; 16], TextureFormat::Rgba32Float, RenderAssetUsages::RENDER_WORLD)
}

/// A mesh's triangles in the world.
fn triangles(mesh: &Mesh, at: &GlobalTransform, out: &mut Vec<[Vec3; 3]>) {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let w: Vec<Vec3> = pos.iter().map(|p| at.transform_point(Vec3::from(*p))).collect();
    let mut tri = |a: usize, b: usize, c: usize| {
        if let (Some(a), Some(b), Some(c)) = (w.get(a), w.get(b), w.get(c)) {
            out.push([*a, *b, *c]);
        }
    };
    match mesh.indices() {
        Some(Indices::U16(i)) => i.chunks_exact(3).for_each(|t| tri(t[0] as usize, t[1] as usize, t[2] as usize)),
        Some(Indices::U32(i)) => i.chunks_exact(3).for_each(|t| tri(t[0] as usize, t[1] as usize, t[2] as usize)),
        None => (0..w.len() / 3).for_each(|k| tri(3 * k, 3 * k + 1, 3 * k + 2)),
    }
}

/// `f` over the items, on the compute pool when there is more than one.
fn par_map<T: Send, R: Send + 'static>(items: Vec<T>, f: impl Fn(T) -> R + Sync) -> Vec<R> {
    if items.len() < 2 {
        return items.into_iter().map(f).collect();
    }
    let f = &f;
    bevy::tasks::ComputeTaskPool::get().scope(|scope| {
        for t in items {
            scope.spawn(async move { f(t) });
        }
    })
}

/// The box in the world that holds what `aabb` (in the mesh's space) holds, from its corners without visiting them.
fn world_box(aabb: &Aabb, at: &GlobalTransform) -> (Vec3, Vec3) {
    let a = at.affine();
    let (c, m) = (a.transform_point3a(aabb.center), a.matrix3);
    let h = m.x_axis.abs() * aabb.half_extents.x + m.y_axis.abs() * aabb.half_extents.y + m.z_axis.abs() * aabb.half_extents.z;
    ((c - h).into(), (c + h).into())
}

/// Whether the middle of something inside this box can be within [`RANGE`] of `eye`.
fn may_reach(eye: Vec3, (lo, hi): (Vec3, Vec3)) -> bool {
    eye.distance_squared(eye.clamp(lo, hi)) <= RANGE * RANGE
}

/// What was worked out last for an object whose parts have not moved (a tank parked, or still): the same silhouette.
/// Not for the skinned ones: their vertices change where the transforms do not show it.
struct Known {
    placed: Vec<(AssetId<Mesh>, [u32; 12])>,
    mid: Vec3,
    silhouette: Option<std::sync::Arc<Silhouette>>,
}

/// The objects known, the sun they were seen from, and what is in the material and its atlas now.
#[derive(Default)]
pub struct Cache {
    known: std::collections::HashMap<usize, Known>,
    sun: Vec3,
    /// (another level, or the same one again: another material, with nothing in it)
    material: Option<AssetId<crate::level::TerrainMaterial>>,
    shown: Vec<std::sync::Arc<Silhouette>>,
    /// `World::bounding_radius` by template
    radius: std::collections::HashMap<String, f32>,
}

fn placed(parts: &[(&Handle<Mesh>, GlobalTransform)]) -> Vec<(AssetId<Mesh>, [u32; 12])> {
    parts.iter().map(|(m, at)| (m.id(), at.affine().to_cols_array().map(f32::to_bits))).collect()
}

/// Every frame, with the setting on: the silhouettes of the nearest casters into the terrain's material. The boxes
/// say which objects can be near, so that only their triangles are read (some 1,3 objects a frame in
/// Berlin, of all the soldiers and vehicles); the box of the triangles still decides, as it always did.
// TODO: some 3,4 ms a frame with ~11 soldiers in range: each is ~5000 triangles and none can be kept from the frame before. A coarser mesh for the silhouette?
#[allow(clippy::type_complexity)]
pub fn update(
    casters: Query<(&Caster, &Mesh3d, &GlobalTransform, &InheritedVisibility, Option<&bevy::camera::visibility::RenderLayers>, Option<&Aabb>, Has<crate::play::SkinFrames>, Option<&ShadowMesh>)>,
    camera: Query<&GlobalTransform, With<crate::play::PlayCamera>>,
    terrain: Query<&MeshMaterial3d<crate::level::TerrainMaterial>>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<crate::level::TerrainMaterial>>,
    sun: Option<Res<crate::level::ShadowLight>>,
    mut cache: Local<Cache>,
    mut frame: ResMut<ShadowFrame>,
    game: Option<Res<crate::play::Game>>,
) {
    let (Ok(mat), Some(sun)) = (terrain.single(), sun) else { return };
    let on = crate::assets::graphics().object_shadows;
    if cache.sun != sun.dir || cache.material != Some(mat.0.id()) {
        *cache = Cache { sun: sun.dir, material: Some(mat.0.id()), ..Default::default() };
    }
    let mut found: Vec<(usize, std::sync::Arc<Silhouette>)> = Vec::new();
    if on && let Ok(eye) = camera.single() {
        // (not the arms and weapon drawn before the eyes)
        // nor a soldier in a vehicle: the original draws none for a seated pilot, closed or open cockpit (MiG-29, F-14B,
        // F-15C, Bf109, F11 in the sun); its vehicle's silhouette is the vehicle's own. Level 2: in the open seats
        // measured (Willys, Humvee, M1A1's gunner, UH-60's door, ZPU-4) his shadow would fall within the vehicle's, unseen either way
        let seated = |i: usize| game.as_ref().and_then(|g| g.session.sim.vehicles.get(i)).is_some_and(|v| v.entry.inside.is_some());
        let drawn = casters.iter().filter(|(c, _, _, seen, layers, _, _, _)| seen.get() && layers.is_none_or(|l| l.intersects(&bevy::camera::visibility::RenderLayers::layer(0))) && !seated(c.0));
        // each object's box; one without a box yet (its first frame) counts as everywhere
        let mut boxes: std::collections::HashMap<usize, (Vec3, Vec3)> = Default::default();
        for (c, _, at, _, _, aabb, _, _) in drawn.clone() {
            let (lo, hi) = aabb.map_or((Vec3::MIN, Vec3::MAX), |b| world_box(b, at));
            let b = boxes.entry(c.0).or_insert((lo, hi));
            *b = (b.0.min(lo), b.1.max(hi));
        }
        boxes.retain(|_, b| may_reach(eye.translation(), *b));
        // (a piece with a shadow LOD puts that in, not the mesh drawn)
        let mut parts: std::collections::HashMap<usize, (bool, Vec<(&Handle<Mesh>, GlobalTransform)>)> = Default::default();
        if !boxes.is_empty() {
            for (c, m, at, _, _, _, skinned, shadow) in drawn {
                if boxes.contains_key(&c.0) {
                    let e = parts.entry(c.0).or_default();
                    e.0 |= skinned;
                    e.1.push((shadow.map_or(&m.0, |s| &s.0), *at));
                }
            }
        }
        let eye = eye.translation();
        cache.known.retain(|i, _| parts.contains_key(i));
        // the objects that have not moved keep what they had; the others read their triangles
        let (mut still, mut moved) = (Vec::new(), Vec::new());
        for (i, (skinned, list)) in parts {
            let now = placed(&list);
            match cache.known.get(&i) {
                Some(k) if !skinned && k.placed == now => still.push((i, k.mid, k.silhouette.clone(), list)),
                _ => moved.push((i, skinned, now, list)),
            }
        }
        let meshes = &*meshes;
        let tris = |list: &[(&Handle<Mesh>, GlobalTransform)]| {
            let mut t = Vec::new();
            for (m, at) in list {
                if let Some(mesh) = meshes.get(*m) {
                    triangles(mesh, at, &mut t);
                }
            }
            let (lo, hi) = t.iter().flatten().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            (t, (lo + hi) * 0.5)
        };
        enum Have {
            Silhouette(std::sync::Arc<Silhouette>),
            Triangles(Vec<[Vec3; 3]>),
        }
        let read = par_map(moved, |(i, skinned, now, list)| {
            let (t, mid) = tris(&list);
            (i, skinned, now, mid, t)
        });
        let mut near: Vec<(f32, usize, Have)> = Vec::new();
        for (i, skinned, now, mid, t) in read {
            let d = eye.distance(mid);
            if !skinned {
                cache.known.insert(i, Known { placed: now, mid, silhouette: None });
            }
            if d <= RANGE {
                near.push((d, i, Have::Triangles(t)));
            }
        }
        for (i, mid, known, list) in still {
            let d = eye.distance(mid);
            if d <= RANGE {
                match known {
                    Some(s) => near.push((d, i, Have::Silhouette(s))),
                    // (it was among the ones left out: it has none yet)
                    None => near.push((d, i, Have::Triangles(tris(&list).0))),
                }
            }
        }
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.truncate(MAX);
        // each one's bounding sphere, for the original's square (the radius by template, kept)
        let mut spheres: std::collections::HashMap<usize, (Vec3, f32)> = Default::default();
        if let Some(g) = game.as_ref() {
            let sim = &g.session.sim;
            for (_, i, have) in &near {
                let (Have::Triangles(_), Some(v)) = (have, sim.vehicles.get(*i)) else { continue };
                let r = *cache.radius.entry(v.template.clone()).or_insert_with(|| sim.world.bounding_radius(&v.template));
                spheres.insert(*i, (Vec3::from(v.craft.body.pos), r));
            }
        }
        let dir = sun.dir;
        let done = par_map(near, |(_, i, have)| {
            (
                i,
                match have {
                    Have::Silhouette(s) => Some(s),
                    Have::Triangles(t) => silhouette(&t, dir, spheres.get(&i).copied()).map(std::sync::Arc::new),
                },
            )
        });
        for (i, s) in done {
            let Some(s) = s else { continue };
            if let Some(k) = cache.known.get_mut(&i) {
                k.silhouette = Some(s.clone());
            }
            found.push((i, s));
        }
        // (in the order of the objects: the shader multiplies them, in any order, and the same ones keep their places)
        found.sort_by_key(|f| f.0);
    }
    // the same ones as last time: nothing to send
    if found.len() == cache.shown.len() && found.iter().zip(&cache.shown).all(|(a, b)| std::sync::Arc::ptr_eq(&a.1, b)) {
        return;
    }
    let Some(m) = materials.get(&mat.0) else { return };
    let (Some(atlas), Some(rows)) = (&m.extension.atlas, &m.extension.shadow_rows) else { return };
    cache.shown = found.into_iter().map(|f| f.1).collect();
    frame.number += 1;
    frame.atlas = atlas.id();
    frame.rows = rows.id();
    frame.alpha = sun.alpha;
    frame.shown = cache.shown.clone();
}

/// What the terrain's two textures should hold, written straight into them on the GPU. Changing the `Image`s or the
/// material instead makes Bevy prepare them again, which took some 4,7 ms a frame with soldiers near (Tobruk, Original,
/// 64 bots: from 68 to 100 fps without it, and the shader itself is cheap).
#[derive(Resource, Clone)]
pub struct ShadowFrame {
    number: u64,
    atlas: AssetId<Image>,
    rows: AssetId<Image>,
    alpha: f32,
    shown: Vec<std::sync::Arc<Silhouette>>,
}

impl Default for ShadowFrame {
    fn default() -> ShadowFrame {
        ShadowFrame { number: 0, atlas: AssetId::default(), rows: AssetId::default(), alpha: 0.0, shown: Vec::new() }
    }
}

/// On the render side: the last [`ShadowFrame`] seen, whether it is written, and what each row of the atlas holds now.
#[derive(Resource, Default)]
struct Written {
    frame: ShadowFrame,
    done: u64,
    atlas: AssetId<Image>,
    rows: Vec<Option<std::sync::Arc<Silhouette>>>,
}

fn extract(frame: bevy::render::Extract<Res<ShadowFrame>>, mut written: ResMut<Written>) {
    if frame.number != written.frame.number {
        written.frame = (**frame).clone();
    }
}

fn write(mut written: ResMut<Written>, images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>, queue: Res<bevy::render::renderer::RenderQueue>) {
    use bevy::render::render_resource::{Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect};
    let w = &mut *written;
    if w.frame.number == w.done {
        return;
    }
    // (their first frame: not on the GPU yet, the next one)
    let (Some(atlas), Some(rows)) = (images.get(w.frame.atlas), images.get(w.frame.rows)) else { return };
    if w.atlas != w.frame.atlas {
        w.atlas = w.frame.atlas;
        w.rows = vec![None; MAX];
    }
    let mut table = vec![0u8; ROW_BYTES * (MAX + 1)];
    let mut put = |x: usize, y: usize, v: Vec4| {
        for (k, c) in v.to_array().into_iter().enumerate() {
            table[y * ROW_BYTES + x * 16 + k * 4..][..4].copy_from_slice(&c.to_ne_bytes());
        }
    };
    put(0, MAX, Vec4::new(w.frame.shown.len() as f32, w.frame.alpha, 0.0, 0.0));
    let tex = |texture, y: u32| TexelCopyTextureInfo { texture, mip_level: 0, origin: Origin3d { x: 0, y, z: 0 }, aspect: TextureAspect::All };
    for (k, s) in w.frame.shown.iter().enumerate() {
        for (x, r) in s.rows.iter().enumerate() {
            put(x, k, *r);
        }
        if w.rows[k].as_ref().is_none_or(|had| !std::sync::Arc::ptr_eq(had, s)) {
            let layout = TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE as u32), rows_per_image: None };
            queue.write_texture(tex(&atlas.texture, (k * SIZE) as u32), &s.mask, layout, Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 });
            w.rows[k] = Some(s.clone());
        }
    }
    let layout = TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ROW_BYTES as u32), rows_per_image: None };
    queue.write_texture(tex(&rows.texture, 0), &table, layout, Extent3d { width: 3, height: MAX as u32 + 1, depth_or_array_layers: 1 });
    w.done = w.frame.number;
}

/// Sends the silhouettes to the GPU ([`ShadowFrame`]).
pub struct ShadowsPlugin;

impl Plugin for ShadowsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShadowFrame>();
        if let Some(r) = app.get_sub_app_mut(bevy::render::RenderApp) {
            r.init_resource::<Written>().add_systems(bevy::render::ExtractSchedule, extract).add_systems(bevy::render::Render, write.in_set(bevy::render::RenderSystems::PrepareResources));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(c: Vec3, h: f32) -> Vec<[Vec3; 3]> {
        let v = |x: f32, y: f32, z: f32| c + Vec3::new(x, y, z) * h;
        let q = [
            [v(-1., -1., -1.), v(1., -1., -1.), v(1., 1., -1.), v(-1., 1., -1.)],
            [v(-1., -1., 1.), v(1., -1., 1.), v(1., 1., 1.), v(-1., 1., 1.)],
            [v(-1., -1., -1.), v(-1., 1., -1.), v(-1., 1., 1.), v(-1., -1., 1.)],
            [v(1., -1., -1.), v(1., 1., -1.), v(1., 1., 1.), v(1., -1., 1.)],
            [v(-1., -1., -1.), v(1., -1., -1.), v(1., -1., 1.), v(-1., -1., 1.)],
            [v(-1., 1., -1.), v(1., 1., -1.), v(1., 1., 1.), v(-1., 1., 1.)],
        ];
        q.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect()
    }

    fn at(s: &Silhouette, p: Vec3) -> Option<u8> {
        let p = p.extend(1.0);
        let uv = Vec2::new(s.rows[0].dot(p), s.rows[1].dot(p));
        if s.rows[2].dot(p) > 0.0 || uv.cmplt(Vec2::ZERO).any() || uv.cmpge(Vec2::ONE).any() {
            return None;
        }
        let (x, y) = ((uv.x * SIZE as f32) as usize, (uv.y * SIZE as f32) as usize);
        Some(s.mask[y * SIZE + x])
    }

    #[test]
    fn the_world_box_holds_the_mesh_however_it_is_placed() {
        let aabb = Aabb { center: bevy::math::Vec3A::new(1.0, 2.0, -1.0), half_extents: bevy::math::Vec3A::new(0.5, 2.0, 1.5) };
        let at = GlobalTransform::from(Transform::from_xyz(100.0, 5.0, -30.0).with_rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 1.1)).with_scale(Vec3::new(1.0, 2.0, 0.5)));
        let (lo, hi) = world_box(&aabb, &at);
        for k in 0..8 {
            let s = Vec3::new(if k & 1 == 0 { -1.0 } else { 1.0 }, if k & 2 == 0 { -1.0 } else { 1.0 }, if k & 4 == 0 { -1.0 } else { 1.0 });
            let p = at.transform_point(Vec3::from(aabb.center) + Vec3::from(aabb.half_extents) * s);
            assert!(p.cmpge(lo - 1e-3).all() && p.cmple(hi + 1e-3).all(), "{p} outside {lo} {hi}");
        }
    }

    #[test]
    fn only_a_box_out_of_range_rules_an_object_out() {
        let eye = Vec3::new(10.0, 0.0, 0.0);
        assert!(may_reach(eye, (Vec3::splat(-1.0), Vec3::splat(1.0))));
        assert!(may_reach(eye, (Vec3::new(50.0, -1.0, -1.0), Vec3::new(60.0, 1.0, 1.0))), "40 m to its near side, though its middle is at 45");
        assert!(!may_reach(eye, (Vec3::new(56.0, -1.0, -1.0), Vec3::new(60.0, 1.0, 1.0))));
        assert!(may_reach(eye, (Vec3::MIN, Vec3::MAX)), "no box yet");
    }

    #[test]
    fn a_box_with_the_sun_overhead_shades_the_ground_under_it() {
        let s = silhouette(&cube(Vec3::new(10.0, 2.0, 5.0), 1.0), Vec3::Y, None).unwrap();
        assert_eq!(at(&s, Vec3::new(10.0, 0.0, 5.0)), Some(255));
        assert_eq!(at(&s, Vec3::new(10.9, 0.0, 4.1)), Some(255));
        assert_eq!(at(&s, Vec3::new(11.02, 0.0, 5.0)), Some(0), "the clear border");
        assert_eq!(at(&s, Vec3::new(14.0, 0.0, 5.0)), None);
        assert_eq!(at(&s, Vec3::new(10.0, 4.0, 5.0)), None, "nothing above it");
        assert!((123 * 123..=125 * 125).contains(&s.mask.iter().filter(|t| **t > 0).count()));
    }

    #[test]
    fn a_low_sun_casts_it_away_from_the_sun() {
        // the sun to the west (−x), 45° up: the shadow of a box 1 m over the ground falls from 0 to 4 m along +x
        let s = silhouette(&cube(Vec3::new(0.0, 2.0, 0.0), 1.0), Vec3::new(-1.0, 1.0, 0.0), None).unwrap();
        // (x + y from 0 to 4 over the box)
        assert_eq!(at(&s, Vec3::new(2.0, 0.0, 0.0)), Some(255));
        assert_eq!(at(&s, Vec3::new(3.9, 0.0, 0.0)), Some(255));
        assert_eq!(at(&s, Vec3::new(-2.0, 0.0, 0.0)), None);
    }
}
