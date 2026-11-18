//! A rigid projectile (`setHasPointPhysics 0`: bombs, grenades, most rockets and missiles) that runs into a vehicle gives it
//! the hit's damage, and its explosion leaves that vehicle out, as a point projectile's. The contact pass did not
//! say which vehicle it was, so there was no hit and the explosion took it whole: a Stuka's bomb on a Tiger, 55 instead of
//! 150; the Katyusha on a T72 from the front, 46 instead of 3,9. The numbers are the original game's:
//! the target held 30 m over the ground at (1000, 1000), the shot from 15 m in front or 11 m above.
//! Skipped without the real data.
use crate::support::rigid_hit_damage::*;

#[test]
fn the_katyusha_hits_what_it_runs_into() {
    for (m, w) in crate::support::dc_worlds("DC_Al_Nas", "conquest") {
        check(&format!("{m} T72 front"), &shot(&w, "T72", 180.0, "KatyushaFireArmsBundle", false), &[("Hit", 3.909)]);
        check(&format!("{m} T72 top"), &shot(&w, "T72", 0.0, "KatyushaFireArmsBundle", true), &[("Hit", 21.0)]);
        // the +2 is its own critical damage burning, later
        check(&format!("{m} Humvee front"), &shot(&w, "Humvee", 180.0, "KatyushaFireArmsBundle", false), &[("Hit", 70.0), ("Burning", 2.0)]);
    }
}

/// Not rigid, but the same bench: the mortar (`setMinDev 1`) leaves with the deviation of input index 0, as the
/// the reference harness fires it; without it the shot flew 15 cm high over the Humvee's hood and gave nothing.
#[test]
fn the_mortar_grazes_the_humvee_as_the_original() {
    for (m, w) in crate::support::dc_worlds("DC_Al_Nas", "conquest") {
        check(&format!("{m} Humvee front"), &shot(&w, "Humvee", 180.0, "Mortar_fire", false), &[("Hit", 33.484)]);
        check(&format!("{m} BMP2 front"), &shot(&w, "BMP2", 180.0, "Mortar_fire", false), &[("Hit", 39.687)]);
    }
}
