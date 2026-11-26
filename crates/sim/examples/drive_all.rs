//! Every vehicle a level's spawners make, driven: for each template (its first one), a fresh
//! round, a soldier in the driver's seat with the throttle down for `seconds` (8), and what came
//! of it: how far it went, its top speed, the health it lost, whether it ended upside down or
//! destroyed. A vehicle a person cannot drive (it does not move, flips, breaks) shows up here
//! (a detector for DC and GC, as `bots` with `BOTS_CHECK` is for the bots).
//! `cargo run --release -p bytefield-sim --example drive_all -- data/game DC_Final DC_Basrahs_Edge [seconds] [--mode conquest]`
//! `--trace <template>`: only that one, its place, how upright, speed and health every half second.
//! `--throttle x` (0..1, default 1) and `--cap v`: the throttle let go above v m/s, as the bots' driver; `--yaw y` turns too (346: the AT-ST tumbles at full throttle, also in the original).
//! One line per template: `drive <template> cat <0 land|1 sea|2 air> moved <m> top <m/s> lost <hp> [upside] [destroyed] [no seat]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seconds: f64 = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(8.0);
    let mode = a.iter().position(|x| x == "--mode").and_then(|i| a.get(i + 1)).map_or("conquest", |m| m.as_str());
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], mode).expect("world");
    let soldier_template = ["USSoldier", "GermanSoldier"].into_iter().find(|t| w.vehicle(t).is_some()).expect("a soldier");
    let first = {
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let mut seen = std::collections::BTreeMap::new();
        for (i, v) in s.vehicles.iter().enumerate() {
            if v.soldier().is_none() && !v.fixed && !v.removed {
                seen.entry(v.template.clone()).or_insert(i);
            }
        }
        seen
    };
    let throttle: f32 = a.iter().position(|x| x == "--throttle").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let cap: Option<f32> = a.iter().position(|x| x == "--cap").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok());
    let yaw: f32 = a.iter().position(|x| x == "--yaw").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let trace = a.iter().position(|x| x == "--trace").and_then(|i| a.get(i + 1)).map(|t| t.to_ascii_lowercase());
    for (template, &k) in &first {
        if trace.as_ref().is_some_and(|t| *t != template.to_ascii_lowercase()) {
            continue;
        }
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let cat = w.vehicle_category(template);
        if s.vehicles[k].entry.seats.is_empty() {
            println!("drive {template} cat {cat} no seat");
            continue;
        }
        let at = s.vehicles[k].craft.body.pos;
        let soldier = s.spawn_vehicle(soldier_template, [at[0], at[1] + 3.0, at[2]], [0.0; 3]).unwrap();
        s.vehicles[soldier].team = 1;
        s.vehicles[k].team = 0;
        s.enter(soldier, k, 0);
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::THROTTLE, throttle), (bytefield_sim::soldier::axis::YAW, yaw)]);
        let hp0 = s.vehicles[k].armor.as_ref().map_or(0.0, |a| a.hp);
        let mut top = 0.0f32;
        let mut tilted = 0usize;
        for f in 1..=(seconds * 30.0) as usize {
            if let Some(c) = cap {
                let u = s.vehicles[k].craft.body.vel;
                let t = if (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt() > c { 0.0 } else { throttle };
                s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::THROTTLE, t), (bytefield_sim::soldier::axis::YAW, yaw)]);
            }
            s.step();
            let u = s.vehicles[k].craft.body.vel;
            top = top.max((u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt());
            tilted += (s.vehicles[k].craft.body.rows[1][1] < 0.7) as usize;
            if trace.is_some() && f % 15 == 0 {
                let b = &s.vehicles[k].craft.body;
                let hp = s.vehicles[k].armor.as_ref().map_or(0.0, |a| a.hp);
                println!("{:6.2} s at {:?} up {:.2} fwd {:?} speed {:.1} hp {hp:.1}", s.clock.time, b.pos.map(|x| (x * 10.0).round() / 10.0), b.rows[1][1], b.rows[2].map(|x| (x * 100.0).round() / 100.0), (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt());
            }
        }
        let v = &s.vehicles[k];
        let q = v.craft.body.pos;
        let moved = ((q[0] - at[0]).powi(2) + (q[2] - at[2]).powi(2)).sqrt();
        let lost = v.armor.as_ref().map_or(0.0, |a| hp0 - a.hp);
        let upside = if v.craft.body.rows[1][1] < 0.0 { " upside" } else { "" };
        let destroyed = if v.armor.as_ref().is_some_and(|a| a.destroyed) || v.removed { " destroyed" } else { "" };
        // (the seconds it spent leaning over 45°, only with --throttle or --cap: the census reads the line as it was)
        let tilt = if cap.is_some() || throttle != 1.0 || yaw != 0.0 { format!(" tilted {:.1}", tilted as f64 / 30.0) } else { String::new() };
        println!("drive {template} cat {cat} moved {moved:.1} top {top:.1} lost {lost:.1}{upside}{destroyed}{tilt}");
    }
}
