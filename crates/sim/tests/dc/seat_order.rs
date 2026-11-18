//! Which `SeatObject` a `PlayerControlObject` seats its player in when it has two: the original's
//! `internalFindFirstChildOfCID` looks at a node and its later siblings before any of their children, so
//! the SA-9's launcher gunner sits in the seat of `BRDM2_Gaskin_Gun_Holder` (the bundle that yaws), not in the one of
//! `BRDM2_Gaskin_Turret` under it, which comes first depth first and pitches. Skipped without the real data.
#[test]
fn the_sa9_gunner_sits_in_the_holders_seat() {
    for (m, w) in crate::support::dc_worlds("DC_DustBowl", "conquest") {
        let pco = bytefield_sim::vehicle::model(&w.db, "BRDM2_Gaskin_Gun_position").unwrap_or_else(|| panic!("{m}: no SA-9 gun position"));
        // depth first: the turret's seat (0), then the holder's (1)
        assert_eq!(pco.seat_rank(), Some(1), "{m}");
        let seat = pco.first_child_of_kind("SeatObject", "PlayerControlObject").unwrap();
        assert!((seat.position[1] + 0.42).abs() < 1e-6 && (seat.position[2] - 0.10).abs() < 1e-6, "{m}: {:?}", seat.position);
        // the vehicle's seat 1 rides there: the gun position at 0.005/1.58/0.5 in the complex, turned 179.99/1.7/0, the
        // holder at 0/0/.4 in it, turned 179.9, and its seat 0/-0.42/0.10 in that: ~0.005/1.16/0.20 (the turret's, 0.04)
        let v = w.vehicle("SA-9_Gaskin").unwrap();
        let s = &v.entry.seats[1];
        assert!(s.pco.eq_ignore_ascii_case("BRDM2_Gaskin_Gun_position"), "{m}: {}", s.pco);
        let want = [0.005, 1.16, 0.20];
        assert!((0..3).all(|i| (s.ride[i] - want[i]).abs() < 0.03), "{m}: rides at {:?}", s.ride);
    }
}
