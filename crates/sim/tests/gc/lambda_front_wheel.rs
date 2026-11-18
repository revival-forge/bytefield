//! A spring's chain of links follows the moving parts above it: the Lambda's front wheel hangs from a `RotationalBundle`
//! under a landing gear that turns to -7,5 degrees, and the original pushes from where the turned gear holds it (with the
//! gear's axes at rest the push fell 3,4 m to one side and the takeoff drifted 11,6 m from the original's reference run).
#[test]
fn the_lambda_front_wheel_pushes_from_the_turned_gear() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Corellia", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let (x, z) = (1389.45, 1839.61);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 1.0;
    let k = s.spawn_vehicle("lambda", [x, y, z], [0.0; 3]).unwrap();
    for _ in 0..30 {
        s.step();
    }
    let v = &s.vehicles[k];
    let r = &v.craft.body.rows;
    let mut checked = 0;
    for c in v.contacts.iter().filter(|c| c.spring.as_ref().is_some_and(|sp| !sp.chain.is_empty())) {
        // the push's point (relative to the root, in the world's axes) against the contactor's place now
        let p = c.pos;
        let want = [r[0][0] * p[0] + r[1][0] * p[1] + r[2][0] * p[2], r[0][1] * p[0] + r[1][1] * p[1] + r[2][1] * p[2], r[0][2] * p[0] + r[1][2] * p[1] + r[2][2] * p[2]];
        let got = c.spring.as_ref().unwrap().last_push.0;
        let d = (0..3).map(|i| (got[i] - want[i]).powi(2)).sum::<f32>().sqrt();
        assert!(d < 1e-3, "{}: push at {got:?}, placed at {want:?} ({d} m)", c.name);
        checked += 1;
    }
    assert!(checked >= 2, "the two RollWheels under the bay gears");
}
