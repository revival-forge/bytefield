//! `content.cfg`: what the player has and turns on, as `openmw.cfg` does it: `key=value`
//! lines, some keys repeated, in `$XDG_CONFIG_HOME/bytefield/content.cfg` (or `~/.config/bytefield/…`;
//! `BYTEFIELD_CONTENT` names another). Unknown keys, comments and broken lines are skipped.
//!
//! ```text
//! install=/games/bf1942
//! mods_dir=/games/hd/custom/Mods
//! pack=hd-base|/games/hd/base/Graphics Mod|on
//! graphics=enhanced
//! aniso=8
//! ```
//!
//! Only the client and the `bytefield-content` tool read it: the bench and the tests go by their
//! arguments and variables alone.

use std::path::{Path, PathBuf};

use crate::graphics::{Overrides, Profile};

/// A graphics pack: a tree like the install's (`Mods/<x>/Archives/…`), laid over it as if copied.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub name: String,
    pub path: PathBuf,
    pub on: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub install: Option<PathBuf>,
    /// folders with more mods (`<mod>/init.con`…) besides the install's `Mods`
    pub mods_dirs: Vec<PathBuf>,
    /// in order: the later ones over the earlier
    pub packs: Vec<Pack>,
    pub graphics: Profile,
    pub overrides: Overrides,
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "install" if !v.is_empty() => c.install = Some(v.into()),
                "mods_dir" if !v.is_empty() => c.mods_dirs.push(v.into()),
                "pack" => {
                    let mut f = v.split('|').map(str::trim);
                    let (Some(name), Some(path)) = (f.next(), f.next()) else { continue };
                    if name.is_empty() || path.is_empty() || c.pack(name).is_some() {
                        continue;
                    }
                    let on = f.next().is_none_or(|s| s != "off");
                    c.packs.push(Pack { name: name.into(), path: path.into(), on });
                }
                "graphics" => c.graphics = Profile::parse(v).unwrap_or(c.graphics),
                // (the loose graphics settings; anything else is skipped)
                k => {
                    let _ = c.overrides.set(k, v);
                }
            }
        }
        c
    }

    pub fn write(&self) -> String {
        let mut s = String::from("# bytefield: content\n");
        if let Some(i) = &self.install {
            s += &format!("install={}\n", i.display());
        }
        for d in &self.mods_dirs {
            s += &format!("mods_dir={}\n", d.display());
        }
        for p in &self.packs {
            s += &format!("pack={}|{}|{}\n", p.name, p.path.display(), if p.on { "on" } else { "off" });
        }
        s += &format!("graphics={}\n", self.graphics.name());
        for (k, v) in self.overrides.entries() {
            s += &format!("{k}={v}\n");
        }
        s
    }

    pub fn pack(&self, name: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn pack_mut(&mut self, name: &str) -> Option<&mut Pack> {
        self.packs.iter_mut().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// `name`, or `name-2`… if a pack has it.
    /// A folder taken as if dropped in a content folder (`bytefield-content add`, or dropped on the menu's window): a pack, a
    /// mod (its folder above becomes a `mods_dir`) or a folder of mods. What was added, as a line, or why not.
    pub fn add_dir(&mut self, d: &std::path::Path) -> Result<String, String> {
        use crate::discover::{self, Found};
        match discover::classify(d, self.install.as_deref()) {
            Some(Found::Pack(p)) => {
                if let Some(q) = self.packs.iter().find(|q| q.path == p) {
                    return Ok(format!("pack {}: {}", q.name, p.display()));
                }
                let name = self.unique_pack_name(crate::detect::pack_name(&p));
                let line = format!("pack {name}: {}", p.display());
                self.packs.push(crate::Pack { name, path: p, on: true });
                Ok(line)
            }
            Some(f @ (Found::Mod(_) | Found::Mods(_))) => {
                let dir = match f {
                    Found::Mod(m) => m.parent().ok_or("a mod with no folder above")?.to_path_buf(),
                    Found::Mods(m) => m,
                    Found::Pack(_) => unreachable!(),
                };
                let line = format!("mods_dir: {}", dir.display());
                if !self.mods_dirs.contains(&dir) {
                    self.mods_dirs.push(dir);
                }
                Ok(line)
            }
            // a folder of mods (`…/Mods`)
            None if discover::scan(std::slice::from_ref(&d.to_path_buf()), None).iter().any(|f| matches!(f, Found::Mod(_))) => {
                if !self.mods_dirs.iter().any(|m| m == d) {
                    self.mods_dirs.push(d.to_path_buf());
                }
                Ok(format!("mods_dir: {}", d.display()))
            }
            None => Err(format!("{}: neither a pack (Mods/ inside) nor a mod (init.con) nor a folder of mods", d.display())),
        }
    }

    pub fn unique_pack_name(&self, name: String) -> String {
        let mut v = name.clone();
        let mut n = 2;
        while self.pack(&v).is_some() {
            v = format!("{name}-{n}");
            n += 1;
        }
        v
    }

    /// The packs turned on, in order.
    pub fn active_packs(&self) -> Vec<PathBuf> {
        self.packs.iter().filter(|p| p.on).map(|p| p.path.clone()).collect()
    }

    /// The active packs in `BYTEFIELD_HD`'s form (`dir:dir…`), for `con::session::set_overlays`.
    pub fn overlays_string(&self) -> String {
        std::env::join_paths(self.active_packs()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Hands the mods folders and the active packs to the loader (`con::session`).
    pub fn apply_to_session(&self) {
        bytefield_con::session::set_mods_dirs(&self.mods_dirs);
        bytefield_con::session::set_overlays(&self.overlays_string());
    }

    /// The file's (None if there is none, or no home to find it in).
    pub fn load() -> Option<Config> {
        Self::load_from(&path()?)
    }

    pub fn load_from(p: &Path) -> Option<Config> {
        std::fs::read_to_string(p).ok().map(|t| Config::parse(&t))
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let p = path().ok_or("no config folder (HOME unset)")?;
        self.save_to(&p)?;
        Ok(p)
    }

    pub fn save_to(&self, p: &Path) -> Result<(), String> {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(p, self.write()).map_err(|e| format!("{}: {e}", p.display()))
    }
}

/// `content.cfg`'s place.
pub fn path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("BYTEFIELD_CONTENT") {
        return Some(PathBuf::from(p));
    }
    Some(config_dir()?.join("content.cfg"))
}

/// `$XDG_CONFIG_HOME/bytefield` (or `~/.config/bytefield`); on Windows `%APPDATA%\bytefield`, where there is no `HOME`
/// (Wine passes Linux's, so it never showed there).
pub fn config_dir() -> Option<PathBuf> {
    let xdg = || std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = || std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"));
    let appdata = || std::env::var_os("APPDATA").map(PathBuf::from);
    let base = if cfg!(windows) { xdg().or_else(appdata).or_else(home) } else { xdg().or_else(home).or_else(appdata) }?;
    Some(base.join("bytefield"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let text = "install=/g\nmods_dir=/m1\nmods_dir=/m2\npack=a|/p/a|on\npack=b|/p/b c|off\npack=a|/dup|on\ngraphics=mejorado\naniso=8\nlook=warm\nbloom=x\nbroken\n# x=y\nzzz=1\n";
        let c = Config::parse(text);
        assert_eq!(c.install, Some("/g".into()));
        assert_eq!(c.mods_dirs.len(), 2);
        assert_eq!(c.packs.len(), 2, "a repeated name is skipped");
        assert_eq!(c.packs[1], Pack { name: "b".into(), path: "/p/b c".into(), on: false });
        assert_eq!(c.graphics, Profile::Enhanced);
        assert_eq!(c.overrides.aniso, Some(8));
        assert_eq!((c.overrides.look, c.overrides.bloom), (Some(crate::graphics::Look::Warm), None), "a bad value is skipped");
        assert_eq!(c.active_packs(), vec![PathBuf::from("/p/a")]);
        assert_eq!(Config::parse(&c.write()), c);
    }

    /// `add_dir`: a mod's folder adds the one above as a `mods_dir`, once; what is
    /// neither a pack nor a mod is refused.
    #[test]
    fn a_dropped_mod_adds_its_mods_dir() {
        let root = std::env::temp_dir().join(format!("bytefield-add-dir-{}", std::process::id()));
        let m = root.join("Mods").join("MyMod");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(m.join("init.con"), "game.setActiveCombatArea 0\n").unwrap();
        let mut c = Config::default();
        assert!(c.add_dir(&m).is_ok());
        assert!(c.add_dir(&m).is_ok());
        assert_eq!(c.mods_dirs, vec![root.join("Mods")]);
        assert!(c.add_dir(&root.join("nothing")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn defaults_are_vanilla() {
        let c = Config::parse("");
        assert_eq!(c.graphics, Profile::Original);
        assert!(c.packs.is_empty() && c.overlays_string().is_empty());
    }
}
