//! Which aircraft each level puts in the world, by team, and what each seat fires: the ground for an air-to-air front (who can meet
//! whom in the air, with what). Only what is in the world once the round starts (spawners included); wrecks and empty seats count.
//!   air_census <game> <mod> <level>...
//! Team: the vehicle's, or else that of the nearest flag owned at the start (DC's spawned vehicles have none).
//! One line per (level, team, template): `level team class template xN seats=<n> | seat:key:weapon(projectile,v=<m/s>,rof=<r>,mag=<n>x<m>) ...`
//! and a last line per level: `level fighters-per-team t1=<n> t2=<n> both=<yes|no>` (a fighter: a plane with a forward-firing gun
//! at its pilot's seat that also flies: put at 900 m and 90 m/s with full throttle and the wings level, it loses less than 100 m in
//! 20 s; speeder bikes and podracers do not).
use bytefield_sim::bot_api::VehicleClass;
use std::collections::BTreeMap;

fn d2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Does this template fly? Put 900 m over the middle of the map at 90 m/s with full throttle and flown by the stick of a player who
/// keeps the wings level and the nose where it holds the height (the level-flight law of `air_envelope`): it must lose less than 100 m (the ones that fly lose 10-55 m settling, the ones that do not 800)
/// in 20 s. Speeder bikes and podracers do not.
fn flies(w: &bytefield_sim::World, template: &str) -> bool {
    use bytefield_sim::soldier::axis;
    let dt = 1.0f32 / 30.0;
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut s = bytefield_sim::Sim::new(w, dt as f64);
    s.round.playing = true;
    let Some(ground) = w.height_at(1024.0, 1024.0) else { return false };
    let at = [1024.0, ground as f32 + 900.0, 1024.0];
    let Some(craft) = s.spawn_vehicle(template, at, [0.0; 3]) else { return false };
    let Some(soldier) = s.spawn_vehicle("USSoldier", [at[0] + 30.0, at[1], at[2]], [0.0; 3]) else { return false };
    s.vehicles[soldier].team = 1;
    s.enter(soldier, craft, 0);
    s.vehicles[craft].craft.body.vel = [0.0, 0.0, 90.0];
    let mut before = s.vehicles[craft].craft.body.rows;
    // pitch rate (nose up) and roll rate (right wing down), degrees per second
    let (mut pitch_rate, mut roll_rate) = (0.0f32, 0.0f32);
    for _ in 0..(20.0 / dt) as usize {
        let b = &s.vehicles[craft].craft.body;
        let [r, u, f] = b.rows;
        let speed = dot(b.vel, b.vel).sqrt();
        let (pitch, bank) = (f[1].clamp(-1.0, 1.0).asin().to_degrees(), -r[1].atan2(u[1]).to_degrees());
        let height = b.pos[1] - ground as f32;
        // (the stick's reach grows with the square of the speed: less of it the faster it goes)
        let k = (45.0 / speed.max(20.0)).powi(2).clamp(0.3, 3.0);
        let want = ((900.0 - height) * 0.4).clamp(-12.0, 30.0);
        let p = (k * (0.05 * (want - pitch) - 0.02 * pitch_rate)).clamp(-1.0, 1.0);
        let roll = (k * (0.02 * (0.0 - bank) - 0.006 * roll_rate)).clamp(-1.0, 1.0);
        s.vehicles[soldier].movement.as_mut().unwrap().set_input(&[(axis::THROTTLE, 1.0), (1, -p), (2, roll)]);
        s.step();
        let now = s.vehicles[craft].craft.body.rows;
        let rate = |x: f32| x.clamp(-1.0, 1.0).asin().to_degrees() / dt;
        (pitch_rate, roll_rate) = (rate(dot(now[2], before[1])), rate(-dot(now[0], before[1])));
        before = now;
    }
    let lost = at[1] - s.vehicles[craft].craft.body.pos[1];
    if std::env::var_os("AIR_CENSUS_DEBUG").is_some() {
        eprintln!("{template}: lost {lost:.0} m");
    }
    lost < 100.0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("air_census <game> <mod> <level>...");
        std::process::exit(2);
    }
    for level in &args[3..] {
        let w = match bytefield_sim::World::load(std::path::Path::new(&args[1]), &args[2], level, "conquest") {
            Ok(w) => w,
            Err(e) => {
                println!("{level} error {e}");
                continue;
            }
        };
        let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        // (a vehicle with no team of its own, as DC's, goes with the nearest flag that has one at the start)
        let flags: Vec<([f32; 3], u32)> = s.modes.conquest.as_ref().map_or(Vec::new(), |c| c.points.iter().filter(|p| p.team != 0).map(|p| (p.pos, p.team)).collect());
        let side = |v: &bytefield_sim::mover::Vehicle| -> u32 {
            if v.team != 0 {
                return v.team;
            }
            let p = v.craft.body.pos;
            flags.iter().min_by(|a, b| d2(a.0, p).total_cmp(&d2(b.0, p))).map_or(0, |f| f.1)
        };
        // (team, template) -> (class, count, seats, weapons line, armed pilot)
        let mut seen: BTreeMap<(u32, String), (VehicleClass, usize, usize, String, bool)> = BTreeMap::new();
        for v in s.vehicles.iter().filter(|v| v.in_world() && !v.is_soldier() && !v.entry.seats.is_empty()) {
            let class = VehicleClass::of(v);
            if !class.flies() {
                continue;
            }
            let e = seen.entry((side(v), v.template.clone())).or_insert_with(|| {
                let mut ws: Vec<String> = Vec::new();
                let mut pilot_gun = false;
                for (part, f) in &v.weapons {
                    let t = &f.template;
                    let seat = v.craft.parts[*part].seat;
                    if seat == 0 && t.velocity > 0.0 && t.input_fire != 23 {
                        pilot_gun = true;
                    }
                    ws.push(format!("{seat}:{}:{}({},v={:.0},rof={:.0},mag={}x{})", t.input_fire, t.name, t.projectile, t.velocity, t.round_of_fire, t.mag_size, t.num_of_mag));
                }
                (class, 0, v.entry.seats.len(), ws.join(" "), pilot_gun)
            });
            e.1 += 1;
        }
        let mut fighters: BTreeMap<u32, usize> = BTreeMap::new();
        let mut flew: BTreeMap<String, bool> = BTreeMap::new();
        for ((team, template), (class, n, seats, ws, gun)) in &seen {
            let up = *class == VehicleClass::Plane && *gun && *flew.entry(template.clone()).or_insert_with(|| flies(&w, template));
            println!("{level} {team} {class:?} {template} x{n} seats={seats}{} | {ws}", if *class == VehicleClass::Plane && *gun && !up { " (does not fly)" } else { "" });
            if up {
                *fighters.entry(*team).or_default() += n;
            }
        }
        let (a, b) = (fighters.get(&1).copied().unwrap_or(0), fighters.get(&2).copied().unwrap_or(0));
        println!("{level} fighters-per-team t1={a} t2={b} both={}", if a > 0 && b > 0 { "yes" } else { "no" });
    }
}
