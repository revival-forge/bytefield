//! The level that does not move: terrain, water, the placed static meshes and the fixed objects
//! the spawners make (guns without mobile physics stay where they are made: obstacles).
//! What moves (vehicles, soldiers) is drawn from the simulation (`play`).

use bevy::prelude::*;
use bytefield_sim::World;
use bytefield_world::scene::{self, Mat4};

use crate::assets::{self, Cache};
use crate::model;

/// The terrain's material: the colour tiles (the base's texture) times the detail texture ×2
/// (`shaders/terrain.wesl`).
pub type TerrainMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, Detail>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct Detail {
    /// x: detail repeats per metre; yzw: [`Atmosphere::shade`] (the sun's, the contact shadows' and SSAO's)
    #[uniform(100)]
    pub scale: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub texture: Option<Handle<Image>>,
    /// the original's shadows of soldiers and vehicles (`shadows.rs`)
    #[texture(103, sample_type = "float", filterable = false)]
    pub shadow_rows: Option<Handle<Image>>,
    #[texture(104)]
    #[sampler(105)]
    pub atlas: Option<Handle<Image>>,
    /// x: the world's far plane ([`Atmosphere::far`]), past which (in the view's depth) the terrain is not drawn
    #[uniform(106)]
    pub clip: Vec4,
}

/// How the level's objects cast their shadows ([`Atmosphere::object_shadow`]): from where (Bevy's axes) and how dark.
#[derive(Resource)]
pub struct ShadowLight {
    pub dir: Vec3,
    pub alpha: f32,
}

impl bevy::pbr::MaterialExtension for Detail {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/terrain.wesl".into()
    }
}

/// A tree's material (`shaders/tree.wesl`): the `StandardMaterial` it would have (its texture and alpha), the original's
/// light by vertex, what it keeps of it in a shadow, and the sprites turned to the camera.
pub type TreeMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, TreeShade>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct TreeShade {
    #[uniform(100)]
    pub light: TreeLight,
}

#[derive(Clone, Copy, Debug, Reflect, bevy::render::render_resource::ShaderType)]
pub struct TreeLight {
    /// [`Atmosphere::tree_shade`] (rgb)
    pub shade: Vec4,
    /// `renderer.ambientColor` + `renderer.globalAmbientColor`, in the screen's space
    pub ambient: Vec4,
    /// `renderer.diffuseColor`, in the screen's space
    pub diffuse: Vec4,
    /// towards the sun
    pub sun: Vec4,
    /// the tree's TEXTUREFACTOR, in the screen's space (`bytefield_world::shadow`): rgb from how lit the terrain says the tree
    /// stands, alpha 1 (its alpha in the original is the fade towards the billboard: `fade`)
    pub factor: Vec4,
    /// x 1: what is blended (branches, sprites) takes an alpha that makes Bevy's linear blending come out as D3D8's in the
    /// screen's space, from the copy of the world under it; 0, as it is
    pub blend: Vec4,
    /// rgb: the colour of the sprites (the leaves), the vertex shader's `c11`: the GeometryTemplate's `lightColor`, 1
    /// without it. Above 1
    /// too (Salerno's aspens, 1,3): MODULATE2X clamps.
    /// TODO: `shadowColor` (Kharkov, XPack1, Eagles_Nest): not seen at work; every tree with it stood lit in the reference run
    pub sprite: Vec4,
    /// The fade of the branches and the leaves towards the billboard:
    /// x `billboardDistance` × `billboardDistanceScale` (0: the species has no billboard), y `billboardFadeScale`, z 1 for
    /// `useNewBillboardFadeFunc`. See [`TreeFade`].
    pub fade: Vec4,
}

/// A tree that gives way to its billboard far off. The client's tree renderer:
/// with the view's half vertical angle k (its `getFieldOfView`: 0,5 at `renderer.fieldOfView 1`; the
/// fitting of 21 trees in a reference run in Salerno gives it, to 0,001) and the distance d on the ground plane, past
/// D = `billboardDistance` × the tree's largest scale × `billboardDistanceScale` (1) only the billboard is drawn; from
/// D − F on (F = D × `billboardFadeScale`, 0,5) both: t = (d·k − (D − F)) / F, the mesh's alpha (its TEXTUREFACTOR's,
/// on the blended branches and leaves) cos(t·π/2) and the billboard's sin(t·π/2) (`useNewBillboardFadeFunc`, on by
/// default; the old: 1 − t³ and t). The alpha goes in `tree.wesl`; hiding the whole mesh past D, here (`fade_trees`).
#[derive(Component, Clone, Copy, Debug)]
pub struct TreeFade {
    /// where it stands (x, z)
    pub at: Vec2,
    /// D
    pub far: f32,
}

/// The client's `TreeRenderer` settings (only `billboardLightScale` is set by
/// any level, in its `Init.con`).
#[derive(Clone, Copy, Debug)]
pub struct TreeRenderer {
    pub distance_scale: f32,
    pub fade_scale: f32,
    /// the billboard's colour: the tree's TEXTUREFACTOR × this (`updateBillboardColor`)
    pub light_scale: Vec3,
    pub new_fade: bool,
}

impl TreeRenderer {
    pub fn of(world: &World) -> TreeRenderer {
        let v: Vec<f32> = world.db.globals.iter().rev().find(|c| c.command.eq_ignore_ascii_case("treeRenderer.billboardLightScale")).and_then(|c| c.args.first()).map_or(Vec::new(), |a| a.split('/').filter_map(|x| x.trim().parse().ok()).collect());
        let light_scale = match v[..] {
            [g] => Vec3::splat(g),
            [r, g, b] => Vec3::new(r, g, b),
            _ => Vec3::splat(0.5),
        };
        TreeRenderer { distance_scale: 1.0, fade_scale: 0.5, light_scale, new_fade: true }
    }

    /// The `fade` of a species' [`TreeLight`] (`billboardDistance`, 0 without a billboard).
    fn fade(&self, distance: f32) -> Vec4 {
        Vec4::new(distance * self.distance_scale, self.fade_scale, if self.new_fade { 1.0 } else { 0.0 }, 0.0)
    }
}

/// The world's eye's half vertical angle: the factor on the distance of the trees' fade ([`TreeFade`]).
/// k = 0,5 fitted with the default view (`renderer.fieldOfView` 1), ∝ the view's FOV as in the client; the camera's vehicle-FOV setter
/// sets that same FOV, so a vehicle's `vehicleFov` moves it
/// as here.
/// TODO: whether a weapon's zoom goes through that FOV too (here it does, as the projection's): a reference run while zoomed
/// (with the zoom on) or the original client's zoom code.
pub fn fade_factor(p: &Projection) -> f32 {
    match p {
        Projection::Perspective(p) => 0.5 * p.fov,
        _ => 0.5,
    }
}

/// Hides the meshes of the trees past their billboard's distance ([`TreeFade`]); before the transforms propagate.
pub fn fade_trees(eye: Query<(&Transform, &Projection), (With<WorldCamera>, Without<ReflectionCamera>)>, mut trees: Query<(&TreeFade, &mut Visibility)>) {
    let Ok((tr, p)) = eye.single() else { return };
    let (at, k) = (Vec2::new(tr.translation.x, tr.translation.z), fade_factor(p));
    for (f, mut v) in &mut trees {
        let want = if at.distance(f.at) * k >= f.far { Visibility::Hidden } else { Visibility::Inherited };
        v.set_if_neq(want);
    }
}

/// The sway of a tree (level 2: the form; the original's `Rand` gives the direction and the phases,
/// they cannot be matched). The client's tree draw asks the swayer for its update with (t, template, f)
/// and multiplies `getSwayMatrix` (a turn about the local x by `angleX`, then about
/// z by `angleZ`, in degrees) before the tree's world matrix, i.e. in its own space, before its scale. With
/// s = sin(clamp(p·k·(2π·t + φ), ±1000)): angleX = R·dx·s·f, angleZ = R·dz·s·f, where R is `swayRangeDegrees` (1 by
/// default; 1.1 in the data), p `swayPeriodSeconds` (0.2 by default, used as a rate: a cycle every 5/k s), (dx, dz) the
/// template's `swayDirection` (by default (cos θ, sin θ) with θ at random per template), k ∈ [1, 1.4) and φ ∈ [0, 2π) at
/// random per tree, t the `TreeRenderer`'s clock (the frames' steps since the game started, never reset). The ±1000 rad
/// clamp stops each tree after (5000/k − φ)/2π s, 9.5 to 13 minutes, leant R·sin(1000) = 0.83·R: in the Original
/// profile, here too. f fades it with the distance d to the eye: (L² − d²)/F² in [0, 1], L = 40 m
/// × the zoom (tan(fov₀/2)/tan(fov/2)), F = 0.7·L. Midway's palms in
/// a reference run: one plane per species, through 0, 1.05–1.17° at most (1.1 × the tree's horizontal
/// scale over its vertical one: the turn is before the scale), half a cycle every 64–73 frames.
/// TODO: `swayWindFactor` (0 by default, no data sets it): a second sine across the direction, with the rate
/// p·u/2 drawn in `setSwayPeriodSeconds`; and the bushes' and the collision's sway (`bushSwayEnable`,
/// `CollisionSwayer`, `geometry.applySwayForce`), unused by the data. The 5 StandardMesh templates with `swayEnable 1`
/// (one in vanilla, three in DC, one in GC): whether `StandardMeshRenderer` sways them, not looked at.
#[derive(Component, Clone, Copy, Debug)]
pub struct TreeSway {
    /// where it stands without the sway
    pub base: Transform,
    /// degrees about the local x and z at s·f = 1: R·(dx, dz), each × the tree's scale across over its scale up (the turn
    /// before a non-uniform scale is a shear; this keeps the trunk's lean)
    pub amp: Vec2,
    /// the sine's argument: rate·t + phase (p·k·2π and p·k·φ)
    pub rate: f32,
    pub phase: f32,
}

impl TreeSway {
    /// A TreeMesh tree's, when its GeometryTemplate has `swayEnable 1`.
    fn of(db: &bytefield_con::templates::TemplateDb, geometry: &str, instance: usize, base: Transform) -> Option<TreeSway> {
        let t = db.get("geometrytemplate", geometry)?;
        let num = |k: &str| t.get(k).and_then(|a| a.first()).and_then(|a| a.trim().parse::<f32>().ok());
        if num("swayenable").unwrap_or(0.0) == 0.0 {
            return None;
        }
        let range = num("swayrangedegrees").unwrap_or(1.0);
        let period = num("swayperiodseconds").unwrap_or(0.2);
        // (the original's draws: one θ per template, one k and φ per tree; here from their names, the same every run)
        let unit = |key: &str, salt: u64| {
            let h = key.bytes().fold(0xcbf2_9ce4_8422_2325u64 ^ salt, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
            (h >> 40) as f32 / (1u64 << 24) as f32
        };
        let dir = t.get("swaydirection").and_then(|a| a.first()).map(|a| a.split('/').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()).filter(|v| v.len() == 2).map(|v| Vec2::new(v[0], v[1]).normalize_or_zero()).unwrap_or_else(|| Vec2::from_angle(unit(&geometry.to_ascii_lowercase(), 1) * std::f32::consts::TAU));
        let id = format!("{instance}");
        let k = 1.0 + 0.4 * unit(&id, 2);
        let phi = unit(&id, 3) * std::f32::consts::TAU;
        let s = base.scale.abs();
        let up = s.y.max(1e-6);
        Some(TreeSway { base, amp: Vec2::new(dir.x * s.z / up, dir.y * s.x / up) * range, rate: period * k * std::f32::consts::TAU, phase: period * k * phi })
    }

    /// Its transform at the clock `t`, `d2` the squared distance to the eye, `zoom` the view's ([`TreeSway`]).
    pub fn at(&self, t: f64, d2: f32, zoom: f32, freeze: bool) -> Transform {
        let l = 40.0 * zoom;
        let f = 0.7 * l;
        if d2 >= l * l {
            return self.base;
        }
        let fade = ((l * l - d2) / (f * f)).clamp(0.0, 1.0);
        let arg = self.rate as f64 * t + self.phase as f64;
        // (without the freeze, the same sine without its clamp: in f64, wrapped)
        let s = if freeze { (arg as f32).clamp(-1000.0, 1000.0).sin() } else { arg.rem_euclid(std::f64::consts::TAU).sin() as f32 };
        let a = self.amp * (s * fade);
        Transform { rotation: self.base.rotation * Quat::from_rotation_x(a.x.to_radians()) * Quat::from_rotation_z(a.y.to_radians()), ..self.base }
    }
}

/// The view's zoom, tan(fov₀/2)/tan(fov/2), fov₀ the starting one: 1 without it.
fn fov_modifier(p: &Projection) -> f32 {
    match p {
        Projection::Perspective(p) => ((crate::settings::world_fov() * 0.5).tan() / (p.fov * 0.5).tan().max(1e-6)).max(0.0),
        _ => 1.0,
    }
}

/// Sways the trees near the eye ([`TreeSway`]); before the transforms propagate.
pub fn sway_trees(time: Res<Time>, eye: Query<(&Transform, &Projection), (With<WorldCamera>, Without<ReflectionCamera>, Without<TreeSway>)>, mut trees: Query<(&TreeSway, &mut Transform)>) {
    let Ok((tr, p)) = eye.single() else { return };
    let (at, zoom, t) = (tr.translation, fov_modifier(p), time.elapsed_secs_f64());
    let freeze = assets::graphics().profile == bytefield_content::Profile::Original;
    for (s, mut tr) in &mut trees {
        let want = s.at(t, at.distance_squared(s.base.translation), zoom, freeze);
        if *tr != want {
            *tr = want;
        }
    }
}

/// The billboards of one species: the views of the tree side by side in
/// `TreeMesh/Billboards/<file>.dds` and a quad per tree, turned to the eye (on the ground plane) on the shader.
/// Blended in the screen's space over the copy of the world drawn so far, as the branches (`gamma_alpha`, with the
/// setting `water_gamma`).
pub type BillboardMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, BillboardShade>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct BillboardShade {
    /// x the views in the texture, y `billboardFadeScale`, z 1 for `useNewBillboardFadeFunc`
    #[uniform(100)]
    pub params: Vec4,
}

impl bevy::pbr::MaterialExtension for BillboardShade {
    fn vertex_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/billboard.wesl".into()
    }
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/billboard.wesl".into()
    }
    // (`BILLBOARD_FIRST` is only for the order: no bias in the depth test)
    fn specialize(_: &bevy::pbr::MaterialExtensionPipeline, descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor, _: &bevy::mesh::MeshVertexBufferLayoutRef, _: bevy::pbr::MaterialExtensionKey<Self>) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if let Some(d) = descriptor.depth_stencil.as_mut() {
            d.bias.constant = 0;
        }
        Ok(())
    }
}

/// The billboards' `depth_bias`: before every other blended thing. A species' billboards are one mesh over the whole
/// level, and Bevy sorts what is blended by the centre of each mesh: when that centre fell nearer than a tree in front,
/// the billboards were drawn over its branches (a white fir of the fog over a near one in Battle of the Bulge).
/// They only show past where the trees' meshes end.
const BILLBOARD_FIRST: f32 = -1.0e6;

/// One tree's billboard quad: its foot (the original's position, y lowered by the box's floor × the scale), the half
/// width (the box's largest |x|, |z| × the x scale), the height (the box's × the y scale), D ([`TreeFade`]), its
/// forward and right in the original's axes (x, z; for the view it shows), its up and its colour.
struct BillboardQuad {
    foot: Vec3,
    size: Vec3,
    axes: Vec4,
    up: Vec3,
    colour: Vec3,
}

/// A species' billboards as one mesh: four corners a tree, everything the shader needs in the standard attributes
/// (position: the foot; normal: half width, height, D; uv: the corner; uv_b: up's x and z; tangent: the axes; colour).
fn billboard_mesh(quads: &[BillboardQuad]) -> Mesh {
    let (mut pos, mut nrm, mut uv, mut uvb, mut tan, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, q) in quads.iter().enumerate() {
        for c in [[-1.0, 0.0], [1.0, 0.0], [1.0, 1.0], [-1.0, 1.0]] {
            pos.push(q.foot.to_array());
            nrm.push(q.size.to_array());
            uv.push(c);
            uvb.push([q.up.x, q.up.z]);
            tan.push(q.axes.to_array());
            col.push([q.colour.x, q.colour.y, q.colour.z, 1.0]);
        }
        let b = 4 * i as u32;
        idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, Default::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uvb)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tan)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(bevy::mesh::Indices::U32(idx))
}

impl bevy::pbr::MaterialExtension for TreeShade {
    fn vertex_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/tree.wesl".into()
    }
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/tree.wesl".into()
    }
}

/// A static with its own lightmap: the
/// `StandardMaterial` it would have (its texture and alpha), lit only by the lightmap.
pub type BakedMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, Baked>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct Baked {
    /// the lightmap as the level's palette's colours ([`Lightmaps::image`]), on the mesh's second UVs
    #[texture(100)]
    #[sampler(101)]
    pub lightmap: Handle<Image>,
    /// [`Atmosphere::shade`] (rgb) with the sun's shadows, else 1; w: a `textureFade` piece's `textureFadeMaxDist`, else 0
    #[uniform(102)]
    pub shade: Vec4,
}

impl bevy::pbr::MaterialExtension for Baked {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/baked.wesl".into()
    }
}

/// The level's statics' lightmaps: `<level>/ObjectLightmaps/<name>.tga` ([`bytefield_world::scene::Draw::lightmap`]), an
/// 8-bit index into the level's `Palette.pal`. Only the intact geometry (`_m1`) is
/// drawn; a destroyed one's (`_m2`) waits for it.
struct Lightmaps {
    shade: Vec4,
    dir: String,
    palette: Vec<[u8; 3]>,
    images: std::collections::HashMap<String, Option<Handle<Image>>>,
    /// `texture/defaultTexture.dds`, for the statics with no map of their own ([`Lightmaps::image`])
    stand_in: Option<Option<Handle<Image>>>,
    made: std::collections::HashMap<(AssetId<StandardMaterial>, AssetId<Image>, u32), Handle<BakedMaterial>>,
}

impl Lightmaps {
    fn new(world: &World, shade: Vec4) -> Lightmaps {
        let dir = world.level.terrain.heightmap.rsplit_once('/').map_or("", |d| d.0).to_string();
        // (without the level's palette, the file's own: grey)
        let palette = world.vfs.read(&format!("{dir}/ObjectLightmaps/Palette.pal")).ok().and_then(|b| bytefield_formats::tga::riff_palette(&b)).unwrap_or_else(|| (0..=255).map(|i| [i; 3]).collect());
        Lightmaps { shade, dir, palette, images: Default::default(), stand_in: None, made: Default::default() }
    }

    /// The lightmap, as an image of the palette's colours (not sRGB: they go into the gamma space as they are). When the
    /// level has no file by that name (a static moved after its map was made: Husky's `rubble03_m1`, GC_Alaris_Prime's
    /// hangar), the original's texture manager hands over its stand-in for a texture it cannot find,
    /// `texture/defaultTexture.dds`, a flat (76, 72, 74), and the piece is drawn as the mapped ones.
    fn image(&mut self, vfs: &bytefield_vfs::Vfs, key: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        let (dir, palette) = (&self.dir, &self.palette);
        let own = self
            .images
            .entry(key.to_string())
            .or_insert_with(|| {
                let file = vfs.read(&format!("{dir}/ObjectLightmaps/{key}.tga")).ok()?;
                let (w, h, rgba) = if let Ok((w, h, idx)) = bytefield_formats::tga::indexed(&file) {
                    (w, h, idx.iter().flat_map(|&i| palette.get(i as usize).copied().unwrap_or([i; 3]).into_iter().chain([255])).collect::<Vec<u8>>())
                } else {
                    // (GC_Bespin_Night's are in 24 bits, with no palette: its lamps' colours)
                    let i = bytefield_formats::tga::decode(&file).ok()?;
                    (i.width as usize, i.height as usize, i.rgba)
                };
                Some(images.add(raw_image(w, h, rgba)))
            })
            .clone();
        own.or_else(|| {
            self.stand_in
                .get_or_insert_with(|| {
                    let i = vfs.read("texture/defaultTexture.dds").ok().and_then(|b| bytefield_formats::dds::decode(&b).ok())?;
                    Some(images.add(raw_image(i.width, i.height, i.rgba)))
                })
                .clone()
        })
    }

    /// A part's baked material, when it has a lightmap and takes one: drawn lit, opaque or cut by its alpha, with the
    /// lightmap's UVs. `fade`: a `textureFade` piece's far distance, blended in by `baked.wesl`.
    #[allow(clippy::too_many_arguments)]
    fn material(&mut self, vfs: &bytefield_vfs::Vfs, key: &str, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, fade: Option<f32>, meshes: &Assets<Mesh>, materials: &Assets<StandardMaterial>, images: &mut Assets<Image>, baked: &mut Assets<BakedMaterial>) -> Option<Handle<BakedMaterial>> {
        let base = materials.get(mat).filter(|m| !matches!(m.alpha_mode, AlphaMode::Blend))?;
        meshes.get(mesh).filter(|m| m.contains_attribute(Mesh::ATTRIBUTE_UV_1))?;
        let lightmap = self.image(vfs, key, images)?;
        let shade = self.shade.truncate().extend(fade.unwrap_or(0.0));
        let base = StandardMaterial { alpha_mode: if fade.is_some() { AlphaMode::Blend } else { base.alpha_mode }, ..base.clone() };
        Some(self.made.entry((mat.id(), lightmap.id(), shade.w.to_bits())).or_insert_with(|| baked.add(BakedMaterial { base, extension: Baked { lightmap, shade } })).clone())
    }
}

/// A lightmap's image, its bytes as they are (not sRGB), filtered.
fn raw_image(w: usize, h: usize, rgba: Vec<u8>) -> Image {
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        rgba,
        bevy::render::render_resource::TextureFormat::Rgba8Unorm,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::linear();
    img
}

/// The water's material (`shaders/water.wesl`): its two layers that move, its colour by the depth;
/// the depth comes in each vertex (`uv.x`).
pub type WaterMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, Water>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone)]
pub struct Water {
    /// `water.color`/`shallowColor` (rgb), `waterShallowAlpha` (`wateShallowAlpha`, in some levels, the
    /// engine does not register)
    #[uniform(100)]
    pub color: Vec4,
    /// `water.deepColor` (rgb), `waterAlphaDepth`
    #[uniform(100)]
    pub deep: Vec4,
    /// layer 1: scroll per second (xy), repeats per metre (z), the time in seconds (w); layer 2, with
    /// `waterColorDepth` (w, 0 when the level has none)
    #[uniform(100)]
    pub layer1: Vec4,
    #[uniform(100)]
    pub layer2: Vec4,
    /// the normal map: scroll per second (xy), repeats per metre (z)
    #[uniform(100)]
    pub normal: Vec4,
    /// `specularColor` (rgb, 0 without `specularEnable`) and whether it is on (w)
    #[uniform(100)]
    pub specular: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub tex1: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    pub tex2: Option<Handle<Image>>,
    #[texture(105)]
    #[sampler(106)]
    pub normal_map: Option<Handle<Image>>,
    /// the enhanced profile's reflection: its strength (x, 0 without it), the water's
    /// height (y)
    #[uniform(100)]
    pub reflect: Vec4,
    /// what [`ReflectionCamera`] draws, the screen's size
    #[texture(107)]
    #[sampler(108)]
    pub reflection: Option<Handle<Image>>,
    /// [`Atmosphere::shade`] (rgb) with the sun's shadows; (1, 1, 1) without them
    #[uniform(100)]
    pub shade: Vec4,
    /// the setting `water_fx`: its foam and refraction (x, 1 or 0); `water_gamma`: blended by hand, in gamma (y); see [`water_time`]
    #[uniform(100)]
    pub fx: Vec4,
    /// The direction of the sun in the highlight's cube map; w, the world's far plane ([`Atmosphere::far`])
    #[uniform(100)]
    pub sun: Vec4,
}

impl bevy::pbr::MaterialExtension for Water {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://bytefield/shaders/water.wesl".into()
    }
}

/// The water (`water.*` of the level's `Init.con`, `Terrain`'s `waterLevel`): the terrain's cells
/// below it, at its height, each vertex with its depth; around the map, deep water.
fn spawn_water(world: &World, commands: &mut Commands, meshes: &mut Assets<Mesh>, water_materials: &mut Assets<WaterMaterial>, cache: &mut Cache, images: &mut Assets<Image>) {
    let g = &world.db.globals;
    // the last of any of the names (`|`-separated: `water.color` and `water.shallowColor` are one setting)
    let last = |names: &str| g.iter().rev().find(|c| names.split('|').any(|n| c.command.eq_ignore_ascii_case(n))).and_then(|c| c.args.first()).map(|a| a.trim().to_string());
    // a single number fills a vector (`deepColor 0.5`)
    let nums = |name: &str, d: &[f32]| -> Vec<f32> {
        let v: Vec<f32> = last(name).map(|a| a.split('/').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
        match v.len() {
            n if n == d.len() => v,
            1 => vec![v[0]; d.len()],
            _ => d.to_vec(),
        }
    };
    let tex = |name: &str, d: &str, cache: &mut Cache, images: &mut Assets<Image>| cache.texture(&world.vfs, &last(name).unwrap_or(d.into()), images);
    let layer = |dir: &str, speed: &str, tile: &str| {
        let d = nums(dir, &[1.0, 0.0]);
        let s = nums(speed, &[0.02])[0];
        // Refractor's z is Bevy's -z: the texture is sampled on (x, -z)
        Vec4::new(d[0] * s, d[1] * s, nums(tile, &[0.5])[0] * water_tile(), 0.0)
    };
    let c = nums("water.color|water.shallowColor", &[0.77, 0.8, 0.6]);
    let (shallow_alpha, alpha_depth) = (nums("water.waterShallowAlpha", &[0.0])[0], nums("water.waterAlphaDepth", &[0.5])[0]);
    let deep = nums("water.deepColor", &[0.55, 0.6, 0.45]);
    let material = water_materials.add(WaterMaterial {
        base: StandardMaterial { alpha_mode: AlphaMode::Blend, unlit: true, fog_enabled: false, ..default() },
        extension: Water {
            color: Vec4::new(c[0], c[1], c[2], shallow_alpha),
            deep: Vec4::new(deep[0], deep[1], deep[2], alpha_depth),
            layer1: layer("water.scrollDirection1", "water.scrollLayer1", "water.tileLayer1"),
            layer2: layer("water.scrollDirection2", "water.scrollLayer2", "water.tileLayer2").with_w(nums("water.waterColorDepth", &[5.0])[0]),
            tex1: tex("water.texLayer1", "texture/water01", cache, images),
            tex2: tex("water.texLayer2", "texture/water02", cache, images),
            normal: layer("water.scrollDirectionNormalmap", "water.scrollNormalmap", "water.tileNormalmap"),
            specular: {
                let on = nums("water.specularEnable", &[1.0])[0] != 0.0;
                let c = nums("water.specularColor", &[0.75, 0.75, 0.75]);
                Vec4::new(c[0], c[1], c[2], if on { 1.0 } else { 0.0 })
            },
            normal_map: tex("water.normalMap", "texture/normalMap02", cache, images),
            reflect: Vec4::new(0.0, world.level.terrain.water_level as f32, 0.0, 0.0),
            reflection: None,
            shade: if assets::graphics().shadows { Atmosphere::of(world).shade } else { Vec3::ONE }.extend(1.0),
            fx: Vec4::ZERO,
            // the sun of the cube map of the highlight: flattened by `specularStreakFactor`
            sun: {
                let (s, streak) = (nums("sky.sunLightDirectionVec", &[-0.5, 0.7, 0.5]), nums("water.specularStreakFactor", &[0.001])[0]);
                Vec3::new(s[0], s[1] * streak, s[2]).normalize_or_zero().extend(Atmosphere::of(world).far())
            },
        },
    });
    let Some(mesh) = assets::water_mesh(&world.level, shallow_alpha, alpha_depth) else { return };
    let water = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material.clone()), WaterSurface(material))).id();
    // with the reflection, only the world's eye sees it (not the mirrored camera)
    if assets::graphics().reflections {
        commands.entity(water).insert(bevy::camera::visibility::RenderLayers::layer(WATER_LAYER));
    }
}

/// The water's layer when it reflects: [`WorldCamera`] sees it, [`ReflectionCamera`] does not.
const WATER_LAYER: usize = 3;

/// The statics' layer: the light of their own shadow map reaches only them. The world's eye sees it so
/// that its view has that light; the mirrored camera does not need it.
const STATIC_SHADOW_LAYER: usize = 4;
/// The merged silhouettes of the statics ([`ShadowProxies`]): the sun's and the statics' lights reach them, no camera sees them.
const SUN_PROXY_LAYER: usize = 5;
const STATIC_PROXY_LAYER: usize = 6;

/// The world's eye (the player's camera, `--fly`'s): the sky follows it and the water mirrors it.
#[derive(Component)]
pub struct WorldCamera;

/// What the world's eye sees: the world, the water when it reflects and the statics' shadow light.
pub fn world_layers() -> bevy::camera::visibility::RenderLayers {
    bevy::camera::visibility::RenderLayers::from_layers(&[0, WATER_LAYER, STATIC_SHADOW_LAYER])
}

/// The camera that draws the world mirrored on the water's plane into the water's `reflection`, at half
/// the window's size: the original has none, the water only adds its highlight.
#[derive(Component)]
pub struct ReflectionCamera;

/// How much the water reflects at most (Schlick's Fresnel times this, and less where it is shallow): it
/// keeps its colour and the sand under it, as in the original. Tuned on Wake.
const REFLECTION: f32 = 0.55;

/// The mirrored camera follows the world's eye (before the transforms propagate and the projections are
/// made): reflected on the water's plane, its near plane on the water (oblique) so that nothing under
/// it shows, the culling inverted. Off while the eye is under the water. Made the first time, and its
/// image again when the window changes size.
pub fn reflect_water(
    mut commands: Commands,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    eye: Query<(&Transform, &Projection), (With<WorldCamera>, Without<ReflectionCamera>)>,
    mut mirror: Query<(&mut Transform, &mut Projection, &mut Camera, &mut bevy::camera::RenderTarget), With<ReflectionCamera>>,
    water: Query<&WaterSurface>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
    atm: Option<Res<Atmosphere>>,
) {
    if !assets::graphics().reflections {
        return;
    }
    let (Some(w), Ok((tr, Projection::Perspective(proj))), Some(atm)) = (water.iter().next(), eye.single(), atm) else { return };
    let Some(mat) = materials.get(&w.0) else { return };
    let h = mat.extension.reflect.y;
    let size = window.single().map_or(UVec2::new(1280, 720), |w| w.physical_size()).max(UVec2::splat(2)) / 2;
    let image = mat.extension.reflection.clone().filter(|i| images.get(i).is_some_and(|i| i.size() == size));
    let image = match image {
        Some(i) => i,
        None => {
            let i = images.add(Image::new_target_texture(size.x, size.y, bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb, None));
            if let Some(mut m) = materials.get_mut(&w.0) {
                if let Some(old) = m.extension.reflection.replace(i.clone()) {
                    images.remove(&old);
                }
            }
            i
        }
    };
    // TODO: off when no water is in view (Liberation_of_Caen: 21 fps with it on, 33 off, the river out of sight; every other frame does not help). Needs the terrain's height here
    let above = tr.translation.y > h + 0.05;
    use bevy::math::Mat4 as M;
    let plane = M::from_translation(Vec3::Y * h) * M::from_mat3a(bevy::math::reflection_matrix(Vec3::Y)) * M::from_translation(Vec3::NEG_Y * h);
    let mirrored = Transform::from_matrix(plane * tr.to_matrix());
    // the plane in the eye's view space with the water's underside as its front: what the mirrored camera
    // sees of the world is what the eye would see of the world mirrored
    let normal = tr.rotation.inverse() * Vec3::NEG_Y;
    let projection = PerspectiveProjection { near_clip_plane: normal.extend(h - tr.translation.y), ..proj.clone() };
    let target = bevy::camera::RenderTarget::Image(image.clone().into());
    if let Ok((mut t, mut p, mut c, mut rt)) = mirror.single_mut() {
        *t = mirrored;
        *p = Projection::Perspective(projection);
        c.is_active = above;
        if !matches!(&*rt, bevy::camera::RenderTarget::Image(i) if i.handle == image) {
            *rt = target;
        }
    } else {
        commands.spawn((
            Camera3d::default(),
            Camera { order: -1, invert_culling: true, is_active: above, ..default() },
            target,
            mirrored,
            Projection::Perspective(projection),
            atm.camera(),
            ReflectionCamera,
        )).insert(bevy::render::view::Msaa::Off);
    }
    if let Some(mut m) = materials.get_mut(&w.0) {
        m.extension.reflect.x = if above { REFLECTION } else { 0.0 };
    }
}

/// The water's material, to move its layers.
#[derive(Component)]
pub struct WaterSurface(Handle<WaterMaterial>);

/// TODO: with `water_fx` (experimental) a soldier's backpack in the water came out white and the shore got a white line
/// (Wake).
/// The water's time (its layers move with it), and the setting `water_fx`: with it the water reads the
/// world drawn before it (Bevy's transmissive pass, on the camera that has `ScreenSpaceTransmission`).
pub fn water_time(time: Res<Time>, water: Query<&WaterSurface>, mut materials: ResMut<Assets<WaterMaterial>>) {
    let (fx, blend) = (assets::graphics().water_fx, assets::graphics().water_gamma);
    for w in &water {
        if let Some(mut m) = materials.get_mut(&w.0) {
            m.extension.layer1.w = time.elapsed_secs();
            m.extension.fx.x = if fx { 1.0 } else { 0.0 };
            m.extension.fx.y = if blend { 1.0 } else { 0.0 };
            // (always in the transmissive pass, before what is blended: `graphics::apply`)
            m.base.specular_transmission = 1.0;
        }
    }
}

/// Where the colour tiles lie: tiles of worldSize / (columns + 2 `texOffsetX`) metres (256 in all
/// of bf1942's), starting `texOffsetX` tiles from x 0 and `texOffsetY` from z 0 (Wake 2/2 with 4 × 4
/// in 2048: 512..1536; GuadalCanal 4/4 with 8, Midway 6/6 with 4, in 4096). Tobruk's 4/−10 (8 × 6 in
/// 4096) starts at z 0: a negative offset taken as 0, from that one level. Fitted on the edges of
/// the tiles against the heightmap's. The mesh's UV is x/worldSize.
fn tile_uv(world_size: f64, [cols, rows]: [usize; 2], [ox, oy]: [f64; 2]) -> bevy::math::Affine2 {
    let tile = world_size / (cols as f64 + 2.0 * ox);
    let total = (world_size / tile) as f32;
    let start = Vec2::new(ox as f32, oy.max(0.0) as f32);
    let size = Vec2::new(cols as f32, rows as f32);
    bevy::math::Affine2::from_scale_angle_translation(Vec2::splat(total) / size, 0.0, -start / size)
}

/// How many times the detail texture repeats per metre. Not in the original's rendering: the terrain (FVF 0x202, TEX2) carries
/// the detail's UVs in its vertices, not by a texture matrix. Tuned against the original's screenshots: 16 m until
/// then 8 by the grain of Cassino, Salerno and Tobruk (their ground −5 → −2, −5 → −2, −4 → 0; the four
/// mods' sweeps better or equal, `264d8*`).
fn detail_per_metre() -> f32 {
    std::env::var("BYTEFIELD_DETAIL_M").ok().and_then(|v| v.parse::<f32>().ok()).map_or(1.0 / DETAIL_M, |m| 1.0 / m)
}
const DETAIL_M: f32 = 8.0;

pub fn spawn(world: &World, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, terrain_materials: &mut Assets<TerrainMaterial>, water_materials: &mut Assets<WaterMaterial>, tree_materials: &mut Assets<TreeMaterial>, baked_materials: &mut Assets<BakedMaterial>, billboard_materials: &mut Assets<BillboardMaterial>, images: &mut Assets<Image>, cache: &mut Cache) {
    let lvl = &world.level;

    // terrain with its colour tiles
    // Every vanilla tile is 1024² (DXT1, its mips in the file): the GPU gets them whole as BC1;
    // this RGBA mosaic at 512 a tile (`BYTEFIELD_TILE_PX`) is the fallback without BC and what `Tiles` reads. (It used to go
    // to the GPU, down to `max_texture`: why 1024 here did not move the sweep.)
    let tile_px: usize = std::env::var("BYTEFIELD_TILE_PX").ok().and_then(|v| v.parse().ok()).unwrap_or(512);
    let mosaic = lvl.colour_mosaic(&world.vfs, tile_px).map(|(mut img, grid)| {
        // only the rows of tiles there are (Tobruk: 8 × 6)
        let row_bytes = img.width * tile_px * 4;
        let rows = (1..=grid).rev().find(|r| img.rgba[(r - 1) * row_bytes..r * row_bytes].iter().any(|&b| b != 0)).unwrap_or(grid);
        img.rgba.truncate(rows * row_bytes);
        img.height = rows * tile_px;
        (img, grid, rows)
    });
    let uv_transform = mosaic.as_ref().map_or(bevy::math::Affine2::IDENTITY, |(_, cols, rows)| tile_uv(lvl.terrain.world_size, [*cols, *rows], lvl.terrain.tex_offset));
    // (on the GPU, the tiles whole as BC1 when they can be; this RGBA one also feeds `Tiles`)
    let bc = mosaic.as_ref().and_then(|(_, grid, rows)| lvl.colour_tiles(&world.vfs).and_then(|(found, g)| (g == *grid).then(|| assets::bc_mosaic(&world.vfs, &lvl.terrain.tex_base, &found, g, *rows)).flatten()));
    let tex = mosaic.as_ref().map(|(img, _, _)| {
        let mut i = bc.unwrap_or_else(|| assets::rgba_image(img));
        // past the tiles (`texOffset`), their edge
        if let bevy::image::ImageSampler::Descriptor(d) = &mut i.sampler {
            d.address_mode_u = bevy::image::ImageAddressMode::ClampToEdge;
            d.address_mode_v = bevy::image::ImageAddressMode::ClampToEdge;
        }
        images.add(i)
    });
    let atm = Atmosphere::of(world);
    let detail = (!lvl.terrain.detail_tex.is_empty()).then(|| cache.texture(&world.vfs, &lvl.terrain.detail_tex, images)).flatten();
    let terrain_mat = terrain_materials.add(TerrainMaterial {
        base: StandardMaterial { base_color_texture: tex, uv_transform, unlit: true, fog_enabled: false, ..default() },
        extension: Detail { scale: Vec4::new(detail_per_metre(), atm.shade.x, atm.shade.y, atm.shade.z), texture: detail, shadow_rows: Some(images.add(crate::shadows::rows_image())), atlas: Some(images.add(crate::shadows::atlas())), clip: Vec4::new(atm.far(), 0.0, 0.0, 0.0) },
    });
    commands.insert_resource(ShadowLight { dir: atm.object_shadow.0, alpha: atm.object_shadow.1 });
    commands.spawn((Mesh3d(meshes.add(assets::terrain_mesh(lvl, 1))), MeshMaterial3d(terrain_mat)));

    spawn_water(world, commands, meshes, water_materials, cache, images);

    // static meshes (a vehicle placed in the level is the simulation's: `World::placed_vehicles`)
    // (also with the dynamic lights: its shader takes them; without the sun's shadows it keeps all its light)
    let g = assets::graphics();
    let tree_light = TreeLight { shade: if g.shadows { atm.tree_shade.extend(1.0) } else { Vec4::ONE }, ambient: atm.fixed.0.extend(1.0), diffuse: atm.fixed.1.extend(1.0), sun: atm.sun.extend(0.0), factor: Vec4::ONE, blend: Vec4::ZERO, sprite: Vec4::ONE, fade: Vec4::ZERO };
    let tiles = mosaic.as_ref().filter(|_| g.shadows).map(|(img, _, _)| Tiles { img, uv: uv_transform, world_size: lvl.terrain.world_size as f32 });
    let mut trees = Trees { light: tree_light, cast: g.shadows, made: std::collections::HashMap::new(), proxies: ShadowProxies::default() };
    // how lit each tree stands: the terrain's baked shadow and colour tiles
    let shadow = bytefield_world::shadow::ShadowSampler::new(&world.vfs, &world.db, lvl);
    let mut tree_boxes: std::collections::HashMap<String, Option<[[f32; 3]; 2]>> = std::collections::HashMap::new();
    let mut boxes = MeshBoxes::default();
    // and a small static's without a lightmap, as a vehicle's
    let mut tree_factor = |kind: &str, mesh: &str, at: Vec3, colour: [f32; 3]| -> [u8; 3] {
        if kind == "standardmesh" {
            if !g.mesh_shade {
                return [255; 3];
            }
            let Some((s, b)) = shadow.as_ref().zip(boxes.get(&world.vfs, mesh)).filter(|(s, b)| s.takes_shadow(*b)) else { return [255; 3] };
            return s.mesh_factor(s.mesh_intensity(lvl, [at.x, at.y, -at.z], b));
        }
        if kind != "treemesh" {
            return [255; 3];
        }
        let Some(s) = &shadow else { return bytefield_world::shadow::tree_factor(bytefield_world::shadow::LIT, [0.0; 3], colour) };
        let b = *tree_boxes.entry(mesh.to_string()).or_insert_with(|| world.vfs.read(&format!("treemesh/{mesh}.tm")).ok().and_then(|b| bytefield_formats::tm::parse(&b).ok()).map(|tm| tm.bounds));
        // (Bevy's z is the original's −z; a tree larger than the cull box, as most, stays lit)
        let i = b.map_or(bytefield_world::shadow::LIT, |b| s.mesh_intensity(lvl, [at.x, at.y, -at.z], b));
        bytefield_world::shadow::tree_factor(i, s.ambient, colour)
    };
    let mut lightmaps = g.lightmaps.then(|| Lightmaps::new(world, if g.shadows { atm.shade.extend(1.0) } else { Vec4::ONE }));
    let mut baked_parts = 0;
    let exp = scene::expand_level(&world.db);
    let mut drawn = 0;
    let mut objects: std::collections::BTreeMap<usize, Vec<(Part, ([u8; 3], [f32; 3], Option<(TreeFade, Vec4)>, Option<TreeSway>))>> = std::collections::BTreeMap::new();
    // the trees' billboards, by species: the `.tm`'s box and views, the texture, the quads
    let renderer = TreeRenderer::of(world);
    let mut tms: std::collections::HashMap<String, Option<([[f32; 3]; 2], u32)>> = std::collections::HashMap::new();
    let mut billboards: std::collections::BTreeMap<String, (u32, Vec<BillboardQuad>)> = std::collections::BTreeMap::new();
    for d in &exp.draws {
        if world.is_vehicle(&world.db.instances[d.instance].template) {
            continue;
        }
        let t = assets::to_transform(&d.world);
        let key = d.lightmap();
        let sprite = if d.kind == "treemesh" { light_color(&world.db, &d.geometry) } else { [1.0; 3] };
        let fade = (d.kind == "treemesh").then(|| billboard_distance(&world.db, &d.geometry)).flatten().map(|dist| {
            let far = dist * renderer.distance_scale * t.scale.abs().max_element();
            (TreeFade { at: Vec2::new(t.translation.x, t.translation.z), far }, renderer.fade(dist))
        });
        let sway = (d.kind == "treemesh").then(|| TreeSway::of(&world.db, &d.geometry, d.instance, t)).flatten();
        if let Some((f, _)) = fade {
            let tm = *tms.entry(d.mesh.to_ascii_lowercase()).or_insert_with(|| world.vfs.read(&format!("treemesh/{}.tm", d.mesh)).ok().and_then(|b| bytefield_formats::tm::parse(&b).ok()).map(|tm| (tm.bounds, tm.angles)));
            if let Some(([lo, hi], angles)) = tm {
                let factor = tree_factor(&d.kind, &d.mesh, t.translation, bytefield_world::shadow::instance_colour(&world.db.instances[d.instance]));
                let w = &d.world.0;
                let fwd = Vec2::new(w[2][0] as f32, w[2][2] as f32).normalize_or_zero();
                let half = [lo[0], lo[2], hi[0], hi[2]].iter().fold(0.0f32, |a, v| a.max(v.abs()));
                billboards.entry(d.mesh.to_ascii_lowercase()).or_insert_with(|| (angles, Vec::new())).1.push(BillboardQuad {
                    foot: t.translation + Vec3::Y * lo[1] * t.scale.y,
                    size: Vec3::new(half * t.scale.x.abs(), (hi[1] - lo[1]) * t.scale.y.abs(), f.far),
                    axes: Vec4::new(fwd.x, fwd.y, w[0][0] as f32, w[0][2] as f32),
                    up: (t.rotation * Vec3::Y).normalize_or(Vec3::Y),
                    colour: Vec3::from(factor.map(|c| c as f32 / 255.0)) * renderer.light_scale,
                });
            }
        }
        for (m, mat) in cache.get(&world.vfs, &d.kind, &d.mesh, meshes, materials, images) {
            // (`textureFade`: its geometry's `textureFadeMaxDist`; 200 without it, from Husky's houses)
            let texture_fade = cache.faded.contains(&mat.id()).then(|| world.db.get("geometrytemplate", &d.geometry).and_then(|g| g.get("texturefademaxdist")).and_then(|a| a.first()).and_then(|a| a.trim().parse::<f32>().ok()).unwrap_or(200.0));
            let lit = lightmaps.as_mut().filter(|_| d.kind == "standardmesh").and_then(|l| l.material(&world.vfs, &key, &m, &mat, texture_fade, meshes, materials, images, baked_materials));
            baked_parts += lit.is_some() as usize;
            // TODO: a small static with its lightmap (none in view in Husky's reference run): whether the client darkens it
            // too (the material's flag & 4); here not.
            let factor = if lit.is_some() { [255; 3] } else { tree_factor(&d.kind, &d.mesh, t.translation, bytefield_world::shadow::instance_colour(&world.db.instances[d.instance])) };
            objects.entry(d.instance).or_default().push(((d.kind.clone(), m, mat, lit, t), (factor, sprite, fade, sway)));
        }
    }
    // with the sun's shadows: which statics' shadows the tiles carry (into the statics' map too), and which not (into
    // the sun's only: they shade the terrain)
    let mut unbaked = Vec::new();
    for (i, parts) in objects {
        let (parts, factors): (Vec<Part>, Vec<([u8; 3], [f32; 3], Option<(TreeFade, Vec4)>, Option<TreeSway>)>) = parts.into_iter().unzip();
        let baked = tiles.as_ref().zip(bounds(&parts, meshes)).is_none_or(|(tl, b)| tl.carry_shadow(b, atm.sun));
        if !baked {
            unbaked.push(world.db.instances[i].template.as_str());
        }
        for ((kind, m, mat, lit, t), (factor, sprite, fade, sway)) in parts.into_iter().zip(factors) {
            trees.spawn(commands, &kind, Mesh3d(m), mat, lit, t, baked, factor, sprite, fade, sway, materials, tree_materials, meshes);
            drawn += 1;
        }
    }
    if !unbaked.is_empty() {
        info!("{} statics whose shadow the colour tiles lack (they shade the terrain): {}", unbaked.len(), unbaked.join(", "));
    }
    // what the spawners make that never moves (a fixed gun is the simulation's: it turns)
    for s in &lvl.spawners {
        let Some(t) = s.template() else { continue };
        if world.db.object_template(t).is_none() || world.is_mobile(t) || world.is_fixed_gun(t) {
            continue;
        }
        let at = Mat4::from_pos_ypr([0, 1, 2].map(|i| s.pos[i] + s.offset[i]), s.rotation);
        for d in model::draws(&world.db, t, &[]) {
            let tr = assets::to_transform(&at.mul(&d.local));
            for (m, mat) in cache.get(&world.vfs, &d.kind, &d.file, meshes, materials, images) {
                let factor = tree_factor(&d.kind, &d.file, tr.translation, [1.0; 3]);
                trees.spawn(commands, &d.kind, Mesh3d(m), mat, None, tr, true, factor, [1.0; 3], None, None, materials, tree_materials, meshes);
                drawn += 1;
            }
        }
    }
    trees.proxies.spawn(commands, meshes, materials);
    // a species without its texture fades and goes all the same (the client's list takes it only with one)
    let mut quads = 0;
    for (mesh, (angles, list)) in &billboards {
        let Some(tex) = cache.texture(&world.vfs, &format!("treemesh/billboards/{mesh}"), images) else { continue };
        // (the views side by side: the first's left neighbour is the last)
        if let Some(mut i) = images.get_mut(&tex) {
            let mut d = match &i.sampler {
                bevy::image::ImageSampler::Descriptor(d) => d.clone(),
                _ => bevy::image::ImageSamplerDescriptor::linear(),
            };
            d.address_mode_u = bevy::image::ImageAddressMode::Repeat;
            d.address_mode_v = bevy::image::ImageAddressMode::ClampToEdge;
            i.sampler = bevy::image::ImageSampler::Descriptor(d);
        }
        let base = StandardMaterial { base_color_texture: Some(tex), alpha_mode: AlphaMode::Blend, unlit: true, cull_mode: None, double_sided: true, depth_bias: BILLBOARD_FIRST, ..default() };
        let mat = billboard_materials.add(BillboardMaterial { base, extension: BillboardShade { params: Vec4::new(*angles as f32, renderer.fade_scale, if renderer.new_fade { 1.0 } else { 0.0 }, if g.water_gamma { 1.0 } else { 0.0 }) } });
        commands.spawn((Mesh3d(meshes.add(billboard_mesh(list))), MeshMaterial3d(mat), bevy::camera::visibility::NoFrustumCulling));
        quads += list.len();
    }
    info!("trees: {} billboards of {} species with one; `billboardLightScale` {:?}", quads, billboards.len(), renderer.light_scale);
    if let Some(l) = &lightmaps {
        info!("trees: {} materials, {} blended in gamma", trees.made.len(), trees.made.values().filter(|h| tree_materials.get(*h).is_some_and(|m| m.extension.light.blend.x > 0.0)).count());
        info!("lightmaps: {} of {} found, {} parts lit by them", l.images.values().filter(|i| i.is_some()).count(), l.images.len(), baked_parts);
    }
    info!("{} draws ({} parts), {} missing templates, {} missing geometry", exp.draws.len(), drawn, exp.missing_templates, exp.missing_geometry);
    if !exp.missing.is_empty() {
        info!("missing: {}", exp.missing.iter().cloned().collect::<Vec<_>>().join(", "));
    }

    spawn_sky(world, commands, meshes, materials, images, cache);

    // it lights the world and the first person's arms (their own layer, `play`); in the enhanced
    // profile with the shadows of what has `hasDynamicShadow 1`, out to 200 m (medium quality)
    let shadows = assets::graphics().shadows;
    commands.spawn((
        DirectionalLight { illuminance: atm.diffuse.to_vec3().max_element().max(1e-3) * std::f32::consts::PI, color: atm.diffuse.with_luminance_ratio(), shadow_maps_enabled: shadows, ..default() },
        crate::graphics::cascades(assets::graphics().shadow_quality),
        Transform::from_translation(atm.sun * 100.0).looking_at(Vec3::ZERO, Vec3::Y),
        bevy::camera::visibility::RenderLayers::from_layers(&[0, 1, SUN_PROXY_LAYER]),
    ));
    // the statics' own shadow map: a sun without light that only those whose shadow the tiles carry reach
    // (their layer); the terrain, the water and the trees darken by the sun's only where this one leaves them lit
    if shadows {
        commands.spawn((
            DirectionalLight { illuminance: 0.0, shadow_maps_enabled: true, ..default() },
            crate::graphics::cascades(assets::graphics().shadow_quality),
            Transform::from_translation(atm.sun * 100.0).looking_at(Vec3::ZERO, Vec3::Y),
            bevy::camera::visibility::RenderLayers::from_layers(&[STATIC_SHADOW_LAYER, STATIC_PROXY_LAYER]),
        ));
    }
    commands.insert_resource(GlobalAmbientLight { color: atm.ambient.with_luminance_ratio(), brightness: atm.ambient.to_vec3().max_element(), ..default() });
    commands.insert_resource(ClearColor(atm.fog_colour()));
    commands.insert_resource(atm);
}

/// The static meshes as they are spawned: a tree takes a [`TreeMaterial`] (one per `StandardMaterial`).
struct Trees {
    light: TreeLight,
    /// With the sun's shadows the other statics cast them, and into their own map ([`STATIC_SHADOW_LAYER`])
    /// those whose shadow the colour tiles carry (`baked`).
    cast: bool,
    /// by material, TEXTUREFACTOR (rgb bytes), sprites' colour and fade
    made: std::collections::HashMap<(AssetId<StandardMaterial>, [u8; 3], [u32; 3], [u32; 4]), Handle<TreeMaterial>>,
    proxies: ShadowProxies,
}

/// The side of a [`ShadowProxies`] cell, in metres.
const SHADOW_CELL: f32 = 64.0;

/// The opaque statics' silhouettes, merged by cell into meshes only the sun's lights see: each static part
/// casting by itself is a draw in each of the 8 cascades (2 lights × 4), and where they all fit in the cascades, as in
/// Berlin's 2500 parts, that took 200 ms a frame on the CPU with the GPU to spare (7 fps; El Alamein has as many, but
/// spread out). What is cut by its alpha needs its texture, and casts by itself. Keyed by cell, `baked` and two-sidedness.
#[derive(Default)]
struct ShadowProxies(std::collections::HashMap<(i32, i32, bool, bool), (Vec<[f32; 3]>, Vec<u32>)>);

impl ShadowProxies {
    /// Takes the part's silhouette; false if it has to cast by itself.
    fn add(&mut self, mesh: &Mesh, material: &StandardMaterial, t: &Transform, baked: bool) -> bool {
        let (true, bevy::mesh::PrimitiveTopology::TriangleList, Some(bevy::mesh::VertexAttributeValues::Float32x3(pos))) = (matches!(material.alpha_mode, AlphaMode::Opaque), mesh.primitive_topology(), mesh.attribute(Mesh::ATTRIBUTE_POSITION)) else { return false };
        if pos.is_empty() {
            return false;
        }
        let world: Vec<Vec3> = pos.iter().map(|p| t.transform_point(Vec3::from(*p))).collect();
        let centre = world.iter().fold(Vec3::ZERO, |a, p| a + *p) / world.len() as f32;
        let cell = ((centre.x / SHADOW_CELL).floor() as i32, (centre.z / SHADOW_CELL).floor() as i32, baked, material.cull_mode.is_none());
        let (verts, indices) = self.0.entry(cell).or_default();
        let base = verts.len() as u32;
        verts.extend(world.iter().map(|p| p.to_array()));
        // (a transform that mirrors turns the faces round: the cull sees them as the part's own does)
        let flip = t.scale.x * t.scale.y * t.scale.z < 0.0;
        let tri = |a: u32, b: u32, c: u32| if flip { [base + a, base + c, base + b] } else { [base + a, base + b, base + c] };
        match mesh.indices() {
            Some(ix) => {
                let ix: Vec<u32> = ix.iter().map(|i| i as u32).collect();
                indices.extend(ix.chunks_exact(3).flat_map(|c| tri(c[0], c[1], c[2])));
            }
            None => indices.extend((0..pos.len() as u32 / 3).flat_map(|i| tri(3 * i, 3 * i + 1, 3 * i + 2))),
        }
        true
    }

    fn spawn(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
        use bevy::camera::visibility::RenderLayers;
        let [one, two] = [Some(bevy::render::render_resource::Face::Back), None].map(|cull_mode| materials.add(StandardMaterial { unlit: true, cull_mode, ..default() }));
        for ((_, _, baked, two_sided), (verts, indices)) in self.0.drain() {
            let mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, Default::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, verts).with_inserted_indices(bevy::mesh::Indices::U32(indices));
            let layers = if baked { RenderLayers::from_layers(&[SUN_PROXY_LAYER, STATIC_PROXY_LAYER]) } else { RenderLayers::layer(SUN_PROXY_LAYER) };
            commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(if two_sided { two.clone() } else { one.clone() }), Transform::IDENTITY, CastsShadow, layers));
        }
    }
}

impl Trees {
    #[allow(clippy::too_many_arguments)]
    fn spawn(&mut self, commands: &mut Commands, kind: &str, mesh: Mesh3d, mat: Handle<StandardMaterial>, lit: Option<Handle<BakedMaterial>>, t: Transform, baked: bool, factor: [u8; 3], sprite: [f32; 3], fade: Option<(TreeFade, Vec4)>, sway: Option<TreeSway>, materials: &Assets<StandardMaterial>, tree_materials: &mut Assets<TreeMaterial>, meshes: &Assets<Mesh>) {
        let merged = self.cast && kind != "treemesh" && meshes.get(&mesh.0).zip(materials.get(&mat)).is_some_and(|(m, s)| self.proxies.add(m, s, &t, baked));
        let cast = self.cast && !merged;
        // (with its lightmap: as the others, its own material)
        if let Some(lit) = lit {
            let mut e = commands.spawn((mesh, MeshMaterial3d(lit), t));
            if cast {
                e.insert(CastsShadow);
            }
            if cast && baked {
                e.insert(bevy::camera::visibility::RenderLayers::from_layers(&[0, STATIC_SHADOW_LAYER]));
            }
            return;
        }
        let base = (kind == "treemesh").then(|| materials.get(&mat).cloned()).flatten();
        match base {
            Some(base) => {
                // (blended in the screen's space as D3D8 does, over the copy of the world drawn so far, `backdrop`;
                // the branches' thin twigs over the sand came out lighter, Tobruk's bush +22)
                let gamma = assets::graphics().water_gamma && matches!(base.alpha_mode, AlphaMode::Blend);
                let fv = fade.map_or(Vec4::ZERO, |f| f.1);
                let light = TreeLight { factor: Vec3::from(factor.map(|c| c as f32 / 255.0)).extend(1.0), blend: Vec4::X * if gamma { 1.0 } else { 0.0 }, sprite: Vec3::from(sprite).extend(1.0), fade: fv, ..self.light };
                let tm = self.made.entry((mat.id(), factor, sprite.map(f32::to_bits), fv.to_array().map(f32::to_bits))).or_insert_with(|| tree_materials.add(TreeMaterial { base: StandardMaterial { base_color: Color::WHITE, ..base }, extension: TreeShade { light } })).clone();
                let bounds = sprite_bounds(meshes.get(&mesh.0));
                let mut e = commands.spawn((mesh, MeshMaterial3d(tm), t));
                if let Some(b) = bounds {
                    e.insert(b);
                }
                if let Some((f, _)) = fade {
                    e.insert(f);
                }
                if let Some(s) = sway {
                    e.insert(s);
                }
            }
            None => {
                let mut e = commands.spawn((mesh, MeshMaterial3d(mat), t));
                if cast && kind != "treemesh" {
                    e.insert(CastsShadow);
                    if baked {
                        e.insert(bevy::camera::visibility::RenderLayers::from_layers(&[0, STATIC_SHADOW_LAYER]));
                    }
                }
                if factor != [255; 3] {
                    e.insert(shade_tag(factor));
                }
            }
        }
    }
}

/// A mesh's TEXTUREFACTOR in the terrain's shadow for the shader of the fixed
/// light (`envmap.wesl`): how much each channel is below 255, so that a mesh without a tag is as lit as it gets.
pub fn shade_tag(factor: [u8; 3]) -> bevy::mesh::MeshTag {
    bevy::mesh::MeshTag { value: factor.iter().enumerate().map(|(k, &c)| ((255 - c) as u32) << (8 * k)).sum() }
}

/// Each standard mesh's box (min, max) from its `.sm`, read once (`None`: no such file).
#[derive(Default)]
pub struct MeshBoxes(std::collections::HashMap<String, Option<[[f32; 3]; 2]>>);

impl MeshBoxes {
    pub fn get(&mut self, vfs: &bytefield_vfs::Vfs, mesh: &str) -> Option<[[f32; 3]; 2]> {
        *self.0.entry(mesh.to_ascii_lowercase()).or_insert_with(|| vfs.read(&format!("standardmesh/{mesh}.sm")).ok().and_then(|b| bytefield_formats::sm::parse(&b).ok()).map(|m| m.bounds))
    }
}

/// A mesh of sprites reaches past its vertices by their offsets: its box with them (a mesh without them, the one Bevy works out).
fn sprite_bounds(mesh: Option<&Mesh>) -> Option<bevy::camera::primitives::Aabb> {
    let mesh = mesh?;
    let Some(bevy::mesh::VertexAttributeValues::Float32x2(offsets)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1) else { return None };
    let reach = offsets.iter().map(|o| Vec2::from(*o).length()).fold(0.0, f32::max);
    let b = <Mesh as bevy::camera::primitives::MeshAabb>::get_aabb(mesh)?;
    Some(bevy::camera::primitives::Aabb { center: b.center, half_extents: b.half_extents + bevy::math::Vec3A::splat(reach * 1.1) })
}

/// A placed static's piece: its kind, mesh, material and where.
type Part = (String, Handle<Mesh>, Handle<StandardMaterial>, Option<Handle<BakedMaterial>>, Transform);

/// A static's box in the world (Bevy's axes), from its pieces' vertices (not a tree's).
fn bounds(parts: &[Part], meshes: &Assets<Mesh>) -> Option<(Vec3, Vec3)> {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (kind, m, _, _, t) in parts {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = (kind != "treemesh").then(|| meshes.get(m)?.attribute(Mesh::ATTRIBUTE_POSITION)).flatten() else { continue };
        let (a, b) = pos.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(Vec3::from(*p)), b.max(Vec3::from(*p))));
        if a.x > b.x {
            continue;
        }
        for i in 0..8 {
            let p = t.transform_point(Vec3::new([a.x, b.x][i & 1], [a.y, b.y][(i >> 1) & 1], [a.z, b.z][i >> 2]));
            (lo, hi) = (lo.min(p), hi.max(p));
        }
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

/// How alike the ground off a static's shadow side and off its sunny side must be on the colour tiles for its shadow to
/// count as not painted: painted ones come at 0.3–0.85 (El Alamein's, Bocage's buildings), Bocage's bridges at 0.86–1.1;
/// past 1.15 a neighbour's shadow falls on the sunny side (Berlin's streets), and it cannot tell.
const SAME: (f32, f32) = (0.85, 1.15);

/// The colour tiles as the terrain shows them, to tell which statics' shadows they carry: most are
/// painted in (El Alamein's buildings, Bocage's church and farms) but not all (Bocage's bridges).
struct Tiles<'a> {
    img: &'a bytefield_formats::image::Image,
    uv: bevy::math::Affine2,
    world_size: f32,
}

impl Tiles<'_> {
    /// The tiles' brightness round the ground at (x, z) (Bevy's axes): 3 × 3 texels.
    fn light(&self, x: f32, z: f32) -> Option<f32> {
        let uv = self.uv.transform_point2(Vec2::new(x, -z) / self.world_size);
        let (w, h) = (self.img.width as i64, self.img.height as i64);
        let (cx, cy) = ((uv.x * w as f32) as i64, (uv.y * h as f32) as i64);
        if cx < 1 || cy < 1 || cx + 1 >= w || cy + 1 >= h {
            return None;
        }
        let mut sum = 0.0;
        for y in cy - 1..=cy + 1 {
            for x in cx - 1..=cx + 1 {
                let p = self.img.get(x as usize, y as usize);
                sum += (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0;
            }
        }
        Some(sum / 9.0)
    }

    /// Whether the tiles carry a box's shadow: the ground in a band 1–2.5 m off its sides that face away from the
    /// sun not about as light as off those that face it ([`SAME`]). The band, not where the sun puts the roof: the tiles'
    /// shadows are shorter than the level's sun makes them (El Alamein's) and not always the same way (Bocage's fall
    /// south, its sun's to the south-west), so only the side matters. When it cannot tell (under 2 m high, off the
    /// tiles), it takes it as painted: at worst its shadow is missing, as before, never doubled.
    fn carry_shadow(&self, (lo, hi): (Vec3, Vec3), sun: Vec3) -> bool {
        let away = -Vec2::new(sun.x, sun.z).normalize_or_zero();
        if hi.y - lo.y < 2.0 || away == Vec2::ZERO {
            return true;
        }
        let (mut dark, mut lit) = ((0.0, 0), (0.0, 0));
        for (n, a, b) in [
            (Vec2::X, Vec2::new(hi.x, lo.z), Vec2::new(hi.x, hi.z)),
            (Vec2::NEG_X, Vec2::new(lo.x, lo.z), Vec2::new(lo.x, hi.z)),
            (Vec2::Y, Vec2::new(lo.x, hi.z), Vec2::new(hi.x, hi.z)),
            (Vec2::NEG_Y, Vec2::new(lo.x, lo.z), Vec2::new(hi.x, lo.z)),
        ] {
            let side = n.dot(away);
            let sum = if side > 0.5 { &mut dark } else if side < -0.5 { &mut lit } else { continue };
            for f in [0.2, 0.4, 0.6, 0.8] {
                for d in [1.0, 2.5] {
                    let p = a.lerp(b, f) + n * d;
                    if let Some(l) = self.light(p.x, p.y) {
                        *sum = (sum.0 + l, sum.1 + 1);
                    }
                }
            }
        }
        if dark.1 < 4 || lit.1 < 4 {
            return true;
        }
        let ratio = dark.0 / (dark.1 as f32) / (lit.0 / (lit.1 as f32)).max(1.0);
        !(SAME.0..SAME.1).contains(&ratio)
    }
}

/// A mesh that casts the sun's shadow in the enhanced profile: a part of what has
/// `hasDynamicShadow 1` (soldiers, vehicles, hand weapons), not from the player's own eyes.
#[derive(Component)]
pub struct CastsShadow;

/// Whether a template says `hasDynamicShadow 1`.
pub fn has_dynamic_shadow(world: &World, template: &str) -> bool {
    world.db.object_template(template).and_then(|t| t.get("hasdynamicshadow")).and_then(|a| a.first()).is_some_and(|v| v.trim() == "1")
}

/// With the shadows on, every other mesh casts none (Bevy's default is all of them), nor a caster
/// drawn only on the first person's layer.
pub fn shadow_casters(
    mut commands: Commands,
    added: Query<Entity, (Added<Mesh3d>, Without<CastsShadow>)>,
    casters: Query<(Entity, Option<&bevy::camera::visibility::RenderLayers>), (With<CastsShadow>, Or<(Added<CastsShadow>, Changed<bevy::camera::visibility::RenderLayers>)>)>,
) {
    if !assets::graphics().shadows {
        return;
    }
    for e in &added {
        commands.entity(e).insert(bevy::light::NotShadowCaster);
    }
    for (e, layers) in &casters {
        if layers.is_none_or(|l| l.intersects(&bevy::camera::visibility::RenderLayers::from_layers(&[0, SUN_PROXY_LAYER]))) {
            commands.entity(e).remove::<bevy::light::NotShadowCaster>();
        } else {
            commands.entity(e).insert(bevy::light::NotShadowCaster);
        }
    }
}

/// Bevy gives each camera its own cascades of each light, and the casters are drawn again for each: 16 views
/// a frame instead of 8. The mirror's world goes without the sun's shadows, and the arms, within 10 m of the eye, only need the first.
pub fn trim_cascades(mirror: Query<Entity, With<ReflectionCamera>>, arms: Query<Entity, With<crate::play::ArmsCamera>>, mut lights: Query<&mut bevy::light::cascade::Cascades>) {
    for mut c in &mut lights {
        for e in &mirror {
            if let Some(v) = c.cascades.get_mut(&e) {
                v.clear();
            }
        }
        for e in &arms {
            if let Some(v) = c.cascades.get_mut(&e) {
                v.truncate(1);
            }
        }
    }
}

/// The sky (`Init/SkyAndSun.con`): the `SkyBox` mesh (a cube of 4000 m with the sun and the clouds
/// painted in its textures, `lighting false`) around the camera and out of the fog. No cloud layer:
/// `Sky.addCloud` draws only with a `GeometryTemplate` named `Cloud`, and
/// every level has it commented out (GC's `GC_BCanyon` names a mesh `cloud` that no archive has).
#[derive(Component)]
pub struct Sky {
    /// `sky.changeOfsSkyHeight`: the cube this much over the camera (Battleaxe's 200: its clouds ~70 px higher)
    height: f32,
}

fn spawn_sky(world: &World, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, cache: &mut Cache) {
    let g = &world.db.globals;
    let after = |name: &str| g.iter().position(|c| c.command.eq_ignore_ascii_case(name));
    let height = after("sky.changeOfsSkyHeight").and_then(|at| g[at].args.first()?.parse::<f32>().ok()).unwrap_or(0.0);
    let sky = commands.spawn((Sky { height }, Transform::default(), Visibility::default())).id();
    // `Sky.initSky` takes the geometry template named `SkyBox`, turned `Sky.setRotAngle` degrees
    // clockwise seen from above (180 in most levels; the sign from Liberation of Caen's 80 against
    // the original)
    if after("sky.initSky").is_some() {
        let turn = after("sky.setRotAngle").and_then(|at| g[at].args.first()?.parse::<f32>().ok()).unwrap_or(0.0);
        let turn = Transform::from_rotation(Quat::from_rotation_y(-turn.to_radians()));
        let file = world.db.get("geometrytemplate", "SkyBox").and_then(|t| t.get("file")).and_then(|a| a.first().cloned());
        for (m, mat) in file.map(|f| cache.get(&world.vfs, "standardmesh", &f, meshes, materials, images)).unwrap_or_default() {
            // The original draws the sky without light whatever its `.rs` says (GC_Bespin's `sky_Bespin_m1` has
            // `lighting true`: its reference run draws the cube with LIGHTING 0, MODULATE(TEXTURE, white)); out of the lit and
            // envmapped sets too, or `envmap::swap` gives it the fixed light again, the sun's orange over the clouds
            cache.lit.remove(&mat.id());
            cache.envmapped.remove(&mat.id());
            if let Some(mut mt) = materials.get_mut(&mat) {
                mt.fog_enabled = false;
                mt.unlit = true;
                // seen from inside
                mt.cull_mode = None;
            }
            // (past the world's far plane, its own: not culled by it)
            commands.spawn((Mesh3d(m), MeshMaterial3d(mat), turn, ChildOf(sky), bevy::camera::visibility::NoFrustumCulling));
        }
    }
}

/// The sky goes with the world's camera (the one with the fog).
pub fn follow_sky(cams: Query<&Transform, (With<WorldCamera>, Without<Sky>)>, mut sky: Query<(&mut Transform, &Sky)>) {
    let Some(cam) = cams.iter().next() else { return };
    for (mut t, sky) in &mut sky {
        t.translation = cam.translation + Vec3::Y * sky.height;
    }
}

/// A camera over the combat area, looking at its centre.
pub fn overview(world: &World) -> Transform {
    let lvl = &world.level;
    let area = lvl.combat_area.unwrap_or([0.0, 0.0, lvl.terrain.world_size, lvl.terrain.world_size]);
    let centre = Vec3::new((area[0] + area[2] / 2.0) as f32, 0.0, -((area[1] + area[3] / 2.0) as f32));
    let ground = world.height_at(centre.x as f64, -centre.z as f64).unwrap_or(0.0) as f32;
    let look = centre + Vec3::Y * ground;
    let at = look + Vec3::new(0.0, (area[2] as f32) * 0.35, (area[3] as f32) * 0.45);
    Transform::from_translation(at).looking_at(look, Vec3::Y)
}

/// The original's free camera (the one before spawning) at `at`, looking at `look`: the original lifts it to
/// 1 m above the terrain under it when lower, without turning it.
pub fn free_camera(world: &World, at: Vec3, look: Vec3) -> Transform {
    let ground = world.height_at(at.x as f64, -at.z as f64).map_or(f32::MIN, |h| h as f32);
    let lift = Vec3::Y * (ground + 1.0 - at.y).max(0.0);
    Transform::from_translation(at + lift).looking_at(look + lift, Vec3::Y)
}

/// What the original shows a player of `team` before spawning (its player object takes the setup's before-spawn camera position when the server creates the player
///; there is no rotation command, it looks north, +Z): `game.setBeforeSpawnCameraPosition`
/// of the level's `Init.con`, the last line of the team. A level without one, the [`overview`].
pub fn before_spawn_camera(world: &World, team: u32) -> Transform {
    match before_spawn_position(&world.db.globals, team) {
        Some(at) => free_camera(world, at, at - Vec3::Z * 100.0),
        None => overview(world),
    }
}

fn before_spawn_position(globals: &[bytefield_con::templates::Call], team: u32) -> Option<Vec3> {
    let team = team.to_string();
    globals.iter().rev().find_map(|c| {
        if !c.command.eq_ignore_ascii_case("game.setBeforeSpawnCameraPosition") || *c.args.first()? != team {
            return None;
        }
        let mut v = c.args.get(1)?.split('/').map(|n| n.parse::<f32>().ok());
        Some(Vec3::new(v.next()??, v.next()??, -v.next()??))
    })
}

/// The level's light and fog (its `Init.con`: `renderer.*`, `Game.setViewDistance`,
/// `sky.sunLightDirectionVec`), in the original's fixed-function terms: a lit face is
/// texture × (ambient + global ambient + diffuse · N·L). The cameras take `exposure` and no tone
/// mapping, so Bevy's light units come out as those factors.
#[derive(Resource, Clone)]
pub struct Atmosphere {
    pub fog: Vec3,
    /// The trees' light (sRGB): their vertices carry 0.5 and D3D's default `DIFFUSEMATERIALSOURCE`
    /// COLOR1 makes it their diffuse material, so 2 min(A + 0.5 D N·L, 1) (MODULATE2X); with N·L
    /// taken as ½ (two-sided leaves).
    pub tree: Vec3,
    /// D3D's FOGSTART and FOGEND (see `fog_range`)
    pub fog_range: (f32, f32),
    /// The world's far plane: the last `Game.setViewDistance` (800 without one). The original's projection for the world
    /// ends there (Cassino's 250: `_33` = 1.0004 with near 0.1, in a reference run) and the sky has its own (1000, 40000), so past
    /// it the sky shows, not fogged ground: DC_Weapon_Bunkers, 93 m up, its white fog against a grey sky.
    pub view: f32,
    pub ambient: LinearColor,
    pub diffuse: LinearColor,
    /// towards the sun, in Bevy's axes
    pub sun: Vec3,
    /// What the sun's shadow leaves of an unlit surface's light, in the screen's (gamma) space: the
    /// face away from the sun over the one facing it, A / min(A + D, 1) (the enhanced profile's
    /// shadows on the terrain)
    pub shade: Vec3,
    /// The same for the trees: their light in a shadow (2A) over theirs ([`crate::level::Atmosphere::tree`])
    pub tree_shade: Vec3,
    /// The original's shadows of soldiers and vehicles (`shadows.rs`): the direction they are cast from (Bevy's axes)
    /// and what each takes of the terrain's colour. The sun's vector with its height doubled, and `shadow.shadowColor`
    /// × 0.915: both measured in El Alamein (its shadows' length; the texture factor, 0x89–0x8f, over its 0.6).
    pub object_shadow: (Vec3, f32),
    /// The level's ambient (with the global one) and sun as D3D's fixed function takes them, in the screen's (gamma)
    /// space: a lit object is texture × 2 min(A + D N·L, 1)
    pub fixed: (Vec3, Vec3),
}

/// A colour factor in linear terms, kept as its components (light colour and strength apart).
#[derive(Clone, Copy)]
pub struct LinearColor(pub Vec3);

impl LinearColor {
    pub fn to_vec3(self) -> Vec3 {
        self.0
    }
    /// The colour with its largest component 1 (the strength goes in the light's brightness).
    pub fn with_luminance_ratio(self) -> Color {
        let m = self.0.max_element().max(1e-3);
        Color::linear_rgb(self.0.x / m, self.0.y / m, self.0.z / m)
    }
}

/// A TreeMesh GeometryTemplate's `lightColor` (`0.5`, or `0.5/0.45/0.5`), the colour of its sprites; 1 without it.
fn light_color(db: &bytefield_con::templates::TemplateDb, geometry: &str) -> [f32; 3] {
    let v: Vec<f32> = db.get("geometrytemplate", geometry).and_then(|t| t.get("lightcolor")).and_then(|a| a.first()).map_or(Vec::new(), |a| a.split('/').filter_map(|x| x.trim().parse().ok()).collect());
    match v[..] {
        [g] => [g; 3],
        [r, g, b] => [r, g, b],
        _ => [1.0; 3],
    }
}

/// A TreeMesh GeometryTemplate's `billboardDistance` when it has `billboard 1` (10000 without the distance, the
/// constructor's); `None` without a billboard.
fn billboard_distance(db: &bytefield_con::templates::TemplateDb, geometry: &str) -> Option<f32> {
    let t = db.get("geometrytemplate", geometry)?;
    let num = |k: &str| t.get(k).and_then(|a| a.first()).and_then(|a| a.trim().parse::<f32>().ok());
    (num("billboard")? != 0.0).then(|| num("billboarddistance").unwrap_or(10000.0))
}

/// `shadow.shadowColor` as one grey (`0.6`, or `0.36/0.36/0.38`: their mean); 0.5 without it, the most levels' value.
fn shadow_color(arg: Option<&str>) -> f32 {
    let v: Vec<f32> = arg.unwrap_or("").split('/').filter_map(|x| x.trim().parse().ok()).collect();
    if v.is_empty() { 0.5 } else { (v.iter().sum::<f32>() / v.len() as f32).clamp(0.0, 1.0) }
}

impl Atmosphere {
    /// What the original falls back to when the level says nothing (a guess: no level we read lacks them).
    pub fn of(world: &World) -> Atmosphere {
        let last = |name: &str| world.db.globals.iter().rev().find(|c| c.command.eq_ignore_ascii_case(name)).and_then(|c| c.args.first()).cloned();
        let vec = |name: &str, d: [f32; 3]| {
            last(name).and_then(|a| {
                let v: Vec<f32> = a.split('/').filter_map(|x| x.trim().parse().ok()).collect();
                (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
            })
            .unwrap_or(Vec3::from(d))
        };
        let srgb = |v: Vec3| {
            let c = Color::srgb(v.x, v.y, v.z).to_linear();
            Vec3::new(c.red, c.green, c.blue)
        };
        let sun = vec("sky.sunLightDirectionVec", [-0.5, 0.7, 0.5]);
        // D3D8 adds the factors in the screen's (gamma) space, clamps them to 1 and the lit objects
        // take them ×2 (MODULATE2X, Wake's rendering): in linear terms the face in shadow gets lin(2A)
        // and the one facing the sun lin(2 min(A + D, 1)); the sun takes the difference
        let a = vec("renderer.ambientColor", [0.2; 3]) + vec("renderer.globalAmbientColor", [0.0; 3]);
        let d = vec("renderer.diffuseColor", [0.6; 3]);
        Atmosphere {
            fog: vec("renderer.fogColorVec", [0.6, 0.62, 0.65]),
            fog_range: fog_range(world),
            view: world.db.globals.iter().rev().find(|c| c.command.eq_ignore_ascii_case("game.setViewDistance")).and_then(|c| c.args.first()?.parse().ok()).unwrap_or(800.0),
            ambient: LinearColor(srgb(2.0 * a)),
            diffuse: LinearColor(srgb(2.0 * (a + d).min(Vec3::ONE)) - srgb(2.0 * a)),
            sun: Vec3::new(sun.x, sun.y, -sun.z).normalize_or(Vec3::Y),
            tree: 2.0 * (a + 0.25 * d).min(Vec3::ONE),
            shade: (a / (a + d).min(Vec3::ONE).max(Vec3::splat(1e-3))).min(Vec3::ONE),
            tree_shade: (a / (a + 0.25 * d).min(Vec3::ONE).max(Vec3::splat(1e-3))).min(Vec3::ONE),
            object_shadow: (Vec3::new(sun.x, 2.0 * sun.y, -sun.z).normalize_or(Vec3::Y), shadow_color(last("shadow.shadowColor").as_deref()) * 0.915),
            fixed: (a, d),
        }
    }

    pub fn tree_light(&self) -> Color {
        Color::srgb(self.tree.x, self.tree.y, self.tree.z)
    }

    pub fn fog_colour(&self) -> Color {
        Color::srgb(self.fog.x, self.fog.y, self.fog.z)
    }

    /// The world camera's far plane: [`Atmosphere::view`] times the settings' view distance (1 as the original).
    pub fn far(&self) -> f32 {
        self.view * assets::graphics().view_distance
    }

    /// The fog: the level's colour, linear with the distance (D3D's range fog), over `fog_range`
    /// (times the settings' view distance, 1 as the original).
    pub fn distance_fog(&self) -> DistanceFog {
        let k = assets::graphics().view_distance;
        let (start, end) = (self.fog_range.0 * k, self.fog_range.1 * k);
        DistanceFog { color: self.fog_colour(), falloff: FogFalloff::Linear { start, end }, ..default() }
    }

    /// What a world camera carries besides its projection: the fog, no tone mapping and an
    /// exposure of 1 (Bevy's is 1/(1.2 · 2^ev100)).
    /// In the enhanced profile, the HD Remaster's `renderer.mipmapBias -1` (`Settings/VideoDefault.con`):
    /// sharper textures; 0, the original's, otherwise. 4× MSAA unless the profile's
    /// anti-aliasing is another (every camera on the window the same, or they would not share it).
    pub fn camera(&self) -> impl Bundle {
        let g = crate::assets::graphics();
        let msaa = if g.msaa() { bevy::render::view::Msaa::Sample4 } else { bevy::render::view::Msaa::Off };
        (self.distance_fog(), bevy::core_pipeline::tonemapping::Tonemapping::None, bevy::camera::Exposure { ev100: -(1.2f32).log2() }, bevy::render::camera::MipBias(g.mip_bias), msaa)
    }
}

/// FOGSTART and FOGEND: the last of the level's commands wins. `Game.setViewDistance v` sets
/// v/4 − 50 and v − 50, `renderer.fogStart`/`fogEnd` each their own (measured in the original:
/// 500 → 75/450 in El Alamein and Wake, Kursk 400 → 50/350, Omaha 300 → 25/250 with
/// its earlier −50/290 overridden, Berlin's 0/100 after its 100). Without the −50 when the start would
/// not be above 0: Stalingrad 90 → 22.5/90, Bulge 200 → 50/200 (Caen 225 → 6.25/175). Tobruk's
/// `renderer.setViewdistance 700` does nothing (its 300 → 25/250). A `fogEnd` at or past the view
/// distance gives the view distance's FOGEND instead: Baytown and Anzio (300, then 80/300) draw with
/// 80/250, Kharkov (400, then −40/400) with −40/350, but Invasion_of_the_Philippines (500, then 25/475)
/// with 25/475; the same with `fogStart`: GC_Alaris_Prime (700, then 700/1000) with 125/650 (reference runs b263, a261c, k263,
/// p263, al263). Why (a clamp against the far plane?): not
/// known.
fn fog_range(world: &World) -> (f32, f32) {
    let (mut start, mut end) = fog_from_view(800.0);
    let (mut view, mut view_start, mut view_end) = (800.0, start, end);
    for c in &world.db.globals {
        let Some(v) = c.args.first().and_then(|a| a.parse::<f32>().ok()) else { continue };
        match c.command.to_ascii_lowercase().as_str() {
            "game.setviewdistance" => {
                (start, end) = fog_from_view(v);
                (view, view_start, view_end) = (v, start, end);
            }
            "renderer.fogstart" => start = v,
            "renderer.fogend" => end = v,
            _ => {}
        }
    }
    (if start >= view { view_start } else { start }, if end >= view { view_end } else { end })
}

fn fog_from_view(v: f32) -> (f32, f32) {
    if v / 4.0 > 50.0 { (v / 4.0 - 50.0, v - 50.0) } else { (v / 4.0, v) }
}

/// What `water.tile*` multiplies to give repeats per metre: not in the original's rendering (the texture
/// matrix is a pointer), tuned against the original's screenshots (`BYTEFIELD_WATER_TILE` to try).
fn water_tile() -> f32 {
    std::env::var("BYTEFIELD_WATER_TILE").ok().and_then(|v| v.parse().ok()).unwrap_or(WATER_TILE)
}
const WATER_TILE: f32 = 0.0625;

/// D3D mixes its vertex fog in the screen's (gamma) space, Bevy's linear fog in linear space: far
/// objects came out washed (Wake's houses at 250 m). Bevy's fog module, rewritten once it
/// has loaded so that every `StandardMaterial` mixes like the original (the terrain and the water
/// do their own fog). Returns when done.
pub fn gamma_fog(asset_server: Res<AssetServer>, mut shaders: ResMut<Assets<bevy::shader::Shader>>, mut done: Local<bool>) {
    const PATH: &str = "embedded://bevy_pbr/render/fog.wesl";
    const LINEAR: &str = "return vec4<f32>(mix(input_color.rgb, fog_color.rgb, fog_color.a), input_color.a);\n}\n\nfn exponential_fog";
    // (the exact sRGB curve, as Bevy decodes the textures and encodes the screen, not x^2.2)
    const GAMMA: &str = "let a = clamp(input_color.rgb, vec3(0.0), vec3(1.0));\n    let b = clamp(fog_color.rgb, vec3(0.0), vec3(1.0));\n    let sa = select(1.055 * pow(a, vec3(1.0 / 2.4)) - 0.055, 12.92 * a, a <= vec3(0.0031308));\n    let sb = select(1.055 * pow(b, vec3(1.0 / 2.4)) - 0.055, 12.92 * b, b <= vec3(0.0031308));\n    let m = mix(sa, sb, fog_color.a);\n    return vec4<f32>(select(pow((m + 0.055) / 1.055, vec3(2.4)), m / 12.92, m <= vec3(0.04045)), input_color.a);\n}\n\nfn exponential_fog";
    if *done {
        return;
    }
    let handle: Handle<bevy::shader::Shader> = asset_server.load(PATH);
    let Some(source) = shaders.get(&handle).and_then(|s| match &s.source {
        bevy::shader::Source::Wesl(src) => Some(src.to_string()),
        _ => None,
    }) else { return };
    *done = true;
    if !source.contains(LINEAR) {
        warn!("Bevy's fog module changed: the fog stays linear");
        return;
    }
    let _ = shaders.insert(handle.id(), bevy::shader::Shader::from_wesl(source.replace(LINEAR, GAMMA), PATH));
}

#[cfg(test)]
mod tests {
    /// The sway's form: Midway's palm of the reference run with x scale 0.946 and y 0.893 leans 1.166° at
    /// most along its species' direction (1.1 × 0.946/0.893), not at all past 40 m, farther when zoomed, and stops
    /// after the ±1000 rad clamp in the Original profile.
    #[test]
    fn a_tree_sways_in_one_plane_and_fades_with_the_distance() {
        use super::TreeSway;
        use bevy::prelude::*;
        let base = Transform::from_xyz(10.0, 2.0, 5.0).with_scale(Vec3::new(0.946, 0.893, 0.946));
        let s = TreeSway { base, amp: Vec2::new(0.0, 0.946 / 0.893) * 1.1, rate: 0.2 * std::f32::consts::TAU, phase: 0.0 };
        let lean = |tr: Transform| (tr.rotation * base.rotation.inverse() * Vec3::Y).angle_between(Vec3::Y).to_degrees();
        let most = (0..500).map(|i| lean(s.at(i as f64 * 0.01, 0.0, 1.0, true))).fold(0.0f32, f32::max);
        assert!((most - 1.166).abs() < 0.005, "{most}");
        assert_eq!(s.at(1.25, 41.0 * 41.0, 1.0, true), base);
        assert!(lean(s.at(1.25, 41.0 * 41.0, 2.0, true)) > 1.0);
        // (past 28.6 m it fades: (L² − d²)/F² < 1)
        assert!((lean(s.at(1.25, 35.0 * 35.0, 1.0, true)) / lean(s.at(1.25, 0.0, 1.0, true)) - (1600.0 - 1225.0) / 784.0).abs() < 0.01);
        let late = |t: f64, freeze| s.at(t, 0.0, 1.0, freeze);
        assert_eq!(late(900.0, true), late(901.3, true));
        assert_ne!(late(900.0, false), late(901.3, false));
    }

    #[test]
    fn a_sprite_s_box_reaches_past_its_vertices() {
        use bevy::prelude::*;
        let mesh = |offset: [f32; 2]| {
            Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3], [1.0, 0.0, 0.0]])
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![offset, offset])
        };
        assert!(super::sprite_bounds(Some(&mesh([3.0, 4.0]))).unwrap().half_extents.x > 5.0);
        assert!(super::sprite_bounds(Some(&mesh([0.0, 0.0]))).unwrap().half_extents.x < 0.6);
    }

    #[test]
    fn the_camera_before_spawning_is_the_last_line_of_the_team() {
        use bytefield_con::templates::Call;
        let line = |team: &str, at: &str| Call { command: "Game.setBeforeSpawnCameraPosition".into(), args: vec![team.into(), at.into()] };
        let globals = [line("1", "1/2/3"), line("2", "4/5/6"), line("1", "7/8/9")];
        assert_eq!(super::before_spawn_position(&globals, 1), Some(bevy::math::Vec3::new(7.0, 8.0, -9.0)));
        assert_eq!(super::before_spawn_position(&globals, 2), Some(bevy::math::Vec3::new(4.0, 5.0, -6.0)));
        assert_eq!(super::before_spawn_position(&globals, 0), None);
    }

    #[test]
    fn the_colour_tiles_sit_at_their_offset() {
        use bevy::math::Vec2;
        // Wake: 4 tiles, offset 2, 2048 m: the tiles cover 512..1536
        let t = super::tile_uv(2048.0, [4, 4], [2.0, 2.0]);
        assert!((t.transform_point2(Vec2::splat(512.0 / 2048.0)) - Vec2::ZERO).length() < 1e-6);
        assert!((t.transform_point2(Vec2::splat(1536.0 / 2048.0)) - Vec2::ONE).length() < 1e-6);
        // no offset: the whole world
        assert_eq!(super::tile_uv(2048.0, [8, 8], [0.0, 0.0]), bevy::math::Affine2::IDENTITY);
        // Tobruk: 8 × 6 tiles at 4/−10 in 4096: x 1024..3072, z 0..1536
        let t = super::tile_uv(4096.0, [8, 6], [4.0, -10.0]);
        assert!((t.transform_point2(Vec2::new(1024.0, 0.0) / 4096.0) - Vec2::ZERO).length() < 1e-6);
        assert!((t.transform_point2(Vec2::new(3072.0, 1536.0) / 4096.0) - Vec2::ONE).length() < 1e-6);
    }

    #[test]
    fn the_fog_range_is_the_originals() {
        let game = bytefield_testdata::require!(Base, Xpack1, Gc);
        // FOGSTART/FOGEND as the original sets them
        for (level, range) in [("El_Alamein", (75.0, 450.0)), ("Wake", (75.0, 450.0)), ("Kursk", (50.0, 350.0)), ("Omaha_Beach", (25.0, 250.0)), ("Berlin", (0.0, 100.0)), ("Stalingrad", (22.5, 90.0)), ("Battle_of_the_Bulge", (50.0, 200.0)), ("Liberation_of_Caen", (6.25, 175.0)), ("Tobruk", (25.0, 250.0)), ("Kharkov", (-40.0, 350.0)), ("Invasion_of_the_Philippines", (25.0, 475.0))] {
            let w = bytefield_sim::World::load(&game, "bf1942", level, "conquest").unwrap();
            assert_eq!(super::fog_range(&w), range, "{level}");
        }
        // FOGEND capped by the view distance's
        for (level, range) in [("baytown", (80.0, 250.0)), ("Anzio", (80.0, 250.0))] {
            let w = bytefield_sim::World::load(&game, "XPack1", level, "conquest").unwrap();
            assert_eq!(super::fog_range(&w), range, "{level}");
        }
        if let Ok(w) = bytefield_sim::World::load(&game, "GCMOD", "GC_Alaris_Prime", "conquest") {
            assert_eq!(super::fog_range(&w), (125.0, 650.0));
        }
    }
}
