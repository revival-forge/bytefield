//! The TEXTUREFACTOR of meshes and trees in the expansions against the original's, skipped without
//! the game's data.
use bytefield_world::{shadow, Level};

/// A standard mesh's TEXTUREFACTOR against the original's: Husky's reference run: a house's furniture (two beds, a table, a chair: within 3) and the two Kübelwagens (all
/// their pieces with their root's: 5–7 below).
#[test]
fn husky_meshes_as_the_original_darkens_them() {
    let Some(game) = bytefield_testdata::with_mod("XPack1") else { return };
    let Ok((_, vfs)) = bytefield_con::session::mount(&game, "XPack1") else { return };
    let it = bytefield_con::session::load(&vfs, Some("Husky"), "conquest");
    let db = it.host.db;
    let Ok(level) = Level::from_db(&vfs, &db, "Husky") else { return };
    let s = shadow::ShadowSampler::new(&vfs, &db, &level).expect("Husky has its .lsb");
    assert_eq!((s.ambient, s.cull_box, s.fade_time), ([100.0; 3], [5.0; 3], 0.075));
    // (where, its mesh, the reference run's byte, how far off we may be)
    let trace = [
        ([1339.9, 42.2384, 1423.64], "Eubed_M1", 0x81, 3),
        ([1342.3, 46.5134, 1423.79], "civiltable_m1", 0x89, 3),
        ([1342.9, 42.2384, 1423.64], "Eubed_M1", 0x8a, 3),
        ([1343.2001, 46.5134, 1423.3401], "euchair02_M1", 0x8c, 3),
        ([1352.0, 42.6, 1428.9], "Kubel_Hul_M1", 0x9f, 8),
        ([1356.2, 42.6, 1428.9], "Kubel_Hul_M1", 0xa0, 8),
        // (another run: the one driven, at two more places; all its pieces with one TF still)
        ([1334.2, 42.6, 1434.8], "Kubel_Hul_M1", 0x99, 3),
        ([1334.7, 42.6, 1446.5], "Kubel_Hul_M1", 0x95, 12),
    ];
    for (p, mesh, want, off) in trace {
        let b = vfs.read(&format!("standardmesh/{mesh}.sm")).ok().and_then(|b| bytefield_formats::sm::parse(&b).ok()).map(|m| m.bounds).unwrap_or_else(|| panic!("{mesh}"));
        assert!(s.takes_shadow(b), "{mesh}");
        let got = s.mesh_factor(s.object_intensity(&level, p, shadow::radius(b)))[0] as i32;
        eprintln!("{mesh} at {p:?}: {got:#x}, the original {want:#x}");
        assert!((got - want).abs() <= off, "{mesh} at {p:?}: {got:#x}, the original {want:#x}");
    }
    // the soldier driving a Kübelwagen, skinned: `skin_factor` (measured from the original's
    // draws of the skinning vertex shader), its radius about a metre
    for (p, want) in [([1351.7, 42.9, 1429.0], 0x84), ([1334.7, 42.9, 1446.2], 0x7f), ([1334.1, 42.9, 1435.1], 0x81), ([1351.6, 42.9, 1451.3], 0xf0)] {
        let got = s.skin_factor(s.object_intensity(&level, p, 1.0))[0] as i32;
        eprintln!("soldier at {p:?}: {got:#x}, the original {want:#x}");
        assert!((got - want).abs() <= 6, "soldier at {p:?}: {got:#x}, the original {want:#x}");
    }
    // a house is too big for the shadow
    let house = bytefield_formats::sm::parse(&vfs.read("standardmesh/italianhouse08_m1.sm").unwrap()).unwrap();
    assert!(!s.takes_shadow(house.bounds));
}

/// Salerno: every tree near the previous camera
/// white, though most stand in the baked shadow: none fits in the cull box (5/5/5), as El Alamein's palms.
#[test]
fn salerno_trees_as_the_original_draws_them() {
    let Some(game) = bytefield_testdata::with_mod("XPack1") else { return };
    let Ok((_, vfs)) = bytefield_con::session::mount(&game, "XPack1") else { return };
    let it = bytefield_con::session::load(&vfs, Some("salerno"), "conquest");
    let db = it.host.db;
    let Ok(level) = Level::from_db(&vfs, &db, "salerno") else { return };
    let s = shadow::ShadowSampler::new(&vfs, &db, &level).expect("Salerno has its .lsb");
    let exp = bytefield_world::scene::expand_level(&db);
    let trace = [
        (447.8, 433.3), (450.5, 425.2), (476.7, 404.3), (477.9, 405.4), (478.8, 403.6), (480.3, 404.2), (485.1, 386.3),
        (504.3, 368.9), (504.7, 367.1), (505.2, 371.8), (506.3, 370.0), (506.4, 367.7), (513.7, 371.6), (531.2, 428.1),
        (546.2, 407.3), (547.4, 436.0), (551.6, 434.9), (552.1, 438.7), (552.9, 436.7), (554.6, 435.4), (556.3, 439.2),
    ];
    let mut bad = Vec::new();
    for (x, z) in trace {
        let d = exp
            .draws
            .iter()
            .filter(|d| d.kind.eq_ignore_ascii_case("treemesh"))
            .min_by(|a, b| ((a.world.0[3][0] - x).powi(2) + (a.world.0[3][2] - z).powi(2)).total_cmp(&((b.world.0[3][0] - x).powi(2) + (b.world.0[3][2] - z).powi(2))))
            .unwrap();
        let tm = bytefield_formats::tm::parse(&vfs.read(&format!("treemesh/{}.tm", d.mesh)).unwrap()).unwrap();
        let p = [0, 1, 2].map(|i| d.world.0[3][i] as f32);
        let i = s.mesh_intensity(&level, p, tm.bounds);
        eprintln!("{} at {x},{z}: origin {}, intensity {i}", d.mesh, s.intensity(&level, p));
        if i < shadow::LIT {
            bad.push(format!("{} at {x},{z}: {i}", d.mesh));
        }
    }
    assert!(bad.is_empty(), "lit in the original: {bad:?}");
}
