//! The level's ladders: every obstacle whose template is in
//! `c_CGLadders` or named like a ladder, with its place, axes, collision box (each LOD) and the
//! materials of its faces.
//!   cargo run --release -p bytefield-sim --example ladders -- data/game bf1942 El_Alamein [x z radius]
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let w = bytefield_sim::World::load(std::path::Path::new(&a[0]), &a[1], &a[2], "conquest").unwrap();
    let near: Option<[f32; 3]> = (a.len() >= 6).then(|| [a[3].parse().unwrap(), a[4].parse().unwrap(), a[5].parse().unwrap()]);
    for o in w.obstacles() {
        let ladder = o.name.to_ascii_lowercase().contains("ladder");
        let close = near.is_some_and(|[x, z, r]| (o.pos[0] - x).hypot(o.pos[2] - z) < r);
        if !ladder && !close {
            continue;
        }
        println!("{} pos {:?} rows {:?} scale {:?}", o.name, o.pos, o.rows, o.scale);
        for (k, m) in o.lods.iter().enumerate() {
            let mut mats: Vec<u32> = m.faces.iter().map(|f| f.material).collect();
            mats.sort();
            mats.dedup();
            println!("  lod {k}: bounds {:?} faces {} materials {:?}", m.bounds, m.faces.len(), mats);
        }
    }
}
