//! The per-frame simulation step: whole 1/30 s ticks from the gathered input, scripts, replay and session events.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn step(
    mut commands: Commands,
    time: Res<Time>,
    mut g: ResMut<Game>,
    mut shown: ResMut<Shown>,
    mut shells: ResMut<Shells>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut visuals: Visuals,
    mut shell_q: Query<(&mut Transform, &mut Visibility), Without<Visual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<Cache>,
    mut skins: ResMut<Skins>,
    mut weapons: ResMut<Weapons>,
    (mut in_hand, mut shade): (ResMut<InHand>, ResMut<Shade>),
) {
    // a script runs one tick per frame (the same pictures whatever the machine), once the
    // renderer is warm (it compiles its shaders in the background: until then nothing 3D shows)
    // (on a server the round goes at its clock: a script too, by the real ticks; the original's too)
    // (`BYTEFIELD_REALTIME`: a script at the clock, as a round is played: what a measure of the frame rate needs)
    let lockstep = g.script.is_some() && g.net.is_none() && !g.on_orig() && !g.realtime;
    // (P: only in a round of one's own, not on a server, ours or the original's, nor while typing or in the menu)
    if crate::controls::current().just(&keys, &buttons, "p") && g.net.is_none() && !g.on_orig() && g.script.is_none() && g.chat_box.is_none() && !g.menu_open {
        g.paused = !g.paused;
    }
    let ticks = if g.paused {
        g.acc = 0.0;
        0
    } else if lockstep {
        (time.elapsed_secs() > WARM_UP) as u32
    } else if g.script.is_some() && time.elapsed_secs() <= WARM_UP {
        0
    } else {
        g.acc += time.delta_secs_f64();
        let n = ((g.acc / TICK) as u32).min(MAX_TICKS);
        g.acc = (g.acc - n as f64 * TICK).clamp(0.0, TICK);
        n
    };
    for _ in 0..ticks {
        let mut now = held_now(&mut g, &keys, &buttons);
        // for a ticket's replay: what is held, before the radio and the spawn screen take their keys (they take
        // them again from the replay's); and on a replay, the spawn screen's and the console's events of this tick
        if g.net.is_none() && !g.on_orig() {
            let t = g.tick;
            g.recording.held(t, &now);
            if g.script.is_some() {
                crate::replay::apply_events(&mut g, &now);
            }
        }
        radio_keys(&mut g, &mut now);
        wheel_keys(&g, &mut now);
        spawn_keys(&mut g, &mut now);
        // (`BYTEFIELD_BOARD_TICK=<n>`: a script's `board` at that tick, for a test that plays through the devices)
        if g.script.is_none() && std::env::var("BYTEFIELD_BOARD_TICK").ok().and_then(|n| n.parse().ok()) == Some(g.tick) {
            now.keys.insert("board".to_string());
            g.record_event("board".into());
        }
        // (a script's `kill`: the player's soldier dies, to see the screen when dead)
        if now.keys.contains("kill") && !g.before.keys.contains("kill") && g.net.is_none() {
            let k = g.player;
            let sim = &mut g.session.sim;
            if let Some(i) = sim.players[k].soldier {
                sim.give_damage(i, 1000.0, bytefield_sim::damage::Source::Hit);
            }
        }
        // (a script's `board`: into the driver seat of the nearest vehicle armed there, wherever it
        // is, to see the gauges in one; `BYTEFIELD_BOARD=<template>`, the nearest of that one, armed or not;
        // `BYTEFIELD_BOARD_SEAT=<n>`, that seat of it, to see who rides there)
        if now.keys.contains("board") && !g.before.keys.contains("board") && g.net.is_none() {
            let player = g.player;
            let sim = &mut g.session.sim;
            if let Some(i) = sim.players[player].soldier.filter(|&i| sim.vehicles[i].entry.inside.is_none()) {
                let at = sim.vehicles[i].craft.body.pos;
                let d = |v: &bytefield_sim::mover::Vehicle| (0..3).map(|j| (v.craft.body.pos[j] - at[j]).powi(2)).sum::<f32>();
                let only = std::env::var("BYTEFIELD_BOARD").ok();
                let free = sim.vehicles.iter().enumerate().filter(|(_, v)| only.as_ref().is_none_or(|t| v.template.eq_ignore_ascii_case(t)) && !v.removed && !v.is_soldier() && v.entry.occupants.first().is_some_and(|o| o.is_none()) && (only.is_some() || v.weapons.iter().any(|(p, _)| v.craft.parts[*p].seat == 0))).min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map(|(k, _)| k);
                if let Some(k) = free {
                    let seat = std::env::var("BYTEFIELD_BOARD_SEAT").ok().and_then(|n| n.parse().ok()).filter(|&n| n < sim.vehicles[k].entry.seats.len()).unwrap_or(0);
                    sim.enter(i, k, seat);
                }
            }
        }
        // (a script's `wreck`: the nearest vehicle is destroyed, to see its wreck and its fade;
        // `BYTEFIELD_WRECK=<template>`, the nearest of that one)
        if now.keys.contains("wreck") && !g.before.keys.contains("wreck") && g.net.is_none() {
            let player = g.player;
            let sim = &mut g.session.sim;
            if let Some(i) = sim.players[player].soldier {
                let at = sim.vehicles[i].craft.body.pos;
                let d = |v: &bytefield_sim::mover::Vehicle| (0..3).map(|j| (v.craft.body.pos[j] - at[j]).powi(2)).sum::<f32>();
                let only = std::env::var("BYTEFIELD_WRECK").ok();
                let mine = sim.vehicles[i].entry.inside;
                let near = sim.vehicles.iter().enumerate().filter(|(k, v)| Some(*k) != mine && only.as_ref().is_none_or(|t| v.template.eq_ignore_ascii_case(t)) && !v.removed && !v.is_soldier() && v.armor.as_ref().is_some_and(|a| !a.destroyed)).min_by(|a, b| d(a.1).total_cmp(&d(b.1))).map(|(k, _)| k);
                if let Some(k) = near {
                    println!("wreck: {} at {:?}", sim.vehicles[k].template, sim.vehicles[k].craft.body.pos);
                    sim.give_damage(k, 100_000.0, bytefield_sim::damage::Source::Hit);
                }
            }
        }
        // (a script's `goto`: the player's soldier to `BYTEFIELD_GOTO=x,z`, 1 m over the terrain there)
        if now.keys.contains("goto") && !g.before.keys.contains("goto") && g.net.is_none()
            && let Some([x, z]) = std::env::var("BYTEFIELD_GOTO").ok().and_then(|s| s.split(',').map(|v| v.trim().parse().ok()).collect::<Option<Vec<f32>>>()).and_then(|v| <[f32; 2]>::try_from(v).ok())
        {
            let player = g.player;
            let sim = &mut g.session.sim;
            if let Some(i) = sim.players[player].soldier
                && let Some(h) = sim.world.height_at(x as f64, z as f64)
            {
                let b = &mut sim.vehicles[i].craft.body;
                b.pos = [x, h as f32 + 1.0, z];
                b.vel = [0.0; 3];
            }
        }
        // the views (`c_PICameraMode1–4`, `c_PIToggleCameraMode`): inside, chase, from the front, fly-by
        let pressed = |k: &str| now.keys.contains(k) && !g.before.keys.contains(k);
        let rmb = pressed("rmb");
        // (Ctrl+F11 and Ctrl+F12 are the tester's mark and the debug panel, `debug.rs`)
        let ctrl = g.script.is_none() && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        // on foot, the views the soldier's `Camera` allows (`CVMChase`, `CVMFrontChase`, `CVMFlyBy`): none in bf1942 (F10–F12 and C do
        // nothing there, and out of a vehicle the view goes back inside; `game.serverExternalViews 1`, the original's soldier with the four
        // keys), all three in GC (its soldier from behind, from the front and passing by, GC_Hoth in the original)
        let riding = g.session.sim.players[g.player].soldier.is_some_and(|i| g.session.sim.vehicles[i].entry.inside.is_some());
        let foot = if riding { [false; 3] } else { g.session.sim.players[g.player].soldier.map_or([false; 3], |i| foot_views(&g.session.sim, &g.session.sim.vehicles[i].template)) };
        let views = [Outside::Chase, Outside::Front, Outside::FlyBy];
        let foot_ok = |o: Outside| g.third_on_foot || views.iter().zip(foot).any(|(v, ok)| *v == o && ok);
        // The nose camera: F9 inside toggles it, from outside it goes back
        // inside with the cockpit; C inside goes first to it, then out. Only with `serverAllowNoseCam`: without it F9
        // keeps the cockpit.
        // A seat whose `Camera` has no `outsideHudOffset` has none: C skips it, and F9 changes nothing (T34, b278/f9: 3 pixels).
        // C goes round every view: inside, the nose camera if the seat has one, chase, front, fly-by and inside again
        // (the original's T34 and LCVP, five C's in a row; neither has a nose camera).
        let allowed = g.session.sim.rules.server.nose_cam && {
            let sim = &g.session.sim;
            sim.players[g.player].soldier.and_then(|i| sim.vehicles[i].entry.inside.map(|k| nose_offset(&g, &sim.vehicles[k], sim.vehicles[i].entry.position).is_some())).unwrap_or(false)
        };
        if !riding && g.third_person && !foot_ok(g.outside) {
            g.third_person = false;
        } else if pressed("c") {
            if riding && !g.third_person && !g.nose && allowed {
                g.nose = true;
            } else if !riding && !g.third_on_foot {
                // on foot: the allowed views in turn, then inside
                let from = if g.third_person { views.iter().position(|v| *v == g.outside).map_or(0, |k| k + 1) } else { 0 };
                match (from..3).find(|&k| foot[k]) {
                    Some(k) => (g.third_person, g.outside) = (true, views[k]),
                    None => g.third_person = false,
                }
            } else if !g.third_person {
                (g.third_person, g.nose, g.outside) = (true, false, Outside::Chase);
            } else if !riding {
                g.third_person = false;
            } else {
                match g.outside {
                    Outside::Chase => g.outside = Outside::Front,
                    Outside::Front => g.outside = Outside::FlyBy,
                    Outside::FlyBy => g.third_person = false,
                }
            }
            g.orbit = Vec2::ZERO;
        } else if pressed("f9") {
            g.nose = riding && !g.third_person && !g.nose && allowed;
            g.third_person = false;
            g.orbit = Vec2::ZERO;
        } else if !ctrl && let Some(o) = [("f10", Outside::Chase), ("f11", Outside::Front), ("f12", Outside::FlyBy)].into_iter().find(|(k, _)| pressed(k)).map(|x| x.1).filter(|&o| riding || foot_ok(o)) {
            g.third_person = true;
            g.outside = o;
            g.orbit = Vec2::ZERO;
        }
        if !riding {
            g.nose = false;
        }
        scout_toggle(&mut g, rmb);
        // the numeric pad turns the outside view about the vehicle (`c_PICameraX/Y`, 4/6 and 8/2). Level 3: the
        // original's client does it (the server's `Camera` does not read them); its rate and limits are guesses.
        // TODO: compare with the original (its rate, whether it springs back, and on foot).
        let pair = |a: &str, b: &str| (now.keys.contains(a) as i32 - now.keys.contains(b) as i32) as f32;
        let turn = Vec2::new(pair("num6", "num4"), pair("num8", "num2")) * std::f32::consts::FRAC_PI_2 * TICK as f32;
        g.orbit = Vec2::new((g.orbit.x + turn.x) % std::f32::consts::TAU, (g.orbit.y + turn.y).clamp(-0.5, 1.2));
        let mut a = axes(&now, &g.before);
        keyboard_sensitivity(&g, &mut a);
        air_controls(&g, &now, &mut a);
        let player = g.player;
        // (a script's `seek` on the original server: to the nearest free land vehicle and in, as the simple bots go)
        if now.keys.contains("seek") && g.on_orig() {
            let sim = &g.session.sim;
            if let Some(i) = sim.players[player].soldier.filter(|&i| sim.vehicles[i].entry.inside.is_none()) {
                let at = sim.vehicles[i].craft.body.pos;
                let d2 = |j: usize| (0..3).map(|n| (sim.vehicles[j].craft.body.pos[n] - at[n]).powi(2)).sum::<f32>();
                if let Some(j) = (0..sim.vehicles.len()).filter(|&j| sim.free_to_ride(j)).min_by(|&x, &y| d2(x).total_cmp(&d2(y))) {
                    a = bytefield_sim::input::through_actions(&sim.seek_axes(i, j, g.tick % 2 == 0));
                }
            }
        }
        // (a script's `say` and `sayteam`, on the original server or ours: `BYTEFIELD_SAY` in the chat)
        for (k, to_all) in [("say", true), ("sayteam", false)] {
            if now.keys.contains(k) && !g.before.keys.contains(k)
                && let Ok(text) = std::env::var("BYTEFIELD_SAY")
            {
                if let Some(n) = g.net.as_mut() {
                    n.mirror.say(&text, to_all);
                }
            }
        }
        g.before = now;
        let g = &mut *g;
        let seated = g.session.sim.players[player].soldier.is_some_and(|i| g.session.sim.vehicles[i].entry.inside.is_some());
        g.seat_head = if seated {
            let value = |n: usize| a.iter().find(|x| x.0 == n).map_or(0.0, |x| x.1);
            bytefield_sim::soldier::seat_head_turn(g.seat_head, value(axis::MOUSE_X), value(axis::THROTTLE) != 0.0 || value(axis::YAW) != 0.0, value(axis::FIRE) != 0.0, TICK as f32)
        } else {
            0.0
        };
        state_log(g, &a);
        if !g.on_orig() {
            match g.net.as_mut() {
                // (the action labelled after the last step we know; the server takes it as soon as
                // it comes, and our soldier runs ahead with those it has not yet taken)
                Some(net) => {
                    let step = net.mirror.next_step();
                    net.send(step, a);
                    net.poll();
                    let odd = g.prediction.follow(&mut g.session.sim, &net.mirror, player);
                    if odd > 0 {
                        warn!("{odd} objects of the snapshot the copy does not have");
                    }
                }
                None => {
                    g.session.set_action(player, &a);
                    let t0 = Instant::now();
                    g.session.step();
                    g.spent_ms[0] += t0.elapsed().as_secs_f64() * 1e3;
                }
            }
        }
        // the logs only grew: cut as a server cuts them, the readers' cursors count from the round's start
        // (`BYTEFIELD_KEEP_LOGS=1`: not cut, to measure against)
        if g.tick.is_multiple_of(bytefield_server::LOG_TRIM_EVERY) && !keep_logs() {
            g.session.sim.trim_logs(bytefield_server::LOG_KEEP);
        }
        g.tick += 1;
        let t0 = Instant::now();
        sync(&mut commands, &g, &mut shown, &mut visuals, &mut meshes, &mut materials, &mut images, &mut cache, &mut skins);
        hands(&mut commands, &g, &mut weapons, &mut in_hand, &mut visuals, &mut meshes, &mut materials, &mut images, &mut cache);
        kit_parts(&mut commands, &g, &shown, &mut in_hand, &mut visuals, &mut meshes, &mut materials, &mut images, &mut cache);
        shade_objects(&mut commands, &g, &shown, &in_hand, &mut shade);
        g.spent_ms[1] += t0.elapsed().as_secs_f64() * 1e3;
        update_chase(g);
        let cam = camera_now(&g);
        g.camera = (g.camera.1, cam);
    }
    g.alpha = if lockstep { 1.0 } else { (g.acc / TICK) as f32 };
    // the projectiles where they are now, with their template's meshes (a bullet, a shell, a
    // tracer); known by template and starting point (they have no lasting index)
    let world = g.session.sim.world;
    let mut alive = HashSet::new();
    // (an ended one stays in the list until the round ends: gone)
    for p in g.session.sim.projectiles.iter().filter(|p| p.alive()) {
        let key = (p.template.name.clone(), p.start.map(f32::to_bits));
        let at = assets::frame_matrix(&p.rows, p.pos);
        let es = shells.0.entry(key.clone()).or_insert_with(|| {
            let mut es = Vec::new();
            for d in model::draws(&world.db, &p.template.name, &[]) {
                for (m, mat) in cache.get(&world.vfs, &d.kind, &d.file, &mut meshes, &mut materials, &mut images) {
                    es.push((commands.spawn((Mesh3d(m), MeshMaterial3d(mat), assets::to_transform(&at.mul(&d.local)))).id(), d.local));
                }
            }
            es
        });
        for (e, local) in es.iter() {
            if let Ok((mut t, _)) = shell_q.get_mut(*e) {
                *t = assets::to_transform(&at.mul(local));
            }
        }
        alive.insert(key);
    }
    shells.0.retain(|k, es| {
        let keep = alive.contains(k);
        if !keep {
            for (e, _) in es.iter() {
                commands.entity(*e).despawn();
            }
        }
        keep
    });
}
