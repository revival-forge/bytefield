//! What class the player API gives each vehicle template of a level, and the physical features the class could read:
//!   vehicle_classes <game> <mod> <level>...
//! One line per template: `class template engines=<types> bundles=<n> contacts=<n> sprung=<n> anchored=<n> lifts=<torque up> pushes=<torque along> seats=<n> floats=<bool> lift=<throttle that lifts it: -1, 0, 1>`.
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("vehicle_classes <game> <mod> <level>...");
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
            let engines: Vec<u32> = v.craft.parts.iter().filter_map(|p| if let Kind::Engine(e) = &p.kind { Some(e.engine_type) } else { None }).collect();
            let bundles = v.craft.parts.iter().filter(|p| p.bundle.is_some()).count();
            let sprung = v.contacts.iter().filter(|c| !c.is_root && c.spring.is_some()).count();
            let anchored = v.contacts.iter().filter(|c| !c.is_root && c.anchor.is_some()).count();
            let (lifts, pushes) = bytefield_sim::bot_api::VehicleClass::thrust(v);
            // (the lift of its wings that lie flat, and its mass: what holds a plane up once it goes along)
            let wings: f32 = v.craft.parts.iter().zip(&v.craft.frames_in_root()).filter_map(|(p, f)| if let Kind::Wing(w) = &p.kind { (f.rows[1][1].abs() > 0.7).then_some(w.wing_lift) } else { None }).sum();
            let line = format!("{:?} {} wings={wings:.1} mass={:.0} engines={:?} bundles={} contacts={} sprung={} anchored={} lifts={lifts:.0} pushes={pushes:.0} seats={} floats={} lift={}", bytefield_sim::bot_api::VehicleClass::of(v), v.template, v.craft.body.mass, engines, bundles, v.contacts.len(), sprung, anchored, v.entry.seats.len(), bytefield_sim::bot_api::VehicleClass::floats(v), bytefield_sim::bot_api::VehicleClass::lift_throttle(v));
            seen.entry(v.template.clone()).or_insert(line);
        }
        for l in seen.values() {
            println!("{level}: {l}");
        }
    }
}
