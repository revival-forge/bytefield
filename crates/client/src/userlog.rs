//! The log a player has without a terminal: what the client logs (`info!`, `warn!`, `error!`) goes to
//! `out_dir("logs")/latest.log` too, the run before it to `previous.log`, and a crash's text after it. For a
//! ticket or a report: «send latest.log». Not in checks (`--screenshot`, `--script`, `--steps`: tools run them
//! by the dozen, at once) nor under `scripts/bytefield`, which keeps everything it prints already (`BYTEFIELD_NO_LOG`).

use bevy::log::tracing_subscriber::{self, fmt::MakeWriter, Layer};
use bevy::log::{BoxedLayer, LogPlugin};
use bevy::prelude::default;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

/// Opens the log (once), moving the last one aside; `false` if there is none to write.
pub fn open(check: bool) -> bool {
    if check || std::env::var_os("BYTEFIELD_NO_LOG").is_some() {
        return false;
    }
    if FILE.get().is_some() {
        return true;
    }
    let dir = bytefield_content::first_run::out_dir("logs");
    let latest = dir.join("latest.log");
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    // (a link is `scripts/bytefield`'s, to the log of its last run, which a player reports with: left as it is)
    if std::fs::symlink_metadata(&latest).is_ok_and(|m| m.file_type().is_symlink()) {
        return false;
    }
    let _ = std::fs::rename(&latest, dir.join("previous.log"));
    let Ok(mut f) = std::fs::File::create(&latest) else { return false };
    let _ = writeln!(f, "bytefield {} ({}), {}", crate::tickets::COMMIT, std::env::consts::OS, chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    let _ = writeln!(f, "args: {}", std::env::args().skip(1).collect::<Vec<_>>().join(" "));
    FILE.set(Mutex::new(f)).is_ok()
}

/// Appends `text` as it is (the crash's, which the log layer does not see).
pub fn append(text: &str) {
    if let Some(f) = FILE.get() {
        if let Ok(mut f) = f.lock() {
            let _ = f.write_all(text.as_bytes());
            let _ = f.flush();
        }
    }
}

struct Writer;

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        append(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Writer {
    type Writer = Writer;
    fn make_writer(&'a self) -> Writer {
        Writer
    }
}

fn layer(_: &mut bevy::app::App) -> Option<BoxedLayer> {
    FILE.get()?;
    Some(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(Writer).boxed())
}

/// The log plugin, writing to the file as well when `open` opened it.
pub fn plugin() -> LogPlugin {
    LogPlugin { custom_layer: layer, ..default() }
}
