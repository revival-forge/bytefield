//! A rigid projectile (`setHasPointPhysics 0`: bombs, grenades, most rockets and missiles) that runs into a vehicle gives it
//! the hit's damage, and its explosion leaves that vehicle out, as a point projectile's. The contact pass did not
//! say which vehicle it was, so there was no hit and the explosion took it whole: a Stuka's bomb on a Tiger, 55 instead of
//! 150; the Katyusha on a T72 from the front, 46 instead of 3,9. The numbers are the original game's:
//! the target held 30 m over the ground at (1000, 1000), the shot from 15 m in front or 11 m above.
//! Skipped without the real data.
use crate::support::rigid_hit_damage::*;

#[test]
fn a_bomb_on_a_tank_hits_it() {
    if !crate::support::has_level("bf1942", "Kursk") {
        eprintln!("no Kursk, skipped");
        return;
    }
    let w = bytefield_sim::World::load(&crate::support::game(), "bf1942", "Kursk", "conquest").unwrap();
    check("Tiger", &shot(&w, "Tiger", 0.0, "StukaBombRack", true), &[("Hit", 150.0)]);
    check("Sherman", &shot(&w, "Sherman", 0.0, "StukaBombRack", true), &[("Hit", 200.0)]);
}
