//! The saved options: a text file of ours, `key=value` per line, in
//! `$XDG_CONFIG_HOME/bytefield/settings.cfg` (or `~/.config/bytefield/…`; `BYTEFIELD_CONFIG` names another).
//! The command line wins over it, and it is only written from the menus: a check (a level on the
//! command line, a script) never changes it.

use std::path::PathBuf;
use std::sync::RwLock;

use bevy::prelude::*;

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings {
    pub sens: f32,
    pub invert_y: bool,
    /// the master volume, 0–1 (the original's `game.setMasterVolume` / 100, 75 in all its sound profiles)
    pub volume: f32,
    /// the lexicon's name of the language (`English`, `Spanish`…)
    pub lang: String,
    pub name: String,
    pub last_mod: String,
    pub last_level: String,
    pub last_mode: String,
    pub team: u32,
    pub bots: u32,
    /// CREATE GAME's FRIENDLY FIRE, % (`serverSoldierFriendlyFire` and `serverVehicleFriendlyFire`; 100 in the game's `ServerSettings.con`)
    pub friendly_fire: u32,
    pub last_server: String,
    /// the world's vertical field of view, degrees (the original's 1 radian: `play::WORLD_FOV`)
    pub fov: f32,
    pub vsync: bool,
    /// the whole screen (borderless, the monitor's size), or a window (`--windowed` or `--size` for one run)
    pub fullscreen: bool,
    /// the most threads of the compute pool (`task_pools`)
    pub compute_threads: u32,
    /// the install, the mods kept apart, the graphics packs and the graphics profile:
    /// `content.cfg`, not this file
    pub content: bytefield_content::Config,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sens: 1.0,
            invert_y: false,
            volume: 0.75,
            lang: "Spanish".into(),
            name: "Player".into(),
            last_mod: String::new(),
            last_level: String::new(),
            last_mode: String::new(),
            team: 1,
            bots: 16,
            friendly_fire: 100,
            last_server: String::new(),
            fov: crate::play::WORLD_FOV_DEG,
            vsync: true,
            fullscreen: true,
            compute_threads: COMPUTE_THREADS,
            content: Default::default(),
        }
    }
}

/// By default, the compute pool stops at 4 threads. Bevy gives it about half the machine's, and at 8 of 16 threads on a
/// hybrid laptop (balanced power profile) the work spreads thin and the core running the main thread drops to ~1.8 GHz: Berlin
/// with 64 bots goes from 65 to 86 fps with this, and no machine of 8 threads or fewer changes, as Bevy already gives those 4 or
/// fewer. It asks nothing about the CPU.
pub const COMPUTE_THREADS: u32 = 4;

/// The task pools for a cap on the compute pool; `forced` (`BYTEFIELD_COMPUTE_THREADS`, to measure) sets its exact size.
pub fn task_pools(cap: u32, forced: Option<usize>) -> bevy::app::TaskPoolOptions {
    let mut options = bevy::app::TaskPoolOptions::default();
    options.compute.max_threads = forced.unwrap_or(cap as usize);
    options.compute.min_threads = forced.unwrap_or(1);
    options
}

/// The languages of the lexicon, with their short names.
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("fr", "French"), ("it", "Italian"), ("es", "Spanish"), ("de", "German")];
// (the lexicon also has Japanese, Korean and Chinese: out of scope, their glyphs need other fonts; lang.rs)

/// The lexicon's name of a language given either way (`en`, `english`), if it is one.
pub fn language_name(given: &str) -> Option<&'static str> {
    LANGUAGES.iter().find(|(s, l)| given.eq_ignore_ascii_case(s) || given.eq_ignore_ascii_case(l)).map(|(_, l)| *l)
}

impl Settings {
    /// Reads `key=value` lines; unknown keys, comments and broken lines are skipped. The old
    /// `hd`/`hd_packs` become `content`: the packs, and the enhanced profile
    /// if they were on, as they looked then.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let (mut hd, mut hd_packs) = (false, String::new());
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "sens" => s.sens = v.parse().ok().filter(|x: &f32| x.is_finite() && *x > 0.0).unwrap_or(s.sens),
                "invert_y" => s.invert_y = v == "1" || v.eq_ignore_ascii_case("true"),
                "volume" => s.volume = v.parse().ok().filter(|x: &f32| x.is_finite()).map_or(s.volume, |x| x.clamp(0.0, 1.0)),
                "lang" => s.lang = language_name(v).map_or(s.lang, String::from),
                "name" if !v.is_empty() => s.name = v.into(),
                "last_mod" => s.last_mod = v.into(),
                "last_level" => s.last_level = v.into(),
                "last_mode" => s.last_mode = v.into(),
                "team" => s.team = v.parse().ok().filter(|t| *t == 1 || *t == 2).unwrap_or(s.team),
                "bots" => s.bots = v.parse().ok().map_or(s.bots, |b: u32| b.min(64)),
                "friendly_fire" => s.friendly_fire = v.parse().ok().map_or(s.friendly_fire, |f: u32| f.min(100)),
                "last_server" => s.last_server = v.into(),
                "fov" => s.fov = v.parse().ok().filter(|x: &f32| x.is_finite()).map_or(s.fov, |x| x.clamp(30.0, 120.0)),
                "vsync" => s.vsync = v == "1" || v.eq_ignore_ascii_case("true"),
                "fullscreen" => s.fullscreen = v == "1" || v.eq_ignore_ascii_case("true"),
                "compute_threads" => s.compute_threads = v.parse().ok().map_or(s.compute_threads, |n: u32| n.clamp(1, 256)),
                "hd" => hd = v == "1" || v.eq_ignore_ascii_case("true"),
                "hd_packs" => hd_packs = v.into(),
                _ => {}
            }
        }
        for p in std::env::split_paths(&hd_packs).filter(|p| !p.as_os_str().is_empty()) {
            let name = bytefield_content::detect::pack_name(&p);
            if s.content.pack(&name).is_none() {
                s.content.packs.push(bytefield_content::Pack { name, path: p, on: hd });
            }
        }
        if hd && !s.content.packs.is_empty() {
            s.content.graphics = bytefield_content::Profile::Enhanced;
        }
        s
    }

    pub fn write(&self) -> String {
        format!(
            "# bytefield\nsens={}\ninvert_y={}\nvolume={}\nlang={}\nname={}\nlast_mod={}\nlast_level={}\nlast_mode={}\nteam={}\nbots={}\nfriendly_fire={}\nlast_server={}\nfov={}\nvsync={}\nfullscreen={}\ncompute_threads={}\n",
            self.sens, self.invert_y as u8, self.volume, self.lang, self.name, self.last_mod, self.last_level, self.last_mode, self.team, self.bots, self.friendly_fire, self.last_server, self.fov, self.vsync as u8, self.fullscreen as u8, self.compute_threads
        )
    }

    /// The files' (defaults if there are none). `content.cfg` wins over the old `hd` keys; with
    /// no `content.cfg`, those keys are its start.
    pub fn load() -> Settings {
        let mut s = path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Settings::parse(&t)).unwrap_or_default();
        if let Some(c) = bytefield_content::Config::load() {
            s.content = c;
        }
        // what was dropped in a content folder: kept with the next save, so that one turned off stays off
        let ch = bytefield_content::discover::apply(&mut s.content, &bytefield_content::discover::content_dirs());
        for n in &ch.packs {
            info!("content: found pack {n} (on)");
        }
        for d in &ch.mods_dirs {
            info!("content: found mods in {}", d.display());
        }
        for n in &ch.gone {
            info!("content: pack {n} has gone");
        }
        s
    }

    pub fn save(&self) {
        if READ_ONLY.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Some(p) = path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&p, self.write()) {
            warn!("{}: {e}", p.display());
        }
        if (self.content != Default::default() || bytefield_content::config::path().is_some_and(|p| p.exists()))
            && let Err(e) = self.content.save()
        {
            warn!("{e}");
        }
    }

    /// The packs to lay over the install now (`BYTEFIELD_HD`'s form): `BYTEFIELD_HD` as given at the
    /// start, else the ones turned on in `content.cfg`; `forced` (`--hd`/`--no-hd`): all the
    /// registered ones, or none.
    pub fn hd_overlays(&self, forced: Option<bool>) -> String {
        static GIVEN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        let given = GIVEN.get_or_init(|| std::env::var("BYTEFIELD_HD").ok().filter(|v| !v.is_empty()));
        let all = || std::env::join_paths(self.content.packs.iter().map(|p| &p.path)).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match (forced, given) {
            (Some(false), _) => String::new(),
            (_, Some(g)) => g.clone(),
            (Some(true), None) => all(),
            (None, None) => self.content.overlays_string(),
        }
    }

    /// Whether any pack is turned on (the menu's «HD»).
    pub fn packs_on(&self) -> bool {
        self.content.packs.iter().any(|p| p.on)
    }

    /// Lays the content over the loader (`con::session`) and fixes the graphics for the next
    /// textures and cameras; `packs`/`profile` as given on the command line.
    pub fn apply_content(&self, packs: Option<bool>, profile: Option<bytefield_content::Profile>) -> String {
        bytefield_con::session::set_mods_dirs(&self.content.mods_dirs);
        let overlays = self.hd_overlays(packs);
        bytefield_con::session::set_overlays(&overlays);
        crate::assets::set_graphics(bytefield_content::Graphics::resolve(profile.unwrap_or(self.content.graphics), &self.content.overrides));
        overlays
    }

    /// Makes the sound, the texts and the field of view follow these (the mouse is the round's:
    /// `play::Game::set_mouse`; the graphics, [`Settings::apply_content`] and [`Settings::apply_graphics`]).
    pub fn apply(&self) {
        crate::sound::set_volume(self.volume);
        set_language(&self.lang);
        FOV.store(self.fov.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// The graphics as these say (after the menu changed them; the command line's profile is gone then).
    pub fn apply_graphics(&self) {
        crate::assets::set_graphics(bytefield_content::Graphics::resolve(self.content.graphics, &self.content.overrides));
    }
}

/// Set for checks (a screenshot, a script, steps): they never write the file, unless they name
/// their own (`BYTEFIELD_CONFIG`).
pub static READ_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("BYTEFIELD_CONFIG") {
        return Some(PathBuf::from(p));
    }
    Some(bytefield_content::config::config_dir()?.join("settings.cfg"))
}

static FOV: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(crate::play::WORLD_FOV_DEG.to_bits());

/// The world cameras' vertical field of view, radians (exactly the original's at its default).
pub fn world_fov() -> f32 {
    let deg = f32::from_bits(FOV.load(std::sync::atomic::Ordering::Relaxed));
    if deg == crate::play::WORLD_FOV_DEG { crate::play::WORLD_FOV } else { deg.to_radians() }
}

static LANGUAGE: RwLock<String> = RwLock::new(String::new());

/// The texts' language now (the lexicon's name).
pub fn language() -> String {
    let l = LANGUAGE.read().map(|l| l.clone()).unwrap_or_default();
    if l.is_empty() { "Spanish".into() } else { l }
}

pub fn set_language(l: &str) {
    if let Ok(mut w) = LANGUAGE.write() {
        *w = language_name(l).unwrap_or("Spanish").to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_and_back() {
        let s = Settings { sens: 1.5, invert_y: true, volume: 0.25, lang: "English".into(), name: "x".into(), last_mod: "DC_Final".into(), last_level: "DC_Basrahs_Edge".into(), last_mode: "coop".into(), team: 2, bots: 8, friendly_fire: 40, last_server: "10.0.0.2:14567".into(), fov: 75.0, compute_threads: 6, vsync: false, fullscreen: false, content: Default::default() };
        assert_eq!(Settings::parse(&s.write()), s);
    }

    #[test]
    fn the_compute_pool_has_a_cap() {
        let threads = |o: bevy::app::TaskPoolOptions| (o.compute.min_threads, o.compute.max_threads);
        assert_eq!(Settings::default().compute_threads, 4);
        assert_eq!(threads(task_pools(4, None)), (1, 4));
        assert_eq!(threads(task_pools(4, Some(8))), (8, 8));
        assert_eq!(Settings::parse("compute_threads=0\n").compute_threads, 1);
        assert_eq!(Settings::parse("compute_threads=x\n").compute_threads, 4);
    }

    #[test]
    fn old_hd_keys_become_content() {
        let s = Settings::parse("hd=1\nhd_packs=/a/sky:/b/Graphics Mod\n");
        let names: Vec<&str> = s.content.packs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["sky", "Graphics Mod"]);
        assert!(s.packs_on() && s.content.graphics == bytefield_content::Profile::Enhanced);
        let s = Settings::parse("hd=0\nhd_packs=/a/sky\n");
        assert!(!s.packs_on() && s.content.graphics == bytefield_content::Profile::Original);
    }

    #[test]
    fn broken_lines_are_skipped() {
        let s = Settings::parse("garbage\nsens=abc\nvolume=7\nlang=fr\nteam=5\nfoo=bar\n=\nbots=200\n");
        assert_eq!((s.sens, s.volume, s.lang.as_str(), s.team, s.bots), (1.0, 1.0, "French", 1, 64));
    }
}
