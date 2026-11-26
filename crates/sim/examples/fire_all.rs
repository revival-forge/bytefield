//! Every weapon of every vehicle a level's spawners make, fired: for each template (its first
//! one) and each seat with a weapon, a fresh round, a soldier in that seat with the fire and the
//! alt fire down for `seconds` (5), and what came of it: the shots, those whose projectile did not
//! come out (a template nothing makes), the projectiles' names, the explosions they set off and
//! the hits. A gun a person fires and nothing happens shows up here (a detector for DC
//! and GC, as `drive_all` is for driving).
//! `cargo run --release -p bytefield-sim --example fire_all -- data/game DC_Final DC_Basrahs_Edge [seconds] [--mode conquest]`
//! One line per seat: `fire <template> seat <n> shots <n> lost <n> boom <n> hits <n> [<projectile>…] [silent] [weapons <weapon>-><projectile>[(none)]… when some were lost; `(none)`: no template defines it, the data's] [made <object>…] [depot <depot>… gave <n>]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seconds: f64 = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let mode = a.iter().position(|x| x == "--mode").and_then(|i| a.get(i + 1)).map_or("conquest", |m| m.as_str());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], mode).expect("world");
    let soldier_template = ["USSoldier", "GermanSoldier"].into_iter().find(|t| w.vehicle(t).is_some()).expect("a soldier");
    let first = {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let mut seen = std::collections::BTreeMap::new();
        for (i, v) in s.vehicles.iter().enumerate() {
            if v.soldier().is_none() && !v.removed && !v.weapons.is_empty() {
                seen.entry(v.template.clone()).or_insert(i);
            }
        }
        seen
    };
    for (template, &k) in &first {
        let seats: std::collections::BTreeSet<usize> = {
            let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            let v = &s.vehicles[k];
            v.weapons.iter().map(|(p, _)| v.craft.parts[*p].seat).collect()
        };
        for seat in seats {
            let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            if seat >= s.vehicles[k].entry.seats.len() {
                println!("fire {template} seat {seat} no seat");
                continue;
            }
            let at = s.vehicles[k].craft.body.pos;
            let soldier = s.spawn_vehicle(soldier_template, [at[0], at[1] + 3.0, at[2]], [0.0; 3]).unwrap();
            s.vehicles[soldier].team = 1;
            s.vehicles[k].team = 0;
            s.enter(soldier, k, seat);
            if s.vehicles[soldier].entry.inside != Some(k) {
                println!("fire {template} seat {seat} not in");
                continue;
            }
            let (f0, e0, h0) = (s.journal.fired.len(), s.journal.explosions.len(), s.journal.hit_log.len());
            let n0 = s.vehicles.len();
            s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, 1.0), (bytefield_sim::soldier::axis::ALT_FIRE, 1.0)]);
            // BYTEFIELD_FIRE_TRACK=<template>: where that projectile's first shot goes, every half second
            let track = std::env::var("BYTEFIELD_FIRE_TRACK").ok();
            let p0 = s.projectiles.len();
            let mut track_done = false;
            for i in 0..(seconds * 30.0) as usize {
                s.step();
                if let Some(t) = &track
                    && !track_done
                    && let Some(p) = s.projectiles[p0..].iter().find(|p| p.template.name.eq_ignore_ascii_case(t))
                    && (i % 15 == 14 || p.end.is_some())
                {
                    track_done = p.end.is_some(); // the ended shot stays in the list: print its end once
                    let (x, v) = (p.pos, p.vel);
                    let ground = w.height_at(x[0] as f64, x[2] as f64).unwrap_or(f64::NAN);
                    println!("track {t} t {:.1} pos {:.1}/{:.1}/{:.1} ground {ground:.1} vel {:.1}/{:.1}/{:.1} end {:?} contacts {:?}", p.age, x[0], x[1], x[2], v[0], v[1], v[2], p.end, p.rigid.as_ref().map(|r| r.contacts.len()));
                }
                // flying a guided missile: back out of it, or its warhead goes off at once
                if s.vehicles[soldier].entry.inside.is_some_and(|r| r >= n0) {
                    s.vehicles[soldier].soldier_mut().unwrap().set_input(&[]);
                }
            }
            let fired = &s.journal.fired[f0..];
            let launched: Vec<&str> = s.vehicles[n0..].iter().map(|v| v.template.as_str()).filter(|t| w.is_player_control_object(t)).collect();
            let lost = fired.iter().filter(|f| f.3.is_none()).count().saturating_sub(launched.len());
            let mut names: std::collections::BTreeSet<&str> = fired.iter().filter_map(|f| f.3).map(|p| s.projectiles[p].template.name.as_str()).collect();
            names.extend(launched);
            let boom = s.journal.explosions[e0..].iter().filter(|(_, e)| e.by.is_some_and(|b| b == k || b == soldier || b >= n0)).count();
            let hits = s.journal.hit_log[h0..].iter().filter(|h| h.1.is_some_and(|b| b == k || b == soldier || b >= n0)).count();
            let silent = if fired.is_empty() { " silent" } else { "" };
            // which of the seat's weapons may be the lost ones (a horn has no projectile: not one)
            let missing: Vec<String> = if lost == 0 {
                Vec::new()
            } else {
                let v = &s.vehicles[k];
                let all: Vec<_> = v.weapons.iter().filter(|(p, f)| v.craft.parts[*p].seat == seat && !f.template.projectile.is_empty()).map(|(_, f)| f).collect();
                // a projectile no template defines is the data's (the Nimitz's `RocketBlast->Blank_Projectile` in DC_Final,
                // whose only definition is 0.7's MLRS): marked `(none)`, and the others are left out
                let none: Vec<String> = all.iter().filter(|f| w.db.object_template(&f.template.projectile).is_none()).map(|f| format!("{}->{}(none)", f.template.name, f.template.projectile)).collect();
                if none.is_empty() { all.iter().map(|f| format!("{}->{}", f.template.name, f.template.projectile)).collect() } else { none }
            };
            let lost = if missing.is_empty() { 0 } else { lost };
            let silent = if missing.is_empty() { silent.to_string() } else { format!("{silent} weapons {}", missing.join(" ")) };
            // what its projectiles deployed and the depots they carried (their own spawners and
            // depots: DC's paradropped ammo crate, GC's repair droids), with what those gave
            let mut made: std::collections::BTreeSet<&str> = s.journal.spawn_log.iter().filter(|e| s.spawners[e.1].on_projectile()).map(|e| s.vehicles[e.2].template.as_str()).collect();
            // (and the objects that only lie there: a kit)
            made.extend(s.spawners.iter().filter(|p| p.on_projectile()).filter_map(|p| match p.last {
                Some(bytefield_sim::create::Made::Item(i)) => Some(s.items[i].template.as_str()),
                _ => None,
            }));
            let mut silent = silent;
            if !made.is_empty() {
                silent += &format!(" made {}", made.into_iter().collect::<Vec<_>>().join(" "));
            }
            let depots: std::collections::BTreeSet<&str> = s.depots.iter().filter(|d| d.on_projectile).map(|d| d.template.name.as_str()).collect();
            if !depots.is_empty() {
                let given = s.journal.supply_log.iter().filter(|e| s.depots[e.depot].on_projectile).count();
                silent += &format!(" depot {} gave {given}", depots.into_iter().collect::<Vec<_>>().join(" "));
            }
            // (what `Sim::instantiate` does not make)
            for (k, n) in &s.journal.unhandled {
                silent += &format!(" unhandled {k} ×{n}");
            }
            let names: Vec<&str> = names.into_iter().collect();
            println!("fire {template} seat {seat} shots {} lost {lost} boom {boom} hits {hits} [{}]{silent}", fired.len(), names.join(" "));
        }
    }
}
