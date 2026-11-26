//! N clients against a server in one process, step by step: what each one knows has to be worth the same in all.
//! Each client runs with its own delay, so they are compared by the step of what they hold, against the server's state at that step.
//! Shared by `tests/memory.rs` (short) and `examples/coherence.rs` (the night's sweep, with a report).
//! The five invariants, by the number in [`Report::fails`]:
//! 1 `odd == 0` from `follow`; 2 an object not `stale` is the server's at that step, in the mirror and in the copy; 3 a `stale` one is off by no
//! more than its age explains; 4 what is discrete agrees within `x_limit` steps; 5 at the end, without loss, all of it agrees;
//! 6 at the end, each player died as many times in a copy as on the server since it joined.
#![allow(dead_code)]
use std::collections::{BTreeMap, HashMap};

use bytefield_server::{Mirror, Server, round};
use bytefield_sim::replica::{ObjectState, Snapshot};
use bytefield_sim::soldier::axis;
use bytefield_sim::{Sim, World};

/// Packets in flight: each arrives `delay` ticks later (and up to `jitter` more, at random: they can pass each other),
/// unless lost.
pub struct Link {
    pub delay: u64,
    pub jitter: u64,
    pub loss: f32,
    pub seed: u64,
    pub flying: Vec<(u64, Vec<u8>)>,
}

impl Link {
    pub fn new(delay: u64, loss: f32, seed: u64) -> Self {
        Link { delay, jitter: 0, loss, seed, flying: Vec::new() }
    }
    pub fn send(&mut self, now: u64, p: Vec<u8>) {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        if ((self.seed >> 40) as f32 / (1u64 << 24) as f32) >= self.loss {
            let extra = if self.jitter > 0 { (self.seed >> 16) % (self.jitter + 1) } else { 0 };
            self.flying.push((now + self.delay + extra, p));
        }
    }
    pub fn arrived(&mut self, now: u64) -> Vec<Vec<u8>> {
        let (due, rest) = std::mem::take(&mut self.flying).into_iter().partition(|(t, _)| *t <= now);
        self.flying = rest;
        due.into_iter().map(|(_, p)| p).collect()
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub clients: usize,
    pub bots: usize,
    pub steps: u64,
    pub loss: f32,
    /// up to this many ticks more on each packet, at random (they arrive out of order)
    pub jitter: u64,
    pub seed: u64,
    /// steps without loss at the end, for invariant 5
    pub settle: u64,
    /// invariant 4: the most steps something discrete may differ
    pub x_limit: u64,
}

/// A broken invariant, the first of each kind: where, and what differed.
#[derive(Clone, Debug)]
pub struct Violation {
    pub invariant: usize,
    pub step: u64,
    pub client: usize,
    pub object: Option<usize>,
    pub diff: String,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub steps: u64,
    /// by invariant (index 1 to 6)
    pub fails: [u64; 7],
    pub first: Vec<Violation>,
    /// invariant 4: the longest something discrete differed (steps), and what
    pub x_max: u64,
    pub x_what: String,
    /// invariant 3: the worst error of a stale object (m), its age (steps), the bound it had, and the object
    pub stale_err: f32,
    pub stale_err_age: u64,
    pub stale_err_bound: f32,
    pub stale_err_what: String,
    pub stale_age_max: u64,
    pub bytes: u64,
    pub joined: u32,
    pub left: u32,
    pub deaths: u32,
    pub boarded: u32,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.fails.iter().all(|n| *n == 0)
    }
    fn fail(&mut self, invariant: usize, step: u64, client: usize, object: Option<usize>, diff: impl FnOnce() -> String) {
        self.fails[invariant] += 1;
        if !self.first.iter().any(|v| v.invariant == invariant) {
            self.first.push(Violation { invariant, step, client, object, diff: diff() });
        }
    }
}

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

struct Peer {
    id: usize,
    mirror: Mirror,
    copy: Sim<'static>,
    up: Link,
    down: Link,
    axes: Vec<(usize, f32)>,
    until: u64,
    /// by object: the step it last came whole, the speed it came with, the server's path then
    fresh: Vec<(u64, f32, f32)>,
    /// what discrete differs since which step
    since: HashMap<(u8, u32), u64>,
    /// the copy's step when its world was first whole (invariant 6: a death before it, the client never had the player to see die)
    joined_at: Option<u64>,
}

/// Each discrete thing of the server's `s` that `mine` has otherwise, as (kind, index).
fn discrete(s: &Snapshot, mine: &Snapshot, mut f: impl FnMut(u8, u32)) {
    for (k, o) in s.objects.iter().enumerate() {
        match mine.objects.get(k) {
            Some(m) if m.template == o.template && m.gone == o.gone && m.inside == o.inside && m.seat == o.seat && m.team == o.team && m.hp == o.hp => {}
            _ => f(0, k as u32),
        }
    }
    for (k, p) in s.players.iter().enumerate() {
        match mine.players.get(k) {
            Some(m) if m.team == p.team && m.ai == p.ai && m.kit == p.kit && m.alive == p.alive && m.soldier == p.soldier && m.score == p.score => {}
            _ => f(1, k as u32),
        }
    }
    if s.tickets != mine.tickets {
        f(2, 0);
    }
    for (k, p) in s.points.iter().enumerate() {
        if mine.points.get(k).is_none_or(|m| m.team != p.team) {
            f(3, k as u32);
        }
    }
    if s.groups != mine.groups {
        f(4, 0);
    }
    for t in &s.modes.items {
        if !mine.modes.items.iter().any(|m| m.at == t.at && m.template == t.template) {
            f(5, t.at);
        }
    }
    for t in &mine.modes.items {
        if !s.modes.items.iter().any(|m| m.at == t.at) {
            f(5, t.at);
        }
    }
}

fn describe(kind: u8, k: u32, s: &Snapshot, mine: &Snapshot) -> String {
    match kind {
        0 => format!("object {k}: server {:#?}\nclient {:#?}", s.objects.get(k as usize), mine.objects.get(k as usize)),
        1 => format!("player {k}: server {:#?}\nclient {:#?}", s.players.get(k as usize), mine.players.get(k as usize)),
        2 => format!("tickets: server {:?} client {:?}", s.tickets, mine.tickets),
        3 => format!("point {k}: server {:?} client {:?}", s.points.get(k as usize), mine.points.get(k as usize)),
        4 => format!("groups: server {:?} client {:?}", s.groups, mine.groups),
        _ => format!("item {k}: server {:?} client {:?}", s.modes.items.iter().find(|m| m.at == k), mine.modes.items.iter().find(|m| m.at == k)),
    }
}

/// Player `j`'s deaths in `sim` after step `from` and up to `to`.
fn died(sim: &bytefield_sim::Sim, j: usize, from: u64, to: u64) -> usize {
    sim.journal.player_log.iter().filter(|e| e.1 == j && e.0 > from && e.0 <= to && matches!(e.2, bytefield_sim::players::PlayerEvent::Died(_))).count()
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn history_pos(h: &BTreeMap<u64, (Snapshot, Vec<f32>)>, step: u64, o: usize) -> Option<([f32; 3], Option<u32>, bool)> {
    let x = h.get(&step)?.0.objects.get(o)?;
    Some((x.pos, x.inside, x.gone))
}

fn speed(o: &ObjectState) -> f32 {
    dist(o.vel, [0.0; 3])
}

/// Off by this much a copy still is the server's: what writing a rounded state and reading it back may leave.
const COPY_SLACK: f32 = 0.05;
/// What the bound of invariant 3 allows at age 0 (m) and per second of age besides what the object itself moved (m/s).
const STALE_BASE: f32 = 6.0;
const STALE_RATE: f32 = 5.0;

pub fn run(world: &'static World, c: &Config) -> Report {
    let mut r = Report::default();
    let mut rng = Rng(c.seed);
    let mut server = Server::new(round(world, c.bots, false));
    // the server's world by step (and each object's path so far, for invariant 3)
    let mut history: BTreeMap<u64, (Snapshot, Vec<f32>)> = BTreeMap::new();
    let mut path: Vec<f32> = Vec::new();
    let mut last_pos: Vec<[f32; 3]> = Vec::new();
    let mut peers: Vec<Option<Peer>> = (0..c.clients).map(|_| None).collect();
    let join_at: Vec<u64> = (0..c.clients).map(|k| if k < 2 { 0 } else { rng.next() % (c.steps / 3).max(1) }).collect();
    // one leaves and comes back
    let leave_at = c.steps / 2 + rng.next() % (c.steps / 8).max(1);
    let back_at = leave_at + 150;
    let leaver = c.clients - 1;
    let total = c.steps + c.settle;
    let new_peer = |server: &mut Server<'static>, k: usize, rng: &mut Rng, loss: f32| {
        let team = 1 + (k % 2) as u32;
        let kit = k % 5;
        let (id, _) = server.join(team, kit);
        let mut mirror = Mirror::default();
        mirror.spawn = Some((None, kit as u8));
        let copy = bytefield_server::copy_round(world);
        let mut up = Link::new(1 + rng.next() % 6, loss, rng.next());
        let mut down = Link::new(1 + rng.next() % 6, loss, rng.next());
        (up.jitter, down.jitter) = (c.jitter, c.jitter);
        let joined_at = None;
        Peer { id, mirror, copy, up, down, axes: Vec::new(), until: 0, fresh: Vec::new(), since: HashMap::new(), joined_at }
    };
    for now in 0..total {
        if now == c.steps {
            for p in peers.iter_mut().flatten() {
                p.up.loss = 0.0;
                p.down.loss = 0.0;
            }
        }
        for k in 0..c.clients {
            let due = (peers[k].is_none() && now == join_at[k]) || (k == leaver && now == back_at && now < c.steps);
            if due && peers[k].is_none() {
                let loss = if now < c.steps { c.loss } else { 0.0 };
                peers[k] = Some(new_peer(&mut server, k, &mut rng, loss));
                r.joined += 1;
            }
        }
        if now == leave_at && peers[leaver].is_some() && back_at < c.steps {
            let p = peers[leaver].take().unwrap();
            server.leave(p.id);
            r.left += 1;
        }
        for k in 0..c.clients {
            let Some(p) = peers[k].as_mut() else { continue };
            if now >= p.until && now < c.steps {
                p.axes = vec![(axis::THROTTLE, if rng.next() % 5 == 0 { -1.0 } else { 1.0 }), (axis::YAW, rng.unit() * 0.5), (axis::MOUSE_X, rng.unit() * 0.3), (axis::MOUSE_Y, rng.unit() * 0.1)];
                if rng.next() % 100 < 40 {
                    p.axes.push((axis::FIRE, 1.0));
                }
                p.until = now + 30 + rng.next() % 120;
            }
            let player = server.player(p.id);
            if now < c.steps {
                let alive = server.sim.players[player].soldier.filter(|_| server.sim.players[player].alive);
                if let Some(i) = alive {
                    if rng.next() % 900 == 0 {
                        server.sim.vehicles[i].armor.as_mut().unwrap().destroyed = true;
                        r.deaths += 1;
                    } else if now % 150 == 149 && server.sim.vehicles[i].entry.inside.is_none() && rng.next() % 100 < 40 {
                        let s = &mut server.sim;
                        let free: Vec<usize> = s.vehicles.iter().enumerate().filter(|(n, v)| *n != i && v.in_world() && v.soldier().is_none() && !v.armor.as_ref().is_some_and(|a| a.destroyed) && v.entry.occupants.iter().any(|o| o.is_none())).map(|(n, _)| n).collect();
                        if !free.is_empty() {
                            let v = free[(rng.next() % free.len() as u64) as usize];
                            let seat = s.vehicles[v].entry.occupants.iter().position(|o| o.is_none()).unwrap();
                            s.enter(i, v, seat);
                            r.boarded += 1;
                        }
                    }
                }
            }
            let step = server.sim.clock.steps + 1 + p.up.delay;
            let packet = p.mirror.send(step, p.axes.clone());
            p.up.send(now, packet);
            for packet in p.up.arrived(now) {
                server.receive(p.id, &packet);
            }
        }
        let out = server.tick();
        for (id, packet) in out {
            r.bytes += packet.len() as u64;
            if let Some(p) = peers.iter_mut().flatten().find(|p| p.id == id) {
                p.down.send(now, packet);
            }
        }
        let snap = server.latest().unwrap().clone();
        path.resize(snap.objects.len(), 0.0);
        last_pos.resize(snap.objects.len(), [f32::NAN; 3]);
        for (k, o) in snap.objects.iter().enumerate() {
            if !last_pos[k][0].is_nan() {
                path[k] += dist(last_pos[k], o.pos);
            }
            last_pos[k] = o.pos;
        }
        let step = snap.step;
        history.insert(step, (snap, path.clone()));
        while history.len() > 600 {
            history.pop_first();
        }
        for k in 0..c.clients {
            let Some(p) = peers[k].as_mut() else { continue };
            for packet in p.down.arrived(now) {
                p.mirror.receive(&packet);
            }
            let odd = bytefield_server::follow(&mut p.copy, &p.mirror);
            if odd != 0 {
                r.fail(1, step, k, None, || format!("odd = {odd}"));
            }
            let Some(mine) = p.mirror.latest() else { continue };
            if p.joined_at.is_none() && !mine.partial && p.copy.clock.steps > 0 {
                p.joined_at = Some(p.copy.clock.steps);
            }
            let Some((s, paths)) = history.get(&mine.step) else { continue };
            p.fresh.resize(s.objects.len(), (mine.step, 0.0, 0.0));
            // 2 and 3
            for (o, srv) in s.objects.iter().enumerate() {
                let Some(m) = mine.objects.get(o) else { continue };
                if mine.stale.contains(&(o as u32)) {
                    // (one inside a vehicle is where the copy's vehicle is: that object answers for it)
                    if srv.gone {
                        continue;
                    }
                    let (at, v0, path0) = p.fresh[o];
                    let age = mine.step.saturating_sub(at);
                    r.stale_age_max = r.stale_age_max.max(age);
                    let secs = age as f32 / 30.0;
                    let moved = paths.get(o).map_or(0.0, |x| x - path0).max(0.0);
                    let bound = STALE_BASE + moved + (v0 + STALE_RATE) * secs;
                    let Some(v) = p.copy.vehicles.get(o) else { continue };
                    let err = dist(v.craft.body.pos, srv.pos);
                    if err > r.stale_err {
                        (r.stale_err, r.stale_err_age, r.stale_err_bound) = (err, age, bound);
                        r.stale_err_what = format!("client {k}, object {o} ({}), step {}", srv.template, mine.step);
                    }
                    if err > bound {
                        let pos = v.craft.body.pos;
                        let (ci, cs, cg, cv) = (v.entry.inside, v.entry.position, v.removed || !v.in_world(), v.craft.body.vel);
                        let before = history_pos(&history, at, o);
                        r.fail(3, mine.step, k, Some(o), || format!("{}: off by {err} m, bound {bound}, age {age} steps; server at {:?} (inside {:?} seat {} gone {} vel {:?}), copy at {pos:?} (inside {ci:?} seat {cs} gone {cg} vel {cv:?}); server when it last came {before:?}, client's stale flag since step {at}", srv.template, srv.pos, srv.inside, srv.seat, srv.gone, srv.vel));
                    }
                } else {
                    p.fresh[o] = (mine.step, speed(m), paths.get(o).copied().unwrap_or(0.0));
                    if m != srv {
                        r.fail(2, mine.step, k, Some(o), || format!("mirror:\nserver {srv:#?}\nclient {m:#?}"));
                    } else if let Some(v) = p.copy.vehicles.get(o).filter(|_| !srv.gone && p.copy.clock.steps == mine.step) {
                        let err = dist(v.craft.body.pos, srv.pos);
                        if err > COPY_SLACK {
                            let pos = v.craft.body.pos;
                            r.fail(2, mine.step, k, Some(o), || format!("copy of {}: off by {err} m; server at {:?}, copy at {pos:?}", srv.template, srv.pos));
                        }
                    }
                }
            }
            // 4
            let mut now_differs = HashMap::new();
            discrete(s, mine, |kind, i| {
                now_differs.insert((kind, i), *p.since.get(&(kind, i)).unwrap_or(&mine.step));
            });
            for (&(kind, i), &from) in &now_differs {
                let x = mine.step - from;
                if x > r.x_max {
                    r.x_max = x;
                    r.x_what = format!("client {k}: {}", describe(kind, i, s, mine));
                }
                if x > c.x_limit {
                    r.fail(4, mine.step, k, (kind == 0).then_some(i as usize), || format!("{x} steps: {}", describe(kind, i, s, mine)));
                }
            }
            p.since = now_differs;
            // 5
            if now + 1 == total {
                if mine.partial {
                    r.fail(5, mine.step, k, None, || "still joining".to_string());
                }
                let mut n = Vec::new();
                discrete(s, mine, |kind, i| n.push((kind, i)));
                if let Some(&(kind, i)) = n.first() {
                    let count = n.len();
                    r.fail(5, mine.step, k, (kind == 0).then_some(i as usize), || format!("{count} differ, the first: {}", describe(kind, i, s, mine)));
                }
                // 6
                for j in 0..p.copy.players.len().min(server.sim.players.len()) {
                    // (the server's since it joined and up to the copy's step: what is still on its way is not missed)
                    let (theirs, ours) = (died(&server.sim, j, p.joined_at.unwrap_or(u64::MAX), p.copy.clock.steps), died(&p.copy, j, 0, u64::MAX));
                    if ours != theirs {
                        let sol = |sim: &bytefield_sim::Sim| {
                            let q = &sim.players[j];
                            let v = q.soldier.and_then(|i| sim.vehicles.get(i));
                            format!("alive {} soldier {:?} {:?} removed {:?} destroyed {:?} hp {:?}", q.alive, q.soldier, v.map(|v| &v.template), v.map(|v| v.removed), v.and_then(|v| v.armor.as_ref()).map(|a| a.destroyed), v.and_then(|v| v.armor.as_ref()).map(|a| a.hp))
                        };
                        let why: Vec<String> = p.copy.journal.player_log.iter().filter(|e| e.1 == j).filter_map(|e| match e.2 {
                            bytefield_sim::players::PlayerEvent::Died(v) => Some(format!("{}:{:?}", e.0, p.copy.journal.damage_log.iter().rev().find(|d| d.vehicle == v && d.status == Some(bytefield_sim::damage::Status::Destroyed)).map(|d| (d.step, d.source)))),
                            _ => None,
                        }).take(4).collect();
                        let at: Vec<u64> = server.sim.journal.player_log.iter().filter(|e| e.1 == j && matches!(e.2, bytefield_sim::players::PlayerEvent::Died(_))).map(|e| e.0).collect();
                        r.fail(6, mine.step, k, None, || format!("player {j} died {ours} times in the copy, {theirs} on the server (at {at:?}, whole at {:?}); copy: {}; server: {}; deaths (step: damage) {why:?}", p.joined_at, sol(&p.copy), sol(&server.sim)));
                    }
                }
            }
        }
    }
    r.steps = total;
    r
}
