//! Keeps the Bevy entities in step with the simulation: spawns, updates and removes what each object shows.

use super::*;

/// After a tick: every object's geometries where its parts are now (new objects get theirs,
/// removed ones lose them).
#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
    commands: &mut Commands,
    g: &Game,
    shown: &mut Shown,
    visuals: &mut Visuals,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    cache: &mut Cache,
    skins: &mut Skins,
) {
    let world = g.session.sim.world;
    shown.1.clear();
    for (i, v) in g.session.sim.vehicles.iter().enumerate() {
        // in a vehicle, on a seat that shows it: the seat's own place and pose
        let seated = v.entry.inside.filter(|_| !v.removed).and_then(|j| {
            let c = &g.session.sim.vehicles[j];
            let seats = shown.3.entry(c.template.to_ascii_lowercase()).or_insert_with(|| Arc::new(model::seats(&world.db, &c.template, &c.craft.parts))).clone();
            let seat = model::seat_at(&seats, &c.entry.seats, v.entry.position)?.clone();
            let frame = match seat.part {
                Some(k) => {
                    let f = c.craft.frames();
                    assets::frame_matrix(&f.get(k)?.child_rows, f[k].pos)
                }
                None => assets::frame_matrix(&c.craft.body.rows, c.craft.body.pos),
            };
            Some((frame.mul(&seat.local), seat))
        });
        let gone = v.removed || (!v.in_world() && seated.is_none());
        if let Some((t, es)) = shown.0.get(&i)
            && (gone || *t != v.template)
        {
            for s in es {
                commands.entity(s.entity).despawn();
            }
            shown.0.remove(&i);
        }
        if gone {
            shown.4.remove(&i);
            continue;
        }
        let frames = v.craft.frames();
        let at = |part: Option<usize>, local: &Mat4| {
            let m = match (&seated, part) {
                (Some((on_seat, _)), _) => *on_seat,
                (None, Some(k)) => assets::frame_matrix(&frames[k].child_rows, frames[k].pos),
                (None, None) => assets::frame_matrix(&v.craft.body.rows, v.craft.body.pos),
            };
            assets::to_transform(&m.mul(local))
        };
        let parachute = v.movement.as_ref().is_some_and(|m| m.parachute);
        // from its own eyes the player does not see its body: its arms instead
        let own = !g.third_person && g.session.sim.players[g.player].soldier == Some(i);
        let arms = own && first_person_arms(g, i);
        let seen = |para: bool, fp: bool| if fp != arms || (!fp && own) || (para && !parachute) { Visibility::Hidden } else { Visibility::Inherited };
        if !shown.0.contains_key(&i) {
            let skeleton = v.movement.as_ref().and_then(|m| m.skeleton.as_ref()).map(|s| s.file.clone());
            let mut es = Vec::new();
            // (a soldier in a vehicle casts none: `shadows::update`)
            let casts = level::has_dynamic_shadow(world, &v.template);
            // (`BYTEFIELD_BODY`: a check's choice of `setRandomGeometries`, to compare with the original's)
            let body = std::env::var("BYTEFIELD_BODY").ok().and_then(|b| b.parse().ok()).unwrap_or(i + 1);
            for d in model::draws_with_cockpits(&world.db, &v.template, &v.craft.parts, body) {
                let parts = cache.get(&world.vfs, &d.kind, &d.file, meshes, materials, images);
                // a soldier's skin follows its skeleton; without one it stands in its bind pose
                // a track's, its own skeleton (moved by its wheels)
                let track = d.track.as_ref().and_then(|(ske, bones)| {
                    let file = Arc::new(bytefield_formats::anim::Skeleton::parse(&world.vfs.read(ske).ok()?).ok()?);
                    Some((file.clone(), Arc::new((file.bones.iter().map(bytefield_sim::skeleton::bone_matrix).collect::<Vec<_>>(), bones.clone()))))
                });
                let skins = match (&d.skin, &skeleton, &track) {
                    (Some(f), Some(sk), _) if d.soldier => skins.get(&world.vfs, &v.template, &d.file, f, sk, false),
                    (Some(f), _, Some((sk, _))) => skins.get(&world.vfs, &v.template, &d.file, f, sk, true),
                    _ => None,
                }
                .filter(|s| s.len() == parts.len());
                let local = if d.soldier && skins.is_none() { d.local.mul(&model::SKELETON_ROOT) } else { d.local };
                let t = at(d.part, &local);
                let spin = d.wheel.then(|| wheel(&parts.iter().filter_map(|(m, _)| meshes.get(m)).collect::<Vec<_>>())).flatten().map(|w| (w, 0.0));
                for (k, (m, mat)) in parts.into_iter().enumerate() {
                    // a running texture needs its own material (the cache shares them)
                    let scroll = d.tex_speed.and_then(|speed| {
                        let material = materials.add(materials.get(&mat)?.clone());
                        Some(Scroll { speed, side: d.local.0[3][0] as f32, rate: [0.0; 2], offset: [0.0; 2], material })
                    });
                    let mat = scroll.as_ref().map_or(mat, |s| s.material.clone());
                    let fade = d.fade.then(|| materials.get(&mat).cloned()).flatten().map(|m| materials.add(m));
                    let mat = fade.clone().unwrap_or(mat);
                    let skin = skins.as_ref().and_then(|s| Some((s[k].clone(), meshes.add(meshes.get(&m)?.clone()))));
                    let mut e = commands.spawn((Mesh3d(skin.as_ref().map_or(m, |s| s.1.clone())), MeshMaterial3d(mat), t, seen(d.parachute, d.first_person), Visual { prev: t, cur: t }));
                    if casts && !d.first_person && !matches!(d.compare, Some((_, true))) {
                        // Bevy's sun shadow map (`CastsShadow`) casts every piece's visible mesh, so with it on the Stryker's aerials (part of
                        // its hull mesh) throw thin lines on the sand that the original's silhouette lacks. Filtering the pieces by `d.shadow` changes
                        // nothing there (the hull is a piece that casts); a shadow mesh of its own for Bevy was judged not worth it (an extra option).
                        e.insert(level::CastsShadow);
                        // the original's silhouette: a soldier's skin as drawn; a piece's shadow LOD, only
                        // with its geometry's `hasDynamicShadow 1` and a file that has one (the Panzer IV: its hull, not the turret, gun or wheels)
                        // (not a track's skin: an `AnimatedMesh` without the flag, out of the Panzer IV's pass, 600/200 its hull's alone)
                        if d.soldier {
                            e.insert(crate::shadows::Caster(i));
                        } else if d.shadow && let Some(h) = cache.shadow(&world.vfs, &d.kind, &d.file, meshes) {
                            e.insert((crate::shadows::Caster(i), crate::shadows::ShadowMesh(h)));
                        }
                    }
                    if let Some((sk, h)) = &skin {
                        // its vertices move: the bind pose's box, wide enough for any pose (the arms' first person, none)
                        let bounds = meshes.get(h).and_then(|m| <Mesh as bevy::camera::primitives::MeshAabb>::get_aabb(m)).filter(|_| !d.first_person);
                        match bounds {
                            Some(b) => e.insert((bevy::camera::visibility::NoAutoAabb, bevy::camera::primitives::Aabb { center: b.center, half_extents: b.half_extents + bevy::math::Vec3A::splat(SKIN_REACH) })),
                            None => e.insert(NoFrustumCulling),
                        };
                        e.insert(SkinFrames { skin: sk.clone(), mesh: h.clone(), prev: Vec::new(), cur: Vec::new(), written: false, arms: d.first_person, rest_normals: !d.parachute });
                        // (the soldier's body from outside too: the same shader and constants)
                        if !d.parachute {
                            e.insert(crate::envmap::ArmsLight);
                        }
                    }
                    if d.first_person {
                        e.insert((FirstPerson { local: Transform::IDENTITY, on: true }, RenderLayers::layer(FIRST_PERSON_LAYER)));
                    }
                    let chute = track.as_ref().filter(|_| skin.is_some() && d.parachute).map(|t| skin::Chute::new(t.0.clone()));
                    let track = track.as_ref().filter(|_| skin.is_some()).map(|t| t.1.clone());
                    es.push(Shape { entity: e.id(), part: d.part, local, parachute: d.parachute, skin, first_person: d.first_person, spring: (d.template.clone(), d.instance), track, chute, scroll, spin, cockpit: d.cockpit, compare: d.compare, dummy: d.dummy, wreck: d.wreck, fade, file: matches!(d.kind.as_str(), "standardmesh" | "animatedmesh").then(|| d.file.clone()) });
                }
            }
            shown.0.insert(i, (v.template.clone(), es));
        }
        let drive = v.drive();
        let es = &mut shown.0.get_mut(&i).expect("just shown").1;
        let posed = if es.iter().any(|s| s.skin.is_some()) {
            v.movement.as_ref().and_then(|m| match &seated {
                // (the player's own, on a seat fixed to the vehicle: a gun's seat turns with the gun, unwatched in the original)
                Some((_, seat)) => m.posed_skeleton_seated(&seat.lower, &seat.upper, seat.half_body, if g.session.sim.players[g.player].soldier == Some(i) && seat.part.is_none() { g.seat_head } else { 0.0 }),
                None => m.posed_skeleton(),
            })
        } else {
            None
        };
        // (where the kit's parts hang, `kit_parts`)
        let kit_pose = match (&posed, es.iter().find(|s| s.skin.is_some() && !s.first_person && !s.parachute)) {
            (Some(sk), Some(s)) => {
                let m = match &seated {
                    Some((on_seat, _)) => *on_seat,
                    None => assets::frame_matrix(&v.craft.body.rows, v.craft.body.pos),
                };
                let own_eyes = !g.third_person && g.session.sim.players[g.player].soldier == Some(i);
                Some(KitPose { frame: m.mul(&s.local), abs: sk.abs.clone(), file: sk.file.clone(), own_eyes })
            }
            _ => None,
        };
        let posed_1p = if arms { v.movement.as_ref().and_then(|m| m.posed_skeleton_1p(g.tick as f32 * TICK as f32)) } else { None };
        let offset = if arms { arms_offset(g, i) } else { Vec3::ZERO };
        // a cockpit is seen from inside by its own crew in first person, the camera near it
        let p = &g.session.sim.players[g.player];
        let eye = (!g.third_person && p.alive && p.soldier.is_some_and(|s| g.session.sim.vehicles[s].entry.inside == Some(i))).then(|| camera_now(g).translation);
        let my_pco = p.soldier.and_then(|s| v.entry.seats.get(g.session.sim.vehicles[s].entry.position)).map(|seat| model::pco_id(&seat.pco));
        // the view's angle in `getLodLevelRelative`, against the default one
        let lod_fov = vehicle_fov(g).unwrap_or(WORLD_FOV) / WORLD_FOV;
        for s in es.iter_mut() {
            if let Some((w, angle)) = &mut s.spin {
                let b = &v.craft.body;
                let forward = (0..3).map(|k| b.vel[k] * b.rows[2][k]).sum::<f32>();
                *angle = (*angle + forward * TICK as f32 / w.radius).rem_euclid(std::f32::consts::TAU);
            }
            if let Some(sc) = &mut s.scroll {
                let n = drive.map_or(0.0, |d| d.ratio * d.rpm_at(sc.side)) * SCROLL_UPDATES_PER_SECOND;
                sc.rate = [n * sc.speed[0], n * sc.speed[1]];
            }
            // (shut, it is hidden: its vertices wait until it opens)
            let chute_bones = s.chute.as_mut().and_then(|c| match v.movement.as_ref().filter(|m| m.parachute) {
                Some(m) => Some(c.advance(&m.machine, TICK as f32).to_vec()),
                None => {
                    c.close();
                    None
                }
            });
            let s = &*s;
            let Ok((mut vis, mut shown_now, fp, frames_of)) = visuals.get_mut(s.entity) else { continue };
            vis.prev = vis.cur;
            vis.cur = match s.spin {
                Some((w, angle)) => at(s.part, &spun(&s.local, &w, angle)),
                None => at(s.part, &s.local),
            };
            if let Some(d) = spring_offset(v, &s.spring.0, s.spring.1) {
                vis.cur.translation += assets::to_bevy(d);
            }
            *shown_now = seen(s.parachute, s.first_person);
            let destroyed = v.armor.as_ref().is_some_and(|a| a.destroyed);
            if s.wreck.is_some_and(|w| w != destroyed) {
                *shown_now = Visibility::Hidden;
            }
            // The object's update sets the alpha (none given: it keeps the last one)
            if let Some(h) = &s.fade
                && let Some(alpha) = v.armor.as_ref().and_then(|a| a.wreck_alpha())
                && let Some(mut m) = materials.get_mut(h)
            {
                m.base_color.set_alpha(alpha as f32 / 255.0);
                m.alpha_mode = AlphaMode::Blend;
            }
            // `disableWhenFired`: the dummies hide from the shot (`fireFinished`) to the end of the
            // reload (`ReloadFinished`)
            if s.dummy
                && let Some(k) = s.part
                && v.weapons.iter().any(|(p, w)| *p == k && w.template.disable_when_fired && (w.clip() == 0 || w.reload > 0.0))
            {
                *shown_now = Visibility::Hidden;
            }
            // the nose camera draws nothing of one's own vehicle: neither cockpit nor propeller (the Bf109's)
            // TODO: in a tank or a ship, not seen in the original.
            if g.nose && eye.is_some() {
                *shown_now = Visibility::Hidden;
            }
            if let Some((at, high)) = s.compare {
                // the rpm of the first engine at or above the part, as `LandingGear` asks for it
                let mut k = s.part;
                let rpm = loop {
                    let Some(p) = k.and_then(|k| v.craft.parts.get(k)) else { break 0.0 };
                    if let bytefield_sim::craft::Kind::Engine(e) = &p.kind {
                        break e.rpm.abs();
                    }
                    k = p.parent;
                };
                if (rpm * model::COMPARE_RPM >= at) != high {
                    *shown_now = Visibility::Hidden;
                }
            }
            if let Some(c) = &s.cockpit {
                let near = my_pco == Some(c.owner) && eye.is_some_and(|e| e.distance_squared(at(s.part, &c.center).translation) * model::COCKPIT_K * lod_fov * lod_fov <= c.distance);
                if near != c.inside || (near && g.nose) {
                    *shown_now = Visibility::Hidden;
                } else if near && world.vehicle_category(&v.template) != 2 {
                    // not a plane's: its canopy is open, and the sides show sky as they should
                    shown.1.push(s.entity);
                }
            }
            if let Some(mut fp) = fp {
                fp.local = Transform::from_translation(offset);
            }
            let (Some((skin, _)), Some(mut sf)) = (&s.skin, frames_of) else { continue };
            if s.chute.is_some() {
                if let Some(abs) = &chute_bones {
                    sf.push(crate::skin::positions(skin, abs));
                }
            } else if let Some(t) = &s.track {
                // each bone as far from its rest as its wheel, in the track's frame
                let rows = match s.part {
                    Some(k) => frames[k].child_rows,
                    None => v.craft.body.rows,
                };
                let mut abs = t.0.clone();
                for (b, (name, n)) in abs.iter_mut().zip(&t.1) {
                    if let Some(d) = spring_offset(v, name, *n) {
                        for (k, row) in rows.iter().enumerate() {
                            b[12 + k] += d[0] * row[0] + d[1] * row[1] + d[2] * row[2];
                        }
                    }
                }
                sf.push(crate::skin::positions(skin, &abs));
            } else if let Some(sk) = if s.first_person { &posed_1p } else { &posed } {
                sf.push(crate::skin::positions(skin, &sk.abs));
            }
        }
        match kit_pose {
            Some(k) => {
                shown.4.insert(i, k);
            }
            None => {
                shown.4.remove(&i);
            }
        }
    }
    shown.4.retain(|&i, _| i < g.session.sim.vehicles.len() && !g.session.sim.vehicles[i].removed);
    // what lies on the ground: a kit shows its own geometry (the bag; its parts are the
    // soldier's), anything else its whole tree
    for (k, t) in g.session.sim.items.iter().enumerate() {
        let here = !t.removed && t.carrier.is_none();
        if let Some((name, at, es)) = shown.2.get(&k)
            && (!here || *name != t.template || *at != t.pos)
        {
            for e in es {
                commands.entity(*e).despawn();
            }
            shown.2.remove(&k);
        }
        if !here || shown.2.contains_key(&k) {
            continue;
        }
        let m = assets::frame_matrix(&t.rows, t.pos);
        let mut es = Vec::new();
        for d in model::draws(&world.db, &t.template, &[]).into_iter().filter(|d| t.kind != "kit" || d.template.eq_ignore_ascii_case(&t.template)) {
            let tr = assets::to_transform(&m.mul(&d.local));
            for (mesh, mat) in cache.get(&world.vfs, &d.kind, &d.file, meshes, materials, images) {
                es.push(commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), tr, Visibility::Inherited, Visual { prev: tr, cur: tr })).id());
            }
        }
        shown.2.insert(k, (t.template.clone(), t.pos, es));
    }
}

/// The weapon's animation for the soldier's upper body state (`setOtherState
/// c_AsmWeaponState`), at the time it has run with that state: the state's time (0..1 at its
/// own speed) scaled to the weapon animation's speed.
pub(super) fn weapon_animation(m: &bytefield_sim::soldier::Movement) -> Option<(&bytefield_formats::anim::BoneAnimation, f32, bool)> {
    let (a, speed, looping, own) = m.machine.weapon_animation(m.upper.state?)?;
    let t = if own != 0.0 { m.upper.time * speed / own } else { m.upper.time };
    Some((a, t, looping))
}

/// Whether soldier `i` (the player's) shows its first-person arms: alive, on foot.
pub(super) fn first_person_arms(g: &Game, i: usize) -> bool {
    let v = &g.session.sim.vehicles[i];
    g.session.sim.players[g.player].alive && v.entry.inside.is_none() && v.movement.is_some()
}
