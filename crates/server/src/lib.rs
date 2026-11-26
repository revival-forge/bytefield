//! The dedicated server: one `Sim`, each client's `PlayerAction`s queued by the
//! step they are for, and per client a [`Delta`] against the last snapshot it acknowledged.
//!
//! Transport-free: [`Server::receive`] takes a client's packet and [`Server::tick`] gives the
//! packets to send. A late action repeats the one in force, as the original's current
//! `PlayerAction` does.

use std::collections::{BTreeMap, VecDeque};

use bytefield_sim::Sim;
use bytefield_sim::replica::{Action, Axes, Delta, Exact, ObjectDelta, PredictionMode, Snapshot, apply_action, prediction_mode};
use serde::{Deserialize, Serialize};

pub mod udp;

/// The server's step.
pub const TICK: f64 = 1.0 / 30.0;

/// The round as the client's own (`play::start`): conquest, and `bots` bots by turns (`bots`, or
/// the other ones if this build has them and `better`).
pub fn round(world: &'static bytefield_sim::World, bots: usize, better: bool) -> Sim<'static> {
    let mut sim = Sim::new(world, TICK);
    sim.round.playing = true;
    sim.start_conquest();
    if bots > 0 {
        sim.enable_bots();
        if better {
            sim.enable_better_bots();
        }
        // (as `gameStatusPlaying` fills the server: each to `getTeamToAddPlayer`'s team)
        for i in 0..bots {
            let p = sim.join(0, true);
            sim.players[p].kit = (i / 2) % 5;
        }
        // (the places they fill: a person who joins takes one from a bot of the longer team as it dies, and one who leaves gives it back)
        sim.rules.bot_places = bots;
    }
    sim
}

/// A client's copy of a server's round ([`round`] with no bots): `remote`, its projectiles hit for the eye and give no damage, the deaths
/// are the server's ([`Snapshot::write`]). Before, its own hits killed soldiers the server had not, again and again.
pub fn copy_round(world: &'static bytefield_sim::World) -> Sim<'static> {
    let mut sim = round(world, 0, false);
    sim.role.remote = true;
    sim
}

/// `game.serverGameStartDelay` in the game's `ServerSettings.con`: each round's pre-game, in seconds.
pub const START_DELAY: f32 = 20.0;

/// A server's rules over [`round`]: each round's pre-game of `start_delay` seconds and the least
/// people (`setMinNrOfPlayers`, 2, [`Sim::min_players`]); none in coop or with bots, as the
/// original's `game.isAiLevel`.
pub fn hosted(sim: &mut Sim, mode: &str, bots: usize, start_delay: f32, min_players: Option<u32>) {
    sim.start_pregame(start_delay);
    sim.rules.min_players = min_players.unwrap_or(if bots > 0 || mode.eq_ignore_ascii_case("coop") { 0 } else { 2 });
}

/// The snapshots kept to diff against (2 s at 30 Hz): a client further behind gets a whole one.
pub const HISTORY: usize = 60;
/// Every how many steps the server trims `Sim`'s logs, and how many entries of each it keeps (each log at most
/// twice this between trims; the radio's are read back `HISTORY` steps at most)
pub const LOG_TRIM_EVERY: u64 = 300;
pub const LOG_KEEP: usize = 4096;

/// A client's packet: the last snapshot it has, and its last actions by step (the few last
/// again in every packet, so one lost costs nothing).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ClientPacket {
    pub ack: Option<u64>,
    pub actions: Vec<(u64, Axes)>,
    /// its radio picks not yet taken: (number, menu, key), from the first not taken
    pub radio: Vec<(u32, u8, u8)>,
    /// its choice for its next spawn (the screen when dead): spawn group (None: the first of
    /// its team) and kit; the latest goes in every packet
    pub spawn: Option<(Option<i64>, u8)>,
    /// still choosing on that screen (its first spawn waits for it: [`bytefield_sim::players::Player::hold`])
    pub hold: bool,
    /// its chat lines not yet taken: (number, to all or to its team, text), from the first not taken
    pub chat: Vec<(u32, bool, String)>,
}

impl ClientPacket {
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("a packet encodes")
    }
    pub fn decode(b: &[u8]) -> Option<ClientPacket> {
        use bincode::Options;
        // (as `bincode::deserialize`, but an impossible length is not even looked at)
        bincode::DefaultOptions::new().with_fixint_encoding().allow_trailing_bytes().with_limit(b.len() as u64).deserialize(b).ok()
    }
}

/// The most characters in a chat line.
pub const CHAT_LINE: usize = 63;
/// The most in a player's name.
pub const NAME_LINE: usize = 31;
/// Actions a client may have queued ahead of the round (6 s).
const MOST_QUEUED: usize = 180;
/// How far ahead an action may be labelled (the same).
const MOST_AHEAD: u64 = 180;
/// Radio picks waiting from one client.
const MOST_RADIO: usize = 16;
/// Seconds between one client's chat lines: what comes faster is not said.
const CHAT_EVERY: f64 = 0.5;
/// Chat lines kept to send again.
const CHAT_KEPT: usize = 64;

/// What is safe to show of what someone typed: no control characters, and no longer than `most`.
pub fn clean_text(t: &str, most: usize) -> String {
    t.chars().filter(|c| !c.is_control()).take(most).collect::<String>().trim().to_string()
}

struct ChatLine {
    step: u64,
    /// 0: to all
    team: u32,
    text: String,
}

struct Client {
    player: usize,
    queue: BTreeMap<u64, Axes>,
    current: Axes,
    /// the step of `current` (an action for a step already past is still newer than it)
    current_step: u64,
    acked: Option<u64>,
    /// what it was sent, by step (the snapshot with what had no room as before): the bases
    sent: VecDeque<Snapshot>,
    /// by object: how long it has waited for room, weighed by how near it is
    waited: Vec<f32>,
    /// its radio picks taken, and those come and not yet taken (menu, key)
    radio_taken: u32,
    radio: Vec<(u8, u8)>,
    chat_taken: u32,
    /// when it last said something
    chat_at: f64,
    /// it left: its place is for the next to come
    gone: bool,
}

pub struct Server<'w> {
    pub sim: Sim<'w>,
    clients: Vec<Client>,
    chat: VecDeque<ChatLine>,
    history: VecDeque<Snapshot>,
    /// bytes a packet may take (None: all that changed goes)
    pub budget: Option<usize>,
    /// the level's view distance: what a client can see of the world (m)
    pub view: f32,
    /// send a snapshot every this many steps (1: all 30 a second)
    pub every: u64,
    /// where the bytes of the packets went (`Some`: counted; the load test)
    pub sizes: Option<Sizes>,
}

/// Where the bytes of the packets went, by part.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sizes {
    pub packets: u64,
    pub total: u64,
    /// the receiver's own objects, unrounded
    pub exact: u64,
    pub actions: u64,
    pub players: u64,
    pub stale: u64,
    /// chat and radio
    pub talk: u64,
    /// the objects' deltas: the receiver's own, those that changed state, the new, and those that only moved
    pub own: u64,
    pub state: u64,
    pub new: u64,
    pub moved: u64,
    /// fields of the objects
    pub pos: u64,
    pub rot: u64,
    pub vel: u64,
    pub angles: u64,
    /// the first packets of a client (against nothing): how many, the biggest
    pub first: u64,
    pub first_biggest: usize,
    /// the objects there are, the most seen
    pub objects: usize,
}

impl Sizes {
    fn count(&mut self, d: &Delta, own: &[usize], base_len: usize, first: bool, n: usize) {
        self.packets += 1;
        self.total += n as u64;
        self.exact += Delta::field_size(&d.exact) as u64;
        self.actions += Delta::field_size(&d.actions) as u64;
        self.players += (Delta::field_size(&d.players) + Delta::field_size(&d.names)) as u64;
        self.stale += (d.stale.iter().max().map_or(0, |m| *m as usize / 8 + 1) + 1) as u64;
        self.talk += (Delta::field_size(&d.chat) + Delta::field_size(&d.radio)) as u64;
        for o in &d.objects {
            let (k, b) = (o.0 as usize, Delta::object_size(o) as u64);
            let x = &o.1;
            *(if own.contains(&k) {
                &mut self.own
            } else if k >= base_len {
                &mut self.new
            } else if x.template.is_some() || x.born.is_some() || x.gone.is_some() || x.inside.is_some() || x.seat.is_some() || x.team.is_some() || x.hp.is_some() {
                &mut self.state
            } else {
                &mut self.moved
            }) += b;
            self.pos += x.pos.as_ref().map_or(0, Delta::field_size) as u64;
            self.rot += x.rot.as_ref().map_or(0, Delta::field_size) as u64;
            self.vel += (x.vel.as_ref().map_or(0, Delta::field_size) + x.rotvel.as_ref().map_or(0, Delta::field_size)) as u64;
            self.angles += x.angles.as_ref().map_or(0, Delta::field_size) as u64;
        }
        if first {
            self.first += 1;
            self.first_biggest = self.first_biggest.max(n);
        }
        self.objects = self.objects.max(d.len as usize);
    }
}

/// A packet's bytes: what goes in one UDP datagram without being split on the way.
pub const BUDGET: usize = 1200;
/// The most an axis of a client's action can be worth (a person's mouse in a frame stays far under it).
pub const MAX_AXIS: f32 = 10_000.0;
/// Up to this far (m) an object is as urgent as the nearest; further, less by distance.
pub const NEAR: f32 = 50.0;

impl<'w> Server<'w> {
    pub fn new(sim: Sim<'w>) -> Self {
        let view = view_distance(sim.world);
        Server { view, sim, clients: Vec::new(), chat: VecDeque::new(), history: VecDeque::new(), budget: Some(BUDGET), every: 1, sizes: None }
    }

    /// A client playing `player` (an index in `sim.players`); its id.
    pub fn connect(&mut self, player: usize) -> usize {
        let c = Client { player, queue: BTreeMap::new(), current: Vec::new(), current_step: 0, acked: None, sent: VecDeque::new(), waited: Vec::new(), radio_taken: 0, radio: Vec::new(), chat_taken: 0, chat_at: f64::NEG_INFINITY, gone: false };
        if let Some(k) = self.clients.iter().position(|c| c.gone) {
            self.clients[k] = c;
            return k;
        }
        self.clients.push(c);
        self.clients.len() - 1
    }

    /// `client` left: its soldier goes, and its places are for whoever comes next.
    pub fn leave(&mut self, client: usize) {
        let Some(c) = self.clients.get_mut(client).filter(|c| !c.gone) else { return };
        c.gone = true;
        c.sent.clear();
        c.queue.clear();
        let p = c.player;
        self.sim.remove_player(p);
    }

    /// A new player of `team` (0: the shorter, `getTeamToAddPlayer`) with kit `kit` (a person: `ai`
    /// false), played by a new client; (client, player).
    pub fn join(&mut self, team: u32, kit: usize) -> (usize, usize) {
        let p = self.sim.join_person(team);
        self.sim.players[p].kit = kit;
        (self.connect(p), p)
    }

    /// The name of `client`'s player, as others will see it; one already taken gets a number after it.
    pub fn set_name(&mut self, client: usize, name: &str) {
        let name = clean_text(name, NAME_LINE);
        let p = self.clients[client].player;
        let taken = |n: &str, players: &[bytefield_sim::players::Player]| players.iter().enumerate().any(|(k, q)| k != p && !q.kicked && q.name.eq_ignore_ascii_case(n));
        let base = if name.is_empty() { "Player".to_string() } else { name };
        let unique = (1..).map(|k| if k == 1 { base.clone() } else { format!("{base}{k}") }).find(|n| !taken(n, &self.sim.players)).unwrap();
        self.sim.players[p].name = unique;
    }

    pub fn player(&self, client: usize) -> usize {
        self.clients[client].player
    }

    pub fn receive(&mut self, client: usize, packet: &[u8]) {
        let Some(p) = ClientPacket::decode(packet) else { return };
        if self.clients[client].gone {
            return;
        }
        if let Some((g, kit)) = p.spawn {
            self.sim.choose_spawn(self.clients[client].player, g, kit as usize);
        }
        self.sim.hold_spawn(self.clients[client].player, p.hold);
        let c = &mut self.clients[client];
        let next = self.sim.clock.steps + 1;
        if p.ack > c.acked {
            c.acked = p.ack;
        }
        for (step, mut axes) in p.actions {
            // (anyone can send anything: an axis that does not exist or is not a number stops here)
            axes.retain(|&(i, v)| i < 64 && v.is_finite());
            for a in &mut axes {
                a.1 = a.1.clamp(-MAX_AXIS, MAX_AXIS);
            }
            if step >= next {
                // (not without end, nor for a step that will never come)
                if step <= next + MOST_AHEAD && c.queue.len() < MOST_QUEUED {
                    c.queue.insert(step, axes);
                }
            } else if step > c.current_step {
                // late: the newest of them is in force from the next step
                c.current = axes;
                c.current_step = step;
            }
        }
        for (n, f, k) in p.radio {
            if c.radio.len() < MOST_RADIO && n == c.radio_taken.wrapping_add(c.radio.len() as u32) {
                c.radio.push((f, k));
            }
        }
        let time = self.sim.clock.time;
        let player = c.player;
        let mut said = Vec::new();
        for (n, to_all, text) in p.chat {
            if n != c.chat_taken {
                continue;
            }
            c.chat_taken = c.chat_taken.wrapping_add(1);
            let text = clean_text(&text, CHAT_LINE);
            if !text.is_empty() && time - c.chat_at >= CHAT_EVERY {
                c.chat_at = time;
                said.push((to_all, text));
            }
        }
        for (to_all, text) in said {
            self.say(player, to_all, &text);
        }
    }

    fn say(&mut self, player: usize, to_all: bool, text: &str) {
        let Some(p) = self.sim.players.get(player) else { return };
        let team = if to_all { 0 } else { p.team };
        let line = if to_all { format!("{}: {text}", p.name) } else { format!("({}) {text}", p.name) };
        self.chat.push_back(ChatLine { step: self.sim.clock.steps + 1, team, text: line });
        if self.chat.len() > CHAT_KEPT {
            self.chat.pop_front();
        }
    }

    /// One step with every client's action for it; the packets to send, by client.
    pub fn tick(&mut self) -> Vec<(usize, Vec<u8>)> {
        let next = self.sim.clock.steps + 1;
        for c in self.clients.iter_mut().filter(|c| !c.gone) {
            let rest = c.queue.split_off(&(next + 1));
            if let Some((step, a)) = std::mem::replace(&mut c.queue, rest).into_iter().next_back() {
                c.current = a;
                c.current_step = step;
            }
            apply_action(&mut self.sim, c.player, &c.current);
        }
        self.sim.step();
        // its logs only grew: the last `LOG_KEEP` of each (the radio's newest are what the snapshots send, by step)
        if self.sim.clock.steps.is_multiple_of(LOG_TRIM_EVERY) {
            self.sim.trim_logs(LOG_KEEP);
        }
        // (after the step: said in the step of the snapshot that carries it)
        for c in self.clients.iter_mut().filter(|c| !c.gone) {
            for (f, k) in c.radio.drain(..) {
                self.sim.radio_pick(c.player, f as u32, k as u32);
                c.radio_taken += 1;
            }
        }
        let snap = Snapshot::of(&self.sim);
        let mut out = Vec::new();
        let send_now = self.every <= 1 || snap.step.is_multiple_of(self.every);
        for (i, c) in self.clients.iter_mut().enumerate().filter(|(_, c)| !c.gone && send_now) {
            let base = c.acked.and_then(|s| c.sent.iter().find(|h| h.step == s));
            let mut d = snap.delta(base);
            d.yours = c.current_step;
            let after = base.map_or(snap.step.saturating_sub(HISTORY as u64), |b| b.step);
            let new = self.sim.journal.radio_log.iter().rev().take_while(|e| e.step > after).count();
            d.radio = self.sim.journal.radio_log[self.sim.journal.radio_log.len() - new..].iter().map(|e| e.said()).collect();
            d.radio_taken = c.radio_taken;
            let team = self.sim.players[c.player].team;
            d.chat = self.chat.iter().filter(|l| l.step > after && (l.team == 0 || l.team == team)).map(|l| (l.step, l.text.clone())).collect();
            d.chat_taken = c.chat_taken;
            let own = own(&self.sim, c.player);
            d.exact = own.iter().map(|&k| (k as u32, Exact::of(&self.sim.vehicles[k]))).collect();
            if let Some(budget) = self.budget {
                fit(&mut d, &self.sim, &own, base.map_or(0, |b| b.objects.len()), base.is_none_or(|b| b.partial), &mut c.waited, budget, self.view);
            }
            let sent = Snapshot::apply(base, &d);
            let bytes = d.encode();
            if let Some(z) = &mut self.sizes {
                z.count(&d, &own, base.map_or(0, |b| b.objects.len()), base.is_none(), bytes.len());
            }
            out.push((i, bytes));
            c.sent.push_back(sent);
            if c.sent.len() > HISTORY {
                c.sent.pop_front();
            }
        }
        self.history.push_back(snap);
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
        out
    }

    pub fn latest(&self) -> Option<&Snapshot> {
        self.history.back()
    }

    pub fn snapshot_at(&self, step: u64) -> Option<&Snapshot> {
        self.history.iter().find(|h| h.step == step)
    }

    /// What `client` was sent of step `step` (as [`Self::snapshot_at`] but for what had no room).
    pub fn sent_at(&self, client: usize, step: u64) -> Option<&Snapshot> {
        self.clients[client].sent.iter().find(|h| h.step == step)
    }
}

/// `d` cut to `budget` bytes, which it never passes, so that no datagram is split on the way (a frame lost would lose all of
/// it). What has no room is not lost: it still differs from what was acknowledged, and goes in a later packet. In order: the receiver's own
/// objects and action, what was said (oldest first), the objects whose kind of state changed (template, gone, in a vehicle, team, health),
/// the new objects (in order: the list only grows as far as they fit) and the players; of the rest, those that have waited longest, the
/// waiting weighed by nearness to the receiver's soldier (`NetworkableInfo.basePriority` is `c_NIGhostAlways` only for the flags in bf1942,
/// and the flags always go). A player's action goes with its soldier (left out, the receiver keeps the one before, as the original
/// repeats its `PlayerAction`). An object with no room is `stale`: the receiver keeps its own step of it. Two thirds at most for the
/// first group, so what waits always gets its turn; a client that is joining gets the whole packet for its world, over several packets.
fn fit(d: &mut Delta, sim: &Sim, own: &[usize], base_len: usize, joining: bool, waited: &mut Vec<f32>, budget: usize, view: f32) {
    /// what a list's length may grow by, and the bitset's rounding
    const MARGIN: isize = 24;
    waited.resize(sim.vehicles.len(), 0.0);
    let turn = d.step as usize;
    let objects = std::mem::take(&mut d.objects);
    let all_actions = std::mem::take(&mut d.actions);
    let mut players = std::mem::take(&mut d.players);
    let mut names = std::mem::take(&mut d.names);
    let mut talk: Vec<(u64, usize)> = d.radio.iter().map(|e| (e.step, Delta::field_size(e))).chain(d.chat.iter().map(|l| (l.0, Delta::field_size(l)))).collect();
    talk.sort();
    let (radio, chat) = (std::mem::take(&mut d.radio), std::mem::take(&mut d.chat));

    // what cannot be split, the bitset of the stale at its largest and the receiver's own
    let mut left = budget as isize - d.size() as isize - (base_len / 8 + 1) as isize - MARGIN;
    let mut changed = Vec::new();
    let mut new = Vec::new();
    let mut rest: BTreeMap<usize, (Option<ObjectDelta>, Vec<(u32, Option<Action>)>)> = BTreeMap::new();
    let mut must = Vec::new();
    for (i, o) in objects {
        let k = i as usize;
        if k >= base_len {
            new.push((i, o));
        } else if own.contains(&k) {
            let n = Delta::object_size(&(i, o.clone())) as isize;
            // (what is exact goes in `exact` too: if there is no room for this, the copy still has that)
            if n <= left {
                waited[k] = 0.0;
                left -= n;
                must.push((i, o));
            } else {
                d.stale.push(i);
            }
        } else if o.template.is_some() || o.born.is_some() || o.gone.is_some() || o.inside.is_some() || o.seat.is_some() || o.team.is_some() || o.hp.is_some() {
            changed.push((i, o));
        } else {
            rest.entry(k).or_default().0 = Some(o);
        }
    }
    let mut actions = Vec::new();
    let mut other_actions = Vec::new();
    for (p, a) in all_actions {
        match sim.players.get(p as usize).and_then(|q| q.soldier) {
            Some(k) if own.contains(&k) => {
                left -= Delta::action_size(&(p, a.clone())) as isize;
                actions.push((p, a));
            }
            Some(k) if k < base_len => rest.entry(k).or_default().1.push((p, a)),
            _ => other_actions.push((p, a)),
        }
    }
    // the first group's share (the rest of the packet is for what waits)
    let mut first = if joining { left } else { left.min(budget as isize * 2 / 3) };
    let spend = |n: usize, left: &mut isize, first: &mut isize| -> bool {
        let n = n as isize;
        if n > (*first).min(*left) {
            return false;
        }
        *left -= n;
        *first -= n;
        true
    };

    // what was said: oldest first, as far as it fits, a whole step at a time
    let mut upto = d.step;
    let mut i = 0;
    while i < talk.len() {
        let step = talk[i].0;
        let n: usize = talk[i..].iter().take_while(|t| t.0 == step).map(|t| t.1).sum();
        if !spend(n, &mut left, &mut first) {
            upto = step - 1;
            break;
        }
        i += talk[i..].iter().take_while(|t| t.0 == step).count();
    }
    d.radio = radio.into_iter().filter(|e| e.step <= upto).collect();
    d.chat = chat.into_iter().filter(|l| l.0 <= upto).collect();
    d.talk_upto = upto;

    // (from a place that changes with the step: none is last for ever)
    let spin = changed.len().max(1);
    changed.rotate_left(turn % spin);
    for (i, o) in changed {
        let k = i as usize;
        if spend(Delta::object_size(&(i, o.clone())), &mut left, &mut first) {
            waited[k] = 0.0;
            must.push((i, o));
        } else {
            d.stale.push(i);
        }
    }

    // the new ones, without a gap: `len` says where they stop
    let mut len = base_len as u32;
    let mut held = false;
    for (i, o) in new {
        // (one with no delta is all default: what growing the list makes)
        if !held && spend(Delta::object_size(&(i, o.clone())), &mut left, &mut first) {
            waited[i as usize] = 0.0;
            must.push((i, o));
            len = i + 1;
        } else {
            held = true;
        }
    }
    if held {
        d.len = d.len.min(len);
    }
    let there = |k: Option<u32>, n: u32| k.is_none_or(|k| k < n);

    // the players (none whose soldier the receiver does not have yet)
    let spin = players.len().max(1);
    players.rotate_left(turn % spin);
    let mut sent_players = Vec::new();
    for p in players {
        if there(p.1.soldier, d.len) && spend(Delta::field_size(&p), &mut left, &mut first) {
            sent_players.push(p);
        } else {
            d.stale_players.push(p.0);
            held = true;
        }
    }
    d.stale_players.sort();
    sent_players.sort_by_key(|p| p.0);
    d.players = sent_players;
    names.retain(|n| {
        let fits = spend(Delta::field_size(n), &mut left, &mut first);
        held |= !fits;
        fits
    });
    d.names = names;
    // the things on the ground
    let items = std::mem::take(&mut d.items);
    let gone = std::mem::take(&mut d.items_gone);
    for t in items {
        if spend(Delta::field_size(&t), &mut left, &mut first) {
            d.items.push(t);
        } else {
            held = true;
        }
    }
    for t in gone {
        if spend(Delta::field_size(&t), &mut left, &mut first) {
            d.items_gone.push(t);
        }
    }
    d.partial = joining && held;
    for (p, a) in other_actions {
        let k = sim.players.get(p as usize).and_then(|q| q.soldier);
        if k.is_none_or(|k| k < d.len as usize) && spend(Delta::action_size(&(p, a.clone())), &mut left, &mut first) {
            actions.push((p, a));
        }
    }

    d.objects = must;
    d.actions = actions;
    let eye = own.last().map(|&k| {
        let (v, b) = (&sim.vehicles[k], &sim.vehicles[k].craft.body);
        (b.pos, v.soldier().map_or(b.rows[2], |m| m.view(b, [0.0; 2]).0[2]))
    });
    for &k in rest.keys() {
        waited[k] += eye.map_or(1.0, |(e, ahead)| weight(&sim.vehicles[k].craft.body, e, ahead, view));
    }
    // what is out of sight waits for its turn, not for room
    let far: Vec<usize> = eye.map_or(Vec::new(), |(e, _)| rest.keys().copied().filter(|&k| waited[k] < 1.0 && out_of_sight(sim.vehicles[k].craft.body.pos, e, view)).collect());
    for k in far {
        if let Some((Some(_), _)) = rest.remove(&k) {
            d.stale.push(k as u32);
        }
    }
    let mut rest: Vec<_> = rest.into_iter().collect();
    rest.sort_by(|a, b| waited[b.0].total_cmp(&waited[a.0]));
    for (k, (o, acts)) in rest {
        let n = o.as_ref().map_or(0, |o| Delta::object_size(&(k as u32, o.clone()))) + acts.iter().map(Delta::action_size).sum::<usize>();
        if n as isize <= left {
            left -= n as isize;
            waited[k] = 0.0;
            d.objects.extend(o.map(|o| (k as u32, o)));
            d.actions.extend(acts);
        } else if o.is_some() {
            d.stale.push(k as u32);
        }
    }
    d.objects.sort_by_key(|o| o.0);
    d.actions.sort_by_key(|a| a.0);
    d.stale.sort();
}

/// The level's view distance: its last `Game.setViewDistance`, or 800 (the client's far plane, `level.rs`).
fn view_distance(world: &bytefield_sim::World) -> f32 {
    world.db.globals.iter().rev().find(|c| c.command.eq_ignore_ascii_case("game.setViewDistance")).and_then(|c| c.args.first()?.parse().ok()).unwrap_or(800.0)
}

/// How far past the view distance a client still hears of an object. It was once taken for 50.
const SIGHT_MARGIN: f32 = 20.0;
/// An object out of sight gets a turn this often (steps): the minimap's, which the original did not need.
const OUT_OF_SIGHT_EVERY: f32 = 20.0;

fn out_of_sight(p: [f32; 3], eye: [f32; 3], view: f32) -> bool {
    let d2 = (p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2);
    d2 > (view + SIGHT_MARGIN).powi(2)
}

/// What a step of waiting is worth for an object the receiver may hear about, in the form of the original's priority function
///: in front of its eye 1, the nearness over the view, the speed (full at 40 m/s). Without what the receiver can
/// never see, which [`out_of_sight`] leaves to its own turn at [`OUT_OF_SIGHT_EVERY`]. Ours: the soldier's or vehicle's front, and a half-angle of 60
/// degrees.
// TODO: the original's is the server's own camera of the player, with the cosine's bound
//: port the client's camera here.
fn weight(b: &bytefield_sim::rigid::RigidBody, eye: [f32; 3], ahead: [f32; 3], view: f32) -> f32 {
    let to = [b.pos[0] - eye[0], b.pos[1] - eye[1], b.pos[2] - eye[2]];
    let d = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
    let range = view + SIGHT_MARGIN;
    if d > range {
        return 1.0 / OUT_OF_SIGHT_EVERY;
    }
    let cos = (to[0] * ahead[0] + to[1] * ahead[1] + to[2] * ahead[2]) / d.max(1e-3);
    let speed2 = b.vel[0] * b.vel[0] + b.vel[1] * b.vel[1] + b.vel[2] * b.vel[2];
    f32::from(cos >= 0.5) + (1.0 - d / range) + (speed2 / 1600.0).min(1.0)
}

/// What `player` predicts: its live soldier and the vehicle it is in.
fn own(sim: &Sim, player: usize) -> Vec<usize> {
    let Some(i) = sim.players.get(player).and_then(|p| p.soldier.filter(|_| p.alive)) else { return Vec::new() };
    std::iter::once(i).chain(sim.vehicles[i].entry.inside).collect()
}

/// A client's copy of the world: the snapshots it rebuilt (the newest, and those the server
/// may still diff against), and its actions not yet past.
#[derive(Default)]
pub struct Mirror {
    pub snapshots: BTreeMap<u64, Snapshot>,
    /// by step: its own objects unrounded (written over the snapshot's)
    exact: BTreeMap<u64, Vec<(u32, Exact)>>,
    /// the actions sent and not yet confirmed (the latest snapshot's `yours` and before go)
    sent: VecDeque<(u64, Axes)>,
    /// the label of this client's action in force in the latest snapshot
    pub confirmed: u64,
    newest: Option<Axes>,
    /// what was said on the radio, as it came (the copy's `radio_log` follows it), the newest [`LOG_KEEP`] or more; how
    /// many came before them
    pub radio: Vec<bytefield_sim::radio::RadioEvent>,
    pub radio_base: usize,
    /// the chat lines that came, as they came, the newest [`LOG_KEEP`] or more; how many came before them
    pub chat: Vec<String>,
    pub chat_base: usize,
    /// this client's chat lines (to all, text), and how many of them the server took
    chat_out: Vec<(bool, String)>,
    chat_taken: u32,
    /// the step of the newest packet taken (what was said up to it is here)
    radio_upto: u64,
    /// this client's radio picks (menu, key), and how many of them the server took
    radio_out: Vec<(u8, u8)>,
    radio_taken: u32,
    /// the choice for the next spawn (spawn group, kit), once made: in every packet
    pub spawn: Option<(Option<i64>, u8)>,
    /// still choosing where and with what to spawn the first time: in every packet
    pub hold: bool,
}

/// Below this speed (m/s, rad/s) a stale object is at rest in the copy (see [`follow`]).
const REST_SPEED: f32 = 1.0;
/// What a stale object may move in the copy's step besides three times its speed (m): further is the copy's spawner putting it back, not a step.
const JUMP: f32 = 2.0;

fn norm2(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// Past twice [`LOG_KEEP`], a mirror's log keeps its newest `LOG_KEEP` and counts the rest in `base` (its readers count from
/// the round's start).
fn trim_log<T>(log: &mut Vec<T>, base: &mut usize) {
    if log.len() > 2 * LOG_KEEP {
        let cut = log.len() - LOG_KEEP;
        log.drain(..cut);
        *base += cut;
    }
}

/// A copy further behind than this many steps jumps to the latest snapshot.
pub const CATCH_UP: u64 = 15;

/// The client's copy of the world (`copy`, a `Sim` of the same round) up to the mirror's
/// latest snapshot: each step with the players' actions of it, then that step's snapshot
/// written over (what the copy's step does not get right: the contacts' state it lacks; the
/// step is for what the snapshot does not carry: animations, shots, triggers). The objects
/// the copy could not match (0 if all is well).
pub fn follow(copy: &mut Sim, mirror: &Mirror) -> usize {
    // (the copy's radio log is cut as the others, `trim_logs`, and so is the mirror's, `trim_log`: both counted from the
    // round's start; the copy follows every frame, far less behind than `LOG_KEEP`)
    if let Some(new) = mirror.radio.get((copy.journal.log_base.radio + copy.journal.radio_log.len()).saturating_sub(mirror.radio_base)..) {
        copy.journal.radio_log.extend_from_slice(new);
    }
    let Some(last) = mirror.latest() else { return 0 };
    if last.step <= copy.clock.steps {
        return 0;
    }
    if last.step - copy.clock.steps > CATCH_UP {
        let odd = last.write(copy);
        mirror.write_exact(copy, last.step);
        return odd;
    }
    let mut odd = 0;
    while copy.clock.steps < last.step {
        let k = copy.clock.steps + 1;
        // (a step whose snapshot is lost: the next one's actions)
        let (_, s) = mirror.snapshots.range(k..).next().expect("the latest is there");
        s.act(copy);
        // A stale object is the copy's to step, within reason: stepped from a rounded state and with no contacts to
        // hold it a parked vehicle left the ground at 11 m/s, and a landing craft's spawner on a carrier put it back there (DaiHatsu, 30–70 m)
        let before: Vec<_> = s.stale.iter().filter_map(|&i| {
            let (o, v) = (s.objects.get(i as usize)?, copy.vehicles.get(i as usize)?);
            o.inside.is_none().then(|| (i as usize, v.craft.body.pos, v.craft.body.rows, norm2(o.vel) < REST_SPEED * REST_SPEED && norm2(o.rotvel) < REST_SPEED * REST_SPEED, o.vel))
        }).collect();
        copy.step();
        let dt = copy.clock.dt as f32;
        for (i, pos, rows, resting, vel) in before {
            let b = &mut copy.vehicles[i].craft.body;
            let d = [b.pos[0] - pos[0], b.pos[1] - pos[1], b.pos[2] - pos[2]];
            if resting {
                (b.pos, b.rows, b.vel, b.rotvel) = (pos, rows, [0.0; 3], [0.0; 3]);
            } else if norm2(d).sqrt() > JUMP + 3.0 * norm2(vel).sqrt() * dt {
                (b.pos, b.vel) = ([pos[0] + vel[0] * dt, pos[1] + vel[1] * dt, pos[2] + vel[2] * dt], vel);
            }
        }
        if s.step == k {
            odd += s.write(copy);
            mirror.write_exact(copy, k);
        }
    }
    odd
}

/// Actions sent again in each packet.
pub const RESEND: usize = 8;

/// Unconfirmed actions kept to predict with (2 s: a longer trip predicts no further).
pub const PENDING: usize = 60;

/// The client's own soldier ahead of the copy: after [`follow`], its actions
/// the server has not yet taken stepped again on it alone ([`Sim::step_alone`]: its parts,
/// its movement, its contacts with the level); in a vehicle, on the vehicle (its seat's
/// input from them), with the soldier on its seat. Before the next `follow` the copy gets
/// them back as the snapshot left them, so a wrong guess lasts until then.
///
/// A correction is not shown at once: where this prediction lands against where the last one
/// leads (it a step further with the newest action) is an `offset` the shown object
/// carries and that fades ([`FADE`] per step; a jump of [`SNAP`] m or more is shown as is).
///
/// The others are shown where they will be as far ahead as ours (`NetworkableInfo`'s
/// prediction mode: a `PMLinear` body moved on with its speed, the soldiers in it on their
/// seats; `PMNone` as the snapshot left it; `PMUsePhysics`, only parts in bf1942, as linear).
#[derive(Default)]
pub struct Prediction {
    held: Vec<(usize, bytefield_sim::mover::Vehicle)>,
    /// the steps predicted the last time
    pub steps: usize,
    /// the object shown (the soldier or its vehicle) and the soldier
    shown: Option<(usize, usize)>,
    /// what the shown object is moved by (the corrections still fading)
    pub offset: [f32; 3],
    /// the others moved on, and where the snapshot left them
    moved: Vec<(usize, [f32; 3])>,
    /// by object: its template's prediction mode
    modes: Vec<PredictionMode>,
}

/// What is left of a correction after each step (a third after 5 steps).
pub const FADE: f32 = 0.8;
/// A correction this long (m) or more is a jump (a respawn, a seat): not smoothed.
pub const SNAP: f32 = 4.0;

impl Prediction {
    /// The copy's soldier as the snapshots left it (the offset goes with it).
    pub fn restore(&mut self, copy: &mut Sim) {
        self.shown = None;
        for (i, p) in self.moved.drain(..).rev() {
            copy.vehicles[i].craft.body.pos = p;
        }
        for (i, v) in self.held.drain(..) {
            copy.vehicles[i] = v;
        }
    }

    /// Object `k` kept as the snapshot left it, to give back.
    fn hold(&mut self, copy: &Sim, k: usize) {
        if !self.held.iter().any(|h| h.0 == k) {
            self.held.push((k, copy.vehicles[k].clone()));
        }
    }

    /// The shown object moved by the offset (and its soldier on its seat).
    fn place(&self, copy: &mut Sim) {
        let Some((k, i)) = self.shown else { return };
        let b = &mut copy.vehicles[k].craft.body;
        b.pos = [0, 1, 2].map(|j| b.pos[j] + self.offset[j]);
        if k != i {
            copy.ride_one(i);
        }
    }

    /// Where the last prediction leads with the newest action: it a step further (the
    /// copy as it was is restored after).
    fn lead(&mut self, copy: &mut Sim, mirror: &Mirror, player: usize) -> Option<(usize, [f32; 3])> {
        let (k, i) = self.shown.take()?;
        let axes = mirror.newest()?;
        let keep: Vec<_> = [i, k].into_iter().map(|j| (j, copy.vehicles[j].clone())).collect();
        let b = &mut copy.vehicles[k].craft.body;
        b.pos = [0, 1, 2].map(|j| b.pos[j] - self.offset[j]);
        apply_action(copy, player, axes);
        if k != i {
            copy.feed_seats(k);
        }
        let ok = copy.step_alone(k, self.steps as u32);
        let at = copy.vehicles[k].craft.body.pos;
        for (j, v) in keep {
            copy.vehicles[j] = v;
        }
        ok.then_some((k, at))
    }

    /// [`follow`], then `player`'s soldier stepped with the mirror's pending actions.
    pub fn follow(&mut self, copy: &mut Sim, mirror: &Mirror, player: usize) -> usize {
        let expected = self.lead(copy, mirror, player);
        self.restore(copy);
        let odd = follow(copy, mirror);
        self.steps = 0;
        let Some(i) = copy.players.get(player).and_then(|p| p.soldier.filter(|_| p.alive)) else {
            self.offset = [0.0; 3];
            return odd;
        };
        self.hold(copy, i);
        if let Some(k) = copy.vehicles[i].entry.inside {
            self.hold(copy, k);
        }
        for (n, (_, axes)) in mirror.pending().enumerate() {
            apply_action(copy, player, axes);
            // getting in, out or to another seat as the server would (the vehicles it touches
            // are held first)
            if let Some(k) = copy.vehicles[i].entry.inside {
                if copy.vehicles[k].entry.driver.is_some() {
                    copy.vehicles[k].set_engines_on(true);
                }
            } else if let Some((k, _)) = copy.find_entry(i) {
                self.hold(copy, k);
            }
            copy.entry_step(i);
            let stepped = match copy.vehicles[i].entry.inside {
                Some(k) => {
                    copy.feed_seats(k);
                    let ok = copy.step_alone(k, n as u32);
                    copy.ride_one(i);
                    ok
                }
                None => copy.step_alone(i, n as u32),
            };
            if !stepped {
                break;
            }
            self.steps += 1;
        }
        let inside = copy.vehicles[i].entry.inside;
        let shown = inside.unwrap_or(i);
        let pos = copy.vehicles[shown].craft.body.pos;
        // the correction against what the last prediction leads to
        self.offset = self.offset.map(|o| o * FADE);
        match expected {
            Some((k, e)) if k == shown => {
                let d = [0, 1, 2].map(|j| e[j] - pos[j]);
                if d.iter().map(|x| x * x).sum::<f32>() < SNAP * SNAP {
                    self.offset = [0, 1, 2].map(|j| self.offset[j] + d[j]);
                } else {
                    self.offset = [0.0; 3];
                }
            }
            _ => self.offset = [0.0; 3],
        }
        self.shown = Some((shown, i));
        self.place(copy);
        self.move_others(copy, i, inside);
        odd
    }

    /// The others (not `mine` nor the vehicle it is in) moved on as far as ours.
    fn move_others(&mut self, copy: &mut Sim, mine: usize, inside: Option<usize>) {
        let t = self.steps as f32 * copy.clock.dt as f32;
        for k in self.modes.len()..copy.vehicles.len() {
            self.modes.push(prediction_mode(copy.world, &copy.vehicles[k].template));
        }
        let mut riders = Vec::new();
        for (k, v) in copy.vehicles.iter_mut().enumerate() {
            if k == mine || Some(k) == inside || v.removed || !v.in_world() || v.fixed {
                if v.entry.inside.is_some_and(|j| j != mine && Some(j) != inside) && k != mine {
                    riders.push(k);
                }
                continue;
            }
            if self.modes[k] == PredictionMode::None {
                continue;
            }
            let b = &mut v.craft.body;
            self.moved.push((k, b.pos));
            b.pos = [0, 1, 2].map(|j| b.pos[j] + b.vel[j] * t);
        }
        for k in riders {
            self.moved.push((k, copy.vehicles[k].craft.body.pos));
            copy.ride_one(k);
        }
    }
}

impl Mirror {
    /// The newest whole snapshot (a client still joining has none).
    pub fn latest(&self) -> Option<&Snapshot> {
        self.snapshots.values().rev().find(|s| !s.partial)
    }

    /// The newest step that came, whole or not: what to acknowledge.
    fn newest_step(&self) -> Option<u64> {
        self.snapshots.keys().next_back().copied()
    }

    /// A server packet: rebuilt when its base is here (else dropped: the next one will be
    /// against what this mirror acknowledged).
    pub fn receive(&mut self, packet: &[u8]) {
        let Some(d) = Delta::decode(packet).filter(|d| d.sane()) else { return };
        if self.snapshots.contains_key(&d.step) || self.newest_step().is_some_and(|l| l > d.step) {
            return;
        }
        let base = match d.base {
            Some(b) => match self.snapshots.get(&b) {
                Some(s) => Some(s),
                None => return,
            },
            None => None,
        };
        let s = Snapshot::apply(base, &d);
        // the server diffs against what was acked; older than the base it used is no use
        if let Some(b) = d.base {
            self.snapshots = self.snapshots.split_off(&b);
        }
        while self.snapshots.len() > HISTORY {
            self.snapshots.pop_first();
        }
        self.snapshots.insert(d.step, s);
        let first = *self.snapshots.keys().next().unwrap();
        self.exact = self.exact.split_off(&first);
        self.exact.insert(d.step, d.exact);
        self.confirmed = d.yours;
        let upto = self.radio_upto;
        self.radio.extend(d.radio.iter().filter(|e| e.step > upto).filter_map(|e| e.event()));
        self.chat.extend(d.chat.iter().filter(|e| e.0 > upto).map(|e| clean_text(&e.1, CHAT_LINE + NAME_LINE + 4)));
        trim_log(&mut self.radio, &mut self.radio_base);
        trim_log(&mut self.chat, &mut self.chat_base);
        self.chat_taken = self.chat_taken.max(d.chat_taken);
        self.radio_upto = self.radio_upto.max(d.talk_upto);
        self.radio_taken = self.radio_taken.max(d.radio_taken);
        while self.sent.front().is_some_and(|a| a.0 <= self.confirmed) {
            self.sent.pop_front();
        }
    }

    /// Its own objects of step `step` unrounded onto the copy.
    fn write_exact(&self, copy: &mut Sim, step: u64) {
        for (k, e) in self.exact.get(&step).into_iter().flatten() {
            if let Some(v) = copy.vehicles.get_mut(*k as usize) {
                e.write(v);
            }
        }
    }

    /// The label for the next action: the step after the latest snapshot, and never one
    /// already used (the server takes a late one at once if it is the newest).
    pub fn next_step(&self) -> u64 {
        let latest = self.latest().map_or(0, |s| s.step) + 1;
        self.sent.back().map_or(latest, |a| latest.max(a.0 + 1))
    }

    /// The last action sent.
    pub fn newest(&self) -> Option<&Axes> {
        self.newest.as_ref()
    }

    /// The actions the server has not yet taken, oldest first (what to predict).
    pub fn pending(&self) -> impl Iterator<Item = &(u64, Axes)> {
        self.sent.iter().filter(|a| a.0 > self.confirmed)
    }

    /// A radio pick: key `k` of menu `f` (it goes in every packet until the server takes it).
    pub fn radio_pick(&mut self, f: u32, k: u32) {
        self.radio_out.push((f as u8, k as u8));
    }

    /// A chat line (it goes in every packet until the server takes it).
    pub fn say(&mut self, text: &str, to_all: bool) {
        self.chat_out.push((to_all, clean_text(text, CHAT_LINE)));
    }

    /// The packet for the action of `step`.
    pub fn send(&mut self, step: u64, axes: Axes) -> Vec<u8> {
        self.newest = Some(axes.clone());
        self.sent.push_back((step, axes));
        while self.sent.len() > PENDING {
            self.sent.pop_front();
        }
        let resend = self.sent.len().saturating_sub(RESEND);
        let t = self.radio_taken;
        let radio = self.radio_out.iter().enumerate().skip(t as usize).map(|(n, &(f, k))| (n as u32, f, k)).collect();
        let c = self.chat_taken;
        let chat = self.chat_out.iter().enumerate().skip(c as usize).map(|(n, (a, t))| (n as u32, *a, t.clone())).collect();
        ClientPacket { ack: self.newest_step(), actions: self.sent.iter().skip(resend).cloned().collect(), radio, spawn: self.spawn, hold: self.hold, chat }.encode()
    }
}

#[cfg(test)]
mod tests {
    /// A mirror's log keeps its newest `LOG_KEEP` past twice that, and counts what it cut.
    #[test]
    fn a_mirror_log_is_cut_and_counted() {
        let (mut log, mut base): (Vec<usize>, usize) = ((0..2 * super::LOG_KEEP).collect(), 0);
        super::trim_log(&mut log, &mut base);
        assert_eq!((log.len(), base), (2 * super::LOG_KEEP, 0));
        log.push(2 * super::LOG_KEEP);
        super::trim_log(&mut log, &mut base);
        assert_eq!((log.len(), base, log[0]), (super::LOG_KEEP, super::LOG_KEEP + 1, super::LOG_KEEP + 1));
    }
}
