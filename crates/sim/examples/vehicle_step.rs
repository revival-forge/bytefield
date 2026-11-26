//! What a ground vehicle drives up: the highest step and the steepest ramp, found by bisection, driving the vehicle at full
//! throttle into synthetic boxes and wedges on a flat stretch of a level. For the ground vehicles' navigation mesh (its
//! step and slope by vehicle):
//!   vehicle_step <game> <mod> <level> <x> <z> <template>... [--seconds 10]
use bytefield_sim::obstacle::Obstacle;
use bytefield_sim::soldier::axis;

struct Bench {
    game: std::path::PathBuf,
    module: String,
    level: String,
    at: [f32; 2],
    seconds: f64,
}

impl Bench {
    /// Drives `template` forward into what `make` places (given the ground height and the vehicle's forward sign along z) and
    /// returns the highest its body got over where it stood and how far it went along z.
    fn run(&self, template: &str, make: &dyn Fn(f32, f32) -> Option<Obstacle>) -> (f32, f32) {
        let mut w = bytefield_sim::World::load(&self.game, &self.module, &self.level, "conquest").unwrap();
        let ground = w.height_at(self.at[0] as f64, self.at[1] as f64).unwrap() as f32;
        // which way along z the vehicle faces with no rotation: settled once with nothing in front
        let sign = {
            let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
            let k = s.spawn_vehicle(template, [self.at[0], ground + 2.0, self.at[1]], [0.0; 3]).expect("the vehicle");
            s.step();
            s.vehicles[k].craft.body.rows[2][2].signum()
        };
        if let Some(o) = make(ground, sign) {
            w.push_obstacles(vec![o]);
        }
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        let k = s.spawn_vehicle(template, [self.at[0], ground + 2.0, self.at[1]], [0.0; 3]).expect("the vehicle");
        let soldier = ["USSoldier", "GermanSoldier", "GermanDesertSoldier"].into_iter().find(|t| w.vehicle(t).is_some()).expect("a soldier");
        let sol = s.spawn_vehicle(soldier, [self.at[0] + 6.0, ground + 2.0, self.at[1]], [0.0; 3]).unwrap();
        s.vehicles[sol].team = 1;
        s.vehicles[k].team = 1;
        s.enter(sol, k, 0);
        for _ in 0..30 {
            s.step();
        }
        let y0 = s.vehicles[k].craft.body.pos[1];
        let z0 = s.vehicles[k].craft.body.pos[2];
        let mut up = f32::MIN;
        for _ in 0..(self.seconds * 30.0) as usize {
            s.vehicles[sol].soldier_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0)]);
            s.step();
            up = up.max(s.vehicles[k].craft.body.pos[1] - y0);
        }
        (up, (s.vehicles[k].craft.body.pos[2] - z0) * sign)
    }

    /// A box ahead from `ahead` m to 30 m further, 16 m wide, its top `h` over the ground.
    fn step(&self, template: &str, h: f32) -> bool {
        let at = self.at;
        let flat = self.run(template, &|_, _| None).1;
        let (up, along) = self.run(template, &|g, s| {
            let (a, b) = (at[1] + s * 8.0, at[1] + s * 38.0);
            Some(Obstacle::synthetic_box("step", [at[0] - 8.0, g - 2.0, a.min(b)], [at[0] + 8.0, g + h, a.max(b)]))
        });
        let on = up > h - 0.3 && along > 12.0;
        eprintln!("   {template} step {h:.2} m: up {up:.2}, along {along:.1} (on the flat {flat:.1}): {}", if on { "on top" } else { "no" });
        on
    }

    /// A wedge ahead rising `rise` m at `deg`, then a flat top.
    fn ramp(&self, template: &str, deg: f32) -> bool {
        let at = self.at;
        let rise = 4.0;
        let (up, along) = self.run(template, &|g, s| {
            let len = rise / deg.to_radians().tan();
            let z = |d: f32| at[1] + s * d;
            // a wedge rising toward the vehicle's forward: built along +z and mirrored when it faces -z
            let (lo, hi) = ([at[0] - 8.0, g - 0.05, z(8.0)], [at[0] + 8.0, g + rise, z(8.0 + len)]);
            let mut r = Obstacle::synthetic_ramp("ramp", [lo[0], lo[1], lo[2].min(hi[2])], [hi[0], hi[1], lo[2].max(hi[2])]);
            if s < 0.0 {
                let c = z(8.0) + z(8.0 + len);
                let verts: Vec<[f32; 3]> = r.lods[0].vertices.iter().map(|v| [v[0], v[1], c - v[2]]).collect();
                let tris: Vec<[u32; 3]> = r.lods[0].faces.iter().map(|f| [f.v[0], f.v[2], f.v[1]]).collect();
                r = Obstacle::synthetic("ramp", verts, &tris);
            }
            let (a, b) = (z(8.0 + len), z(38.0 + len));
            let top = Obstacle::synthetic_box("top", [at[0] - 8.0, g - 2.0, a.min(b)], [at[0] + 8.0, g + rise, a.max(b)]);
            let base = r.lods[0].vertices.len() as u32;
            let mut verts = r.lods[0].vertices.clone();
            verts.extend_from_slice(&top.lods[0].vertices);
            let mut tris: Vec<[u32; 3]> = r.lods[0].faces.iter().map(|f| f.v).collect();
            tris.extend(top.lods[0].faces.iter().map(|f| f.v.map(|i| i + base)));
            Some(Obstacle::synthetic("ramp", verts, &tris))
        });
        // over the top's edge and on, not up the wedge and rolled back
        let on = up > rise - 0.4 && along > 10.0 + rise / deg.to_radians().tan();
        eprintln!("   {template} ramp {deg:.0}°: up {up:.2}, along {along:.1}: {}", if on { "on top" } else { "no" });
        on
    }
}

/// The nearest place to the bench's point with flat ground (within 0.5 m) 20 m across and 160 m along z, nothing standing
/// within 40 m of that stretch and dry.
fn flat_spot(b: &Bench) -> Option<[f32; 2]> {
    let w = bytefield_sim::World::load(&b.game, &b.module, &b.level, "conquest").unwrap();
    let water = w.level.terrain.water_level as f32;
    let h = |x: f32, z: f32| w.height_at(x as f64, z as f64).map(|y| y as f32);
    let mut spots = Vec::new();
    for dz in (-800..=800).step_by(16) {
        for dx in (-800..=800).step_by(16) {
            spots.push((((dx * dx + dz * dz) as f32).sqrt(), b.at[0] + dx as f32, b.at[1] + dz as f32));
        }
    }
    spots.sort_by(|a, b| a.0.total_cmp(&b.0));
    spots.into_iter().map(|s| [s.1, s.2]).find(|&[x, z]| {
        let mut ys = Vec::new();
        for k in -16..=16 {
            for i in -2..=2 {
                match h(x + i as f32 * 5.0, z + k as f32 * 5.0) {
                    Some(y) if y > water + 1.0 => ys.push(y),
                    _ => return false,
                }
            }
        }
        let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |a, &y| (a.0.min(y), a.1.max(y)));
        hi - lo < 0.5 && !w.obstacles().iter().any(|o| (o.center[0] - x).abs() < o.radius + 50.0 && (o.center[2] - z).abs() < o.radius + 120.0)
    })
}

/// The largest value in [lo, hi] for which `ok` holds, to `tol`, assuming it holds below some value and not above.
fn bisect(mut lo: f32, mut hi: f32, tol: f32, ok: impl Fn(f32) -> bool) -> f32 {
    if !ok(lo) {
        return f32::NAN;
    }
    while hi - lo > tol {
        let m = (lo + hi) / 2.0;
        if ok(m) {
            lo = m;
        } else {
            hi = m;
        }
    }
    lo
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 7 {
        eprintln!("vehicle_step <game> <mod> <level> <x> <z> <template>... [--seconds 10]");
        std::process::exit(2);
    }
    let seconds = a.iter().position(|x| x == "--seconds").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(10.0);
    let mut b = Bench { game: a[1].clone().into(), module: a[2].clone(), level: a[3].clone(), at: [a[4].parse().unwrap(), a[5].parse().unwrap()], seconds };
    b.at = flat_spot(&b).expect("no flat stretch within 800 m");
    eprintln!("flat stretch at {:.0},{:.0}", b.at[0], b.at[1]);
    let skip = a.iter().position(|x| x == "--seconds").map_or(usize::MAX, |i| i + 1);
    for (i, t) in a.iter().enumerate().skip(6) {
        if t.starts_with("--") || i == skip {
            continue;
        }
        let step = bisect(0.1, 3.0, 0.05, |h| b.step(t, h));
        let ramp = bisect(10.0, 60.0, 1.0, |d| b.ramp(t, d));
        println!("{t}: highest step {step:.2} m, steepest ramp {ramp:.0}°");
    }
}
