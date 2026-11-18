//! Content found without being told: what the player drops in a `content` folder is there the next time
//! the client starts, the packs turned on, as copying them over the install did with the original (where nothing
//! could be turned off). Found once, it goes to `content.cfg`: turned off there or in the menu, it stays off.
//!
//! The folders: `$XDG_DATA_HOME/bytefield/content` (or `~/.local/share/…`; on Windows `%LOCALAPPDATA%\bytefield\content`) and
//! `content` beside the program (a copy that travels in one folder). `BYTEFIELD_CONTENT_DIRS` names others (`:`-separated,
//! `;` on Windows); with `BYTEFIELD_CONTENT` set and not it, none (checks go by their arguments alone).
//!
//! In each, a subfolder is:
//! - a **pack** if it has `Mods/` (a tree like the install's), or one folder down has (the HD Remaster's base zip
//!   unpacks to `Graphics Mod/Mods/…`);
//! - a **mod** if it has `init.con` (`DC_Final/init.con`): the content folder is then a mods folder too.

use std::path::{Path, PathBuf};

use bytefield_con::session::find_ci;

use crate::catalog;
use crate::config::{Config, Pack};

/// The folders looked in, those that exist.
pub fn content_dirs() -> Vec<PathBuf> {
    let given = std::env::var_os("BYTEFIELD_CONTENT_DIRS");
    let dirs: Vec<PathBuf> = match given {
        Some(v) => std::env::split_paths(&v).collect(),
        None if std::env::var_os("BYTEFIELD_CONTENT").is_some() => Vec::new(),
        None => {
            let local = || std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
            let xdg = || std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
            let home = || std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"));
            let data = if cfg!(windows) { local().or_else(xdg).or_else(home) } else { xdg().or_else(home).or_else(local) };
            let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("content")));
            data.map(|d| d.join("bytefield").join("content")).into_iter().chain(beside).collect()
        }
    };
    let mut v: Vec<PathBuf> = Vec::new();
    for d in dirs.into_iter().filter(|d| d.is_dir()) {
        if !v.contains(&d) {
            v.push(d);
        }
    }
    v
}

/// What one subfolder of a content folder is.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// its root (the folder with `Mods/`)
    Pack(PathBuf),
    /// a mod (`<x>/init.con`): its folder
    Mod(PathBuf),
    /// a tree whose `Mods/` only brings mods the install lacks (the HD Remaster's «custom», `Mods/graphicsplus`): that `Mods`
    Mods(PathBuf),
}

/// What `dir` is, for an install with these mods (`Mods/<x>`; all a pack if not known): a tree with `Mods/` is a pack if
/// it lays files over one of the install's mods, and a folder of new mods otherwise.
pub fn classify(dir: &Path, install: Option<&Path>) -> Option<Found> {
    if find_ci(dir, "init.con").is_some() {
        return Some(Found::Mod(dir.to_path_buf()));
    }
    let has_mods = |d: &Path| find_ci(d, "Mods").filter(|m| m.is_dir());
    let root = if has_mods(dir).is_some() {
        dir.to_path_buf()
    } else {
        // one folder down, if it is the only one that has it
        let mut inner = subdirs(dir).into_iter().filter(|d| has_mods(d).is_some());
        match (inner.next(), inner.next()) {
            (Some(p), None) => p,
            _ => return None,
        }
    };
    let mods = has_mods(&root)?;
    let over_install = match install.and_then(|i| find_ci(i, "Mods")) {
        None => true,
        Some(im) => subdirs(&mods).iter().any(|m| m.file_name().is_some_and(|n| find_ci(&im, &n.to_string_lossy()).is_some())),
    };
    Some(if over_install { Found::Pack(root) } else { Found::Mods(mods) })
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    v
}

/// Everything in these content folders.
pub fn scan(dirs: &[PathBuf], install: Option<&Path>) -> Vec<Found> {
    dirs.iter().flat_map(|d| subdirs(d)).filter_map(|d| classify(&d, install)).collect()
}

/// What [`apply`] changed, to say it.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub packs: Vec<String>,
    pub mods_dirs: Vec<PathBuf>,
    /// found before and gone now
    pub gone: Vec<String>,
}

impl Changes {
    pub fn any(&self) -> bool {
        !self.packs.is_empty() || !self.mods_dirs.is_empty() || !self.gone.is_empty()
    }
}

/// Puts in `cfg` what the content folders have and it lacks: each new pack turned on, over the ones there were, in
/// the catalog's order (the HD Remaster's base, then its skies over it, then its XPack part), the unknown ones after
/// by name; each folder with mods as a `mods_dir`. A pack of theirs whose folder has gone is taken out.
pub fn apply(cfg: &mut Config, dirs: &[PathBuf]) -> Changes {
    let mut ch = Changes::default();
    let inside = |p: &Path| dirs.iter().any(|d| p.starts_with(d));
    cfg.packs.retain(|p| {
        let keep = !inside(&p.path) || p.path.is_dir();
        if !keep {
            ch.gone.push(p.name.clone());
        }
        keep
    });
    let found = scan(dirs, cfg.install.as_deref());
    let order = |p: &Path| catalog::pack_at(p).and_then(|e| catalog::catalog().iter().position(|c| c.id == e.id)).unwrap_or(usize::MAX);
    let mut new: Vec<PathBuf> = found
        .iter()
        .filter_map(|f| match f {
            Found::Pack(p) => Some(p.clone()),
            _ => None,
        })
        .filter(|p| !cfg.packs.iter().any(|q| q.path == *p))
        .collect();
    new.sort_by_key(|p| (order(p), p.to_string_lossy().to_lowercase()));
    for p in new {
        let name = cfg.unique_pack_name(crate::detect::pack_name(&p));
        ch.packs.push(name.clone());
        cfg.packs.push(Pack { name, path: p, on: true });
    }
    for f in &found {
        let d = match f {
            Found::Mod(m) => m.parent(),
            Found::Mods(m) => Some(m.as_path()),
            Found::Pack(_) => None,
        };
        if let Some(d) = d
            && !cfg.mods_dirs.iter().any(|x| x == d)
        {
            cfg.mods_dirs.push(d.to_path_buf());
            ch.mods_dirs.push(d.to_path_buf());
        }
    }
    ch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "").unwrap();
    }

    #[test]
    fn dropped_packs_and_mods_are_found_on_and_stay_off_once_turned_off() {
        let root = std::env::temp_dir().join(format!("bytefield-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let c = root.join("content");
        // (named so that by name they would go the other way round: the catalog's order wins)
        write(&c, "a-sky/Mods/bf1942/Archives/bf1942/levels/El_Alamein_006.rfa");
        write(&c, "b-hd/Graphics Mod/Mods/bf1942/Settings/VideoDefault.con");
        write(&c, "b-hd/Graphics Mod/Mods/bf1942/Archives/texture.rfa");
        write(&c, "b-hd/readme.txt");
        write(&c, "mine/Mods/bf1942/Archives/x.rfa");
        write(&c, "DC_Final/init.con");
        write(&c, "nothing/readme.txt");
        write(&c, "custom/Mods/graphicsplus/init.con");
        let game = root.join("game");
        write(&game, "Mods/bf1942/init.con");
        write(&game, "Mods/XPack1/init.con");
        let dirs = vec![c.clone()];

        let mut cfg = Config { install: Some(game.clone()), ..Default::default() };
        let ch = apply(&mut cfg, &dirs);
        let names: Vec<&str> = cfg.packs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["hd-base", "hd-skybox", "mine"]);
        assert!(cfg.packs.iter().all(|p| p.on));
        assert_eq!(cfg.packs[0].path, c.join("b-hd/Graphics Mod"));
        assert_eq!(cfg.mods_dirs, [c.join("custom/Mods"), c.clone()], "a tree of new mods is a mods folder, not a pack");
        assert_eq!(ch.packs.len(), 3);

        // turned off, and started again: as it was
        cfg.pack_mut("hd-skybox").unwrap().on = false;
        let mut again = Config::parse(&cfg.write());
        assert!(!apply(&mut again, &dirs).any());
        assert_eq!(again, cfg);

        // a new one goes on top, on; a removed one goes away
        write(&c, "z-new/Mods/XPack1/Archives/y.rfa");
        std::fs::remove_dir_all(c.join("mine")).unwrap();
        let ch = apply(&mut again, &dirs);
        assert_eq!((ch.packs.clone(), ch.gone.clone()), (vec!["z-new".to_string()], vec!["mine".to_string()]));
        let names: Vec<&str> = again.packs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["hd-base", "hd-skybox", "z-new"]);
        assert!(!again.pack("hd-skybox").unwrap().on);

        // one registered by hand elsewhere is left alone
        let mut by_hand = Config { install: Some(game.clone()), packs: vec![Pack { name: "far".into(), path: "/nowhere/far".into(), on: true }], ..Default::default() };
        apply(&mut by_hand, &dirs);
        assert_eq!(by_hand.packs[0].name, "far");
        let _ = std::fs::remove_dir_all(&root);
    }
}
