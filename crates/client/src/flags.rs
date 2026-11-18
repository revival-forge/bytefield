//! The flags of capture the flag: each base's flag (its `flagTemplate`'s model) where
//! the simulation has it: at home on its base, on its carrier, or on the ground. The bases
//! themselves are the level's static objects.
//!
//! And a control point's flag (`setTeamGeometry`, `scene::team_flag`): its team's geometry, none
//! for a team without one (the neutral). Only the team reaches a client (`ControlPointNetworkable`
//! carries it and the enabled bit, not how far a capture goes), so the flag changes, it does not climb.
//! It hangs from its skeleton (`createSkeleton`, `setSkin`, `setAnimationState`): the bare mesh is centred on the
//! pole's tip and upside down.
//! TODO: the flags of capture the flag still stand as a bare mesh.

use std::sync::Arc;

use bevy::prelude::*;
use bytefield_world::scene::{self, Mat4};

use bytefield_formats::anim::Skeleton;

use crate::assets::{self, Cache};
use crate::level;
use crate::model;
use crate::play::Game;
use crate::skin;

/// The flag of base `.0`.
#[derive(Component)]
struct FlagOf(usize);

/// Control point `.0`'s flag when it is team `.1`'s.
#[derive(Component)]
struct PointFlag(usize, u32);

/// A flag's mesh waving on its skeleton: one material of its skin, its own copy of the mesh.
#[derive(Component)]
struct Waving {
    bones: skin::Looping,
    skin: Arc<skin::MaterialSkin>,
    mesh: Handle<Mesh>,
}

/// How far the wave takes a vertex from the box of the pose it was spawned in.
const WAVE_REACH: f32 = 2.0;

pub struct FlagsPlugin;

impl Plugin for FlagsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, ((setup, setup_points).run_if(resource_added::<Game>), (follow, wave_team, wave).run_if(resource_exists::<Game>)).chain());
    }
}

fn setup(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let sim = &g.session.sim;
    let Some(c) = sim.modes.ctf.as_ref() else { return };
    let world = &sim.world;
    for (b, base) in c.bases.iter().enumerate() {
        let draws = model::draws(&world.db, &base.flag_template, &[]);
        info!("ctf: the flag of {} ({}), {} draws", base.template, base.flag_template, draws.len());
        commands.spawn((Transform::default(), Visibility::default(), FlagOf(b))).with_children(|f| {
            for d in draws {
                let t = assets::to_transform(&d.local);
                for (m, mat) in cache.get(&world.vfs, &d.kind, &d.file, &mut meshes, &mut materials, &mut images) {
                    f.spawn((Mesh3d(m), MeshMaterial3d(mat), t));
                }
            }
        });
    }
}

/// Each flag where the simulation has it (a carrier's: over his head).
fn follow(g: Res<Game>, mut flags: Query<(&FlagOf, &mut Transform)>) {
    let Some(c) = g.session.sim.modes.ctf.as_ref() else { return };
    for (f, mut t) in flags.iter_mut() {
        let Some(flag) = c.bases.get(f.0).map(|b| &b.flag) else { continue };
        let up = if flag.carrier.is_some() { 1.0 } else { 0.0 };
        let want = assets::to_transform(&Mat4::from_pos_ypr([flag.pos[0] as f64, (flag.pos[1] + up) as f64, flag.pos[2] as f64], [0.0; 3]));
        if *t != want {
            *t = want;
        }
    }
}

/// Each control point's flags, one per team that has a geometry, hidden until [`wave_team`] shows its
/// team's. The points are the level's `ControlPoint` instances in order (as [`bytefield_sim::conquest::Conquest::from_db`]).
fn setup_points(mut commands: Commands, g: Res<Game>, mut cache: ResMut<Cache>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let world = &g.session.sim.world;
    let db = &world.db;
    let points = db.instances.iter().filter(|i| db.object_template(&i.template).is_some_and(|t| t.kind.eq_ignore_ascii_case("controlpoint")));
    for (k, (inst, at)) in points.filter_map(|i| Some((i, scene::instance_world(i)?))).enumerate() {
        let Some(((name, local), teams)) = db.object_template(&inst.template).and_then(scene::team_flag) else { continue };
        let t = assets::to_transform(&at.mul(&local));
        let bundle = db.object_template(&name);
        let ske = bundle.and_then(|b| b.get("createskeleton")).and_then(|a| a.first()).map(|f| f.replace('\\', "/")).and_then(|f| Some(Arc::new(Skeleton::parse(&world.vfs.read(&f).ok()?).ok()?)));
        let state = bundle.and_then(|b| b.get("setanimationstate")).and_then(|a| a.first());
        for (team, geometry) in teams {
            let Some(gt) = db.get("geometrytemplate", &geometry) else { continue };
            let Some(file) = gt.get("file").and_then(|a| a.first()) else { continue };
            let file = file.replace('\\', "/");
            let kind = gt.kind.to_ascii_lowercase();
            let parts = cache.get(&world.vfs, &kind, &file, &mut meshes, &mut materials, &mut images);
            let skins = gt.get("setskin").and_then(|a| a.first()).zip(ske.as_ref()).and_then(|(f, sk)| skin::load(&world.vfs, &file, &f.replace('\\', "/"), sk, false)).filter(|s| s.len() == parts.len());
            for (i, (m, mat)) in parts.into_iter().enumerate() {
                // its own moment of the wave (`setUserRandomStartTime`), the same for all its materials
                let start = (k as f32 + team as f32 * 0.5) * 0.37;
                let waving = skins.as_ref().zip(ske.as_ref()).zip(state).and_then(|((s, sk), state)| {
                    let mut bones = skin::Looping::new(sk.clone(), world.animations(), state, start)?;
                    // posed from the start: it may show before `wave` sees it
                    let mut mesh = meshes.get(&m)?.clone();
                    skin::write(&s[i], skin::positions(&s[i], bones.advance(0.0)), &mut mesh);
                    Some(Waving { bones, skin: s[i].clone(), mesh: meshes.add(mesh) })
                });
                let Some(w) = waving else {
                    commands.spawn((Mesh3d(m), MeshMaterial3d(mat), t, Visibility::Hidden, PointFlag(k, team)));
                    continue;
                };
                let bounds = meshes.get(&w.mesh).and_then(|m| <Mesh as bevy::camera::primitives::MeshAabb>::get_aabb(m));
                let mut e = commands.spawn((Mesh3d(w.mesh.clone()), MeshMaterial3d(mat), t, Visibility::Hidden, PointFlag(k, team)));
                if let Some(b) = bounds {
                    e.insert((bevy::camera::visibility::NoAutoAabb, bevy::camera::primitives::Aabb { center: b.center, half_extents: b.half_extents + bevy::math::Vec3A::splat(WAVE_REACH) }));
                }
                e.insert(w);
            }
        }
    }
}

/// The flags in view, on their skeletons; far away, every few frames (as the soldiers' skins, `play::interpolate`).
fn wave(time: Res<Time>, mut flags: Query<(Entity, &mut Waving, &ViewVisibility, &GlobalTransform)>, mut meshes: ResMut<Assets<Mesh>>, queue: Res<skin::SkinQueue>, eye: Query<&GlobalTransform, With<level::WorldCamera>>, mut frame: Local<u32>) {
    *frame = frame.wrapping_add(1);
    let from = eye.iter().next().map(|t| t.translation());
    let dt = time.delta_secs();
    for (e, mut w, seen, at) in flags.iter_mut() {
        let stride = from.map_or(1, |f| match f.distance(at.translation()) {
            d if d < 60.0 => 1,
            d if d < 200.0 => 3,
            _ => 8,
        });
        let w = &mut *w;
        // (hidden or out of view it does not move, and nobody can tell)
        if !seen.get() || !(e.index_u32() + *frame).is_multiple_of(stride) {
            continue;
        }
        let pos = skin::positions(&w.skin, w.bones.advance(dt * stride as f32));
        if let Some(mesh) = meshes.get_mut_untracked(&w.mesh) {
            skin::write(&w.skin, pos, mesh);
            queue.push(w.mesh.id(), mesh.create_packed_vertex_buffer_data());
        }
    }
}

/// The flag of each control point's team.
fn wave_team(g: Res<Game>, mut flags: Query<(&PointFlag, &mut Visibility)>) {
    let Some(c) = g.session.sim.modes.conquest.as_ref() else { return };
    for (f, mut v) in flags.iter_mut() {
        let want = if c.points.get(f.0).is_some_and(|p| p.team == f.1) { Visibility::Inherited } else { Visibility::Hidden };
        v.set_if_neq(want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag hangs from its skeleton: it comes out of the pole on one side only and below the tip, upright (the black at the top
    /// on the German one) and waves. Without a skin it was centred on the tip and upside down (El_Alamein).
    #[test]
    fn a_flag_hangs_from_its_pole_and_waves() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").expect("level");
        let ske = Arc::new(Skeleton::parse(&w.vfs.read("animations/flag.ske").unwrap()).unwrap());
        let skins = skin::load(&w.vfs, "flagge_m1", "animations/flag.skn", &ske, false).expect("its skin");
        let mut bones = skin::Looping::new(ske, w.animations(), "FlagBlow", 0.0).expect("its state");
        let a: Vec<[f32; 3]> = skins.iter().flat_map(|s| skin::positions(s, bones.advance(0.0))).collect();
        let b: Vec<[f32; 3]> = skins.iter().flat_map(|s| skin::positions(s, bones.advance(0.3))).collect();
        let span = |k: usize| a.iter().fold((f32::MAX, f32::MIN), |r, p| (r.0.min(p[k]), r.1.max(p[k])));
        eprintln!("x {:?} y {:?} z {:?}", span(0), span(1), span(2));
        assert_ne!(a, b, "it waves");
        let (x, z) = (span(0), span(2));
        let (from, to) = if x.1 - x.0 > z.1 - z.0 { x } else { z };
        assert!(from.abs().min(to.abs()) < 0.3 && from.abs().max(to.abs()) > 1.0, "out to one side of the pole: {from} to {to}");
    }

    /// `FlagBlow` breaks into `FlagIdle1`/`FlagIdle2` once its idle timer (4–7 s) runs out, and comes back.
    #[test]
    fn a_flag_breaks_into_its_idles_and_comes_back() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").expect("level");
        let ske = Arc::new(Skeleton::parse(&w.vfs.read("animations/flag.ske").unwrap()).unwrap());
        let m = w.animations();
        let (blow, idles) = (m.find("FlagBlow").unwrap(), [m.find("FlagIdle1").unwrap(), m.find("FlagIdle2").unwrap()]);
        let mut bones = skin::Looping::new(ske, m, "FlagBlow", 0.37).expect("its state");
        let seen: Vec<usize> = (0..30 * 30).filter_map(|_| { bones.advance(1.0 / 30.0); bones.state() }).collect();
        let first_idle = seen.iter().position(|s| idles.contains(s)).expect("an idle within 30 s");
        assert!((4 * 30 - 2..=8 * 30).contains(&first_idle), "after the idle timer: frame {first_idle}");
        assert!(seen[first_idle..].contains(&blow), "back to FlagBlow");
    }
}
