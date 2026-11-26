//! A lone soldier pushed forward from a point with a heading, to tell a place the walking mesh lets through from one the body
//! cannot pass: prints where it is each half second.
//!   walk_probe <game> <mod> <level> <x,z> <heading deg> [seconds] [soldier template]
//! With `SCAN=x0,x1,z0,z1` (and `HS=h,...`, heights over the ground, 0.3,1.0,1.4 by default) it walks no one: the z's, every 2 cm,
//! where level rays from x0 to x1 at those heights meet nothing (the rays the walking mesh's doors are tried with).
use bytefield_sim::soldier::axis;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let g = std::path::Path::new(&a[1]);
    let w = bytefield_sim::World::load(g, &a[2], &a[3], "conquest").unwrap();
    let xz: Vec<f32> = a[4].split(',').map(|s| s.parse().unwrap()).collect();
    let yaw: f32 = a[5].parse().unwrap();
    let secs: f32 = a.get(6).map_or(4.0, |s| s.parse().unwrap());
    let tpl = a.get(7).map_or("US_Soldier", |s| s.as_str());
    let ground = w.height_at(xz[0] as f64, xz[1] as f64).unwrap() as f32;
    if let Ok(scan) = std::env::var("SCAN") {
        // SCAN=x0,x1,z0,z1: which z's let a level ray at 0.3, 1.0 and 1.4 m pass from x0 to x1, every 2 cm
        let v: Vec<f32> = scan.split(',').map(|s| s.parse().unwrap()).collect();
        let hs: Vec<f32> = std::env::var("HS").map_or(vec![0.3, 1.0, 1.4], |s| s.split(',').map(|x| x.parse().unwrap()).collect());
        let mut z = v[2];
        while z <= v[3] {
            let free = hs.iter().all(|&h| !w.walk_blocked([v[0], ground + h, z], [v[1], ground + h, z]));
            if free {
                println!("free at z={z:.2}");
            }
            z += 0.02;
        }
        return;
    }
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    let sol = s.spawn_vehicle(tpl, [xz[0], ground + 1.6, xz[1]], [yaw, 0.0, 0.0]).expect("a soldier");
    for _ in 0..30 {
        s.step();
    }
    for k in 0..(secs * 30.0) as usize {
        s.vehicles[sol].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0)]);
        s.step();
        if k % 15 == 0 {
            let p = s.vehicles[sol].craft.body.pos;
            println!("t={:.1} pos=[{:.2}, {:.2}, {:.2}]", k as f32 / 30.0, p[0], p[1], p[2]);
        }
    }
}
