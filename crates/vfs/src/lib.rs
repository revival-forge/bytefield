//! The Refractor 1 file system: what `run objects/foo` or a mesh path resolves to.
//!
//! A mod's `init.con` lists its search path with `game.addModPath Mods/X/` (first wins), and
//! each mod path has an `Archives/` directory of `.rfa` files. An archive's entries are paths
//! relative to that `Archives/` root (`bf1942/levels/Berlin/Init.con`). Within one mod path,
//! patch archives override the base one: `Berlin_003.rfa` > `Berlin_001.rfa` > `Berlin.rfa`.
//! Loose files under `Mods/X/` (like an unpacked mod) are also searched, before the archives.
//! Lookups ignore case, like Windows.

pub mod lzo;
pub mod rfa;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use rfa::Archive;

/// Where one file comes from.
#[derive(Debug, Clone)]
enum Source {
    Loose(PathBuf),
    Packed { archive: usize, entry: usize },
}

#[derive(Default)]
pub struct Vfs {
    archives: Vec<Archive>,
    /// lowercase path -> the winning source; the first mount wins
    files: HashMap<String, (String, Source)>,
}

/// `Berlin_003.rfa` -> ("berlin", 3); `Berlin.rfa` -> ("berlin", -1)
fn patch_level(file_name: &str) -> (String, i32) {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s).to_lowercase();
    if let Some((base, n)) = stem.rsplit_once('_')
        && n.len() == 3
        && let Ok(n) = n.parse()
    {
        return (base.to_string(), n);
    }
    (stem, -1)
}

pub fn normalize(path: &str) -> String {
    let p = path.replace('\\', "/").to_lowercase();
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out.join("/")
}

impl Vfs {
    pub fn new() -> Vfs {
        Vfs::default()
    }

    /// Mount a mod path (`<game>/Mods/X`), with lower priority than anything mounted before.
    pub fn mount_mod(&mut self, mod_dir: &Path) -> io::Result<()> {
        self.mount_mod_with(mod_dir, &[])
    }

    /// [`mount_mod`](Self::mount_mod) as if the `overlays` (the same mod path in other trees, such
    /// as a graphics pack's) had been copied over it, in order: a file of theirs replaces the one
    /// at the same place, an archive too. Then the patch levels go as always (the install's `Berlin_003.rfa`
    /// over a pack's `Berlin.rfa`), except for the files the pack changed: a file of a pack's archive that is not
    /// as in the install's archive it replaces wins over the install's patches of it (a pack
    /// turned on wins over the install's patches; the «Skybox Multiplayer» pack's `Berlin.rfa` has the 1.6
    /// patch in it and its own `Init.con`, which names its skies, and the original with it copied over a 1.6
    /// keeps `Berlin_003`'s). A file left as it was stays under the patches: the XPack pack of the HD Remaster
    /// brings XPack1's levels as before the 1.6 patch (`salerno.rfa`'s `AI.con` without `if v_arg1 == host`),
    /// and they must not undo it. The textures of an archive a pack
    /// replaced stay under everything of this mod: the HD Remaster's `texture.rfa` lacks nine of the
    /// Fletcher's (`Fletch_*_Z.dds`), which the game with the pack copied would draw without.
    pub fn mount_mod_with(&mut self, mod_dir: &Path, overlays: &[PathBuf]) -> io::Result<()> {
        // (a mod a pack brings is its own overlay)
        let dirs: Vec<&Path> = overlays.iter().rev().map(PathBuf::as_path).filter(|d| *d != mod_dir).chain([mod_dir]).collect();
        // loose files first (an unpacked mod, or the mod's own init.con and Settings)
        for dir in &dirs {
            for path in walk(dir)? {
                let rel = path.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                let rel = rel.strip_prefix("Archives/").or_else(|| rel.strip_prefix("archives/")).unwrap_or(&rel).to_string();
                if rel.to_lowercase().ends_with(".rfa") {
                    continue;
                }
                self.files.entry(normalize(&rel)).or_insert((rel.clone(), Source::Loose(path)));
            }
        }
        // then the archives (each place's from the last tree that has one), the highest patch
        // level first
        let mut by_place: HashMap<String, PathBuf> = HashMap::new();
        let mut replaced = Vec::new();
        // (the install's archive a pack's replaced, by place)
        let mut under: HashMap<String, PathBuf> = HashMap::new();
        for dir in dirs.iter().rev() {
            let Some(d) = find_child(dir, "archives") else { continue };
            for p in walk(&d)?.into_iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rfa"))) {
                let place = p.strip_prefix(&d).unwrap().to_string_lossy().to_lowercase();
                if let Some(old) = by_place.insert(place.clone(), p) {
                    if old.starts_with(mod_dir) {
                        under.insert(place, old.clone());
                    }
                    replaced.push(old);
                }
            }
        }
        // (by the place within `Archives/`: the trees' own paths do not count)
        let mut rfas: Vec<(String, PathBuf)> = by_place.into_iter().collect();
        rfas.sort_by_key(|(rel, _)| {
            let (dir, name) = rel.rsplit_once(['/', '\\']).unwrap_or(("", rel));
            let (base, level) = patch_level(name);
            (dir.to_string(), base, std::cmp::Reverse(level))
        });
        let mut packs_over = Vec::new();
        for (place, path) in rfas {
            if let Some(install) = under.remove(&place) {
                packs_over.push((self.archives.len(), install));
            }
            // a patch (`<base>_NNN.rfa`) goes over its base whatever form the base takes: over the loose files of a mod
            // kept unpacked too (GC from its sources), not under them; a higher patch, mounted first, still wins
            let name = place.rsplit(['/', '\\']).next().unwrap_or(&place);
            self.mount_archive_with(&path, |_| true, patch_level(name).1 >= 0)?;
        }
        // what a pack changed, over the install's patches
        for (idx, install) in packs_over {
            let was_archive = Archive::open(&install)?;
            let was: HashMap<String, &rfa::Entry> = was_archive.entries.iter().map(|e| (normalize(&e.name), e)).collect();
            let pack = &self.archives[idx];
            let mut wins = Vec::new();
            for (i, e) in pack.entries.iter().enumerate() {
                let k = normalize(&e.name);
                let Some((_, Source::Packed { archive, .. })) = self.files.get(&k) else { continue };
                if *archive == idx || !self.archives[*archive].path.starts_with(mod_dir) {
                    continue;
                }
                let changed = match was.get(&k) {
                    None => true,
                    Some(w) => w.ucsize != e.ucsize || was_archive.read(w)? != pack.read(e)?,
                };
                if changed {
                    wins.push((k, e.name.clone(), i));
                }
            }
            for (k, name, entry) in wins {
                self.files.insert(k, (name, Source::Packed { archive: idx, entry }));
            }
        }
        // the top tree's replaced ones first
        for path in replaced.iter().rev() {
            self.mount_archive_only(path, is_texture)?;
        }
        Ok(())
    }

    pub fn mount_archive(&mut self, path: &Path) -> io::Result<()> {
        self.mount_archive_only(path, |_| true)
    }

    /// [`mount_archive`](Self::mount_archive) with only the files that `keep` accepts (by their
    /// normalized path).
    fn mount_archive_only(&mut self, path: &Path, keep: impl Fn(&str) -> bool) -> io::Result<()> {
        self.mount_archive_with(path, keep, false)
    }

    /// [`mount_archive_only`](Self::mount_archive_only); `over_loose`: its files replace loose ones already mounted.
    fn mount_archive_with(&mut self, path: &Path, keep: impl Fn(&str) -> bool, over_loose: bool) -> io::Result<()> {
        let archive = Archive::open(path)?;
        let idx = self.archives.len();
        for (i, e) in archive.entries.iter().enumerate() {
            let k = normalize(&e.name);
            if keep(&k) {
                let packed = (e.name.clone(), Source::Packed { archive: idx, entry: i });
                match self.files.entry(k) {
                    std::collections::hash_map::Entry::Occupied(mut o) if over_loose && matches!(o.get().1, Source::Loose(_)) => {
                        o.insert(packed);
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {}
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(packed);
                    }
                }
            }
        }
        self.archives.push(archive);
        Ok(())
    }

    /// Put the files of `other` that `keep` accepts (by their normalized path) over this one's:
    /// they win over anything mounted before or after. Returns the paths taken, as stored.
    pub fn overlay(&mut self, other: Vfs, keep: impl Fn(&str) -> bool) -> Vec<String> {
        let base = self.archives.len();
        let mut taken = Vec::new();
        for (k, (name, src)) in other.files {
            if !keep(&k) {
                continue;
            }
            let src = match src {
                Source::Packed { archive, entry } => Source::Packed { archive: archive + base, entry },
                l => l,
            };
            taken.push(name.clone());
            self.files.insert(k, (name, src));
        }
        self.archives.extend(other.archives);
        taken.sort_by_key(|s| s.to_lowercase());
        taken
    }

    /// A level's own meshes and textures (`bf1942/levels/<map>/StandardMesh/…`, `Texture/…`,
    /// `TreeMesh/…`, `AnimatedMesh/…`) seen also where the mod's are, over them: a mesh file
    /// names no level (`Vehicles/Speederbike/Speederbike1_bb`, only in GC_Alaris_Prime's; vanilla's
    /// Battle_of_Britain brings its factory, Liberation_of_Caen its wrecks). Also its HUD's pictures
    /// (`Menu/Texture/…`: GC_Podrace's `setVehicleIcon "vehicle/Icon_podracer.dds"` is only in the mod's
    /// own archive). Returns how many.
    ///
    /// A texture of the level's under a name the mod has already is not seen: the original keeps the mod's (
    /// a red `bf1942/levels/Berlin/texture/Sky_Berlin_01.dds` in a patch leaves Berlin's sky as it was, so the HD Remaster's
    /// level skies do not show in it either). A level that wants its own over the mod's names its folder in
    /// `textureManager.alternativePath` (Battle_of_Britain's `Sky_Bocage_0*`, Raid_on_Agheila, Liberation_of_Caen).
    // TODO: the same for a mesh or a `.rs` (graphicsplus brings `standardmesh/c01f_grass_m1.rs` in Berlin and Wake): not
    // tried in the original; none of vanilla's, XPack's, DC's or GC's levels has one. How: a red probe with a
    // `.rs` whose texture is red.
    pub fn mount_level_assets(&mut self, level_dir: &str) -> usize {
        let prefix = format!("{}/", normalize(level_dir));
        let dirs = ["standardmesh/", "texture/", "treemesh/", "animatedmesh/", "menu/texture/"];
        let found: Vec<(String, (String, Source))> = self
            .files
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).filter(|rest| dirs.iter().any(|d| rest.starts_with(d))).map(|rest| (rest.to_string(), v.clone())))
            .filter(|(rest, _)| !(rest.starts_with("texture/") && self.files.contains_key(rest)))
            .collect();
        let n = found.len();
        self.files.extend(found);
        n
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(&normalize(path))
    }

    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        match self.files.get(&normalize(path)) {
            None => Err(io::Error::new(io::ErrorKind::NotFound, path.to_string())),
            Some((_, Source::Loose(p))) => std::fs::read(p),
            Some((_, Source::Packed { archive, entry })) => {
                let a = &self.archives[*archive];
                a.read(&a.entries[*entry])
            }
        }
    }

    /// Every visible path, as stored (first spelling of the winning source), sorted.
    pub fn list(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.files.values().map(|(name, _)| name.as_str()).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    /// Where a path comes from, for diagnostics.
    pub fn origin(&self, path: &str) -> Option<String> {
        self.files.get(&normalize(path)).map(|(_, s)| match s {
            Source::Loose(p) => p.display().to_string(),
            Source::Packed { archive, .. } => self.archives[*archive].path.display().to_string(),
        })
    }
}

fn find_child(dir: &Path, lower_name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == lower_name)
    })
}

fn walk(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// A texture, by its normalized path: what an archive a pack replaced still lends.
fn is_texture(path: &str) -> bool {
    path.ends_with(".dds") || path.ends_with(".tga")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overlay_replaces_loose_files() {
        let root = std::env::temp_dir().join(format!("bytefield-vfs-{}", std::process::id()));
        let (game, pack) = (root.join("game"), root.join("pack"));
        for (dir, name, text) in [(&game, "a.con", "game"), (&game, "b.con", "game"), (&pack, "A.con", "pack")] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(name), text).unwrap();
        }
        let mut v = Vfs::new();
        v.mount_mod_with(&game, std::slice::from_ref(&pack)).unwrap();
        assert_eq!(v.read("a.con").unwrap(), b"pack");
        assert_eq!(v.read("b.con").unwrap(), b"game");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_level_brings_its_meshes_and_hud_pictures() {
        let root = std::env::temp_dir().join(format!("bytefield-vfs-level-{}", std::process::id()));
        let lvl = root.join("bf1942/levels/Pod");
        for (dir, name) in [("Menu/Texture/vehicle", "Icon_Pod.dds"), ("StandardMesh", "pod.sm"), ("Menu", "thumbnail.dds")] {
            std::fs::create_dir_all(lvl.join(dir)).unwrap();
            std::fs::write(lvl.join(dir).join(name), "x").unwrap();
        }
        let mut v = Vfs::new();
        v.mount_mod(&root).unwrap();
        assert_eq!(v.mount_level_assets("bf1942/levels/Pod"), 2);
        assert!(v.exists("menu/texture/vehicle/icon_pod.dds") && v.exists("standardmesh/pod.sm") && !v.exists("menu/thumbnail.dds"));
        let _ = std::fs::remove_dir_all(root);
    }

    /// The level's `texture/` gives what the mod lacks, never over it.
    #[test]
    fn a_level_texture_does_not_hide_the_mod_s() {
        let root = std::env::temp_dir().join(format!("bytefield-vfs-leveltex-{}", std::process::id()));
        for (dir, name, text) in [("texture", "Sky_01.dds", "mod"), ("bf1942/levels/B/texture", "Sky_01.dds", "level"), ("bf1942/levels/B/texture", "new.dds", "level")] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(name), text).unwrap();
        }
        let mut v = Vfs::new();
        v.mount_mod(&root).unwrap();
        assert_eq!(v.mount_level_assets("bf1942/levels/B"), 1);
        assert_eq!(v.read("texture/sky_01.dds").unwrap(), b"mod");
        assert_eq!(v.read("texture/new.dds").unwrap(), b"level");
        let _ = std::fs::remove_dir_all(root);
    }

    /// An uncompressed `.rfa` with these files.
    fn write_rfa(path: &Path, files: &[(&str, &str)]) {
        let mut data = vec![0u8; 8 + 148];
        let mut table = (files.len() as u32).to_le_bytes().to_vec();
        for (name, text) in files {
            let offset = data.len() as u32;
            data.extend_from_slice(text.as_bytes());
            table.extend_from_slice(&(name.len() as u32).to_le_bytes());
            table.extend_from_slice(name.as_bytes());
            for v in [text.len() as u32, text.len() as u32, offset, 0, 0, 0] {
                table.extend_from_slice(&v.to_le_bytes());
            }
        }
        let at = data.len() as u32;
        data[0..4].copy_from_slice(&at.to_le_bytes());
        data.extend(table);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    /// DC_Final as 7-Zip unpacks it (every `.rfa` loose in `Archives/`) and as it installs (`bf1942/game.rfa`,
    /// `bf1942/levels/*.rfa`): the same files win. In the real data only `sound.rfa`
    /// and its patch share paths, and they sit in the same place in both trees.
    // TODO: two archives in different places with the same path would win by place here (`""` before `bf1942/`); the
    // original's order for that case is not known. None in any mod we have (bf1942, XPack1,
    // XPack2, DC_Final, DesertCombat, GC_Redux, graphicsplus).
    #[test]
    fn loose_and_nested_archives_agree() {
        let root = std::env::temp_dir().join(format!("bytefield-vfs-trees-{}", std::process::id()));
        let game = [("bf1942/game/damage.con", "game")];
        let berlin = [("bf1942/levels/Berlin/Init.con", "base"), ("bf1942/levels/Berlin/a.con", "base")];
        let patch = [("bf1942/levels/Berlin/Init.con", "patch")];
        let objects = [("objects/tank/objects.con", "obj")];
        let lists: Vec<Vec<(String, Vec<u8>)>> = ["", "bf1942/levels/"]
            .iter()
            .map(|lv| {
                let dir = root.join(if lv.is_empty() { "loose" } else { "nested" });
                let a = dir.join("Archives");
                let g = if lv.is_empty() { "" } else { "bf1942/" };
                write_rfa(&a.join(format!("{g}Game.rfa")), &game);
                write_rfa(&a.join(format!("{lv}Berlin.rfa")), &berlin);
                write_rfa(&a.join(format!("{lv}Berlin_001.rfa")), &patch);
                write_rfa(&a.join("objects.rfa"), &objects);
                let mut v = Vfs::new();
                v.mount_mod(&dir).unwrap();
                v.list().iter().map(|p| (p.to_string(), v.read(p).unwrap())).collect()
            })
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(lists[0], lists[1]);
        assert_eq!(lists[0].len(), 4);
        assert!(lists[0].contains(&("bf1942/levels/Berlin/Init.con".into(), b"patch".to_vec())));
    }

    /// A mod kept unpacked (GC from its sources) with a patch archive over one of its levels: the patch wins over the
    /// loose files it carries, a base archive does not, and the loose files it lacks stay.
    #[test]
    fn a_patch_goes_over_loose_files() {
        let root = std::env::temp_dir().join(format!("bytefield-vfs-patchloose-{}", std::process::id()));
        let lvl = root.join("Archives/bf1942/levels/Hoth/Conquest");
        std::fs::create_dir_all(&lvl).unwrap();
        for name in ["ObjectSpawns.con", "SoldierSpawns.con", "Extra.con"] {
            std::fs::write(lvl.join(name), "loose").unwrap();
        }
        write_rfa(&root.join("Archives/bf1942/levels/Hoth_009.rfa"), &[("bf1942/levels/Hoth/Conquest/ObjectSpawns.con", "patch")]);
        write_rfa(&root.join("Archives/bf1942/levels/Hoth.rfa"), &[("bf1942/levels/Hoth/Conquest/SoldierSpawns.con", "base")]);
        let mut v = Vfs::new();
        v.mount_mod(&root).unwrap();
        let read = |p: &str| String::from_utf8(v.read(p).unwrap()).unwrap();
        let got = [read("bf1942/levels/hoth/conquest/objectspawns.con"), read("bf1942/levels/hoth/conquest/soldierspawns.con"), read("bf1942/levels/hoth/conquest/extra.con")];
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(got, ["patch", "loose", "loose"]);
    }

    /// A pack's level archive over a patched install: what it changed wins over the patch, what it left as it was
    /// (XPack1's levels in the HD Remaster's XPack pack, from before the 1.6 patch) does not.
    #[test]
    fn a_pack_wins_over_the_patches_only_where_it_changed() {
        let root = std::env::temp_dir().join(format!("bytefield-vfs-pack-patch-{}", std::process::id()));
        let lv = "bf1942/levels/";
        let (game, pack) = (root.join("game/Mods/bf1942"), root.join("pack/Mods/bf1942"));
        write_rfa(&game.join(format!("Archives/{lv}Berlin.rfa")), &[("bf1942/levels/Berlin/Init.con", "old"), ("bf1942/levels/Berlin/AI.con", "old"), ("bf1942/levels/Berlin/x.con", "old")]);
        write_rfa(&game.join(format!("Archives/{lv}Berlin_003.rfa")), &[("bf1942/levels/Berlin/Init.con", "1.6"), ("bf1942/levels/Berlin/AI.con", "1.6"), ("bf1942/levels/Berlin/new.con", "1.6")]);
        write_rfa(
            &pack.join(format!("Archives/{lv}Berlin.rfa")),
            &[("bf1942/levels/Berlin/Init.con", "1.6 + sky"), ("bf1942/levels/Berlin/AI.con", "old"), ("bf1942/levels/Berlin/x.con", "old"), ("bf1942/levels/Berlin/sky.con", "pack")],
        );
        let mut v = Vfs::new();
        v.mount_mod_with(&game, std::slice::from_ref(&pack)).unwrap();
        let read = |p: &str| String::from_utf8(v.read(p).unwrap()).unwrap();
        assert_eq!(read("bf1942/levels/Berlin/Init.con"), "1.6 + sky", "changed by the pack");
        assert_eq!(read("bf1942/levels/Berlin/AI.con"), "1.6", "left as before the patch");
        assert_eq!(read("bf1942/levels/Berlin/new.con"), "1.6");
        assert_eq!(read("bf1942/levels/Berlin/sky.con"), "pack");
        // without the pack, as always
        let mut v = Vfs::new();
        v.mount_mod(&game).unwrap();
        assert_eq!(String::from_utf8(v.read("bf1942/levels/Berlin/Init.con").unwrap()).unwrap(), "1.6");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn textures() {
        assert!(is_texture("texture/fletch_alfa_z.dds") && is_texture("texture/objectlightmaps/fletch_hull_m1.tga"));
        assert!(!is_texture("objects/fletcher/objects.con"));
    }

    #[test]
    fn patch_levels() {
        assert_eq!(patch_level("Berlin_003.rfa"), ("berlin".into(), 3));
        assert_eq!(patch_level("Berlin.rfa"), ("berlin".into(), -1));
        assert_eq!(patch_level("Battle_of_the_Bulge.rfa"), ("battle_of_the_bulge".into(), -1));
        assert_eq!(patch_level("StandardMesh_001.rfa"), ("standardmesh".into(), 1));
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize("bf1942\\Levels\\Berlin\\..\\Wake/./Init.con"), "bf1942/levels/wake/init.con");
    }
}
