//! Whether a soldier can walk out of a place: put at a point, it walks straight in each of N directions for some seconds, jumping
//! now and then, and the distance it gets is printed. A spawn point the bots never leave is either closed by the collision
//! meshes (every direction stops short) or only closed in the path map (some direction gets far).
//!     walk_out <game> <mod> <level> <x,y,z> [directions=16] [seconds=6]
use bytefield_sim::bot_api::{BotView, Mind};
use bytefield_sim::soldier::axis;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

struct Straight(Arc<AtomicU32>);

impl Mind for Straight {
    fn think(&mut self, v: &BotView) -> Vec<(usize, f32)> {
        let a = f32::from_bits(self.0.load(Ordering::Relaxed));
        let (d, f) = ([a.cos(), a.sin()], v.me.view[2]);
        let g = (f[2] * d[0] - f[0] * d[1]).atan2(f[0] * d[0] + f[2] * d[1]).to_degrees();
        let jump = (v.time * 2.0) as u64 % 3 == 0;
        vec![(axis::MOUSE_X, (g / 3.0).clamp(-10.0, 10.0)), (axis::MOUSE_Y, 0.0), (axis::THROTTLE, if g.abs() < 20.0 { 1.0 } else { 0.0 }), (axis::ACTION, if jump { 1.0 } else { 0.0 })]
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("walk_out <game> <mod> <level> <x,y,z> [directions=16] [seconds=6]");
        std::process::exit(2);
    }
    let at: Vec<f32> = a[4].split(',').map(|q| q.parse().expect("x,y,z")).collect();
    let at = [at[0], at[1], at[2]];
    let n: usize = a.get(5).map_or(16, |x| x.parse().unwrap());
    let secs: f64 = a.get(6).map_or(6.0, |x| x.parse().unwrap());
    let w = bytefield_sim::World::load(Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    let dir = Arc::new(AtomicU32::new(0));
    for team in 1..=2 {
        let d = dir.clone();
        s.set_minds(team, Box::new(move |_| Box::new(Straight(d.clone()))));
    }
    let p = s.join(0, true);
    let mut soldier = None;
    for _ in 0..600 {
        s.step();
        soldier = s.players[p].soldier.filter(|_| s.players[p].alive);
        if soldier.is_some() {
            break;
        }
    }
    let i = soldier.expect("the bot spawns");
    let mut out = 0;
    for k in 0..n {
        let ang = k as f32 * std::f32::consts::TAU / n as f32;
        dir.store(ang.to_bits(), Ordering::Relaxed);
        s.vehicles[i].craft.body.pos = at;
        s.vehicles[i].craft.body.vel = [0.0; 3];
        for _ in 0..(secs * 30.0) as u64 {
            s.step();
        }
        let q = s.vehicles[i].craft.body.pos;
        let d = (q[0] - at[0]).hypot(q[2] - at[2]);
        out += (d > secs as f32 * 2.5) as usize;
        println!("dir {:>3.0} deg ({:+.2}, {:+.2}): {:>5.1} m, ends at [{:.0}, {:.0}, {:.0}]", ang.to_degrees(), ang.cos(), ang.sin(), d, q[0], q[1], q[2]);
    }
    println!("summary {} {:?}: {out} of {n} directions get out", a[3], at);
}
