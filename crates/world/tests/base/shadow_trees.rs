//! The trees' TEXTUREFACTOR against the original's: the 13 trees in view in the original's rendering of El Alamein
//!, skipped without the game's data.
use bytefield_world::{shadow, Level};

#[test]
fn el_alamein_trees_as_the_original_draws_them() {
    let Some(game) = bytefield_testdata::with_mod("bf1942") else { return };
    let Ok((_, vfs)) = bytefield_con::session::mount(&game, "bf1942") else { return };
    let it = bytefield_con::session::load(&vfs, Some("El_Alamein"), "conquest");
    let db = it.host.db;
    let Ok(level) = Level::from_db(&vfs, &db, "El_Alamein") else { return };
    let s = shadow::ShadowSampler::new(&vfs, &db, &level).expect("El Alamein has its .lsb");
    assert_eq!(s.ambient, [50.0; 3]);
    let exp = bytefield_world::scene::expand_level(&db);
    // (x, z, the reference run's red byte)
    let trace = [
        (1272.9, 653.2, 0x37), (1266.8, 647.6, 0x96), (1268.0, 652.4, 0x97), (1267.1, 648.3, 0x9a),
        (1261.8, 654.2, 0xff), (1262.8, 653.5, 0xff), (1262.9, 643.7, 0xff), (1268.6, 650.9, 0xff),
        (1272.8, 664.6, 0xff), (1273.8, 667.7, 0xff), (1275.9, 672.4, 0xff), (1277.1, 670.5, 0xff), (1281.6, 662.2, 0xff),
    ];
    for (x, z, want) in trace {
        let d = exp
            .draws
            .iter()
            .filter(|d| d.kind.eq_ignore_ascii_case("treemesh"))
            .min_by(|a, b| {
                let da = (a.world.0[3][0] - x).powi(2) + (a.world.0[3][2] - z).powi(2);
                let db_ = (b.world.0[3][0] - x).powi(2) + (b.world.0[3][2] - z).powi(2);
                da.total_cmp(&db_)
            })
            .unwrap();
        let tm = bytefield_formats::tm::parse(&vfs.read(&format!("treemesh/{}.tm", d.mesh)).unwrap()).unwrap();
        let p = [0, 1, 2].map(|i| d.world.0[3][i] as f32);
        let i = s.mesh_intensity(&level, p, tm.bounds);
        let colour = shadow::instance_colour(&db.instances[d.instance]);
        let got = shadow::tree_factor(i, s.ambient, colour)[0] as i32;
        // the original's rand % 8 − 4 on the colour
        eprintln!("{} at {x},{z}: {got:#x}, the original {want:#x} (intensity {i})", d.mesh);
        assert!((got - want).abs() <= 4, "{} at {x},{z}: {got:#x}, the original {want:#x} (intensity {i})", d.mesh);
    }
}

