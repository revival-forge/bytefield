//! What is there: the mods of the install and of the `mods_dir`s (each `<x>/init.con`, its
//! `addModPath` chain and version) and the packs, told apart with the catalog.

use std::path::{Path, PathBuf};

use bytefield_con::session::{ModInit, find_ci, mod_init, resolve_ci, resolve_mod_path_in};

use crate::catalog::{self, Entry};
use crate::config::{Config, Pack};

#[derive(Debug, Clone)]
pub struct ModEntry {
    /// the folder's name, as `Mods/<x>`
    pub folder: String,
    pub dir: PathBuf,
    /// from a `mods_dir`, not the install
    pub external: bool,
    pub init: ModInit,
    /// the `addModPath`s that are nowhere (a mod it needs is not there)
    pub missing: Vec<String>,
    pub entry: Option<&'static Entry>,
}

impl ModEntry {
    /// Its version is one the catalog knows.
    pub fn version_known(&self) -> bool {
        self.entry.is_some_and(|e| self.init.version.as_ref().is_some_and(|v| e.versions.iter().any(|k| k.eq_ignore_ascii_case(v))))
    }

    /// The chain of folders it loads, from its own down (`DC_Final → DesertCombat → BF1942`).
    pub fn chain(&self) -> Vec<String> {
        self.init.paths.iter().map(|p| p.trim_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(p).to_string()).collect()
    }

    pub fn playable(&self) -> bool {
        self.missing.is_empty() && !self.init.paths.is_empty()
    }
}

fn mods_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| find_ci(&e.path(), "init.con").is_some())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    v.sort_by_key(|(n, _)| n.to_lowercase());
    v
}

/// The mods: the install's, then those of the `mods_dirs` it lacks, each sorted by name.
pub fn mods(install: &Path, mods_dirs: &[PathBuf]) -> Vec<ModEntry> {
    let mut found: Vec<(String, PathBuf, bool)> = resolve_ci(install, "Mods").map(|d| mods_in(&d)).unwrap_or_default().into_iter().map(|(n, p)| (n, p, false)).collect();
    for d in mods_dirs {
        for (n, p) in mods_in(d) {
            if !found.iter().any(|(m, _, _)| m.eq_ignore_ascii_case(&n)) {
                found.push((n, p, true));
            }
        }
    }
    found
        .into_iter()
        .map(|(folder, dir, external)| {
            let init = mod_init(&dir).unwrap_or_default();
            let missing = init.paths.iter().filter(|p| resolve_mod_path_in(install, mods_dirs, p).is_none()).cloned().collect();
            let entry = catalog::by_folder(&folder);
            ModEntry { folder, dir, external, init, missing, entry }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct PackEntry {
    pub pack: Pack,
    pub exists: bool,
    /// the `Mods/<x>` it lays files over
    pub folders: Vec<String>,
    pub entry: Option<&'static Entry>,
}

pub fn pack(p: &Pack) -> PackEntry {
    let folders = resolve_ci(&p.path, "Mods").map(|d| {
        let mut v: Vec<String> = std::fs::read_dir(d).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v
    });
    PackEntry { pack: p.clone(), exists: p.path.is_dir(), folders: folders.unwrap_or_default(), entry: catalog::pack_at(&p.path) }
}

/// A name for a pack at `path`: the catalog's id, else the folder's.
pub fn pack_name(path: &Path) -> String {
    catalog::pack_at(path).map(|e| e.id.clone()).unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "pack".into()))
}

/// What the catalog knows of and is not there (a mod folder, a registered pack), with what it
/// needs that is.
pub fn missing(mods: &[ModEntry], packs: &[PackEntry]) -> Vec<&'static Entry> {
    let have = |e: &Entry| match e.kind.as_str() {
        "pack" => packs.iter().any(|p| p.exists && p.entry.is_some_and(|x| x.id == e.id)),
        _ => mods.iter().any(|m| m.entry.is_some_and(|x| x.id == e.id)),
    };
    catalog::catalog().iter().filter(|e| !have(e) && !e.internal).collect()
}

/// The ids of what is there (to check `needs`).
pub fn present(mods: &[ModEntry], packs: &[PackEntry]) -> Vec<String> {
    let mut v: Vec<String> = mods.iter().filter_map(|m| m.entry.map(|e| e.id.clone())).collect();
    v.extend(packs.iter().filter(|p| p.exists && p.pack.on).filter_map(|p| p.entry.map(|e| e.id.clone())));
    v
}

/// Everything at once, for the tool and the menus.
pub struct Scan {
    pub mods: Vec<ModEntry>,
    pub packs: Vec<PackEntry>,
}

/// The mods dirs, then the `Mods` of the active packs (the mods a pack brings: `session::mods_dirs`).
pub fn scan(cfg: &Config, install: &Path) -> Scan {
    let mut dirs = cfg.mods_dirs.clone();
    for d in cfg.active_packs().iter().rev().filter_map(|p| resolve_ci(p, "Mods")) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    Scan { mods: mods(install, &dirs), packs: cfg.packs.iter().map(pack).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bytefield-content-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn mods_chains_and_mods_dirs() {
        let root = tmp("mods");
        let (game, extra) = (root.join("game"), root.join("extra"));
        write(&game, "Mods/bf1942/init.con", "game.CustomGameName BF1942\ngame.addModPath Mods/BF1942/\ngame.setCustomGameVersion 1.6\n");
        write(&game, "Mods/DesertCombat/init.con", "game.setCustomGameName DesertCombat\ngame.addModPath Mods/DesertCombat/\ngame.addModPath Mods/BF1942/\ngame.setCustomGameVersion 0.7\n");
        write(&game, "Mods/DC_Final/init.con", "game.addModPath Mods/DC_Final/\ngame.addModPath Mods/DesertCombat/\ngame.addModPath Mods/BF1942/\ngame.customGameVersion 0.8\n");
        write(&game, "Mods/Broken/init.con", "game.addModPath Mods/Broken/\ngame.addModPath Mods/Nowhere/\n");
        write(&extra, "graphicsplus/init.con", "game.addmodPath Mods/graphicsplus/\ngame.addModPath Mods/bf1942/\ngame.setCustomGameVersion 1.0\n");
        write(&extra, "bf1942/init.con", "shadowed by the install's\n");

        let m = mods(&game, std::slice::from_ref(&extra));
        let names: Vec<&str> = m.iter().map(|m| m.folder.as_str()).collect();
        assert_eq!(names, ["bf1942", "Broken", "DC_Final", "DesertCombat", "graphicsplus"]);
        let dcf = &m[2];
        assert_eq!(dcf.chain(), ["DC_Final", "DesertCombat", "BF1942"]);
        assert!(dcf.playable() && dcf.version_known());
        assert_eq!(m[1].missing, ["Mods/Nowhere/"]);
        let gp = &m[4];
        assert!(gp.external && gp.playable(), "{gp:?}");
        assert!(!m[0].external);

        // without the mods_dir, graphicsplus is gone and nothing else changes
        assert_eq!(mods(&game, &[]).len(), 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn packs_by_markers() {
        let root = tmp("packs");
        write(&root, "sky/Mods/bf1942/Archives/bf1942/levels/El_Alamein_006.rfa", "");
        write(&root, "other/Mods/XPack1/Archives/x.rfa", "");
        let sky = pack(&Pack { name: "s".into(), path: root.join("sky"), on: true });
        assert_eq!(sky.entry.map(|e| e.id.as_str()), Some("hd-skybox"));
        assert_eq!(sky.folders, ["bf1942"]);
        let other = pack(&Pack { name: "o".into(), path: root.join("other"), on: true });
        assert!(other.entry.is_none() && other.exists);
        assert_eq!(pack_name(&root.join("other")), "other");
        let gone = pack(&Pack { name: "g".into(), path: root.join("gone"), on: true });
        assert!(!gone.exists);
        let _ = std::fs::remove_dir_all(&root);
    }
}
