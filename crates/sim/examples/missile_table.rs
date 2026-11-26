//! A missile's flight from outside: an aircraft flying straight and level on rails fires its secondary (the alt-fire
//! key) and the first missile's time, speed, distance flown and range from the launcher are printed every `--step` s.
//!   missile_table <game> <mod> <level> --plane mig29 [--speed 110] [--seconds 8] [--step 0.25] [--height 600] [--rails]
//! With `--rails` the launcher keeps `--speed` along +z and its attitude each frame (without it, it flies free with the throttle
//! open), so the table shows the missile alone. A line per sample:
//!   `t speed axial_speed flown range launcher_speed`
use bytefield_sim::soldier::axis;

type V3 = [f32; 3];
const DT: f32 = 1.0 / 30.0;

fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let plane = opt("--plane").expect("--plane");
    let speed: f32 = opt("--speed").map_or(110.0, |x| x.parse().unwrap());
    let seconds: f32 = opt("--seconds").map_or(8.0, |x| x.parse().unwrap());
    let step: f32 = opt("--step").map_or(0.25, |x| x.parse().unwrap());
    let height: f32 = opt("--height").map_or(600.0, |x| x.parse().unwrap());
    let rails = a.iter().any(|x| x == "--rails");
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, DT as f64);
    s.round.playing = true;
    let (x, z) = (1024.0f32, 600.0f32);
    let y = w.height_at(x as f64, z as f64).unwrap() as f32 + height;
    let craft = s.spawn_vehicle(&plane, [x, y, z], [0.0; 3]).unwrap_or_else(|| panic!("no aircraft {plane}"));
    let soldier = s.spawn_vehicle("USSoldier", [x + 40.0, y, z], [0.0; 3]).expect("a soldier");
    s.vehicles[soldier].team = 1;
    s.enter(soldier, craft, 0);
    {
        let b = &mut s.vehicles[craft].craft.body;
        b.rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        b.vel = [0.0, 0.0, speed];
    }
    let mut fire_at: Option<u64> = None;
    let mut first: Option<usize> = None;
    let mut origin = [0.0; 3];
    let mut next = 0.0f32;
    for i in 0..((seconds + 1.0) / DT) as usize {
        // one press of the alt-fire at step 5, then released
        let keys: &[(usize, f32)] = if i == 5 { &[(axis::THROTTLE, 1.0), (axis::ALT_FIRE, 1.0)] } else { &[(axis::THROTTLE, 1.0)] };
        s.vehicles[soldier].soldier_mut().unwrap().set_input(keys);
        if rails {
            let b = &mut s.vehicles[craft].craft.body;
            b.rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            (b.pos[0], b.pos[1]) = (x, y);
            (b.vel, b.rotvel) = ([0.0, 0.0, speed], [0.0; 3]);
        }
        s.step();
        if first.is_none() {
            if let Some(k) = s.projectiles.iter().position(|p| p.owner == Some(craft)) {
                first = Some(k);
                fire_at = Some(s.clock.steps);
                origin = s.projectiles[k].pos;
                println!("fired at step {i}: {} at {:?}, launcher at {:?}, vel {:?}", s.projectiles[k].template.name, origin, s.vehicles[craft].craft.body.pos, s.projectiles[k].vel);
            }
            continue;
        }
        let k = first.unwrap();
        let t = (s.clock.steps - fire_at.unwrap()) as f32 * DT;
        let p = &s.projectiles[k];
        let (pp, pv) = match &p.rigid {
            Some(v) => (v.craft.body.pos, v.craft.body.vel),
            None => (p.pos, p.vel),
        };
        if t + 1e-4 >= next {
            let fwd = match &p.rigid {
                Some(v) => v.craft.body.rows[2],
                None => [0.0, 0.0, 1.0],
            };
            let axial = pv[0] * fwd[0] + pv[1] * fwd[1] + pv[2] * fwd[2];
            let l = &s.vehicles[craft].craft.body;
            let rel = [pp[0] - l.pos[0], pp[1] - l.pos[1], pp[2] - l.pos[2]];
            let d = [pp[0] - origin[0], pp[1] - origin[1], pp[2] - origin[2]];
            println!("{t:5.2} {:7.1} {:7.1} {:7.1} {:7.1} {:6.1}{}", len(pv), axial, len(d), len(rel), len(l.vel), if p.end.is_some() { "  END" } else { "" });
            next += step;
        }
        if p.end.is_some() || t > seconds {
            break;
        }
    }
}
