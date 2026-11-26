//! F3: a Patriot at rest on DC_Basrahs_Edge fires its guided rocket with the tubes level (or raised, `up`);
//! the rocket's state and hits, frame by frame, from its birth. Against the original game's reference run of the same case.
//!     f3_patriot <game> [level|up] [frames]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let game = std::path::Path::new(&a[1]);
    let up = a.get(2).map_or(false, |v| v == "up");
    let frames: usize = a.get(3).map_or(40, |v| v.parse().unwrap());
    let w = bytefield_sim::World::load(game, "DC_Final", "DC_Basrahs_Edge", "conquest").unwrap();
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    let (x, z) = (1000.0, 1000.0);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + 0.3;
    let l = s.spawn_vehicle("Patriot", [x, y, z], [0.0; 3]).unwrap();
    let soldier = s.spawn_vehicle("USSoldier", [x - 2.0, y, z + 2.2], [0.0; 3]).unwrap();
    s.vehicles[soldier].team = 2;
    for _ in 0..30 {
        s.step();
    }
    s.enter(soldier, l, 0);
    if up {
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::MOUSE_Y, -1.0)]);
        for _ in 0..40 {
            s.step();
        }
        s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::MOUSE_Y, 0.0)]);
        for _ in 0..10 {
            s.step();
        }
    }
    let lv = &s.vehicles[l];
    println!("Patriot at {:?} radius {} parts:", lv.craft.body.pos, lv.radius);
    for (k, c) in lv.contacts.iter().enumerate() {
        let (rows, pos) = c.frame(&lv.craft.body);
        println!("  part {k} root {} radius {:.2} verts {} faces {:?} at {:?} fwd {:?}", c.is_root, c.radius, c.vertices.len(), c.faces.iter().map(|f| f.faces.len()).collect::<Vec<_>>(), pos, rows[2]);
    }
    s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, 1.0)]);
    let n = s.vehicles.len();
    let mut born = None;
    for _ in 0..40 {
        s.step();
        if s.vehicles.len() > n {
            born = Some(s.clock.steps);
            break;
        }
    }
    let r = n;
    let born = born.expect("no rocket");
    println!("rocket {} born at step {born}, radius {}", s.vehicles[r].template, s.vehicles[r].radius);
    for (k, c) in s.vehicles[r].contacts.iter().enumerate() {
        println!("  part {k} root {} radius {:.2} verts {} faces {:?} vertices {:?}", c.is_root, c.radius, c.vertices.len(), c.faces.iter().map(|f| f.faces.len()).collect::<Vec<_>>(), c.vertices);
    }
    s.vehicles[soldier].soldier_mut().unwrap().set_input(&[(bytefield_sim::soldier::axis::FIRE, 0.0)]);
    for f in 0..frames {
        let v = &s.vehicles[r];
        let b = &v.craft.body;
        let hp = v.armor.as_ref().map(|a| (a.hp, a.destroyed));
        println!("+{f} step {} pos {:?} vel {:?} fwd {:?} sleep {} hp {:?} inside {:?}", s.clock.steps, b.pos, b.vel, b.rows[2], b.sleepiness, hp, s.vehicles[soldier].entry.inside);
        for (k, c) in v.contacts.iter().enumerate() {
            for h in &c.hits {
                println!("    part {k} hit {:?}", h);
            }
            for i in &c.last_impulses {
                println!("    part {k} impulse with {:?} r {:?} v {:?} n {:?} depth {}", i.with, i.r, i.v, i.n, i.depth);
            }
        }
        let lvb = &s.vehicles[l].craft.body;
        for (k, c) in s.vehicles[l].contacts.iter().enumerate() {
            for h in &c.hits {
                println!("    Patriot part {k} hit {:?}", h);
            }
        }
        let _ = lvb;
        s.step();
    }
    for d in s.journal.damage_log.iter().filter(|d| d.vehicle == r || d.vehicle == l) {
        println!("damage step {} vehicle {} amount {} source {:?} hp {}", d.step, d.vehicle, d.amount, d.source, d.hp);
    }
}
