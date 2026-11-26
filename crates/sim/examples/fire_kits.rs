//! Every hand weapon of every kit a level's teams may choose, fired: for each team, kit and
//! weapon of the kit (`game.setKit`, the team's `setTeamSkin` soldier), a fresh round, a soldier
//! at one of the team's spawn points with that weapon in his hand and the fire tapped (two frames
//! down, ten up) for `seconds` (5), and what came of it: the shots, those whose projectile did
//! not come out (a template nothing makes), the projectiles' names, the explosions, the hits and what the projectiles deployed (their
//! own `ObjectSpawner`s).
//! The kits' side of `fire_all`.
//! `cargo run --release -p bytefield-sim --example fire_kits -- data/game DC_Final DC_Basrahs_Edge [seconds] [--mode conquest]`
//! One line per weapon: `kit <team> <kit> <weapon> shots <n> lost <n> boom <n> hits <n> [<projectile>…] [silent] [made <object>…] [depot <depot>… gave <n>] [held <other>] [no projectile | wants <projectile> when some were lost]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seconds: f64 = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let mode = a.iter().position(|x| x == "--mode").and_then(|i| a.get(i + 1)).map_or("conquest", |m| m.as_str());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], mode).expect("world");
    let fresh = || {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s
    };
    let s0 = fresh();
    let Some(c) = s0.modes.conquest.as_ref() else {
        println!("kit no conquest");
        return;
    };
    for team in 1..=2u8 {
        let Some(skin) = c.teams.skins[team as usize - 1].clone() else {
            println!("kit {team} no skin");
            continue;
        };
        let Some(sp) = c.groups.values().filter(|g| g.team == team as i64).flat_map(|g| g.spawns.iter()).next().copied() else {
            println!("kit {team} no spawn point");
            continue;
        };
        for kit in c.teams.kits[team as usize - 1].iter().flatten() {
            let weapons = w.kit_weapons(kit);
            if weapons.is_empty() {
                println!("kit {team} {kit} no weapons");
                continue;
            }
            for weapon in weapons.iter().map(|x| x.name.clone()) {
                let mut s = fresh();
                let Some(sol) = s.spawn_vehicle(&skin, [sp.pos[0], sp.pos[1] + 1.0, sp.pos[2]], sp.rot) else {
                    println!("kit {team} {kit} {weapon} no soldier {skin}");
                    continue;
                };
                s.vehicles[sol].team = team as u32;
                s.vehicles[sol].give_kit(w.kit_weapons(kit), Some(&weapon));
                s.vehicles[sol].soldier_mut().unwrap().has_player = true;
                let first_held = s.vehicles[sol].soldier().and_then(|m| m.weapon.as_ref()).map(|x| x.template.name.clone());
                let projectile = s.vehicles[sol].soldier().and_then(|m| m.weapon.as_ref()).map_or(String::new(), |x| x.template.projectile.clone());
                let (f0, e0, h0) = (s.journal.fired.len(), s.journal.explosions.len(), s.journal.hit_log.len());
                for f in 0..(seconds * 30.0) as usize {
                    let v = if f % 12 < 2 { 1.0 } else { 0.0 };
                    s.vehicles[sol].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, v)]);
                    s.step();
                }
                let fired: Vec<_> = s.journal.fired[f0..].iter().filter(|f| f.1 == sol).collect();
                let lost = fired.iter().filter(|f| f.3.is_none()).count();
                let names: std::collections::BTreeSet<&str> = fired.iter().filter_map(|f| f.3).map(|p| s.projectiles[p].template.name.as_str()).collect();
                let boom = s.journal.explosions[e0..].iter().filter(|(_, e)| e.by == Some(sol)).count();
                let hits = s.journal.hit_log[h0..].iter().filter(|h| h.1 == Some(sol)).count();
                // what its projectiles deployed (their own spawners: DC's mortar, GC's E-Web)
                let mut made: std::collections::BTreeSet<&str> = s.journal.spawn_log.iter().filter(|e| s.spawners[e.1].on_projectile()).map(|e| s.vehicles[e.2].template.as_str()).collect();
                // (and the objects that only lie there: a kit)
                made.extend(s.spawners.iter().filter(|p| p.on_projectile()).filter_map(|p| match p.last {
                    Some(bytefield_sim::create::Made::Item(i)) => Some(s.items[i].template.as_str()),
                    _ => None,
                }));
                let mut tail = String::new();
                if !made.is_empty() {
                    tail += &format!(" made {}", made.into_iter().collect::<Vec<_>>().join(" "));
                }
                // and the depots they carried (GC's repair droids), with what those gave
                let depots: std::collections::BTreeSet<&str> = s.depots.iter().filter(|d| d.on_projectile).map(|d| d.template.name.as_str()).collect();
                if !depots.is_empty() {
                    let given = s.journal.supply_log.iter().filter(|e| s.depots[e.depot].on_projectile).count();
                    tail += &format!(" depot {} gave {given}", depots.into_iter().collect::<Vec<_>>().join(" "));
                }
                if fired.is_empty() {
                    tail += " silent";
                }
                if !first_held.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(&weapon)) {
                    tail += &format!(" held {}", first_held.as_deref().unwrap_or("-"));
                }
                if projectile.is_empty() {
                    tail += " no projectile";
                } else if lost > 0 {
                    let made = w.projectile_template(&projectile).is_some();
                    tail += &format!(" wants {projectile}{}", if made { "" } else { " (no such projectile)" });
                }
                // (what `Sim::instantiate` does not make)
                for (k, n) in &s.journal.unhandled {
                    tail += &format!(" unhandled {k} ×{n}");
                }
                let names: Vec<&str> = names.into_iter().collect();
                println!("kit {team} {kit} {weapon} shots {} lost {lost} boom {boom} hits {hits} [{}]{tail}", fired.len(), names.join(" "));
            }
        }
    }
}
