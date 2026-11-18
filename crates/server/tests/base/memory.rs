//! A server and two clients in one process over a link that loses and delays packets: the
//! clients' copies end as the server's world, and their actions move their soldiers (N2).

use bytefield_server::{Mirror, Server};
use bytefield_sim::{Sim, World, soldier::axis};

fn world(level: &str) -> Option<&'static World> {
    bytefield_testdata::base().map(|game| &*Box::leak(Box::new(World::load(&game, "bf1942", level, "conquest").expect("level"))))
}

#[path = "common/coherence.rs"]
mod coherence;
use coherence::{Config, Link};

/// Four clients and bots, step by step: what each knows is the server's.
#[test]
fn what_n_clients_know_is_the_servers() {
    let Some(w) = world("Berlin") else { return };
    for (seed, loss) in [(1, 0.0), (2, 0.05), (3, 0.05)] {
        let r = coherence::run(w, &Config { clients: 4, bots: 4, steps: 450, loss, jitter: 0, seed, settle: 150, x_limit: 30 });
        assert!(r.ok(), "seed {seed}, loss {loss}: {:?} {:#?}", r.fails, r.first);
        assert!(r.joined >= 4, "joined {}", r.joined);
    }
}

#[test]
fn two_clients_over_a_lossy_link_end_with_the_servers_world() {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = Sim::new(w, 1.0 / 30.0);
    sim.round.playing = true;
    sim.start_conquest();
    let players = [sim.add_player(1, "GermanSoldier", false), sim.add_player(2, "USSoldier", false)];
    // (no spawn waves: it spawns again 3 s after it dies)
    sim.round.waves = None;
    let mut server = Server::new(sim);
    let ids = players.map(|p| server.connect(p));
    let mut mirrors = [Mirror::default(), Mirror::default()];
    // the first chooses the last of its team's spawn groups and kit 1 (the screen when dead),
    // and dies at 360: it spawns again there
    let chosen = *server.sim.spawn_groups(players[0]).last().unwrap();
    mirrors[0].spawn = Some((Some(chosen), 1));
    let mut up = [Link::new(3, 0.25, 1), Link::new(2, 0.25, 2)];
    let mut down = [Link::new(3, 0.25, 3), Link::new(4, 0.25, 4)];
    let mut start = [None; 2];
    let (lossy, total) = (400u64, 560u64);
    let mut bytes = 0usize;
    let mut again = None;
    let mut killed = None;
    let mut feed = [None; 2];
    for now in 0..total {
        let p0 = &server.sim.players[players[0]];
        if now == 360 {
            let i = p0.soldier.unwrap();
            // (killed by the other's soldier: the kill feed's «who»)
            let by = server.sim.players[players[1]].soldier;
            let a = server.sim.vehicles[i].armor.as_mut().unwrap();
            a.destroyed = true;
            a.killed_by = by;
            killed = Some((i, by.unwrap()));
        } else if now > 360 && again.is_none() && p0.alive {
            let i = p0.soldier.unwrap();
            again = Some(server.sim.vehicles[i].craft.body.pos);
        }
        if now == lossy {
            for l in up.iter_mut().chain(down.iter_mut()) {
                l.loss = 0.0;
            }
        }
        for k in 0..2 {
            // each client runs ahead by its link's delay: its action for the step that arrives then
            let step = server.sim.clock.steps + 1 + up[k].delay;
            let axes = vec![(axis::THROTTLE, 1.0), (axis::YAW, if k == 0 { 0.0 } else { 0.3 })];
            // radio picks while the link loses (F1 1 roger, F5 5 roger out loud, F3 2 infantry)
            for (t, who, f, d) in [(150, 0, 1, 1), (250, 0, 5, 5), (350, 1, 3, 2)] {
                if now == t && who == k {
                    mirrors[k].radio_pick(f, d);
                }
            }
            let p = mirrors[k].send(step, axes);
            up[k].send(now, p);
            for p in up[k].arrived(now) {
                server.receive(ids[k], &p);
            }
        }
        for (c, p) in server.tick() {
            bytes += p.len();
            down[c].send(now, p);
        }
        for k in 0..2 {
            for p in down[k].arrived(now) {
                mirrors[k].receive(&p);
            }
            // while it lies dead, the copy knows who killed it
            if let (Some((i, by)), Some(s)) = (killed, mirrors[k].latest())
                && s.players.get(players[0]).is_some_and(|p| !p.alive && p.soldier == Some(i as u32))
            {
                let mut copy = Sim::new(w, 1.0 / 30.0);
                s.write(&mut copy);
                feed[k] = Some(copy.vehicles[i].armor.as_ref().unwrap().killed_by == Some(by));
            }
            if start[k].is_none()
                && let Some(i) = server.sim.players[players[k]].soldier.filter(|_| server.sim.players[players[k]].alive)
            {
                start[k] = Some((i, server.sim.vehicles[i].craft.body.pos));
            }
        }
    }
    let last = server.latest().unwrap().step;
    for (k, m) in mirrors.iter().enumerate() {
        let s = m.latest().expect("a snapshot");
        assert!(last - s.step <= 4, "behind: {} of {last}", s.step);
        assert!(server.sent_at(ids[k], s.step) == Some(s), "the mirror of step {} differs", s.step);
    }
    for (k, &p) in players.iter().enumerate() {
        let (i, p0) = start[k].expect("spawned");
        let p1 = server.sim.vehicles[i].craft.body.pos;
        let d = ((p1[0] - p0[0]).powi(2) + (p1[2] - p0[2]).powi(2)).sqrt();
        assert!(d > 5.0, "player {p} moved {d} m");
    }
    // it spawned there and with that kit; the copies know the groups' teams
    let at = again.expect("spawned again");
    let c = server.sim.modes.conquest.as_ref().unwrap();
    assert!(c.groups[&chosen].spawns.iter().any(|sp| (sp.pos[0] - at[0]).abs() < 1.0 && (sp.pos[2] - at[2]).abs() < 1.0), "spawned at {at:?}");
    assert_eq!(server.sim.players[players[0]].kit, 1);
    for m in &mirrors {
        assert_eq!(m.latest().unwrap().groups, c.groups.iter().map(|(id, g)| (*id, g.team)).collect::<Vec<_>>());
    }
    assert_eq!(feed, [Some(true); 2], "the copies know who killed it");
    let said: Vec<_> = server.sim.journal.radio_log.iter().map(|e| (e.step, e.player, e.msg)).collect();
    use bytefield_sim::radio::{INFANTRY_SPOTTED, LOCAL_ROGER, ROGER};
    assert_eq!(said.iter().map(|e| (e.1, e.2)).collect::<Vec<_>>(), [(players[0], ROGER), (players[0], LOCAL_ROGER), (players[1], INFANTRY_SPOTTED)], "said once each");
    for m in &mirrors {
        assert_eq!(m.radio.iter().map(|e| (e.step, e.player, e.msg)).collect::<Vec<_>>(), said, "heard once each");
    }
    eprintln!("{} B per client per tick", bytes / 2 / total as usize);
}

#[test]
#[ignore]
fn what_a_snapshot_weighs() {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = Sim::new(w, 1.0 / 30.0);
    sim.round.playing = true;
    sim.start_conquest();
    sim.enable_bots();
    for i in 0..16 {
        let t = 1 + (i % 2) as u32;
        sim.add_player(t, if t == 1 { "GermanSoldier" } else { "USSoldier" }, true);
    }
    let mut prev = None;
    for _ in 0..600 {
        sim.step();
        let s = bytefield_sim::replica::Snapshot::of(&sim);
        if sim.clock.steps % 100 == 0 {
            let d = s.delta(prev.as_ref());
            let n = d.objects.len();
            let ang: usize = d.objects.iter().filter_map(|o| o.1.angles.as_ref()).map(|a| a.1.len()).sum();
            let names: usize = d.objects.iter().filter_map(|o| o.1.template.as_ref()).map(String::len).sum();
            let part = |f: &dyn Fn(&mut bytefield_sim::replica::Delta)| {
                let mut e = d.clone();
                f(&mut e);
                d.encode().len() - e.encode().len()
            };
            let (players, actions, objects) = (part(&|e| e.players.clear()), part(&|e| e.actions.clear()), part(&|e| e.objects.clear()));
            let field = |f: &dyn Fn(&mut bytefield_sim::replica::ObjectDelta)| part(&|e| e.objects.iter_mut().for_each(|o| f(&mut o.1)));
            let fields = [field(&|o| o.pos = None), field(&|o| o.rot = None), field(&|o| o.vel = None), field(&|o| o.rotvel = None), field(&|o| o.angles = None)];
            eprintln!("step {} objects {} changed {n} bytes {} (players {players}, actions {actions}, objects {objects}: pos/rot/vel/rotvel/angles {fields:?}) angles {ang} name bytes {names}", sim.clock.steps, s.objects.len(), d.encode().len());
        }
        prev = Some(s);
    }
}

/// The client's soldier predicted (N3): with 4 ticks each way, what the client shows of its
/// soldier is where the server has it once the client's latest action arrived there (the
/// copy alone shows it a round trip late).
#[test]
fn the_clients_soldier_runs_ahead_of_the_copy() {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = Sim::new(w, 1.0 / 30.0);
    sim.round.playing = true;
    sim.start_conquest();
    let me = sim.add_player(2, "USSoldier", false);
    let mut server = Server::new(sim);
    let id = server.connect(me);
    let mut copy = Sim::new(w, 1.0 / 30.0);
    copy.round.playing = true;
    copy.start_conquest();
    let (mut mirror, mut prediction) = (Mirror::default(), bytefield_server::Prediction::default());
    let (mut up, mut down) = (Link::new(4, 0.0, 1), Link::new(4, 0.0, 2));
    // by tick: the server's soldier, the shown one, the copy's
    let (mut server_at, mut shown, mut copied) = (Vec::new(), Vec::new(), Vec::new());
    let total = 400u64;
    for now in 0..total {
        // walking and turning, stopping now and then
        let walk = if (now / 60) % 3 == 2 { 0.0 } else { 1.0 };
        let axes = vec![(axis::THROTTLE, walk), (axis::YAW, if (now / 90) % 2 == 0 { 0.2 } else { -0.2 })];
        let p = mirror.send(mirror.next_step(), axes);
        up.send(now, p);
        for p in up.arrived(now) {
            server.receive(id, &p);
        }
        for (_, p) in server.tick() {
            down.send(now, p);
        }
        for p in down.arrived(now) {
            mirror.receive(&p);
        }
        prediction.follow(&mut copy, &mirror, me);
        let at = |s: &Sim| s.players.get(me).and_then(|p| p.soldier.filter(|_| p.alive)).map(|i| s.vehicles[i].craft.body.pos);
        server_at.push(at(&server.sim));
        shown.push(at(&copy));
        // the copy's own, as the snapshot left it
        let s = mirror.latest();
        copied.push(copy.players.get(me).and_then(|p| p.soldier.filter(|_| p.alive)).and_then(|i| Some(s?.objects.get(i)?.pos)));
    }
    let dist = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    // what the client shows at tick t is the server's at t + 4 (its action of t arrives then)
    let error = |xs: &[Option<[f32; 3]>]| {
        let e: Vec<f32> = (100..total as usize - 4).filter_map(|t| Some(dist(xs[t]?, server_at[t + 4]?))).collect();
        (e.iter().sum::<f32>() / e.len() as f32, e.iter().cloned().fold(0.0, f32::max))
    };
    let (shown_mean, shown_max) = error(&shown);
    let (copy_mean, copy_max) = error(&copied);
    eprintln!("predicted: mean {shown_mean:.3} m max {shown_max:.3}; the copy alone: mean {copy_mean:.3} max {copy_max:.3}; {} steps ahead", prediction.steps);
    assert!(prediction.steps >= 6, "{} steps predicted", prediction.steps);
    assert!(shown_mean < 0.05 && shown_max < 0.3, "the prediction is off");
    assert!(copy_mean > 10.0 * shown_mean.max(0.01));
}

/// The same in a vehicle: the client drives the nearest one with seats (put in it on the
/// server and in the copy); the vehicle shown is where the server has it 4 ticks later.
#[test]
fn the_clients_vehicle_runs_ahead_of_the_copy() {
    the_vehicle_ahead(0);
}

/// The same for who joins a round under way: the copy jumps, and what its steps would have built must come with the snapshots.
#[test]
fn the_vehicle_of_who_joins_late_runs_ahead_too() {
    the_vehicle_ahead(200);
}

fn the_vehicle_ahead(warm: u64) {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = Sim::new(w, 1.0 / 30.0);
    sim.round.playing = true;
    sim.start_conquest();
    let me = sim.add_player(2, "USSoldier", false);
    let mut server = Server::new(sim);
    for _ in 0..warm {
        server.tick();
    }
    let id = server.connect(me);
    let mut copy = Sim::new(w, 1.0 / 30.0);
    copy.round.playing = true;
    copy.start_conquest();
    let (mut mirror, mut prediction) = (Mirror::default(), bytefield_server::Prediction::default());
    let (mut up, mut down) = (Link::new(4, 0.0, 1), Link::new(4, 0.0, 2));
    let mut entered: Option<(u64, usize, usize)> = None;
    let mut copy_in = false;
    let (mut server_at, mut shown, mut copied) = (Vec::new(), Vec::new(), Vec::new());
    let total = 500u64;
    for now in 0..total {
        let s = &mut server.sim;
        if entered.is_none()
            && let Some(i) = s.players[me].soldier.filter(|_| s.players[me].alive)
        {
            let p = s.vehicles[i].craft.body.pos;
            let d = |k: usize| {
                let q = s.vehicles[k].craft.body.pos;
                (q[0] - p[0]).powi(2) + (q[2] - p[2]).powi(2)
            };
            let k = (0..s.vehicles.len()).filter(|&k| !s.vehicles[k].fixed && s.vehicles[k].in_world() && s.vehicles[k].entry.seats.len() > 1 && s.vehicles[k].movement.is_none()).min_by(|&a, &b| d(a).total_cmp(&d(b))).expect("a vehicle");
            s.enter(i, k, 0);
            eprintln!("in {} at step {}", s.vehicles[k].template, s.clock.steps);
            entered = Some((s.clock.steps, i, k));
        }
        let driving = entered.is_some_and(|e| s.clock.steps > e.0 + 30);
        let axes = vec![(axis::THROTTLE, if driving && (now / 80) % 3 != 2 { 1.0 } else { 0.0 }), (axis::YAW, if (now / 100) % 2 == 0 { 0.5 } else { -0.5 })];
        let p = mirror.send(mirror.next_step(), axes);
        up.send(now, p);
        for p in up.arrived(now) {
            server.receive(id, &p);
        }
        for (_, p) in server.tick() {
            down.send(now, p);
        }
        for p in down.arrived(now) {
            mirror.receive(&p);
        }
        prediction.follow(&mut copy, &mirror, me);
        if let Some((step, i, k)) = entered
            && !copy_in
            && copy.clock.steps >= step
        {
            prediction.restore(&mut copy);
            copy.enter(i, k, 0);
            copy_in = true;
            prediction.follow(&mut copy, &mirror, me);
        }
        let at = |s: &Sim| entered.map(|e| s.vehicles[e.2].craft.body.pos);
        server_at.push(at(&server.sim));
        shown.push(at(&copy).filter(|_| copy_in));
        copied.push(entered.filter(|_| copy_in).and_then(|e| Some(mirror.latest()?.objects.get(e.2)?.pos)));
    }
    let dist = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let from = (entered.unwrap().0 - warm) as usize + 60;
    let moved = dist(server_at[from].unwrap(), server_at[total as usize - 1].unwrap());
    let error = |xs: &[Option<[f32; 3]>]| {
        let e: Vec<f32> = (from..total as usize - 4).filter_map(|t| Some(dist(xs[t]?, server_at[t + 4]?))).collect();
        (e.iter().sum::<f32>() / e.len() as f32, e.iter().cloned().fold(0.0, f32::max))
    };
    let (shown_mean, shown_max) = error(&shown);
    let (copy_mean, copy_max) = error(&copied);
    eprintln!("moved {moved:.1} m; predicted: mean {shown_mean:.3} m max {shown_max:.3}; the copy alone: mean {copy_mean:.3} max {copy_max:.3}; {} steps ahead", prediction.steps);
    assert!(moved > 10.0, "the vehicle moved {moved} m");
    assert!(prediction.steps >= 6);
    assert!(shown_mean < 0.1 && shown_max < 0.5, "the prediction is off");
    assert!(copy_mean > 5.0 * shown_mean.max(0.01));
}

/// With 30 % of the client's packets lost the server repeats actions and the prediction
/// misses: the corrections are smoothed (the shown soldier jumps less from tick to tick
/// than the bare prediction), and it stays near the server.
#[test]
fn a_missed_prediction_is_smoothed() {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = Sim::new(w, 1.0 / 30.0);
    sim.round.playing = true;
    sim.start_conquest();
    let me = sim.add_player(2, "USSoldier", false);
    let mut server = Server::new(sim);
    let id = server.connect(me);
    let mut copy = Sim::new(w, 1.0 / 30.0);
    copy.round.playing = true;
    copy.start_conquest();
    let (mut mirror, mut prediction) = (Mirror::default(), bytefield_server::Prediction::default());
    let (mut up, mut down) = (Link::new(4, 0.3, 7), Link::new(4, 0.0, 8));
    let (mut server_at, mut shown, mut bare) = (Vec::new(), Vec::new(), Vec::new());
    let total = 400u64;
    for now in 0..total {
        // the input changes often: what a lost packet makes the server miss
        let walk = if (now / 7) % 3 == 0 { 0.0 } else { 1.0 };
        let axes = vec![(axis::THROTTLE, walk), (axis::YAW, if (now / 11) % 2 == 0 { 0.6 } else { -0.6 })];
        let p = mirror.send(mirror.next_step(), axes);
        up.send(now, p);
        for p in up.arrived(now) {
            server.receive(id, &p);
        }
        for (_, p) in server.tick() {
            down.send(now, p);
        }
        for p in down.arrived(now) {
            mirror.receive(&p);
        }
        prediction.follow(&mut copy, &mirror, me);
        let at = |s: &Sim| s.players.get(me).and_then(|p| p.soldier.filter(|_| p.alive)).map(|i| s.vehicles[i].craft.body.pos);
        server_at.push(at(&server.sim));
        let o = prediction.offset;
        shown.push(at(&copy));
        bare.push(at(&copy).map(|p| [p[0] - o[0], p[1] - o[1], p[2] - o[2]]));
    }
    let dist = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    // a jump: how much more a tick moves it than the server moves it then
    let jumps = |xs: &[Option<[f32; 3]>]| -> (f32, f32) {
        let j: Vec<f32> = (100..total as usize - 5)
            .filter_map(|t| {
                let (a, b, c, d) = (xs[t]?, xs[t + 1]?, server_at[t + 4]?, server_at[t + 5]?);
                Some(dist([b[0] - a[0], 0.0, b[2] - a[2]], [d[0] - c[0], 0.0, d[2] - c[2]]))
            })
            .collect();
        (j.iter().sum::<f32>() / j.len() as f32, j.iter().cloned().fold(0.0, f32::max))
    };
    let error = |xs: &[Option<[f32; 3]>]| {
        let e: Vec<f32> = (100..total as usize - 4).filter_map(|t| Some(dist(xs[t]?, server_at[t + 4]?))).collect();
        e.iter().sum::<f32>() / e.len() as f32
    };
    let (sj, sm) = jumps(&shown);
    let (bj, bm) = jumps(&bare);
    eprintln!("jumps: shown mean {sj:.4} max {sm:.3}, bare mean {bj:.4} max {bm:.3}; error: shown {:.3} bare {:.3}", error(&shown), error(&bare));
    assert!(bm > 0.05, "the prediction never missed ({bm})");
    assert!(sm < 0.5 * bm, "smoothed jumps {sm} against {bm}");
    assert!(error(&shown) < 0.3);
}

/// The others on our timeline: with 8 bots and 4 ticks each way, the soldiers and vehicles
/// shown (moved on with their speed, `PMLinear`) are nearer where the server has them when our
/// latest action arrives than the snapshot is.
#[test]
fn the_others_are_shown_as_far_ahead_as_ours() {
    the_others_shown(false);
}


fn the_others_shown(better: bool) {
    let Some(w) = world("El_Alamein") else { return };
    let mut server = Server::new(bytefield_server::round(w, 8, better));
    let (id, me) = server.join(2, 0);
    let mut copy = Sim::new(w, 1.0 / 30.0);
    copy.round.playing = true;
    copy.start_conquest();
    let (mut mirror, mut prediction) = (Mirror::default(), bytefield_server::Prediction::default());
    let (mut up, mut down) = (Link::new(4, 0.0, 1), Link::new(4, 0.0, 2));
    let (mut shown_err, mut snap_err, mut n) = (0.0f64, 0.0f64, 0usize);
    let mut history: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut pending: Vec<(u64, Vec<(usize, [f32; 3], [f32; 3])>)> = Vec::new();
    for now in 0..600u64 {
        let p = mirror.send(mirror.next_step(), vec![(axis::THROTTLE, 1.0)]);
        up.send(now, p);
        for p in up.arrived(now) {
            server.receive(id, &p);
        }
        for (_, p) in server.tick() {
            down.send(now, p);
        }
        for p in down.arrived(now) {
            mirror.receive(&p);
        }
        prediction.follow(&mut copy, &mirror, me);
        history.push(server.sim.vehicles.iter().map(|v| v.craft.body.pos).collect());
        // the soldiers of the others, moving, shown and in the snapshot
        if now > 200 && let Some(s) = mirror.latest() {
            let mine = copy.players[me].soldier;
            let others = copy.players.iter().enumerate().filter(|(k, p)| *k != me && p.alive).filter_map(|(_, p)| p.soldier).filter(|&i| Some(i) != mine && i < s.objects.len());
            let row = others.filter(|&i| s.objects[i].vel.iter().map(|x| x * x).sum::<f32>() > 1.0).map(|i| (i, copy.vehicles[i].craft.body.pos, s.objects[i].pos)).collect();
            pending.push((now + 4, row));
        }
    }
    let dist = |a: [f32; 3], b: [f32; 3]| (((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)) as f64).sqrt();
    for (t, row) in pending {
        let Some(at) = history.get(t as usize) else { continue };
        for (i, shown, snap) in row {
            shown_err += dist(shown, at[i]);
            snap_err += dist(snap, at[i]);
            n += 1;
        }
    }
    let (shown_err, snap_err) = (shown_err / n as f64, snap_err / n as f64);
    eprintln!("{n} samples: shown {shown_err:.3} m from the server, the snapshot {snap_err:.3}");
    assert!(n > 500, "{n} samples of moving soldiers");
    assert!(shown_err < 0.5 * snap_err);
}

/// Getting in and out predicted: next to a Sherman, the use key; the soldier shown is in it
/// when the key goes (the server, 4 ticks later; the snapshot alone, 8), and stays; out
/// the same way.
#[test]
fn getting_in_and_out_is_predicted() {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = Sim::new(w, 1.0 / 30.0);
    sim.round.playing = true;
    sim.start_conquest();
    let me = sim.add_player(2, "USSoldier", false);
    let mut server = Server::new(sim);
    let id = server.connect(me);
    let mut copy = Sim::new(w, 1.0 / 30.0);
    copy.round.playing = true;
    copy.start_conquest();
    let (mut mirror, mut prediction) = (Mirror::default(), bytefield_server::Prediction::default());
    let (mut up, mut down) = (Link::new(4, 0.0, 1), Link::new(4, 0.0, 2));
    let tank = server.sim.vehicles.iter().position(|v| v.template == "sherman").expect("a sherman");
    let (mut placed, mut shown_in, mut server_in) = (None, Vec::new(), Vec::new());
    for now in 0..300u64 {
        if placed.is_none()
            && let Some(i) = server.sim.players[me].soldier.filter(|_| server.sim.players[me].alive)
        {
            let p = server.sim.vehicles[tank].craft.body.pos;
            server.sim.vehicles[i].craft.body.pos = [p[0] + 2.5, p[1] + 0.5, p[2]];
            placed = Some((now, i));
        }
        // the key from 60 ticks after being placed (for 5), and again 90 later
        let use_key = placed.is_some_and(|(t, _)| (now >= t + 60 && now < t + 65) || (now >= t + 150 && now < t + 155));
        let p = mirror.send(mirror.next_step(), vec![(bytefield_sim::entry::USE, if use_key { 1.0 } else { 0.0 })]);
        up.send(now, p);
        for p in up.arrived(now) {
            server.receive(id, &p);
        }
        for (_, p) in server.tick() {
            down.send(now, p);
        }
        for p in down.arrived(now) {
            mirror.receive(&p);
        }
        prediction.follow(&mut copy, &mirror, me);
        let inside = |s: &Sim| placed.and_then(|(_, i)| s.vehicles.get(i)).is_some_and(|v| v.entry.inside.is_some());
        shown_in.push(inside(&copy));
        server_in.push(inside(&server.sim));
    }
    let edges = |xs: &[bool]| (1..xs.len()).filter(|&t| xs[t] != xs[t - 1]).collect::<Vec<_>>();
    let (shown, server) = (edges(&shown_in), edges(&server_in));
    let t0 = placed.unwrap().0 as usize;
    eprintln!("in and out: shown at {:?}, the server at {:?} (key at {} and {})", shown, server, t0 + 60, t0 + 150);
    assert_eq!(server.len(), 2, "the server's soldier in and out: {server:?}");
    assert_eq!(shown.len(), 2, "shown in and out once each: {shown:?}");
    for (a, b) in shown.iter().zip(&server) {
        assert!(*a + 3 <= *b, "shown at {a}, the server at {b}");
    }
}

/// A packet cut to a budget (N3): 24 bots in El Alamein and 350 B a packet. The packets keep to
/// it, even a joining client's; the mirror is what the server
/// sent; every object that moves gets through now and then, the near ones more often; and
/// the copy, stepping what is stale itself, stays near the server.
#[test]
fn a_tight_budget_sends_the_near_first() {
    let Some(w) = world("El_Alamein") else { return };
    let mut server = Server::new(bytefield_server::round(w, 24, false));
    server.budget = Some(350);
    let (id, me) = server.join(2, 0);
    let mut copy = bytefield_server::round(w, 0, false);
    let mut mirror = Mirror::default();
    let (mut up, mut down) = (Link::new(2, 0.0, 1), Link::new(2, 0.0, 2));
    let (total, from) = (900u64, 300u64);
    let (mut sizes, mut over) = (Vec::new(), 0);
    // by object: the steps it last came fresh, and how far it was from me then
    let mut fresh: Vec<Vec<(u64, f32)>> = Vec::new();
    let (mut near_err, mut far_err) = ((0.0f32, 0), (0.0f32, 0));
    for now in 0..total {
        let p = mirror.send(mirror.next_step(), vec![(axis::THROTTLE, 1.0), (axis::YAW, 0.2)]);
        up.send(now, p);
        for p in up.arrived(now) {
            server.receive(id, &p);
        }
        for (_, p) in server.tick() {
            if now >= from {
                sizes.push(p.len());
            }
            over += (p.len() > 350) as usize;
            down.send(now, p);
        }
        for p in down.arrived(now) {
            let before = mirror.latest().map(|s| s.step);
            mirror.receive(&p);
            let Some(s) = mirror.latest() else { continue };
            if Some(s.step) != before && now >= from {
                assert!(server.sent_at(id, s.step) == Some(s), "the mirror of step {} differs", s.step);
            }
        }
        assert_eq!(bytefield_server::follow(&mut copy, &mirror), 0);
        // the copy against the server where it is (its step: the latest snapshot's)
        let Some(s) = mirror.latest().filter(|_| now >= from) else { continue };
        let Some(snap) = server.snapshot_at(s.step) else { continue };
        let Some(eye) = server.sim.players[me].soldier.map(|i| snap.objects[i].pos) else { continue };
        fresh.resize(snap.objects.len(), Vec::new());
        for (k, o) in snap.objects.iter().enumerate() {
            let Some(v) = copy.vehicles.get(k) else { continue };
            if o.gone || o.vel.iter().all(|x| x.abs() < 0.1) {
                continue;
            }
            let r = ((o.pos[0] - eye[0]).powi(2) + (o.pos[2] - eye[2]).powi(2)).sqrt();
            if !s.stale.contains(&(k as u32)) {
                fresh[k].push((s.step, r));
            }
            let e = ((v.craft.body.pos[0] - o.pos[0]).powi(2) + (v.craft.body.pos[2] - o.pos[2]).powi(2)).sqrt();
            let acc = if r < 50.0 { &mut near_err } else { &mut far_err };
            acc.0 += e;
            acc.1 += 1;
        }
    }
    let gaps = |near: bool| {
        let g: Vec<u64> = fresh.iter().flat_map(|f| f.windows(2).filter(|w| (w[1].1 < 50.0) == near).map(|w| w[1].0 - w[0].0)).collect();
        (g.iter().sum::<u64>() as f32 / g.len().max(1) as f32, g.iter().copied().max().unwrap_or(0))
    };
    let ((near_gap, near_max), (far_gap, far_max)) = (gaps(true), gaps(false));
    let mean = sizes.iter().sum::<usize>() / sizes.len();
    eprintln!(
        "packets: mean {mean} B, max {}, over the budget {over} of {}; steps between fresh: near {near_gap:.1} (max {near_max}), far {far_gap:.1} (max {far_max}); the copy off: near {:.3} m, far {:.3} m",
        sizes.iter().max().unwrap(),
        sizes.len(),
        near_err.0 / near_err.1.max(1) as f32,
        far_err.0 / far_err.1.max(1) as f32
    );
    assert_eq!(over, 0, "packets over the budget");
    assert!(near_gap < far_gap, "the near ones are not sent more often");
    assert!(far_max < 200, "an object waited {far_max} steps");
    assert!(near_gap < 2.0, "a near object waits {near_gap} steps");
}

/// A client that joins a running round, with the packets of 1200 B at most and a few lost: it builds its world over several packets, none of them
/// over the budget, shows nothing until it is whole, and ends with all of it.
#[test]
fn a_joining_client_gets_its_world_in_pieces() {
    let Some(w) = world("El_Alamein") else { return };
    let mut server = Server::new(bytefield_server::round(w, 24, false));
    for _ in 0..200 {
        server.tick();
    }
    let (id, _) = server.join(1, 0);
    let (mut up, mut down) = (Link::new(2, 0.05, 11), Link::new(2, 0.05, 12));
    let mut mirror = Mirror::default();
    let mut most = 0;
    let mut first_whole = None;
    for now in 0..240 {
        let p = mirror.send(mirror.next_step(), Vec::new());
        up.send(now, p);
        for p in up.arrived(now) {
            server.receive(id, &p);
        }
        for (_, p) in server.tick() {
            most = most.max(p.len());
            down.send(now, p);
        }
        for p in down.arrived(now) {
            mirror.receive(&p);
        }
        if first_whole.is_none() && mirror.latest().is_some() {
            first_whole = Some(now);
        }
    }
    let s = mirror.latest().expect("whole by now");
    eprintln!("the biggest packet {most} B; whole after {first_whole:?} steps");
    assert!(most <= bytefield_server::BUDGET, "a packet of {most} B");
    assert!(first_whole.unwrap() < 90, "{first_whole:?}");
    let there = server.latest().unwrap();
    assert_eq!(s.objects.len(), there.objects.len());
    assert_eq!(s.names, there.names);
}

/// What is out of sight: an object past the view distance gets an update about once in 20 steps, one in sight far more often, and
/// a change of state goes at once, however far.
#[test]
fn what_is_out_of_sight_waits_but_its_state_does_not() {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = bytefield_server::round(w, 0, false);
    sim.step();
    let mut server = Server::new(sim);
    let (id, _) = server.join(1, 0);
    server.view = 50.0;
    let mut mirror = Mirror::default();
    let far = |server: &Server| {
        let me = server.sim.players[0].soldier?;
        let e = server.sim.vehicles[me].craft.body.pos;
        (0..server.sim.vehicles.len()).filter(|&k| k != me && server.sim.vehicles[k].in_world() && !server.sim.vehicles[k].fixed && server.sim.vehicles[k].armor.is_some()).find(|&k| {
            let p = server.sim.vehicles[k].craft.body.pos;
            (p[0] - e[0]).hypot(p[2] - e[2]) > 200.0
        })
    };
    let mut k = None;
    let (mut sent, mut quiet) = (0, 0);
    let mut spin = 0.0;
    for now in 0..420 {
        let p = mirror.send(mirror.next_step(), vec![(axis::FIRE, 0.0)]);
        server.receive(id, &p);
        if k.is_none() && now > 120 {
            k = far(&server);
        }
        if let Some(k) = k {
            spin += 1.0;
            server.sim.vehicles[k].craft.body.pos[1] += 0.01 * (spin as f32).sin() + 0.02;
        }
        let packets = server.tick();
        for (_, p) in &packets {
            let d = bytefield_sim::replica::Delta::decode(p).unwrap();
            if let Some(k) = k.filter(|_| now > 160) {
                if d.objects.iter().any(|o| o.0 as usize == k) {
                    sent += 1;
                } else {
                    quiet += 1;
                }
            }
            mirror.receive(p);
        }
        if now == 400 {
            let k = k.expect("a far vehicle");
            let hp = server.sim.vehicles[k].armor.as_ref().unwrap().hp;
            server.sim.vehicles[k].armor.as_mut().unwrap().hp = hp * 0.5;
            let packets = server.tick();
            let d = bytefield_sim::replica::Delta::decode(&packets[0].1).unwrap();
            assert!(d.objects.iter().any(|o| o.0 as usize == k && o.1.hp.is_some()), "the damage waits");
            mirror.receive(&packets[0].1);
        }
    }
    eprintln!("the far one: {sent} packets with it, {quiet} without");
    assert!(sent >= 8 && quiet > 8 * sent, "{sent} and {quiet}");
}
