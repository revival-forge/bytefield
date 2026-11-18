//! Which conquest flags cannot be taken standing on the ground (or swimming)? bots3's team head leaves those out of its attack
//! targets unless a teammate has been inside one (`bots3::team::reachable_on_foot`). One line per flag out of reach, per level.
//!   flag_reach <game> <mod> [<mod>...]
use bytefield_sim::bots3::team::reachable_on_foot;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        eprintln!("flag_reach <game> <mod> [<mod>...]");
        std::process::exit(2);
    }
    let game = std::path::Path::new(&a[0]);
    let (mut levels, mut flags, mut out) = (0, 0, 0);
    for m in &a[1..] {
        let Ok((_, vfs)) = bytefield_con::session::mount(game, m) else {
            println!("{m}: does not mount");
            continue;
        };
        for (level, _) in bytefield_con::session::levels(&vfs) {
            let Ok(w) = bytefield_sim::World::load(game, m, &level, "conquest") else {
                println!("{m} {level}: does not load");
                continue;
            };
            let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
            s.start_conquest();
            let Some(c) = s.modes.conquest.as_ref() else { continue };
            levels += 1;
            let water = w.level.terrain.water_level as f32;
            for (i, p) in c.points.iter().enumerate() {
                flags += 1;
                let pos = p.pos.map(|x| x as f32);
                let r = p.template.radius as f32;
                if !reachable_on_foot(pos, r, water, |x, z| w.height_at(x as f64, z as f64).map(|h| h as f32)) {
                    out += 1;
                    let g = w.height_at(pos[0] as f64, pos[2] as f64).unwrap_or(f64::NAN);
                    println!("{m} {level} flag {i} at {:?} radius {r} ground {g:.1} team {} takeable {}", pos.map(|x| x.round() as i32), p.team, !p.template.unable_to_change_team);
                }
            }
        }
    }
    println!("{levels} levels, {flags} flags, {out} out of reach on foot");
}
