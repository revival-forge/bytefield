//! Which seats fire from a camera that turns by itself (`fireInCameraDof` on a weapon, a RotationalBundle on its seat's
//! `Camera`): our bots aim from the camera held still, so these are the
//! seats where that could differ. Usage: camera_dof <game> <mod>...
use std::collections::BTreeSet;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = std::path::Path::new(&a[0]);
    for m in &a[1..] {
        let Ok((_, vfs)) = bytefield_con::session::mount(game, m) else {
            println!("{m}: does not mount");
            continue;
        };
        let mut seen = BTreeSet::new();
        for (level, _) in bytefield_con::session::levels(&vfs) {
            let Ok(w) = bytefield_sim::World::load(game, m, &level, "conquest") else { continue };
            let w: &'static bytefield_sim::World = Box::leak(Box::new(w));
            let mut s = bytefield_sim::Sim::new(w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            for _ in 0..3 {
                s.step();
            }
            for v in &s.vehicles {
                
                for (part, f) in &v.weapons {
                    if !f.template.fire_in_camera {
                        continue;
                    }
                    let seat = v.craft.parts[*part].seat;
                    // a camera with a bundle turns only on an axis with an input and a speed (`DaihatsuCamera_For_PCO1`: `setMaxSpeed 0/0/0`,
                    // still; counting every bundle would give 291 of 291)
                    let turns = v.craft.parts.iter().any(|p| p.seat == seat && matches!(p.kind, bytefield_sim::craft::Kind::Camera) && p.bundle.as_ref().is_some_and(|b| (0..3).any(|i| b.input_to[i].is_some() && b.max_speed[i] != 0.0)));
                    if seen.insert((v.template.clone(), seat, f.template.name.clone())) {
                        println!("{m} {level} {} seat {seat} {} camera {}", v.template, f.template.name, if turns { "TURNS" } else { "still" });
                    }
                }
            }
        }
    }
}
