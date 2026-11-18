//! The world under the blended trees: D3D8 blends in the screen's (gamma) space and Bevy in linear light, so a
//! dark twig half seen over the sand came out lighter (Tobruk's bush, +22). `shaders/tree.wesl` takes the alpha that makes
//! Bevy's blending come out as D3D8's, from a copy of what is under it. Bevy's copy for its transmissive pass
//! (`view_transmission_texture`, the water's) is taken before the water is drawn and only when something transmissive is
//! in view; here, on the cameras that have it ([`ScreenSpaceTransmission`], the setting `water_gamma`), the texture is
//! always there and is copied again after that pass, before the transparent one, with the water in it. Level 2.
use bevy::core_pipeline::{core_3d::main_transparent_pass_3d, schedule::{Core3d, Core3dSystems}};
use bevy::image::ToExtents;
use bevy::pbr::{main_transmissive_pass_3d, ScreenSpaceTransmission, ViewTransmissionTexture};
use bevy::prelude::*;
use bevy::render::{
    camera::ExtractedCamera,
    render_resource::{FilterMode, SamplerDescriptor, TextureDescriptor, TextureDimension, TextureUsages},
    renderer::{RenderContext, RenderDevice, ViewQuery},
    texture::TextureCache,
    view::{ExtractedView, ViewTarget},
    Render, RenderApp, RenderSystems,
};

pub struct BackdropPlugin;

impl Plugin for BackdropPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(Render, always_a_copy.in_set(RenderSystems::PrepareResourcesFlush))
            .add_systems(Core3d, copy_with_the_water.after(main_transmissive_pass_3d).before(main_transparent_pass_3d).in_set(Core3dSystems::MainPass));
    }
}

/// The copy's texture on each camera with transmission, whether or not Bevy made it this frame (it makes it only with
/// something transmissive in view: Tobruk has no water). The same as Bevy's: the view's size and format, one sample.
fn always_a_copy(mut commands: Commands, mut cache: ResMut<TextureCache>, device: Res<RenderDevice>, views: Query<(Entity, &ExtractedCamera, &ExtractedView), With<ScreenSpaceTransmission>>) {
    for (entity, camera, view) in &views {
        let Some(size) = camera.physical_target_size else { continue };
        let texture = cache.get(
            &device,
            TextureDescriptor {
                label: Some("bytefield_backdrop"),
                size: size.to_extents(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: view.target_format,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
        );
        let sampler = device.create_sampler(&SamplerDescriptor { label: Some("bytefield_backdrop"), mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
        commands.entity(entity).insert(ViewTransmissionTexture { texture: texture.texture, view: texture.default_view, sampler });
    }
}

/// After the transmissive pass (the water), the picture so far into the copy, for the transparent pass (the trees).
fn copy_with_the_water(view: ViewQuery<(&ExtractedCamera, &ViewTarget, &ViewTransmissionTexture), With<ScreenSpaceTransmission>>, mut ctx: RenderContext) {
    let (camera, target, copy) = view.into_inner();
    let Some(size) = camera.physical_target_size else { return };
    ctx.command_encoder().copy_texture_to_texture(target.main_texture().as_image_copy(), copy.texture.as_image_copy(), size.to_extents());
}
