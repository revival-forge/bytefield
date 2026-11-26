//! Each seat's entry points of some vehicle templates, in the vehicle's frame (x right, y up, z ahead), to place a soldier
//! within reach of one when boarding by walking:
//!   seat_entries <game> <mod> <level> [--yaw deg] <template>...
//! One line per seat: `template seat <n>: entry (x, y, z) r <radius> ...  ride (x, y, z)`; with `--yaw`, the entry points turned as
//! a vehicle spawned with that yaw (`Object.rotation <yaw>/0/0`): offsets from its origin along the world's axes.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("seat_entries <game> <mod> <level> <template>...");
        std::process::exit(2);
    }
    let w = bytefield_sim::World::load(std::path::Path::new(&args[1]), &args[2], &args[3], "conquest").expect("world");
    let yaw = args.iter().position(|a| a == "--yaw").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<f32>().ok());
    let rows = bytefield_sim::craft::ypr([yaw.unwrap_or(0.0), 0.0, 0.0]);
    let skip = yaw.map_or(4, |_| 6);
    for t in &args[skip..] {
        let Some(m) = bytefield_sim::vehicle::model(&w.db, t) else {
            println!("{t}: no such template");
            continue;
        };
        let seats = bytefield_sim::entry::Seat::all_from_model(&m);
        // (the outside views' reach: chase 1.2 ×, fly-by 3.2 × that, client play/seats.rs)
        println!("{t}: bounding radius {:.2}", w.bounding_radius(t));
        if seats.is_empty() {
            println!("{t}: no seats ({})", m.kind);
        }
        for (i, s) in seats.iter().enumerate() {
            let e: Vec<String> = s
                .entries
                .iter()
                .map(|e| {
                    let p = bytefield_sim::craft::to_parent(e.pos, &rows);
                    format!("entry ({:.2}, {:.2}, {:.2}) r {:.1}", p[0], p[1], p[2], e.radius)
                })
                .collect();
            println!("{t} seat {}: {}  ride ({:.2}, {:.2}, {:.2})", i + 1, e.join("  "), s.ride[0], s.ride[1], s.ride[2]);
        }
    }
}
