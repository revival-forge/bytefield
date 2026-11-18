//! The tester's tickets, one folder each in `tickets/` (`first_run::out_dir`): Ctrl+F11 leaves there the screenshot, the state and a
//! note (`debug.rs`), and a panic leaves what it said and warns on the next launch.

use std::path::PathBuf;

use bevy::prelude::*;

/// The commit of this binary (a `+`: with changes not committed).
pub const COMMIT: &str = env!("BYTEFIELD_COMMIT");
/// In `tickets/`: the crash nobody has been told about yet.
const UNSEEN: &str = "crash-unseen.txt";

fn base() -> PathBuf {
    bytefield_content::first_run::out_dir("tickets")
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// A new ticket's folder with its `ticket.txt`, what's left of it being the screenshot and the note.
pub fn open(mark: u32, state: &[String]) -> std::io::Result<PathBuf> {
    let dir = base().join(format!("{}-mark{mark}", stamp()));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("ticket.txt"), format!("mark {mark}\ncommit {COMMIT}\n{}\n", state.join("\n")))?;
    Ok(dir)
}

/// What the tester wrote about it.
pub fn note(dir: &std::path::Path, text: &str) {
    use std::io::Write;
    let file = std::fs::OpenOptions::new().append(true).open(dir.join("ticket.txt"));
    if let Err(e) = file.and_then(|mut f| writeln!(f, "note: {text}")) {
        warn!("tickets: {e}");
    }
}

/// From here on a panic also leaves a ticket, on top of what it prints.
pub fn watch_panics() {
    let before = std::panic::take_hook();
    // (one per run: a system that panics may do so every frame, `log_system_error` lets the round go on)
    let told = std::sync::atomic::AtomicBool::new(false);
    std::panic::set_hook(Box::new(move |info| {
        before(info);
        if told.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let text = format!("crash\ncommit {COMMIT}\n{info}\n\n{}\n", std::backtrace::Backtrace::force_capture());
        crate::userlog::append(&text);
        let dir = base().join(format!("{}-crash", stamp()));
        if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(dir.join("ticket.txt"), &text).is_ok() {
            let _ = std::fs::write(base().join(UNSEEN), dir.display().to_string());
        }
    }));
}

/// The folder of a crash in the last run, once: to say so on this one.
fn last_crash() -> Option<String> {
    let marker = base().join(UNSEEN);
    let dir = std::fs::read_to_string(&marker).ok()?;
    let _ = std::fs::remove_file(marker);
    Some(dir)
}

/// The warning on the screen, and how long it stays.
#[derive(Component)]
struct Warning(f32);

const WARNING_SECONDS: f32 = 20.0;

pub struct TicketsPlugin;

impl Plugin for TicketsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, warn_of_crash).add_systems(Update, fade);
    }
}

fn warn_of_crash(mut commands: Commands) {
    let Some(dir) = last_crash() else { return };
    warn!("CRASH in the last game: {dir}");
    commands.spawn((
        Text::new(format!("{}\n{dir}", crate::lang::ours("TICKET_LAST_CRASH").unwrap_or("The last game hit an error. It is written down in:"))),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        TextColor(Color::srgb(1.0, 0.85, 0.4)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), bottom: Val::Px(12.0), padding: UiRect::all(Val::Px(8.0)), ..default() },
        GlobalZIndex(1000),
        Warning(WARNING_SECONDS),
    ));
}

fn fade(mut commands: Commands, time: Res<Time<Real>>, mut q: Query<(Entity, &mut Warning)>) {
    for (e, mut w) in &mut q {
        w.0 -= time.delta_secs();
        if w.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
