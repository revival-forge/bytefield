//! The end of a round and the next one, as the original game does it:
//! `gameStatusPlaying` ends a conquest round when a team's tickets fall
//! below 1 (the other wins; both: a draw), `gameStatusFirstEndGame` clears the world, and
//! `gameStatusEndGame` waits its end delay and goes to the maplist's next
//! level: here the same one again, from the start.
//!
//! The time limit (`serverGameTime`; 0 by default: none): when the round's world time
//! passes it, in conquest, coop and objective mode the team with more of its tickets left (each
//! over its start, times the ticket ratio: the same for both) wins; in CTF and TDM the team with
//! more kills (in CTF too, not the captures); even, a draw. A level with an
//! attacker (`game.assaultTeam`) gives it to the defender in the first three, before any tickets.
//!
//! Not modeled: the victory's kind (`gameStatusPlaying` compares the tickets' difference with
//! `minorVictory` 0.4 and `majorVictory` 0.8, and gives 1, 2 or 3 to the score
//! manager: only the debriefing shows it).
//!
//! The pre-game: `resetTimers` puts the status at 3 and the countdown
//! at its delay (`serverGameStartDelay` sets both: 20 in the game's settings, so every round has
//! it; `serverGameRoundStartDelay` only the later rounds'), `gameStatusPreGame` counts it down,
//! shows it to every player («TIME UNTIL GAME STARTS») and at 0 sets status 1.
//! Spawning, tickets and the rest are `gameStatusPlaying`'s: in the original no
//! bot spawns until then. Only in a round played by a person (the client's `--choose`) and on our
//! server (`bytefield_server::hosted`, the snapshot carries it); the bench and the original's reference runs
//! start playing (theirs force `setGameStatus(1)`).
//! The least people: `updateGameLogic`, on a hosted game, counts
//! the people (not bots) and, once they reach `setMinNrOfPlayers` (2), sets the flag for good
//! and, if the pre-game is over, `restartMap`s. Until then the round plays as ever and each client
//! shows «ADDITIONAL PLAYERS NEEDED UNTIL GAME STARTS: N». The flag is true from the start in a level
//! with bots (`game.isAiLevel 1`, `AIdefault.con`, coop only): here [`Sim::min_players`] 0, the
//! default; our server sets 2 in conquest without bots.

use crate::Sim;

/// The server's settings a round plays by (`ServerSettings.con`, the `game.server*` commands, the mode's scores): set when
/// the round is made and fixed during it; the systems read them.
// TODO: they are filled in after `Sim::new` by whoever makes the round; they could come in through it.
#[derive(Debug, Clone)]
pub struct RoundRules {
    /// `game.serverScoreLimit`: the captures (CTF) that end a round (0: none)
    pub score_limit: u32,
    /// friendly fire and the nose camera (`ServerSettings.con`; the game's by default)
    pub server: crate::damage::ServerRules,
    /// `game.setServerWelcomeMessage <n> "<line>"`: the server's lines, which go to each
    /// client as it joins (`authentificationOk`'s `WelcomeMsgEvent`) and fill the briefing's COMMENTS
    pub welcome: Vec<String>,
    /// `game.setTeamRatio` (`GameServer` +700; 1 and 1): the share of the players each team
    /// gets as they join (`serverAxisTeamRatio` team 1's, `serverAlliedTeamRatio` team 2's)
    pub team_ratio: [f32; 2],
    /// `game.serverAutoBalanceTeams` (off): a dead human goes over to the shorter team
    pub auto_balance: bool,
    /// The places the server fills with bots (`autoSpawnBots`; 0: none, as a bench round's fixed
    /// bots): while fewer play, a bot joins; with more, a dead bot of the team that would not get the next one goes
    /// ([`Sim::update_players`]). Level 2: the state test (2 and 5 out) as outside the end of a round.
    pub bot_places: usize,
    /// `game.serverGameTime` in seconds (`setTimeLimit`): the round's length (0: none)
    pub time_limit: u32,
    /// the pre-game of each next round (`delayBeforeStartingGame`/`roundDelay`)
    pub pregame_delay: f32,
    /// the least people (`setMinNrOfPlayers`; 2 on a server of the game's) before the
    /// round counts: until then it plays, and when they are there the map starts again
    /// ([`Sim::count_players`]). 0: none, as in coop or with bots (`game.isAiLevel`, true from the start)
    pub min_players: u32,
    /// the server's team kill rules
    pub team_kills: crate::players::TeamKills,
    /// the points of each score event (`start_conquest` reads the mode's)
    pub scores: crate::players::ScoreRules,
}

impl Default for RoundRules {
    fn default() -> Self {
        RoundRules {
            score_limit: 0,
            server: crate::damage::ServerRules::default(),
            welcome: Vec::new(),
            team_ratio: [1.0, 1.0],
            auto_balance: false,
            bot_places: 0,
            time_limit: 0,
            pregame_delay: 0.0,
            min_players: 0,
            team_kills: Default::default(),
            scores: Default::default(),
        }
    }
}

/// Seconds the end of a round lasts before the next: `timeBeforeRestartMap` (copied to
/// a second field), 10 from the server's init; no settings file nor code sets it (only an admin's console).
pub const END_DELAY: f32 = 10.0;

impl Sim<'_> {
    /// The round starts with a pre-game of `delay` seconds, and so will every next one
    /// (`delayBeforeStartingGame`); 0: it plays at once.
    pub fn start_pregame(&mut self, delay: f32) {
        self.rules.pregame_delay = delay.max(0.0);
        self.round.pregame = self.rules.pregame_delay;
        self.round.playing = self.round.pregame <= 0.0;
    }

    /// `updateGameLogic`'s count of the people (here not a bot
    /// and not gone): once they are [`Sim::min_players`] the count stops for good and, out
    /// of the pre-game, the map starts again (`restartMap`: [`Sim::next_round`]).
    pub(crate) fn count_players(&mut self) {
        if self.rules.min_players == 0 {
            return;
        }
        let people = self.players.iter().filter(|p| !p.ai && !p.kicked).count() as u32;
        self.round.players_needed = self.rules.min_players.saturating_sub(people);
        if self.round.players_needed == 0 {
            self.rules.min_players = 0;
            if self.round.pregame <= 0.0 {
                self.restart_map();
            }
        }
    }

    /// `gameStatusPreGame`'s part of a frame: the time left down, and at 0 the game plays
    /// (`setGameStatus(1)`; its world time from 0). Whether this frame was still the pre-game's.
    pub(crate) fn pre_game(&mut self, dt: f32) -> bool {
        if self.round.pregame <= 0.0 {
            return false;
        }
        self.round.pregame = (self.round.pregame - dt).max(0.0);
        if self.round.pregame == 0.0 {
            self.round.playing = true;
            self.round.round_start = self.clock.time;
        }
        true
    }

    /// The end of a round's part of a frame: the winner, when the tickets say so, then the world
    /// cleared; when the delay is over, the next round.
    pub(crate) fn end_of_round(&mut self, dt: f32) {
        if let Some(t) = self.round.round_end.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                // (331: `gameStatusEndGame` ends the delay with `restartMap` while rounds are left, `resetStats`
                // to all, or else restarts the whole server on the next level of the list: the counts from 0 too)
                self.restart_map();
            }
            return;
        }
        let Some(c) = self.modes.conquest.as_ref() else { return };
        // (not in objective mode: there only the awards and the time limit end it)
        let out = [c.tickets[0] < 1, c.tickets[1] < 1].map(|o| o && c.normal.is_none());
        // (or one already named: CTF's score limit, `ctf_frame`)
        let winner = match (c.winner, out) {
            (Some(w), _) => w,
            // (no tickets in CTF nor TDM)
            (None, _) if !c.bleed => match self.time_up() {
                Some(w) => w,
                None => return,
            },
            (None, [true, true]) => 0,
            (None, [true, false]) => 2,
            (None, [false, true]) => 1,
            (None, [false, false]) => match self.time_up() {
                Some(w) => w,
                None => return,
            },
        };
        let Some(c) = self.modes.conquest.as_mut() else { return };
        c.winner = Some(winner);
        self.round.rounds += 1;
        self.round.round_end = Some(END_DELAY);
        self.clear_world();
    }

    /// The time limit's winner, once the round's time is past it (`gameStatusPlaying`).
    fn time_up(&self) -> Option<u8> {
        if self.rules.time_limit == 0 || self.clock.time - self.round.round_start <= self.rules.time_limit as f64 {
            return None;
        }
        let c = self.modes.conquest.as_ref()?;
        // a level with an attacker (`game.assaultTeam`): the defender wins, whatever the tickets
        // (in conquest, coop and objective mode; 1 gives 2, anything else 1)
        if c.bleed && c.rules.assault_team != 0 {
            return Some(if c.rules.assault_team == 1 { 2 } else { 1 });
        }
        let [a, b] = if c.bleed {
            // (no guard: a team the level gives none has an infinite ratio, or NaN with none left)
            let start = c.rules.tickets;
            [0, 1].map(|t| c.tickets[t] as f32 / start[t] as f32)
        } else {
            let mut kills = [0u32; 2];
            for p in &self.players {
                if (1..=2).contains(&p.team) {
                    kills[p.team as usize - 1] += p.kills;
                }
            }
            kills.map(|k| k as f32)
        };
        // (as the server compares them: NaN gives team 2)
        Some(if b < a { 1 } else if b <= a { 0 } else { 2 })
    }

    /// `clearWorld`: every vehicle and soldier gone, the shells too; the players dead, to spawn
    /// as soon as the next round starts.
    fn clear_world(&mut self) {
        for v in self.vehicles.iter_mut() {
            v.removed = true;
        }
        self.projectiles.clear();
        self.spent.clear();
        self.carried.clear();
        self.carried_from.clear();
        self.pools.clear();
        for p in self.players.iter_mut() {
            p.alive = false;
            p.soldier = None;
            p.spawn_time = 0.0;
            p.group = None;
        }
    }

    /// `restartMap`: the world cleared (`clearWorld`), each player's score back to 0
    /// (The player's reset stats, 331: nine counts, three maps and more; ours, these five) and the round again, with its pre-game.
    pub fn restart_map(&mut self) {
        self.clear_world();
        for p in self.players.iter_mut() {
            (p.score, p.kills, p.deaths, p.captures, p.tks) = (0, 0, 0, 0, 0);
        }
        self.next_round();
    }

    /// The next round: the level's vehicles and spawners, the control points and tickets as at the
    /// start (`start_conquest`), the spawn waves from the start and the bots' minds afresh.
    pub fn next_round(&mut self) {
        self.round.round_end = None;
        self.round.round_start = self.clock.time;
        self.spawners.clear();
        self.items.clear();
        self.start_conquest();
        // (`resetTimers` starts the cycle again; the long part, stays the level's)
        let add = self.round.waves.as_ref().map_or(crate::players::Waves::default().add, |w| w.add);
        self.round.waves = Some(crate::players::Waves { add, ..Default::default() });
        self.restart_better_bots();
        self.drivers.minds.live.clear();
        // (`resetTimers`: the status back to 3, with the delay's time)
        if self.rules.pregame_delay > 0.0 {
            self.start_pregame(self.rules.pregame_delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A round on El Alamein with its tickets taken away ends with the other team the winner,
    /// clears the world, and after the end delay starts again with the tickets full.
    #[test]
    fn a_round_ends_and_the_next_starts() {
        let game = bytefield_testdata::require!(Base);
        let world = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut sim = Sim::new(&world, 1.0 / 30.0);
        sim.round.playing = true;
        sim.start_conquest();
        let p = sim.add_player(1, crate::replica::soldier_template(1), false);
        let full = sim.modes.conquest.as_ref().unwrap().tickets;
        for _ in 0..90 {
            sim.step();
        }
        assert!(sim.players[p].alive, "the player spawned");
        (sim.players[p].score, sim.players[p].kills) = (5, 3);
        sim.modes.conquest.as_mut().unwrap().tickets[1] = 0;
        sim.step();
        assert_eq!(sim.modes.conquest.as_ref().unwrap().winner, Some(1));
        assert!(sim.round.round_end.is_some() && !sim.players[p].alive);
        assert!(sim.vehicles.iter().all(|v| v.removed));
        let n = (END_DELAY / (1.0 / 30.0)) as usize + 2;
        for _ in 0..n {
            sim.step();
        }
        assert!(sim.round.round_end.is_none(), "the next round started");
        let c = sim.modes.conquest.as_ref().unwrap();
        assert_eq!((c.winner, c.tickets), (None, full));
        // (331: `restartMap` and its `resetStats`)
        assert_eq!((sim.players[p].score, sim.players[p].kills), (0, 0), "the counts start again");
        for _ in 0..90 {
            sim.step();
        }
        assert!(sim.players[p].alive, "the player spawns in the next round");
        assert_eq!(sim.round.rounds, 1);
    }

    /// With a pre-game (the game's `serverGameStartDelay`, here 2 s), nobody spawns until it is over
    /// and the world time stays 0; then the round plays, and the next round has its pre-game too.
    #[test]
    fn the_pregame_holds_the_spawns() {
        let game = bytefield_testdata::require!(Base);
        let world = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut sim = Sim::new(&world, 1.0 / 30.0);
        sim.start_conquest();
        sim.start_pregame(2.0);
        let p = sim.add_player(1, crate::replica::soldier_template(1), false);
        for _ in 0..55 {
            sim.step();
        }
        assert!(!sim.round.playing && !sim.players[p].alive, "nobody spawns in the pre-game");
        for _ in 0..6 {
            sim.step();
        }
        assert!(sim.round.playing && sim.round.pregame == 0.0, "the game plays at 2 s");
        for _ in 0..90 {
            sim.step();
        }
        assert!(sim.players[p].alive, "the player spawned");
        sim.modes.conquest.as_mut().unwrap().tickets[1] = 0;
        let n = (END_DELAY / (1.0 / 30.0)) as usize + 3;
        for _ in 0..n {
            sim.step();
        }
        assert!(sim.round.round_end.is_none() && !sim.round.playing && sim.round.pregame > 1.5, "the next round's pre-game");
        for _ in 0..60 {
            sim.step();
        }
        assert!(!sim.players[p].alive);
    }

    /// With the least people at 2 (a server without bots), one person plays as ever but the round
    /// waits for one more; when it comes, the map starts again: scores to 0, a new pre-game, and
    /// no more waiting. In the pre-game nothing takes damage.
    #[test]
    fn the_round_waits_for_the_least_people() {
        let game = bytefield_testdata::require!(Base);
        let world = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut sim = Sim::new(&world, 1.0 / 30.0);
        sim.start_conquest();
        sim.start_pregame(1.0);
        sim.rules.min_players = 2;
        let p = sim.join_person(1);
        for _ in 0..100 {
            sim.step();
        }
        assert!(sim.round.playing && sim.players[p].alive, "one person plays");
        assert_eq!(sim.round.players_needed, 1);
        sim.players[p].score = 5;
        // a bot does not count
        sim.join(2, true);
        sim.step();
        assert_eq!(sim.round.players_needed, 1);
        sim.join_person(2);
        sim.step();
        assert_eq!((sim.round.players_needed, sim.rules.min_players, sim.players[p].score), (0, 0, 0));
        assert!(!sim.round.playing && sim.round.pregame > 0.9 && !sim.players[p].alive, "the map starts again, with its pre-game");
        let v = sim.vehicles.iter().position(|v| !v.removed && v.armor.as_ref().is_some_and(|a| a.hp > 0.0)).unwrap();
        let hp = sim.vehicles[v].armor.as_ref().unwrap().hp;
        sim.give_damage(v, 10.0, crate::damage::Source::Hit);
        assert_eq!(sim.vehicles[v].armor.as_ref().unwrap().hp, hp, "no damage in the pre-game");
    }

    /// With a time limit, the round ends when it passes: the team with more of its tickets left
    /// wins (El Alamein conquest), and the next round's clock starts again.
    #[test]
    fn the_time_limit_ends_a_round() {
        let game = bytefield_testdata::require!(Base);
        let world = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut sim = Sim::new(&world, 1.0 / 30.0);
        sim.round.playing = true;
        sim.start_conquest();
        sim.rules.time_limit = 2;
        sim.modes.conquest.as_mut().unwrap().tickets[0] -= 10;
        for _ in 0..55 {
            sim.step();
        }
        assert!(sim.round.round_end.is_none(), "not yet");
        for _ in 0..10 {
            sim.step();
        }
        assert_eq!(sim.modes.conquest.as_ref().unwrap().winner, Some(2));
        let n = (END_DELAY / (1.0 / 30.0)) as usize + 2;
        for _ in 0..n {
            sim.step();
        }
        assert!(sim.round.round_end.is_none() && sim.modes.conquest.as_ref().unwrap().winner.is_none(), "the next round's clock is its own");
    }

    /// Berlin's `game.assaultTeam 2` (the Allies attack): at the time limit the Axis wins, though
    /// it has fewer tickets left (`gameStatusPlaying`).
    #[test]
    fn the_defender_wins_at_the_time_limit() {
        let game = bytefield_testdata::require!(Base);
        let world = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let mut sim = Sim::new(&world, 1.0 / 30.0);
        sim.round.playing = true;
        sim.start_conquest();
        assert_eq!(sim.modes.conquest.as_ref().unwrap().rules.assault_team, 2);
        sim.rules.time_limit = 1;
        sim.modes.conquest.as_mut().unwrap().tickets[0] -= 10;
        for _ in 0..40 {
            sim.step();
        }
        assert_eq!(sim.modes.conquest.as_ref().unwrap().winner, Some(1));
    }

    /// Cassino's coop sets `game.timeToNextWave 0` after the server's 20 (original, noche 234): its
    /// spawn waves have no long part, in the next round too; Berlin keeps the server's.
    #[test]
    fn the_level_sets_the_time_to_the_next_wave() {
        let Some(game) = bytefield_testdata::with_mod("XPack1") else { return; };
        let world = crate::World::load(&game, "XPack1", "cassino", "coop").unwrap();
        let mut sim = Sim::new(&world, 1.0 / 30.0);
        sim.start_conquest();
        assert_eq!(sim.round.waves.as_ref().unwrap().add, 0.0);
        sim.next_round();
        assert_eq!(sim.round.waves.as_ref().unwrap().add, 0.0);
        let world = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let mut sim = Sim::new(&world, 1.0 / 30.0);
        sim.start_conquest();
        assert_eq!(sim.round.waves.as_ref().unwrap().add, 20.0);
    }
}

/// Where the round is (pre-game, playing, ended) and its counters. This module and the players' update move it; the systems read it.
pub struct RoundState {
    /// Bots deactivated to go: removed once dead, by the next update.
    pub leaving: Vec<usize>,
    /// when the round started (`resetTimers` sets the server's world time back to 0)
    pub round_start: f64,
    /// spawn waves (None: `serverSpawnDelay` alone)
    pub waves: Option<crate::players::Waves>,
    /// `gameStatusPlaying`'s `spawnDelay` (a static): frames until the next spawn pass
    pub spawn_delay: u32,
    /// the game is playing (`setGameStatus(1)`): the world time runs (in pre-game, as on an
    /// empty dedicated server, it stays 0)
    pub playing: bool,
    /// the pre-game's seconds left (`gameStatusPreGame`, status 3; [`Sim::start_pregame`]):
    /// nobody spawns and the tickets wait, until at 0 the game plays. 0: none, as the bench's
    pub pregame: f32,
    /// how many people are still missing (0: enough; the client's «ADDITIONAL PLAYERS NEEDED»)
    pub players_needed: u32,
    /// the round is over: seconds left of its end (`gameStatusEndGame`) before the next
    pub round_end: Option<f32>,
    /// rounds played to their end
    pub rounds: u32,
}

impl Default for RoundState {
    fn default() -> Self {
        RoundState {
            leaving: Vec::new(),
            round_start: 0.0,
            waves: Some(crate::players::Waves::default()),
            spawn_delay: 0,
            playing: false,
            pregame: 0.0,
            players_needed: 0,
            round_end: None,
            rounds: 0,
        }
    }
}
