//! Every seat with two weapons or more on a level's vehicles: whether its first weapon in the template's tree is the fire key's
//! and the first on another key the alt-fire's (`same`, the client's two panels as before), or not (`DIFF`, with the declared
//! primary icon). The client's two-panel rule (crates/client/src/gauges.rs) was checked with it over vanilla, XPack1/2, both DCs
//! and GC.
//!     cargo run --release -p bytefield-sim --example panel_order -- data/game GCMOD GC_Hoth
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let mut seen = std::collections::HashSet::new();
    let (mut same, mut diff) = (0, 0);
    for t in w.db.objects_of_kind("PlayerControlObject") {
        if !seen.insert(t.name.to_lowercase()) {
            continue;
        }
        let Some(k) = s.spawn_vehicle(&t.name, [500.0, 300.0, 500.0], [0.0; 3]) else { continue };
        let v = &s.vehicles[k];
        for (seat, st) in v.entry.seats.iter().enumerate() {
            let Some(pt) = w.db.object_template(&st.pco) else { continue };
            let mine: Vec<_> = v.weapons.iter().filter(|(p, _)| v.craft.parts[*p].seat == seat).map(|(_, w)| (w.template.name.clone(), w.template.input_fire)).collect();
            let Some(first) = mine.first().filter(|_| mine.len() >= 2) else { continue };
            let fire = mine.iter().find(|w| w.1 != 23).map(|w| &w.0);
            let alt_by_key = mine.iter().find(|w| w.1 == 23).map(|w| &w.0);
            let alt_by_tree = mine.iter().find(|w| w.1 != first.1).map(|w| &w.0);
            if fire == Some(&first.0) && alt_by_key == alt_by_tree {
                same += 1;
            } else {
                diff += 1;
                let icon = pt.get("setprimaryammoicon").and_then(|a| a.first()).cloned().unwrap_or_default();
                println!("DIFF {} seat {seat} primary icon {icon} weapons {:?}", t.name, mine);
            }
        }
    }
    println!("{same} same, {diff} differ");
}
