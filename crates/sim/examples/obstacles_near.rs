//! What the level has around a place, for telling a missing obstacle from a missed one:
//!     obstacles_near <game> <mod> <level> <x> <z> [radius]    # the sim's obstacles (name, place, radius, its LODs) and what a round spawns there
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").unwrap();
    let (x, z): (f32, f32) = (a[4].parse().unwrap(), a[5].parse().unwrap());
    let r: f32 = a.get(6).map_or(20.0, |v| v.parse().unwrap());
    for o in w.obstacles() {
        let d = ((o.center[0] - x).powi(2) + (o.center[2] - z).powi(2)).sqrt();
        if d < r + o.radius {
            println!("{d:.1} {} at {:?} center {:?} radius {:.1} scale {:?} lods {:?}", o.name, o.pos, o.center, o.radius, o.scale, o.lods.len());
        }
    }
    let mut sim = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    sim.round.playing = true;
    sim.start_conquest();
    for v in sim.vehicles.iter().filter(|v| v.in_world()) {
        let q = v.craft.body.pos;
        let d = ((q[0] - x).powi(2) + (q[2] - z).powi(2)).sqrt();
        if d < r + v.radius {
            println!("{d:.1} vehicle {} at {q:?} radius {:.1}", v.template, v.radius);
        }
    }
    println!("ground {:?}", w.height_at(x as f64, z as f64));
}
