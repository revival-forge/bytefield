//! A server and a client over UDP on this machine (N3): the client's copy of the world, written
//! from each snapshot and stepped with the players' actions, is the server's.

use std::time::Duration;

use bytefield_server::udp::{Hello, UdpClient, UdpServer};
use bytefield_server::{Server, follow, round};
use bytefield_sim::replica::Snapshot;
use bytefield_sim::{Sim, World, soldier::axis};

fn world(level: &str) -> Option<&'static World> {
    bytefield_testdata::base().map(|game| &*Box::leak(Box::new(World::load(&game, "bf1942", level, "conquest").expect("level"))))
}

#[test]
fn a_client_over_udp_keeps_a_copy_of_the_round() {
    let Some(w) = world("El_Alamein") else { return };
    let mut server = Server::new(round(w, 4, false));
    let mut udp = UdpServer::bind("127.0.0.1:0", "bf1942", "El_Alamein", "conquest").unwrap();
    // its welcome lines (`--welcome`), as the briefing's COMMENTS: with a control character taken out
    udp.comments = vec!["Hello".into(), "two\nlines".into()];
    let addr = udp.local_addr();
    // the server answers while the client waits for it
    let hello = std::thread::spawn(move || UdpClient::connect(addr, Hello { team: 2, kit: 0, name: "tester".into() }, Duration::from_secs(5)));
    while !hello.is_finished() {
        udp.poll(&mut server);
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut client = hello.join().unwrap().expect("welcomed");
    assert_eq!(client.welcome.level, "El_Alamein");
    assert_eq!(client.welcome.comments(), ["Hello", "twolines"]);
    let me = client.welcome.player as usize;
    let mut copy = Sim::new(w, bytefield_server::TICK);
    copy.round.playing = true;
    copy.start_conquest();
        let (mut odd, mut written, mut start) = (0, 0, None);
    for _ in 0..300 {
        let step = server.sim.clock.steps + 1;
        client.send(step, vec![(axis::THROTTLE, 1.0)]);
        std::thread::sleep(Duration::from_millis(1));
        udp.poll(&mut server);
        udp.tick(&mut server);
        std::thread::sleep(Duration::from_millis(1));
        client.poll();
        if client.mirror.latest().is_some_and(|s| s.step > copy.clock.steps) {
            odd += follow(&mut copy, &client.mirror);
            written += 1;
        }
        if start.is_none()
            && let Some(i) = server.sim.players[me].soldier.filter(|_| server.sim.players[me].alive)
        {
            start = Some((i, server.sim.vehicles[i].craft.body.pos));
        }
    }
    assert_eq!(odd, 0, "objects the copy could not match");
    assert!(written > 250, "{written} snapshots");
    let s = client.mirror.latest().unwrap();
    assert_eq!(server.sent_at(0, s.step), Some(s));
    // the copy is the last snapshot
    assert_eq!(copy.clock.steps, s.step);
    let mine = Snapshot::of(&copy);
    // (but what is out of sight: the copy steps that on its own)
    for (a, b) in mine.objects.iter().zip(&s.objects).enumerate().filter(|(k, _)| !s.stale.contains(&(*k as u32))).map(|(_, x)| x) {
        assert_eq!((a.pos, a.vel, a.hp), (b.pos, b.vel, b.hp), "{}", a.template);
        // (rows to a quaternion again may land a step away)
        assert!(a.rot.iter().zip(&b.rot).all(|(x, y)| (x - y).abs() <= 1), "{}", a.template);
    }
    assert_eq!(mine.players.len(), s.players.len());
    let (i, p0) = start.expect("spawned");
    let p1 = server.sim.vehicles[i].craft.body.pos;
    assert!(((p1[0] - p0[0]).powi(2) + (p1[2] - p0[2]).powi(2)).sqrt() > 5.0, "the client's soldier did not move");
}

/// A server's pre-game and its wait for a second person reach the client's copy: it waits, plays
/// with one person and the «players needed», and when another comes the map starts again with
/// its pre-game.
#[test]
fn the_copy_has_the_servers_pregame() {
    let Some(w) = world("El_Alamein") else { return };
    let mut sim = round(w, 0, false);
    bytefield_server::hosted(&mut sim, "conquest", 0, 1.0, None);
    let mut server = Server::new(sim);
    let mut udp = UdpServer::bind("127.0.0.1:0", "bf1942", "El_Alamein", "conquest").unwrap();
    let addr = udp.local_addr();
    let hello = std::thread::spawn(move || UdpClient::connect(addr, Hello { team: 2, kit: 0, name: "tester".into() }, Duration::from_secs(5)));
    while !hello.is_finished() {
        udp.poll(&mut server);
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut client = hello.join().unwrap().expect("welcomed");
    let mut copy = Sim::new(w, bytefield_server::TICK);
    copy.start_pregame(0.0);
    copy.start_conquest();
    let mut run = |server: &mut Server, copy: &mut Sim, n: usize| {
        for _ in 0..n {
            udp.poll(server);
            udp.tick(server);
            std::thread::sleep(Duration::from_millis(1));
            client.poll();
            client.send(client.mirror.next_step(), Vec::new());
            if client.mirror.latest().is_some_and(|s| s.step > copy.clock.steps) {
                follow(copy, &client.mirror);
            }
        }
    };
    run(&mut server, &mut copy, 10);
    assert!(!copy.round.playing && copy.round.pregame > 0.5 && copy.rules.pregame_delay == 1.0, "the copy waits: {}", copy.round.pregame);
    assert_eq!(copy.round.players_needed, 1);
    run(&mut server, &mut copy, 40);
    assert!(copy.round.playing && copy.round.pregame == 0.0, "then it plays");
    assert_eq!(copy.round.players_needed, 1, "and the round waits for another person");
    server.join(1, 0);
    run(&mut server, &mut copy, 5);
    assert!(!copy.round.playing && copy.round.pregame > 0.5 && copy.round.players_needed == 0, "the map starts again");
}

/// The state a copy's step lacks: a bf109 (or `OBJ`) of the server and of a copy stepped and
/// written for 60 steps, one step further, as `{:#?}` in `OUT`/server.txt and copy.txt (to diff).
#[test]
#[ignore]
fn what_the_copy_lacks() {
    let Some(w) = world("El_Alamein") else { return };
    let mut server = round(w, 0, false);
    let mut copy = Sim::new(w, bytefield_server::TICK);
    copy.round.playing = true;
    copy.start_conquest();
    for _ in 0..60 {
        server.step();
        let s = Snapshot::of(&server);
        s.write(&mut copy);
        s.act(&mut copy);
        copy.step();
    }
    server.step();
    let i: usize = std::env::var("OBJ").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| server.vehicles.iter().position(|v| v.template == "bf109").unwrap());
    let a = format!("{:#?}", server.vehicles[i]);
    let b = format!("{:#?}", copy.vehicles[i]);
    let Ok(dir) = std::env::var("OUT") else { return };
    std::fs::write(format!("{dir}/server.txt"), a).unwrap();
    std::fs::write(format!("{dir}/copy.txt"), b).unwrap();
}
