//! Skinned meshes (`GeometryTemplate.setSkin`): a soldier's body, head and hands follow its
//! skeleton as the simulation poses it ([`bytefield_sim::soldier::Movement::posed_skeleton`], the server's bones).
//!
//! A `.skn` gives each vertex its place in the frame of the bones it hangs from, with weights;
//! the place in the soldier's frame is Σ weight · (offset · the bone's absolute
//! matrix), which already holds the skeleton's root turn and `setCharacterHeight`. The `.sm`
//! splits vertices by material (740 for `GerBody`'s 534): each finds its skin vertex by its
//! bind position (the same, or 1 ulp off). Skinned on the CPU once per tick and drawn
//! between the last two ticks' vertices (`play`); the normals come from the triangles.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use bytefield_formats::anim::{Skeleton as SkeletonFile, Skin};
use bytefield_formats::sm;
use bytefield_sim::skeleton::M4;
use bytefield_vfs::Vfs;

/// One material of a skinned mesh: each vertex's bones (in the skeleton), weights and offsets
/// (none: it stays at `rest`), and the triangles (already in Bevy's winding).
pub struct MaterialSkin {
    pub influences: Vec<Vec<(usize, f32, [f32; 3])>>,
    pub rest: Vec<[f32; 3]>,
    /// the mesh's own normals, which the bones do not turn: what soldiers are lit with ([`write_arms`]). `x` negated and `z` kept, chosen
    /// among the four signs against the original (only `z` kept lights a soldier's front, as
    /// the original does; the x by the arms of weakly). Not derived from the shader. The first-person arms turn them by half
    /// a turn, derived.
    pub normals: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// A mesh's materials skinned against a skeleton, in the mesh's material order; None if the
/// files do not load or a vertex finds no skin vertex. With `partial`, a vertex without one
/// stays where the mesh has it (a track's upper run: its skin covers the lower one only).
pub fn load(vfs: &Vfs, mesh_file: &str, skin_file: &str, skeleton: &SkeletonFile, partial: bool) -> Option<Vec<Arc<MaterialSkin>>> {
    let mesh = sm::parse(&vfs.read(&format!("standardmesh/{mesh_file}.sm")).ok()?).ok()?;
    let skin = Skin::parse(&vfs.read(skin_file).ok()?).ok()?;
    fit(&mesh, &skin, skeleton, partial, NEAR).or_else(|| (!partial).then(|| fit(&mesh, &skin, skeleton, false, LOOSE)).flatten())
}

/// How far (the largest axis) a mesh vertex may be from the skin's own for the skin to fit: the same bits, or (a few, by 1 ulp) the nearest.
const NEAR: f32 = 1e-4;
/// A skin made for another mesh: GC's `usbody.skn`, from a body some 6 cm taller than its own `usbody.sm` (3–80 mm off, not one vertex
/// the same), which the original draws walking, crouching and lying (GC_Hoth in the original). Level 3: each vertex takes the nearest skin
/// vertex's bones, its offsets moved by the difference ([`bind_frames`]). How the original pairs them is not read.
const LOOSE: f32 = 0.1;

fn fit(mesh: &sm::StandardMesh, skin: &Skin, skeleton: &SkeletonFile, partial: bool, tolerance: f32) -> Option<Vec<Arc<MaterialSkin>>> {
    let bones: Vec<Option<usize>> = skin.bones.iter().map(|n| skeleton.index(n)).collect();
    let frames = if tolerance > NEAR { bind_frames(skin) } else { Vec::new() };
    let key = |p: &[f32; 3]| p.map(f32::to_bits);
    let by_pos: HashMap<[u32; 3], usize> = skin.vertices.iter().enumerate().map(|(i, (p, _))| (key(p), i)).collect();
    // TODO: the original changes a skinned mesh's LOD with the distance: the Type 38's seated soldier, some 15 m off in the chase view,
    // was `BritBody`'s lod 2 (draws of 138, 108 and 38 vertices, its three materials). Which distances, unread.
    let lod = mesh.lods.first()?;
    lod.materials
        .iter()
        .map(|m| {
            let influences = m
                .positions
                .iter()
                .map(|p| {
                    // the same bits, or (a few, by 1 ulp) the nearest
                    let near = || {
                        let d = |q: &[f32; 3]| (0..3).map(|k| (q[k] - p[k]).abs()).fold(0.0, f32::max);
                        skin.vertices.iter().enumerate().map(|(i, (q, _))| (i, d(q))).min_by(|a, b| a.1.total_cmp(&b.1)).filter(|x| x.1 < tolerance).map(|x| x.0)
                    };
                    let Some(j) = by_pos.get(&key(p)).copied().or_else(near) else {
                        return partial.then(Vec::new);
                    };
                    let (q, inf) = &skin.vertices[j];
                    let delta = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
                    // a bone with no weight moves nothing: `GerEliteBody.skn` names `Root`, which the skeleton lacks, with weight 0
                    inf.iter()
                        .filter(|i| i.weight != 0.0)
                        .map(|i| {
                            let o = match frames.get(i.bone as usize).copied().flatten() {
                                Some(inv) => std::array::from_fn(|r| i.offset[r] + (0..3).map(|c| inv[r][c] * delta[c]).sum::<f32>()),
                                None => i.offset,
                            };
                            Some((bones[i.bone as usize]?, i.weight, o))
                        })
                        .collect::<Option<Vec<_>>>()
                })
                .collect::<Option<Vec<_>>>()?;
            let triangles = m.triangles.iter().map(|t| [t[0] as u32, t[2] as u32, t[1] as u32]).collect();
            Some(Arc::new(MaterialSkin { influences, rest: m.positions.clone(), normals: m.normals.iter().map(|n| [-n[0], n[1], n[2]]).collect(), triangles }))
        })
        .collect()
}

/// Each bone's pose for its skin (the inverse of the linear part of `q = A o + t`), by least squares over the skin vertices that
/// follow that bone alone: with it a mesh vertex `p` given the bones of a skin vertex `q` keeps its own place (`o + A⁻¹ (p − q)`
/// for every bone, as the weights add to 1). None for a bone with fewer than 4 such vertices or flat ones.
fn bind_frames(skin: &Skin) -> Vec<Option<[[f32; 3]; 3]>> {
    (0..skin.bones.len())
        .map(|b| {
            // normal equations for [A | t] from o (with a 1) to q, in f64
            let mut ata = [[0f64; 4]; 4];
            let mut atq = [[0f64; 3]; 4];
            let mut n = 0;
            for (q, inf) in &skin.vertices {
                let mut live = inf.iter().filter(|i| i.weight != 0.0);
                let (Some(i), None) = (live.next(), live.next()) else { continue };
                if i.bone as usize != b {
                    continue;
                }
                n += 1;
                let x = [i.offset[0] as f64, i.offset[1] as f64, i.offset[2] as f64, 1.0];
                for r in 0..4 {
                    for c in 0..4 {
                        ata[r][c] += x[r] * x[c];
                    }
                    for c in 0..3 {
                        atq[r][c] += x[r] * q[c] as f64;
                    }
                }
            }
            if n < 4 {
                return None;
            }
            let m = solve4(ata, atq)?;
            // A's rows: q[c] = sum_r m[r][c] x[r], so A[c][r] = m[r][c]
            let a: [[f64; 3]; 3] = std::array::from_fn(|c| std::array::from_fn(|r| m[r][c]));
            invert3(a).map(|inv| inv.map(|row| row.map(|v| v as f32)))
        })
        .collect()
}

/// `ata x = atq` by Gauss–Jordan with pivoting; None if singular.
fn solve4(mut a: [[f64; 4]; 4], mut b: [[f64; 3]; 4]) -> Option<[[f64; 3]; 4]> {
    for col in 0..4 {
        let piv = (col..4).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        for r in 0..4 {
            if r != col {
                let f = a[r][col] / a[col][col];
                for c in 0..4 {
                    a[r][c] -= f * a[col][c];
                }
                for c in 0..3 {
                    b[r][c] -= f * b[col][c];
                }
            }
        }
    }
    Some(std::array::from_fn(|r| b[r].map(|v| v / a[r][r])))
}

fn invert3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let c = |r: usize, k: usize| m[(r + 1) % 3][(k + 1) % 3] * m[(r + 2) % 3][(k + 2) % 3] - m[(r + 1) % 3][(k + 2) % 3] * m[(r + 2) % 3][(k + 1) % 3];
    let det = (0..3).map(|k| m[0][k] * c(0, k)).sum::<f64>();
    (det.abs() > 1e-9).then(|| std::array::from_fn(|r| std::array::from_fn(|k| c(k, r) / det)))
}

/// The parachute on its own skeleton (`createSkeleton animations/Parachute.ske`): from when it opens, `OpenParachute`
/// once at its speed and then `IdleParachute` in a loop, morphing in by its `setMorphFactor`. Unlike a soldier's, its
/// bones have no root turn: its bind pose is already y up.
pub struct Chute {
    skeleton: bytefield_sim::skeleton::Skeleton,
    /// seconds open
    time: f32,
}

impl Chute {
    pub fn new(file: Arc<SkeletonFile>) -> Chute {
        Chute { skeleton: bytefield_sim::skeleton::Skeleton::new(file, Vec::new(), 0.0), time: 0.0 }
    }

    /// Folded again, for the next time it opens.
    pub fn close(&mut self) {
        self.time = 0.0;
    }

    /// The bones' absolute matrices now, and `dt` seconds further for the next call.
    pub fn advance(&mut self, m: &bytefield_sim::anim::Machine, dt: f32) -> &[M4] {
        let sk = &mut self.skeleton;
        sk.reset();
        if let (Some(open), Some(idle)) = (m.find("OpenParachute"), m.find("IdleParachute")) {
            let speed = |i: usize| m.states[i].anim().map_or(1.0, |a| a.0);
            let opening = self.time * speed(open);
            if let Some(a) = m.animation(open) {
                // (at exactly 1 it would wrap to its first frame: past it, it stays on the last)
                sk.apply(a, if opening < 1.0 { opening } else { 2.0 }, 1.0, false);
            }
            if opening >= 1.0
                && let Some(a) = m.animation(idle)
            {
                let since = self.time - 1.0 / speed(open);
                sk.apply(a, since * speed(idle), (since * m.states[idle].morph).min(1.0), true);
            }
        }
        chain(sk);
        self.time += dt;
        &sk.abs
    }
}

/// The bones' absolute matrices from their relative ones, without a soldier's root turn.
fn chain(sk: &mut bytefield_sim::skeleton::Skeleton) {
    for i in 0..sk.rel.len() {
        sk.abs[i] = match sk.file.bones[i].parent {
            Some(p) => bytefield_sim::skeleton::mult(&sk.rel[i], &sk.abs[p]),
            None => sk.rel[i],
        };
    }
}

/// An object on its own skeleton playing a state of the machine (a flag: `setAnimationState FlagBlow`), from its own
/// moment (`setUserRandomStartTime`: not all flags wave alike), through the same `AnimationStateMachineInstance` as a
/// soldier's with no input: when its idle timer
/// (4–7 s) has run out, a state that returns to itself breaks into one of its `addIdle` (FlagIdle1/2) at random, which
/// plays once and goes back (`addTransitionWhenDone`), morphing in by its `setMorphFactor`. Level 2: our `rand`.
pub struct Looping {
    skeleton: bytefield_sim::skeleton::Skeleton,
    machine: Arc<bytefield_sim::anim::Machine>,
    instance: bytefield_sim::anim::Instance,
}

impl Looping {
    pub fn new(file: Arc<SkeletonFile>, machine: Arc<bytefield_sim::anim::Machine>, state: &str, start: f32) -> Option<Looping> {
        let state = machine.find(state)?;
        machine.animation(state)?;
        let mut instance = bytefield_sim::anim::Instance::new(Some(state));
        // (its own moment: the first loop's time, and its own dice for the idles)
        let speed = machine.states[state].anim().map_or(1.0, |a| a.0);
        instance.start = (start * speed).rem_euclid(1.0).max(f32::MIN_POSITIVE);
        instance.rng = u64::from(start.to_bits()) | 1;
        let mut skeleton = bytefield_sim::skeleton::Skeleton::new(file, Vec::new(), 0.0);
        skeleton.reset();
        Some(Looping { skeleton, machine, instance })
    }

    /// The bones' absolute matrices, `dt` seconds further.
    pub fn advance(&mut self, dt: f32) -> &[M4] {
        let (m, inst) = (&*self.machine, &mut self.instance);
        let first = inst.start > 0.0;
        inst.check(m, &bytefield_sim::anim::Input::default());
        inst.update(m, dt, "");
        // (only the first loop starts at its own moment, and already waving: no morph from the bind pose)
        if first {
            (inst.start, inst.morph) = (0.0, 1.0);
        }
        let sk = &mut self.skeleton;
        // (the state's animation over the pose the bones had, by its morph)
        if let (Some(i), Some((_, looping))) = (inst.state, inst.anim)
            && let Some(a) = m.animation(i)
        {
            sk.apply(a, inst.time, inst.morph, looping);
        }
        chain(sk);
        &sk.abs
    }

    /// The state it plays now.
    #[cfg(test)]
    pub fn state(&self) -> Option<usize> {
        self.instance.state
    }
}

/// The vertices of a material posed by these bones' absolute matrices (row vectors,
/// translation in the last row), mirrored to Bevy's frame.
pub fn positions(skin: &MaterialSkin, abs: &[M4]) -> Vec<[f32; 3]> {
    skin.influences
        .iter()
        .zip(&skin.rest)
        .map(|(inf, rest)| {
            if inf.is_empty() {
                return [rest[0], rest[1], -rest[2]];
            }
            let mut q = [0.0f32; 3];
            for &(b, w, o) in inf {
                let m = &abs[b];
                for (k, v) in q.iter_mut().enumerate() {
                    *v += w * (o[0] * m[k] + o[1] * m[4 + k] + o[2] * m[8 + k] + m[12 + k]);
                }
            }
            [q[0], q[1], -q[2]]
        })
        .collect()
}

/// A material's vertices into its mesh, with the normals of its triangles.
/// TODO: which meshes keep GC_Mini_Hoth at 20 fps? `allocate_and_free_meshes` takes 33 ms a frame there. Out of the performance goal.
pub fn write(skin: &MaterialSkin, pos: Vec<[f32; 3]>, mesh: &mut Mesh) {
    let mut nor = vec![Vec3::ZERO; pos.len()];
    for t in &skin.triangles {
        let [a, b, c] = t.map(|i| Vec3::from(pos[i as usize]));
        let n = (b - a).cross(c - a);
        for &i in t {
            nor[i as usize] += n;
        }
    }
    let nor: Vec<[f32; 3]> = nor.iter().map(|n| n.normalize_or(Vec3::Y).to_array()).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
}

/// The first-person arms' vertices, lit as the original's skinning shader lights them (`SkinningShader2Bones.vso`:
/// `dp3 r1.x, v3, -c6`, the mesh's own normal, which the bones do not turn, against the sun in the soldier's space: `c6` turns
/// with his yaw). So the normal follows the yaw but not the pitch of `turn` (the entity's rotation).
/// `body`: the yaw that lights them, if not `turn`'s. The first-person arms hang from the camera, but `c6` follows the body, which
/// stays behind the view until the head's turn reaches `setTurnLeftRightAngle` (42° one way, 60° the other).
pub fn write_arms(skin: &MaterialSkin, pos: Vec<[f32; 3]>, turn: Quat, body: Option<Quat>, seated: bool, mesh: &mut Mesh) {
    let yaw = Quat::from_rotation_y(body.unwrap_or(turn).to_euler(EulerRot::YXZ).0);
    let mut back = turn.inverse() * yaw;
    // The arms: `dp3 v3, -c6` with the file's normal and `c6` the sun in the body's frame, which our light in the yawed frame
    // already is (Husky in shade: `yaw⁻¹ · sun` = to_bevy(−c6) = (−0.3675, 0.432, −0.8235)). So the
    // normal is the file's mirrored to Bevy as a static mesh's, (x, y, −z): the half turn of the stored (−x, y, z). The right hand
    // then fits (72.8 against the original's 73.2 of red), the left 142 against 126.
    // The outside body too: its 289-vertex draws take the same `c6` from outside as the arms in 1P, (0.755,
    // −0.463, −0.463) in F9, F10 and F11 alike, so there is no half turn of its own.
    // Before, (−x, y, z) chosen by eye.
    back *= Quat::from_rotation_y(std::f32::consts::PI);
    // A soldier in a seat: the normal the other way. Worked out from the shader, `dp3 v3, −c6` with `c6` the sun in the soldier's
    // frame and the static mesh's (x, y, −z) as the file-to-Bevy turn, the normal to light with is R·(−x, −y, z) of the file's, the
    // negative of the one above. Measured on the Type 38 seen from behind (the khaki above and below the belt: 121 and 132 of red
    // against the original's 128 and 137; before, 111 and 110) and on the Willys (the driver's back, khaki as in the original,
    // olive before).
    // On foot the same derivation does not hold: against the original from outside (an M16 soldier in El_Alamein, front and back)
    // the negated normal darkens the front further, and of six sign patterns the one below comes closest. The front stays darker
    // than the original's even so, which is the amount of light, not the normal's sign.
    let sign = if seated { -1.0 } else { 1.0 };
    let nor: Vec<[f32; 3]> = skin.normals.iter().map(|n| (back * (Vec3::from(*n) * sign)).to_array()).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
}

/// The vertices of the skinned meshes this frame, packed as their buffer holds them ([`Mesh::create_packed_vertex_buffer_data`]),
/// written straight over the mesh's place in Bevy's buffers. Changing the `Mesh` asset instead makes Bevy free it, place it
/// again and prepare it, and everything that draws it, again (`allocate_and_free_meshes`, 3,7 ms a frame in
/// Battle_of_the_Bulge with 64 bots). The asset in the main world still gets the vertices, without telling
/// anyone: the object shadows read them from there.
#[derive(Resource, Default)]
pub struct SkinQueue(std::sync::Mutex<Vec<(AssetId<Mesh>, Vec<u8>)>>);

impl SkinQueue {
    pub fn push(&self, mesh: AssetId<Mesh>, bytes: Vec<u8>) {
        self.0.lock().unwrap().push((mesh, bytes));
    }
}

/// On the render side: what is still to be written, and for how many frames it has waited.
#[derive(Resource, Default)]
struct SkinPending(Vec<(AssetId<Mesh>, Vec<u8>, u32)>);

/// How long a mesh's vertices wait for it to be placed in the buffers (it was added this frame) or for ever (it is gone).
const SKIN_WAIT: u32 = 120;

fn extract_skins(queue: bevy::render::Extract<Res<SkinQueue>>, mut pending: ResMut<SkinPending>) {
    for (mesh, bytes) in queue.0.lock().unwrap().drain(..) {
        // (the last for each one)
        pending.0.retain(|p| p.0 != mesh);
        pending.0.push((mesh, bytes, 0));
    }
}

fn write_skins(mut pending: ResMut<SkinPending>, allocator: Res<bevy::render::mesh::allocator::MeshAllocator>, queue: Res<bevy::render::renderer::RenderQueue>) {
    pending.0.retain_mut(|(mesh, bytes, waited)| {
        let Some(place) = allocator.mesh_vertex_slice(mesh) else {
            *waited += 1;
            return *waited < SKIN_WAIT;
        };
        let count = place.range.len() as u64;
        // (another size of vertex, or another count, would be another mesh: it does not happen, it must not write over its neighbours)
        if count > 0 && (bytes.len() as u64).is_multiple_of(count) && (bytes.len() as u64).is_multiple_of(4) {
            let size = bytes.len() as u64 / count;
            if (place.range.start as u64 * size).is_multiple_of(4) {
                queue.write_buffer(place.buffer, place.range.start as u64 * size, bytes);
            }
        }
        false
    });
}

/// Sends the skinned meshes' vertices to the GPU ([`SkinQueue`]).
pub struct SkinPlugin;

impl Plugin for SkinPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SkinQueue>();
        if let Some(r) = app.get_sub_app_mut(bevy::render::RenderApp) {
            r.init_resource::<SkinPending>().add_systems(bevy::render::ExtractSchedule, extract_skins).add_systems(bevy::render::Render, write_skins.in_set(bevy::render::RenderSystems::PrepareResources));
        }
    }
}

/// [`positions`] into the mesh.
#[cfg(test)]
pub fn pose(skin: &MaterialSkin, abs: &[M4], mesh: &mut Mesh) {
    write(skin, positions(skin, abs), mesh);
}

#[cfg(test)]
mod tests {
    /// `GerEliteBody.skn` names `Root`, which the skeleton lacks, with weight 0: rejecting it left the body lying on
    /// the ground, in its rest pose, with the head, the hands and the weapon in the air (Raid_on_Agheila).
    #[test]
    fn a_bone_with_no_weight_does_not_topple_the_skin() {
        let game = bytefield_testdata::require!(Base, Xpack2);
        let w = bytefield_sim::World::load(&game, "XPack2", "Raid_on_Agheila", "conquest").expect("level");
        let ske = bytefield_formats::anim::Skeleton::parse(&w.vfs.read("animations/UsSoldier.ske").unwrap()).unwrap();
        assert!(ske.index("Root").is_none(), "if the skeleton has it already, this test proves nothing");
        let skin = super::load(&w.vfs, "GerEliteBody", "animations/GerEliteBody.skn", &ske, false).expect("fits");
        assert!(skin.iter().all(|m| m.influences.iter().all(|v| !v.is_empty())), "every vertex follows the skeleton");
    }

    /// GC's `usbody.skn` was made for a taller body than its `usbody.sm`: no vertex the same, and the soldier stood in his rest pose,
    /// headless, never crouching or lying (GC_Hoth). Taken loosely, every vertex follows the skeleton, and the bones' poses
    /// read from the skin put a vertex with several bones where the mesh has it.
    #[test]
    fn a_skin_for_another_body_still_follows_the_skeleton() {
        let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
        let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").expect("level");
        let ske = bytefield_formats::anim::Skeleton::parse(&w.vfs.read("animations/UsSoldier.ske").unwrap()).unwrap();
        let mesh = bytefield_formats::sm::parse(&w.vfs.read("standardmesh/UsBody.sm").unwrap()).unwrap();
        let skn = bytefield_formats::anim::Skin::parse(&w.vfs.read("animations/USbody.skn").unwrap()).unwrap();
        assert!(super::fit(&mesh, &skn, &ske, false, super::NEAR).is_none(), "if it fits strictly, this test proves nothing");
        let skin = super::load(&w.vfs, "UsBody", "animations/USbody.skn", &ske, false).expect("fits loosely");
        assert!(skin.iter().all(|m| m.influences.iter().all(|v| !v.is_empty())), "every vertex follows the skeleton");
        // the skin's own vertices, posed by its bones' fitted poses, come back where they are
        let frames = super::bind_frames(&skn);
        assert!(frames.iter().filter(|f| f.is_some()).count() >= 10, "most bones get a pose");
        let mut worst = 0f32;
        for (q, inf) in &skn.vertices {
            let live: Vec<_> = inf.iter().filter(|i| i.weight != 0.0).collect();
            if live.len() != 1 {
                continue;
            }
            let Some(inv) = frames[live[0].bone as usize] else { continue };
            // A⁻¹ q − A⁻¹ t = o for every vertex of that bone: compare two of them through A⁻¹ (q₁ − q₂) = o₁ − o₂
            let (q2, o2) = skn.vertices.iter().find_map(|(q2, i2)| (i2.len() == 1 && i2[0].bone == live[0].bone).then_some((q2, i2[0].offset))).unwrap();
            for r in 0..3 {
                let lhs: f32 = (0..3).map(|c| inv[r][c] * (q[c] - q2[c])).sum();
                worst = worst.max((lhs - (live[0].offset[r] - o2[r])).abs());
            }
        }
        assert!(worst < 1e-3, "the bones' poses fit their vertices ({worst} m)");
    }
}
