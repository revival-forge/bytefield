//! The play camera: eye placement, vehicle zoom, the scope overlay and the cockpit bars.

use super::*;

/// How far in each side's bar goes (normalised x, −1..1): from the screen's edge inwards, the
/// first column whose rays all hit one of the triangles, at most to `limit` (the 4:3 strip).
pub(super) fn cockpit_edges(tris: &[[Vec3; 3]], eye: &Transform, tan_y: f32, aspect: f32, limit: f32) -> [f32; 2] {
    const ROWS: [f32; 5] = [-0.9, -0.45, 0.0, 0.45, 0.9];
    const STEPS: usize = 200;
    let hits = |x: f32, y: f32| {
        let d = eye.rotation * Vec3::new(x * tan_y * aspect, y * tan_y, -1.0);
        tris.iter().any(|t| ray_hits(eye.translation, d, t))
    };
    let side = |sign: f32| {
        (0..=STEPS).map(|k| 1.0 - (1.0 - limit) * k as f32 / STEPS as f32).find(|&x| ROWS.iter().all(|&y| hits(sign * x, y))).unwrap_or(limit)
    };
    [-side(-1.0), side(1.0)]
}

/// Möller–Trumbore: whether the ray from `o` along `d` crosses the triangle ahead.
pub(super) fn ray_hits(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> bool {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return false;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    v >= 0.0 && u + v <= 1.0 && e2.dot(q) * inv > 1e-4
}

/// The triangles of a mesh, placed by `t`.
pub(super) fn mesh_triangles(mesh: &Mesh, t: &Transform, out: &mut Vec<[Vec3; 3]>) {
    let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let p = |i: usize| t.transform_point(Vec3::from(pos[i]));
    let idx: Vec<usize> = match mesh.indices() {
        Some(ix) => ix.iter().collect(),
        None => (0..pos.len()).collect(),
    };
    out.extend(idx.chunks_exact(3).filter(|c| c.iter().all(|&i| i < pos.len())).map(|c| [p(c[0]), p(c[1]), p(c[2])]));
}

/// The player's weapon zoomed: the world's field of view becomes
/// `zoomFov`, the first person's is multiplied by `soldierZoomFov` (the soldier's), the
/// arms move by `soldierZoomPosition` instead of `soldierCameraPosition`; a scope instead hides
/// the arms behind its picture.
pub struct Zoom {
    pub fov: f32,
    pub arms: f32,
    pub scope: bool,
}

/// `vehicleFov` of the PlayerControlObject the player sits in: 0 or none leaves the view's
/// angle; any other replaces it, in `renderer.fieldOfView`'s unit, in every view mode (nothing there looks at the mode).
pub(super) fn vehicle_fov(g: &Game) -> Option<f32> {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let e = &sim.vehicles[p.soldier.filter(|_| p.alive)?].entry;
    let seat = sim.vehicles[e.inside?].entry.seats.get(e.position)?;
    let fov = sim.world.db.object_template(&seat.pco)?.get("vehiclefov")?.first()?.trim().parse::<f32>().ok()?;
    (fov != 0.0).then_some(fov)
}

pub fn zoom(g: &Game) -> Option<Zoom> {
    let p = &g.session.sim.players[g.player];
    let s = &g.session.sim.vehicles[p.soldier.filter(|_| p.alive)?];
    let w = s.movement.as_ref()?.weapon.as_ref().filter(|w| w.zoomed() && s.entry.inside.is_none())?;
    let t = g.session.sim.world.db.object_template(&w.template.name)?;
    let num = |k: &str| t.get(k).and_then(|a| a.first()).and_then(|a| a.trim().parse::<f32>().ok());
    let arms = num("soldierzoomfov").filter(|&k| k > 0.0).unwrap_or(1.0);
    let scope = num("usescope").is_some_and(|v| v != 0.0) || num("setsnipersight").is_some_and(|v| v != 0.0);
    Some(Zoom { fov: w.template.zoom_fov, arms, scope })
}

/// Where the player looks from after a tick: its soldier's camera, or in a vehicle its seat's
/// `Camera` part (the body without one); in third person, behind and above that. Dead, the
/// last one stays.
pub(super) fn camera_now(g: &Game) -> Transform {
    let p = &g.session.sim.players[g.player];
    let Some(i) = p.soldier.filter(|_| p.alive) else { return g.camera.1 };
    let s = &g.session.sim.vehicles[i];
    if let Some(p) = g.scout_view.and_then(|k| g.session.sim.projectiles.get(k)) {
        return Transform::from_translation(assets::to_bevy(p.pos)).looking_to(assets::to_bevy(p.rows[2]), assets::to_bevy(p.rows[1]));
    }
    if let Some(c) = g.chase.as_ref() {
        return Transform::from_translation(c.eye).looking_at(c.look, Vec3::Y);
    }
    let (rows, pos, chase) = match s.entry.inside {
        Some(k) => {
            let v = &g.session.sim.vehicles[k];
            let seat = s.entry.position;
            let frames = v.craft.frames();
            match v.craft.parts.iter().position(|q| matches!(q.kind, Kind::Camera) && q.seat == seat) {
                Some(c) => {
                    let (r, mut at) = (frames[c].child_rows, frames[c].pos);
                    // the nose camera sits at the offset, in the camera's axes
                    if let Some(o) = nose_offset(g, v, seat).filter(|_| g.nose) {
                        at = [0, 1, 2].map(|k| at[k] + o[0] * r[0][k] + o[1] * r[1][k] + o[2] * r[2][k]);
                    }
                    (r, at, CHASE[1])
                }
                None => (v.craft.body.rows, v.craft.body.pos, CHASE[1]),
            }
        }
        None => match on_foot_view(g, Vec2::ZERO) {
            Some((rows, pos)) => (rows, pos, CHASE[0]),
            None => (s.craft.body.rows, s.craft.body.pos, CHASE[0]),
        },
    };
    place(g, assets::to_bevy(pos), &rows, chase)
}

/// The camera at `at` with these axes (x, y, z rows in the simulation's frame): from them
/// (first person), or behind and above (third, `chase`: how far back and up).
pub(super) fn place(g: &Game, at: Vec3, rows: &[[f32; 3]; 3], chase: (f32, f32)) -> Transform {
    let (fwd, up) = (assets::to_bevy(rows[2]), assets::to_bevy(rows[1]));
    if g.third_person {
        let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
        if g.chase_yaw != 0.0 {
            // from the side (a check's view): looking at it
            // (`BYTEFIELD_CHASE_DIST`: nearer, to compare a seat with the original's)
            let flat = Quat::from_rotation_y(-g.chase_yaw.to_radians()) * flat;
            let back = std::env::var("BYTEFIELD_CHASE_DIST").ok().and_then(|d| d.parse().ok()).unwrap_or(chase.0);
            return Transform::from_translation(above_ground(g, at - flat * back + Vec3::Y * chase.1 * 0.3)).looking_at(at, Vec3::Y);
        }
        let eye = above_ground(g, at - flat * chase.0 + Vec3::Y * chase.1);
        Transform::from_translation(eye).looking_at(at + fwd * 20.0, Vec3::Y)
    } else {
        Transform::from_translation(at).looking_to(fwd, up)
    }
}

/// The third person's eye at least 1 m over the terrain, as the vehicles' (`update_chase`): on a slope it went under it and
/// showed the fog through the terrain.
/// No collision with the statics, as the original: turning in a narrow alley (GC_Mos_Eisley, 398/12/545) its third-person eye
/// also ends behind a wall and the screen is that wall (measured).
pub(super) fn above_ground(g: &Game, eye: Vec3) -> Vec3 {
    let h = g.session.sim.world.height_at(eye.x as f64, -eye.z as f64);
    Vec3::new(eye.x, h.map_or(eye.y, |h| eye.y.max(h as f32 + 1.0)), eye.z)
}

/// Every frame. On foot the view is not drawn between two ticks: it is the soldier's after
/// the last one plus the mouse gathered since (what the next tick will send), so it answers
/// the mouse at once; its position is the eyes' between the last two ticks. In a vehicle (its
/// turret turns at its own rate) or dead, both between the last two ticks.
#[allow(clippy::type_complexity)]
pub(super) fn camera(
    mut g: ResMut<Game>,
    mut q: Query<(&mut Transform, &mut Projection), (With<PlayCamera>, Without<FirstPerson>, Without<ArmsCamera>)>,
    mut arms: Query<(&mut Projection, &mut RenderLayers, &ArmsCamera), Without<PlayCamera>>,
    mut near: Query<(&mut Transform, &FirstPerson), Without<PlayCamera>>,
) {
    let Ok((mut t, mut proj)) = q.single_mut() else { return };
    camera_place(&g, &mut t);
    let z = zoom(&g);
    let p = &g.session.sim.players[g.player];
    let eased = p.soldier.filter(|_| p.alive).map(|i| {
        let target = (arms_target(&g, i), z.as_ref().map_or(1.0, |z| z.arms));
        g.arms_eased.map_or(target, |cur| arms_ease(cur, target))
    });
    g.arms_eased = eased;
    if let Projection::Perspective(p) = proj.as_mut() {
        let fov = z.as_ref().map(|z| z.fov).or_else(|| vehicle_fov(&g)).unwrap_or_else(crate::settings::world_fov);
        if p.fov != fov {
            p.fov = fov;
        }
    }
    if let Ok((mut proj, mut layers, a)) = arms.single_mut() {
        // behind a scope it draws nothing but the HUD (the UI goes with the last camera)
        let layer = RenderLayers::layer(if z.as_ref().is_some_and(|z| z.scope) { NOTHING_LAYER } else { FIRST_PERSON_LAYER });
        if *layers != layer {
            *layers = layer;
        }
        if let Projection::Perspective(p) = proj.as_mut() {
            let fov = a.fov * g.arms_eased.map_or(1.0, |e| e.1);
            if p.fov != fov {
                p.fov = fov;
            }
        }
    }
    for (mut n, fp) in near.iter_mut() {
        if fp.on {
            *n = *t * fp.local;
        }
    }
}

/// The scope's picture: made once (its texture from `menu/texture/<icon>`, as wide as a 4:3
/// screen as high as ours, black on both sides), shown while zoomed with a scope. (The `.dds` of
/// the same name when there is no `.tga`: DC's `m25_scope.tga` is `m25_scope.dds`.)
pub(super) fn scope(mut commands: Commands, g: Res<Game>, mut images: ResMut<Assets<Image>>, mut cache: ResMut<Cache>, mut q: Query<&mut Visibility, With<Scope>>, mut made: Local<bool>) {
    if g.is_added() {
        *made = false;
    }
    let z = zoom(&g);
    if !*made && z.as_ref().is_some_and(|z| z.scope) {
        *made = true;
        let p = &g.session.sim.players[g.player];
        let icon = p.soldier.and_then(|i| g.session.sim.vehicles[i].movement.as_ref()?.weapon.as_ref().map(|w| w.template.name.clone()));
        let file = icon.and_then(|w| g.session.sim.world.db.object_template(&w)?.get("setscopeicon")?.first().map(|f| f.trim_matches('"').to_string())).unwrap_or_else(|| "sniper.tga".into());
        let stem = file.rsplit_once('.').map_or(file.as_str(), |s| s.0);
        let Some(handle) = cache.texture(&g.session.sim.world.vfs, &format!("menu/Texture/{stem}"), &mut images) else {
            warn!("no scope picture {file}");
            return;
        };
        let black = || (Node { flex_grow: 1.0, height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK));
        commands
            .spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ZIndex(-1), Visibility::Hidden, Scope))
            .with_children(|c| {
                c.spawn(black());
                // drawn over a 4:3 screen in the original: its circle is round so stretched
                c.spawn((ImageNode { image: handle, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }, Node { height: Val::Percent(100.0), aspect_ratio: Some(4.0 / 3.0), ..default() }));
                c.spawn(black());
            });
        return;
    }
    let shown = if z.is_some_and(|z| z.scope) { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in q.iter_mut() {
        if *v != shown {
            *v = shown;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cockpit_bars(
    mut commands: Commands,
    g: Res<Game>,
    shown: Res<Shown>,
    mut q: Query<&mut Visibility, With<CockpitBars>>,
    mut bars: Query<(&mut Node, &CockpitBar)>,
    windows: Query<&Window>,
    eye: Query<(&Transform, &Projection), With<PlayCamera>>,
    shapes: Query<(&Mesh3d, &Transform), Without<PlayCamera>>,
    meshes: Res<Assets<Mesh>>,
    mut measure: Local<BarsMeasure>,
) {
    if g.is_added() {
        let black = |k| (Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK), CockpitBar(k));
        commands
            .spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::SpaceBetween, ..default() }, ZIndex(-1), Visibility::Hidden, CockpitBars))
            .with_children(|c| {
                c.spawn(black(0));
                c.spawn(black(1));
            });
        return;
    }
    let on = !shown.1.is_empty();
    let vis = if on { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in q.iter_mut() {
        if *v != vis {
            *v = vis;
        }
    }
    let (Some(w), Ok((t, Projection::Perspective(p)))) = (windows.iter().next(), eye.single()) else { return };
    if !on {
        measure.key.0.clear();
        return;
    }
    let aspect = w.width() / w.height().max(1.0);
    let key = (shown.1.clone(), aspect.to_bits(), p.fov.to_bits());
    if key != measure.key {
        let mut tris = Vec::new();
        for &e in &shown.1 {
            if let Ok((m, st)) = shapes.get(e)
                && let Some(mesh) = meshes.get(&m.0)
            {
                mesh_triangles(mesh, st, &mut tris);
            }
        }
        let limit = (4.0 / 3.0 / aspect).min(1.0);
        measure.edges = cockpit_edges(&tris, t, (p.fov * 0.5).tan(), aspect, limit);
        measure.key = key;
    }
    for (mut n, b) in bars.iter_mut() {
        let x = if b.0 == 0 { measure.edges[0] + 1.0 } else { 1.0 - measure.edges[1] };
        let width = Val::Percent(x * 50.0);
        if n.width != width {
            n.width = width;
        }
    }
}

pub(super) fn camera_place(g: &Game, t: &mut Transform) {
    if let Some(f) = g.fixed {
        *t = f;
        return;
    }
    let (a, b) = g.camera;
    let pending = if g.script.is_some() { Vec2::ZERO } else { g.mouse };
    if g.chase_yaw == 0.0
        && g.chase.is_none()
        && let Some((rows, pos)) = on_foot_view(g, pending)
    {
        *t = place(g, assets::to_bevy(pos), &rows, CHASE[0]);
        // between the last two ticks: back by what it moved in the last one
        t.translation -= (b.translation - a.translation) * (1.0 - g.alpha);
        return;
    }
    t.translation = a.translation.lerp(b.translation, g.alpha);
    t.rotation = a.rotation.slerp(b.rotation, g.alpha);
}
