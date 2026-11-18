//! The weapon in the soldier's hand.
//!
//! A hand weapon (`HandFireArms`) has its own skeleton (`createSkeleton animations/No4.ske`) whose
//! root bone is named as the soldier's right hand (`Bip01 R Hand`): the weapon hangs from that
//! bone of the posed soldier. Its main bone (`useSkeletonPartAsMain`, `BaseNo4` for `No4`: the
//! name exact or, failing that, contained) carries the meshes of its `*Complex` (`AnimatedBundle`,
//! the first of its `LodObject`): the main one and the pieces of `bindToSkeletonPart`. All are
//! modelled in the main bone's frame as the `.ske` stands; a piece follows its bone away from
//! there when the weapon's own animation moves it (the bolt, the magazine: the soldier's state
//! names the weapon's, `setOtherState c_AsmWeaponState`).
//!
//! The main bone's z points back along the barrel and its x to the left: a mesh is its bone
//! turned half a turn about y ([`MESH`]; aiming, standing, the main bone is the soldier's frame
//! so turned to 0.1°).


use std::sync::Arc;

use bytefield_con::templates::TemplateDb;
use bytefield_formats::anim::{BoneAnimation, Skeleton as SkeletonFile};
use bytefield_sim::skeleton::{bone_matrix, mult, M4};
use bytefield_vfs::Vfs;

/// The soldier's bone the weapon's root stands for.
pub const HAND: &str = "Bip01 R Hand";

/// A mesh's frame in its bone's (row vectors): x → −x, z → −z.
pub const MESH: M4 = [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// One mesh of the weapon: its geometry kind and file, and the bone it follows.
#[derive(Debug, Clone)]
pub struct Piece {
    pub kind: String,
    pub file: String,
    pub bone: usize,
}

/// A weapon's model: its skeleton and meshes.
#[derive(Debug, Clone)]
pub struct Model {
    pub skeleton: Arc<SkeletonFile>,
    /// the first follows the main bone (`useSkeletonPartAsMain`)
    pub pieces: Vec<Piece>,
}

fn geometry(db: &TemplateDb, t: &bytefield_con::templates::Template) -> Option<(String, String)> {
    let g = t.get("geometry")?.first()?;
    let gt = db.get("geometrytemplate", g)?;
    let file = gt.get("file")?.first()?;
    Some((gt.kind.to_ascii_lowercase(), file.replace('\\', "/")))
}

/// `addTemplate` children with their `bindToSkeletonPart`, in order.
fn bound_children(t: &bytefield_con::templates::Template) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for p in &t.props {
        match p.name.as_str() {
            "addtemplate" => out.extend(p.args.first().map(|n| (n.clone(), None))),
            "bindtoskeletonpart" => {
                if let (Some(last), Some(b)) = (out.last_mut(), p.args.first()) {
                    last.1 = Some(b.clone());
                }
            }
            // (DC's `RemingtonLod`: `addTemplate RemingtonComplex` + `setRandomGeometries 3` is RemingtonComplex1..3; the first,
            // as the sim's; the shotgun used to be left undrawn in the first person)
            "setrandomgeometries" => {
                if let Some(last) = out.last_mut() {
                    last.0 = bytefield_world::scene::random_child(&last.0, &p.args);
                }
            }
            _ => {}
        }
    }
    out
}

/// The model of a hand weapon template, if it has a skeleton that loads.
pub fn load(db: &TemplateDb, vfs: &Vfs, weapon: &str) -> Option<Model> {
    let t = db.object_template(weapon)?;
    let ske = t.get("createskeleton")?.first()?.replace('\\', "/");
    let skeleton = Arc::new(SkeletonFile::parse(&vfs.read(&ske).ok()?).ok()?);
    let find = |name: &str| {
        let n = name.to_ascii_lowercase();
        skeleton.index(name).or_else(|| skeleton.bones.iter().position(|b| b.name.to_ascii_lowercase().contains(&n)))
    };
    let main = t.get("useskeletonpartasmain").and_then(|a| a.first()).and_then(|n| find(n)).unwrap_or(1.min(skeleton.bones.len() - 1));
    // the LodObject among its children, and its first (the complex one)
    let lod = bound_children(t).into_iter().filter_map(|(c, _)| db.object_template(&c)).find(|c| c.kind.eq_ignore_ascii_case("lodobject"))?;
    let complex = db.object_template(&bound_children(lod).first()?.0)?;
    let mut pieces = Vec::new();
    if let Some((kind, file)) = geometry(db, complex) {
        pieces.push(Piece { kind, file, bone: main });
    }
    for (c, bone) in bound_children(complex) {
        let Some(ct) = db.object_template(&c) else { continue };
        if let Some((kind, file)) = geometry(db, ct) {
            pieces.push(Piece { kind, file, bone: bone.and_then(|b| find(&b)).unwrap_or(main) });
        }
    }
    Some(Model { skeleton, pieces })
}

impl Model {
    /// The bones relative to their parents: the `.ske`'s, with an animation at a time (0..1)
    /// over them (as the soldier's).
    pub fn rel(&self, anim: Option<(&BoneAnimation, f32, bool)>) -> Vec<M4> {
        match anim {
            None => self.skeleton.bones.iter().map(bone_matrix).collect(),
            Some((a, t, looping)) => {
                let mut sk = bytefield_sim::skeleton::Skeleton::new(self.skeleton.clone(), Vec::new(), 0.0);
                sk.apply(a, t, 1.0, looping);
                sk.rel
            }
        }
    }

    /// Each weapon bone's matrix in the soldier's frame, its root at `hand` (the soldier's hand
    /// bone, absolute), from [`Self::rel`].
    pub fn bones(&self, hand: &M4, rel: &[M4]) -> Vec<M4> {
        let mut abs: Vec<M4> = Vec::with_capacity(rel.len());
        for (i, b) in self.skeleton.bones.iter().enumerate() {
            let m = match b.parent {
                Some(p) if p < i => mult(&rel[i], &abs[p]),
                _ => *hand,
            };
            abs.push(m);
        }
        abs
    }

    /// Where a piece's mesh is, in the soldier's frame, with `bones` from [`Self::bones`]: on its bone, as the original draws it
    /// (the original's rendering of the M16 and the M249: the M249's cartridges hang from `bullet01..06`).
    pub fn piece(&self, bones: &[M4], piece: usize) -> M4 {
        mult(&MESH, &bones[self.pieces[piece].bone])
    }
}

#[cfg(test)]
mod with_data {
    use super::*;

    #[test]
    fn the_no4_hangs_from_the_hand() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let m = load(&w.db, &w.vfs, "No4").expect("No4");
        assert_eq!(m.skeleton.bones[0].name, HAND);
        assert_eq!(m.skeleton.bones[m.pieces[0].bone].name, "BaseNo4");
        assert!(m.pieces.len() >= 5, "{:?}", m.pieces);
        for p in &m.pieces {
            assert!(w.vfs.read(&format!("standardmesh/{}.sm", p.file)).is_ok(), "{p:?}");
        }
        // the stock a few centimetres from the hand
        let abs = m.bones(&bytefield_sim::skeleton::IDENTITY, &m.rel(None));
        let p = &abs[m.pieces[0].bone][12..15];
        assert!(p.iter().map(|v| v * v).sum::<f32>().sqrt() < 0.2, "{p:?}");
    }

    /// Every hand weapon of the game has a model: a skeleton hung from the hand and meshes.
    #[test]
    fn every_hand_weapon_has_a_model() {
        let game = bytefield_testdata::require!(Base);
        let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut none = Vec::new();
        let mut n = 0;
        for t in w.db.templates.values().filter(|t| t.namespace == "objecttemplate" && t.kind.eq_ignore_ascii_case("handfirearms")) {
            n += 1;
            match load(&w.db, &w.vfs, &t.name) {
                Some(m) if !m.pieces.is_empty() && m.skeleton.bones[0].name == HAND => {}
                m => none.push(format!("{} ({:?})", t.name, m.map(|m| (m.pieces.len(), m.skeleton.bones[0].name.clone())))),
            }
        }
        eprintln!("{n} hand weapons; without a model: {none:?}");
        assert!(none.len() * 10 < n, "{none:?}");
    }
}
