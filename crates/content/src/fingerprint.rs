//! Which version of the game a folder holds: size and MD5 of its files against `fingerprints.json`, made by
//! `python/tools/fingerprints.py`. It says what it sees and forbids nothing: an edition or a patch not in the table may play well.

use std::io::Read;
use std::path::Path;

use bytefield_con::session::resolve_ci;
use md5::{Digest, Md5};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Table {
    /// the folder under `Mods/`
    pub folder: String,
    pub version: String,
    /// (the path from that folder, in any case; its size; its MD5)
    pub files: Vec<(String, u64, String)>,
}

pub fn tables() -> &'static [Table] {
    static T: std::sync::OnceLock<Vec<Table>> = std::sync::OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("../fingerprints.json")).expect("fingerprints.json"))
}

/// A folder against one version.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub folder: String,
    pub version: String,
    pub same: usize,
    pub different: Vec<String>,
    pub missing: Vec<String>,
}

impl Check {
    pub fn matches(&self) -> bool {
        self.different.is_empty() && self.missing.is_empty()
    }
}

fn md5_of(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Md5::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        match f.read(&mut buf)? {
            0 => break,
            n => h.update(&buf[..n]),
        }
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The folder at `dir` against one table. A different size is enough: the hash is only read when the size agrees.
pub fn check(dir: &Path, table: &Table) -> Check {
    let mut c = Check { folder: table.folder.clone(), version: table.version.clone(), same: 0, different: vec![], missing: vec![] };
    for (rel, size, md5) in &table.files {
        let Some(p) = resolve_ci(dir, rel).filter(|p| p.is_file()) else {
            c.missing.push(rel.clone());
            continue;
        };
        let same = p.metadata().is_ok_and(|m| m.len() == *size) && md5_of(&p).is_ok_and(|m| m.eq_ignore_ascii_case(md5));
        if same {
            c.same += 1;
        } else {
            c.different.push(rel.clone());
        }
    }
    c
}

/// The folders in the table that are in this install, each against the version it comes closest to.
pub fn check_install(install: &Path) -> Vec<Check> {
    check_with(install, tables())
}

fn check_with(install: &Path, tables: &[Table]) -> Vec<Check> {
    let mut out: Vec<Check> = vec![];
    for t in tables {
        let Some(dir) = resolve_ci(install, &format!("Mods/{}", t.folder)) else { continue };
        let c = check(&dir, t);
        match out.iter_mut().find(|o| o.folder == c.folder) {
            Some(o) if c.same > o.same => *o = c,
            Some(_) => {}
            None => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(version: &str, files: &[(&str, &[u8])]) -> Table {
        let hash = |d: &[u8]| Md5::digest(d).iter().map(|b| format!("{b:02x}")).collect::<String>();
        Table { folder: "bf1942".into(), version: version.into(), files: files.iter().map(|(n, d)| (n.to_string(), d.len() as u64, hash(d))).collect() }
    }

    #[test]
    fn the_known_md5_of_a_text() {
        let d = std::env::temp_dir().join(format!("bytefield-md5-{}", std::process::id()));
        std::fs::write(&d, "abc").unwrap();
        assert_eq!(md5_of(&d).unwrap(), "900150983cd24fb0d6963f7d28e17f72");
        let _ = std::fs::remove_file(&d);
    }

    #[test]
    fn the_nearest_version_and_what_differs() {
        let root = std::env::temp_dir().join(format!("bytefield-fingerprint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("Mods/BF1942/Archives");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Game.rfa"), b"game").unwrap();
        std::fs::write(dir.join("menu.rfa"), b"mine").unwrap();
        let old = table("1.0", &[("archives/game.rfa", b"gone"), ("archives/menu.rfa", b"menu")]);
        let new = table("1.6", &[("archives/game.rfa", b"game"), ("archives/menu.rfa", b"menu"), ("archives/sound.rfa", b"x")]);
        let c = &check_with(&root, &[old, new])[0];
        assert_eq!((c.version.as_str(), c.same), ("1.6", 1));
        // (the same size: only the hash tells it)
        assert_eq!(c.different, ["archives/menu.rfa"]);
        assert_eq!(c.missing, ["archives/sound.rfa"]);
        assert!(!c.matches());
        // what the install lacks is not reported
        assert!(check_with(&root.join("nothing"), &[table("1.6", &[])]).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_table_reads_and_has_the_game() {
        for f in ["bf1942", "XPack1", "XPack2"] {
            assert!(tables().iter().any(|t| t.folder == f && t.version == "1.6" && t.files.iter().any(|(n, _, _)| n == "init.con")), "{f}");
        }
    }
}
