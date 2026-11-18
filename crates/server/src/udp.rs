//! The packets over UDP: our own protocol, between our client and our server; it talks to nothing of the original.
//! A client says `Hello` with no token; the server answers `Challenge` with one that only that address can read, and joins only a
//! `Hello` that brings it back, so that nobody can join someone else's address or have snapshots sent to it. Then
//! the client sends its [`ClientPacket`]s and the server its [`Delta`]s, each tagged, and what a client sends carries the token. A packet
//! lost is not sent again: the next one carries the same (the last actions; a delta against what was acked).
//! Anyone can send anything to the port: what does not fit is dropped without an answer.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hash, Hasher};
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{Mirror, Server};
use bytefield_sim::replica::Axes;

/// The port if none is said.
pub const PORT: u16 = 14567;
/// The first bytes of a `Hello`: what comes without them is not for us.
const MAGIC: u32 = u32::from_le_bytes(*b"fbf\0");
/// Changes when the packets do.
pub const PROTOCOL: u32 = 5;
/// Players on a server unless it says otherwise.
pub const MOST_CLIENTS: usize = 64;
/// Datagrams taken at most in a poll: a flood slows the server down, but the round goes on.
const MOST_IN_A_POLL: usize = 4096;
/// A datagram's most (the IPv4 limit for UDP).
const MAX: usize = 65507;
/// Seconds without a packet after which a client is dropped.
const TIMEOUT: f32 = 10.0;

const HELLO: u8 = 1;
const WELCOME: u8 = 2;
const ACTIONS: u8 = 3;
const DELTA: u8 = 4;
const BYE: u8 = 5;
const CHALLENGE: u8 = 6;
const REFUSED: u8 = 7;
const REFUSED_PROTOCOL: u8 = 1;
const REFUSED_FULL: u8 = 2;

/// What a client says it wants.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Hello {
    pub team: u32,
    pub kit: u32,
    pub name: String,
}

/// A `Hello` as it goes.
#[derive(Serialize, Deserialize)]
struct HelloPacket {
    magic: u32,
    protocol: u32,
    token: u64,
    team: u32,
    kit: u32,
    name: String,
}

fn decode<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Option<T> {
    use bincode::Options;
    bincode::DefaultOptions::new().with_fixint_encoding().allow_trailing_bytes().with_limit(body.len() as u64).deserialize(body).ok()
}

/// A mod's or level's name, as it may come from a server: it goes into paths on the client's disk.
fn plain_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 128 && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' ' | '.')) && !n.contains("..")
}

/// What a client needs to load the same round: the level, and which player it is.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Welcome {
    pub mod_name: String,
    pub level: String,
    pub mode: String,
    pub player: u32,
    /// the server's welcome lines (`Sim::welcome`), as the original's `WelcomeMsgEvent`s at `authentificationOk`
    pub comments: Vec<String>,
}

/// The most welcome lines a client keeps, and their length: the briefing shows five.
pub const MOST_COMMENTS: usize = 16;
const COMMENT_CHARS: usize = 256;

impl Welcome {
    /// The server's lines, as a client may take them: a few, short and without control characters.
    pub fn comments(&self) -> Vec<String> {
        self.comments.iter().take(MOST_COMMENTS).map(|l| l.chars().filter(|c| !c.is_control()).take(COMMENT_CHARS).collect()).collect()
    }
}

fn tagged(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + 1);
    v.push(tag);
    v.extend_from_slice(body);
    v
}

fn recv(socket: &UdpSocket, buf: &mut [u8]) -> Option<(usize, SocketAddr)> {
    match socket.recv_from(buf) {
        Ok(r) => Some(r),
        // (a client gone answers with an ICMP that some systems give as ConnectionReset)
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset) => None,
        Err(e) => {
            eprintln!("udp: {e}");
            None
        }
    }
}

struct Peer {
    client: usize,
    welcome: Vec<u8>,
    heard: Instant,
}

/// The biggest UDP payload that fits a 1500-byte Ethernet frame on IPv4: more goes in IP fragments, and losing one loses the datagram.
pub const MTU_PAYLOAD: usize = 1472;

/// Datagrams and bytes through a socket.
#[derive(Clone, Copy, Debug, Default)]
pub struct NetCount {
    pub datagrams: u64,
    pub bytes: u64,
    pub biggest: usize,
    pub over_mtu: u64,
}

impl NetCount {
    fn count(&mut self, n: usize) {
        self.datagrams += 1;
        self.bytes += n as u64;
        self.biggest = self.biggest.max(n);
        self.over_mtu += u64::from(n > MTU_PAYLOAD);
    }
}

/// The server's end: one socket, a client per address.
pub struct UdpServer {
    socket: UdpSocket,
    peers: HashMap<SocketAddr, Peer>,
    level: (String, String, String),
    /// what each `Welcome` brings as `comments` ([`Welcome::comments`])
    pub comments: Vec<String>,
    buf: Vec<u8>,
    /// this run's secret for the tokens
    secret: std::collections::hash_map::RandomState,
    pub most_clients: usize,
    /// what went out: datagrams, bytes, the biggest datagram, and how many went over `MTU_PAYLOAD` (the load test, `examples/loadtest.rs`)
    pub sent: NetCount,
    /// what came in
    pub heard: NetCount,
}

impl UdpServer {
    pub fn bind(addr: impl ToSocketAddrs, mod_name: &str, level: &str, mode: &str) -> std::io::Result<UdpServer> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(UdpServer { socket, peers: HashMap::new(), level: (mod_name.into(), level.into(), mode.into()), comments: Vec::new(), buf: vec![0; MAX], secret: Default::default(), most_clients: MOST_CLIENTS, sent: NetCount::default(), heard: NetCount::default() })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.socket.local_addr().expect("bound")
    }

    pub fn clients(&self) -> usize {
        self.peers.len()
    }

    fn token(&self, addr: &SocketAddr) -> u64 {
        let mut h = self.secret.build_hasher();
        addr.hash(&mut h);
        // (never 0: that is what a first `Hello` brings)
        h.finish() | 1
    }

    /// Everything that came: a new client with the right token joins `server` (a `Hello` again gets the same
    /// `Welcome`), actions go to theirs.
    pub fn poll(&mut self, server: &mut Server) {
        for _ in 0..MOST_IN_A_POLL {
            let Some((n, from)) = recv(&self.socket, &mut self.buf) else { break };
            self.heard.count(n);
            let (tag, body) = match self.buf[..n].split_first() {
                Some((t, b)) => (*t, b),
                None => continue,
            };
            let token = self.token(&from);
            match tag {
                HELLO => {
                    let Some(h) = decode::<HelloPacket>(body).filter(|h| h.magic == MAGIC) else { continue };
                    if h.protocol != PROTOCOL {
                        let _ = self.socket.send_to(&[REFUSED, REFUSED_PROTOCOL], from);
                        continue;
                    }
                    if h.token != token {
                        // (smaller than what asked for it: this port cannot be used to shout at someone)
                        let _ = self.socket.send_to(&tagged(CHALLENGE, &token.to_le_bytes()), from);
                        continue;
                    }
                    if !self.peers.contains_key(&from) {
                        if self.peers.len() >= self.most_clients {
                            let _ = self.socket.send_to(&[REFUSED, REFUSED_FULL], from);
                            continue;
                        }
                        let (client, player) = server.join(h.team.min(2), h.kit as usize);
                        server.set_name(client, &h.name);
                        let (mod_name, level, mode) = self.level.clone();
                        let mut w = Welcome { mod_name, level, mode, player: player as u32, comments: self.comments.clone() };
                        // (a welcome in one frame: the last lines of a very long message give way)
                        while bincode::serialized_size(&w).unwrap_or(0) as usize + 1 > MTU_PAYLOAD && w.comments.pop().is_some() {}
                        eprintln!("{from} joins: player {player}, team {}", h.team);
                        self.peers.insert(from, Peer { client, welcome: tagged(WELCOME, &bincode::serialize(&w).unwrap()), heard: Instant::now() });
                    }
                    let p = &self.peers[&from];
                    let _ = self.socket.send_to(&p.welcome, from);
                }
                ACTIONS | BYE if body.len() >= 8 && body[..8] == token.to_le_bytes() => {
                    let body = &body[8..];
                    if tag == ACTIONS {
                        if let Some(p) = self.peers.get_mut(&from) {
                            p.heard = Instant::now();
                            server.receive(p.client, body);
                        }
                    } else if let Some(p) = self.peers.remove(&from) {
                        eprintln!("{from} leaves");
                        server.leave(p.client);
                    }
                }
                _ => {}
            }
        }
        let mut lost = Vec::new();
        self.peers.retain(|a, p| {
            let keep = p.heard.elapsed().as_secs_f32() < TIMEOUT;
            if !keep {
                eprintln!("{a}: no word in {TIMEOUT} s, dropped");
                lost.push(p.client);
            }
            keep
        });
        for c in lost {
            server.leave(c);
        }
    }

    /// One step of `server`, its packets out.
    pub fn tick(&mut self, server: &mut Server) {
        let out = server.tick();
        for (addr, p) in &self.peers {
            if let Some((_, d)) = out.iter().find(|(c, _)| *c == p.client) {
                if d.len() + 1 > MAX {
                    eprintln!("udp: a snapshot of {} B does not fit a datagram", d.len());
                    continue;
                }
                self.sent.count(d.len() + 1);
                let _ = self.socket.send_to(&tagged(DELTA, d), addr);
            }
        }
    }
}

/// The client's end: a socket connected to the server and its copy of the world.
pub struct UdpClient {
    socket: UdpSocket,
    pub mirror: Mirror,
    pub welcome: Welcome,
    token: u64,
    buf: Vec<u8>,
    /// what came in
    pub heard: NetCount,
    /// in tests: of 1000 frames of 1472 B, how many are lost, each way (a bigger datagram needs all of its frames)
    pub lose: u32,
    dice: u64,
    /// when the server was last heard from
    last_heard: Instant,
}

impl UdpClient {
    /// How long the server has said nothing.
    pub fn silent(&self) -> Duration {
        self.last_heard.elapsed()
    }

    /// Whether a datagram of `n` B is lost on the way ([`Self::lose`]).
    fn lost(&mut self, n: usize) -> bool {
        (0..n.div_ceil(MTU_PAYLOAD).max(1)).fold(false, |any, _| {
            self.dice = self.dice.wrapping_mul(0x5851_F42D_4C95_7F2D).wrapping_add(0x14_0577);
            any | ((self.dice >> 33) % 1000 < self.lose as u64)
        })
    }

    /// `Hello` every 200 ms until the server welcomes, or `wait` passes.
    pub fn connect(server: impl ToSocketAddrs, hello: Hello, wait: Duration) -> std::io::Result<UdpClient> {
        let server = server.to_socket_addrs()?.next().ok_or_else(|| std::io::Error::new(ErrorKind::NotFound, "no address"))?;
        let bind: SocketAddr = if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse().unwrap();
        let socket = UdpSocket::bind(bind)?;
        socket.connect(server)?;
        socket.set_read_timeout(Some(Duration::from_millis(200)))?;
        let mut token = 0u64;
        let packet = |token| tagged(HELLO, &bincode::serialize(&HelloPacket { magic: MAGIC, protocol: PROTOCOL, token, team: hello.team, kit: hello.kit, name: hello.name.clone() }).unwrap());
        let start = Instant::now();
        let mut buf = vec![0; MAX];
        while start.elapsed() < wait {
            socket.send(&packet(token))?;
            let until = Instant::now() + Duration::from_millis(200);
            while Instant::now() < until {
                let n = match socket.recv(&mut buf) {
                    Ok(n) => n,
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset) => break,
                    Err(e) => return Err(e),
                };
                match (buf.first(), &buf[..n]) {
                    (Some(&CHALLENGE), [_, t @ ..]) if t.len() == 8 => {
                        token = u64::from_le_bytes(t.try_into().unwrap());
                        socket.send(&packet(token))?;
                    }
                    (Some(&REFUSED), [_, why]) => {
                        let text = match *why {
                            REFUSED_PROTOCOL => "the server is another version of bytefield",
                            REFUSED_FULL => "the server is full",
                            _ => "the server refused",
                        };
                        return Err(std::io::Error::new(ErrorKind::ConnectionRefused, text));
                    }
                    (Some(&WELCOME), [_, w @ ..]) => {
                        let Some(welcome) = decode::<Welcome>(w) else { continue };
                        if !plain_name(&welcome.mod_name) || !plain_name(&welcome.level) || !plain_name(&welcome.mode) {
                            return Err(std::io::Error::new(ErrorKind::InvalidData, "the server names a level that cannot be"));
                        }
                        socket.set_nonblocking(true)?;
                        return Ok(UdpClient { socket, mirror: Mirror::default(), welcome, token, buf, heard: NetCount::default(), lose: 0, dice: token, last_heard: Instant::now() });
                    }
                    _ => {}
                }
            }
        }
        Err(std::io::Error::new(ErrorKind::TimedOut, format!("no answer from {server}")))
    }

    /// The deltas that came, into the mirror; how many.
    pub fn poll(&mut self) -> usize {
        let mut got = 0;
        loop {
            let n = match self.socket.recv(&mut self.buf) {
                Ok(n) => n,
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset) => break,
                Err(e) => {
                    eprintln!("udp: {e}");
                    break;
                }
            };
            self.heard.count(n);
            self.last_heard = Instant::now();
            if self.lose > 0 && self.lost(n) {
                continue;
            }
            if n > 1 && self.buf[0] == DELTA {
                self.mirror.receive(&self.buf[1..n]);
                got += 1;
            }
        }
        got
    }

    /// The action for `step` (with the last few again and the ack).
    pub fn send(&mut self, step: u64, axes: Axes) {
        let p = self.mirror.send(step, axes);
        let p = self.signed(ACTIONS, &p);
        if self.lose > 0 && self.lost(p.len()) {
            return;
        }
        let _ = self.socket.send(&p);
    }

    pub fn bye(&self) {
        let _ = self.socket.send(&self.signed(BYE, &[]));
    }

    fn signed(&self, tag: u8, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(body.len() + 9);
        v.push(tag);
        v.extend_from_slice(&self.token.to_le_bytes());
        v.extend_from_slice(body);
        v
    }
}

impl Drop for UdpClient {
    fn drop(&mut self) {
        self.bye();
    }
}

/// Steps at 30 Hz on the real clock (a slow step is caught up, at most 5 at a time), each
/// after taking what came.
pub fn serve(server: &mut Server, udp: &mut UdpServer, steps: Option<u64>) {
    serve_while(server, udp, steps, &std::sync::atomic::AtomicBool::new(true));
}

/// [`serve`] while `run` holds (a client hosting stops its server with its round).
pub fn serve_while(server: &mut Server, udp: &mut UdpServer, steps: Option<u64>, run: &std::sync::atomic::AtomicBool) {
    let start = Instant::now();
    let mut done = 0u64;
    let mut report = Instant::now();
    while steps.is_none_or(|n| done < n) && run.load(std::sync::atomic::Ordering::Relaxed) {
        let due = (start.elapsed().as_secs_f64() / crate::TICK) as u64;
        if due <= done {
            // (at 0: the clock may have passed the tick since `due`; negative, it was a panic on a loaded machine)
            std::thread::sleep(Duration::from_secs_f64((crate::TICK * (done + 1) as f64 - start.elapsed().as_secs_f64()).max(0.0)).min(Duration::from_millis(5)));
            udp.poll(server);
            continue;
        }
        for _ in 0..(due - done).min(5) {
            udp.poll(server);
            udp.tick(server);
        }
        done = due;
        if report.elapsed() > Duration::from_secs(30) {
            report = Instant::now();
            let t = server.sim.modes.conquest.as_ref().map(|c| c.tickets).unwrap_or_default();
            eprintln!("step {}: {} clients, tickets {} {}", server.sim.clock.steps, udp.clients(), t[0], t[1]);
        }
    }
}
