//! A session over the sim: the players' input in, one step, the state that replicates
//! out. [`LocalSession`] is the one-machine game (the client owns the `Sim`); a server feeds its
//! `Sim` the same way from its players' `PlayerAction`s.
//!
//! The replicated state is a [`Snapshot`]: per object (by its stable index in `Sim.vehicles`,
//! which never reorders: a gone one stays, `removed`) where it is, how it moves and its parts'
//! angles; the players, flags and tickets. A [`Delta`] carries what changed against a snapshot
//! the receiver has (the last it acknowledged), so a lost packet costs nothing but its step.

use crate::Sim;
use serde::{Deserialize, Serialize};

/// The axes of one step (`PlayerInput` index, value), as [`crate::soldier::Movement::set_input`] takes them.
pub type Axes = Vec<(usize, f32)>;

pub trait Session {
    /// The player's input for the next step (kept until another comes, as the original keeps
    /// its `PlayerAction`).
    fn set_action(&mut self, player: usize, axes: &[(usize, f32)]);
    fn step(&mut self);
    fn snapshot(&self) -> Snapshot;
}


/// Whose game a simulation is: its own (a server's, a single player's), or a copy of a server's. Written by the server
/// and the client when they make the round, and by [`Snapshot::write`]; the systems only read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Role {
    /// A copy of another's game: its projectiles hit and explode for the eye, with no damage nor push (the server's to give).
    pub remote: bool,
    /// A copy of a server's game (it was given a [`Snapshot`]): its objects' places are the server's, so it never takes a
    /// free one itself ([`Sim::spawn_vehicle`]).
    pub copy: bool,
}

/// The game on one machine: the input goes straight to the player's soldier.
pub struct LocalSession<'w> {
    pub sim: Sim<'w>,
}

impl<'w> LocalSession<'w> {
    pub fn new(sim: Sim<'w>) -> Self {
        LocalSession { sim }
    }
}

/// The input to a player's soldier, when it has a live one (in a vehicle, the soldier's
/// movement takes it to its seat).
pub fn apply_action(sim: &mut Sim, player: usize, axes: &[(usize, f32)]) {
    let Some(p) = sim.players.get(player) else { return };
    if let Some(i) = p.soldier.filter(|_| p.alive)
        && let Some(m) = sim.vehicles[i].movement.as_mut()
    {
        m.set_input(axes);
    }
}

impl Session for LocalSession<'_> {
    fn set_action(&mut self, player: usize, axes: &[(usize, f32)]) {
        apply_action(&mut self.sim, player, axes);
    }
    fn step(&mut self) {
        self.sim.step();
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot::of(&self.sim)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ObjectState {
    pub template: String,
    pub pos: [f32; 3],
    /// its rotation, as [`q::quat`] (its rows: [`q::rows`])
    pub rot: [i16; 4],
    pub vel: [f32; 3],
    pub rotvel: [f32; 3],
    /// its parts' bundle angles (turrets, wheels, wings), in the order of `craft.parts`
    pub angles: Vec<[f32; 3]>,
    pub hp: Option<f32>,
    pub team: u32,
    /// the vehicle a soldier is in, and its seat there
    pub inside: Option<u32>,
    pub seat: u32,
    /// gone from the world (or not in it: a soldier not yet spawned)
    pub gone: bool,
    /// which object of the server's it is ([`crate::mover::Vehicle::born`]): a place taken for another changes it
    #[serde(default)]
    pub born: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct PlayerState {
    pub team: u32,
    pub ai: bool,
    pub kit: u32,
    pub alive: bool,
    pub soldier: Option<u32>,
    pub spawn_time: f32,
    /// the scoreboard's: score, kills, deaths
    pub score: (i32, u32, u32),
    /// the vehicle that killed its soldier (`Armor.killed_by`): the kill feed's «who, with what». The copy's own
    /// step only knows it when it simulated the hit
    pub killed_by: Option<u32>,
}

/// The input a player's soldier took in a step: the mapped axes (`mask`) and those not 0.
/// A copy of the world steps with them between snapshots (the bots' too: it runs no bots).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Action {
    pub mask: u64,
    /// the axes that are whole hundredths, as `PlayerAction` leaves the analog ones (and the
    /// buttons' 1)
    pub hundredths: Vec<(u8, i16)>,
    /// the rest, as they are
    pub values: Vec<(u8, f32)>,
}

/// `v` as hundredths, if that is exactly what it is.
fn hundredths(v: f32) -> Option<i16> {
    let n = (v as f64 * 100.0).round();
    (n.abs() < 32768.0 && ((n / 100.0) as f32).to_bits() == v.to_bits()).then_some(n as i16)
}

impl Action {
    fn of(input: &crate::anim::Input) -> Action {
        let mut a = Action { mask: input.mask, ..Default::default() };
        for i in (0..input.axes.len().min(64)).filter(|&i| input.mask >> i & 1 == 1 && input.axes[i] != 0.0) {
            match hundredths(input.axes[i]) {
                Some(n) => a.hundredths.push((i as u8, n)),
                None => a.values.push((i as u8, input.axes[i])),
            }
        }
        a
    }
    pub fn axes(&self) -> Axes {
        let value = |i: usize| {
            let h = self.hundredths.iter().find(|v| v.0 as usize == i).map(|v| (v.1 as f64 / 100.0) as f32);
            h.or_else(|| self.values.iter().find(|v| v.0 as usize == i).map(|v| v.1)).unwrap_or(0.0)
        };
        (0..64).filter(|i| self.mask >> i & 1 == 1).map(|i| (i, value(i))).collect()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct PointState {
    pub team: u32,
    pub get: f32,
    pub lose: f32,
}

/// A flag of capture the flag: where it is, at home, who carries it, its clock.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct FlagState {
    pub pos: [f32; 3],
    pub home: bool,
    pub carrier: Option<u32>,
    pub timer: f32,
}

/// An object that lies on the ground (`Sim.items` at `at`, not carried): a kit, a bundle.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ItemState {
    pub at: u32,
    pub template: String,
    pub pos: [f32; 3],
    pub rot: [i16; 4],
}

/// The state of the modes beyond conquest: the flags and captures of CTF, each objective's
/// state: whether it is done, its clock once met and a timer's own clock. And what lies on the
/// ground (it changes seldom: a kit falls, is taken, goes).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Modes {
    pub flags: Vec<FlagState>,
    pub captures: [u32; 2],
    pub objectives: Vec<(bool, f32, f32)>,
    pub items: Vec<ItemState>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Snapshot {
    pub step: u64,
    pub tickets: [i32; 2],
    pub points: Vec<PointState>,
    /// the spawn groups' teams, by id (a copy does not run the flags' change of them)
    pub groups: Vec<(i64, i64)>,
    pub players: Vec<PlayerState>,
    /// by player: its name
    pub names: Vec<String>,
    /// by player: what its live soldier took in this step (None: no live soldier)
    pub actions: Vec<Option<Action>>,
    /// by index in `Sim.vehicles`
    pub objects: Vec<ObjectState>,
    /// the objects left as an earlier snapshot had them (the packet had no room for them): a
    /// copy goes on with its own step of them
    pub stale: Vec<u32>,
    /// the same of the players: one with no room keeps the base's state, maybe some steps old (a death undone and done again);
    /// a copy keeps its own
    pub stale_players: Vec<u32>,
    /// still being sent to a client that joined: not a world to show yet, only to build on
    pub partial: bool,
    pub modes: Modes,
    /// the pre-game's seconds left and of each round ([`Sim::pregame`], [`Sim::pregame_delay`])
    pub pregame: [f32; 2],
    /// how many people the round still waits for ([`Sim::players_needed`])
    pub players_needed: u32,
    /// friendly fire and the nose camera ([`Sim::rules`]): the briefing shows the server's
    pub rules: crate::damage::ServerRules,
}

impl Snapshot {
    pub fn of(sim: &Sim) -> Snapshot {
        let objects = sim
            .vehicles
            .iter()
            .map(|v| ObjectState {
                template: v.template.clone(),
                pos: q::snap3(v.craft.body.pos, q::POS),
                rot: q::quat(&v.craft.body.rows),
                vel: q::snap3(v.craft.body.vel, q::VEL),
                rotvel: q::snap3(v.craft.body.rotvel, q::ROTVEL),
                angles: v.craft.parts.iter().map(|p| q::snap3(p.bundle.as_ref().map_or([0.0; 3], |b| b.angle), q::ANGLE)).collect(),
                hp: v.armor.as_ref().map(|a| a.hp),
                team: v.team,
                inside: v.entry.inside.map(|k| k as u32),
                seat: v.entry.inside.map_or(0, |_| v.entry.position as u32),
                gone: v.removed || !v.in_world(),
                born: v.born,
            })
            .collect();
        let players = sim.players.iter().map(|p| PlayerState { team: p.team, ai: p.ai, kit: p.kit as u32, alive: p.alive, soldier: p.soldier.map(|k| k as u32), spawn_time: p.spawn_time, score: (p.score, p.kills, p.deaths), killed_by: p.soldier.and_then(|s| sim.vehicles.get(s)?.armor.as_ref()?.killed_by).map(|b| b as u32) }).collect();
        let actions = sim
            .players
            .iter()
            .map(|p| p.soldier.filter(|_| p.alive).and_then(|i| sim.vehicles[i].movement.as_ref()).filter(|m| m.driven).map(|m| Action::of(&m.input)))
            .collect();
        let (tickets, points) = match &sim.modes.conquest {
            Some(c) => (c.tickets, c.points.iter().map(|p| PointState { team: p.team, get: p.get, lose: p.lose }).collect()),
            None => ([0, 0], Vec::new()),
        };
        let groups = sim.modes.conquest.as_ref().map_or(Vec::new(), |c| c.groups.iter().map(|(id, g)| (*id, g.team)).collect());
        let mut modes = Modes::default();
        if let Some(c) = &sim.modes.ctf {
            modes.flags = c.bases.iter().map(|b| FlagState { pos: b.flag.pos, home: b.flag.home, carrier: b.flag.carrier.map(|k| k as u32), timer: b.flag.timer }).collect();
            modes.captures = c.captures;
        }
        if let Some(o) = &sim.modes.objectives {
            modes.objectives = o.list.iter().map(|o| (o.done, o.timer, if let crate::objective::Goal::Timer { clock, .. } = o.goal { clock } else { 0.0 })).collect();
        }
        modes.items = (sim.items.iter().enumerate())
            .filter(|(_, t)| !t.removed && t.carrier.is_none())
            .map(|(k, t)| ItemState { at: k as u32, template: t.template.clone(), pos: q::snap3(t.pos, q::POS), rot: q::quat(&t.rows) })
            .collect();
        let names = sim.players.iter().map(|p| p.name.clone()).collect();
        let pregame = [sim.round.pregame, sim.rules.pregame_delay];
        Snapshot { step: sim.clock.steps, tickets, points, groups, players, names, actions, objects, stale: Vec::new(), stale_players: Vec::new(), partial: false, modes, pregame, players_needed: sim.round.players_needed, rules: sim.rules.server }
    }

    /// This state onto a copy of the world (`sim`, of the same level): the players it lacks
    /// added, each object where it is and how it moves, the round's score. An object the copy
    /// lacks is made from its template; one whose template differs is left (counted: the
    /// return, which should be 0).
    pub fn write(&self, sim: &mut Sim) -> usize {
        let mut odd = 0;
        let mut pending_killers = Vec::new();
        let mut to_kill = Vec::new();
        let mut deaths = Vec::new();
        let remote = sim.role.remote;
        sim.role.copy = true;
        for (k, p) in self.players.iter().enumerate() {
            // (one the packet had no room for: the copy's own, newer than the base's)
            if k < sim.players.len() && self.stale_players.contains(&(k as u32)) {
                continue;
            }
            // (the deaths before the copy's first step are old news: every player the server had then comes in its first
            // snapshot, `nplayers`. One new to a copy that has stepped joined the server since, and all its deaths are new:
            // with jitter the copy's first snapshot with it can come after its first death, spawned and killed in a step;
            // Berlin)
            // One held back (`stale_players`) in the snapshot that brought it came as a default entry, 0 deaths: when it is
            // first sent its deaths are old news too
            let held = self.stale_players.contains(&(k as u32));
            if k >= sim.players.len() {
                let q = sim.add_player(p.team, soldier_template(p.team), p.ai);
                sim.players[q].unsent = held;
                if held {
                    continue;
                }
            }
            let known = sim.clock.steps > 0 && !std::mem::take(&mut sim.players[k].unsent);
            let q = &mut sim.players[k];
            if let Some(n) = self.names.get(k).filter(|n| **n != q.name) {
                q.name = n.clone();
            }
            q.team = p.team;
            q.kit = p.kit as usize;
            // a death more on the server: the copy's (a `remote` one does not decide them: its own step killed a soldier just spawned and
            // not yet sent, and never saw one out of view, `stale`, die; a round's restart kills nobody)
            if remote && known && p.score.2 > q.deaths {
                deaths.push((k, p.soldier.map(|s| s as usize).or(q.soldier)));
            }
            q.alive = p.alive;
            q.soldier = p.soldier.map(|s| s as usize);
            q.spawn_time = p.spawn_time;
            (q.score, q.kills, q.deaths) = p.score;
            if let (Some(b), Some(s)) = (p.killed_by, p.soldier) {
                pending_killers.push((s as usize, b as usize));
            }
        }
        // a soldier the copy killed on its own (a hit it simulated and the server did not give) lives while the server's
        // player does: else the copy's step kills it again every step
        for p in self.players.iter().filter(|p| p.alive) {
            let Some(s) = p.soldier.map(|s| s as usize) else { continue };
            if self.objects.get(s).is_none_or(|o| o.gone) {
                continue;
            }
            let Some(v) = sim.vehicles.get_mut(s) else { continue };
            if v.removed || v.armor.as_ref().is_some_and(|a| a.destroyed) {
                v.removed = false;
                if let Some(a) = v.armor.as_mut() {
                    *a = crate::damage::Armor::new(a.template.clone());
                }
                if let Some(m) = v.movement.as_mut() {
                    m.revive();
                }
            }
        }
        for (i, o) in self.objects.iter().enumerate() {
            if self.stale.contains(&(i as u32)) && i < sim.vehicles.len() {
                continue;
            }
            if i >= sim.vehicles.len() && sim.spawn_vehicle(&o.template, o.pos, [0.0; 3]) != Some(i) {
                odd += 1;
                continue;
            }
            // the copy made it its own way (a soldier its step spawned before the player's template came, in a late joiner's first packets): the server's stands;
            // and the server took the place for another
            let born = sim.vehicles[i].born;
            if sim.vehicles[i].template != o.template || (born != 0 && born != o.born) {
                let Some(mut made) = sim.world.vehicle(&o.template) else {
                    odd += 1;
                    continue;
                };
                made.id = i;
                made.born = o.born;
                made.place_gen = sim.vehicles[i].place_gen.wrapping_add(1);
                sim.vehicles[i] = made;
                // (317, the monkey in Midway coop) who rode the object it replaces is out (its seats are gone): the loops below
                // put each where the server has it; else `ride` reads a seat a soldier does not have
                for v in sim.vehicles.iter_mut().filter(|v| v.entry.inside == Some(i)) {
                    v.entry.inside = None;
                }
            }
            let v = &mut sim.vehicles[i];
            v.born = o.born;
            v.craft.body.pos = o.pos;
            v.craft.body.rows = q::rows(o.rot);
            v.craft.body.vel = o.vel;
            v.craft.body.rotvel = o.rotvel;
            for (p, a) in v.craft.parts.iter_mut().zip(&o.angles) {
                if let Some(b) = p.bundle.as_mut() {
                    b.angle = *a;
                }
            }
            if let (Some(a), Some(hp)) = (v.armor.as_mut(), o.hp) {
                // destroyed on the server and not yet here: by `giveDamage`, below, for what follows (the death animation, the wreck)
                if hp <= crate::damage::DEAD_HP && a.hp > crate::damage::DEAD_HP && !a.destroyed {
                    to_kill.push(i);
                } else {
                    a.hp = hp;
                }
            }
            v.team = o.team;
            // (in a vehicle is gone too: getting in and out the copy does itself, from the actions)
            if o.gone && o.inside.is_none() {
                v.removed = true;
            }
        }
        // in the vehicle and seat the server has them (the copy gets in and out from the actions,
        // but a better bot picks its seat its own way): out first, then in. A stale one too, as
        // the server had it last (the copy's own actions put one in a vehicle the server never
        // boarded: 13 m off, GC_Hoth)
        // (a copy that is `remote` gives no damage of its own: deaths are the server's)
        for i in to_kill {
            let hp = sim.vehicles[i].armor.as_ref().map_or(0.0, |a| a.hp);
            sim.damage.attacker = None;
            let remote = std::mem::replace(&mut sim.role.remote, false);
            sim.give_damage(i, hp + 1.0, crate::damage::Source::Hit);
            sim.role.remote = remote;
        }
        for (k, sol) in deaths {
            sim.journal.player_log.push((sim.clock.steps, k, crate::players::PlayerEvent::Died(sol.unwrap_or(usize::MAX))));
        }
        // who killed each soldier, once the objects are there (the copy's own guess, if it had one, gives way)
        for (s, b) in pending_killers {
            if let Some(a) = sim.vehicles.get_mut(s).and_then(|v| v.armor.as_mut()) {
                a.killed_by = Some(b);
            }
        }
        let inside = |i: usize| self.objects.get(i).map(|o| o.inside.map(|k| (k as usize, o.seat as usize)));
        for i in 0..sim.vehicles.len() {
            if let (Some(want), Some(k)) = (inside(i), sim.vehicles[i].entry.inside)
                && want != Some((k, sim.vehicles[i].entry.position))
            {
                sim.exit(i, k);
                // (`exit` puts it at the vehicle's exit place: where the server has it stands, 3.7 m off for a step)
                let o = &self.objects[i];
                let b = &mut sim.vehicles[i].craft.body;
                (b.pos, b.vel) = (o.pos, o.vel);
            }
        }
        for i in 0..sim.vehicles.len() {
            if let Some(Some((k, seat))) = inside(i)
                && sim.vehicles[i].entry.inside.is_none()
                && sim.vehicles.get(k).is_some_and(|v| seat < v.entry.occupants.len())
            {
                if let Some(j) = sim.vehicles[k].entry.occupants[seat].filter(|&j| j != i) {
                    sim.exit(j, k);
                }
                sim.enter(i, k, seat);
                let o = &self.objects[i];
                let b = &mut sim.vehicles[i].craft.body;
                (b.pos, b.vel) = (o.pos, o.vel);
            }
        }
        // the players' soldiers (made here from the template: no player yet) are theirs: they
        // get in and out with their use key
        for p in &self.players {
            if let Some(m) = p.soldier.and_then(|i| sim.vehicles.get_mut(i as usize)).and_then(|v| v.movement.as_mut()) {
                m.has_player = true;
                m.human = !p.ai;
            }
        }
        if let Some(c) = sim.modes.conquest.as_mut() {
            c.tickets = self.tickets;
            for (p, s) in c.points.iter_mut().zip(&self.points) {
                p.team = s.team;
                p.get = s.get;
                p.lose = s.lose;
            }
            for (id, team) in &self.groups {
                if let Some(g) = c.groups.get_mut(id) {
                    g.team = *team;
                }
            }
        }
        if let Some(c) = sim.modes.ctf.as_mut() {
            for (b, f) in c.bases.iter_mut().zip(&self.modes.flags) {
                b.flag.pos = f.pos;
                b.flag.home = f.home;
                b.flag.carrier = f.carrier.map(|k| k as usize);
                b.flag.timer = f.timer;
            }
            c.captures = self.modes.captures;
        }
        if let Some(o) = sim.modes.objectives.as_mut() {
            for (o, &(done, timer, c)) in o.list.iter_mut().zip(&self.modes.objectives) {
                o.done = done;
                o.timer = timer;
                if let crate::objective::Goal::Timer { clock, .. } = &mut o.goal {
                    *clock = c;
                }
            }
        }
        // what lies on the ground, as the snapshot has it (the rest is carried or gone)
        let n = self.modes.items.iter().map(|t| t.at as usize + 1).max().unwrap_or(0).max(sim.items.len());
        sim.items.resize_with(n, || crate::create::Item { template: String::new(), kind: String::new(), pos: [0.0; 3], rows: [[0.0; 3]; 3], removed: true, carrier: None, weapons: Vec::new(), parts: Vec::new(), expires: None });
        let ground: std::collections::HashMap<usize, &ItemState> = self.modes.items.iter().map(|t| (t.at as usize, t)).collect();
        for (k, t) in sim.items.iter_mut().enumerate() {
            match ground.get(&k) {
                Some(s) => {
                    if t.template != s.template {
                        t.kind = sim.world.db.object_template(&s.template).map_or(String::new(), |o| o.kind.to_ascii_lowercase());
                        t.template = s.template.clone();
                    }
                    (t.pos, t.rows, t.removed, t.carrier, t.expires) = (s.pos, q::rows(s.rot), false, None, None);
                }
                None => t.removed = true,
            }
        }
        // the pre-game: the copy waits as the server does, and plays when it does
        let [left, delay] = self.pregame;
        if left > 0.0 {
            (sim.round.pregame, sim.round.playing) = (left, false);
        } else if sim.round.pregame > 0.0 {
            (sim.round.pregame, sim.round.playing, sim.round.round_start) = (0.0, true, sim.clock.time);
        }
        (sim.rules.pregame_delay, sim.round.players_needed) = (delay, self.players_needed);
        sim.rules.server = self.rules;
        sim.clock.steps = self.step;
        // the players' soldiers with their kit's weapons, as the server's spawn hands them: the snapshot names the kit, not
        // the weapons, and a soldier made from its template alone has none (the copy drew empty hands and no ammunition)
        for k in 0..sim.players.len() {
            let (Some(s), team, kit) = (sim.players[k].soldier, sim.players[k].team, sim.players[k].kit) else { continue };
            let unarmed = sim.vehicles.get(s).and_then(|v| v.movement.as_ref()).is_some_and(|m| m.weapon.is_none() && m.stowed.is_empty());
            if unarmed && let Some(name) = sim.team_kit(team, kit) {
                sim.arm(s, &name);
            }
        }
        odd
    }

    /// Each player's action of this snapshot to its soldier, for the copy's next step.
    pub fn act(&self, sim: &mut Sim) {
        for (k, a) in self.actions.iter().enumerate() {
            if let Some(a) = a {
                apply_action(sim, k, &a.axes());
            }
        }
    }
}

/// An object as it is, unrounded: what a client predicts (its soldier, its vehicle) has to
/// start from the server's very bits, or the contacts (a tank's springs) make a few-ulp
/// difference into decimetres within the prediction.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Exact {
    pub pos: [f32; 3],
    pub rows: [[f32; 3]; 3],
    pub vel: [f32; 3],
    pub rotvel: [f32; 3],
    /// as many as parts; most are 0 (a warship has a hundred): only the others travel
    #[serde(with = "sparse_angles")]
    pub angles: Vec<[f32; 3]>,
    /// its engines' state, in the order of the parts: the step of a copy that jumped (a late joiner) has no history to get them from
    pub engines: Vec<ExactEngine>,
}

/// What an engine carries from step to step (`craft::Engine`'s state).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExactEngine {
    pub rpm: f32,
    pub load: f32,
    pub load_prev: f32,
    pub load_n: f32,
    pub gear: i32,
    pub gear_timer: f32,
    pub on: bool,
    pub braking: bool,
    pub steer: f32,
    pub heat: f32,
    pub burnt: bool,
}

/// A list of angles as its length and those not 0.
mod sparse_angles {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[[f32; 3]], s: S) -> Result<S::Ok, S::Error> {
        let kept: Vec<(u16, [f32; 3])> = v.iter().enumerate().filter(|(_, a)| a.iter().any(|x| x.to_bits() != 0)).map(|(i, a)| (i as u16, *a)).collect();
        (v.len() as u16, kept).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<[f32; 3]>, D::Error> {
        let (n, kept) = <(u16, Vec<(u16, [f32; 3])>)>::deserialize(d)?;
        let mut v = vec![[0.0; 3]; n as usize];
        for (i, a) in kept {
            if let Some(x) = v.get_mut(i as usize) {
                *x = a;
            }
        }
        Ok(v)
    }
}

impl Exact {
    pub fn of(v: &crate::mover::Vehicle) -> Exact {
        let b = &v.craft.body;
        Exact { pos: b.pos, rows: b.rows, vel: b.vel, rotvel: b.rotvel, angles: v.craft.parts.iter().map(|p| p.bundle.as_ref().map_or([0.0; 3], |b| b.angle)).collect(), engines: v.craft.parts.iter().filter_map(|p| match &p.kind {
            crate::craft::Kind::Engine(e) => Some(ExactEngine { rpm: e.rpm, load: e.load, load_prev: e.load_prev, load_n: e.load_n, gear: e.gear, gear_timer: e.gear_timer, on: e.on, braking: e.braking, steer: e.steer, heat: e.heat, burnt: e.burnt }),
            _ => None,
        }).collect() }
    }
    pub fn write(&self, v: &mut crate::mover::Vehicle) {
        let b = &mut v.craft.body;
        (b.pos, b.rows, b.vel, b.rotvel) = (self.pos, self.rows, self.vel, self.rotvel);
        for (p, a) in v.craft.parts.iter_mut().zip(&self.angles) {
            if let Some(b) = p.bundle.as_mut() {
                b.angle = *a;
            }
        }
        let engines = v.craft.parts.iter_mut().filter_map(|p| match &mut p.kind {
            crate::craft::Kind::Engine(e) => Some(e),
            _ => None,
        });
        for (e, x) in engines.zip(&self.engines) {
            (e.rpm, e.load, e.load_prev, e.load_n, e.gear, e.gear_timer, e.on, e.braking, e.steer, e.heat, e.burnt) = (x.rpm, x.load, x.load_prev, x.load_n, x.gear, x.gear_timer, x.on, x.braking, x.steer, x.heat, x.burnt);
        }
    }
}

/// `NetworkableInfo.setPredictionMode`: how a client shows an object between
/// snapshots. In bf1942 the bodies (soldiers, vehicles) are `PMLinear`, most parts `PMNone`, a
/// ship's turret `PMUsePhysics`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredictionMode {
    /// as the snapshot left it
    None,
    /// moved on with its speed
    Linear,
    /// its physics run on the client
    Physics,
}

/// The prediction mode of `template` (its `networkableInfo`; none: `None`).
pub fn prediction_mode(world: &crate::World, template: &str) -> PredictionMode {
    let mode = world
        .db
        .object_template(template)
        .and_then(|t| t.get("networkableinfo"))
        .and_then(|a| a.first())
        .and_then(|info| world.db.get("networkableinfo", info))
        .and_then(|i| i.get("predictionmode"))
        .and_then(|a| a.first())
        .map(|m| m.to_ascii_lowercase());
    match mode.as_deref() {
        Some("pmlinear") => PredictionMode::Linear,
        Some("pmusephysics") => PredictionMode::Physics,
        _ => PredictionMode::None,
    }
}

/// The soldier a player of `team` spawns as (the client's and the server's choice).
pub fn soldier_template(team: u32) -> &'static str {
    if team == 1 { "GermanSoldier" } else { "USSoldier" }
}

impl Snapshot {

    /// What changed from `base` (a snapshot the receiver has) to this one.
    pub fn delta(&self, base: Option<&Snapshot>) -> Delta {
        let empty = Snapshot::default();
        let b = base.unwrap_or(&empty);
        let objects = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| {
                let d = ObjectDelta::between(b.objects.get(i), o);
                (d != ObjectDelta::default()).then_some((i as u32, d))
            })
            .collect();
        Delta {
            step: self.step,
            base: base.map(|b| b.step),
            score: (self.tickets != b.tickets || self.points != b.points || self.groups != b.groups).then(|| (self.tickets, self.points.clone(), self.groups.clone())),
            players: self.players.iter().enumerate().filter(|(k, p)| b.players.get(*k) != Some(p)).map(|(k, p)| (k as u32, *p)).collect(),
            nplayers: self.players.len() as u32,
            names: self.names.iter().enumerate().filter(|(k, n)| b.names.get(*k) != Some(n)).map(|(k, n)| (k as u32, n.clone())).collect(),
            actions: self.actions.iter().enumerate().filter(|(k, a)| b.actions.get(*k) != Some(a)).map(|(k, a)| (k as u32, a.clone())).collect(),
            len: self.objects.len() as u32,
            objects,
            yours: 0,
            exact: Vec::new(),
            stale: Vec::new(),
            stale_players: Vec::new(),
            partial: false,
            talk_upto: self.step,
            radio: Vec::new(),
            radio_taken: 0,
            chat: Vec::new(),
            chat_taken: 0,
            modes: (self.modes.flags != b.modes.flags || self.modes.captures != b.modes.captures || self.modes.objectives != b.modes.objectives).then(|| Modes { items: Vec::new(), ..self.modes.clone() }),
            items: self.modes.items.iter().filter(|t| !b.modes.items.contains(t)).cloned().collect(),
            items_gone: b.modes.items.iter().filter(|t| !self.modes.items.iter().any(|n| n.at == t.at)).map(|t| t.at).collect(),
            round: (self.pregame != b.pregame || self.players_needed != b.players_needed).then_some((self.pregame, self.players_needed)),
            rules: (base.is_none() || self.rules != b.rules).then_some(self.rules),
        }
    }

    /// `base` with `d` on it (`base` must be the snapshot of `d.base`).
    pub fn apply(base: Option<&Snapshot>, d: &Delta) -> Snapshot {
        let mut s = base.cloned().unwrap_or_default();
        s.step = d.step;
        if let Some((t, p, g)) = &d.score {
            s.tickets = *t;
            s.points = p.clone();
            s.groups = g.clone();
        }
        s.players.resize(d.nplayers as usize, PlayerState::default());
        for (k, p) in &d.players {
            s.players[*k as usize] = *p;
        }
        s.names.resize(d.nplayers as usize, String::new());
        for (k, n) in &d.names {
            s.names[*k as usize] = n.clone();
        }
        s.actions.resize(d.nplayers as usize, None);
        for (k, a) in &d.actions {
            s.actions[*k as usize] = a.clone();
        }
        s.objects.resize(d.len as usize, ObjectState::default());
        for (i, o) in &d.objects {
            o.apply(&mut s.objects[*i as usize]);
        }
        s.stale = d.stale.clone();
        s.stale_players = d.stale_players.clone();
        s.partial = d.partial;
        if let Some(m) = &d.modes {
            (s.modes.flags, s.modes.captures, s.modes.objectives) = (m.flags.clone(), m.captures, m.objectives.clone());
        }
        s.modes.items.retain(|t| !d.items_gone.contains(&t.at) && !d.items.iter().any(|n| n.at == t.at));
        s.modes.items.extend(d.items.iter().cloned());
        s.modes.items.sort_by_key(|t| t.at);
        if let Some((p, n)) = d.round {
            (s.pregame, s.players_needed) = (p, n);
        }
        if let Some(r) = d.rules {
            s.rules = r;
        }
        s
    }
}

/// A snapshot as what changed against an earlier one (`base`; None: against nothing, the
/// whole of it). The flags are few: they go whole.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Delta {
    pub step: u64,
    pub base: Option<u64>,
    /// the tickets, the flags and the spawn groups' teams, if they changed
    pub score: Option<([i32; 2], Vec<PointState>, Vec<(i64, i64)>)>,
    /// the players that changed, and how many there are
    pub players: Vec<(u32, PlayerState)>,
    pub nplayers: u32,
    /// the players whose name changed
    pub names: Vec<(u32, String)>,
    /// the players whose action changed
    pub actions: Vec<(u32, Option<Action>)>,
    pub len: u32,
    pub objects: Vec<(u32, ObjectDelta)>,
    /// for its receiver: the step label of its own action in force in this step (what of its
    /// prediction is confirmed; 0: none yet)
    pub yours: u64,
    /// for its receiver: its soldier and the vehicle it is in, unrounded ([`Exact`])
    pub exact: Vec<(u32, Exact)>,
    /// the objects that changed and are not here (no room): as the base has them, and stale
    #[serde(with = "bitset")]
    pub stale: Vec<u32>,
    /// the players that changed and are not here (no room, or their soldier not sent yet): stale too
    #[serde(with = "bitset")]
    pub stale_players: Vec<u32>,
    /// a joining client's world, with more to come (it builds on this and shows nothing yet)
    pub partial: bool,
    /// every line said up to this step is here or already sent (later ones wait for room)
    pub talk_upto: u64,
    /// what was said on the radio after the base
    pub radio: Vec<crate::radio::Said>,
    /// for its receiver: how many of its radio picks the server has taken
    pub radio_taken: u32,
    /// the chat lines this receiver may read, said after the base: (step, line)
    pub chat: Vec<(u64, String)>,
    /// for its receiver: how many of its chat lines the server has taken
    pub chat_taken: u32,
    /// the flags and objectives ([`Modes`], without its items), if they changed
    pub modes: Option<Modes>,
    /// the things on the ground that are new or changed, and the places of those gone (each on its own: with 64 players dying they are many)
    pub items: Vec<ItemState>,
    pub items_gone: Vec<u32>,
    /// the pre-game ([`Snapshot::pregame`]) and the people still needed, if they changed
    pub round: Option<([f32; 2], u32)>,
    /// the server's rules ([`Snapshot::rules`]): against nothing, or if they changed
    pub rules: Option<crate::damage::ServerRules>,
}

/// A list of indices as a bitset (a byte per 8 objects, not a byte or two per index).
mod bitset {
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u32], s: S) -> Result<S::Ok, S::Error> {
        let mut b = vec![0u8; v.iter().max().map_or(0, |&m| m as usize / 8 + 1)];
        for &i in v {
            b[i as usize / 8] |= 1 << (i % 8);
        }
        s.serialize_bytes(&b)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u32>, D::Error> {
        let b: Vec<u8> = serde_bytes_vec(d)?;
        Ok((0..b.len() as u32 * 8).filter(|i| b[*i as usize / 8] >> (i % 8) & 1 == 1).collect())
    }
    fn serde_bytes_vec<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
        }
        d.deserialize_byte_buf(V)
    }
}

/// An object's fields that changed (None: as in the base). It goes as a mask of the fields
/// there and then those (not a tag per field).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ObjectDelta {
    pub template: Option<String>,
    /// in steps of [`q::POS`] from the base's
    pub pos: Option<[i32; 3]>,
    /// the quaternion's steps from the base's
    pub rot: Option<[i32; 4]>,
    pub vel: Option<[i32; 3]>,
    pub rotvel: Option<[i32; 3]>,
    /// the parts whose angles changed (index, steps of [`q::ANGLE`] from the base's), and how
    /// many there are
    pub angles: Option<(u16, Vec<(u16, [i32; 3])>)>,
    pub hp: Option<Option<f32>>,
    pub team: Option<u32>,
    pub inside: Option<Option<u32>>,
    pub seat: Option<u32>,
    pub gone: Option<bool>,
    pub born: Option<u32>,
}

/// [`ObjectDelta`]'s fields in order, for its mask: (field, its type).
macro_rules! object_delta_fields {
    ($m:ident) => {
        $m!(template: String, pos: [i32; 3], rot: [i32; 4], vel: [i32; 3], rotvel: [i32; 3], angles: (u16, Vec<(u16, [i32; 3])>), hp: Option<f32>, team: u32, inside: Option<u32>, seat: u32, gone: bool, born: u32)
    };
}

impl Serialize for ObjectDelta {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        macro_rules! ser {
            ($($f:ident: $t:ty),*) => {{
                let (mut mask, mut bit, mut n) = (0u16, 1u16, 1);
                $(if self.$f.is_some() { mask |= bit; n += 1; } bit <<= 1;)*
                let _ = bit;
                let mut t = s.serialize_tuple(n)?;
                t.serialize_element(&mask)?;
                $(if let Some(v) = &self.$f { t.serialize_element(v)?; })*
                t.end()
            }};
        }
        object_delta_fields!(ser)
    }
}

impl<'de> Deserialize<'de> for ObjectDelta {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = ObjectDelta;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object delta")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<ObjectDelta, A::Error> {
                use serde::de::Error;
                let mask: u16 = a.next_element()?.ok_or_else(|| A::Error::custom("no mask"))?;
                let mut o = ObjectDelta::default();
                let mut bit = 1u16;
                macro_rules! de {
                    ($($f:ident: $t:ty),*) => {$(
                        if mask & bit != 0 {
                            o.$f = Some(a.next_element::<$t>()?.ok_or_else(|| A::Error::custom("short"))?);
                        }
                        bit <<= 1;
                    )*};
                }
                object_delta_fields!(de);
                let _ = bit;
                Ok(o)
            }
        }
        d.deserialize_tuple(13, V)
    }
}

impl ObjectDelta {
    fn between(base: Option<&ObjectState>, o: &ObjectState) -> ObjectDelta {
        let empty = ObjectState::default();
        let b = base.unwrap_or(&empty);
        fn ch<T: PartialEq + Clone>(a: &T, b: &T) -> Option<T> {
            (a != b).then(|| b.clone())
        }
        fn steps(a: [f32; 3], b: [f32; 3], unit: f32) -> [i32; 3] {
            [0, 1, 2].map(|j| q::steps(b[j], unit) - q::steps(a[j], unit))
        }
        fn diff(a: [f32; 3], b: [f32; 3], unit: f32) -> Option<[i32; 3]> {
            (a != b).then(|| steps(a, b, unit))
        }
        let angles = (b.angles != o.angles).then(|| {
            let changed = o.angles.iter().enumerate().filter(|(k, a)| b.angles.get(*k).copied().unwrap_or_default() != **a).map(|(k, a)| (k as u16, steps(b.angles.get(k).copied().unwrap_or_default(), *a, q::ANGLE))).collect();
            (o.angles.len() as u16, changed)
        });
        let rot = (b.rot != o.rot).then(|| [0, 1, 2, 3].map(|j| o.rot[j] as i32 - b.rot[j] as i32));
        ObjectDelta {
            template: ch(&b.template, &o.template),
            pos: diff(b.pos, o.pos, q::POS),
            rot,
            vel: diff(b.vel, o.vel, q::VEL),
            rotvel: diff(b.rotvel, o.rotvel, q::ROTVEL),
            angles,
            hp: ch(&b.hp, &o.hp),
            team: ch(&b.team, &o.team),
            inside: ch(&b.inside, &o.inside),
            seat: ch(&b.seat, &o.seat),
            gone: ch(&b.gone, &o.gone),
            born: ch(&b.born, &o.born),
        }
    }

    fn apply(&self, o: &mut ObjectState) {
        macro_rules! set {
            ($($f:ident),*) => {$(if let Some(v) = &self.$f { o.$f = v.clone(); })*};
        }
        fn add(a: [f32; 3], d: [i32; 3], unit: f32) -> [f32; 3] {
            [0, 1, 2].map(|j| (q::steps(a[j], unit) + d[j]) as f32 * unit)
        }
        set!(template, hp, team, inside, seat, gone, born);
        if let Some(d) = self.pos {
            o.pos = add(o.pos, d, q::POS);
        }
        if let Some(d) = self.vel {
            o.vel = add(o.vel, d, q::VEL);
        }
        if let Some(d) = self.rotvel {
            o.rotvel = add(o.rotvel, d, q::ROTVEL);
        }
        if let Some(d) = self.rot {
            o.rot = [0, 1, 2, 3].map(|j| (o.rot[j] as i32 + d[j]) as i16);
        }
        if let Some((n, changed)) = &self.angles {
            o.angles.resize(*n as usize, [0.0; 3]);
            for (k, d) in changed {
                o.angles[*k as usize] = add(o.angles[*k as usize], *d, q::ANGLE);
            }
        }
    }
}

/// What a snapshot is rounded to (the server's own history too, so a delta is exact): whole
/// steps of a unit, carried as how many steps they moved from the base's.
pub mod q {
    /// 1 mm
    pub const POS: f32 = 1.0 / 1024.0;
    pub const VEL: f32 = 1.0 / 256.0;
    pub const ROTVEL: f32 = 1.0 / 1024.0;
    /// a part's angles (degrees, as `RotationalBundle` keeps them)
    pub const ANGLE: f32 = 1.0 / 64.0;
    /// a quaternion's component, ±1
    const UNIT: f32 = 32767.0;

    pub fn steps(x: f32, unit: f32) -> i32 {
        (x / unit).round() as i32
    }
    pub fn snap3(v: [f32; 3], unit: f32) -> [f32; 3] {
        v.map(|x| steps(x, unit) as f32 * unit)
    }

    /// The rotation `rows` (a row-major orthonormal matrix) as a quaternion (x, y, z, w) of
    /// whole steps, w ≥ 0 (each rotation one way).
    pub fn quat(m: &[[f32; 3]; 3]) -> [i16; 4] {
        let (m, t) = (m.map(|r| r.map(f64::from)), f64::from(m[0][0] + m[1][1] + m[2][2]));
        let q = if t > 0.0 {
            let s = (t + 1.0).sqrt() * 2.0;
            [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s]
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
            [0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s]
        } else if m[1][1] > m[2][2] {
            let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
            [(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s]
        } else {
            let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
            [(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s]
        };
        let n = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        let sign = if q[3] < 0.0 { -1.0 } else { 1.0 };
        q.map(|x| (x * sign / n * UNIT as f64).round() as i16)
    }

    /// The rows of a quaternion of [`quat`] (normalised first: the steps are not unit).
    pub fn rows(q: [i16; 4]) -> [[f32; 3]; 3] {
        let q = q.map(|x| x as f64);
        let n = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        if n == 0.0 {
            return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        }
        let [x, y, z, w] = q.map(|c| c / n);
        [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ]
        .map(|r| r.map(|v| v as f32))
    }
}

fn codec() -> impl bincode::Options {
    use bincode::Options;
    bincode::DefaultOptions::new().with_varint_encoding()
}

impl Delta {
    /// Bytes this delta takes encoded.
    pub fn size(&self) -> usize {
        use bincode::Options;
        codec().serialized_size(self).unwrap_or(0) as usize
    }
    pub fn object_size(o: &(u32, ObjectDelta)) -> usize {
        use bincode::Options;
        codec().serialized_size(o).unwrap_or(0) as usize
    }
    pub fn action_size(a: &(u32, Option<Action>)) -> usize {
        use bincode::Options;
        codec().serialized_size(a).unwrap_or(0) as usize
    }
    /// Bytes `v` takes in a delta.
    pub fn field_size<T: Serialize>(v: &T) -> usize {
        use bincode::Options;
        codec().serialized_size(v).unwrap_or(0) as usize
    }
    pub fn encode(&self) -> Vec<u8> {
        use bincode::Options;
        codec().serialize(self).expect("a delta encodes")
    }
    pub fn decode(b: &[u8]) -> Option<Delta> {
        use bincode::Options;
        codec().with_limit(b.len() as u64).deserialize(b).ok()
    }

    /// Whether [`Snapshot::apply`] can take it: a server may send anything, and a client must not fall over or fill its memory for it.
    pub fn sane(&self) -> bool {
        const MOST_PLAYERS: u32 = 1 << 12;
        const MOST_OBJECTS: u32 = 1 << 20;
        if self.nplayers > MOST_PLAYERS || self.len > MOST_OBJECTS {
            return false;
        }
        // (a pre-game of an hour at most)
        if self.round.is_some_and(|(p, n)| !p.iter().all(|t| (0.0..=3600.0).contains(t)) || n > MOST_PLAYERS) {
            return false;
        }
        // (a player's soldier is one of the objects the receiver has: the server holds a player back until it is, and one past
        // the end made the copy fall over later. Its killer and `exact` may come before their objects: their readers look them up)
        self.players.iter().all(|p| p.0 < self.nplayers && p.1.soldier.is_none_or(|s| s < self.len))
            && self.items.iter().all(|t| t.at < MOST_OBJECTS)
            && self.names.iter().all(|n| n.0 < self.nplayers)
            && self.actions.iter().all(|a| a.0 < self.nplayers)
            && self.objects.iter().all(|(i, o)| *i < self.len && o.angles.as_ref().is_none_or(|(n, changed)| changed.iter().all(|c| c.0 < *n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(x: f32) -> ObjectState {
        ObjectState { template: "T34".into(), pos: [x, 0.0, 0.0], angles: vec![[0.0; 3]; 2], ..Default::default() }
    }

    #[test]
    fn the_bodies_are_linear_and_the_parts_not() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        for (t, m) in [("USSoldier", PredictionMode::Linear), ("GermanSoldier", PredictionMode::Linear), ("sherman", PredictionMode::Linear), ("bf109", PredictionMode::Linear), ("ShermanTurret", PredictionMode::None)] {
            assert_eq!(prediction_mode(&w, t), m, "{t}");
        }
    }

    /// a server takes a place removed for `REUSE_AFTER` s for its next object, not one a player's soldier still
    /// names; the copy builds the new object there afresh (its `born`), out of the wire.
    #[test]
    fn a_free_place_is_taken_and_the_copy_builds_it_again() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = Sim::new(&w, 1.0 / 30.0);
        let tank = s.spawn_vehicle("sherman", [0.0, 100.0, 0.0], [0.0; 3]).unwrap();
        let held = s.spawn_vehicle("USSoldier", [5.0, 100.0, 0.0], [0.0; 3]).unwrap();
        let p = s.add_player(2, "USSoldier", false);
        s.players[p].soldier = Some(held);
        let mut copy = Sim::new(&w, 1.0 / 30.0);
        Snapshot::of(&s).write(&mut copy);
        assert_eq!(copy.vehicles[tank].born, s.vehicles[tank].born);
        // a dead soldier left inside the tank (the monkey's «occupant of a vehicle out of the world»)
        let dead = s.spawn_vehicle("USSoldier", [7.0, 100.0, 0.0], [0.0; 3]).unwrap();
        s.vehicles[dead].entry.inside = Some(tank);
        s.retire(dead);
        s.retire(tank);
        s.retire(held);
        s.step();
        // not yet: the place waits
        let n = s.vehicles.len();
        assert_eq!(s.spawn_vehicle("GermanSoldier", [0.0; 3], [0.0; 3]), Some(n));
        s.clock.time += crate::REUSE_AFTER;
        let base = Snapshot::of(&s);
        let d0 = base.delta(None);
        Delta::decode(&d0.encode()).unwrap();
        Snapshot::apply(None, &d0).write(&mut copy);
        assert!(copy.vehicles[tank].removed);
        // the tank's place, and not the soldier's (its player names it)
        assert_eq!(s.spawn_vehicle("sherman", [9.0, 100.0, 0.0], [0.0; 3]), Some(tank));
        assert_eq!(s.vehicles[tank].id, tank);
        assert_eq!(s.vehicles[dead].entry.inside, None);
        // then the dead soldier's, never the held one's
        assert_eq!(s.spawn_vehicle("USSoldier", [9.0, 100.0, 0.0], [0.0; 3]), Some(dead));
        assert_eq!(s.spawn_vehicle("USSoldier", [9.0, 100.0, 0.0], [0.0; 3]), Some(n + 1));
        let d = Delta::decode(&Snapshot::of(&s).delta(Some(&base)).encode()).unwrap();
        Snapshot::apply(Some(&base), &d).write(&mut copy);
        let v = &copy.vehicles[tank];
        // (the same template: only `born` says it is another)
        assert_eq!((v.template.as_str(), v.born, v.removed), ("sherman", s.vehicles[tank].born, false));
        assert!(copy.role.copy && copy.spawn_vehicle("sherman", [0.0; 3], [0.0; 3]) == Some(copy.vehicles.len() - 1));
    }

    /// a player's soldier in the copy carries its kit's weapons, as the server's spawn gave them: the snapshot names the
    /// kit only (the copy drew empty hands and no ammunition)
    #[test]
    fn the_copys_soldiers_carry_their_kit() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = Sim::new(&w, 1.0 / 30.0);
        s.start_conquest();
        let p = s.add_player(1, "GermanSoldier", false);
        s.players[p].kit = 1;
        s.round.playing = true;
        for _ in 0..300 {
            if s.players[p].soldier.is_some() {
                break;
            }
            s.step();
        }
        let sol = s.players[p].soldier.expect("the player spawned");
        let theirs = s.vehicles[sol].movement.as_ref().and_then(|m| m.weapon.as_ref()).map(|f| f.template.name.clone());
        assert!(theirs.is_some(), "the server's soldier is armed");
        let mut copy = Sim::new(&w, 1.0 / 30.0);
        copy.start_conquest();
        copy.role.remote = true;
        Snapshot::of(&s).write(&mut copy);
        let ours = copy.vehicles[sol].movement.as_ref().and_then(|m| m.weapon.as_ref()).map(|f| f.template.name.clone());
        assert_eq!(ours, theirs);
    }

    /// the deaths a `remote` copy finds in its first snapshot are old news; those of a player new to a copy that
    /// has stepped are not (with jitter its first snapshot with it came after a death in its first step: Berlin, seed 1).
    #[test]
    fn a_player_new_to_a_stepped_copy_brings_its_deaths() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = Sim::new(&w, 1.0 / 30.0);
        let old = s.add_player(2, "USSoldier", false);
        s.players[old].deaths = 3;
        let mut copy = Sim::new(&w, 1.0 / 30.0);
        copy.role.remote = true;
        Snapshot::of(&s).write(&mut copy);
        copy.clock.steps = 10;
        let new = s.add_player(1, "GermanSoldier", true);
        s.players[new].deaths = 1;
        Snapshot::of(&s).write(&mut copy);
        let died: Vec<usize> = copy.journal.player_log.iter().filter(|e| matches!(e.2, crate::players::PlayerEvent::Died(_))).map(|e| e.1).collect();
        assert_eq!(died, vec![new]);
    }

    /// a player the server held back (`stale_players`) in a late joiner's first snapshot comes as a default
    /// entry; when it is sent, after the copy has stepped, its deaths are old news, not deaths to show (nor to kill its
    /// soldier for).
    #[test]
    fn a_player_held_back_in_the_first_snapshot_brings_no_deaths() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = Sim::new(&w, 1.0 / 30.0);
        let seen = s.add_player(1, "GermanSoldier", true);
        let held = s.add_player(2, "USSoldier", true);
        s.players[held].deaths = 3;
        s.players[seen].deaths = 2;
        let mut copy = Sim::new(&w, 1.0 / 30.0);
        copy.role.remote = true;
        // (what [`Snapshot::apply`] makes of a delta that had no room for it)
        let mut first = Snapshot::of(&s);
        first.players[held] = PlayerState::default();
        first.stale_players = vec![held as u32];
        first.write(&mut copy);
        assert_eq!((copy.players.len(), copy.players[held].deaths, copy.players[held].unsent), (2, 0, true));
        copy.clock.steps = 10;
        Snapshot::of(&s).write(&mut copy);
        assert_eq!((copy.players[held].deaths, copy.players[held].unsent), (3, false));
        // a death after that is news
        s.players[held].deaths = 4;
        copy.clock.steps = 20;
        Snapshot::of(&s).write(&mut copy);
        let died: Vec<usize> = copy.journal.player_log.iter().filter(|e| matches!(e.2, crate::players::PlayerEvent::Died(_))).map(|e| e.1).collect();
        assert_eq!(died, vec![held]);
    }

    #[test]
    fn a_rotation_survives_its_quaternion() {
        let mut seed = 7u64;
        let mut r = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 40) as f32 / (1u64 << 24) as f32 * 6.4 - 3.2
        };
        for _ in 0..1000 {
            let (y, p, l) = (r(), r(), r());
            let (q0, q1, q2) = (q::rows([0, (y / 2.0).sin().mul_add(32767.0, 0.0) as i16, 0, ((y / 2.0).cos() * 32767.0) as i16]), q::rows([((p / 2.0).sin() * 32767.0) as i16, 0, 0, ((p / 2.0).cos() * 32767.0) as i16]), q::rows([0, 0, ((l / 2.0).sin() * 32767.0) as i16, ((l / 2.0).cos() * 32767.0) as i16]));
            let mul = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| -> [[f32; 3]; 3] { std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum())) };
            let m = mul(mul(q0, q1), q2);
            let back = q::rows(q::quat(&m));
            let e = (0..9).map(|k| (m[k / 3][k % 3] - back[k / 3][k % 3]).abs()).fold(0.0, f32::max);
            assert!(e < 1e-4, "{m:?} -> {back:?}");
        }
    }

    #[test]
    fn a_delta_carries_only_what_changed_and_rebuilds_the_snapshot() {
        let a = Snapshot { step: 1, objects: vec![object(1.0), object(2.0), object(3.0)], ..Default::default() };
        let mut b = a.clone();
        b.step = 5;
        b.objects[1].pos[0] = 2.5;
        b.objects.push(object(4.0));
        let d = b.delta(Some(&a));
        assert_eq!(d.objects.iter().map(|o| o.0).collect::<Vec<_>>(), vec![1, 3]);
        let d = Delta::decode(&d.encode()).unwrap();
        assert_eq!(Snapshot::apply(Some(&a), &d), b);
        assert_eq!(Snapshot::apply(None, &b.delta(None)), b);
    }

    /// A server that names a soldier past the end of its world is not taken (the copy indexes its vehicles with it).
    #[test]
    fn a_soldier_past_the_world_is_not_sane() {
        let a = Snapshot { step: 1, objects: vec![object(1.0), object(2.0)], ..Default::default() };
        let mut b = Snapshot { step: 2, players: vec![PlayerState { alive: true, soldier: Some(1), ..Default::default() }], ..a.clone() };
        assert!(b.delta(Some(&a)).sane());
        b.players[0].soldier = Some(2);
        assert!(!b.delta(Some(&a)).sane());
    }

    /// The server's rules go whole against nothing, and again only when they change.
    #[test]
    fn the_rules_go_when_they_change() {
        let a = Snapshot { step: 1, rules: crate::damage::ServerRules::default().with_friendly_fire(30.0), ..Default::default() };
        assert_eq!(a.delta(None).rules, Some(a.rules));
        let mut b = Snapshot { step: 2, ..a.clone() };
        assert_eq!(b.delta(Some(&a)).rules, None);
        b.rules.nose_cam = false;
        let d = Delta::decode(&b.delta(Some(&a)).encode()).unwrap();
        assert_eq!(Snapshot::apply(Some(&a), &d).rules, b.rules);
    }
}
