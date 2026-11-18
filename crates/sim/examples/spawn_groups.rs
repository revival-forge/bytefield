//! The spawn groups of a level's conquest round after `steps` steps: team, points, how many are
//! active for a person and for a bot of each team, and where.
//! `cargo run --release -p bytefield-sim --example spawn_groups -- data/game bf1942 Coral_sea [steps] [--mode objectivemode]`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mode = a.iter().position(|x| x == "--mode").and_then(|i| a.get(i + 1)).map_or("conquest", |m| m.as_str());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], mode).expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    let steps: u64 = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(30);
    for _ in 0..steps {
        s.step();
    }
    let c = s.modes.conquest.as_ref().unwrap();
    let kind = |k: &str| w.db.instances.iter().filter(|i| w.db.object_template(&i.template).is_some_and(|t| t.kind.eq_ignore_ascii_case(k))).count();
    println!("instances: {} spawn points, {} control points; {} groups, {} flags, tickets {:?}", kind("spawnpoint"), kind("controlpoint"), c.groups.len(), c.points.len(), c.tickets);
    for (id, g) in &c.groups {
        if g.points == 0 && g.spawns.is_empty() {
            continue;
        }
        let active = |ai: bool| g.spawns.iter().filter(|p| s.spawn_point_active_pub(p, g.team.max(0) as u32, ai)).count();
        let carried: Vec<String> = g.spawns.iter().filter_map(|p| p.on).map(|(k, _)| s.vehicles[k].template.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        println!("group {id:3} team {:2} points {:2} spawns {:2} active person {:2} bot {:2} {}", g.team, g.points, g.spawns.len(), active(false), active(true), carried.join(" "));
    }
}
