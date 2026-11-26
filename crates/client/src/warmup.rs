//! The pipelines' warm-up. Bevy compiles a pipeline for each new combination of material, mesh and view the first time
//! one is seen, in the background: a mesh with a new one (DC's M67, `envmap` with lightmap UVs) was missing for a second or
//! two (the original draws all at once). Whatever gets a mesh is drawn wherever it is, not only in the view's frustum, for
//! a moment, so its variants are asked for when it appears and not when the camera first turns to it: the level's for a
//! few seconds after its camera appears (while the camera before spawning is shown), what appears later (a soldier and his
//! kit, a vehicle, a wreck) for half a second. A scripted run compiles every pipeline the frame it is needed (`main.rs`),
//! with or without this.
//!
//! `BYTEFIELD_WARMUP=0` leaves it out; `BYTEFIELD_PIPELINES=1` logs how many pipelines there are each time the count grows.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

/// How long the level's meshes are drawn after its camera appears, seconds: the meshes and textures reach the GPU a few
/// frames after they are added
const LEVEL: f32 = 2.0;
/// How long what appears later is drawn, seconds
const LATER: f32 = 0.5;

/// Drawn outside the frustum until then ([`Time::elapsed_secs`]), for the warm-up (and without `NoFrustumCulling` of its own).
#[derive(Component)]
pub struct Warming(f32);

pub struct WarmupPlugin;

impl Plugin for WarmupPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var_os("BYTEFIELD_PIPELINES").is_some()
            && let Some(r) = app.get_sub_app_mut(bevy::render::RenderApp)
        {
            r.add_systems(bevy::render::Render, log_pipelines.in_set(bevy::render::RenderSystems::Cleanup));
        }
        if !std::env::var("BYTEFIELD_WARMUP").is_ok_and(|v| v == "0") {
            app.add_systems(PostUpdate, warm.before(bevy::camera::visibility::VisibilitySystems::CheckVisibility));
        }
    }
}

fn warm(
    mut commands: Commands,
    time: Res<Time>,
    mut level_until: Local<f32>,
    level: Query<(), Added<crate::level::WorldCamera>>,
    new: Query<Entity, (Added<Mesh3d>, Without<NoFrustumCulling>)>,
    warming: Query<(Entity, &Warming)>,
) {
    let now = time.elapsed_secs();
    if !level.is_empty() {
        *level_until = now + LEVEL;
    }
    let until = if now < *level_until { *level_until } else { now + LATER };
    for e in &new {
        commands.entity(e).insert((NoFrustumCulling, Warming(until)));
    }
    for (e, w) in &warming {
        if now >= w.0 {
            commands.entity(e).try_remove::<(NoFrustumCulling, Warming)>();
        }
    }
}

/// (with `BYTEFIELD_PIPELINES`) The pipelines made so far, logged when their count grows, and how many still compile.
fn log_pipelines(cache: Res<bevy::render::render_resource::PipelineCache>, mut seen: Local<usize>) {
    let n = cache.pipelines().count();
    if n != *seen {
        *seen = n;
        info!("pipelines: {n} ({} compiling)", cache.waiting_pipelines().count());
    }
}
