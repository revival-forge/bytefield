//! A part touches (its collision vertices) when it is mobile and has `hasCollisionPhysics`, whatever `hasResponsePhysics`
//! says: the original checks GC's AT-AT feet (`hasResponsePhysics 0`) every frame, and they hit the snow from the
//! frame it lands; not the Lambda's front and back gears (mobile, with no collision line).
fn touching(s: &bytefield_sim::Sim, k: usize, name: &str) -> Vec<bool> {
    s.vehicles[k].contacts.iter().filter(|c| c.name.eq_ignore_ascii_case(name)).map(|c| !c.vertices.is_empty()).collect()
}

#[test]
fn the_atat_feet_touch_and_the_lambda_bay_gears_do_not() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Hoth", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let (x, z) = (1394.9, 1837.0);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 10.3;
    let k = s.spawn_vehicle("atat", [x, y, z], [0.0; 3]).unwrap();
    for foot in ["footl", "footr"] {
        let t = touching(&s, k, foot);
        assert!(!t.is_empty() && t.iter().all(|&v| v), "{foot}: {t:?}");
    }
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_BCanyon", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let (x, z) = (691.88, 1590.76);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let k = s.spawn_vehicle("epsilon", [x, y, z], [0.0; 3]).unwrap();
    for gear in ["LambdaLandingGearFront", "LambdaLandingGearBack"] {
        assert!(touching(&s, k, gear).iter().all(|&v| !v), "{gear}");
    }
    assert!(touching(&s, k, "LambdaLandingGearLeft").iter().any(|&v| v));
}
