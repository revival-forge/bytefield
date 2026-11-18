//! How many players a server carries: the real server loop ([`UdpServer::poll`], `tick`), with `<clients>` real
//! `UdpClient`s on loopback, in a few threads of this one process, instead of a game each. A client here does what costs the server: it joins with
//! its own socket and token, chooses a spawn, sends its actions at 30 Hz, decodes every delta into its `Mirror` and acks it. It draws and predicts
//! nothing, which costs the server nothing either.
//!     cargo run --release -p bytefield-server --example loadtest -- data/game bf1942 Berlin 64 [--seconds 60] [--warmup 15] [--bots 0] [--threads 8] [--vehicles 50] [--seed 1] [--mode conquest] [--loss PERMILLE] [--json out.jsonl] [--every S]
//! `--every S`: a line every S seconds (step, memory, objects), for a soak.
//! `--loss N`: of 1000 frames of 1472 B, N are lost each way (a bigger datagram needs all of its frames).
//! `--vehicles P`: every 5 s, P % of the players alive are put in a free vehicle (walking there is not what is being measured).
//! Prints one summary: the server's step against its 33.3 ms, what each client got, what got lost.
//! What it cannot see: real networks (latency, loss, routers), a game client's own frame, and what 64 people choose to do.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use bytefield_server::udp::{Hello, UdpClient, UdpServer};
use bytefield_server::{Server, TICK, round};
use bytefield_sim::World;
use bytefield_sim::soldier::axis;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 2001) as f32 / 1000.0 - 1.0
    }
}

/// A few seconds of what a person does on foot or at the wheel: mostly moving and looking, a good part of it firing.
fn actions(r: &mut Rng) -> (Vec<(usize, f32)>, u32) {
    let mut a = vec![(axis::THROTTLE, if r.next() % 5 == 0 { -1.0 } else { 1.0 }), (axis::YAW, r.unit() * 0.5), (axis::MOUSE_X, r.unit() * 0.3), (axis::MOUSE_Y, r.unit() * 0.1)];
    if r.next() % 100 < 40 {
        a.push((axis::FIRE, 1.0));
    }
    (a, 30 + (r.next() % 120) as u32)
}

#[derive(Clone, Copy, Default)]
struct ClientReport {
    snapshots: u64,
    bytes: u64,
    biggest: usize,
    /// sum of how many steps the newest snapshot was behind the server, when sending
    behind: u64,
    behind_worst: u64,
    samples: u64,
    /// steps with no new snapshot since the previous one (a hole in what arrived)
    holes: u64,
}

fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn udp_rcvbuf_errors() -> u64 {
    // /proc/net/snmp: the second "Udp:" line holds the numbers, in the order of the first
    let Ok(s) = std::fs::read_to_string("/proc/net/snmp") else { return 0 };
    let mut l = s.lines().filter(|l| l.starts_with("Udp:"));
    let (Some(h), Some(v)) = (l.next(), l.next()) else { return 0 };
    let i = h.split_whitespace().position(|x| x == "RcvbufErrors").unwrap_or(usize::MAX);
    v.split_whitespace().nth(i).and_then(|x| x.parse().ok()).unwrap_or(0)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 4 {
        eprintln!("usage: loadtest <game> <mod> <level> <clients> [--seconds 60] [--warmup 15] [--bots 0] [--threads 8] [--vehicles 50] [--seed 1] [--mode conquest] [--json out.jsonl]");
        std::process::exit(2);
    }
    let opt = |n: &str| a.iter().position(|x| x == n).and_then(|i| a.get(i + 1)).cloned();
    let num = |n: &str, d: u64| opt(n).and_then(|v| v.parse().ok()).unwrap_or(d);
    let (clients, seconds, warmup, bots, threads, vehicles, seed) = (a[3].parse::<usize>().expect("clients"), num("--seconds", 60), num("--warmup", 15), num("--bots", 0) as usize, num("--threads", 8).max(1) as usize, num("--vehicles", 50), num("--seed", 1));
    let mode = opt("--mode").unwrap_or_else(|| "conquest".into());
    let world: &'static World = Box::leak(Box::new(World::load(a[0].as_ref(), &a[1], &a[2], &mode).expect("level")));
    let mut sim = round(world, bots, false);
    bytefield_server::hosted(&mut sim, &mode, bots, 0.0, Some(0));
    let mut server = Server::new(sim);
    let mut udp = UdpServer::bind(("127.0.0.1", 0), &a[1], &a[2], &mode).expect("bind");
    udp.most_clients = clients.max(1);
    server.sizes = Some(Default::default());
    let lose = num("--loss", 0) as u32;
    let addr = udp.local_addr();

    let run = Arc::new(AtomicBool::new(true));
    let measuring = Arc::new(AtomicBool::new(false));
    let now_step = Arc::new(AtomicU64::new(0));
    let joined = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for t in 0..threads.min(clients.max(1)) {
        let (n, first) = (clients / threads.min(clients.max(1)) + usize::from(t < clients % threads.min(clients.max(1))), t);
        let (run, measuring, now_step, joined) = (run.clone(), measuring.clone(), now_step.clone(), joined.clone());
        workers.push(std::thread::spawn(move || {
            let mut r = Rng(seed ^ (first as u64 + 1).wrapping_mul(0x2545_F491_4F6C_DD1D));
            let mut c: Vec<(UdpClient, Vec<(usize, f32)>, u32, ClientReport, u64)> = Vec::with_capacity(n);
            for i in 0..n {
                let hello = Hello { team: 1 + (i as u32 + first as u32) % 2, kit: (i % 5) as u32, name: format!("load{first}-{i}") };
                match UdpClient::connect(addr, hello, Duration::from_secs(30)) {
                    Ok(mut u) => {
                        u.mirror.spawn = Some((None, (i % 5) as u8));
                        u.lose = lose;
                        joined.fetch_add(1, Relaxed);
                        c.push((u, Vec::new(), 0, ClientReport::default(), 0));
                    }
                    Err(e) => eprintln!("client {first}/{i}: {e}"),
                }
            }
            let start = Instant::now();
            let mut ticks = 0u64;
            while run.load(Relaxed) {
                let on = measuring.load(Relaxed);
                let server_step = now_step.load(Relaxed);
                for (u, axes, until, rep, last) in &mut c {
                    let before = u.heard;
                    let got = u.poll() as u64;
                    if *until == 0 {
                        let (x, h) = actions(&mut r);
                        (*axes, *until) = (x, h);
                    }
                    *until -= 1;
                    let step = u.mirror.next_step();
                    let latest = u.mirror.latest().map_or(0, |s| s.step);
                    u.send(step, axes.clone());
                    if on {
                        rep.snapshots += got;
                        rep.bytes += u.heard.bytes - before.bytes;
                        rep.biggest = rep.biggest.max(u.heard.biggest);
                        let lag = server_step.saturating_sub(latest);
                        rep.behind += lag;
                        rep.behind_worst = rep.behind_worst.max(lag);
                        rep.samples += 1;
                        if *last != 0 && latest > *last + 1 {
                            rep.holes += latest - *last - 1;
                        }
                        *last = latest;
                    }
                }
                ticks += 1;
                std::thread::sleep((start + Duration::from_secs_f64(ticks as f64 * TICK)).saturating_duration_since(Instant::now()));
            }
            c.into_iter().map(|x| x.3).collect::<Vec<_>>()
        }));
    }

    let start = Instant::now();
    let total = warmup + seconds;
    let mut done = 0u64;
    let (mut ticks, mut polls): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    let (mut late, mut caught_up, mut slowest) = (0u64, 0u64, (0.0f64, 0.0f64));
    let mut rv = Rng(seed ^ 0xABCD);
    let mut base = (udp.sent, udp.heard, udp_rcvbuf_errors());
    let mut sizes0 = bytefield_server::Sizes::default();
    let mut measured_from = None;
    let mut seated = (0usize, 0usize);
    let every = num("--every", 0);
    let mut window_from = 0usize;
    while start.elapsed().as_secs() < total {
        let due = (start.elapsed().as_secs_f64() / TICK) as u64;
        if due <= done {
            let t0 = Instant::now();
            udp.poll(&mut server);
            if measured_from.is_some() {
                polls.push(t0.elapsed().as_secs_f64() * 1e3);
            }
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        if measured_from.is_none() && start.elapsed().as_secs() >= warmup {
            measured_from = Some(Instant::now());
            base = (udp.sent, udp.heard, udp_rcvbuf_errors());
            sizes0 = server.sizes.unwrap();
            measuring.store(true, Relaxed);
        }
        if due - done > 1 && measured_from.is_some() {
            late += 1;
            caught_up += due - done - 1;
        }
        for _ in 0..(due - done).min(5) {
            let t0 = Instant::now();
            udp.poll(&mut server);
            let t1 = Instant::now();
            udp.tick(&mut server);
            let t2 = Instant::now();
            now_step.store(server.sim.clock.steps, Relaxed);
            if measured_from.is_some() {
                polls.push((t1 - t0).as_secs_f64() * 1e3);
                ticks.push((t2 - t1).as_secs_f64() * 1e3);
                if (t2 - t1).as_secs_f64() * 1e3 > slowest.0 {
                    slowest = ((t2 - t1).as_secs_f64() * 1e3, server.sim.clock.time as f64);
                }
            }
        }
        done = due;
        // `--every S`: a line each S seconds measured, for a soak: the step in that window (mean, p95,
        // max), this process's memory and how many objects the world holds (a leak is a column that keeps rising)
        if every > 0 && measured_from.is_some() && done % (every * 30) == 0 && ticks.len() > window_from {
            let mut w = ticks[window_from..].to_vec();
            w.sort_by(f64::total_cmp);
            let rss = std::fs::read_to_string("/proc/self/status").ok().and_then(|t| t.lines().find(|l| l.starts_with("VmRSS")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())).unwrap_or(0) / 1024;
            let s = &server.sim;
            println!(
                "window {:.0} s: step mean {:.2} p95 {:.2} max {:.2} ms; rss {rss} MB; vehicles {} (removed {}, free {}, wrecks {}) projectiles {} bodies {} players {} alive {}; round time {:.0} s",
                measured_from.unwrap().elapsed().as_secs_f64(),
                w.iter().sum::<f64>() / w.len() as f64,
                w[w.len() * 95 / 100],
                w[w.len() - 1],
                s.vehicles.len(),
                // removed; of them past `REUSE_AFTER`, a place `spawn_vehicle` may take; destroyed and still in the world
                s.vehicles.iter().filter(|v| v.removed).count(),
                s.vehicles.iter().filter(|v| s.past_reuse(v)).count(),
                s.vehicles.iter().filter(|v| !v.removed && v.armor.as_ref().is_some_and(|a| a.destroyed)).count(),
                s.projectiles.len(),
                s.bodies.len(),
                s.players.len(),
                s.players.iter().filter(|p| p.alive).count(),
                s.clock.time
            );
            // who names the places past `REUSE_AFTER` that `spawn_vehicle` cannot take
            let named = s.named_places();
            let mut held = std::collections::BTreeMap::new();
            for (k, _) in s.vehicles.iter().enumerate().filter(|(_, v)| s.past_reuse(v)) {
                *held.entry(named[k].unwrap_or("nobody")).or_insert(0) += 1;
            }
            println!("  free places held by: {held:?}");
            // what the spent projectiles and `Sim`'s logs weigh (inline sizes; the boxes of a rigid one roughly)
            {
                use std::mem::size_of_val;
                let dead: Vec<_> = s.projectiles.iter().filter(|q| !q.alive()).collect();
                let vb = std::mem::size_of::<bytefield_sim::mover::Vehicle>();
                let boxed = |q: &bytefield_sim::projectile::Projectile| q.rigid.as_ref().map_or(0, |_| vb) + q.parts.as_ref().map_or(0, |_| vb);
                let kb = |n: usize| n / 1024;
                println!(
                    "  projectiles: dead {} of {}, ~{} KB inline + {} KB impacts + {} KB boxed (rigid {}, parts {})",
                    dead.len(),
                    s.projectiles.len(),
                    kb(s.projectiles.capacity() * std::mem::size_of::<bytefield_sim::projectile::Projectile>()),
                    kb(s.projectiles.iter().map(|q| q.impacts.capacity() * std::mem::size_of::<bytefield_sim::projectile::Impact>()).sum()),
                    kb(dead.iter().map(|q| boxed(q)).sum()),
                    dead.iter().filter(|q| q.rigid.is_some()).count(),
                    dead.iter().filter(|q| q.parts.is_some()).count()
                );
                let logs = [
                    ("damage", s.journal.damage_log.len(), s.journal.damage_log.capacity() * s.journal.damage_log.first().map_or(0, size_of_val)),
                    ("fired", s.journal.fired.len(), s.journal.fired.capacity() * s.journal.fired.first().map_or(0, size_of_val)),
                    ("hit", s.journal.hit_log.len(), s.journal.hit_log.capacity() * s.journal.hit_log.first().map_or(0, size_of_val)),
                    ("impacts", s.journal.impacts.len(), s.journal.impacts.capacity() * s.journal.impacts.first().map_or(0, size_of_val)),
                    ("kit", s.journal.kit_log.len(), s.journal.kit_log.capacity() * s.journal.kit_log.first().map_or(0, size_of_val)),
                    ("player", s.journal.player_log.len(), s.journal.player_log.capacity() * s.journal.player_log.first().map_or(0, size_of_val)),
                    ("radio", s.journal.radio_log.len(), s.journal.radio_log.capacity() * s.journal.radio_log.first().map_or(0, size_of_val)),
                    ("supply", s.journal.supply_log.len(), s.journal.supply_log.capacity() * s.journal.supply_log.first().map_or(0, size_of_val)),
                ];
                println!("  logs (entries, KB inline): {}", logs.iter().map(|(n, l, b)| format!("{n} {l} {}", kb(*b))).collect::<Vec<_>>().join(", "));
            }
            // a live vehicle's seat that lists a removed soldier: which, and the soldier's state
            for (i, v) in s.vehicles.iter().enumerate().filter(|(_, v)| !v.removed) {
                for (j, k) in v.entry.occupants.iter().enumerate().filter_map(|(j, o)| o.map(|k| (j, k))) {
                    let o = &s.vehicles[k];
                    if o.removed {
                        let a = o.armor.as_ref();
                        println!(
                            "  seat {j} of {i} {} (destroyed {:?}) keeps removed {k} {}: inside {:?} position {} driver {:?} hp {:?} destroyed {:?} gone {:.0} s ago; player {:?}",
                            v.template,
                            v.armor.as_ref().map(|a| a.destroyed),
                            o.template,
                            o.entry.inside,
                            o.entry.position,
                            v.entry.driver,
                            a.map(|a| a.hp),
                            a.map(|a| a.destroyed),
                            o.gone_since.map_or(-1.0, |t| s.clock.time - t),
                            s.players.iter().position(|p| p.soldier == Some(k))
                        );
                    }
                }
            }
            window_from = ticks.len();
        }
        if vehicles > 0 && done % 150 == 0 {
            let s = &mut server.sim;
            let players: Vec<usize> = (0..s.players.len()).collect();
            let mut alive = 0;
            let mut inside = 0;
            for k in players {
                let Some(i) = s.players[k].soldier.filter(|_| s.players[k].alive) else { continue };
                alive += 1;
                if s.vehicles[i].entry.inside.is_some() {
                    inside += 1;
                    continue;
                }
                if (rv.next() % 100) >= vehicles {
                    continue;
                }
                let free: Vec<usize> = s.vehicles.iter().enumerate().filter(|(n, v)| *n != i && v.in_world() && v.movement.is_none() && !v.armor.as_ref().is_some_and(|a| a.destroyed) && v.entry.occupants.iter().any(|o| o.is_none())).map(|(n, _)| n).collect();
                let Some(&v) = free.get((rv.next() % free.len().max(1) as u64) as usize) else { continue };
                let seat = s.vehicles[v].entry.occupants.iter().position(|o| o.is_none()).unwrap();
                s.enter(i, v, seat);
            }
            seated = (inside, alive);
        }
    }
    run.store(false, Relaxed);
    let reports: Vec<ClientReport> = workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect();
    let secs = measured_from.map_or(1.0, |t| t.elapsed().as_secs_f64()).max(0.001);
    let (sent, heard) = (udp.sent, udp.heard);
    let (dg, by) = (sent.datagrams - base.0.datagrams, sent.bytes - base.0.bytes);
    let (dg_in, by_in) = (heard.datagrams - base.1.datagrams, heard.bytes - base.1.bytes);
    let drops = udp_rcvbuf_errors().saturating_sub(base.2);
    ticks.sort_by(f64::total_cmp);
    polls.sort_by(f64::total_cmp);
    let over: usize = ticks.iter().filter(|t| **t > TICK * 1e3).count();
    let mean = ticks.iter().sum::<f64>() / ticks.len().max(1) as f64;
    let n = reports.len().max(1) as f64;
    let per = |f: &dyn Fn(&ClientReport) -> f64| reports.iter().map(f).sum::<f64>() / n;
    let snaps_s = per(&|r| r.snapshots as f64) / secs;
    let kbps = per(&|r| r.bytes as f64) * 8.0 / secs / 1000.0;
    let mut each: Vec<f64> = reports.iter().map(|r| r.bytes as f64 * 8.0 / secs / 1000.0).collect();
    each.sort_by(f64::total_cmp);
    let behind = reports.iter().map(|r| r.behind as f64).sum::<f64>() / reports.iter().map(|r| r.samples as f64).sum::<f64>().max(1.0);
    let worst = reports.iter().map(|r| r.behind_worst).max().unwrap_or(0);
    let holes = per(&|r| r.holes as f64) / secs;
    let biggest = reports.iter().map(|r| r.biggest).max().unwrap_or(0);
    let peers = udp.clients();
    println!("{} {} ({mode}) clients {}/{clients} on the server at the end, bots {bots}, players alive {} ({} in vehicles at the last count), {:.0} s measured", a[1], a[2], peers, seated.1, seated.0, secs);
    println!("server step  mean {mean:.2} ms, p50 {:.2}, p95 {:.2}, p99 {:.2}, max {:.2} (at {:.0} s of round); over 33.3 ms: {over} of {}; steps late: {late}, caught up {caught_up}", percentile(&ticks, 0.5), percentile(&ticks, 0.95), percentile(&ticks, 0.99), slowest.0, slowest.1, ticks.len());
    println!("server poll  p50 {:.3} ms, p99 {:.3}", percentile(&polls, 0.5), percentile(&polls, 0.99));
    println!("server out   {:.1} datagrams/s, {:.0} kbit/s total; in {:.1}/s, {:.0} kbit/s; biggest datagram {} B, over the 1472 B MTU: {}", dg as f64 / secs, by as f64 * 8.0 / secs / 1000.0, dg_in as f64 / secs, by_in as f64 * 8.0 / secs / 1000.0, sent.biggest, sent.over_mtu - base.0.over_mtu);
    println!("per client   {snaps_s:.1} snapshots/s, {kbps:.0} kbit/s down (min {:.0}, median {:.0}, max {:.0}), biggest {biggest} B; {behind:.2} steps behind on average (worst {worst}); holes {holes:.2} steps/s", percentile(&each, 0.0), percentile(&each, 0.5), percentile(&each, 1.0));
    let z = server.sizes.unwrap();
    let pk = (z.packets - sizes0.packets).max(1) as f64;
    let part = |f: fn(&bytefield_server::Sizes) -> u64| (f(&z) - f(&sizes0)) as f64 / pk;
    let parts = format!("total {:.0}, exact {:.0}, own {:.0}, changed state {:.0}, new {:.0}, moved {:.0}, actions {:.0}, players {:.0}, stale {:.0}, talk {:.0}", part(|z| z.total), part(|z| z.exact), part(|z| z.own), part(|z| z.state), part(|z| z.new), part(|z| z.moved), part(|z| z.actions), part(|z| z.players), part(|z| z.stale), part(|z| z.talk));
    println!("per packet   B: {parts}; of the objects: pos {:.0}, rot {:.0}, vel {:.0}, angles {:.0}; {} objects", part(|z| z.pos), part(|z| z.rot), part(|z| z.vel), part(|z| z.angles), z.objects);
    println!("joining      {} first packets of a client in all, the biggest {} B; biggest datagram of the whole run {} B", z.first, z.first_biggest, sent.biggest);
    println!("kernel       UDP receive-buffer errors while measuring: {drops}");
    if let Some(path) = opt("--json") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).expect("json");
        writeln!(f, "{{\"mod\":\"{}\",\"level\":\"{}\",\"mode\":\"{mode}\",\"clients\":{clients},\"connected\":{peers},\"bots\":{bots},\"vehicles\":{vehicles},\"seated\":[{},{}],\"secs\":{secs:.1},\"tick_mean\":{mean:.3},\"tick_p50\":{:.3},\"tick_p95\":{:.3},\"tick_p99\":{:.3},\"tick_max\":{:.3},\"over\":{over},\"ticks\":{},\"late\":{late},\"caught_up\":{caught_up},\"out_kbps\":{:.0},\"in_kbps\":{:.0},\"biggest\":{},\"over_mtu\":{},\"client_snaps\":{snaps_s:.2},\"client_kbps\":{kbps:.0},\"client_kbps_max\":{:.0},\"behind\":{behind:.3},\"behind_worst\":{worst},\"holes\":{holes:.3},\"drops\":{drops},\"first_biggest\":{},\"parts\":\"{parts}\"}}", a[1], a[2], seated.0, seated.1, percentile(&ticks, 0.5), percentile(&ticks, 0.95), percentile(&ticks, 0.99), slowest.0, ticks.len(), by as f64 * 8.0 / secs / 1000.0, by_in as f64 * 8.0 / secs / 1000.0, sent.biggest, sent.over_mtu - base.0.over_mtu, percentile(&each, 1.0), z.first_biggest).unwrap();
    }
}
