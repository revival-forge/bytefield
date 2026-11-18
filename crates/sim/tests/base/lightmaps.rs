//! The statics' lightmaps: every one in El Alamein's archive is found by a draw
//! (`scene::Draw::lightmap`: the GeometryTemplate's name and the truncated position) but the destroyed
//! geometry's (`_m2`, not drawn). Skipped without the real data.
#[test]
fn every_intact_lightmap_of_el_alamein_has_its_static() {
    let Some(game) = bytefield_testdata::with_mod("bf1942") else { return; };
    let w = bytefield_sim::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
    let prefix = "bf1942/levels/el_alamein/objectlightmaps/";
    let tgas: Vec<String> = w.vfs.list().iter().map(|p| p.to_ascii_lowercase()).filter_map(|p| Some(p.strip_prefix(prefix)?.strip_suffix(".tga")?.to_string())).collect();
    let keys: std::collections::BTreeSet<String> = bytefield_world::scene::expand_level(&w.db).draws.iter().map(|d| d.lightmap()).collect();
    let missing: Vec<&String> = tgas.iter().filter(|t| !t.contains("_m2_") && !keys.contains(*t)).collect();
    assert_eq!(tgas.len(), 76);
    assert!(missing.is_empty(), "{missing:?}");
}
