//! Players: a team, a soldier, death and respawn, read from the original game:
//! The server's kill player (the time to the next spawn, the
//! tickets a death costs) and `spawnPlayer` (a spawn group of its team, one of its spawn points
//! at random: the spawn group's spawn point lookup).
//!
//! The bench's players spawn by themselves (as bots do) once their time is up, in the group they
//! chose or else the first of their team that has spawn points.

use serde::Serialize;

/// `game.serverSpawnDelay` (`setSpawnDelay`): seconds from a death to the
/// next spawn without spawn waves (the server's settings: 3).
pub const SPAWN_DELAY: f32 = 3.0;
/// Spawn waves (on by default), as the server on El Alamein in coop
/// (`cq_alamein_respawn`): a cycle of `add` (20 s: `wait`, counts down to 0)
/// and then `period` (5 s: `to_next`, counts down; when it passes 0, `wait` is
/// `add` again and `to_next` is `period`). Both run with the physics' dt.
#[derive(Debug, Clone, Serialize)]
pub struct Waves {
    pub wait: f32,
    pub to_next: f32,
    pub period: f32,
    pub add: f32,
}

impl Default for Waves {
    fn default() -> Waves {
        Waves { wait: 0.0, to_next: 5.0, period: 5.0, add: 20.0 }
    }
}

impl Waves {
    /// `killPlayer`'s time to a dead player's spawn (no team-kill penalty): in the long part of
    /// the cycle what is left of it; in the short one, its rest and a whole long one more.
    pub fn spawn_time(&self) -> f32 {
        let t = if self.period < self.wait { self.wait } else { self.to_next + self.add + self.wait };
        t.max(0.0)
    }

    pub fn frame(&mut self, dt: f32) {
        if self.wait > 0.0 {
            self.wait = (self.wait - dt).max(0.0);
        } else {
            self.to_next -= dt;
            if self.to_next <= 0.0 {
                self.wait = self.add;
                self.to_next = self.period;
            }
        }
    }
}

/// The game's setting of time allowed outside world (the constructor's value): seconds a player may spend outside the play area.
pub const TIME_ALLOWED_OUTSIDE: f32 = 10.0;
/// The server's setting of damage for being outside world (the constructor's value): hit points a second past that time.
pub const DAMAGE_FOR_BEING_OUTSIDE: f32 = 5.0;

/// The server's setting of material to give damage (the constructor's value): the terrain material that counts as outside.
pub const MATERIAL_TO_GIVE_DAMAGE: u32 = 7;

/// `setTicketLosePerDeath`: tickets a human player's death costs its team (1).
pub const TICKETS_PER_DEATH: i32 = 1;
/// `killPlayer` keeps the time to a spawn within 0..127 s.
pub const MAX_SPAWN_TIME: f32 = 127.0;

/// Team kills (the constructor's values): with spawn waves,
/// a player's death adds round(its team kills × `penalty`) whole cycles to its wait, and the
/// count starts again; `kick` team kills in all and it is out (0: never); with
/// `punish_mode` (`serverTKPunishMode`, the victim chooses) a team kill is not counted by itself.
#[derive(Debug, Clone, Serialize)]
pub struct TeamKills {
    pub penalty: f32,
    pub kick: f32,
    pub punish_mode: bool,
}

impl Default for TeamKills {
    fn default() -> TeamKills {
        TeamKills { penalty: 1.0, kick: 5.0, punish_mode: false }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Player {
    /// its name (`getName`): a bot's, [`crate::Sim::random_name_for_team`]; empty, none given
    pub name: String,
    /// its team
    pub team: u32,
    /// the soldier it spawns as (its kit's)
    pub template: String,
    /// a bot (`isAI`): its death costs one ticket (`subTicket`); a human's, `ticketLosePerDeath`
    pub ai: bool,
    /// its soldier now (a vehicle index), alive or its body
    pub soldier: Option<usize>,
    /// whether it is alive
    pub alive: bool,
    /// seconds to its next spawn
    pub spawn_time: f32,
    /// the spawn group it chose (None: the first of its team)
    pub group: Option<i64>,
    /// the kit it chose: an index in its team's (`game.setKit`)
    pub kit: usize,
    pub deaths: u32,
    /// enemies it killed
    pub kills: u32,
    /// as the scoreboard counts them: flags taken (`Attack`), team kills (`TK`)
    pub captures: u32,
    pub tks: u32,
    /// its score (the score manager's score event)
    pub score: i32,
    /// team kills not yet paid for and in all; kicked for them
    pub team_kills: u32,
    pub team_kills_total: u32,
    pub kicked: bool,
    /// a person still choosing on the screen when dead (its first spawn, as the original's, waits
    /// for it): it does not spawn however its time runs (ours: the client says when)
    pub hold: bool,
    /// seconds outside the play area (see [`crate::Sim::update_outside`]); 0 inside
    pub outside: f32,
    /// a copy's: the server held it back (`stale_players`) in the snapshot that brought it, so what the copy has is a
    /// default entry and its first real one is not news
    #[serde(skip)]
    pub unsent: bool,
}

impl Player {
    pub fn new(team: u32, template: &str, ai: bool) -> Player {
        Player { name: String::new(), team, template: template.to_string(), ai, soldier: None, alive: false, spawn_time: 0.0, group: None, kit: 0, deaths: 0, kills: 0, captures: 0, tks: 0, score: 0, team_kills: 0, team_kills_total: 0, kicked: false, hold: false, outside: 0.0, unsent: false }
    }
}

/// The points of each `ScoreMsg` (the constructor's values, which
/// `Bf1942/Game/ScoreManagerSettings*.con` set for each mode: the game's load).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScoreRules {
    pub death: i32,
    pub kill: i32,
    pub tk: i32,
    pub capture: i32,
    pub attack: i32,
    pub defence: i32,
    pub objective: i32,
    pub objective_tk: i32,
}

impl Default for ScoreRules {
    fn default() -> ScoreRules {
        ScoreRules { death: -1, kill: 3, tk: -3, capture: 20, attack: 5, defence: 5, objective: 5, objective_tk: -15 }
    }
}

impl ScoreRules {
    /// The constructor's values with a settings file's `ScoreManager.<key> <n>` lines over them.
    pub fn parse(text: &str) -> ScoreRules {
        let mut r = ScoreRules::default();
        for line in text.lines() {
            let mut w = line.split_whitespace();
            let (Some(k), Some(v)) = (w.next(), w.next().and_then(|v| v.parse::<i32>().ok())) else { continue };
            let Some(k) = k.to_ascii_lowercase().strip_prefix("scoremanager.").map(str::to_string) else { continue };
            match k.as_str() {
                "death" => r.death = v,
                "kill" => r.kill = v,
                "tk" => r.tk = v,
                "capture" => r.capture = v,
                "attack" => r.attack = v,
                "defence" => r.defence = v,
                "objective" => r.objective = v,
                "objectivetk" => r.objective_tk = v,
                _ => {}
            }
        }
        r
    }
}

/// The `ScoreMsg`s the bench gives (the server's score handler's 3, 4/5, 6 and 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Score {
    Kill,
    Death,
    Tk,
    /// in the radius of a point its team takes (the control point's frame update handling, `gotControl`);
    /// in CTF, taking a flag
    Attack,
    /// sending his team's flag home (CTF)
    Defence,
    /// bringing the enemy's flag to his base (`FlagCapture`)
    Capture,
    /// destroying an objective's target of his team (`Objective`), of the other's (`ObjectiveTK`)
    Objective,
    ObjectiveTk,
}

/// What happened to a player (the bench's log).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum PlayerEvent {
    /// its soldier (this vehicle) died
    Died(usize),
    /// it spawned as this vehicle, at this spawn point of this group
    Spawned(usize, i64, usize),
    /// it killed this player of its own team
    TeamKill(usize),
    /// out of the game for its team kills
    Kicked,
}

/// A small deterministic generator for the spawn points (the server's `rand`).
#[derive(Debug, Clone, Serialize)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u32 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as u32
    }
}

/// `BYTEFIELD_CONQUEST_SPAWN_MAP=0`: conquest's bots take any spawn point, also those off the AI map.
fn conquest_spawn_map() -> bool {
    crate::debug::options().conquest_spawn_map
}

impl crate::Sim<'_> {
    /// `giveDamage` killing player `victim`: who did it (the vehicle the damage came from: its
    /// soldier's player, or the player inside it), and if that is another human of its team (bots
    /// do not count), a team kill: counted unless the victim chooses (`punish_mode`); at `kick`
    /// in all the killer is out.
    /// Its points too: `Kill` to a killer of the other team, `TK` to one of its own, and to itself
    /// when it killed itself (its own grenade or charge; not counted as a team kill).
    fn team_kill(&mut self, victim: usize) {
        let Some(by) = self.players[victim].soldier.and_then(|s| self.vehicles.get(s)).and_then(|v| v.armor.as_ref()).and_then(|a| a.killed_by) else { return };
        let Some(j) = self.player_of_attacker(by) else { return };
        self.score(j, if self.players[j].team == self.players[victim].team { Score::Tk } else { Score::Kill });
        let (a, b) = (&self.players[j], &self.players[victim]);
        if j == victim || a.team != b.team || a.ai || b.ai || self.rules.team_kills.punish_mode {
            return;
        }
        let step = self.clock.steps;
        let p = &mut self.players[j];
        p.team_kills += 1;
        p.team_kills_total += 1;
        self.journal.player_log.push((step, j, PlayerEvent::TeamKill(victim)));
        if self.rules.team_kills.kick > 0.0 && p.team_kills_total as f32 >= self.rules.team_kills.kick && !p.kicked {
            self.journal.player_log.push((step, j, PlayerEvent::Kicked));
            self.remove_player(j);
        }
    }

    /// The player behind attacker `by` (`giveDamage`'s, a vehicle index): its soldier's, or the one inside it.
    pub fn player_of_attacker(&self, by: usize) -> Option<usize> {
        (0..self.players.len()).find(|&j| self.players[j].soldier.is_some_and(|s| s == by || self.vehicles.get(s).is_some_and(|v| v.entry.inside == Some(by))))
    }

    /// Player `k` is out of the round: its soldier goes and it no longer spawns or counts for a team.
    pub fn remove_player(&mut self, k: usize) {
        let Some(p) = self.players.get_mut(k) else { return };
        p.kicked = true;
        p.alive = false;
        if let Some(s) = p.soldier.take() {
            // The soldier's immediate destroy: out of its vehicle first (the controlled object's exit)
            if let Some(v) = self.vehicles[s].entry.inside {
                self.exit(s, v);
            }
            self.vehicles[s].removed = true;
        }
    }

    /// The server update's bots: the deactivated ones already dead go; then, as `gameStatusPreGame`/`Playing` fill
    /// the server (`autoSpawnBots`, while the bot count is under the places), one bot joins to [`crate::Sim::team_to_add`]'s team, its kit by turns as
    /// the first ones'. Not in a `remote` copy: its players are the server's.
    fn update_bot_places(&mut self) {
        if self.rules.bot_places == 0 || self.role.remote {
            return;
        }
        let gone: Vec<usize> = self.round.leaving.iter().copied().filter(|&k| self.players.get(k).is_none_or(|p| !p.alive)).collect();
        for k in gone {
            self.round.leaving.retain(|&q| q != k);
            self.remove_player(k);
        }
        let n = self.players.iter().filter(|p| !p.kicked).count();
        if n < self.rules.bot_places && self.round.round_end.is_none() {
            let p = self.join(0, true);
            self.players[p].kit = (self.players.iter().filter(|q| q.ai && !q.kicked).count().saturating_sub(1) / 2) % 5;
        }
    }

    /// A person joining: the place of one who left, or a new one. `kicked` for team kills keeps its place only until someone takes it.
    pub fn join_person(&mut self, team: u32) -> usize {
        let team = if team == 0 { self.team_to_add() } else { team };
        match self.players.iter().position(|p| p.kicked && !p.ai) {
            Some(k) => {
                self.players[k] = crate::players::Player::new(team, crate::replica::soldier_template(team), false);
                k
            }
            None => self.join(team, false),
        }
    }

    /// The spawn point's active lookup: inside the combat area, high enough over the ground
    /// or water if the template asks it, and, if it puts the soldier in a vehicle, with a free
    /// entry point of the team's within 5 m (a bot: 15), whatever its entry radius (`getObjectToEnter`). (A point on a vehicle that is
    /// destroyed is out of its group: `sync_carried_spawn_points`.) Not modeled: the terrain's
    /// out-of-bounds test.
    /// [`Self::spawn_point_active`], for the tools.
    pub fn spawn_point_active_pub(&self, sp: &crate::conquest::SpawnPoint, team: u32, ai: bool) -> bool {
        self.spawn_point_active(sp, team, ai)
    }

    pub(crate) fn spawn_point_active(&self, sp: &crate::conquest::SpawnPoint, team: u32, ai: bool) -> bool {
        let (x, y, z) = (sp.pos[0], sp.pos[1], sp.pos[2]);
        if let Some([cx, cz, w, h]) = self.world.level.combat_area.map(|a| a.map(|v| v as f32))
            && (x < cx || z < cz || cx + w < x || cz + h <= z)
        {
            return false;
        }
        if sp.min_height != -1.0 {
            let ground = self.world.height_at(x as f64, z as f64).unwrap_or(0.0) as f32;
            let water = self.world.level.terrain.water_level as f32;
            if y - ground.max(water) < sp.min_height {
                return false;
            }
        }
        if (sp.ai_enter_on_spawn && ai) || sp.enter_on_spawn {
            let r = if ai { 15.0 } else { 5.0 };
            return self.entry_point_near(sp.pos, team, r, false).is_some();
        }
        true
    }

    /// What a person chose for its next spawn (the screen when dead): a spawn group (None: the
    /// first of its team) and a kit of its team's.
    pub fn choose_spawn(&mut self, k: usize, group: Option<i64>, kit: usize) {
        let Some(team) = self.players.get(k).map(|p| p.team as usize) else { return };
        let c = self.modes.conquest.as_ref();
        let kits = c.and_then(|c| c.teams.kits.get(team.wrapping_sub(1))).map_or(1, |ks| ks.len().max(1));
        let group = group.filter(|g| c.is_some_and(|c| c.groups.contains_key(g)));
        let p = &mut self.players[k];
        p.group = group;
        p.kit = kit.min(kits - 1);
    }

    /// Player `k` waits on the screen when dead until it says (`hold`), or spawns when its time is up.
    pub fn hold_spawn(&mut self, k: usize, hold: bool) {
        if let Some(p) = self.players.get_mut(k) {
            p.hold = hold;
        }
    }

    /// The spawn groups player `k` may choose now: its team's with spawn points and open to it
    /// (`onlyForAI`, `onlyForHuman`), by id.
    pub fn spawn_groups(&self, k: usize) -> Vec<i64> {
        let (Some(c), Some(p)) = (self.modes.conquest.as_ref(), self.players.get(k)) else { return Vec::new() };
        c.groups.iter().filter(|(_, g)| g.team == p.team as i64 && !g.spawns.is_empty() && g.open_to(p.ai)).map(|(id, _)| *id).collect()
    }

    /// A player of `team` that spawns as `template` (alive at its first spawn).
    /// The score manager's score event: the points of `what` to player `k`, only of teams 1 and 2 and
    /// not once the round is over (game status 2 or 5).
    pub fn score(&mut self, k: usize, what: Score) {
        let r = self.rules.scores;
        let ended = self.modes.conquest.as_ref().is_some_and(|c| c.winner.is_some());
        let Some(p) = self.players.get_mut(k).filter(|p| (1..=2).contains(&p.team)) else { return };
        if ended {
            return;
        }
        p.score += match what {
            Score::Kill => {
                p.kills += 1;
                r.kill
            }
            // (the deaths themselves are `killPlayer`'s count)
            Score::Death => r.death,
            Score::Tk => {
                p.tks += 1;
                r.tk
            }
            Score::Attack => {
                p.captures += 1;
                r.attack
            }
            Score::Defence => r.defence,
            Score::Capture => {
                p.captures += 1;
                r.capture
            }
            Score::Objective => r.objective,
            Score::ObjectiveTk => r.objective_tk,
        };
    }

    /// The server's suicide (the console's and the menu's), while playing: player `k` gets a TK's
    /// points (not counted as a team kill), then `killPlayer` (its death).
    pub fn suicide(&mut self, k: usize) {
        let Some(s) = self.players.get(k).filter(|p| p.alive && self.round.playing).and_then(|p| p.soldier) else { return };
        self.score(k, Score::Tk);
        match self.vehicles.get(s).and_then(|v| v.entry.inside) {
            Some(v) => self.kill_driver(v, s),
            None => self.give_damage(s, crate::entry::KILL_DAMAGE, crate::damage::Source::Kill),
        }
    }

    pub fn add_player(&mut self, team: u32, template: &str, ai: bool) -> usize {
        self.players.push(Player::new(team, template, ai));
        self.players.len() - 1
    }

    /// `createPlayer` with its team: 0 asks [`crate::Sim::team_to_add`] (as the server's bots filling up,
    /// `gameStatusPlaying`); the team's soldier.
    pub fn join(&mut self, team: u32, ai: bool) -> usize {
        let team = if team == 0 { self.team_to_add() } else { team };
        // (a bot's name, before `createUnspawnedAIPlayer`)
        let name = if ai { self.random_name_for_team(team) } else { String::new() };
        let k = self.add_player(team, crate::replica::soldier_template(team), ai);
        self.players[k].name = name;
        k
    }

    /// The game's random name for team lookup: a name no player has (`getPlayerFromName`,
    /// any case) from the team's lists (`game.addFirstNameOnTeam` and
    /// `addSecondNameOnTeam`, which the server's load bots runs from the level's `SinglePlayer/`).
    /// Twenty tries at random of a first name with the second of the same index (of the shorter
    /// list's); with `randomNames` any first with any second. Then, without it, each pair
    /// in order; then the last tried with 1, 2… after it. With no lists, `Player`, `Player2`…
    pub fn random_name_for_team(&mut self, team: u32) -> String {
        let (mut first, mut second, mut random) = (Vec::new(), Vec::new(), false);
        for c in &self.world.db.globals {
            let name = || c.args.get(1).map(|n| n.trim()).filter(|_| c.args.first().and_then(|t| t.trim().parse::<u32>().ok()) == Some(team));
            match c.command.to_ascii_lowercase().as_str() {
                "game.addfirstnameonteam" => first.extend(name()),
                "game.addsecondnameonteam" => second.extend(name()),
                // (`setRandomNames` sets it whatever it is given)
                "game.setrandomnames" | "game.randomnames" => random = true,
                _ => {}
            }
        }
        let taken = |players: &[Player], n: &str| players.iter().any(|p| !p.kicked && p.name.eq_ignore_ascii_case(n));
        if !(1..=2).contains(&team) || first.is_empty() || second.is_empty() {
            return (1..).map(|k| if k == 1 { "Player".to_string() } else { format!("Player{k}") }).find(|n| !taken(&self.players, n)).unwrap();
        }
        let n = if random { first.len() } else { first.len().min(second.len()) } as u32;
        let mut last = String::new();
        for _ in 0..20 {
            let i = (self.rng.next() % n) as usize;
            let j = if random { (self.rng.next() % second.len() as u32) as usize } else { i };
            last = format!("{} {}", first[i], second[j]);
            if !taken(&self.players, &last) {
                return last;
            }
        }
        if !random && let Some(free) = (0..n as usize).map(|i| format!("{} {}", first[i], second[i])).find(|c| !taken(&self.players, c)) {
            return free;
        }
        (1..).map(|k| format!("{last}{k}")).find(|n| !taken(&self.players, n)).unwrap()
    }

    /// The player manager's team to add player lookup: the team whose share of the players (bots
    /// too) is further under its ratio's; even, the one nearer its share with the new one in; still
    /// even (and with no one yet, 0/0), either at random (`rand & 1`).
    pub fn team_to_add(&mut self) -> u32 {
        let n = |t| self.players.iter().filter(|p| p.team == t && !p.kicked).count() as f64;
        let (n1, n2) = (n(1), n(2));
        let [r1, r2] = self.rules.team_ratio.map(f64::from);
        let (r1, r2) = (r1 / (r1 + r2), r2 / (r1 + r2));
        let (d1, d2) = (n1 / (n1 + n2) - r1, n2 / (n1 + n2) - r2);
        if d2 < d1 {
            return 2;
        }
        if d1 < d2 {
            return 1;
        }
        let m = n1 + n2 + 1.0;
        let to1 = ((n1 + 1.0) / m - r1 - (n2 / m - r2)).abs();
        let to2 = (n1 / m - r1 - ((n2 + 1.0) / m - r2)).abs();
        if to2 < to1 {
            2
        } else if to1 < to2 {
            1
        } else {
            1 + (self.rng.next() & 1)
        }
    }

    /// The server's setting of team: player `k` to `team` (0, or any with `auto_balance`:
    /// [`crate::Sim::team_to_add`]); another team forgets its group and takes that
    /// team's soldier. (Alive, it would die: `killPlayer`; only the dead are moved here.)
    pub fn set_team(&mut self, k: usize, team: u32) {
        let team = if team == 0 || self.rules.auto_balance { self.team_to_add() } else { team };
        let p = &mut self.players[k];
        if p.team != team {
            p.group = None;
            p.template = crate::replica::soldier_template(team).to_string();
        }
        p.team = team;
    }

    /// `killPlayer` for every alive player whose soldier is destroyed or gone: dead, its spawn
    /// time `SPAWN_DELAY`; in conquest the team loses a ticket (`subTicket` for a bot, tickets −
    /// `ticketLosePerDeath` for a human). Then the dead ones' time runs, and at 0 they spawn
    /// ([`crate::Sim::spawn_player`]), one every third frame.
    /// Is player `k` outside the play area now? (Where its vehicle is, if it is in one.)
    pub fn player_outside(&self, k: usize) -> bool {
        let Some(s) = self.players.get(k).filter(|p| p.alive).and_then(|p| p.soldier) else { return false };
        let on = self.vehicles.iter().position(|o| o.entry.occupants.contains(&Some(s))).unwrap_or(s);
        self.vehicles.get(on).is_some_and(|v| self.outside_play(v.craft.body.pos))
    }

    /// Outside the play area (`gameStatusPlaying`): beyond the active combat area (without one, the
    /// map), or over the terrain material that hurts (`getMaterialToGiveDamage`, 7).
    pub fn outside_play(&self, pos: [f32; 3]) -> bool {
        let level = &self.world.level;
        let ws = level.terrain.world_size;
        let [cx, cz, w, h] = level.combat_area.unwrap_or([0.0, 0.0, ws, ws]).map(|v| v as f32);
        let (x, z) = (pos[0], pos[2]);
        !(cx <= x && x <= cx + w && cz <= z && z <= cz + h) || level.material_at(x as f64, z as f64) == MATERIAL_TO_GIVE_DAMAGE
    }

    /// The deserters (`gameStatusPlaying`, for every player with a soldier): a timer runs while
    /// outside the play area and goes back to 0 inside; past its time each frame costs dt × the
    /// damage, to the soldier also inside a vehicle (the original: the T34 stays whole), from nobody. Bots too. Checked
    /// against the original.
    fn update_outside(&mut self, dt: f32) {
        for k in 0..self.players.len() {
            if !self.player_outside(k) {
                self.players[k].outside = 0.0;
                continue;
            }
            let t = self.players[k].outside + dt;
            if t <= TIME_ALLOWED_OUTSIDE {
                self.players[k].outside = t;
                continue;
            }
            self.players[k].outside = TIME_ALLOWED_OUTSIDE;
            let Some(s) = self.players[k].soldier else { continue };
            let by = std::mem::take(&mut self.damage.attacker);
            self.give_damage(s, dt * DAMAGE_FOR_BEING_OUTSIDE, crate::damage::Source::Outside);
            self.damage.attacker = by;
        }
    }

    pub(crate) fn update_players(&mut self, dt: f32) {
        let step = self.clock.steps;
        self.update_bot_places();
        if self.round.playing {
            self.update_outside(dt);
        }
        self.update_items();
        if let Some(w) = self.round.waves.as_mut() {
            w.frame(dt);
        }
        for k in 0..self.players.len() {
            let p = &self.players[k];
            if p.alive {
                // (not in a `remote` copy: who died is the server's, by its snapshot's count, [`Snapshot::write`]; the copy's own guess killed
                // a soldier just spawned and not yet sent every step)
                let dead = !self.role.remote && p.soldier.and_then(|i| self.vehicles.get(i)).is_none_or(|v| v.removed || v.armor.as_ref().is_some_and(|a| a.destroyed));
                if !dead {
                    continue;
                }
                let (team, ai, sol) = (p.team, p.ai, p.soldier);
                // a carrier's flag falls where he was
                if let Some(at) = sol.and_then(|i| self.vehicles.get(i)).map(|v| v.craft.body.pos)
                    && self.modes.ctf.as_ref().is_some_and(|c| c.carries(k))
                {
                    self.ctf_drop(k, at);
                }
                // its kit falls where it was (`dropKit`); killed in a seat (the player's object is the vehicle, not the
                // soldier), `killPlayer` destroys it at once (the
                // T72's gunner's US_Sniper goes the frame he dies; on foot it lies 30 s)
                if let Some(i) = sol {
                    let seated = self.vehicles[i].entry.inside.is_some();
                    let kit = self.vehicles[i].soldier().and_then(|m| m.kit);
                    self.drop_kit(i, true);
                    if seated && let Some(t) = kit.and_then(|k| self.items.get_mut(k)) {
                        t.removed = true;
                    }
                }
                // killed in a seat (a gunner shot, not its vehicle destroyed: that is `kill_driver`'s): `killPlayer` gets it out
                // (`exitVehicle`) and, as it was inside, destroys it at once.
                // Without it the seat kept the dead one, and its place, for good. A gunner
                // killed with the tank whole, the same frame (T72MG_PCO1
                // free and the soldier gone at record 120).
                if let Some(i) = sol.filter(|&i| !self.vehicles[i].removed)
                    && let Some(v) = self.vehicles[i].entry.inside
                {
                    let seat = self.vehicles[i].entry.position;
                    if self.vehicles[v].entry.occupants.get(seat) == Some(&Some(i)) {
                        self.leave_seat(v, seat);
                    }
                    self.vehicles[i].entry.inside = None;
                    self.vehicles[i].entry.wants = None;
                    self.vehicles[i].removed = true;
                }
                self.team_kill(k);
                self.score(k, Score::Death);
                let by_sai = ai && self.better_bots_spawn();
                let p = &mut self.players[k];
                p.alive = false;
                p.deaths += 1;
                // (a bot of the SAI's chooses again)
                if by_sai {
                    p.group = None;
                }
                // with waves, its own team kills cost it whole cycles (and are paid)
                p.spawn_time = match self.round.waves.as_ref() {
                    Some(w) => {
                        let n = (p.team_kills as f32 * self.rules.team_kills.penalty).round_ties_even();
                        p.team_kills = 0;
                        (w.spawn_time() + n * (w.add + w.period)).clamp(0.0, MAX_SPAWN_TIME)
                    }
                    None => SPAWN_DELAY,
                };
                if let Some(v) = sol.and_then(|i| self.vehicles.get_mut(i))
                    && v.soldier().is_some()
                {
                    // nobody drives it now: without the input, the speed it last asked of the ground would stay, and the body walk on
                    v.set_walk([0.0; 3]);
                }
                // (a ticket per death in conquest and coop: `killPlayer`; in objective mode the
                // attackers' only, from their own count, not the scaled one, per the objective manager's
                // `defenderLoseTicketsOnDeath` false and `attackerLoseTicketsOnDeath` true)
                let defender = self.modes.objectives.as_ref().map(|o| o.defender);
                if let Some(c) = self.modes.conquest.as_mut().filter(|c| c.bleed)
                    && (1..=2).contains(&team)
                    && defender != Some(team)
                {
                    if let Some(n) = c.normal.as_mut() {
                        n[team as usize - 1] -= TICKETS_PER_DEATH;
                    } else {
                        let t = &mut c.tickets[team as usize - 1];
                        *t = if ai { (*t - 1).max(0) } else { *t - TICKETS_PER_DEATH };
                    }
                }
                self.journal.player_log.push((step, k, PlayerEvent::Died(sol.unwrap_or(usize::MAX))));
                // `killPlayer`'s end: with `serverAutoBalanceTeams`, a human of two teams more than
                // one apart goes to the shorter (outside the end of a round)
                if !ai && self.rules.auto_balance && self.round.round_end.is_none() {
                    let n = |t| self.players.iter().filter(|p| p.team == t && !p.kicked).count() as i64;
                    if (n(1) - n(2)).abs() > 1 {
                        self.set_team(k, 0);
                    }
                }
                // and a bot, with more players than places, of the team that would not get the next one: `deactivateBot`, to
                // a list, and gone at the next update (the server's update). Its place frees one for a person
                if ai && self.rules.bot_places > 0 && self.round.round_end.is_none() && !self.round.leaving.contains(&k) {
                    let n = self.players.iter().filter(|p| !p.kicked).count();
                    if self.round.leaving.len() + self.rules.bot_places < n && team != self.team_to_add() {
                        self.round.leaving.push(k);
                    }
                }
                continue;
            }
            if self.players[k].kicked {
                continue;
            }
            // its time runs down to 0
            self.players[k].spawn_time = (self.players[k].spawn_time - dt).max(0.0);
        }
        // `gameStatusPlaying` (on): every third frame the first player whose time is up
        // and that has a group spawns, at most one (in the players' order; one whose spawn fails
        // lets the next try)
        // (not in a `remote` copy: who spawns, where and as what is the server's, by its snapshot; the copy's own spawn, put right by
        // the next snapshot, died again and again)
        if self.round.spawn_delay == 0 && !self.role.remote {
            for k in 0..self.players.len() {
                let p = &self.players[k];
                if !p.alive && !p.kicked && !p.hold && p.spawn_time <= 0.0 && self.spawn_player(k) {
                    break;
                }
            }
            self.round.spawn_delay = 2;
        } else {
            self.round.spawn_delay -= 1;
        }
    }

    /// `spawnPlayer`: player `k` in a group of its team (its own, or the first with spawn points;
    /// a bot of the SAI's waits for its group and chooses again when it does not spawn),
    /// at one of its points at random, the soldier there turned as it is. Whether it spawned.
    /// Team `team`'s kit number `kit` (`game.setKit`), the one a spawn hands its soldier.
    pub fn team_kit(&self, team: u32, kit: usize) -> Option<String> {
        let c = self.modes.conquest.as_ref()?;
        c.teams.kits.get((team as usize).checked_sub(1)?).and_then(|ks| ks.get(kit).cloned().flatten())
    }

    fn spawn_player(&mut self, k: usize) -> bool {
        let step = self.clock.steps;
        // (a bot with a mind picks its group and kit as a person would on the spawn screen: [`Mind::choose_spawn`])
        self.ask_mind_spawn(k);
        let Some(c) = self.modes.conquest.as_ref() else { return false };
        let team = self.players[k].team as i64;
        let ai = self.players[k].ai;
        // a bot of the SAI's waits for its group (`spawnPlayer` with its group at 0), and
        // chooses again when it does not do (the group back to 0)
        let by_sai = ai && self.better_bots_spawn();
        let ok = |g: &crate::conquest::SpawnGroup| g.team == team && !g.spawns.is_empty() && g.open_to(ai);
        // (a person whose group is no longer its team's spawns in the first that is: ours; one with no point open to it
        // is none, or it would wait for ever: Coral Sea's first has only the bots')
        let usable = |g: &crate::conquest::SpawnGroup| ok(g) && (by_sai || g.spawns.iter().any(|s| self.spawn_point_active(s, team as u32, ai)));
        let first = || c.groups.iter().find(|(_, g)| usable(g)).map(|(id, _)| *id);
        let group = match self.players[k].group {
            Some(g) if by_sai => c.groups.get(&g).filter(|g| ok(g)).map(|_| g),
            Some(g) => c.groups.get(&g).filter(|g| usable(g)).map(|_| g).or_else(first),
            None if by_sai => None,
            None => first(),
        };
        let Some(g) = group else {
            if by_sai {
                self.players[k].group = None;
            }
            return false;
        };
        // The spawn group's spawn point lookup: one of its active points, `rand` % their number
        let mut active: Vec<usize> = (0..c.groups[&g].spawns.len()).filter(|&j| self.spawn_point_active(&c.groups[&g].spawns[j], team as u32, ai)).collect();
        // (ours, conquest's better bots, where the original has none: a point off the AI map leaves the bot
        // pressing against a wall; if the group has others, those. `BYTEFIELD_CONQUEST_SPAWN_MAP=0` takes them all)
        if ai && self.world.mode != "GPM_COOP" && conquest_spawn_map() {
            let points: Vec<(usize, [f32; 3], bool)> = active.iter().map(|&j| (j, c.groups[&g].spawns[j].pos, c.groups[&g].spawns[j].enter_on_spawn || c.groups[&g].spawns[j].ai_enter_on_spawn)).collect();
            self.keep_points_on_map(points, &mut active);
        }
        let c = self.modes.conquest.as_ref().unwrap();
        if active.is_empty() {
            if by_sai {
                self.players[k].group = None;
            }
            return false;
        }
        let n = active[self.rng.next() as usize % active.len()];
        let sp = c.groups[&g].spawns[n];
        // the team's soldier (`setTeamSkin`) with the chosen kit
        let skin = c.teams.skins.get(team as usize - 1).cloned().flatten();
        let kit = c.teams.kits.get(team as usize - 1).and_then(|ks| ks.get(self.players[k].kit).cloned().flatten());
        let t = skin.unwrap_or_else(|| self.players[k].template.clone());
        // a metre over the point, unless it drops paratroopers (they fall at 3 m/s, the
        // parachute open)
        let pos = if sp.paratrooper { sp.pos } else { [sp.pos[0], sp.pos[1] + 1.0, sp.pos[2]] };
        let Some(i) = self.spawn_vehicle(&t, pos, sp.rot) else { return false };
        self.vehicles[i].team = self.players[k].team;
        if let Some(kit) = kit {
            let ws = self.world.kit_weapons(&kit);
            self.vehicles[i].give_kit(ws, None);
            if let Some(m) = self.vehicles[i].soldier_mut() {
                m.kit_parts = self.world.kit_parts(&kit);
            }
            self.hand_kit(i, &kit);
        }
        // The soldier's enter: a bot's soldier runs at 0.7–1 of the speeds, by `rand`
        // (in the FPU's long double, rounded once)
        let speed_mod = if ai { (((self.rng.next() & 31) as f64 * (0.3f32 / 31.0) as f64 + 0.7f32 as f64) as f32).min(1.0) } else { 1.0 };
        if let Some(m) = self.vehicles[i].soldier_mut() {
            m.has_player = true;
            m.human = !ai;
            m.speed_mod = speed_mod;
        }
        if sp.paratrooper {
            self.vehicles[i].craft.body.vel = [0.0, -3.0, 0.0];
            self.vehicles[i].parachute(true);
        }
        let p = &mut self.players[k];
        p.soldier = Some(i);
        p.alive = true;
        if by_sai {
            p.group = None;
        }
        // `setEnterOnSpawn` (or a bot at an AI one): into the vehicle there (`toggleEntryPoint`)
        if sp.enter_on_spawn || (ai && sp.ai_enter_on_spawn) {
            self.enter_nearest(i, ai);
        }
        self.journal.player_log.push((step, k, PlayerEvent::Spawned(i, g, n)));
        true
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_rules_from_the_settings_file() {
        let r = ScoreRules::parse("ScoreManager.kill 1\nScoreManager.death 0\nscoremanager.TK -2\nrem EndOfFile\nScoreManager.capture x\n");
        assert_eq!((r.kill, r.death, r.tk), (1, 0, -2));
        // (the rest keep the constructor's)
        assert_eq!((r.capture, r.attack, r.objective_tk), (20, 5, -15));
    }

    /// A kicked team killer's soldier goes, leaving its seat first:
    /// the fixed gun it fired from is free again.
    #[test]
    fn a_kicked_gunner_leaves_his_seat() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.rules.team_kills.kick = 1.0;
        let (a, b) = (s.add_player(1, "GermanSoldier", false), s.add_player(1, "GermanSoldier", false));
        for _ in 0..30 * 30 {
            s.step();
            if s.players[a].alive && s.players[b].alive {
                break;
            }
        }
        let gun = (0..s.vehicles.len()).find(|&i| s.vehicles[i].fixed).unwrap();
        let gunner = s.players[a].soldier.unwrap();
        s.enter(gunner, gun, 0);
        s.step();
        assert_eq!(s.vehicles[gun].entry.occupants[0], Some(gunner));
        let victim = s.players[b].soldier.unwrap();
        s.vehicles[victim].armor.as_mut().unwrap().killed_by = Some(gun);
        s.team_kill(b);
        assert!(s.players[a].kicked && s.vehicles[gunner].removed);
        assert_eq!(s.vehicles[gun].entry.occupants[0], None);
        assert_eq!(s.vehicles[gunner].entry.inside, None);
    }

    /// Killed in a seat, the gunner's kit goes with him (`killPlayer` destroys it: the player's object is the vehicle);
    /// on foot it would lie 30 s.
    #[test]
    fn a_gunner_killed_in_his_seat_takes_his_kit_with_him() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let a = s.add_player(1, "GermanSoldier", false);
        for _ in 0..30 * 30 {
            s.step();
            if s.players[a].alive {
                break;
            }
        }
        let gun = (0..s.vehicles.len()).find(|&i| s.vehicles[i].fixed).unwrap();
        let gunner = s.players[a].soldier.unwrap();
        let kit = s.vehicles[gunner].soldier().and_then(|m| m.kit).expect("a kit");
        s.enter(gunner, gun, 0);
        s.step();
        s.give_damage(gunner, 1000.0, crate::damage::Source::Outside);
        s.step();
        assert!(!s.players[a].alive && s.vehicles[gunner].removed);
        assert_eq!(s.vehicles[gun].entry.occupants[0], None);
        assert!(s.items[kit].removed);
    }

    #[test]
    fn the_generator_spreads() {
        let mut r = Rng(0x9E37_79B9_7F4A_7C15);
        let mut seen = [0; 4];
        for _ in 0..400 {
            seen[r.next() as usize % 4] += 1;
        }
        assert!(seen.iter().all(|&n| n > 60), "{seen:?}");
    }

    /// Wake: the Japanese have no flag at the start; they spawn on their carrier (the groups of
    /// `GlobalSpawnGroups.con`, its template's `SpawnPoint`s). Without those the round was over
    /// in 18 s (no group, nobody alive: the tickets at the end's rate).
    #[test]
    fn on_wake_the_japanese_spawn_on_their_carrier() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Wake", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let c = s.modes.conquest.as_ref().unwrap();
        assert!(c.groups.values().any(|g| g.team == 1 && g.spawns.iter().any(|p| p.on.is_some())), "no spawn point on a team 1 vehicle");
        let jp = s.add_player(1, "JapaneseSoldier", false);
        let us = s.add_player(2, "USSoldier", false);
        for _ in 0..60 * 30 {
            s.step();
        }
        let c = s.modes.conquest.as_ref().unwrap();
        assert!(c.tickets.iter().all(|&t| t > 150), "tickets {:?}", c.tickets);
        for p in [jp, us] {
            assert!(s.players[p].alive, "player {p} never spawned");
        }
        // the Japanese one on a ship's deck, off the island
        let i = s.players[jp].soldier.unwrap();
        let k = c.groups.values().flat_map(|g| &g.spawns).find_map(|p| p.on).unwrap().0;
        let (a, b) = (s.vehicles[i].craft.body.pos, s.vehicles[k].craft.body.pos);
        assert!(((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() < 200.0, "{} at {a:?}, the ship at {b:?}", s.vehicles[i].template);
    }

    /// `GlobalSpawnGroups.con` (groups 64 on, the ones vehicles carry) runs from the game's
    /// `Init.con` (`run globalSpawnGroups`): their team and `onlyForHuman`, in vanilla and in GC.
    /// Its `groupStatus` (GC's group 76) is no command of the original game (only `groupActive`).
    #[test]
    fn the_global_spawn_groups_are_read() {
        let game = bytefield_testdata::require!(Base, Gc);
        for (m, l, want) in [("bf1942", "Wake", [(70, 1, false), (75, 1, true), (72, 2, true)]), ("GCMOD", "GC_Bespin", [(67, 2, false), (70, 1, false), (76, 1, false)])] {
            let w = crate::World::load(&game, m, l, "conquest").unwrap();
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.start_conquest();
            let c = s.modes.conquest.as_ref().unwrap();
            for (g, team, human) in want {
                let g = &c.groups[&g];
                assert_eq!((g.team, g.only_human), (team, human), "{l}");
            }
        }
    }

    /// The screen when dead: a person spawns in the group and with the kit it chose; a group
    /// no longer its team's sends it to the first that is.
    /// The deserters, as the original does them: Berlin's
    /// (1560, 1800) is inside its combat area on material 7, (1805, 1924) on material 4, and
    /// (1400, 1800) outside the area: 10 s, then 5 hit points a second until dead; inside a
    /// T34 too, and the T34 takes nothing.
    #[test]
    fn deserters_lose_life_past_ten_seconds() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let hp_after = |at: [f32; 2], frames: usize, tank: bool| {
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            s.round.playing = true;
            s.start_conquest();
            let p = s.add_player(2, "RussianSoldier", false);
            for _ in 0..40 {
                s.step();
            }
            let sol = s.players[p].soldier.unwrap();
            let y = s.world.height_at(at[0] as f64, at[1] as f64).unwrap() as f32 + 1.0;
            s.vehicles[sol].craft.body.pos = [at[0], y, at[1]];
            s.players[p].outside = 0.0;
            let tank = tank.then(|| {
                let v = s.spawn_vehicle("T34", [at[0], y, at[1]], [0.0, 0.0, 0.0]).unwrap();
                s.enter(sol, v, 0);
                v
            });
            let on = tank.unwrap_or(sol);
            let mut hp = vec![];
            for _ in 0..frames {
                s.step();
                hp.push(s.vehicles[on].armor.as_ref().map_or(0.0, |a| a.hp));
            }
            (hp, s.vehicles[sol].armor.as_ref().map_or(0.0, |a| a.hp))
        };
        let (safe, _) = hp_after([1805.0, 1924.0], 400, false);
        assert_eq!(*safe.last().unwrap(), 30.0);
        for at in [[1560.0, 1800.0], [1400.0, 1800.0]] {
            let (hp, _) = hp_after(at, 400, false);
            assert_eq!(hp[295], 30.0, "{at:?}");
            assert!((hp[350] - (30.0 - 50.0 * 5.0 / 30.0)).abs() < 1.0, "{at:?}: {}", hp[350]);
        }
        let (hp, soldier) = hp_after([1560.0, 1800.0], 400, true);
        assert_eq!(hp[399], 100.0);
        assert!((soldier - (30.0 - 100.0 * 5.0 / 30.0)).abs() < 1.0, "{soldier}");
    }

    #[test]
    fn a_person_spawns_where_and_with_what_it_chose() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let p = s.add_player(1, "GermanSoldier", false);
        let groups = s.spawn_groups(p);
        assert!(groups.len() >= 2, "{groups:?}");
        let last = *groups.last().unwrap();
        s.choose_spawn(p, Some(last), 1);
        for _ in 0..10 {
            s.step();
        }
        assert!(s.players[p].alive);
        let sol = &s.vehicles[s.players[p].soldier.unwrap()];
        let at = sol.craft.body.pos;
        let c = s.modes.conquest.as_ref().unwrap();
        assert!(c.groups[&last].spawns.iter().any(|sp| (sp.pos[0] - at[0]).abs() < 1.0 && (sp.pos[2] - at[2]).abs() < 1.0), "at {at:?}");
        assert_eq!(sol.soldier().unwrap().weapon.as_ref().map(|w| w.template.name.as_str()), Some("Sg44"));
        assert_eq!(s.players[p].kit, 1);
        // its group goes to the other team: it spawns in the first of its own
        s.modes.conquest.as_mut().unwrap().groups.get_mut(&last).unwrap().team = 2;
        s.players[p].alive = false;
        s.players[p].spawn_time = 0.0;
        for _ in 0..10 {
            s.step();
        }
        assert!(s.players[p].alive, "it never spawned again");
    }

    /// A person holding (still on the screen when dead) does not spawn however long it waits;
    /// let go, it does, and the others never wait for it.
    #[test]
    fn a_person_holding_does_not_spawn() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let p = s.add_player(1, "GermanSoldier", false);
        let q = s.add_player(2, "BritishSoldier", false);
        s.hold_spawn(p, true);
        for _ in 0..300 {
            s.step();
        }
        assert!(!s.players[p].alive, "it spawned while holding");
        assert!(s.players[q].alive);
        s.hold_spawn(p, false);
        for _ in 0..10 {
            s.step();
        }
        assert!(s.players[p].alive, "it never spawned");
    }

    /// A soldier killed standing plays one of its death states and
    /// ends on the ground: its head low, not where it stood.
    #[test]
    fn a_killed_soldier_falls() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let p = s.add_player(1, "GermanSoldier", false);
        for _ in 0..60 {
            s.step();
        }
        let v = s.players[p].soldier.unwrap();
        let head = |s: &crate::Sim| {
            let sk = s.vehicles[v].soldier().unwrap().skeleton.as_ref().unwrap();
            sk.bone_pos(sk.file.index("Bip01 Head").unwrap())[1]
        };
        let standing = head(&s);
        // a hit it survives only moves the upper body
        s.give_damage(v, 1.0, crate::damage::Source::Hit);
        let m = s.vehicles[v].soldier().unwrap();
        assert_eq!(m.upper.state, m.machine.find("Ub_HitChestStand"));
        s.give_damage(v, 1000.0, crate::damage::Source::Hit);
        let m = s.vehicles[v].soldier().unwrap();
        let name = |i: Option<usize>| i.map(|i| m.machine.states[i].name.clone()).unwrap_or_default();
        assert!(name(m.lower.state).starts_with("Lb_Die"), "{}", name(m.lower.state));
        assert!(name(m.upper.state).starts_with("Ub_Die"), "{}", name(m.upper.state));
        for _ in 0..90 {
            s.step();
        }
        assert!(!s.vehicles[v].removed);
        let dead = head(&s);
        assert!(dead < 0.6 && dead < standing - 0.8, "the head at {standing} standing, {dead} dead");
    }

    /// `getTeamToAddPlayer` as the original filled El_Alamein: 32 bots in
    /// pairs, the first of each to either team at random; with `setTeamRatio 2 1`, four more after
    /// 16 and 16 all to team 1. With `serverAutoBalanceTeams`, a dead human of a team two longer
    /// goes over.
    #[test]
    fn players_join_the_shorter_team() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let mut first = [0; 2];
        for i in 0..32 {
            let p = s.join(0, true);
            let team = s.players[p].team;
            assert_eq!(s.players[p].template, crate::replica::soldier_template(team));
            if i % 2 == 0 {
                first[team as usize - 1] += 1;
            } else {
                assert_ne!(team, s.players[p - 1].team, "bot {i}");
            }
        }
        assert!(first.iter().all(|&n| n > 3), "the first of each pair {first:?}");
        s.rules.team_ratio = [2.0, 1.0];
        for _ in 0..4 {
            let p = s.join(0, true);
            assert_eq!(s.players[p].team, 1);
        }
        s.rules.team_ratio = [1.0, 1.0];
        assert_eq!(s.team_to_add(), 2);
        // a human of team 1 (22 against 16) dies: with auto-balance he goes to team 2
        s.round.playing = true;
        s.start_conquest();
        let me = s.join(1, false);
        for auto in [false, true] {
            s.rules.auto_balance = auto;
            for _ in 0..30 * 30 {
                if s.players[me].alive {
                    break;
                }
                s.step();
            }
            let v = s.players[me].soldier.expect("spawned");
            assert_eq!(s.players[me].team, 1);
            s.give_damage(v, 1000.0, crate::damage::Source::Hit);
            s.step();
            assert!(!s.players[me].alive);
            assert_eq!(s.players[me].team, if auto { 2 } else { 1 }, "auto-balance {auto}");
        }
        assert_eq!(s.players[me].template, "USSoldier");
    }

    /// The server's bot places: a person joins a full server, and the
    /// dead bot that goes is one of the team that would not get the next player; when the person leaves, a bot fills
    /// the place again.
    #[test]
    fn a_bot_makes_room_for_a_person() {
        let game = bytefield_testdata::require!(Base);
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        for _ in 0..8 {
            s.join(0, true);
        }
        s.rules.bot_places = 8;
        let me = s.join_person(1);
        let on = |s: &crate::Sim| s.players.iter().filter(|p| !p.kicked).count();
        assert_eq!(on(&s), 9);
        assert_eq!(s.team_to_add(), 2);
        for _ in 0..30 * 30 {
            if s.players.iter().all(|p| p.alive) {
                break;
            }
            s.step();
        }
        let kill = |s: &mut crate::Sim, team: u32| {
            let k = (0..s.players.len()).find(|&k| s.players[k].ai && s.players[k].alive && s.players[k].team == team).unwrap();
            s.give_damage(s.players[k].soldier.unwrap(), 1000.0, crate::damage::Source::Hit);
            s.step();
            s.step();
            k
        };
        // (one of team 2, the shorter: it stays)
        let stays = kill(&mut s, 2);
        assert!(!s.players[stays].kicked && on(&s) == 9);
        let goes = kill(&mut s, 1);
        assert!(s.players[goes].kicked, "a bot of the longer team goes");
        assert_eq!(on(&s), 8);
        s.remove_player(me);
        s.step();
        assert_eq!(on(&s), 8, "a bot takes the person's place");
        assert!(s.players.last().unwrap().ai);
    }

    /// `getRandomNameForTeam` as the original named Midway's coop bots: a
    /// first name with the second of the same index, from `Skirmish.con` (team 2 British, team 1
    /// Japanese); in conquest `Bots.con` (team 2 American). Past the 64 pairs, the last tried and
    /// a number.
    #[test]
    fn bots_take_paired_names_from_the_level() {
        let game = bytefield_testdata::require!(Base);
        let list = |w: &crate::World, team: &str, which: &str| -> Vec<String> {
            w.db.globals.iter().filter(|c| c.command.eq_ignore_ascii_case(which) && c.args[0].trim() == team).map(|c| c.args[1].trim().to_string()).collect()
        };
        for (mode, has, hasnt) in [("coop", "Matt Stanley", "Gus Smith"), ("conquest", "Gus Smith", "Matt Stanley")] {
            let w = crate::World::load(&game, "bf1942", "Midway", mode).unwrap();
            let (first, second) = (list(&w, "2", "game.addFirstNameOnTeam"), list(&w, "2", "game.addSecondNameOnTeam"));
            let pairs: Vec<String> = first.iter().zip(&second).map(|(a, b)| format!("{a} {b}")).collect();
            assert!(pairs.iter().any(|p| p == has) && !pairs.iter().any(|p| p == hasnt), "{mode}");
            let mut s = crate::Sim::new(&w, 1.0 / 30.0);
            for _ in 0..pairs.len() + 6 {
                s.join(2, true);
            }
            let names: Vec<&str> = s.players.iter().map(|p| p.name.as_str()).collect();
            let mut unique = names.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), names.len(), "{mode}: {names:?}");
            assert!(names[..pairs.len()].iter().all(|n| pairs.iter().any(|p| p == n)), "{mode}: the pairs first");
            assert!(names[pairs.len()..].iter().all(|n| n.ends_with('1') && pairs.iter().any(|p| p == &n[..n.len() - 1])), "{mode}: {:?}", &names[pairs.len()..]);
        }
        // a level's names are its own: Midway's Japanese for team 1
        let w = crate::World::load(&game, "bf1942", "Midway", "coop").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        let p = s.join(1, true);
        assert!(list(&w, "1", "game.addFirstNameOnTeam").iter().any(|f| s.players[p].name.starts_with(&format!("{f} "))));
        // a person has none of its own; with no lists, `Player`, `Player2`
        let me = s.join(1, false);
        assert!(s.players[me].name.is_empty());
        let a = s.random_name_for_team(0);
        s.players[0].name = a.clone();
        assert_eq!((a.as_str(), s.random_name_for_team(0).as_str()), ("Player", "Player2"));
    }
}
