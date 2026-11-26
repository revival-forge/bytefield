//! A person who presses anything, in every level: the bots never walk a person's path (the seat keys, kits,
//! spawn screen, leaving a plane in the air), so these are `--monkeys` of them with seeded random actions, through the
//! whole server ([`Server::receive`], `tick`, each with its `Mirror`; the first with a predicted copy, as a client) next to
//! `--bots` bots. Every second `bytefield_sim::check::scan`; every 30 s a hash of the world, to compare two runs of a
//! seed (a difference is a determinism bug). `--hostile`: also NaN, huge values, axes out of
//! range and garbage packets, what anyone can send to a server.
//!   cargo run --release -p bytefield-server --example monkey -- data/game bf1942 El_Alamein 5 --mode conquest --monkeys 8 --bots 8 --seed 1
//! Prints `odd <kind> | <template> | <count> | <first second> | <place>`, `slow`, `hash <s> <hex>`, `stats …`.
use std::collections::BTreeMap;
use std::time::Instant;

use bytefield_server::{ClientPacket, Mirror, Prediction, Server, round};
use bytefield_sim::soldier::axis;
use bytefield_sim::{Sim, World};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 2001) as f32 / 1000.0 - 1.0
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

struct Monkey {
    client: usize,
    player: usize,
    mirror: Mirror,
    axes: Vec<(usize, f32)>,
    until: u64,
    spawn: Option<(Option<i64>, u8)>,
    hold: bool,
    dead_since: Option<f64>,
}

#[derive(Default)]
struct Stats {
    boarded: u32,
    exited: u32,
    suicides: u32,
    kits: u32,
    teams: u32,
    garbage: u32,
    /// the predicted copy against the server (m): the worst while alive and on foot or in the same vehicle, and how many
    /// seconds it stayed over 5 m
    worst: f32,
    off: u32,
}

/// What the monkey does next: mostly what a person does, sometimes any key at all.
fn choose(m: &mut Monkey, s: &mut Sim, r: &mut Rng, hostile: bool, st: &mut Stats) -> u64 {
    let k = m.player;
    let soldier = s.players[k].soldier.filter(|_| s.players[k].alive);
    let mut axes = Vec::new();
    let mut hold = 30 + r.below(150) as u64;
    match r.below(100) {
        0..=34 => {
            axes.push((axis::THROTTLE, if r.chance(80) { 1.0 } else { -1.0 }));
            axes.push((axis::YAW, r.unit() * 0.5));
            axes.push((axis::MOUSE_X, r.unit() * 0.3));
            axes.push((axis::MOUSE_Y, r.unit() * 0.2));
            if r.chance(40) {
                axes.push((axis::FIRE, 1.0));
            }
        }
        35..=49 => {
            axes.push((axis::FIRE, 1.0));
            axes.push((axis::MOUSE_X, r.unit()));
            axes.push((axis::MOUSE_Y, r.unit()));
            if r.chance(25) {
                axes.push((axis::ALT_FIRE, 1.0));
            }
        }
        50..=59 => {
            let key = [axis::RELOAD, axis::ACTION, axis::WALK, axis::LIE, axis::CROUCH, axis::WEAPON1, axis::WEAPON1 + 1, axis::WEAPON1 + 2, axis::WEAPON1 + 3, axis::WEAPON1 + 4, axis::WEAPON1 + 5, axis::WEAPON1 + 6][r.below(12)];
            axes.push((key, 1.0));
            hold = 2 + r.below(10) as u64;
        }
        60..=69 => {
            // any key
            for _ in 0..1 + r.below(4) {
                axes.push((r.below(56), if r.chance(50) { 1.0 } else { r.unit() }));
            }
            hold = 2 + r.below(40) as u64;
        }
        70..=79 => {
            // the key of getting in and out, and of changing seats
            axes.push((bytefield_sim::entry::USE, 1.0));
            if r.chance(30) {
                axes.push((axis::WEAPON1 + r.below(8), 1.0));
            }
            hold = 2;
            if let Some(i) = soldier
                && s.vehicles[i].entry.inside.is_some()
            {
                st.exited += 1;
            }
        }
        80..=91 => {
            // into a vehicle: the nearest free, or any, in any free seat (as a script's `seek`: walking there is the bots' path)
            if let Some(i) = soldier.filter(|&i| s.vehicles[i].entry.inside.is_none() && s.vehicles[i].in_world()) {
                let free: Vec<usize> = s.vehicles.iter().enumerate().filter(|(n, v)| *n != i && v.in_world() && v.soldier().is_none() && !v.armor.as_ref().is_some_and(|a| a.destroyed) && v.entry.occupants.iter().any(|o| o.is_none())).map(|(n, _)| n).collect();
                if !free.is_empty() {
                    let at = s.vehicles[i].craft.body.pos;
                    let d = |n: usize| (0..3).map(|j| (s.vehicles[n].craft.body.pos[j] - at[j]).powi(2)).sum::<f32>();
                    let v = if r.chance(50) { free.iter().copied().min_by(|&a, &b| d(a).total_cmp(&d(b))).unwrap() } else { free[r.below(free.len())] };
                    let seats: Vec<usize> = s.vehicles[v].entry.occupants.iter().enumerate().filter(|(_, o)| o.is_none()).map(|(n, _)| n).collect();
                    s.enter(i, v, seats[r.below(seats.len())]);
                    st.boarded += 1;
                }
            }
        }
        92..=93 => {
            s.suicide(k);
            st.suicides += 1;
        }
        94..=97 => {
            let groups = s.spawn_groups(k);
            let g = if groups.is_empty() || r.chance(20) { None } else { Some(groups[r.below(groups.len())]) };
            m.spawn = Some((g, r.below(7) as u8));
            st.kits += 1;
            m.hold = r.chance(5);
        }
        98 => {
            s.set_team(k, 1 + r.below(2) as u32);
            st.teams += 1;
        }
        _ => {}
    }
    if hostile && r.chance(10) {
        let v = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30, 1e-40][r.below(6)];
        axes.push((if r.chance(50) { r.below(64) } else { r.below(100_000) }, v));
    }
    m.axes = axes;
    hold
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opt = |n: &str| a.iter().position(|x| x == n).and_then(|i| a.get(i + 1)).cloned();
    let num = |n: &str, d: u64| opt(n).and_then(|v| v.parse().ok()).unwrap_or(d);
    let minutes: f64 = a.get(4).and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let mode = opt("--mode").unwrap_or_else(|| "conquest".into());
    let hostile = a.iter().any(|x| x == "--hostile");
    let seed = num("--seed", 1);
    let w: &'static World = Box::leak(Box::new(World::load(std::path::Path::new(&a[1]), &a[2], &a[3], &mode).expect("world")));
    let mut sim = round(w, num("--bots", 8) as usize, true);
    sim.rng = bytefield_sim::players::Rng(0x9E37_79B9_7F4A_7C15 ^ seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
    let mut server = Server::new(sim);
    let mut r = Rng(seed);
    let mut monkeys: Vec<Monkey> = (0..num("--monkeys", 8) as usize)
        .map(|i| {
            let (client, player) = server.join(0, i % 5);
            Monkey { client, player, mirror: Mirror::default(), axes: Vec::new(), until: 0, spawn: None, hold: false, dead_since: None }
        })
        .collect();
    let mut copy = Sim::new(w, 1.0 / 30.0);
    copy.round.playing = true;
    copy.start_conquest();
    let mut prediction = Prediction::default();
    let mut st = Stats::default();
    let mut odd: BTreeMap<(&'static str, String), (u32, f64, [i64; 3])> = BTreeMap::new();
    let mut slowest = (0.0f64, 0.0f64);
    let steps = (minutes * 60.0 * 30.0) as u64;
    let t = Instant::now();
    for now in 1..=steps {
        for m in &mut monkeys {
            if now >= m.until {
                m.until = now + choose(m, &mut server.sim, &mut r, hostile, &mut st);
            }
            let step = m.mirror.next_step();
            let bytes = m.mirror.send(step, m.axes.clone());
            let mut p = ClientPacket::decode(&bytes).expect("our own packet");
            p.spawn = m.spawn;
            p.hold = m.hold;
            server.receive(m.client, &p.encode());
            if hostile && r.chance(1) {
                let garbage: Vec<u8> = (0..r.below(300)).map(|_| r.next() as u8).collect();
                server.receive(m.client, &garbage);
                st.garbage += 1;
            }
        }
        let t0 = Instant::now();
        let out = server.tick();
        let dt = t0.elapsed().as_secs_f64();
        if dt > slowest.0 {
            slowest = (dt, server.sim.clock.time);
        }
        for (client, packet) in out {
            if let Some(m) = monkeys.iter_mut().find(|m| m.client == client) {
                m.mirror.receive(&packet);
            }
        }
        if let Some(m) = monkeys.first() {
            prediction.follow(&mut copy, &m.mirror, m.player);
            let at = |s: &Sim| s.players.get(m.player).and_then(|p| p.soldier.filter(|_| p.alive)).map(|i| (s.vehicles[i].craft.body.pos, s.vehicles[i].entry.inside));
            if now % 30 == 0
                && let (Some((a, ia)), Some((b, ib))) = (at(&copy), at(&server.sim))
                && ia == ib
            {
                let e = (0..3).map(|j| (a[j] - b[j]).powi(2)).sum::<f32>().sqrt();
                // (it runs ahead of the server: what its own speed takes in that time is no error)
                let u = server.sim.players[m.player].soldier.map(|i| server.sim.vehicles[server.sim.vehicles[i].entry.inside.unwrap_or(i)].craft.body.vel).unwrap_or_default();
                let e = (e - (0..3).map(|j| u[j] * u[j]).sum::<f32>().sqrt() * 0.2).max(0.0);
                st.worst = st.worst.max(e);
                st.off += (e > 5.0) as u32;
            }
        }
        if now % 30 == 0 {
            let time = server.sim.clock.time;
            for (kind, template, at) in bytefield_sim::check::scan(&server.sim) {
                odd.entry((kind, template)).or_insert((0, time, at)).0 += 1;
            }
            let running = server.sim.modes.conquest.as_ref().is_some_and(|c| c.winner.is_none());
            for m in &mut monkeys {
                let p = &server.sim.players[m.player];
                // (still on its timer it is not stuck: a suicide's penalty can reach 100 s, XPack2/berlin coop)
                if p.alive || p.hold || m.hold || p.kicked || !running || p.spawn_time > 0.0 || server.sim.spawn_groups(m.player).is_empty() {
                    m.dead_since = None;
                } else if time - *m.dead_since.get_or_insert(time) > 90.0 {
                    odd.entry(("dead for 90 s with a place to spawn", format!("{} team {} group {:?} of {:?} spawn_time {:.0}", p.template, p.team, p.group, server.sim.spawn_groups(m.player), p.spawn_time))).or_insert((0, time, [0; 3])).0 += 1;
                }
            }
        }
        if now % 900 == 0 {
            let bytes = server.latest().expect("a snapshot").delta(None).encode();
            let h = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
            println!("hash {} {h:016x}", now / 30);
        }
    }
    for ((kind, template), (n, first, at)) in &odd {
        println!("odd {kind} | {template} | {n} | {first:.0} | {at:?}");
    }
    println!("slow {:.3} at {:.0}", slowest.0, slowest.1);
    let (kills, deaths) = server.sim.players.iter().fold((0, 0), |(k, d), p| (k + p.kills, d + p.deaths));
    println!("stats secs {:.1} boarded {} exited {} suicides {} kits {} teams {} garbage {} kills {kills} deaths {deaths} predicted-worst {:.2} predicted-off {}", t.elapsed().as_secs_f64(), st.boarded, st.exited, st.suicides, st.kits, st.teams, st.garbage, st.worst, st.off);
}
