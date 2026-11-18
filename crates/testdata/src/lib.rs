//! The content layers a test runs on, and one way to ask for them.
//!
//! The engine reads the user's own install: the base game, its expansions, the mods they chose and
//! the packs laid over it. A test on that data says which layers it needs with [`require!`] and is
//! skipped, saying so, when one is missing or not wanted:
//!
//! ```ignore
//! #[test]
//! fn the_m2a3_of_bragg() {
//!     let game = bytefield_testdata::require!(Dc);
//!     // ... load a level of DC_Final from `game`
//! }
//! ```
//!
//! | Layer | Folders under `Mods/` | Who has it |
//! |---|---|---|
//! | (none: synthetic) | fixtures in the repo | everyone, also public CI |
//! | `base` | `bf1942` | whoever owns the game |
//! | `xpack1`, `xpack2` | `XPack1`, `XPack2` | whoever owns the expansions |
//! | `dc`, `dc07` | `DC_Final`, `DesertCombat` | whoever installed Desert Combat |
//! | `gc` | `GCMOD` | whoever installed Galactic Conquest |
//! | `ref` | `DC_Extended`, `GC_Redux` | reference mods: almost nobody |
//! | `hd` | the packs of `BYTEFIELD_HD` | whoever installed one |
//!
//! `BYTEFIELD_LAYERS` picks the layers even when more are installed: a comma list of the names
//! above, `xpack` for both expansions, or `all`. Unset, every installed layer but `ref` and `hd` is
//! used, so the daily run never reaches a reference mod or a pack (they may live on another disk).
//! `BYTEFIELD_GAME` points at another install (`data/game` by default). `BYTEFIELD_SKIP_LOG=<file>`
//! appends one line per skipped test (`layer<TAB>test`), for the test scripts to count.

use std::io::Write;
use std::path::{Path, PathBuf};

/// One layer of content: what a test on the real data may need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Base,
    Xpack1,
    Xpack2,
    Dc,
    Dc07,
    Gc,
    Ref,
    Hd,
}

impl Layer {
    pub const ALL: [Layer; 8] = [Layer::Base, Layer::Xpack1, Layer::Xpack2, Layer::Dc, Layer::Dc07, Layer::Gc, Layer::Ref, Layer::Hd];

    /// The name used in `BYTEFIELD_LAYERS` and in the skip messages.
    pub fn name(self) -> &'static str {
        match self {
            Layer::Base => "base",
            Layer::Xpack1 => "xpack1",
            Layer::Xpack2 => "xpack2",
            Layer::Dc => "dc",
            Layer::Dc07 => "dc07",
            Layer::Gc => "gc",
            Layer::Ref => "ref",
            Layer::Hd => "hd",
        }
    }

    /// The mod folders of the layer, under the install's `Mods/` (none for `hd`).
    pub fn mods(self) -> &'static [&'static str] {
        match self {
            Layer::Base => &["bf1942"],
            Layer::Xpack1 => &["XPack1"],
            Layer::Xpack2 => &["XPack2"],
            Layer::Dc => &["DC_Final"],
            Layer::Dc07 => &["DesertCombat"],
            Layer::Gc => &["GCMOD"],
            Layer::Ref => &["DC_Extended", "GC_Redux"],
            Layer::Hd => &[],
        }
    }

    /// The layer a mod folder belongs to (case-insensitive), if any.
    pub fn of_mod(mod_name: &str) -> Option<Layer> {
        Layer::ALL.into_iter().find(|l| l.mods().iter().any(|m| m.eq_ignore_ascii_case(mod_name)))
    }

    /// Whether the layer is on this machine: its mod folders under `Mods/` (every expansion and mod
    /// also needs the base game), or for `hd`, a pack in `BYTEFIELD_HD`.
    pub fn installed(self) -> bool {
        if self == Layer::Hd {
            return std::env::var_os("BYTEFIELD_HD").is_some_and(|v| std::env::split_paths(&v).any(|p| p.is_dir()));
        }
        let mods = game().join("Mods");
        let base = self == Layer::Base || mods.join("bf1942").is_dir();
        base && self.mods().iter().all(|m| mods.join(m).is_dir())
    }

    /// Whether the run wants the layer: `BYTEFIELD_LAYERS`, or by default all but `ref` and `hd`.
    pub fn wanted(self) -> bool {
        match std::env::var("BYTEFIELD_LAYERS") {
            Ok(list) => parse(&list).contains(&self),
            Err(_) => !matches!(self, Layer::Ref | Layer::Hd),
        }
    }

    /// Wanted and installed.
    pub fn available(self) -> bool {
        self.wanted() && self.installed()
    }
}

/// The layers of a `BYTEFIELD_LAYERS` value (unknown names are ignored).
pub fn parse(list: &str) -> Vec<Layer> {
    let mut out = vec![];
    for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match name.to_ascii_lowercase().as_str() {
            "all" => out.extend(Layer::ALL),
            "xpack" => out.extend([Layer::Xpack1, Layer::Xpack2]),
            n => out.extend(Layer::ALL.into_iter().filter(|l| l.name() == n)),
        }
    }
    out
}

/// The install the tests read: `BYTEFIELD_GAME`, or the repo's `data/game`.
pub fn game() -> PathBuf {
    match std::env::var_os("BYTEFIELD_GAME") {
        Some(g) => PathBuf::from(g),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/game"),
    }
}

/// The install when every layer is available; otherwise `None`, after saying which layer is
/// missing (on stderr, and in `BYTEFIELD_SKIP_LOG`). Use [`require!`] in a test.
pub fn require(layers: &[Layer]) -> Option<PathBuf> {
    match layers.iter().find(|l| !l.available()) {
        None => Some(game()),
        Some(missing) => {
            skipped(missing.name());
            None
        }
    }
}

/// Says that the running test is skipped for want of `what` (a layer name, or anything else a test
/// needs from outside the repo: traces, an extracted folder).
pub fn skipped(what: &str) {
    let test = std::thread::current().name().unwrap_or("?").to_string();
    eprintln!("skipped: {test} needs {what}");
    if let Some(log) = std::env::var_os("BYTEFIELD_SKIP_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
            // one write per line: the test threads append side by side
            let _ = f.write_all(format!("{what}\t{test}\n").as_bytes());
        }
    }
}

/// The install when the base game is available (`require(&[Layer::Base])`).
pub fn base() -> Option<PathBuf> {
    require(&[Layer::Base])
}

/// The install when `mod_name`'s layer (and the base game) is available; a mod outside the layers
/// (one of our own, another the user dropped in) only needs its folder and the base game.
pub fn with_mod(mod_name: &str) -> Option<PathBuf> {
    match Layer::of_mod(mod_name) {
        Some(layer) => require(&[Layer::Base, layer]),
        None if game().join("Mods").join(mod_name).is_dir() => base(),
        None => {
            skipped(mod_name);
            None
        }
    }
}

/// The install as a machine with only the available layers would have it: a folder under
/// `target/testdata/` whose entries link to the install's, with a `Mods/` holding only the
/// available layers' mods (and mods outside the layers, as one the user dropped in). For a test that
/// scans `Mods/` (the menu, content detection): it never reaches a reference mod on another disk,
/// and `BYTEFIELD_LAYERS=base` shows it a base-only install. Made once per set of layers.
///
/// What the engine caches beside the install (`<game>/../out`, the AI's made maps in `pathgen`) is
/// linked beside the view too (`target/testdata/out`), so the view also serves to load levels.
/// Elsewhere than on Unix, the install itself.
#[cfg(not(unix))]
pub fn install_view() -> Option<PathBuf> {
    base()
}

#[cfg(unix)]
pub fn install_view() -> Option<PathBuf> {
    let game = game();
    let mods = std::fs::read_dir(game.join("Mods")).ok()?;
    let keep: Vec<String> = mods
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|m| Layer::of_mod(m).map_or(true, |l| l.available()))
        .collect();
    let mut tag: Vec<&str> = Layer::ALL.iter().filter(|l| l.available()).map(|l| l.name()).collect();
    tag.sort();
    let views = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/testdata");
    let view = views.join(if tag.is_empty() { "none".into() } else { tag.join("-") });
    if let Some(out) = std::fs::canonicalize(&game).ok().and_then(|g| g.parent().map(|p| p.join("out"))).filter(|o| o.is_dir()) {
        let _ = std::fs::create_dir_all(&views);
        // (another thread or process may have made it first)
        let _ = std::os::unix::fs::symlink(out, views.join("out"));
    }
    if view.join("Mods").is_dir() {
        return Some(view);
    }
    // built aside and renamed: test threads and processes may race to make it
    let tmp = view.with_extension(format!("tmp{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
    std::fs::create_dir_all(tmp.join("Mods")).ok()?;
    let abs = std::fs::canonicalize(&game).ok()?;
    for e in std::fs::read_dir(&abs).ok()?.flatten() {
        if e.file_name() != "Mods" {
            std::os::unix::fs::symlink(e.path(), tmp.join(e.file_name())).ok()?;
        }
    }
    for m in &keep {
        std::os::unix::fs::symlink(abs.join("Mods").join(m), tmp.join("Mods").join(m)).ok()?;
    }
    if std::fs::rename(&tmp, &view).is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    view.join("Mods").is_dir().then_some(view)
}

/// The Desert Combat versions the DC tests run on: `BYTEFIELD_DC_MOD` (one mod folder or a comma
/// list, as `DC_Final,DesertCombat`), DC_Final by default; only those whose layer is available.
pub fn dc_mods() -> Vec<String> {
    let list = std::env::var("BYTEFIELD_DC_MOD").unwrap_or_else(|_| "DC_Final".into());
    list.split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty() && Layer::of_mod(m).map_or(game().join("Mods").join(m).is_dir(), |l| l.available() && Layer::Base.available()))
        .map(String::from)
        .collect()
}

/// The mod folders of `layers` that are available, in order: for a test that runs on each
/// version it finds (DC Final and DC 0.7, say) and is fine with none.
pub fn mods_of(layers: &[Layer]) -> Vec<&'static str> {
    layers.iter().filter(|l| l.available()).flat_map(|l| l.mods().iter().copied()).collect()
}

/// The install, or return from the test (skipped, said on stderr) when a layer is missing:
/// `let game = require!(Base, Xpack1);`.
#[macro_export]
macro_rules! require {
    ($($layer:ident),+ $(,)?) => {
        match $crate::require(&[$($crate::Layer::$layer),+]) {
            Some(game) => game,
            None => return,
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_lists() {
        assert_eq!(parse("base, xpack"), vec![Layer::Base, Layer::Xpack1, Layer::Xpack2]);
        assert_eq!(parse("all").len(), Layer::ALL.len());
        assert_eq!(parse("dc,nonsense,gc"), vec![Layer::Dc, Layer::Gc]);
        assert!(parse("").is_empty());
    }

    #[test]
    fn mods_map_to_layers() {
        assert_eq!(Layer::of_mod("dc_final"), Some(Layer::Dc));
        assert_eq!(Layer::of_mod("GC_Redux"), Some(Layer::Ref));
        assert_eq!(Layer::of_mod("SomeDroppedInMod"), None);
    }
}
