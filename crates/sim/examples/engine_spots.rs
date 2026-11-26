//! Where each vehicle's engines sit in its body's frame (x right, y up, z forward) and its contacts' extent along z:
//!   engine_spots <game> <mod> <level>...
//! One line per template: `class template engines=[type@x,y,z axis=..] z=<min>..<max> radius=<r>`.
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("engine_spots <game> <mod> <level>...");
        std::process::exit(2);
    }
    for level in &args[3..] {
        let w = bytefield_sim::World::load(std::path::Path::new(&args[1]), &args[2], level, "conquest").expect("world");
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for v in s.vehicles.iter().filter(|v| v.in_world() && !v.has_soldier_contact() && !v.entry.seats.is_empty()) {
            use bytefield_sim::craft::Kind;
            let f = v.craft.frames_in_root();
            let engines: Vec<String> = v.craft.parts.iter().zip(&f).filter_map(|(p, f)| if let Kind::Engine(e) = &p.kind { Some(format!("{}@{:.1},{:.1},{:.1} axis={:.1},{:.1},{:.1}", e.engine_type, f.pos[0], f.pos[1], f.pos[2], f.rows[2][0], f.rows[2][1], f.rows[2][2])) } else { None }).collect();
            let zs: Vec<f32> = v.contacts.iter().map(|c| c.pos[2]).collect();
            let (lo, hi) = zs.iter().fold((f32::MAX, f32::MIN), |a, z| (a.0.min(*z), a.1.max(*z)));
            let line = format!("{:?} {} engines={:?} z={lo:.1}..{hi:.1} radius={:.1}", bytefield_sim::bot_api::VehicleClass::of(v), v.template, engines, v.radius);
            seen.entry(v.template.clone()).or_insert(line);
        }
        for l in seen.values() {
            println!("{level}: {l}");
        }
    }
}
