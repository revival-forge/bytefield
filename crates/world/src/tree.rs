//! The object tree the server builds from a template: one way
//! for every kind. `*Template::createObject` only makes the object; a `Bundle`'s constructor
//! creates its children (every `addTemplate` in order, each by
//! its own `createObject`, so its children in turn), whatever the parent's or the child's kind. The
//! classes that derive only from `SimpleObject` are leaves: their `addTemplate`s are never created.
//! A `LodObject` is a `Bundle` whose `addBundleChilds` does nothing, but it makes all its LODs.
//! What an `ObjectSpawner` makes is not in its tree (it spawns it later).

use crate::scene::{self, Mat4};
use bytefield_con::templates::TemplateDb;

/// The kinds that are simple objects and not bundles (lowercase): they have no children.
pub const LEAF_KINDS: [&str; 16] = [
    "andcompositeobjective",
    "bfspawnpoint",
    "destroytargetobjective",
    "dummyobject",
    "emitter",
    "entrypoint",
    "flagbase",
    "lensflare",
    "light",
    "objectspawner",
    "objective",
    "simpleobject",
    "spawnpoint",
    "supplydepot",
    "timerobjective",
    "wheeltrack",
];

pub fn is_leaf(kind: &str) -> bool {
    LEAF_KINDS.iter().any(|k| kind.eq_ignore_ascii_case(k))
}

/// One object of the tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// the template it was created from, as the data spell it (a `setRandomGeometries` child: its
    /// first variant, as [`scene::random_child`])
    pub template: String,
    /// lowercase
    pub kind: String,
    /// its parent's index in the tree (the root has none)
    pub parent: Option<usize>,
    /// its matrix in the root's frame
    pub matrix: Mat4,
}

/// The tree the server creates from template `root` (the root first, then depth first in
/// `addTemplate` order); empty when there is no such template. A child whose template is missing
/// is skipped (the original logs `addBundleChilds template not found`), and so is anything below depth 16.
pub fn instantiate(db: &TemplateDb, root: &str) -> Vec<Node> {
    fn walk(db: &TemplateDb, name: &str, parent: Option<usize>, m: Mat4, depth: usize, out: &mut Vec<Node>) {
        if depth > 16 {
            return;
        }
        let Some(t) = db.object_template(name) else { return };
        let kind = t.kind.to_ascii_lowercase();
        let me = out.len();
        out.push(Node { template: name.to_string(), kind: kind.clone(), parent, matrix: m });
        if is_leaf(&kind) {
            return;
        }
        for (child, local) in scene::children(t) {
            walk(db, &child, Some(me), m.mul(&local), depth + 1, out);
        }
        // The weapon's init, after the weapon is in its parent (not `addBundleChilds`): its visible
        // dummy projectiles (`RPG7`'s `RPGDummy`; `createAllDummyProjectiles`: one at each
        // `addFireArmsPosition`, or one at the weapon without positions), then its visible barrels
        // (`createAllBarrels`: one at each position, none without), each turned as the weapon
        if kind.ends_with("firearms") {
            let at: Vec<Mat4> = t
                .all("addfirearmsposition")
                .into_iter()
                .map(|a| Mat4::from_pos_ypr(scene::vec3(a).unwrap_or([0.0; 3]), [0.0; 3]))
                .collect();
            if let Some(d) = t.get("visibledummyprojectiletemplate").and_then(|a| a.first()) {
                for p in if at.is_empty() { vec![Mat4::IDENTITY] } else { at.clone() } {
                    walk(db, d.trim(), Some(me), m.mul(&p), depth + 1, out);
                }
            }
            if let Some(b) = t.get("visiblebarreltemplate").and_then(|a| a.first()) {
                for p in &at {
                    walk(db, b.trim(), Some(me), m.mul(p), depth + 1, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(db, root, None, Mat4::IDENTITY, 0, &mut out);
    out
}

/// The objects of kind `kind` (lowercase) under template `root`, with their matrices in the root's
/// frame (the root itself is not one of them).
pub fn of_kind(db: &TemplateDb, root: &str, kind: &str) -> Vec<(String, Mat4)> {
    instantiate(db, root).into_iter().skip(1).filter(|n| n.kind == kind).map(|n| (n.template, n.matrix)).collect()
}
