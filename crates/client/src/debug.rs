//! What a player sees to say where something went wrong: Ctrl+F12 shows an overlay with the frame
//! rate, the tick, where the soldier is and what it drives; Ctrl+F11 sets a mark (`MARCA n` in the
//! log with the whole state, a ticket's folder with its screenshot, `tickets.rs`, and a line to write a
//! note in, the round going on). Every few seconds the state goes to the log too (`scripts/bytefield`
//! keeps it in `data/out/logs/`).

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::play::Game;
use bytefield_sim::craft::Kind;

/// Seconds between two state lines in the log.
const LOG_EVERY: f64 = 5.0;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Debug>().add_systems(Update, (setup.run_if(resource_added::<Game>), update.run_if(resource_exists::<Game>)).chain());
        if let Some(path) = std::env::var_os("BYTEFIELD_FRAMES") {
            match std::fs::File::create(&path) {
                Ok(f) => {
                    app.insert_resource(FrameLog { out: std::io::BufWriter::new(f), spent: [0.0; 2], writes: 0, lines: 0 }).add_systems(Last, frame_log.run_if(resource_exists::<Game>));
                }
                Err(e) => warn!("BYTEFIELD_FRAMES: {e}"),
            }
        }
    }
}

#[derive(Resource, Default)]
struct Debug {
    shown: bool,
    marks: u32,
    fps: f32,
    last_log: f64,
    auto_marked: bool,
}

#[derive(Component)]
struct Overlay;

/// `BYTEFIELD_FRAMES=<file>`: a line per frame, for a performance run.
#[derive(Resource)]
struct FrameLog {
    out: std::io::BufWriter<std::fs::File>,
    spent: [f64; 2],
    writes: u64,
    lines: u32,
}

/// The frame's milliseconds (real ones: `Time<Virtual>` clamps a long frame), the tick, the milliseconds of `step` and
/// of `sync` in this frame, the skinned meshes written, the players alive and the projectiles flying.
fn frame_log(real: Res<Time<Real>>, g: Res<Game>, mut log: ResMut<FrameLog>) {
    use std::io::Write;
    let writes = crate::play::SKIN_WRITES.load(std::sync::atomic::Ordering::Relaxed);
    let sim = &g.session.sim;
    let (alive, flying) = (sim.players.iter().filter(|p| p.alive).count(), sim.projectiles.iter().filter(|p| p.alive()).count());
    let line = format!("{:.3}\t{}\t{:.3}\t{:.3}\t{}\t{}\t{}\n", real.delta_secs_f64() * 1e3, g.tick, g.spent_ms[0] - log.spent[0], g.spent_ms[1] - log.spent[1], writes - log.writes, alive, flying);
    log.spent = g.spent_ms;
    log.writes = writes;
    log.lines += 1;
    let every = log.lines.is_multiple_of(64);
    let _ = log.out.write_all(line.as_bytes());
    if every {
        let _ = log.out.flush();
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(0.6, 1.0, 0.6)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node { position_type: PositionType::Absolute, right: Val::Px(12.0), top: Val::Px(40.0), padding: UiRect::all(Val::Px(6.0)), ..default() },
        Visibility::Hidden,
        Overlay,
    ));
}

/// The state in a few lines: what the overlay shows and a mark logs.
pub fn state(g: &Game, fps: f32) -> Vec<String> {
    let sim = &g.session.sim;
    let p = &sim.players[g.player];
    let mut out = vec![format!("fps {fps:.0}   tick {}   t {:.1} s   {}", g.tick, sim.clock.time, if g.third_person { "3P" } else { "1P" })];
    let bots = sim.players.iter().filter(|q| q.ai);
    let (n, alive) = bots.fold((0, 0), |(n, a), q| (n + 1, a + q.alive as usize));
    out.push(format!("player {} team {}   bots {alive}/{n} alive", g.player, p.team));
    let Some(s) = p.soldier.filter(|_| p.alive).map(|i| (i, &sim.vehicles[i])) else {
        out.push(format!("dead: spawns in {:.1} s", p.spawn_time));
        return out;
    };
    let (i, s) = s;
    let b = &s.craft.body;
    let f = b.rows[2];
    let yaw = f[0].atan2(f[2]).to_degrees();
    out.push(format!("soldier #{i} {}   pos {:.1} {:.1} {:.1}   heading {yaw:.0}°", s.template, b.pos[0], b.pos[1], b.pos[2]));
    if let Some(k) = s.entry.inside {
        let v = &sim.vehicles[k];
        let vb = &v.craft.body;
        let speed = (vb.vel[0].powi(2) + vb.vel[1].powi(2) + vb.vel[2].powi(2)).sqrt() * 3.6;
        let hp = v.armor.as_ref().map_or(0.0, |a| a.hp);
        out.push(format!("vehicle #{k} {} seat {}   {speed:.0} km/h   health {hp:.0}", v.template, s.entry.position + 1));
        out.push(format!("  pos {:.1} {:.1} {:.1}   up {:.2} {:.2} {:.2}", vb.pos[0], vb.pos[1], vb.pos[2], vb.rows[1][0], vb.rows[1][1], vb.rows[1][2]));
        for part in &v.craft.parts {
            if let Kind::Engine(e) = &part.kind {
                out.push(format!("  engine {}: rpm {:.2} gear {} {}", part.name, e.rpm, e.gear, if e.braking { "braking" } else { "" }));
            }
        }
        let touching = v.contacts.iter().filter(|c| c.spring.is_some()).count();
        out.push(format!("  contacts {} (with a spring {touching})", v.contacts.len()));
    }
    out
}

fn update(mut commands: Commands, mut g: ResMut<Game>, keys: Res<ButtonInput<KeyCode>>, buttons: Res<ButtonInput<MouseButton>>, time: Res<Time>, mut d: ResMut<Debug>, mut q: Query<(&mut Text, &mut Visibility), With<Overlay>>) {
    let dt = time.delta_secs();
    if dt > 0.0 {
        d.fps = if d.fps == 0.0 { 1.0 / dt } else { d.fps * 0.95 + 0.05 / dt };
    }
    // (with Ctrl: F9–F12 alone are the original's views, `play.rs`)
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if ctrl && keys.just_pressed(KeyCode::F12) {
        d.shown = !d.shown;
    }
    let now = time.elapsed_secs_f64();
    if now - d.last_log >= LOG_EVERY {
        d.last_log = now;
        info!("state: {}", state(&g, d.fps).join(" | "));
    }
    // Impr Pant (`c_PIScreenShot`): a screenshot in `shots/` (`first_run::out_dir`)
    if crate::controls::current().just(&keys, &buttons, "print") {
        let dir = bytefield_content::first_run::out_dir("shots");
        let _ = std::fs::create_dir_all(&dir);
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let path = dir.join(format!("{}-{secs}.png", g.session.sim.world.level.name));
        info!("screenshot {}", path.display());
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    // BYTEFIELD_MARK_TICK=<n>: a mark of its own at that tick (a ticket without a person, for its replay's test)
    let auto = !d.auto_marked && std::env::var("BYTEFIELD_MARK_TICK").ok().and_then(|n| n.parse::<u64>().ok()).is_some_and(|n| g.tick >= n);
    if (ctrl && keys.just_pressed(KeyCode::F11)) || auto {
        d.auto_marked |= auto;
        d.marks += 1;
        let mut lines = state(&g, d.fps);
        lines.insert(0, g.session.sim.world.level.name.clone());
        let dir = crate::tickets::open(d.marks, &lines).unwrap_or_else(|e| {
            warn!("tickets: {e}");
            std::env::temp_dir()
        });
        crate::replay::write_ticket(&dir, &g);
        let path = dir.join("screenshot.png");
        warn!("MARK {}: {} | screenshot {}", d.marks, lines.join(" | "), path.display());
        // (with a script nobody writes; and an open chat line is not overwritten)
        if g.chat_box.is_none() && !g.scripted() {
            g.chat_box = Some((false, String::new()));
            g.note_to = Some(dir);
        }
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        // the projectiles in flight (frozen ones: their speed, age and time to live tell why)
        let sim = &g.session.sim;
        for (i, p) in sim.projectiles.iter().enumerate().filter(|(_, p)| p.alive()) {
            let v = (p.vel[0] * p.vel[0] + p.vel[1] * p.vel[1] + p.vel[2] * p.vel[2]).sqrt();
            warn!(
                "  projectile {i}: {} pos {:.1} {:.1} {:.1} vel {:.1} m/s age {:.2} ttl {:.2} end {:?} rigid {} owner {:?} from {:.1} {:.1} {:.1}",
                p.template.name, p.pos[0], p.pos[1], p.pos[2], v, p.age, p.ttl, p.end.is_some(), p.rigid.is_some(), p.owner, p.start[0], p.start[1], p.start[2]
            );
        }
    }
    for (mut text, mut vis) in &mut q {
        *vis = if d.shown { Visibility::Inherited } else { Visibility::Hidden };
        if d.shown {
            let mut lines = state(&g, d.fps);
            lines.push(format!("marks: {} (Ctrl+F11)", d.marks));
            text.0 = lines.join("\n");
        }
    }
}
