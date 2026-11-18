//! The graphics settings one by one: the rows of the menu's video and effects
//! pages, and what puts them on the cameras and the sun while a round goes on. What needs the
//! textures, the materials or the water made again (their size, filtering, the sun's shadows on or off,
//! reflections, smooth foliage) waits for the next round.

use bevy::anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, fxaa::Fxaa, smaa::Smaa, taa::TemporalAntiAliasing};
use bevy::render::camera::TemporalJitter;
use bevy::core_pipeline::oit::OrderIndependentTransparencySettings;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass, NormalPrepass};
use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceTransmission};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap, ShadowFilteringMethod};
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::post_process::effect_stack::Vignette;
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::post_process::motion_blur::MotionBlur;
use bevy::prelude::*;
use bevy::camera::Hdr;
use bevy::camera::visibility::RenderLayers;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, Msaa};
use bytefield_content::graphics::{Aa, Look, Overrides, ShadowQuality};
use bytefield_content::{Graphics, Profile};

use crate::level::{Atmosphere, ReflectionCamera, WorldCamera};
use crate::settings::Settings;

/// A row of the menu's graphics pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Profile,
    Aa,
    Sharpen,
    Aniso,
    Textures,
    MipBias,
    Shadows,
    Reflections,
    Coverage,
    Bloom,
    Look,
    Vignette,
    MotionBlur,
    Ssao,
    ContactShadows,
    SoftParticles,
    Dof,
    RenderScale,
    WaterFx,
    DynLights,
    Oit,
    Lightmaps,
    ObjectShadows,
    EnvMap,
    FixedLight,
    WaterGamma,
    MeshShade,
    ViewDistance,
    Fov,
    Vsync,
    Fullscreen,
}

/// The video page's rows, and the experimental page's: what no profile turns on
/// and seen in a few screenshots, not played.
pub const VIDEO: &[Row] = &[Row::Profile, Row::Aa, Row::Sharpen, Row::Aniso, Row::Textures, Row::MipBias, Row::Shadows, Row::ObjectShadows, Row::EnvMap, Row::FixedLight, Row::WaterGamma, Row::MeshShade, Row::Lightmaps, Row::Reflections, Row::Coverage, Row::RenderScale];
/// On the general page, under the volume and the language
pub const GENERAL: &[Row] = &[Row::ViewDistance, Row::Fov, Row::Vsync, Row::Fullscreen];
pub const EFFECTS: &[Row] = &[Row::Bloom, Row::Look, Row::Vignette, Row::MotionBlur, Row::Ssao, Row::ContactShadows, Row::SoftParticles, Row::DynLights, Row::Oit, Row::Dof, Row::WaterFx];

const SHARPEN: &[f32] = &[0.0, 0.2, 0.4, 0.6];
const ANISO: &[f32] = &[1.0, 2.0, 4.0, 8.0, 16.0];
const TEXTURES: &[f32] = &[512.0, 1024.0, 2048.0, 4096.0];
const MIP_BIAS: &[f32] = &[0.0, -0.5, -1.0];
const BLOOM: &[f32] = &[0.0, 0.15, 0.3];
const VIGNETTE: &[f32] = &[0.0, 0.25, 0.5];
/// the shutter's angle
const MOTION_BLUR: &[f32] = &[0.0, 0.25, 0.5, 1.0];
const RENDER_SCALE: &[f32] = &[1.0, 0.85, 0.75, 0.5];
const VIEW: &[f32] = &[1.0, 1.5, 2.0, 3.0];
/// the world's vertical field of view, degrees
const FOV: &[f32] = &[50.0, crate::play::WORLD_FOV_DEG, 65.0, 75.0, 85.0];

impl Row {
    /// Its name in `--steps` (`<name>+`, `<name>-`): the key of `content.cfg` where it has one.
    pub fn key(self) -> &'static str {
        match self {
            Row::Profile => "graphics",
            Row::Aa => "aa",
            Row::Sharpen => "sharpen",
            Row::Aniso => "aniso",
            Row::Textures => "max_texture",
            Row::MipBias => "mip_bias",
            Row::Shadows => "shadows",
            Row::Reflections => "reflections",
            Row::Coverage => "coverage",
            Row::Bloom => "bloom",
            Row::Look => "look",
            Row::Vignette => "vignette",
            Row::MotionBlur => "motion_blur",
            Row::Ssao => "ssao",
            Row::ContactShadows => "contact_shadows",
            Row::SoftParticles => "soft_particles",
            Row::Dof => "dof",
            Row::RenderScale => "render_scale",
            Row::WaterFx => "water_fx",
            Row::DynLights => "dyn_lights",
            Row::Oit => "oit",
            Row::Lightmaps => "lightmaps",
            Row::ObjectShadows => "object_shadows",
            Row::EnvMap => "envmap",
            Row::FixedLight => "fixed_light",
            Row::WaterGamma => "water_gamma",
            Row::MeshShade => "mesh_shade",
            Row::ViewDistance => "view_distance",
            Row::Fov => "fov",
            Row::Vsync => "vsync",
            Row::Fullscreen => "fullscreen",
        }
    }

    pub fn all() -> impl Iterator<Item = Row> {
        VIDEO.iter().chain(EFFECTS).chain(GENERAL).copied()
    }

    /// The original's lexicon key, or ours (`lang.rs`).
    pub fn label(self) -> &'static str {
        match self {
            Row::Profile => "VIDEO_OPTIONS_GRAPHICS_QUALITY",
            Row::Aa => "VIDEO_OPTIONS_ANTIALIASING",
            Row::Sharpen => "GFX_SHARPEN",
            Row::Aniso => "GFX_ANISO",
            Row::Textures => "VIDEO_OPTIONS_TEXTURE_QUALITY",
            Row::MipBias => "GFX_MIP_BIAS",
            Row::Shadows => "VIDEO_OPTIONS_SHADOWS",
            Row::Reflections => "GFX_REFLECTIONS",
            Row::Coverage => "GFX_COVERAGE",
            Row::Bloom => "GFX_BLOOM",
            Row::Look => "GFX_LOOK",
            Row::Vignette => "GFX_VIGNETTE",
            Row::MotionBlur => "GFX_MOTION_BLUR",
            Row::Ssao => "GFX_SSAO",
            Row::ContactShadows => "GFX_CONTACT_SHADOWS",
            Row::SoftParticles => "GFX_SOFT_PARTICLES",
            Row::Dof => "GFX_DOF",
            Row::RenderScale => "GFX_RENDER_SCALE",
            Row::WaterFx => "GFX_WATER_FX",
            Row::DynLights => "GFX_DYN_LIGHTS",
            Row::Oit => "GFX_OIT",
            Row::Lightmaps => "GFX_LIGHTMAPS",
            Row::ObjectShadows => "GFX_OBJECT_SHADOWS",
            Row::EnvMap => "GFX_ENVMAP",
            Row::FixedLight => "GFX_FIXED_LIGHT",
            Row::WaterGamma => "GFX_WATER_GAMMA",
            Row::MeshShade => "GFX_MESH_SHADE",
            Row::ViewDistance => "VIDEO_OPTIONS_VIEWDISTANCE",
            Row::Fov => "GFX_FOV",
            Row::Vsync => "GFX_VSYNC",
            Row::Fullscreen => "GFX_FULLSCREEN",
        }
    }

    /// What it shows now; `t` gives a key's text (lexicon, else ours).
    pub fn value(self, s: &Settings, t: &dyn Fn(&str) -> String) -> String {
        let g = Graphics::of(s.content.graphics, &s.content.overrides);
        let yes_no = |b: bool| t(if b { "CONFIRMATION_YES" } else { "CONFIRMATION_NO" });
        let percent = |x: f32| if x <= 0.0 { t("CONFIRMATION_NO") } else { format!("{}%", (x * 100.0).round()) };
        let quality = |q: ShadowQuality| {
            t(match q {
                ShadowQuality::Low => "VIDEO_OPTIONS_LOW",
                ShadowQuality::Medium => "VIDEO_OPTIONS_MEDIUM",
                ShadowQuality::High => "VIDEO_OPTIONS_HIGH",
            })
        };
        match self {
            Row::Profile if s.content.overrides != Overrides::default() => t("VIDEO_OPTIONS_CUSTOM"),
            Row::Profile => t(if s.content.graphics == Profile::Enhanced { "GRAPHICS_ENHANCED" } else { "GRAPHICS_ORIGINAL" }),
            Row::Aa => match g.aa {
                Aa::Off => t("CONFIRMATION_NO"),
                // (they want it off: `apply`)
                Aa::Msaa if !g.msaa() => format!("{} (MSAA)", t("CONFIRMATION_NO")),
                Aa::Msaa => "MSAA 4x".into(),
                a => a.name().to_uppercase(),
            },
            Row::Sharpen => percent(g.sharpen),
            Row::Aniso => format!("{}x", g.aniso),
            Row::Textures => g.max_texture.to_string(),
            // (sharper the more: the bias negated)
            Row::MipBias => format!("{}", -g.mip_bias + 0.0),
            Row::Shadows if !g.shadows => t("CONFIRMATION_NO"),
            Row::Shadows => quality(g.shadow_quality),
            Row::Reflections => yes_no(g.reflections),
            Row::Coverage => yes_no(g.coverage),
            Row::Bloom => percent(g.bloom),
            Row::Look => t(match g.look {
                Look::Neutral => "GFX_NEUTRAL",
                Look::Vivid => "GFX_VIVID",
                Look::Warm => "GFX_WARM",
            }),
            Row::Vignette => percent(g.vignette),
            Row::MotionBlur => percent(g.motion_blur),
            Row::Ssao => yes_no(g.ssao),
            Row::ContactShadows => yes_no(g.contact_shadows),
            Row::SoftParticles => yes_no(g.soft_particles),
            Row::Dof => yes_no(g.dof),
            Row::RenderScale => percent(g.render_scale),
            Row::WaterFx => yes_no(g.water_fx),
            Row::DynLights => yes_no(g.dyn_lights),
            Row::Oit => yes_no(g.oit),
            Row::Lightmaps => yes_no(g.lightmaps),
            Row::ObjectShadows => yes_no(g.object_shadows),
            Row::EnvMap => yes_no(g.envmap),
            Row::FixedLight => yes_no(g.fixed_light),
            Row::WaterGamma => yes_no(g.water_gamma),
            Row::MeshShade => yes_no(g.mesh_shade),
            Row::ViewDistance => format!("x{}", g.view_distance),
            Row::Fov => format!("{}°", s.fov.round()),
            Row::Vsync => yes_no(s.vsync),
            Row::Fullscreen => yes_no(s.fullscreen),
        }
    }

    /// One step along its values (`d` ±1, round the end); the profile clears what was set by hand. The
    /// settings keep only what differs from the profile.
    pub fn step(self, s: &mut Settings, d: i32) {
        let c = &mut s.content;
        let g = Graphics::of(c.graphics, &c.overrides);
        let o = &mut c.overrides;
        match self {
            Row::Profile => {
                c.graphics = if c.graphics == Profile::Original { Profile::Enhanced } else { Profile::Original };
                c.overrides = Overrides::default();
                return;
            }
            Row::Aa => o.aa = Some(cycle_of(Aa::ALL, g.aa, d)),
            Row::Sharpen => o.sharpen = Some(cycle(SHARPEN, g.sharpen, d)),
            Row::Aniso => o.aniso = Some(cycle(ANISO, g.aniso as f32, d) as u16),
            Row::Textures => o.max_texture = Some(cycle(TEXTURES, g.max_texture as f32, d) as usize),
            Row::MipBias => o.mip_bias = Some(cycle(MIP_BIAS, g.mip_bias, d)),
            Row::Shadows => {
                // off, then the qualities
                let now = if g.shadows { 1 + ShadowQuality::ALL.iter().position(|q| *q == g.shadow_quality).unwrap_or(1) } else { 0 };
                let next = (now as i32 + d).rem_euclid(1 + ShadowQuality::ALL.len() as i32) as usize;
                o.shadows = Some(next > 0);
                if next > 0 {
                    o.shadow_quality = Some(ShadowQuality::ALL[next - 1]);
                }
            }
            Row::Reflections => o.reflections = Some(!g.reflections),
            Row::Coverage => o.coverage = Some(!g.coverage),
            Row::Bloom => o.bloom = Some(cycle(BLOOM, g.bloom, d)),
            Row::Look => o.look = Some(cycle_of(Look::ALL, g.look, d)),
            Row::Vignette => o.vignette = Some(cycle(VIGNETTE, g.vignette, d)),
            Row::MotionBlur => o.motion_blur = Some(cycle(MOTION_BLUR, g.motion_blur, d)),
            Row::Ssao => o.ssao = Some(!g.ssao),
            Row::ContactShadows => o.contact_shadows = Some(!g.contact_shadows),
            Row::SoftParticles => o.soft_particles = Some(!g.soft_particles),
            Row::Dof => o.dof = Some(!g.dof),
            Row::RenderScale => o.render_scale = Some(cycle(RENDER_SCALE, g.render_scale, d)),
            Row::WaterFx => o.water_fx = Some(!g.water_fx),
            Row::DynLights => o.dyn_lights = Some(!g.dyn_lights),
            Row::Oit => o.oit = Some(!g.oit),
            Row::Lightmaps => o.lightmaps = Some(!g.lightmaps),
            Row::ObjectShadows => o.object_shadows = Some(!g.object_shadows),
            Row::EnvMap => o.envmap = Some(!g.envmap),
            Row::FixedLight => o.fixed_light = Some(!g.fixed_light),
            Row::WaterGamma => o.water_gamma = Some(!g.water_gamma),
            Row::MeshShade => o.mesh_shade = Some(!g.mesh_shade),
            Row::ViewDistance => o.view_distance = Some(cycle(VIEW, g.view_distance, d)),
            Row::Fov => s.fov = cycle(FOV, s.fov, d),
            Row::Vsync => s.vsync = !s.vsync,
            Row::Fullscreen => s.fullscreen = !s.fullscreen,
        }
        s.content.overrides = s.content.overrides.without_defaults(s.content.graphics);
    }

    /// Whether it waits for the next round.
    pub fn next_round(self) -> bool {
        matches!(self, Row::Aniso | Row::Textures | Row::Shadows | Row::Lightmaps | Row::Reflections | Row::Coverage)
    }
}

/// The value `d` steps from the one nearest `now` in `list`, round the ends.
fn cycle(list: &[f32], now: f32, d: i32) -> f32 {
    let i = (0..list.len()).min_by(|&a, &b| (list[a] - now).abs().total_cmp(&(list[b] - now).abs())).unwrap_or(0);
    list[(i as i32 + d).rem_euclid(list.len() as i32) as usize]
}

fn cycle_of<T: Copy + PartialEq>(list: &[T], now: T, d: i32) -> T {
    let i = list.iter().position(|x| *x == now).unwrap_or(0);
    list[(i as i32 + d).rem_euclid(list.len() as i32) as usize]
}

/// The camera that takes the post-process: the last world one on the window (the arms' in a round,
/// `--fly`'s). It does not clear, so the passes see the whole picture, and once.
#[derive(Component)]
pub struct PostCamera;

/// Bevy's indirect drawing costs a batch set per material on the CPU, and with thousands of statics whose materials are
/// nearly all different it costs more than it gives: Liberation_of_Caen, 34 fps with it, 74 without. On every 3D
/// camera from the moment it exists: a view with it next to one without does not validate (a mismatched bind group, and the app quits).
pub fn no_indirect_drawing(new: On<Add<Camera3d>>, mut commands: Commands) {
    commands.entity(new.entity).insert(bevy::render::view::NoIndirectDrawing);
}

/// The sun's cascades: medium is what we have always drawn with, out to 200 m.
pub fn cascades(q: ShadowQuality) -> CascadeShadowConfig {
    let (num_cascades, first_cascade_far_bound, maximum_distance) = match q {
        ShadowQuality::Low => (2, 15.0, 100.0),
        ShadowQuality::Medium => (4, 15.0, 200.0),
        ShadowQuality::High => (4, 10.0, 300.0),
    };
    CascadeShadowConfigBuilder { num_cascades, first_cascade_far_bound, maximum_distance, ..default() }.build()
}

fn shadow_map_size(q: ShadowQuality) -> usize {
    match q {
        ShadowQuality::Low => 1024,
        ShadowQuality::Medium => 2048,
        ShadowQuality::High => 4096,
    }
}

fn grading(look: Look) -> ColorGrading {
    // (no contrast: with it the lit soldiers came out black; the temperature alone goes
    // magenta, the tint against it keeps it warm)
    let global = match look {
        Look::Neutral => return ColorGrading::default(),
        Look::Vivid => ColorGradingGlobal { post_saturation: 1.25, ..default() },
        Look::Warm => ColorGradingGlobal { temperature: 0.04, tint: -0.04, post_saturation: 1.1, ..default() },
    };
    ColorGrading { global, ..default() }
}

/// Puts the graphics settings on the world's cameras and the sun when they change, or when a camera
/// or the sun is made: the anti-aliasing, the fog's distance, the mip bias and the shadows' filtering
/// on each; the post-process (FXAA/SMAA, CAS, bloom, colour, vignette) on [`PostCamera`]; what wants
/// the world's depth and motion (TAA, motion blur, SSAO, contact shadows) on [`WorldCamera`], before the
/// arms are drawn over it; the cascades, the shadow maps' size and the contact shadows on the sun.
pub fn apply(
    mut commands: Commands,
    mut last: Local<Option<Graphics>>,
    // (the new ones, and the sun to change)
    mut new_sun: ParamSet<(Query<(), Or<(Added<DistanceFog>, Added<PostCamera>, Added<DirectionalLight>)>>, Query<(&mut CascadeShadowConfig, &mut DirectionalLight)>)>,
    cameras: Query<(Entity, Has<PostCamera>, Has<ReflectionCamera>, Has<WorldCamera>), With<DistanceFog>>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    atm: Option<Res<Atmosphere>>,
) {
    let g = crate::assets::graphics();
    if *last == Some(g) && new_sun.p0().is_empty() {
        return;
    }
    let Some(atm) = atm else { return };
    *last = Some(g);
    let temporal = g.aa == Aa::Taa;
    for (e, post, mirror, world) in &cameras {
        let mut c = commands.entity(e);
        c.insert((atm.distance_fog(), bevy::render::camera::MipBias(g.mip_bias)));
        // (without point lights Bevy's clustering, on the GPU and for each camera, is for nothing; `None` breaks its GPU path)
        c.insert(if g.dyn_lights { bevy::light::cluster::ClusterConfig::default() } else { bevy::light::cluster::ClusterConfig::Single });
        c.insert(match g.shadow_quality {
            ShadowQuality::Low => ShadowFilteringMethod::Hardware2x2,
            // (the shadows' samples turn each frame and TAA blends them)
            _ if temporal && world => ShadowFilteringMethod::Temporal,
            _ => ShadowFilteringMethod::Gaussian,
        });
        if mirror {
            continue;
        }
        c.insert(if g.msaa() { Msaa::Sample4 } else { Msaa::Off });
        if world {
            // (only the world's: the arms' camera does not clear, its TAA would take the world's pixels
            // as still and smear them when turning; the arms keep their edges)
            if temporal {
                c.insert(TemporalAntiAliasing::default());
            } else {
                c.remove::<(TemporalAntiAliasing, TemporalJitter)>();
            }
            if g.motion_blur > 0.0 {
                c.insert((MotionBlur { shutter_angle: g.motion_blur, samples: 2 }, DepthPrepass));
            } else {
                c.remove::<MotionBlur>();
            }
            if g.ssao {
                // (a soldier is 1.8 m: Bevy's 0.73 leaves only a line at the walls' foot; more greys open ground)
                c.insert(ScreenSpaceAmbientOcclusion { radius: SSAO_RADIUS, ..default() });
            } else {
                c.remove::<(ScreenSpaceAmbientOcclusion, NormalPrepass)>();
            }
            if g.contact_shadows {
                // (Bevy's 0.1 m speckles what the sun grazes, the side of a crate; their noise wants TAA to blend it)
                c.insert(ContactShadows { length: CONTACT_LENGTH, thickness: CONTACT_THICKNESS, ..default() });
            } else {
                c.remove::<ContactShadows>();
            }
            if !temporal && g.motion_blur <= 0.0 {
                c.remove::<MotionVectorPrepass>();
            }
            if g.soft_particles || g.water_fx {
                // (the sprites and the water read it: `shaders/particle.wesl`, `water.wesl`)
                c.insert(DepthPrepass);
            } else if !temporal && g.motion_blur <= 0.0 && !g.ssao && !g.contact_shadows {
                c.remove::<DepthPrepass>();
            }
            // (what is blended goes to a list per pixel, sorted and blended after the transparent pass:
            // Bevy's `pbr.wesl`, and ours under `MATERIAL_OIT_ENABLED`; a pipeline is made the first time
            // something is drawn with it, so the round's first puff of smoke may miss a few frames)
            if g.oit {
                c.insert(OrderIndependentTransparencySettings { sorted_fragment_max_count: OIT_SORTED, ..default() });
            } else {
                c.remove::<OrderIndependentTransparencySettings>();
            }
            // (a copy of the world drawn so far, under the water: it goes in Bevy's transmissive pass. Always, even
            // without `water_fx` or `water_gamma`: without it Bevy sorts the water with the branches, by the centre of
            // each mesh, and the water, bigger, covered the fronds in front of it: Midway's palms in Mejorado)
            c.insert(ScreenSpaceTransmission::default());
        }
        // (bloom and the grading want a float picture: Bevy grades in its tone mapping pass, which only
        // runs on one; every camera on the window the same, or they would not share it)
        if g.bloom > 0.0 || g.look != Look::Neutral {
            c.insert(Hdr);
        } else {
            c.remove::<Hdr>();
        }
        if !post {
            continue;
        }
        match g.aa {
            Aa::Fxaa => c.insert(Fxaa::default()).remove::<Smaa>(),
            Aa::Smaa => c.insert(Smaa::default()).remove::<Fxaa>(),
            Aa::Off | Aa::Msaa | Aa::Taa => c.remove::<(Fxaa, Smaa)>(),
        };
        // (drawn smaller, it sharpens after the scaling: `render_scale`)
        if g.sharpen > 0.0 && g.render_scale >= 1.0 {
            c.insert(ContrastAdaptiveSharpening { enabled: true, sharpening_strength: g.sharpen, denoise: false });
        } else {
            c.remove::<ContrastAdaptiveSharpening>();
        }
        if g.bloom > 0.0 {
            c.insert(Bloom { intensity: g.bloom, prefilter: BloomPrefilter { threshold: 0.6, threshold_softness: 0.3 }, ..Bloom::NATURAL });
        } else {
            c.remove::<Bloom>();
        }
        // (with no tone mapping Bevy skips the pass that grades: linear is the same curve, clamped)
        c.insert(if g.look == Look::Neutral { (Tonemapping::None, ColorGrading::default()) } else { (Tonemapping::Linear, grading(g.look)) });
        if g.vignette > 0.0 {
            c.insert(Vignette { intensity: g.vignette, ..default() });
        } else {
            c.remove::<Vignette>();
        }
    }
    for (mut s, mut light) in &mut new_sun.p1() {
        *s = cascades(g.shadow_quality);
        light.contact_shadows_enabled = g.contact_shadows && light.illuminance > 0.0;
    }
    let size = shadow_map_size(g.shadow_quality);
    if shadow_map.size != size {
        shadow_map.size = size;
    }
}

/// A new history for TAA when the world's camera jumps (a spawn, a change of seat, `--at`): what it kept
/// would smear over the new view for a few frames.
pub fn taa_reset(mut cams: Query<(&GlobalTransform, &mut TemporalAntiAliasing), With<WorldCamera>>, mut last: Local<Option<Vec3>>) {
    for (t, mut taa) in &mut cams {
        let p = t.translation();
        if last.is_some_and(|l| l.distance(p) > JUMP) {
            taa.reset = true;
        }
        *last = Some(p);
    }
}

/// What Bevy's UI shader writes, put in gamma: on a 2D camera with [`CompositingSpace::Srgb`](bevy::camera::CompositingSpace)
/// the picture under the UI keeps gamma values and the blending is done on them, as D3D8's (Bevy blends
/// in linear light, so a dark translucent panel came out lighter, and a coloured one washed out: GC's cyan briefing).
/// Level 2. Every camera the UI is drawn on must then be one: [`Upscale`]'s, the menu's, the first screen's.
/// Bevy's shader (`bevy_ui_render/ui.wesl`) is the only UI one we use (no materials, slices, gradients or shadows).
pub fn ui_in_gamma(mut events: MessageReader<AssetEvent<Shader>>, mut shaders: ResMut<Assets<Shader>>) {
    use bevy::shader::Source;
    for e in events.read() {
        let (AssetEvent::Added { id } | AssetEvent::Modified { id }) = e else { continue };
        let Some(shader) = shaders.get(*id) else { continue };
        if !shader.path.ends_with("bevy_ui_render/ui.wesl") {
            continue;
        }
        let (Source::Wesl(text) | Source::Wgsl(text)) = &shader.source else { continue };
        let Some(patched) = ui_shader_in_gamma(text) else { continue };
        let Some(mut shader) = shaders.get_mut(*id) else { continue };
        shader.source = match shader.source {
            Source::Wesl(_) => Source::Wesl(patched.into()),
            _ => Source::Wgsl(patched.into()),
        };
    }
}

/// [`ui_in_gamma`]'s change to the shader's text: its fragment renamed, and a new one that encodes what it returns.
/// `None` when already done, or when Bevy's shader no longer has the line it looks for.
fn ui_shader_in_gamma(text: &str) -> Option<String> {
    const FRAGMENT: &str = "@fragment\nfn fragment(in: VertexOutput) -> @location(0) vec4<f32> {";
    if text.contains("fn fragment_linear(") || !text.contains(FRAGMENT) {
        if !text.contains("fn fragment_linear(") {
            warn!("ui_in_gamma: Bevy's UI shader changed; the UI blends in linear light");
        }
        return None;
    }
    Some(text.replacen(FRAGMENT, "fn fragment_linear(in: VertexOutput) -> vec4<f32> {", 1)
        + "
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = fragment_linear(in);
    let x = clamp(c.rgb, vec3(0.0), vec3(1.0));
    return vec4(select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3(0.0031308)), c.a);
}
")
}

/// The world drawn smaller, stretched over the window by the 2D camera under it ([`render_scale`]).
#[derive(Component)]
pub struct Upscale;

/// The layer of [`Upscale`]'s picture (no other camera sees it)
const UPSCALE_LAYER: usize = 30;

/// The world drawn at `render_scale` of the window's size: the window's 3D cameras (the world's, the
/// arms') draw into an image that [`Upscale`], a 2D camera after them, stretches over the window (filtered; below 1,
/// sharpened there with CAS). The HUD and the menus go on that camera, at the window's size, blended in gamma as
/// the original's D3D8 does ([`ui_in_gamma`]): so the image even at 1.
#[allow(clippy::type_complexity)]
pub fn render_scale(
    mut commands: Commands,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut cams: Query<&mut bevy::camera::RenderTarget, (With<Camera3d>, With<DistanceFog>, Without<ReflectionCamera>)>,
    mut upscale: Query<(Entity, &mut Sprite), With<Upscale>>,
    mut images: ResMut<Assets<Image>>,
    mut last: Local<Option<(UVec2, f32)>>,
) {
    use bevy::camera::RenderTarget;
    let g = crate::assets::graphics();
    let Ok(w) = window.single() else { return };
    let full = w.physical_size().max(UVec2::ONE);
    let size = (full.as_vec2() * g.render_scale).round().as_uvec2().max(UVec2::ONE);
    // (always: the UI wants the world under it in its own camera, [`ui_in_gamma`])
    let scaled = true;
    let image = upscale.single().ok().map(|(_, s)| s.image.clone()).filter(|_| scaled);
    // (a new image when its size or the sharpening changes; a camera made since is put on it below)
    let same = *last == Some((size, g.sharpen)) && image.is_some();
    // (only a new size: the same picture with a new image. A window resized in the frame after the picture was made
    // would despawn it under the insert of `Mesh2d` that Bevy has queued for a sprite, and that closes the program)
    let resized = scaled && last.is_some_and(|(_, sharpen)| sharpen == g.sharpen);
    if !same {
        *last = Some((size, g.sharpen));
        if !resized {
            for (e, s) in &upscale {
                images.remove(&s.image);
                commands.entity(e).despawn();
            }
        }
    }
    if !scaled {
        for mut t in &mut cams {
            if !matches!(*t, RenderTarget::Window(_)) {
                *t = RenderTarget::default();
            }
        }
        return;
    }
    let image = match image.filter(|_| same) {
        Some(i) => i,
        None => {
            let mut img = Image::new_target_texture(size.x, size.y, bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb, None);
            img.sampler = bevy::image::ImageSampler::linear();
            let i = images.add(img);
            match upscale.single_mut() {
                Ok((_, mut s)) if resized => {
                    images.remove(&s.image);
                    s.image = i.clone();
                }
                _ => {
                    // (the picture, and its camera under it: both go together)
                    let picture = commands.spawn((Sprite { image: i.clone(), custom_size: Some(w.size()), ..default() }, RenderLayers::layer(UPSCALE_LAYER), Upscale)).id();
                    let mut c = commands.spawn((Camera2d, Camera { order: 2, ..default() }, bevy::camera::CompositingSpace::Srgb, Tonemapping::None, bevy::core_pipeline::tonemapping::DebandDither::Disabled, RenderLayers::layer(UPSCALE_LAYER), ChildOf(picture)));
                    if g.sharpen > 0.0 && size != full {
                        c.insert(ContrastAdaptiveSharpening { enabled: true, sharpening_strength: g.sharpen, denoise: false });
                    }
                }
            }
            i
        }
    };
    for (_, mut s) in &mut upscale {
        if s.custom_size != Some(w.size()) {
            s.custom_size = Some(w.size());
        }
    }
    for mut t in &mut cams {
        if !matches!(&*t, RenderTarget::Image(i) if i.handle == image) {
            *t = RenderTarget::Image(image.clone().into());
        }
    }
}

/// While aiming through a sight with the setting `dof`: the world's camera (not the arms') focuses where
/// the sight's line meets something ([`Projectile::sight`], up to [`FOCUS_REACH`]), eased over a few frames;
/// otherwise no depth of field.
pub fn dof(mut commands: Commands, game: Option<Res<crate::play::Game>>, cams: Query<(Entity, &GlobalTransform, Option<&DepthOfField>), With<crate::play::PlayCamera>>, time: Res<Time>) {
    let Ok((e, t, had)) = cams.single() else { return };
    let aiming = game.as_ref().is_some_and(|g| crate::play::zoom(g).is_some());
    let Some(g) = game.filter(|_| aiming && crate::assets::graphics().dof) else {
        if had.is_some() {
            commands.entity(e).remove::<DepthOfField>();
        }
        return;
    };
    let sim = &g.session.sim;
    let from = t.translation();
    let ahead = t.forward().as_vec3() * FOCUS_REACH;
    let sim_v = |v: Vec3| [v.x, v.y, -v.z];
    let me = sim.players[g.player].soldier;
    let at = bytefield_sim::projectile::Projectile::sight(&sim.world.level, sim.obstacles(), &sim.vehicles, sim_v(from), sim_v(ahead), me);
    let want = at.map_or(FOCUS_REACH, |p| crate::assets::to_bevy(p).distance(from)).max(1.0);
    // (towards it in some 0.1 s; at once the first frame)
    let focal = had.map_or(want, |d| d.focal_distance + (want - d.focal_distance) * (time.delta_secs() * 10.0).min(1.0));
    commands.entity(e).insert(DepthOfField { mode: DepthOfFieldMode::Gaussian, focal_distance: focal, aperture_f_stops: DOF_F_STOPS, max_circle_of_confusion_diameter: DOF_MAX_BLUR, max_depth: FOCUS_REACH, ..default() });
}

/// how far the focus looks, metres (beyond, the sky and the far land are taken as there)
const FOCUS_REACH: f32 = 1500.0;
/// the depth of field's aperture: the lower, the more what is off the focus blurs
const DOF_F_STOPS: f32 = 2.0;
/// its widest blur, pixels
const DOF_MAX_BLUR: f32 = 12.0;

/// SSAO's reach, metres (view space)
const SSAO_RADIUS: f32 = 1.0;
/// how far towards the sun a contact shadow looks, metres
const CONTACT_LENGTH: f32 = 0.5;
/// how thick what the depth shows is taken to be, metres
const CONTACT_THICKNESS: f32 = 0.3;
/// the layers OIT sorts exactly per pixel (Bevy's 8 left the edges of the quads in a shell's cloud; 16 and 32
/// differ in a few hundred pixels); past them they are blended in out of order
const OIT_SORTED: u32 = 16;

/// metres in a frame that are a jump, not a move (a plane at full speed does some 2)
const JUMP: f32 = 10.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ui_shader_is_put_in_gamma_once() {
        let bevy = "fn f() {}\n@fragment\nfn fragment(in: VertexOutput) -> @location(0) vec4<f32> {\n    return in.color;\n}\n";
        let once = ui_shader_in_gamma(bevy).unwrap();
        assert_eq!(once.matches("@fragment").count(), 1);
        assert!(once.contains("fn fragment_linear(in: VertexOutput) -> vec4<f32> {\n    return in.color;"));
        assert!(once.contains("let c = fragment_linear(in);"));
        assert_eq!(ui_shader_in_gamma(&once), None);
        assert_eq!(ui_shader_in_gamma("fn something_else() {}"), None);
    }

    #[test]
    fn rows_cycle_and_keep_only_what_differs() {
        let mut s = Settings::default();
        Row::Bloom.step(&mut s, 1);
        assert_eq!(s.content.overrides.bloom, Some(0.15));
        Row::Bloom.step(&mut s, -1);
        assert_eq!(s.content.overrides, Overrides::default(), "back to the profile's: nothing kept");
        Row::Shadows.step(&mut s, 1);
        assert_eq!((s.content.overrides.shadows, s.content.overrides.shadow_quality), (Some(true), Some(ShadowQuality::Low)));
        Row::Shadows.step(&mut s, -1);
        assert_eq!(s.content.overrides.shadows, None);
        Row::Aa.step(&mut s, -1);
        assert_eq!(s.content.overrides.aa, Some(Aa::Off));
        Row::Aa.step(&mut s, -1);
        assert_eq!(s.content.overrides.aa, Some(Aa::Taa), "round the end: the temporal one");
        Row::MotionBlur.step(&mut s, 1);
        assert_eq!(s.content.overrides.motion_blur, Some(0.25));
        let t = |k: &str| k.to_string();
        assert_eq!(Row::Profile.value(&s, &t), "VIDEO_OPTIONS_CUSTOM");
        Row::Profile.step(&mut s, 1);
        assert_eq!((s.content.graphics, s.content.overrides), (Profile::Enhanced, Overrides::default()));
        assert_eq!(Row::Profile.value(&s, &t), "GRAPHICS_ENHANCED");
        Row::Fov.step(&mut s, 1);
        assert_eq!(s.fov, 65.0);
        assert_eq!(cycle(TEXTURES, 4096.0, 1), 512.0);
    }
}
