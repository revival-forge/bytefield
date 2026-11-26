//! The reflection of vehicles and weapons: the original's `game.setEnvironmentMapping 1` (high
//! quality) on the materials with `envmap true` in their `.rs`. In the original's rendering: stage 1 `BLENDCURRENTALPHA(CURRENT,
//! TEXTURE)` on the level's cube (`ShaderManager.setTextureParam envmap <level>/Textures/ENVMAP_G_.rcm`, six `.dds`) by
//! the reflection vector, so where the texture's alpha is 1 the lit texture, where it is 0 the cube, unlit.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use bytefield_sim::World;

/// A material with `envmap true` (`shaders/envmap.wesl`): the `StandardMaterial` it would have, and the level's cube.
pub type EnvMapMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, EnvMap>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct EnvMap {
    #[texture(100, dimension = "cube")]
    #[sampler(101)]
    pub cube: Handle<Image>,
    #[uniform(102)]
    pub light: FixedLight,
}

/// The original's fixed-function light ([`crate::level::Atmosphere::fixed`], gamma): ambient, sun colour, the direction towards the sun
/// (the ambient's w: 1 with this light, 0 with Bevy's, −1 unlit; the sun's: 1 with the reflection, 0 without it; the diffuse's: 1 blended in gamma).
#[derive(Clone, Copy, Debug, Default, Reflect, bevy::render::render_resource::ShaderType)]
pub struct FixedLight {
    pub ambient: Vec4,
    pub diffuse: Vec4,
    pub sun: Vec4,
}

impl bevy::pbr::MaterialExtension for EnvMap {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/envmap.wesl".into()
    }
}

/// The level's cube and the materials made with it, one per `StandardMaterial`.
#[derive(Resource, Default)]
pub struct Cube {
    image: Option<Handle<Image>>,
    light: FixedLight,
    /// a 1×1 cube for the levels without one (the light alone)
    blank: Handle<Image>,
    made: HashMap<(AssetId<StandardMaterial>, bool, bool, bool, bool), Handle<EnvMapMaterial>>,
}

/// An entity whose `StandardMaterial` went for its [`EnvMapMaterial`]: the one to give back.
#[derive(Component)]
pub struct Reflecting(Handle<StandardMaterial>);

impl Cube {
    pub fn of(world: &World, images: &mut Assets<Image>) -> Cube {
        let image = world
            .db
            .globals
            .iter()
            .rev()
            .find(|c| c.command.eq_ignore_ascii_case("ShaderManager.setTextureParam") && c.args.first().is_some_and(|a| a.eq_ignore_ascii_case("envmap")))
            .and_then(|c| c.args.get(1))
            .and_then(|path| cube(&world.vfs, path))
            .map(|i| images.add(i));
        info!("envmap: {}", if image.is_some() { "the level's cube" } else { "none" });
        let atm = crate::level::Atmosphere::of(world);
        // (BYTEFIELD_NO_SUN: no sun on the lit meshes, the ambient alone; to tell whether the original lights a surface at all)
        let light = FixedLight { ambient: atm.fixed.0.extend(1.0), diffuse: if std::env::var_os("BYTEFIELD_NO_SUN").is_some() { Vec4::new(0.0, 0.0, 0.0, 1.0) } else { atm.fixed.1.extend(1.0) }, sun: atm.sun.extend(1.0) };
        let mut blank = Image::new_fill(Extent3d { width: 1, height: 1, depth_or_array_layers: 6 }, TextureDimension::D2, &[0; 4], TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::RENDER_WORLD);
        blank.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
        Cube { image, light, blank: images.add(blank), made: HashMap::new() }
    }
}

/// An `.rcm` (`[CubeMap]`, `PositiveX = <file>.dds` …) as a cube image; none if a face is missing or they differ in size.
fn cube(vfs: &bytefield_vfs::Vfs, path: &str) -> Option<Image> {
    let text = String::from_utf8_lossy(&vfs.read(&path.trim().trim_matches('"').replace('\\', "/")).ok()?).into_owned();
    let face = |key: &str| text.lines().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim().eq_ignore_ascii_case(key)).map(|(_, v)| v.trim().replace('\\', "/")));
    // (wgpu's layers in D3D's order and frame: the Refractor direction looks it up as it is)
    let faces: Vec<_> = ["PositiveX", "NegativeX", "PositiveY", "NegativeY", "PositiveZ", "NegativeZ"]
        .iter()
        .map(|k| bytefield_formats::dds::decode(&vfs.read(&face(k)?).ok()?).ok())
        .collect::<Option<_>>()?;
    let (w, h) = (faces[0].width, faces[0].height);
    if faces.iter().any(|f| (f.width, f.height) != (w, h)) {
        return None;
    }
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 6 },
        TextureDimension::D2,
        faces.iter().flat_map(|f| f.rgba.iter().copied()).collect(),
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    img.sampler = bevy::image::ImageSampler::linear();
    Some(img)
}

/// A skinned soldier, the first-person arms and the body from outside: its shader (`SkinningShader2Bones.vso`, `oD0 = c7·max(N·L, 0) + c8`) takes the
/// sun's colour without the material's `materialDiffuse` and its own ambient. Level 2: constants from the original's rendering
/// of the original on the knife; the same `c7`/`c8` on the body (poses f10–f12).
#[derive(Component)]
pub struct ArmsLight;
/// `c8` (`RS_AMBIENT` 0xff4c4c4c too; the same in Husky): not the level's.
const ARMS_AMBIENT: f32 = 0.3;

/// Gives the lit materials the original's light (setting `fixed_light`) and those with `envmap true` the level's cube
/// (setting `envmap`) as they appear, and all of them back or again when a setting changes.
#[allow(clippy::too_many_arguments)]
pub fn swap(
    mut commands: Commands,
    cache: Option<Res<crate::assets::Cache>>,
    cube: Option<ResMut<Cube>>,
    standard: Res<Assets<StandardMaterial>>,
    mut env: ResMut<Assets<EnvMapMaterial>>,
    added: Query<(Entity, &MeshMaterial3d<StandardMaterial>), Added<MeshMaterial3d<StandardMaterial>>>,
    all: Query<(Entity, &MeshMaterial3d<StandardMaterial>)>,
    reflecting: Query<(Entity, &Reflecting)>,
    arms: Query<(), With<ArmsLight>>,
    mut was: Local<Option<(bool, bool, bool)>>,
) {
    let (Some(cache), Some(mut cube)) = (cache, cube) else { return };
    let g = crate::assets::graphics();
    let now = (g.envmap && cube.image.is_some(), g.fixed_light, g.water_gamma);
    let changed = *was != Some(now);
    if changed && was.is_some() {
        for (e, r) in &reflecting {
            commands.entity(e).remove::<(MeshMaterial3d<EnvMapMaterial>, Reflecting)>().insert(MeshMaterial3d(r.0.clone()));
        }
    }
    *was = Some(now);
    if !now.0 && !now.1 && !now.2 {
        return;
    }
    // (without a cube, a blank one: only the light is used)
    let image = cube.image.clone().unwrap_or_else(|| cube.blank.clone());
    let candidates: Vec<(Entity, Handle<StandardMaterial>)> = if changed { all.iter().map(|(e, m)| (e, m.0.clone())).collect() } else { added.iter().map(|(e, m)| (e, m.0.clone())).collect() };
    for (e, mat) in candidates {
        let reflect = now.0 && cache.envmapped.contains(&mat.id());
        let diffuse = cache.lit.get(&mat.id()).copied();
        let fixed = now.1 && diffuse.is_some();
        // (an unlit blended one only to blend it in gamma: the Mi-24's blurred rotor, its dark blades at an alpha of ~0.1–0.3
        // over the sand, nearly vanished blended in linear light, and showed in the original)
        let plain = now.2 && cache.unlit_blended.contains(&mat.id());
        if !reflect && !fixed && !plain {
            continue;
        }
        let Some(base) = standard.get(&mat) else { continue };
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        // (the glass, blended as D3D8 does: `water_gamma`)
        let gamma = now.2 && matches!(base.alpha_mode, AlphaMode::Blend);
        // (the sun's colour × the material's diffuse, `materialDiffuse` of its `.rs`: D3D's A·Ma + D·Md·N·L with the
        // material's ambient at 1, as the original's `.rs` reader leaves it)
        // TODO: with Bevy's light (the enhanced profile) the material's diffuse is not used
        let is_arms = arms.contains(e);
        let md = if is_arms { Vec3::ONE } else { diffuse.unwrap_or(Vec3::ONE) };
        let ambient = if is_arms { Vec3::splat(ARMS_AMBIENT) } else { cube.light.ambient.truncate() };
        // (ambient.w 2: a skinned one, whose TF scales the sun's colour too: `c7` = diffuse × skin factor)
        // (ambient.w −1: unlit, its texture as it is)
        let light = FixedLight { ambient: ambient.extend(if plain { -1.0 } else if fixed && is_arms { 2.0 } else { flag(fixed) }), sun: cube.light.sun.truncate().extend(flag(reflect)), diffuse: (cube.light.diffuse.truncate() * md).extend(flag(gamma)) };
        let h = cube.made.entry((mat.id(), reflect, fixed, gamma, is_arms)).or_insert_with(|| env.add(EnvMapMaterial { base: base.clone(), extension: EnvMap { cube: image.clone(), light } })).clone();
        commands.entity(e).remove::<MeshMaterial3d<StandardMaterial>>().insert((MeshMaterial3d(h), Reflecting(mat)));
    }
}
