//! What we know of the content people play with: names, versions, where
//! to get it and what it needs. Links only: nothing of theirs is shipped.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// `base`, `expansion`, `mod` or `pack`
    pub kind: String,
    /// a mod's folder under `Mods/` (base, expansion, mod)
    #[serde(default)]
    pub folder: Option<String>,
    /// the `setCustomGameVersion`s we know
    #[serde(default)]
    pub versions: Vec<String>,
    /// for a pack: files (from its root) that tell it apart
    #[serde(default)]
    pub markers: Vec<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub note: String,
    /// ours only, never offered to players as something to get (a mod for our own tests)
    #[serde(default)]
    pub internal: bool,
}

pub fn catalog() -> &'static [Entry] {
    static C: std::sync::OnceLock<Vec<Entry>> = std::sync::OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("../catalog.json")).expect("catalog.json"))
}

/// The entry of a mod folder.
pub fn by_folder(folder: &str) -> Option<&'static Entry> {
    catalog().iter().find(|e| e.folder.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(folder)))
}

pub fn by_id(id: &str) -> Option<&'static Entry> {
    catalog().iter().find(|e| e.id.eq_ignore_ascii_case(id))
}

/// The pack entry a tree is (all its markers there).
pub fn pack_at(root: &std::path::Path) -> Option<&'static Entry> {
    catalog().iter().filter(|e| e.kind == "pack" && !e.markers.is_empty()).find(|e| e.markers.iter().all(|m| bytefield_con::session::resolve_ci(root, m).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_needs_exist() {
        let c = catalog();
        assert!(c.len() >= 10);
        for e in c {
            for n in &e.needs {
                assert!(by_id(n).is_some(), "{} needs unknown {n}", e.id);
            }
        }
        assert_eq!(by_folder("dc_final").unwrap().id, "dc_final");
    }
}
