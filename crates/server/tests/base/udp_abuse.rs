//! Our protocol against what anyone can send to an open port. Skipped without the real data (`data/game`).
use bytefield_server::udp::{Hello, PROTOCOL, UdpClient, UdpServer};
use bytefield_server::{CHAT_LINE, ClientPacket, Mirror, Server, round};
use bytefield_sim::World;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn world() -> Option<&'static World> {
    let Some(game) = bytefield_testdata::with_mod("bf1942") else { return None; };
    Some(Box::leak(Box::new(World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap())))
}

/// A small generator: the tests do not ask for more.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// A server polled on its own thread while `f` runs.
fn with_server<R>(w: &'static World, set: impl FnOnce(&mut UdpServer), f: impl FnOnce(std::net::SocketAddr, &Mutex<Server<'static>>) -> R) -> R {
    let server = Arc::new(Mutex::new(Server::new(round(w, 0, false))));
    let mut udp = UdpServer::bind("127.0.0.1:0", "bf1942", "El_Alamein", "conquest").unwrap();
    set(&mut udp);
    let addr = udp.local_addr();
    let run = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let (s, r) = (server.clone(), run.clone());
    let t = std::thread::spawn(move || {
        while r.load(std::sync::atomic::Ordering::Relaxed) {
            let mut s = s.lock().unwrap();
            udp.poll(&mut s);
            udp.tick(&mut s);
            drop(s);
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let out = f(addr, &server);
    run.store(false, std::sync::atomic::Ordering::Relaxed);
    t.join().expect("the server fell over");
    out
}

fn hello(team: u32, name: &str) -> Hello {
    Hello { team, kit: 0, name: name.into() }
}

fn human_players(s: &Mutex<Server<'static>>) -> usize {
    s.lock().unwrap().sim.players.iter().filter(|p| !p.kicked).count()
}

#[test]
fn a_datagram_alone_makes_no_player_and_gets_little_back() {
    let Some(w) = world() else { return };
    with_server(w, |_| {}, |addr, s| {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.connect(addr).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        // what a first `Hello` looks like: tag, magic, protocol, no token, team, kit, an empty name
        let mut first = vec![1u8];
        first.extend_from_slice(b"fbf\0");
        first.extend_from_slice(&PROTOCOL.to_le_bytes());
        first.extend_from_slice(&[0; 8 + 4 + 4 + 8]);
        sock.send(&first).unwrap();
        let mut buf = [0u8; 2048];
        let n = sock.recv(&mut buf).expect("a challenge");
        assert!(n <= first.len(), "the answer is no bigger than what asked for it");
        assert_eq!(buf[0], 6);
        assert_eq!(human_players(s), 0);
        // another protocol: told so, not joined
        let mut other = first.clone();
        other[5] = other[5].wrapping_add(1);
        sock.send(&other).unwrap();
        let n = sock.recv(&mut buf).expect("refused");
        assert_eq!((n, buf[0], buf[1]), (2, 7, 1));
        // not ours: no answer at all
        sock.send(b"\xfe\xfd\x00garbage").unwrap();
        assert!(sock.recv(&mut buf).is_err());
    });
}

#[test]
fn garbage_does_not_bring_it_down() {
    let Some(w) = world() else { return };
    with_server(w, |_| {}, |addr, s| {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.connect(addr).unwrap();
        let mut r = Rng(0x9E3779B97F4A7C15);
        for k in 0..20000u32 {
            let n = (r.next() % 300) as usize;
            let mut d = r.bytes(n);
            if let Some(t) = d.first_mut() {
                *t = (k % 10) as u8;
            }
            let _ = sock.send(&d);
        }
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(human_players(s), 0);
    });
}

#[test]
fn wild_packets_from_a_client_do_not_bring_it_down() {
    let Some(w) = world() else { return };
    let mut server = Server::new(round(w, 2, false));
    let (c, _) = server.join(2, 0);
    let mut r = Rng(77);
    for _ in 0..2000 {
        let n = (r.next() % 400) as usize;
        server.receive(c, &r.bytes(n));
    }
    for k in 0..300u64 {
        let step = if k % 2 == 0 { r.next() } else { server.sim.clock.steps + 1 + k };
        let p = ClientPacket {
            ack: Some(r.next()),
            actions: vec![(step, vec![(r.next() as usize, f32::from_bits(r.next() as u32)), (3, f32::NAN)]); 8],
            radio: vec![(r.next() as u32, r.next() as u8, r.next() as u8); 40],
            spawn: Some((Some(r.next() as i64), r.next() as u8)),
            hold: k % 3 == 0,
            chat: vec![(k as u32, k % 2 == 0, "\u{7}x".repeat(5000))],
        };
        server.receive(c, &p.encode());
        server.tick();
    }
    // far from everything: what comes after is still heard
    assert!(server.sim.clock.steps >= 300);
}

#[test]
fn a_mirror_takes_a_damaged_delta_without_falling_over() {
    let Some(w) = world() else { return };
    let mut server = Server::new(round(w, 4, false));
    let (c, _) = server.join(2, 0);
    let mut good = Vec::new();
    for _ in 0..30 {
        for (k, d) in server.tick() {
            if k == c {
                good.push(d);
            }
        }
    }
    let mut r = Rng(5);
    let mut m = Mirror::default();
    for _ in 0..20000 {
        let mut d = good[(r.next() % good.len() as u64) as usize].clone();
        for _ in 0..(r.next() % 6) {
            let i = (r.next() % d.len() as u64) as usize;
            d[i] = r.next() as u8;
        }
        d.truncate(1 + (r.next() % d.len() as u64) as usize);
        m.receive(&d);
    }
}

#[test]
fn who_leaves_makes_room_and_the_server_can_be_full() {
    let Some(w) = world() else { return };
    with_server(w, |u| u.most_clients = 2, |addr, s| {
        let wait = Duration::from_secs(5);
        let a = UdpClient::connect(addr, hello(1, "a"), wait).unwrap();
        let b = UdpClient::connect(addr, hello(2, "b"), wait).unwrap();
        assert_eq!(human_players(s), 2);
        let full = UdpClient::connect(addr, hello(1, "c"), wait).err().expect("no room");
        assert!(full.to_string().contains("full"), "{full}");
        let seen = a.welcome.player;
        drop(a);
        let t = Instant::now();
        while human_players(s) != 1 && t.elapsed() < wait {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(human_players(s), 1);
        for _ in 0..3 {
            let c = UdpClient::connect(addr, hello(1, "c"), wait).unwrap();
            assert_eq!(c.welcome.player, seen, "its place, not a new one");
            drop(c);
            std::thread::sleep(Duration::from_millis(200));
        }
        assert_eq!(s.lock().unwrap().sim.players.len(), 2);
        drop(b);
    });
}

#[test]
fn names_and_chat_reach_who_they_should() {
    let Some(w) = world() else { return };
    let mut server = Server::new(round(w, 0, false));
    let (a, pa) = server.join(1, 0);
    let (b, _) = server.join(2, 0);
    let (c, _) = server.join(1, 0);
    server.set_name(a, "Ana\u{7}");
    server.set_name(b, "Ana");
    server.set_name(c, &"x".repeat(200));
    assert_eq!(server.sim.players[pa].name, "Ana");
    assert_eq!(server.sim.players[server.player(b)].name, "Ana2");
    assert_eq!(server.sim.players[server.player(c)].name.chars().count(), bytefield_server::NAME_LINE);
    let mut mirrors = [Mirror::default(), Mirror::default(), Mirror::default()];
    let step = |server: &mut Server, mirrors: &mut [Mirror; 3]| {
        for (k, d) in server.tick() {
            mirrors[k].receive(&d);
        }
        for (k, m) in mirrors.iter_mut().enumerate() {
            let s = m.next_step();
            let p = m.send(s, Vec::new());
            server.receive(k, &p);
        }
    };
    for _ in 0..3 {
        step(&mut server, &mut mirrors);
    }
    mirrors[0].say(&format!("here{}", "y".repeat(500)), true);
    for _ in 0..30 {
        step(&mut server, &mut mirrors);
    }
    mirrors[0].say("team only", false);
    for _ in 0..30 {
        step(&mut server, &mut mirrors);
    }
    let all = format!("Ana: here{}", "y".repeat(CHAT_LINE - 4));
    assert_eq!(mirrors[1].chat, vec![all.clone()], "once, and cut; the other team hears only what is to all");
    assert_eq!(mirrors[2].chat, vec![all.clone(), "(Ana) team only".to_string()]);
    assert_eq!(mirrors[0].chat, vec![all, "(Ana) team only".to_string()]);
    let names = &mirrors[1].latest().unwrap().names;
    assert_eq!(names[pa], "Ana");
}
