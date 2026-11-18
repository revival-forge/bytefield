//! Drawing helpers for what moves: wheels and springs, soldier kit parts, shading, scrolling textures and
//! the interpolation between two ticks.

use super::*;

pub(super) fn wheel(meshes: &[&Mesh]) -> Option<Wheel> {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for m in meshes {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(ps)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        for p in ps {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
    }
    let radius = (hi[1] - lo[1]) / 2.0;
    // the meshes are in Bevy's frame (z the other way)
    (radius > 0.05).then(|| Wheel { radius, axle: [0.0, ((lo[1] + hi[1]) / 2.0) as f64, -((lo[2] + hi[2]) / 2.0) as f64] })
}

/// A wheel turned by `angle` about its axle (x), before its placement. Level 3: the
/// server never turns them, so it is the client's; we roll them by the vehicle's forward speed.
pub(super) fn spun(local: &Mat4, w: &Wheel, angle: f32) -> Mat4 {
    let a = w.axle;
    let to = Mat4::from_pos_ypr([-a[0], -a[1], -a[2]], [0.0; 3]);
    let back = Mat4::from_pos_ypr(a, [0.0; 3]);
    // `a.mul(b)` puts `b` first: to the axle, turn, back, then the placement
    local.mul(&back).mul(&Mat4::from_pos_ypr([0.0; 3], [0.0, angle.to_degrees() as f64, 0.0])).mul(&to)
}

/// Where a wheel on a spring is from its rest, in the world: the collision moved it there
/// in the last tick (the spring keeps minus that, `d_prev`, and puts it back the next one).
pub(super) fn spring_offset(v: &bytefield_sim::mover::Vehicle, name: &str, instance: usize) -> Option<[f32; 3]> {
    let c = v.contacts.iter().filter(|c| c.name.eq_ignore_ascii_case(name)).nth(instance)?;
    let d = c.spring.as_ref()?.d_prev;
    Some([-d[0], -d[1], -d[2]])
}

/// A kit template's `KitPart` children.
pub(super) fn kit_part_list(db: &bytefield_con::templates::TemplateDb, kit: &str) -> Vec<KitPart> {
    let Some(t) = db.object_template(kit) else { return Vec::new() };
    t.all("addtemplate")
        .into_iter()
        .filter_map(|a| a.first().cloned())
        .filter_map(|n| db.object_template(&n))
        .filter(|c| c.kind.eq_ignore_ascii_case("kitpart"))
        .filter_map(|c| {
            let geo = db.get("geometrytemplate", c.get("geometry")?.first()?)?;
            let file = geo.get("file")?.first()?.replace('\\', "/");
            let bone = c.get("setbonename").and_then(|a| a.first()).cloned()?;
            Some(KitPart { kind: geo.kind.to_ascii_lowercase(), file, bone })
        })
        .collect()
}

/// After a tick: each vehicle's, soldier's and kit's TEXTUREFACTOR in the terrain's baked shadow, as the client sets it: the object's intensity
/// (`object_intensity`, the vote round it) faded over `ShadowBorderFadeTime`, on each of its meshes whose box fits in
/// `ShadowStaticObjectsCullBox` (the others stay lit); `ShadowAmbient` + intensity, to 255. Level 2: a reference run in Husky
/// has every piece of a Kübelwagen with the same TF, so here the whole object takes its root's; the original
/// works it out for each node at its own place (it goes down the children), and we are 5–7 below on its two
/// Kübelwagens (0x9f, 0xa0) and within 3 on the furniture of a house.
// TODO: a soldier in a seat takes the vehicle's place (the original, its own: 1351.7/42.9/1429.0 in a Kübelwagen at
// 1352.0/42.6/1428.9, in a reference run). The hand weapon in his own eyes stays at TF 0xff, as in the original's
// reference run on foot, but its picture there is dark in shade: by what (lit0, MODULATE: vertex colours?) unseen.
pub(super) fn shade_objects(commands: &mut Commands, g: &Game, shown: &Shown, in_hand: &InHand, shade: &mut Shade) {
    // (turned off: back to lit; the statics' wait for the next level)
    if !assets::graphics().mesh_shade {
        for (e, _) in shade.tags.drain() {
            if let Ok(mut c) = commands.get_entity(e) {
                c.insert(bevy::mesh::MeshTag { value: 0 });
            }
        }
        return;
    }
    let sim = &g.session.sim;
    let world = sim.world;
    let Some(s) = shade.sampler.get_or_insert_with(|| bytefield_world::shadow::ShadowSampler::new(&world.vfs, &world.db, &world.level)) else { return };
    let mut wanted: HashMap<Entity, u32> = HashMap::new();
    for (&i, (_, shapes)) in &shown.0 {
        let Some(v) = sim.vehicles.get(i) else { continue };
        let at = v.entry.inside.and_then(|j| sim.vehicles.get(j)).map_or(v.craft.body.pos, |c| c.craft.body.pos);
        let root = shapes.iter().find(|x| x.part.is_none() && !x.first_person).and_then(|x| x.file.as_deref()).and_then(|f| shade.boxes.get(&world.vfs, f));
        let now = s.object_intensity(&world.level, at, root.map_or(1.0, bytefield_world::shadow::radius));
        let was = shade.intensity.get(&i).copied();
        let k = was.map_or(now, |w| s.fade(w, now, TICK as f32));
        shade.intensity.insert(i, k);
        let tag = level::shade_tag(s.mesh_factor(k)).value;
        let skin_tag = level::shade_tag(s.skin_factor(k)).value;
        // (a skinned one above ShadowSamplingCullY takes no factor: [`ShadowSampler::samples`])
        let sampled = s.samples(&world.level, at);
        // (the skinned ones, a soldier's arms in his own eyes too, take less: `skin_factor`; the hand weapon in
        // his eyes stays at 0xff in the reference run, as here; the skinned ones take it twice, in the TF and in the sun's `c7`:
        // `envmap.wesl`)
        for x in shapes.iter() {
            let skinned = x.skin.is_some() || x.track.is_some();
            if x.first_person && !skinned {
                continue;
            }
            let fits = x.file.as_deref().and_then(|f| shade.boxes.get(&world.vfs, f)).map_or(skinned && sampled, |b| s.takes_shadow(b));
            // (the arms too: TF 0xf0 in Husky's sun with ShadowAmbient 100, 0x8d in shade;
            // 0xff for a soldier the shadow did not reach)
            wanted.insert(x.entity, if !fits { 0 } else if skinned { skin_tag } else { tag });
        }
        for (e, _) in in_hand.1.get(&i).map_or(&[][..], |k| &k.1[..]) {
            wanted.insert(*e, tag);
        }
    }
    shade.intensity.retain(|i, _| shown.0.contains_key(i));
    for (&e, &tag) in &wanted {
        if shade.tags.get(&e).copied().unwrap_or(0) != tag
            && let Ok(mut c) = commands.get_entity(e)
        {
            c.insert(bevy::mesh::MeshTag { value: tag });
        }
    }
    shade.tags = wanted;
}

/// After a tick ([`sync`] has posed them): each soldier's kit parts (`Kit` → `KitPart`: the helmet on bone `A`, the packs
/// on `backpack` and `HipPack`), each mesh in its bone's frame as the skin's. Not from the player's own eyes. Level 2
/// (the data's; compared with the original's driver).
// TODO: `setCopyLinksCount` (0 in all the data) and the parts' LOD (`setLodDistance 1 30`: a simpler mesh far off).
#[allow(clippy::too_many_arguments)]
pub(super) fn kit_parts(commands: &mut Commands, g: &Game, shown: &Shown, in_hand: &mut InHand, visuals: &mut Visuals, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, cache: &mut Cache) {
    let sim = &g.session.sim;
    let world = sim.world;
    for (i, v) in sim.vehicles.iter().enumerate() {
        let pose = shown.4.get(&i);
        let kit = pose.and_then(|_| v.movement.as_ref()?.kit).and_then(|k| sim.items.get(k)).map(|it| it.template.clone());
        if let Some((had, es)) = in_hand.1.get(&i)
            && Some(had) != kit.as_ref()
        {
            for (e, _) in es {
                commands.entity(*e).despawn();
            }
            in_hand.1.remove(&i);
        }
        let (Some(pose), Some(kit)) = (pose, kit) else { continue };
        let parts = in_hand.2.entry(kit.to_ascii_lowercase()).or_insert_with(|| Arc::new(kit_part_list(&world.db, &kit))).clone();
        let bone_of = |name: &str| pose.file.index(name).or_else(|| pose.file.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name)));
        // (a part is modelled as a hand weapon is in its bone: turned half a turn about y, `weapon::MESH`)
        let at = |bone: usize| assets::to_transform(&pose.frame.mul(&scene_matrix(&bytefield_sim::skeleton::mult(&weapon::MESH, &pose.abs[bone]))));
        let seen = if pose.own_eyes { Visibility::Hidden } else { Visibility::Inherited };
        let es = in_hand.1.entry(i).or_insert_with(|| {
            let mut es = Vec::new();
            for p in parts.iter() {
                let Some(bone) = bone_of(&p.bone) else { continue };
                let t = at(bone);
                for (mesh, mat) in cache.get(&world.vfs, &p.kind, &p.file, meshes, materials, images) {
                    let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, seen, Visual { prev: t, cur: t }, level::CastsShadow, crate::shadows::Caster(i)));
                    es.push((e.id(), bone));
                }
            }
            (kit.clone(), es)
        });
        for (e, bone) in &es.1 {
            if let Ok((mut vis, mut shown_now, _, _)) = visuals.get_mut(*e) {
                vis.prev = vis.cur;
                vis.cur = at(*bone);
                *shown_now = seen;
            }
        }
    }
    in_hand.1.retain(|&i, es| {
        let keep = shown.4.contains_key(&i);
        if !keep {
            for (e, _) in &es.1 {
                commands.entity(*e).despawn();
            }
        }
        keep
    });
}

/// Every frame: the running textures (`Scroll`) move on at the rate of the last tick.
pub(super) fn scroll_textures(time: Res<Time>, mut shown: ResMut<Shown>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let dt = time.delta_secs();
    for sc in shown.0.values_mut().flat_map(|(_, es)| es.iter_mut()).filter_map(|s| s.scroll.as_mut()) {
        if sc.rate == [0.0; 2] {
            continue;
        }
        for k in 0..2 {
            sc.offset[k] = (sc.offset[k] + sc.rate[k] * dt).rem_euclid(1.0);
        }
        if let Some(mut m) = materials.get_mut(&sc.material) {
            m.uv_transform = bevy::math::Affine2::from_translation(Vec2::from(sc.offset));
        }
    }
}

pub(super) fn interpolate(
    g: Res<Game>,
    mut q: Query<(&mut Transform, &Visual, Option<&FirstPerson>)>,
    mut skins: Query<(Entity, &mut SkinFrames, &ViewVisibility, &GlobalTransform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    skin_queue: Res<skin::SkinQueue>,
    eye: Query<(&GlobalTransform, &Projection), With<level::WorldCamera>>,
    mut frame: Local<u32>,
) {
    let a = g.alpha;
    *frame = frame.wrapping_add(1);
    // Writing a mesh costs. What
    // no view sees (last frame) waits, and what is far from the eye (through a sight, as near as it looks) is written every few frames.
    let view = eye.iter().next().map(|(t, p)| (t.translation(), if let Projection::Perspective(p) = p { p.fov / crate::settings::world_fov() } else { 1.0 }));
    // (the first-person arms are lit by the body's yaw, not the camera's: `skin::write_arms`)
    let sim = &g.session.sim;
    let body = sim.players[g.player].soldier.map(|i| &sim.vehicles[i].craft.body).map(|b| assets::to_transform(&assets::frame_matrix(&b.rows, b.pos)).rotation);
    for (e, mut sf, seen, at) in skins.iter_mut() {
        if sf.written || sf.cur.is_empty() || sf.prev.len() != sf.cur.len() || !seen.get() {
            continue;
        }
        let stride = view.map_or(1, |(from, zoom)| match from.distance(at.translation()) * zoom {
            d if d < 50.0 => 1,
            d if d < 120.0 => 2,
            _ => 4,
        });
        if !(e.index_u32() + *frame).is_multiple_of(stride) {
            continue;
        }
        // (the arms' light depends on the camera's turn too: `skin::write_arms`)
        let fixed = crate::assets::graphics().fixed_light;
        let arms = sf.arms && fixed;
        let rest = sf.rest_normals && fixed;
        if sf.prev == sf.cur && !arms {
            sf.written = true;
        }
        let pos = sf.prev.iter().zip(&sf.cur).map(|(p, c)| std::array::from_fn(|k| p[k] + (c[k] - p[k]) * a)).collect();
        if let Some(mesh) = meshes.get_mut_untracked(&sf.mesh) {
            if rest {
                skin::write_arms(&sf.skin, pos, at.rotation(), if sf.arms { body } else { None }, mesh);
            } else {
                skin::write(&sf.skin, pos, mesh);
            }
            skin_queue.push(sf.mesh.id(), mesh.create_packed_vertex_buffer_data());
            SKIN_WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    for (mut t, v, fp) in q.iter_mut() {
        if fp.is_some_and(|f| f.on) {
            continue;
        }
        t.translation = v.prev.translation.lerp(v.cur.translation, a);
        t.rotation = v.prev.rotation.slerp(v.cur.rotation, a);
        t.scale = v.cur.scale;
    }
}
