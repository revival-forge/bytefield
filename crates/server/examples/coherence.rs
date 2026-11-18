//! What N clients know against the server, step by step.
//!     cargo run --release -p bytefield-server --example coherence -- data/game bf1942 Berlin [--clients 8] [--bots 8] [--seconds 60] [--loss 5] [--jitter 0: ticks more per packet, at random] [--seeds 1,2,3] [--x 90] [--out data/out/night293/coherence]
//! A line per seed in <out>/results.jsonl, and at a failure the first of each invariant in <out>/<mod>-<level>-loss<L>-seed<S>.txt.
#[path = "../tests/base/common/coherence.rs"]
mod coherence;

use std::io::Write;

use coherence::{Config, run};
use bytefield_sim::World;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: coherence <game> <mod> <level> [--clients 8] [--bots 8] [--seconds 60] [--loss 5] [--seeds 1,2,3] [--x 90] [--out dir]");
        std::process::exit(2);
    }
    let opt = |n: &str, d: &str| a.iter().position(|x| x == n).and_then(|i| a.get(i + 1)).cloned().unwrap_or_else(|| d.to_string());
    let num = |n: &str, d: u64| opt(n, &d.to_string()).parse::<u64>().expect("a number");
    let loss = num("--loss", 5);
    let out = opt("--out", "data/out/night293/coherence");
    std::fs::create_dir_all(&out).unwrap();
    let world: &'static World = Box::leak(Box::new(World::load(std::path::Path::new(&a[0]), &a[1], &a[2], "conquest").expect("level")));
    let mut failed = false;
    for seed in opt("--seeds", "1,2,3").split(',').map(|s| s.parse::<u64>().expect("a seed")) {
        let c = Config { clients: num("--clients", 8) as usize, bots: num("--bots", 8) as usize, steps: num("--seconds", 60) * 30, loss: loss as f32 / 100.0, jitter: num("--jitter", 0), seed, settle: 150, x_limit: num("--x", 90) };
        let t = std::time::Instant::now();
        let r = run(world, &c);
        let line = format!(
            "{{\"mod\":\"{}\",\"level\":\"{}\",\"loss\":{loss},\"jitter\":{},\"seed\":{seed},\"clients\":{},\"bots\":{},\"ok\":{},\"fails\":{:?},\"x_max\":{},\"stale_err\":{:.2},\"stale_age\":{},\"stale_bound\":{:.2},\"stale_age_max\":{},\"bytes_per_step\":{},\"joined\":{},\"left\":{},\"deaths\":{},\"boarded\":{},\"secs\":{:.1}}}",
            a[1], a[2], c.jitter, c.clients, c.bots, r.ok(), &r.fails[1..], r.x_max, r.stale_err, r.stale_err_age, r.stale_err_bound, r.stale_age_max, r.bytes / r.steps.max(1), r.joined, r.left, r.deaths, r.boarded, t.elapsed().as_secs_f32()
        );
        println!("{line}");
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(format!("{out}/results.jsonl")).unwrap();
        writeln!(f, "{line}").unwrap();
        println!("  x: {}\n  stale worst: {}", r.x_what.lines().next().unwrap_or(""), r.stale_err_what);
        if !r.ok() {
            failed = true;
            let mut f = std::fs::File::create(format!("{out}/{}-{}-loss{loss}-jitter{}-seed{seed}.txt", a[1], a[2], c.jitter)).unwrap();
            writeln!(f, "{c:?}\nx_max {}: {}\n", r.x_max, r.x_what).unwrap();
            for v in &r.first {
                writeln!(f, "== invariant {} at step {}, client {}, object {:?}\n{}\n", v.invariant, v.step, v.client, v.object, v.diff).unwrap();
                println!("  invariant {} at step {}, client {}, object {:?}: {}", v.invariant, v.step, v.client, v.object, v.diff.lines().next().unwrap_or(""));
            }
        }
    }
    std::process::exit(failed as i32);
}
