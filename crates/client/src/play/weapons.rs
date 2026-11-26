//! Weapons and damage effects: muzzle flashes, armour fire and smoke, trigger handling and the first-person hands.

use super::*;

/// A weapon's effects that start when it fires (`addTemplate <EffectBundle>` with
/// `startoneffects 0`: the muzzle's flash; 1 is the shell thrown out, not yet), and where on it.
pub(super) fn fire_effects(db: &bytefield_con::templates::TemplateDb, weapon: &str) -> Vec<(String, Mat4)> {
    let Some(t) = db.object_template(weapon) else { return Vec::new() };
    let mut out: Vec<(String, [f64; 3], [f64; 3], i32)> = Vec::new();
    let mut last: Option<usize> = None;
    for p in &t.props {
        match p.name.as_str() {
            "addtemplate" => {
                last = p.args.first().filter(|n| db.object_template(n).is_some_and(|c| c.kind.eq_ignore_ascii_case("effectbundle"))).map(|n| {
                    out.push((n.clone(), [0.0; 3], [0.0; 3], 0));
                    out.len() - 1
                });
            }
            "setposition" | "setrotation" => {
                if let (Some(i), Some(v)) = (last, p.args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3)) {
                    let v = [v[0], v[1], v[2]];
                    if p.name == "setposition" {
                        out[i].1 = v;
                    } else {
                        out[i].2 = v;
                    }
                }
            }
            "startoneffects" => {
                if let Some(i) = last {
                    out[i].3 = p.args.first().and_then(|a| a.trim().parse().ok()).unwrap_or(0);
                }
            }
            _ => {}
        }
    }
    out.into_iter().filter(|e| e.3 == 0).map(|e| (e.0, Mat4::from_pos_ypr(e.1, e.2))).collect()
}

/// A vehicle template's `addArmorEffect hp effect pos`: by threshold, its effects and where.
pub(super) fn armor_effects(db: &bytefield_con::templates::TemplateDb, template: &str) -> std::collections::BTreeMap<i32, Vec<(String, [f32; 3])>> {
    let mut out: std::collections::BTreeMap<i32, Vec<(String, [f32; 3])>> = std::collections::BTreeMap::new();
    if let Some(t) = db.object_template(template) {
        for p in t.props.iter().filter(|p| p.name == "addarmoreffect") {
            let (Some(hp), Some(name)) = (p.args.first().and_then(|a| a.trim().parse::<f32>().ok()), p.args.get(1)) else { continue };
            let at = p.args.get(2).and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3).map_or([0.0; 3], |v| [v[0] as f32, v[1] as f32, v[2] as f32]);
            out.entry(hp as i32).or_default().push((name.clone(), at));
        }
    }
    out
}

/// The effects for the hit points (rounded): alive, the effects of the least threshold
/// at or above them (none above the greatest); destroyed, those of the sim's `wreck_effect`
/// (−1 in the water), or of 0 without them.
pub(super) fn armor_threshold(effects: &std::collections::BTreeMap<i32, Vec<(String, [f32; 3])>>, hp: f32, destroyed: Option<i32>) -> Option<i32> {
    if let Some(k) = destroyed.filter(|&k| k < 0 && effects.contains_key(&k)) {
        return Some(k);
    }
    if destroyed.is_some() || hp.round() < 1.0 {
        return effects.contains_key(&0).then_some(0);
    }
    effects.range(hp.round() as i32..).next().map(|(k, _)| *k).filter(|&k| k > 0)
}

/// The armor's effects playing on each vehicle: its threshold and their ids.
#[derive(Resource, Default)]
pub(super) struct Burning(HashMap<usize, (i32, Vec<u64>)>);

/// Every tick: each vehicle shows the effects of its armor's state (smoke
/// when hurt, fire nearer the end, the blast and the scrap when destroyed), on it, following it.
pub(super) fn armor_fx(g: Res<Game>, mut fx: ResMut<crate::effects::Effects>, mut burning: ResMut<Burning>, mut known: Local<HashMap<String, std::collections::BTreeMap<i32, Vec<(String, [f32; 3])>>>>) {
    if g.is_added() {
        known.clear();
    }
    let db = &g.session.sim.world.db;
    for (i, v) in g.session.sim.vehicles.iter().enumerate() {
        let Some(a) = v.armor.as_ref().filter(|_| !v.removed && !v.has_soldier_contact()) else {
            if let Some((_, ids)) = burning.0.remove(&i) {
                ids.into_iter().for_each(|id| fx.stop(id));
            }
            continue;
        };
        let effects = known.entry(v.template.to_ascii_lowercase()).or_insert_with(|| armor_effects(db, &v.template));
        let want = armor_threshold(effects, a.hp, a.destroyed.then(|| a.wreck_effect.unwrap_or(0)));
        let now = burning.0.get(&i).map(|b| b.0);
        if want == now {
            continue;
        }
        if let Some((_, ids)) = burning.0.remove(&i) {
            ids.into_iter().for_each(|id| fx.stop(id));
        }
        if let Some(k) = want {
            let ids = effects[&k].iter().filter_map(|(name, at)| fx.start_on(db, name, crate::effects::Follow::Vehicle(i, *at))).collect();
            burning.0.insert(i, (k, ids));
        }
    }
}

/// After the ticks: the effects of what happened in them. A soldier's shot flashes at its
/// weapon's muzzle (from its own eyes, on the first person's layer); a projectile's hit shows
/// the effect of its material against the surface's (`MaterialManager.setEffectTemplate`: dust,
/// sparks, blood, a bazooka's blast); an explosion without one, its own.
#[allow(clippy::type_complexity)]
pub(super) fn triggers(
    g: Res<Game>,
    mut fx: ResMut<crate::effects::Effects>,
    mut flashes: ResMut<crate::lights::Flashes>,
    mut decals: ResMut<crate::decals::Decals>,
    mut blasts: ResMut<crate::heat::Blasts>,
    weapons: Res<Weapons>,
    mut seen: Local<(usize, usize, usize)>,
    mut muzzles: Local<HashMap<String, Vec<(String, Mat4)>>>,
    mut flying: Local<(HashSet<(String, [u32; 3])>, HashMap<String, Vec<String>>)>,
) {
    // (a new round: what this remembered was the last one's)
    if g.is_added() {
        (*seen, *muzzles, *flying) = Default::default();
    }
    let sim = &g.session.sim;
    let db = &sim.world.db;
    // a projectile that left: its trails (its template's effects) follow it while it flies
    let (known, trails) = &mut *flying;
    let now: HashSet<(String, [u32; 3])> = sim.projectiles.iter().filter(|q| q.alive()).map(|q| (q.template.name.clone(), q.start.map(f32::to_bits))).collect();
    for key in now.iter().filter(|k| !known.contains(*k)) {
        let names = trails.entry(key.0.to_ascii_lowercase()).or_insert_with(|| {
            db.object_template(&key.0).map_or(Vec::new(), |t| t.all("addtemplate").into_iter().filter_map(|a| a.first().cloned()).filter(|n| db.object_template(n).is_some_and(|c| c.kind.eq_ignore_ascii_case("effectbundle"))).collect())
        });
        for n in names.iter() {
            fx.start_on(db, n, crate::effects::Follow::Projectile(key.0.clone(), key.1));
        }
    }
    *known = now;
    // (cursors count from the round's start: the sim cuts its logs, `trim_logs`)
    for (_, s, shot, proj) in bytefield_sim::log_since(&sim.journal.fired, sim.journal.log_base.fired, &mut seen.0) {
        let v = &sim.vehicles[*s];
        let Some(m) = v.soldier() else {
            // a vehicle's gun: the weapon whose projectile (or tracer) it was, its effects on the
            // shot's barrel
            let name = proj.and_then(|k| sim.projectiles.get(k)).map(|q| q.template.name.clone());
            let w = v.weapons.iter().find(|(_, w)| name.as_ref().is_some_and(|n| *n == w.template.projectile || w.template.tracer.as_ref().is_some_and(|t| t.0 == *n))).or(v.weapons.first());
            let Some((_, w)) = w else { continue };
            let effects = muzzles.entry(w.template.name.to_ascii_lowercase()).or_insert_with(|| fire_effects(db, &w.template.name)).clone();
            let barrel = assets::frame_matrix(&shot.rows, shot.pos);
            for (k, (name, local)) in effects.into_iter().enumerate() {
                let t = assets::to_transform(&local.mul(&barrel));
                if k == 0 {
                    flashes.push(crate::lights::Flash::muzzle(t.translation));
                }
                fx.start(db, &name, t, assets::to_bevy(v.craft.body.vel), false, 0);
            }
            continue;
        };
        let Some(w) = m.weapon.as_ref() else { continue };
        let effects = muzzles.entry(w.template.name.to_ascii_lowercase()).or_insert_with(|| fire_effects(db, &w.template.name)).clone();
        let Some(Some(model)) = weapons.0.get(&w.template.name.to_lowercase()) else { continue };
        let own = !g.third_person && sim.players[g.player].soldier == Some(*s) && first_person_arms(&g, *s);
        let posed = if own { m.posed_skeleton_1p(g.tick as f32 * TICK as f32) } else { m.posed_skeleton() };
        let Some(sk) = posed else { continue };
        let Some(hand) = sk.file.index(weapon::HAND) else { continue };
        let piece = scene_matrix(&model.piece(&model.bones(&sk.abs[hand], &model.rel(weapon_animation(m))), 0));
        for (k, (name, at)) in effects.into_iter().enumerate() {
            let muzzle = at.mul(&piece);
            let t = if own {
                g.camera.1 * Transform::from_translation(arms_offset(&g, *s)) * assets::to_transform(&muzzle)
            } else {
                assets::to_transform(&assets::frame_matrix(&v.craft.body.rows, v.craft.body.pos).mul(&muzzle))
            };
            let layer = if own { FIRST_PERSON_LAYER } else { 0 };
            // (one light per shot, at its first effect)
            if k == 0 {
                flashes.push(crate::lights::Flash::muzzle(t.translation));
            }
            fx.start(db, &name, t, assets::to_bevy(v.craft.body.vel), own, layer);
        }
    }
    let materials = sim.world.materials();
    // a projectile's hit: the effect of its material against the surface's, its up along the
    // surface's normal
    let mut shown: Vec<[f32; 3]> = Vec::new();
    // (the marks: where a projectile stopped on the terrain or a static, not on a vehicle, a soldier or water; a blast's
    // takes the hit's place below)
    let mut marks: Vec<(Vec3, Vec3)> = Vec::new();
    for (m, hit) in bytefield_sim::log_since(&sim.journal.impacts, sim.journal.log_base.impacts, &mut seen.2) {
        let n = assets::to_bevy(hit.normal).normalize_or(Vec3::Y);
        if hit.ended && hit.vehicle.is_none() && hit.bone.is_none() && !hit.water {
            marks.push((assets::to_bevy(hit.point), n));
        }
        let Some(name) = materials.effects.get(&(*m as i64, hit.material as i64)) else { continue };
        let t = Transform::from_translation(assets::to_bevy(hit.point)).with_rotation(Quat::from_rotation_arc(Vec3::Y, n));
        if fx.start(db, name, t, Vec3::ZERO, false, 0) {
            shown.push(hit.point);
        }
    }
    // an explosion no hit showed (a grenade's time ran out, a wreck): its material against the
    // ground's, else a grenade's
    for (_, e) in bytefield_sim::log_since(&sim.journal.explosions, sim.journal.log_base.explosions, &mut seen.1) {
        flashes.push(crate::lights::Flash::explosion(assets::to_bevy(e.center), e.radius));
        blasts.push(assets::to_bevy(e.center), e.radius);
        scorch(&mut decals, &mut marks, sim.world, e);
        if shown.iter().any(|p| (0..3).map(|k| (p[k] - e.center[k]).powi(2)).sum::<f32>() < 4.0) {
            continue;
        }
        let ground = sim.world.level.material_at(e.center[0] as f64, e.center[2] as f64) as i64;
        let name = materials.effects.get(&(e.material as i64, ground)).cloned().unwrap_or_else(|| "e_ExplGranade".into());
        let t = Transform::from_translation(assets::to_bevy(e.center));
        if !fx.start(db, &name, t, Vec3::ZERO, false, 0) {
            fx.start(db, "e_ExplGranade", t, Vec3::ZERO, false, 0);
        }
    }
    for (at, normal) in marks {
        decals.push(crate::decals::Mark { kind: crate::decals::Kind::Hole, at, normal, size: crate::decals::HOLE_SIZE });
    }
}

/// An explosion's scorch: on the surface its projectile hit (within 3 m of its centre), else on the terrain under it if it
/// went off near the ground; none in the air.
fn scorch(decals: &mut crate::decals::Decals, marks: &mut Vec<(Vec3, Vec3)>, world: &bytefield_sim::World, e: &bytefield_sim::damage::Explosion) {
    let c = assets::to_bevy(e.center);
    let size = crate::decals::scorch_size(e.radius);
    let near = marks.iter().enumerate().map(|(k, m)| (k, m.0.distance_squared(c))).filter(|m| m.1 < 9.0).min_by(|a, b| a.1.total_cmp(&b.1));
    let (at, normal) = if let Some((k, _)) = near {
        marks.swap_remove(k)
    } else {
        let (x, z) = (e.center[0] as f64, e.center[2] as f64);
        let Some(h) = world.height_at(x, z) else { return };
        if h < world.level.terrain.water_level as f64 || e.center[1] as f64 - h > (e.radius as f64 * 0.5).max(1.5) {
            return;
        }
        let dh = |dx: f64, dz: f64| world.height_at(x + dx, z + dz).unwrap_or(h);
        let n = Vec3::new((dh(-0.5, 0.0) - dh(0.5, 0.0)) as f32, 1.0, -(dh(0.0, -0.5) - dh(0.0, 0.5)) as f32).normalize();
        (assets::to_bevy([e.center[0], h as f32, e.center[2]]), n)
    };
    decals.push(crate::decals::Mark { kind: crate::decals::Kind::Scorch, at, normal, size });
}

/// A matrix of the skeleton's (row vectors, `f32`) as the scene's.
pub(super) fn scene_matrix(m: &bytefield_sim::skeleton::M4) -> Mat4 {
    Mat4(std::array::from_fn(|r| std::array::from_fn(|c| m[r * 4 + c] as f64)))
}

/// After a tick: each soldier's weapon in its right hand (`weapon`), as its skeleton is posed;
/// none in a vehicle, in a state that hides it (`c_AsmHideWeapon`: dying, climbing a ladder), or
/// from the player's own eyes.
#[allow(clippy::too_many_arguments)]
pub(super) fn hands(
    commands: &mut Commands,
    g: &Game,
    weapons: &mut Weapons,
    in_hand: &mut InHand,
    visuals: &mut Visuals,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    cache: &mut Cache,
) {
    let world = g.session.sim.world;
    for (i, v) in g.session.sim.vehicles.iter().enumerate() {
        let held = v.soldier().filter(|m| !v.removed && v.in_world() && v.entry.inside.is_none() && (m.lower.flags(&m.machine) | m.upper.flags(&m.machine)) & bytefield_sim::anim::HIDE_WEAPON == 0).and_then(|m| Some((m, m.weapon.as_ref()?.template.name.clone())));
        let name = held.as_ref().map(|h| h.1.clone());
        if let Some((had, es)) = in_hand.0.get(&i)
            && Some(had) != name.as_ref()
        {
            for (e, _) in es {
                commands.entity(*e).despawn();
            }
            in_hand.0.remove(&i);
        }
        let Some((m, name)) = held else { continue };
        let Some(model) = weapons.0.entry(name.to_lowercase()).or_insert_with(|| weapon::load(&world.db, &world.vfs, &name).map(Arc::new)).clone() else { continue };
        let Some(sk) = m.posed_skeleton() else { continue };
        let Some(hand) = sk.file.index(weapon::HAND) else { continue };
        let rel = model.rel(weapon_animation(m));
        let bones = model.bones(&sk.abs[hand], &rel);
        let body = assets::frame_matrix(&v.craft.body.rows, v.craft.body.pos);
        let at = |piece: usize| assets::to_transform(&body.mul(&scene_matrix(&model.piece(&bones, piece))));
        let own = !g.third_person && g.session.sim.players[g.player].soldier == Some(i);
        // from its own eyes, from the first-person skeleton, before the eyes
        let arms = own && first_person_arms(g, i);
        let bones_1p = if arms { m.posed_skeleton_1p(g.tick as f32 * TICK as f32).map(|sk| model.bones(&sk.abs[hand], &rel)) } else { None };
        let shift = if arms { arms_offset(g, i) } else { Vec3::ZERO };
        let near = |piece: usize| bones_1p.as_ref().map(|b| Transform::from_translation(shift) * assets::to_transform(&scene_matrix(&model.piece(b, piece))));
        let seen = if own && bones_1p.is_none() { Visibility::Hidden } else { Visibility::Inherited };
        let es = in_hand.0.entry(i).or_insert_with(|| {
            let mut es = Vec::new();
            for (k, p) in model.pieces.iter().enumerate() {
                let t = at(k);
                for (mesh, mat) in cache.get(&world.vfs, &p.kind, &p.file, meshes, materials, images) {
                    let fp = FirstPerson { local: Transform::IDENTITY, on: false };
                    let mut e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, seen, Visual { prev: t, cur: t }, fp));
                    if level::has_dynamic_shadow(world, &name) {
                        e.insert((level::CastsShadow, crate::shadows::Caster(i)));
                    }
                    es.push((e.id(), k));
                }
            }
            (name.clone(), es)
        });
        for (e, piece) in &es.1 {
            if let Ok((mut vis, mut shown_now, fp, _)) = visuals.get_mut(*e) {
                vis.prev = vis.cur;
                vis.cur = at(*piece);
                *shown_now = seen;
                if let Some(mut fp) = fp {
                    if fp.on != bones_1p.is_some() || own {
                        let layer = if bones_1p.is_some() { FIRST_PERSON_LAYER } else { 0 };
                        commands.entity(*e).insert(RenderLayers::layer(layer));
                    }
                    fp.on = bones_1p.is_some();
                    if let Some(t) = near(*piece) {
                        fp.local = t;
                        arms_log(g.tick, *piece, &t);
                    }
                }
            }
        }
    }
}
