//! A debug view of the navigation mesh over the world (Ctrl+F9, or `BYTEFIELD_NAVMESH_VIEW=1`): every polygon as a translucent
//! face a hand (0.15 m) above itself, coloured as `navmesh::view::picture` does (reached floors by their height over the terrain,
//! islands grey), the ladders (cyan), the drops (blue), the jumps (orange) and the narrow doors (white) as lines. The mesh is built or read from the cache in a thread the first
//! time the view is asked for; the whole map is one mesh with colours per vertex, and another for the lines.

use std::sync::{Arc, Mutex};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::play::Game;
use bytefield_sim::navmesh::{self, LinkKind, NavMesh};

/// How far over a polygon its drawing floats (m).
const LIFT: f32 = 0.15;
/// Faces' opacity.
const ALPHA: f32 = 0.45;

pub struct NavViewPlugin;

impl Plugin for NavViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NavView>().add_systems(Update, update.run_if(resource_exists::<Game>));
    }
}

#[derive(Resource, Default)]
struct NavView {
    shown: bool,
    /// asked for at least once (the env var or the key): the thread is on its way or done
    started: bool,
    env_checked: bool,
    pending: Option<Arc<Mutex<Option<Drawing>>>>,
    entities: Vec<Entity>,
}

/// The mesh as vertex data in Bevy's axes (z mirrored), faces and lines apart.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Drawing {
    pub face_pos: Vec<[f32; 3]>,
    pub face_col: Vec<[f32; 4]>,
    pub face_idx: Vec<u32>,
    pub line_pos: Vec<[f32; 3]>,
    pub line_col: Vec<[f32; 4]>,
}

/// An sRGB byte colour as the linear vertex colour Bevy blends with.
fn linear(c: [u8; 3], a: f32) -> [f32; 4] {
    let s = |v: u8| Color::srgb_u8(v, 0, 0).to_linear().red;
    [s(c[0]), s(c[1]), s(c[2]), a]
}

/// The mesh's faces and links as vertices and colours: `reached` as [`NavMesh::reach`] says, `ground(x, z)` the terrain's height.
pub fn drawing(mesh: &NavMesh, reached: &[bool], ground: &dyn Fn(f32, f32) -> f32) -> Drawing {
    let mut d = Drawing::default();
    for p in 0..mesh.polys.len() as u32 {
        let v: Vec<[f32; 3]> = mesh.poly_verts(p).collect();
        if v.len() < 3 {
            continue;
        }
        let c = mesh.centre(p);
        let on = reached.get(p as usize).copied().unwrap_or(false);
        let rgb = if on { navmesh::view::by_height(c[1] - ground(c[0], c[2])) } else { [130, 130, 130] };
        let col = linear(rgb, ALPHA);
        let first = d.face_pos.len() as u32;
        for q in &v {
            d.face_pos.push([q[0], q[1] + LIFT, -q[2]]);
            d.face_col.push(col);
        }
        // (mirrored z flips the winding: reversed here; the material is double sided anyway)
        for k in 1..v.len() as u32 - 1 {
            d.face_idx.extend([first, first + k + 1, first + k]);
        }
        // the outline, darker, so neighbouring polygons of one colour can be told apart
        let edge = linear(rgb.map(|v| v / 2), 1.0);
        for k in 0..v.len() {
            let (a, b) = (v[k], v[(k + 1) % v.len()]);
            d.line_pos.extend([[a[0], a[1] + LIFT, -a[2]], [b[0], b[1] + LIFT, -b[2]]]);
            d.line_col.extend([edge, edge]);
        }
    }
    for l in &mesh.links {
        let (pts, rgb) = match l.kind {
            LinkKind::Ladder => (vec![mesh.centre(l.a), l.from, l.to, mesh.centre(l.b)], [0, 230, 255]),
            LinkKind::Drop => (vec![l.from, l.to], [40, 90, 255]),
            LinkKind::Jump => (vec![l.from, l.to], [255, 140, 0]),
            LinkKind::Door => (vec![l.from, l.to], [255, 255, 255]),
            LinkKind::Exit => (vec![l.from, l.to], [120, 255, 120]),
        };
        let col = linear(rgb, 1.0);
        for w in pts.windows(2) {
            d.line_pos.extend(w.iter().map(|q| [q[0], q[1] + LIFT, -q[2]]));
            d.line_col.extend([col, col]);
        }
    }
    d
}

fn update(mut commands: Commands, g: Res<Game>, keys: Res<ButtonInput<KeyCode>>, mut v: ResMut<NavView>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    if !v.env_checked {
        v.env_checked = true;
        v.shown = std::env::var("BYTEFIELD_NAVMESH_VIEW").is_ok_and(|s| !s.is_empty() && s != "0");
    }
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) && keys.just_pressed(KeyCode::F9) {
        v.shown = !v.shown;
    }
    if v.shown && !v.started {
        v.started = true;
        let slot = Arc::new(Mutex::new(None));
        v.pending = Some(slot.clone());
        let world = g.session.sim.world;
        std::thread::spawn(move || {
            let mesh = world.navmesh();
            let reached = mesh.reach(&navmesh::seeds(world));
            let d = drawing(&mesh, &reached, &|x, z| world.height_at(x as f64, z as f64).unwrap_or(0.0) as f32);
            info!("navmesh view: {} polygons, {} links", mesh.polys.len(), mesh.links.len());
            *slot.lock().unwrap() = Some(d);
        });
    }
    if let Some(slot) = v.pending.clone() {
        if let Some(d) = slot.lock().unwrap().take() {
            v.pending = None;
            let face = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, d.face_pos)
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, d.face_col)
                .with_inserted_indices(Indices::U32(d.face_idx));
            let lines = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, d.line_pos)
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, d.line_col);
            let mat = |blend: bool| StandardMaterial { unlit: true, base_color: Color::WHITE, double_sided: true, cull_mode: None, alpha_mode: if blend { AlphaMode::Blend } else { AlphaMode::Opaque }, ..default() };
            let fm = materials.add(mat(true));
            let lm = materials.add(mat(false));
            let a = commands.spawn((Mesh3d(meshes.add(face)), MeshMaterial3d(fm), Transform::default(), Visibility::Hidden)).id();
            let b = commands.spawn((Mesh3d(meshes.add(lines)), MeshMaterial3d(lm), Transform::default(), Visibility::Hidden)).id();
            v.entities = vec![a, b];
        }
    }
    let vis = if v.shown { Visibility::Inherited } else { Visibility::Hidden };
    for e in v.entities.clone() {
        commands.entity(e).insert(vis);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytefield_sim::navmesh::{Link, Poly};

    fn square() -> NavMesh {
        let mut m = NavMesh { side: 1, ..Default::default() };
        m.verts = vec![[0.0, 1.0, 0.0], [2.0, 1.0, 0.0], [2.0, 1.0, 2.0], [0.0, 1.0, 2.0], [5.0, 4.0, 5.0], [6.0, 4.0, 5.0], [6.0, 4.0, 6.0]];
        m.idx = vec![0, 1, 2, 3, 4, 5, 6];
        m.polys = vec![Poly { first: 0, n: 4, ..Default::default() }, Poly { first: 4, n: 3, ..Default::default() }];
        m.nei = vec![u32::MAX; 7];
        m.links = vec![Link { a: 0, b: 1, from: [2.0, 1.0, 2.0], to: [5.0, 1.0, 5.0], kind: LinkKind::Drop }];
        m
    }

    /// A quad becomes two faces and a triangle one, lifted a hand and in Bevy's axes; the unreached one is grey, the drop a line.
    #[test]
    fn a_mesh_becomes_faces_and_lines() {
        let d = drawing(&square(), &[true, false], &|_, _| 0.0);
        assert_eq!(d.face_pos.len(), 7);
        assert_eq!(d.face_idx.len(), 3 * 2 + 3);
        assert!(d.face_idx.iter().all(|&i| (i as usize) < 7));
        assert!((d.face_pos[0][1] - 1.15).abs() < 1e-6 && d.face_pos[2][2] == -2.0);
        assert_eq!(d.face_col[4][0], d.face_col[4][1], "grey");
        assert_ne!(d.face_col[0][0], d.face_col[0][1], "green on the ground, not grey");
        assert_eq!(d.line_pos.len(), 2 * (4 + 3 + 1));
        assert_eq!(d.line_pos.len(), d.line_col.len());
    }
}
