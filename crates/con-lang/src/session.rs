//! Loading a mod and a level the way the game does, as far as is known.
//!
//! 1. `Mods/<mod>/init.con`: its `game.addModPath` lines give the search path (first wins)
//! 2. `bf1942/game/init.con`
//! 3. the level's `init.con` (its own objects: GC_Alaris_Prime's `Objects/loadObjects`)
//! 4. every file with `.con` in its name under `objects/` (`loadAllConFiles`), in the order of their whole paths
//! (checked against the numbers the original gives its object templates)
//! 5. the level's `<mode>.con host` (or `gametypes/<mode>.con`), then `staticobjects.con`
//!
//! 3–5 in the order the original's game loads them; a template created twice keeps the first
//! (`templates`), so the level's own, created before the mod's, win.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bytefield_vfs::Vfs;

use crate::templates::TemplateDb;
use crate::{Command, Host, Interpreter, Outcome, ScriptFs};

pub const MODES: &[&str] = &["conquest", "ctf", "tdm", "coop", "objectivemode", "singleplayer"];

/// The template database plus the few engine getters scripts query.
pub struct Session {
    pub db: TemplateDb,
    pub game_play_mode: String,
}

impl Host for Session {
    fn exec(&mut self, cmd: &Command) -> Outcome {
        if cmd.namespace == "game" && cmd.method == "getgameplaymode" {
            return Outcome::Done(Some(self.game_play_mode.clone()));
        }
        self.db.exec(cmd)
    }
}

/// What a mod's init.con says of it: `game.addModPath` (its search path, in order) and the
/// `[set]customGame*` lines (name, version, url, info; both spellings are in the wild).
#[derive(Default, Debug, Clone)]
pub struct ModInit {
    pub paths: Vec<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub url: Option<String>,
    /// its description: a lexicon key (`XPACK1_INFO`) or the text itself (GC's)
    pub info: Option<String>,
}

impl Host for ModInit {
    fn exec(&mut self, cmd: &Command) -> Outcome {
        if cmd.namespace != "game" {
            return Outcome::Done(None);
        }
        let Some(a) = cmd.args.first().cloned() else { return Outcome::Done(None) };
        match cmd.method.strip_prefix("set").unwrap_or(&cmd.method) {
            "addmodpath" => self.paths.push(a),
            "customgamename" => self.name = Some(a),
            "customgameversion" => self.version = Some(a),
            "customgameurl" => self.url = Some(a),
            "customgameinfo" => self.info = Some(a),
            _ => {}
        }
        Outcome::Done(None)
    }
}

/// Reads a mod's init.con (`dir` is `Mods/<x>`).
pub fn mod_init(dir: &Path) -> Result<ModInit, String> {
    let init = std::fs::read(find_ci(dir, "init.con").ok_or("mod has no init.con")?).map_err(|e| e.to_string())?;
    let fs = OneFile(init);
    let mut it = Interpreter::new(&fs, ModInit::default());
    it.run_file("init.con", &[]);
    Ok(it.host)
}

struct OneFile(Vec<u8>);

impl ScriptFs for OneFile {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        (path == "init.con").then(|| self.0.clone())
    }
}

pub fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
    })
}

pub fn resolve_ci(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|s| !s.is_empty()) {
        p = find_ci(&p, part)?;
    }
    Some(p)
}

/// The mod's search path (from its init.con), mounted in order.
/// Packs laid over the install (`BYTEFIELD_HD=dir[:dir…]`, each a tree like the install's, with
/// `Mods/<x>/Archives/…`; the later ones over the earlier, as if copied in that order): the
/// graphics mods that replace the game's archives. Not set: the install alone.
/// A program may set them instead ([`set_overlays`]: the client's options).
pub fn overlays() -> Vec<PathBuf> {
    if let Some(o) = OVERLAYS.read().ok().and_then(|o| o.clone()) {
        return o;
    }
    std::env::var_os("BYTEFIELD_HD").map(|v| std::env::split_paths(&v).filter(|p| p.is_dir()).collect()).unwrap_or_default()
}

static OVERLAYS: std::sync::RwLock<Option<Vec<PathBuf>>> = std::sync::RwLock::new(None);

/// Folders with more mods, besides the install's `Mods` (`BYTEFIELD_MODS_DIRS=dir[:dir…]`, each
/// holding `<mod>/init.con`…; the install's wins): a mod kept apart, as the HD Remaster's
/// `graphicsplus`. A program may set them instead ([`set_mods_dirs`]). Then the `Mods` of each of the
/// [`overlays`], the top one first: the mods a pack brings of its own (the HD Remaster's `SWExtra` and
/// `RoadtoRomeEXTRA`), while it is on.
pub fn mods_dirs() -> Vec<PathBuf> {
    let mut dirs = MODS_DIRS.read().ok().and_then(|o| o.clone()).unwrap_or_else(|| std::env::var_os("BYTEFIELD_MODS_DIRS").map(|v| std::env::split_paths(&v).filter(|p| p.is_dir()).collect()).unwrap_or_default());
    for d in overlays().iter().rev().filter_map(|o| resolve_ci(o, "Mods")) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

static MODS_DIRS: std::sync::RwLock<Option<Vec<PathBuf>>> = std::sync::RwLock::new(None);

/// The folders of [`mods_dirs`] from now on.
pub fn set_mods_dirs(dirs: &[PathBuf]) {
    if let Ok(mut o) = MODS_DIRS.write() {
        *o = Some(dirs.iter().filter(|p| p.is_dir()).cloned().collect());
    }
}

/// A path of the install (`Mods/<x>/…`), else the same under one of the [`mods_dirs`].
pub fn resolve_mod_path(game: &Path, rel: &str) -> Option<PathBuf> {
    resolve_mod_path_in(game, &mods_dirs(), rel)
}

/// [`resolve_mod_path`] with the folders given.
pub fn resolve_mod_path_in(game: &Path, mods_dirs: &[PathBuf], rel: &str) -> Option<PathBuf> {
    resolve_ci(game, rel).or_else(|| {
        let rest = rel.trim_start_matches(['/', '\\']);
        let (first, rest) = rest.split_once(['/', '\\']).unwrap_or((rest, ""));
        if !first.eq_ignore_ascii_case("mods") {
            return None;
        }
        mods_dirs.iter().find_map(|d| resolve_ci(d, rest))
    })
}

/// The packs of [`overlays`] from now on, in `BYTEFIELD_HD`'s form (empty: none).
pub fn set_overlays(list: &str) {
    let v = std::env::split_paths(list).filter(|p| !p.as_os_str().is_empty() && p.is_dir()).collect();
    if let Ok(mut o) = OVERLAYS.write() {
        *o = Some(v);
    }
}

pub fn mount(game: &Path, mod_name: &str) -> Result<(Vec<String>, Vfs), String> {
    let mod_dir = resolve_mod_path(game, &format!("Mods/{mod_name}")).ok_or(format!("no mod {mod_name}"))?;
    let mod_paths = mod_init(&mod_dir)?.paths;
    let mut vfs = Vfs::new();
    let packs = overlays();
    for p in &mod_paths {
        let dir = resolve_mod_path(game, p).ok_or(format!("mod path {p} not found under {}", game.display()))?;
        let over: Vec<PathBuf> = packs.iter().filter_map(|root| resolve_ci(root, p)).collect();
        vfs.mount_mod_with(&dir, &over).map_err(|e| format!("{p}: {e}"))?;
    }
    Ok((mod_paths, vfs))
}

pub fn game_play_mode(mode: &str) -> &'static str {
    match mode.to_lowercase().as_str() {
        "ctf" => "GPM_CTF",
        "tdm" => "GPM_TDM",
        "coop" => "GPM_COOP",
        "objectivemode" => "GPM_OBJECTIVEMODE",
        _ => "GPM_CQ",
    }
}

/// Steps 2-5 above. `level` None loads the game and its objects only.
pub fn load<'a>(vfs: &'a Vfs, level: Option<&str>, mode: &str) -> Interpreter<'a, Session> {
    let session = Session { db: TemplateDb::default(), game_play_mode: game_play_mode(mode).into() };
    let mut it = Interpreter::new(vfs, session);
    // the executable runs this one itself, before the game's init.con (the materials of the
    // collision: friction, elasticity, resistance)
    if it.exists("bf1942/game/materialmanagersettings.con") {
        it.run_file("bf1942/game/materialmanagersettings.con", &[]);
    }
    it.run_file("bf1942/game/init.con", &[]);
    // The game's load: the object templates made so far go (GC's cameras, made again in `objects/`)
    it.host.db.delete_object_templates();
    let base = level.map(|l| format!("bf1942/levels/{l}"));
    if let Some(base) = &base {
        it.run_file(&format!("{base}/init.con"), &[]);
    }
    // and the soldiers' animation state machine
    if it.exists("animations/animationstates.con") {
        it.run_file("animations/animationstates.con", &[]);
    }

    // every file whose name has `.con` in it (DC's `Copy of Effects.conBAK` and `Objects.con~` too), sorted by the
    // whole path without case (a `NoCaseStringCompare` map; checked with the original's template numbers:
    // `aichival-t/` before `aichival/`, `effects.con` before `objects.con`)
    let mut files: Vec<String> = vfs.list().into_iter().map(bytefield_vfs::normalize).filter(|n| n.starts_with("objects/") && n.contains(".con")).collect();
    files.sort();
    files.dedup();
    for f in files {
        it.run_file(&f, &[]);
    }

    if let Some(base) = &base {
        let host = vec!["host".to_string()];
        let mode = mode.to_lowercase();
        // `<level>/<mode>.con` is what runs, loaded as the host;
        // `gametypes/<mode>.con` only says the level has the mode: GC's
        // say so ("The contents of this file are never read", GC_Bespin_Night). Without the
        // first, the second.
        let at_root = format!("{base}/{mode}.con");
        if it.exists(&at_root) {
            it.run_file(&at_root, &host);
        } else {
            it.run_file(&format!("{base}/gametypes/{mode}.con"), &host);
        }
        it.run_file(&format!("{base}/staticobjects.con"), &[]);
        // the bots' names, loaded as the server starts: `Bots.con` in conquest (the
        // mode's 2), `Skirmish.con` in the others (Midway's coop: British, not American)
        let names = if mode == "conquest" { "bots" } else { "skirmish" };
        let names = format!("{base}/singleplayer/{names}.con");
        if it.exists(&names) {
            it.run_file(&names, &[]);
        }
    }
    it
}

/// Each level of the mounted mod, with the game modes it has scripts for.
pub fn levels(vfs: &Vfs) -> Vec<(String, Vec<&'static str>)> {
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    for p in vfs.list() {
        let n = bytefield_vfs::normalize(p);
        if let Some(rest) = n.strip_prefix("bf1942/levels/")
            && let Some((lvl, file)) = rest.split_once('/')
            && file == "init.con"
        {
            found.insert(lvl.to_string(), p.split('/').nth(2).unwrap_or(lvl).to_string());
        }
    }
    found
        .into_iter()
        .map(|(lvl, spelled)| {
            let modes = MODES
                .iter()
                .copied()
                .filter(|m| vfs.exists(&format!("bf1942/levels/{lvl}/gametypes/{m}.con")) || vfs.exists(&format!("bf1942/levels/{lvl}/{m}.con")))
                .collect();
            (spelled, modes)
        })
        .collect()
}
