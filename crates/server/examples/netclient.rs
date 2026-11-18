//! A client without a window for the network numbers: joins a
//! `bytefield-server` (through a recording proxy, as the original's client does) and plays the
//! same script as a recorded session of the original, writing a mark per segment to `<out>/marks.txt` for
//! the network statistics tool.
//!
//!     cargo run --release -p bytefield-server --example netclient -- 127.0.0.1:14568 data/out/net/ours
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytefield_server::udp::{Hello, UdpClient};
use bytefield_sim::soldier::axis;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(addr), Some(out)) = (a.first(), a.get(1)) else {
        eprintln!("usage: netclient <server addr> <out dir>");
        std::process::exit(2);
    };
    std::fs::create_dir_all(out).expect("out dir");
    let mut marks = std::fs::OpenOptions::new().create(true).append(true).open(format!("{out}/marks.txt")).expect("marks");
    let mut mark = |label: &str| {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
        writeln!(marks, "{t:.4} {label}").unwrap();
    };
    let mut net = UdpClient::connect(addr.as_str(), Hello { team: 2, kit: 1, name: "netclient".into() }, Duration::from_secs(10)).expect("join");
    // the original's script: spawn screen 20 s, waiting to spawn 25 s, then 30 s each
    let script: [(&str, f64); 8] =
        [("spawnscreen", 20.0), ("spawnwait", 25.0), ("idle", 30.0), ("look", 30.0), ("walk", 30.0), ("still", 10.0), ("fire", 30.0), ("still2", 15.0)];
    let start = Instant::now();
    let (mut seg, mut seg_start, mut ticks) = (usize::MAX, 0.0, 0u64);
    mark("spawnscreen");
    loop {
        let t = start.elapsed().as_secs_f64();
        let mut at = 0.0;
        let Some(i) = script.iter().position(|&(_, d)| {
            at += d;
            t < at
        }) else {
            break;
        };
        if i != seg {
            if seg != usize::MAX {
                mark(script[i].0);
            }
            seg = i;
            seg_start = at - script[i].1;
        }
        let u = t - seg_start;
        net.poll();
        ticks += 1;
        // the spawn screen: still choosing; then the first spawn group, Return in the original
        net.mirror.hold = seg == 0;
        net.mirror.spawn = (seg > 0).then_some((None, 1));
        let axes = match script[seg].0 {
            "look" => vec![(axis::MOUSE_X, if (u * 2.0) as u64 % 2 == 0 { 0.4 } else { -0.4 })],
            "walk" => vec![(axis::THROTTLE, 1.0)],
            "fire" => vec![(axis::FIRE, if u % 10.0 < 6.0 { 1.0 } else { 0.0 })],
            _ => Vec::new(),
        };
        let step = net.mirror.next_step();
        net.send(step, axes);
        std::thread::sleep((start + Duration::from_secs_f64(ticks as f64 / 30.0)).saturating_duration_since(Instant::now()));
    }
    mark("end");
}
