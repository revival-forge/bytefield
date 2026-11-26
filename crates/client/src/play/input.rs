//! Keys, mouse and console: gathers the player's input each frame into `Held`, turns it into the simulation's
//! `c_PI*` axes, plus the chat box, the radio, the wheel and the spawn screen's keys.

use super::*;

/// The screen when dead (`respawn`): ← and → go through the team's spawn groups, 1–5 pick the
/// kit (and do not reach the weapons until let go).
/// The wheel on foot (`c_PINextItem`/`c_PIPrevItem`): the server has no message for them (the soldier's message handler
/// takes only `enableItem`'s, 10–17), so the client picks the next (previous) weapon it carries, in its slots' order,
/// and asks for it as its digit would. One out of rounds that `cantSelectWhenNoAmmo` is passed over. Level 2 (the
/// original's client function is not found: which items it goes through is a guess). Scripts: `wheelup`, `wheeldown`.
// TODO: in a vehicle the wheel does nothing yet (the original maps it in Land.con and Air.con too).
pub(super) fn wheel_keys(g: &Game, now: &mut Held) {
    let up = now.keys.remove("wheelup") && !g.before.keys.contains("wheelup");
    let down = now.keys.remove("wheeldown") && !g.before.keys.contains("wheeldown");
    if up == down {
        return;
    }
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let Some(s) = p.soldier.filter(|_| p.alive).map(|i| &sim.vehicles[i]).filter(|s| s.entry.inside.is_none()) else { return };
    let Some(m) = s.soldier() else { return };
    let Some(cur) = m.weapon.as_ref().map(|w| w.template.item_index) else { return };
    let mut slots: Vec<i32> = m.stowed.iter().filter(|w| !(w.template.cant_select_empty && w.total_ammo() == 0)).map(|w| w.template.item_index).chain([cur]).collect();
    slots.sort();
    slots.dedup();
    let at = slots.iter().position(|&x| x == cur).unwrap_or(0);
    let n = slots.len();
    let next = slots[if up { (at + 1) % n } else { (at + n - 1) % n }];
    if next != cur && (1..=9).contains(&next) {
        now.keys.insert(next.to_string());
    }
}

pub(super) fn spawn_keys(g: &mut Game, now: &mut Held) {
    if g.dead() {
        let edge = |k: &str| now.keys.contains(k) && !g.before.keys.contains(k);
        let groups = g.session.sim.spawn_groups(g.player);
        let (mut group, mut kit) = g.spawn_choice;
        let step = edge("right") as i32 - edge("left") as i32;
        if step != 0 && !groups.is_empty() {
            let at = group.and_then(|x| groups.iter().position(|&y| y == x)).map_or(if step > 0 { -1 } else { 0 }, |i| i as i32);
            group = Some(groups[(at + step).rem_euclid(groups.len() as i32) as usize]);
        }
        if let Some(d) = (1..=5).find(|d| edge(&d.to_string())) {
            kit = d - 1;
        }
        if (group, kit) != g.spawn_choice {
            g.choose_spawn(group, kit);
        }
        for d in 1..=9u32 {
            if now.keys.contains(&d.to_string()) {
                g.spawn_taken.insert(d.to_string());
            }
        }
    }
    g.spawn_taken.retain(|k| now.keys.contains(k));
    now.keys.retain(|k| !g.spawn_taken.contains(k));
}

/// What the player is at, for the keys (`controls.rs`): the air map in a plane, the land one in anything else, the
/// infantry one on foot (dead too: its screen reads the arrows and the digits).
pub(super) fn control_context(g: &Game) -> crate::controls::Context {
    use crate::controls::{Context, Map};
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let inside = p.soldier.filter(|_| p.alive).and_then(|i| sim.vehicles[i].entry.inside);
    let map = match inside {
        Some(k) if sim.world.vehicle_category(&sim.vehicles[k].template) == 2 => Map::Air,
        Some(_) => Map::Land,
        None => Map::Infantry,
    };
    Context { map, digits: g.radio_menu.is_some() || g.dead(), dead: g.dead() }
}

// TODO: what the original does with a mouse axis on c_PIPitch/c_PIRoll (ControlMap::AxisMapping), against it with
// the same flick and our bench's plane: level 3, one unit of the mouse is one of the axis.

/// `BYTEFIELD_ARMS=<file>`: where each piece of the weapon in the hands is from the eyes, a line per piece and tick, in the
/// engine's axes (+Z ahead) and with its matrix's rows as D3D has them, to set against a reference run of the original.
pub(super) fn arms_log(tick: u64, piece: usize, t: &Transform) {
    use std::io::Write;
    static FILE: std::sync::OnceLock<Option<std::sync::Mutex<std::fs::File>>> = std::sync::OnceLock::new();
    let Some(file) = FILE.get_or_init(|| std::env::var_os("BYTEFIELD_ARMS").and_then(|p| std::fs::File::create(p).ok()).map(std::sync::Mutex::new)) else { return };
    let flip = |v: Vec3| [v.x, v.y, -v.z];
    let m = Mat3::from_quat(t.rotation);
    let (x, y, z) = (flip(m.x_axis), flip(m.y_axis), flip(-m.z_axis));
    let line = format!("tick={tick} piece={piece} t={:?} x={x:?} y={y:?} z={z:?}\n", flip(t.translation));
    if let Ok(mut f) = file.lock() {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `BYTEFIELD_FILM_LOG=<file>`: a line per tick with what happens to the player, from the simulation (with the AI playing
/// the player, the input axes say nothing), for a scorer to pick the windows of a recorded take worth
/// watching: alive, its vehicle and seat, where, how fast, the shots it fired this tick (projectiles fired by its soldier
/// or its vehicle, not alive the tick before), its kills so far, the enemies alive within 150 m, its height over the ground and its pose on foot.
pub(super) fn film_log(g: &Game) {
    use std::io::Write;
    static FILE: std::sync::OnceLock<Option<std::sync::Mutex<std::fs::File>>> = std::sync::OnceLock::new();
    let Some(file) = FILE.get_or_init(|| std::env::var_os("BYTEFIELD_FILM_LOG").and_then(|p| std::fs::File::create(p).ok()).map(std::sync::Mutex::new)) else { return };
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let soldier = p.soldier.filter(|_| p.alive);
    let inside = soldier.and_then(|i| sim.vehicles[i].entry.inside);
    let body = inside.or(soldier).map(|k| &sim.vehicles[k].craft.body);
    let pos = body.map_or([0.0; 3], |b| b.pos);
    let speed = body.map_or(0.0, |b| (b.vel[0].powi(2) + b.vel[1].powi(2) + b.vel[2].powi(2)).sqrt());
    let mine = |o: Option<usize>| o.is_some() && (o == soldier || o == inside);
    // (new: a slot and a muzzle not seen the tick before; a slot is reused once spent)
    static SEEN: std::sync::Mutex<Vec<(usize, [u32; 3])>> = std::sync::Mutex::new(Vec::new());
    let now: Vec<(usize, [u32; 3])> = sim.projectiles.iter().enumerate().filter(|(_, q)| q.alive() && mine(q.owner)).map(|(i, q)| (i, q.start.map(|c| (c as f32).to_bits()))).collect();
    let shots = SEEN.lock().map_or(0, |mut seen| {
        let n = now.iter().filter(|k| !seen.contains(k)).count();
        *seen = now;
        n
    });
    let enemies = if body.is_none() {
        0
    } else {
        sim.players
            .iter()
            .filter(|e| e.alive && e.team != p.team && e.team != 0)
            .filter_map(|e| e.soldier.map(|i| sim.vehicles[i].entry.inside.unwrap_or(i)))
            .filter(|&k| {
                let q = sim.vehicles[k].craft.body.pos;
                (q[0] - pos[0]).powi(2) + (q[1] - pos[1]).powi(2) + (q[2] - pos[2]).powi(2) < 150.0f64.powi(2) as _
            })
            .count()
    };
    let ground = sim.world.height_at(pos[0] as f64, pos[2] as f64).map_or(0.0, |h| pos[1] as f64 - h);
    let line = format!(
        "tick={} alive={} vehicle={} seat={} x={:.1} y={:.1} z={:.1} speed={:.1} shots={shots} kills={} deaths={} enemies={enemies} agl={ground:.1} pose={}\n",
        g.tick,
        soldier.is_some() as u8,
        inside.map_or("-", |k| sim.vehicles[k].template.as_str()),
        soldier.map_or(0, |i| sim.vehicles[i].entry.position),
        pos[0],
        pos[1],
        pos[2],
        speed,
        p.kills,
        p.deaths,
        // (on foot: 0 standing, 1 crouched, 2 lying; - in a vehicle or dead)
        soldier.filter(|_| inside.is_none()).and_then(|i| sim.vehicles[i].soldier()).map_or("-".to_string(), |m| m.pose().to_string()),
    );
    if let Ok(mut f) = file.lock() {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `BYTEFIELD_BOMB_CAM=chase|flyby[:<seconds>]` (to film a bombing run, with the AI flying the player): when a bomb leaves the
/// player's vehicle the view goes out (F10 or F12) for that long (4 s), to see it fall, and then back inside.
pub(super) fn bomb_cam(g: &mut Game) {
    static CONF: std::sync::OnceLock<Option<(Outside, u64)>> = std::sync::OnceLock::new();
    static UNTIL: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
    static SEEN: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
    let Some((view, ticks)) = *CONF.get_or_init(|| {
        let v = std::env::var("BYTEFIELD_BOMB_CAM").ok().filter(|v| !v.is_empty())?;
        let (name, secs) = v.split_once(':').unwrap_or((&v, "4"));
        let view = if name.eq_ignore_ascii_case("flyby") { Outside::FlyBy } else { Outside::Chase };
        Some((view, (secs.parse::<f64>().unwrap_or(4.0) / TICK) as u64))
    }) else {
        return;
    };
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let inside = p.soldier.filter(|_| p.alive).and_then(|i| sim.vehicles[i].entry.inside);
    let bombs: Vec<usize> = sim
        .projectiles
        .iter()
        .enumerate()
        .filter(|(_, q)| q.alive() && inside.is_some() && q.owner == inside && { let n = q.template.name.to_ascii_lowercase(); n.contains("bomb") && !n.contains("mortar") })
        .map(|(i, _)| i)
        .collect();
    let (Ok(mut seen), Ok(mut until)) = (SEEN.lock(), UNTIL.lock()) else { return };
    if bombs.iter().any(|i| !seen.contains(i)) {
        *until = Some(g.tick + ticks);
        (g.third_person, g.outside, g.nose, g.orbit) = (true, view, false, Vec2::ZERO);
    } else if until.is_some_and(|t| g.tick >= t) || (until.is_some() && inside.is_none()) {
        *until = None;
        g.third_person = false;
    }
    *seen = bombs;
}

/// `BYTEFIELD_STATE=<file>`: a line per tick with what the player does, for a test that plays through the devices
/// (`python/tests/base/client/test_controls.py`): where it is, what it is in, its look and the axes that went to the simulation.
pub(super) fn state_log(g: &Game, a: &[(usize, f32)]) {
    use std::io::Write;
    static FILE: std::sync::OnceLock<Option<std::sync::Mutex<std::fs::File>>> = std::sync::OnceLock::new();
    let Some(file) = FILE.get_or_init(|| std::env::var_os("BYTEFIELD_STATE").and_then(|p| std::fs::File::create(p).ok()).map(std::sync::Mutex::new)) else { return };
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let Some(s) = p.soldier.map(|i| &sim.vehicles[i]) else { return };
    let inside = s.entry.inside.map(|k| &sim.vehicles[k]);
    let axis = |n: usize| a.iter().find(|x| x.0 == n).map_or(0.0, |x| x.1);
    let line = format!(
        "tick={} map={:?} vehicle={} seat={} seat_head={:.2} x={:.3} z={:.3} head_yaw={:.4} pitch={:.4} vx={:.3} vz={:.3} yaw_axis={} pitch_axis={} roll_axis={} throttle={} mouse_x={} mouse_y={} fire={} upper={} upper_t={:.3} weapon={} vy={:.2} up={:.2},{:.2},{:.2} fwd={:.2},{:.2},{:.2}\n",
        g.tick,
        control_context(g).map,
        inside.map_or("-", |v| v.template.as_str()),
        s.entry.position,
        g.seat_head,
        s.craft.body.pos[0],
        s.craft.body.pos[2],
        s.soldier().map_or(0.0, |m| m.head_yaw),
        s.soldier().map_or(0.0, |m| m.pitch),
        inside.map_or(0.0, |v| v.craft.body.pos[0]),
        inside.map_or(0.0, |v| v.craft.body.pos[2]),
        axis(axis::YAW),
        axis(1),
        axis(2),
        axis(axis::THROTTLE),
        axis(axis::MOUSE_X),
        axis(axis::MOUSE_Y),
        axis(axis::FIRE),
        // (the upper body's animation state, and its weapon's, the zoom and the grenade in the first person)
        s.soldier().and_then(|m| m.upper.state.map(|i| m.machine.states[i].name.clone())).unwrap_or_default(),
        s.soldier().map_or(0.0, |m| m.upper.time),
        s.soldier().and_then(|m| m.upper.state.and_then(|i| m.machine.states[i].other.iter().find(|o| o.0 == "c_asmweaponstate").map(|o| o.1.clone()))).unwrap_or_default(),
        // (the vehicle's height and attitude: its up and forward axes)
        inside.map_or(0.0, |v| v.craft.body.pos[1]),
        inside.map_or(0.0, |v| v.craft.body.rows[1][0]),
        inside.map_or(0.0, |v| v.craft.body.rows[1][1]),
        inside.map_or(0.0, |v| v.craft.body.rows[1][2]),
        inside.map_or(0.0, |v| v.craft.body.rows[2][0]),
        inside.map_or(0.0, |v| v.craft.body.rows[2][1]),
        inside.map_or(0.0, |v| v.craft.body.rows[2][2]),
    );
    if let Ok(mut f) = file.lock() {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `game.setAirKeyboardSensitivity`, `setLandSeaKeyboardSensitivity` (0.5 by default): what the keys give of the axes
/// that steer. Level 3: a factor, with the throttle left out.
// TODO: what the original does with it.
pub(super) fn keyboard_sensitivity(g: &Game, a: &mut [(usize, f32)]) {
    let map = control_context(g).map;
    if !map.has_keyboard() {
        return;
    }
    let factor = crate::controls::current().keyboard(map) / 0.5;
    for (_, v) in a.iter_mut().filter(|(n, _)| matches!(*n, axis::YAW | 1 | 2)) {
        *v = bytefield_sim::input::through_action((*v * factor).clamp(-1.0, 1.0));
    }
}

/// `c_PIMouseLook`'s input.
pub(super) const MOUSE_LOOK: usize = 11;

/// In a plane the original's `AirPlayerInputControlMap` (`Air.con`): the pilot flies with the mouse (forward as the up
/// arrow, diving: `setAirMouseInvert 1`; to the right as the right arrow) unless the left Shift is held (`c_PIMouseLook`: then it looks, as before);
/// Space fires and the numeric pad's 0 is the alternative fire, there instead of jumping.
pub(super) fn air_controls(g: &Game, now: &Held, a: &mut [(usize, f32)]) {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let Some(s) = p.soldier.filter(|_| p.alive).map(|i| &sim.vehicles[i]) else { return };
    let Some(k) = s.entry.inside else { return };
    if sim.world.vehicle_category(&sim.vehicles[k].template) != 2 {
        return;
    }
    let held = |k: &str| now.keys.contains(k);
    // (the mouse as it came, without `--invert-y`, then the map's: `setAirMouseInvert 1` is forward diving, as ↑)
    let c = crate::controls::current();
    let forward = if c.invert(crate::controls::Map::Air) { 1.0 } else { -1.0 };
    let mouse = Vec2::new(if c.uses_axis(crate::controls::Map::Air, 0) { now.mouse.x } else { 0.0 }, if c.uses_axis(crate::controls::Map::Air, 1) { now.mouse.y * g.mouse_units().y.signum() * forward } else { 0.0 });
    let pilot = s.entry.position == 0 && !held("shift");
    for (axis_n, v) in a.iter_mut() {
        match *axis_n {
            1 if pilot => *v = bytefield_sim::input::through_action((*v - mouse.y).clamp(-1.0, 1.0)),
            2 if pilot => *v = bytefield_sim::input::through_action((*v + mouse.x).clamp(-1.0, 1.0)),
            axis::MOUSE_X | axis::MOUSE_Y if pilot => *v = 0.0,
            // (the pilot's camera turns while it is held, `toggleMouseLook`, and goes back after)
            MOUSE_LOOK => *v = held("shift") as i32 as f32,
            axis::FIRE if held("space") => *v = 1.0,
            axis::ALT_FIRE if held("num0") => *v = 1.0,
            axis::ACTION => *v = 0.0,
            _ => {}
        }
    }
}

/// The chat box, on the original server: K or L opens it, the keys type into it (Latin-1, up to the line the server
/// keeps), Backspace takes one off, Enter says it and Esc drops it (after the menu's `escape`, which leaves it be, and `gather`, which does not free
/// the mouse for it).
pub(crate) fn chat_box(mut g: ResMut<Game>, keys: Res<ButtonInput<KeyCode>>, buttons: Res<ButtonInput<MouseButton>>, mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>, menu: Option<Res<crate::menu::Menu>>) {
    use bevy::input::keyboard::Key;
    // the console (º, `c_GIConsole`): a line under the radio's, for `game.suicide`
    if g.chat_box.is_none() && g.script.is_none() && menu.is_none() && keys.just_pressed(KeyCode::Backquote) {
        typed.clear();
        g.chat_box = Some((false, String::new()));
        g.console = true;
        return;
    }
    if (g.net.is_none() && !g.on_orig() && g.note_to.is_none() && !g.console) || g.script.is_some() || menu.is_some() {
        typed.clear();
        return;
    }
    let g = &mut *g;
    let Some((to_all, text)) = g.chat_box.as_mut() else {
        typed.clear();
        let c = crate::controls::current();
        let (all, team) = (c.just(&keys, &buttons, "k"), c.just(&keys, &buttons, "l"));
        if all || team {
            g.chat_box = Some((all, String::new()));
        }
        return;
    };
    for k in typed.read() {
        if !k.state.is_pressed() {
            continue;
        }
        match &k.logical_key {
            Key::Character(c) => {
                for ch in c.chars().filter(|ch| !ch.is_control() && (*ch as u32) < 256) {
                    if text.chars().count() < bytefield_server::CHAT_LINE {
                        text.push(ch);
                    }
                }
            }
            Key::Space if text.chars().count() < bytefield_server::CHAT_LINE => text.push(' '),
            Key::Backspace => {
                text.pop();
            }
            Key::Enter => {
                let (to_all, text) = (*to_all, std::mem::take(text));
                g.chat_box = None;
                if std::mem::take(&mut g.console) {
                    console(g, &text);
                    return;
                }
                if let Some(dir) = g.note_to.take() {
                    crate::tickets::note(&dir, &text);
                    return;
                }
                if let Some(n) = g.net.as_mut() {
                    n.mirror.say(&text, to_all);
                }
                return;
            }
            Key::Escape => {
                g.chat_box = None;
                g.note_to = None;
                g.console = false;
                return;
            }
            _ => {}
        }
    }
}

/// A console line (`Console.con`'s names, case aside): `game.suicide` (alias `suicide`) kills the player's soldier, as
/// the original's server does. Level 3: only that one; the original's console has every `.con` command.
// TODO: on a server (ours or the original's) the suicide is not sent; the original's console panel and its history.
pub(super) fn console(g: &mut Game, line: &str) {
    let cmd = line.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    match cmd.as_str() {
        "game.suicide" | "suicide" if g.net.is_none() && !g.on_orig() => {
            let k = g.player;
            g.session.sim.suicide(k);
            g.record_event("suicide".into());
        }
        "" => {}
        _ => info!("console: «{line}» is not known"),
    }
}

/// The devices, every frame: the mouse adds up until the next tick; a key pressed and let go
/// between two ticks still counts once. The left button also takes the mouse, Esc frees it.
pub(super) fn gather(mut g: ResMut<Game>, keys: Res<ButtonInput<KeyCode>>, buttons: Res<ButtonInput<MouseButton>>, motion: Res<AccumulatedMouseMotion>, scroll: Res<AccumulatedMouseScroll>, mut cursor: Query<&mut CursorOptions>, menu: Option<Res<crate::menu::Menu>>) {
    if g.script.is_some() {
        return;
    }
    // the menu over the round takes the mouse and the keys
    g.menu_open = menu.is_some();
    if menu.is_some() {
        if let Ok(mut c) = cursor.single_mut()
            && c.grab_mode != CursorGrabMode::None
        {
            c.grab_mode = CursorGrabMode::None;
            c.visible = true;
        }
        return;
    }
    // (the chat box takes the keys)
    if g.chat_box.is_some() {
        return;
    }
    let grabbed = cursor.single().is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    if let Ok(mut c) = cursor.single_mut() {
        if buttons.just_pressed(MouseButton::Left) && !grabbed && !g.dead() {
            c.grab_mode = CursorGrabMode::Locked;
            c.visible = false;
            return;
        }
        if keys.just_pressed(KeyCode::Escape) {
            c.grab_mode = CursorGrabMode::None;
            c.visible = true;
        }
    }
    if grabbed {
        let k = g.mouse_units();
        g.mouse += motion.delta * k;
    }
    // (the wheel, `IDAxis_2`, is a tap: [`wheel_keys`] by default)
    use crate::controls::Input;
    let mut tapped = Vec::new();
    crate::controls::current().each(control_context(&g), |i, n| {
        let now = match i {
            Input::Key(k) => keys.just_pressed(k),
            Input::Button(b) => grabbed && buttons.just_pressed(b),
            Input::WheelUp => grabbed && scroll.delta.y > 0.0,
            Input::WheelDown => grabbed && scroll.delta.y < 0.0,
            Input::MouseAxis(_) => false,
        };
        if now {
            tapped.push(n);
        }
    });
    g.tapped.extend(tapped.into_iter().map(String::from));
}

/// What is held in this tick: the script's, or the devices'.
pub(super) fn held_now(g: &mut Game, keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>) -> Held {
    if let Some(script) = g.script.as_ref() {
        let mut h = Held::default();
        for s in script.iter().filter(|s| (s.from..s.to).contains(&g.tick)) {
            h.keys.extend(s.keys.iter().cloned());
            h.mouse += s.mouse;
        }
        return h;
    }
    let mut h = Held { keys: std::mem::take(&mut g.tapped), mouse: std::mem::take(&mut g.mouse) };
    if g.menu_open {
        return Held::default();
    }
    if g.chat_box.is_some() {
        return h;
    }
    use crate::controls::Input;
    crate::controls::current().each(control_context(g), |i, n| {
        let now = match i {
            Input::Key(k) => keys.pressed(k),
            Input::Button(b) => buttons.pressed(b),
            Input::WheelUp | Input::WheelDown | Input::MouseAxis(_) => false,
        };
        if now {
            h.keys.insert(n.to_string());
        }
    });
    h
}

/// The radio: F1–F7 open a menu (the same again, or F8, closes it), F8 with none open
/// shows or hides the row of headings; while one is open
/// the digits pick from it, and do not reach the weapons until let go.
pub(super) fn radio_keys(g: &mut Game, now: &mut Held) {
    let edge = |k: &str| now.keys.contains(k) && !g.before.keys.contains(k);
    for f in 1..=8u32 {
        if edge(&format!("f{f}")) {
            if f == 8 && g.radio_menu.is_none() {
                g.radio_icons = !g.radio_icons;
            }
            g.radio_menu = if g.radio_menu == Some(f) || f == 8 { None } else { Some(f) };
        }
    }
    if let Some(f) = g.radio_menu {
        let picked = (1..=8u32).find(|d| edge(&d.to_string()));
        for d in 1..=9u32 {
            if now.keys.contains(&d.to_string()) {
                g.radio_taken.insert(d.to_string());
            }
        }
        if let Some(d) = picked {
            g.radio_menu = None;
            // (on a server, it says it there, and the copy hears it with the snapshots)
            match g.net.as_mut() {
                Some(net) => net.mirror.radio_pick(f, d),
                None => {
                    g.session.sim.radio_pick(g.player, f, d);
                }
            }
        }
    }
    g.radio_taken.retain(|k| now.keys.contains(k));
    now.keys.retain(|k| !g.radio_taken.contains(k));
}
