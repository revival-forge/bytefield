//! Why does almost everything come out `hidden` from a helicopter over a town? Soldiers of a running round are the targets (chest
//! height); for each eye height over the target's ground and each horizontal offset, the share the player API calls hidden, split
//! into the terrain between (sampled as the API does) and an object of the level (the rest). The probe is a mind: it looks through
//! the same `BotView` a bot has.
//!   air_sight <game> <mod> <level> [seconds=60] [players=32]
//! Output: one line per (height, offset): elevation angle of the line, % of chests hidden, of them % by terrain, % of bodies
//! wholly hidden ([`BotView::body_seen`]: chest, head and shoulders), and the number of samples.
use bytefield_sim::bot_api::{BotView, Mind};
use std::sync::atomic::{AtomicBool, Ordering};

static DONE: AtomicBool = AtomicBool::new(false);

struct Probe {
    at: f32,
}

impl Mind for Probe {
    fn think(&mut self, view: &BotView) -> Vec<(usize, f32)> {
        if view.time < self.at as f64 || DONE.swap(true, Ordering::SeqCst) {
            return Vec::new();
        }
        let bodies: Vec<[f32; 3]> = view.others().into_iter().filter(|o| !o.in_vehicle).map(|o| o.pos).collect();
        let targets: Vec<[f32; 3]> = bodies.iter().map(|p| [p[0], p[1] + bytefield_sim::bot_api::BODY_CHEST, p[2]]).collect();
        println!("{} soldiers on foot as targets", targets.len());
        let terrain = |a: [f32; 3], b: [f32; 3]| {
            let d: f32 = (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>().sqrt();
            let n = (d / 2.0).ceil().max(1.0) as usize;
            (1..n).any(|i| {
                let t = i as f32 / n as f32;
                let p: [f32; 3] = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
                view.ground(p[0], p[2]).is_some_and(|h| h > p[1] - 0.2)
            })
        };
        println!("height  offset  elev  hidden%  by-terrain%  body-hidden%  samples");
        for h in [20.0f32, 40.0, 80.0] {
            for d in [0.0f32, 30.0, 60.0, 120.0, 250.0] {
                let (mut n, mut hid, mut ter, mut body) = (0u32, 0u32, 0u32, 0u32);
                for (t, pos) in targets.iter().zip(&bodies) {
                    // eight bearings round the target
                    for k in 0..8 {
                        let a = k as f32 * std::f32::consts::FRAC_PI_4;
                        let (x, z) = (t[0] + d * a.cos(), t[2] + d * a.sin());
                        let Some(g) = view.ground(x, z) else { continue };
                        let eye = [x, g.max(t[1] - 1.2) + h, z];
                        n += 1;
                        if terrain(eye, *t) {
                            hid += 1;
                            ter += 1;
                        } else if view.hidden(eye, *t) {
                            hid += 1;
                        }
                        if view.body_seen(eye, *pos).is_none() {
                            body += 1;
                        }
                    }
                }
                let elev = h.atan2(d.max(1e-3)).to_degrees();
                let pct = |x: u32| 100.0 * x as f32 / n.max(1) as f32;
                println!("{h:>5.0}  {d:>6.0}  {elev:>4.0}  {:>7.1}  {:>11.1}  {:>12.1}  {n}", pct(hid), pct(ter), pct(body));
            }
        }
        Vec::new()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("air_sight <game> <mod> <level> [seconds=60] [players=32]");
        std::process::exit(2);
    }
    let seconds: f32 = args.get(4).map_or(60.0, |x| x.parse().unwrap());
    let players: usize = args.get(5).map_or(32, |x| x.parse().unwrap());
    let w = bytefield_sim::World::load(std::path::Path::new(&args[1]), &args[2], &args[3], "conquest").expect("world");
    let mut s = bytefield_sim::Sim::new(&w, 1.0 / 30.0);
    s.round.playing = true;
    s.start_conquest();
    s.enable_bots();
    for team in [1u32, 2] {
        let others = bytefield_sim::bots2::maker(&args[3], 1);
        // (the first of each team probes; its team plays on with the others)
        s.set_minds(team, Box::new(move |k| if k == 0 { Box::new(Probe { at: seconds }) } else { others(k) }));
    }
    for _ in 0..players {
        s.join(0, true);
    }
    for _ in 0..((seconds + 10.0) * 30.0) as u64 {
        s.step();
    }
    if !DONE.load(Ordering::SeqCst) {
        eprintln!("the probe never ran: its player was not alive or was not player 0");
    }
}
