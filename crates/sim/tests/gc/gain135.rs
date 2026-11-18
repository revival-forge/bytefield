//! GC's X-34 landspeeder: its template says `GravityModifier 50`, but
//! object templates have no such property (only particles and projectiles do), so the server keeps
//! gravity 1 and it settles instead of slamming into the ground on its second update. Skipped
//! without the real data (`data/game`).
#[test]
fn the_x34_settles_like_the_original() {
    let Some(game) = bytefield_testdata::with_mod("GCMOD") else { return; };
    let w = bytefield_sim::World::load(&game, "GCMOD", "GC_Corellia", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    for _ in 0..120 {
        s.step();
    }
    let x34: Vec<_> = s.vehicles.iter().filter(|v| v.template.eq_ignore_ascii_case("gain-135")).collect();
    assert!(!x34.is_empty());
    for v in x34 {
        let b = &v.craft.body;
        assert_eq!(b.gravity_modifier, 1.0);
        assert_eq!(v.armor.as_ref().map(|a| a.hp), Some(80.0), "{:?}", b.pos);
        // the one at (1485.07, 940.29), placed at 76.17, rests at 76.791 in the original from
        // its 30th update on
        if (b.pos[0] - 1485.07).abs() < 1.0 {
            assert!((b.pos[1] - 76.791).abs() < 0.01, "{:?}", b.pos);
        }
    }
}
