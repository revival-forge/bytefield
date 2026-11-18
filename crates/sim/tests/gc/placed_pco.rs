//! The vehicles placed in a level's scripts, not by a spawner:
//! DC_First_Light's mortar, GC_BCanyon's medical bed, GC's E-Webs. The server makes them as any other
//! `PlayerControlObject` (a reference run of the original: `createObject` and the `Pco`
//! list); they used to be obstacles only. Skipped without the real data.
use crate::support::placed_pco::*;

#[test]
fn gc_bcanyon_medical_bed_is_a_vehicle() {
    let Some(g) = bytefield_testdata::with_mod("GCMOD") else { return };
    let w = bytefield_sim::World::load(&g, "GCMOD", "GC_BCanyon", "conquest").unwrap();
    let (_, pos, _) = w.placed_vehicles().into_iter().find(|(t, _, _)| t.eq_ignore_ascii_case("medbed1")).expect("placed");
    placed("GCMOD", "GC_BCanyon", "medbed1", pos.map(|c| c as f32), None);
}

/// GC's E-Webs placed in conquest's `ObjectSpawns.con` (not a spawner's): the original has five in its
/// `Pco` list on GC_Anchorhead, and one `Miau` on GC_Ryloth.
#[test]
fn gc_placed_ewebs_as_the_original() {
    let Some(g) = bytefield_testdata::with_mod("GCMOD") else { return };
    for (level, template, n) in [("GC_Anchorhead", "Stationary_Eweb", 5), ("GC_Ryloth", "Miau", 1)] {
        let w = bytefield_sim::World::load(&g, "GCMOD", level, "conquest").unwrap();
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.start_conquest();
        assert_eq!(s.vehicles.iter().filter(|v| v.template.eq_ignore_ascii_case(template)).count(), n, "{level}");
    }
}
