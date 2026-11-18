//! A client's copy of a round has the server's supply depots, the level's and its vehicles'
//! (`copy_round` loads the level, `spawn_vehicle` adds a vehicle's), so the HUD's supply icons (`Sim::supply_icons`) are there
//! on a server too; a TODO in `gauges.rs` said the replica did not carry them.

use bytefield_sim::World;

fn world(level: &str) -> Option<&'static World> {
    bytefield_testdata::base().map(|game| &*Box::leak(Box::new(World::load(&game, "bf1942", level, "conquest").expect("level"))))
}

#[test]
fn a_copy_has_the_servers_depots() {
    for level in ["El_Alamein", "Berlin", "Midway"] {
        let Some(w) = world(level) else { return };
        let mut server = bytefield_server::round(w, 8, false);
        for _ in 0..300 {
            server.step();
        }
        // (the copy steps once more: a moving boat's depots 1 m further)
        let mut copy = bytefield_server::copy_round(w);
        bytefield_sim::replica::Snapshot::of(&server).write(&mut copy);
        copy.step();
        let key = |s: &bytefield_sim::Sim| {
            let mut v: Vec<(String, Option<usize>, f32, f32)> = s.depots.iter().map(|d| (d.template.name.clone(), d.on.map(|o| o.0), d.pos[0], d.pos[2])).collect();
            v.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)).then(a.2.total_cmp(&b.2)).then(a.3.total_cmp(&b.3)));
            v
        };
        let (a, b) = (key(&server), key(&copy));
        eprintln!("{level}: server {} depots, copy {}", a.len(), b.len());
        assert!(!a.is_empty(), "{level}: no depots to compare");
        assert_eq!(a.len(), b.len(), "{level}");
        for (x, y) in a.iter().zip(&b) {
            assert!(x.0 == y.0 && x.1 == y.1 && (x.2 - y.2).hypot(x.3 - y.3) < 3.0, "{level}: {x:?} {y:?}");
        }
    }
}
