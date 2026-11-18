//! Objective mode, as the original game plays it (`ObjectiveManager`, `Objective`,
//! `DestroyTargetObjective`, `TimerObjective`, `ANDCompositeObjective`, `TeamWinsAward`,
//! `PlayerAward`):
//!
//! - Each objective is the object of an `ObjectSpawner` named with `Object.setName` and listed
//!   with `objectiveManager.registerObjectSpawner`; `setRootObjectSpawner <team> <name>` names
//!   each team's last one and `setDefender` the team that defends.
//! - Every frame of the round, an objective not yet met is evaluated:
//!   a `DestroyTargetObjective` is met when the last object of the spawner named by its
//!   `setTargetName` is destroyed; a `TimerObjective` when its clock passes `TimeLimit` ×
//!   `game.objectiveAttackerTicketsMod` %; an `ANDCompositeObjective` when the objectives of all
//!   its `addObjectiveSpawnerToComposite` are done.
//! - Met: the immediate award at once (a target's destroyer gets `Objective`'s points if he is
//!   of the objective's team, `ObjectiveTK`'s if not), and the objective's clock starts; once it
//!   passes `setObjectiveDelay` the objective is done and gives its award: the timer and the
//!   composite make their team win the round (`TeamWinsAward`), a target nothing more.
//! - A death costs a ticket to the attackers only (`killPlayer`, mode 5): the defenders' tickets
//!   never run down.
//! - The tickets each team sees are its own scaled by how far the other team's root objective is
//!   (`gameStatusPlaying`, mode 5): every frame its own count comes back, bleeds, is kept, and
//!   the team gets trunc(own × (1 − completion)); deaths come off the own count (`killPlayer`).
//!   A timer's completion is its clock over its time, a target's 1 − hit points / max (1 when
//!   gone or done), a composite's the mean of its parts'. The round never ends on tickets here.
//!
//! Not modeled: the messages and sounds (`giveVocal`), the AI's `objectiveDeactivated`, the
//! clients' copy of the objectives.

use serde::Serialize;

use crate::players::Score;

/// What an objective asks for.
#[derive(Debug, Clone, Serialize)]
pub enum Goal {
    /// the last object of the spawner of this `Object.setName` destroyed
    Destroy { target: String },
    /// `TimeLimit` seconds (× the attackers' mod) and the clock so far
    Timer { limit: f32, clock: f32 },
    /// every objective of these spawners done
    All(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Objective {
    /// its spawner's `Object.setName` and its template
    pub name: String,
    pub template: String,
    /// the team it is for
    pub team: u32,
    /// `setObjectiveDelay`, and its clock once met
    pub delay: f32,
    pub timer: f32,
    /// whether it is done
    pub done: bool,
    pub goal: Goal,
}

/// What happened to an objective (the log: its index).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum ObjectiveEvent {
    /// met (`triggerImmediateAward`); a target's destroyer, when known
    Met(Option<usize>),
    /// done after its delay (`triggerAward`)
    Done,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Objectives {
    pub list: Vec<Objective>,
    /// `objectiveManager.setDefender`
    pub defender: u32,
    /// `setRootObjectSpawner`: each team's last objective
    pub roots: Vec<(u32, String)>,
    pub log: Vec<(u64, usize, ObjectiveEvent)>,
}

/// `game.objectiveAttackerTicketsMod`, a percentage. HYPOTHESIS: 100 by default
/// (no script sets it; 0 would end every timer at once).
pub const ATTACKER_TICKETS_MOD: i32 = 100;

impl Objectives {
    pub fn from_db(db: &bytefield_con::templates::TemplateDb) -> Objectives {
        let mut o = Objectives::default();
        let mut registered = Vec::new();
        for g in &db.globals {
            let a = |k: usize| g.args.get(k).cloned().unwrap_or_default();
            match g.command.to_ascii_lowercase().as_str() {
                "objectivemanager.setdefender" => o.defender = a(0).parse().unwrap_or(0),
                "objectivemanager.registerobjectspawner" => registered.push(a(0)),
                "objectivemanager.setrootobjectspawner" => o.roots.push((a(0).parse().unwrap_or(0), a(1))),
                _ => {}
            }
        }
        let last = |i: &bytefield_con::templates::Instance, n: &str| i.props.iter().rev().find(|p| p.name == n).and_then(|p| p.args.first()).cloned();
        for name in registered {
            let Some(i) = db.instances.iter().find(|i| last(i, "setname").is_some_and(|n| n.eq_ignore_ascii_case(&name))) else { continue };
            let Some(s) = db.object_template(&i.template).filter(|t| t.kind.eq_ignore_ascii_case("objectspawner")) else { continue };
            let Some(t) = s.all("setobjecttemplate").iter().filter_map(|a| a.get(1)).find_map(|n| db.object_template(n)) else { continue };
            let num = |n: &str, d: f32| t.get(n).and_then(|a| a.first()).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
            let goal = match t.kind.to_ascii_lowercase().as_str() {
                "destroytargetobjective" => Goal::Destroy { target: t.get("settargetname").and_then(|a| a.first()).cloned().unwrap_or_default() },
                "timerobjective" => Goal::Timer { limit: num("timelimit", 0.0), clock: 0.0 },
                "andcompositeobjective" => Goal::All(t.all("addobjectivespawnertocomposite").iter().filter_map(|a| a.first().cloned()).collect()),
                _ => continue,
            };
            let team = t.get("setteam").or_else(|| t.get("team")).and_then(|a| a.first()).and_then(|v| v.parse().ok()).unwrap_or(0);
            o.list.push(Objective { name, template: t.name.clone(), team, delay: num("setobjectivedelay", 0.0), timer: 0.0, done: false, goal });
        }
        o
    }
}

impl crate::Sim<'_> {
    /// The objective's update handling of every objective (server, playing).
    pub fn objective_frame(&mut self, dt: f32) {
        let Some(n) = self.modes.objectives.as_ref().map(|o| o.list.len()) else { return };
        if self.modes.conquest.as_ref().is_some_and(|c| c.winner.is_some()) {
            return;
        }
        for i in 0..n {
            let o = &self.modes.objectives.as_ref().unwrap().list[i];
            // `hasDelayPassed`
            if o.delay < o.timer {
                let o = &mut self.modes.objectives.as_mut().unwrap().list[i];
                o.done = true;
                o.timer = 0.0;
                self.objective_award(i);
                continue;
            }
            if o.done {
                continue;
            }
            if o.timer > 0.0 {
                self.modes.objectives.as_mut().unwrap().list[i].timer += dt;
                continue;
            }
            if self.objective_met(i, dt) {
                self.objective_immediate_award(i);
                self.modes.objectives.as_mut().unwrap().list[i].timer = dt;
            }
        }
    }

    /// The vehicle the spawner named `name` made last (`getTargetObject`), while it exists.
    fn spawner_object(&self, name: &str) -> Option<usize> {
        let s = self.spawners.iter().find(|s| s.label.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(name)))?;
        match s.last? {
            crate::create::Made::Vehicle(v) => self.vehicles.get(v).is_some_and(|v| !v.removed).then_some(v),
            crate::create::Made::Item(_) => None,
        }
    }

    /// `getCompletion` of objective `i`, from 0 to 1 (a target's can pass 1, as the server's).
    fn completion(&self, i: usize) -> f32 {
        let os = self.modes.objectives.as_ref().unwrap();
        let o = &os.list[i];
        match &o.goal {
            // (`TimerObjective`: its clock over `TimeLimit` × the mod, at most 1)
            Goal::Timer { limit, clock } => (clock / (ATTACKER_TICKETS_MOD as f32 * limit * 0.01)).min(1.0),
            // (`DestroyTargetObjective`: the target's armor, 1 when done or gone)
            Goal::Destroy { target } => match self.spawner_object(target).and_then(|v| self.vehicles[v].armor.as_ref()) {
                Some(a) if !o.done => 1.0 - a.hp / a.max_hp,
                _ => 1.0,
            },
            // (`CompositeObjective`: the mean of those its spawners have)
            Goal::All(names) => {
                let parts: Vec<f32> = names.iter().filter_map(|n| os.list.iter().position(|o| o.name.eq_ignore_ascii_case(n))).map(|k| self.completion(k)).collect();
                if parts.is_empty() { 0.0 } else { parts.iter().sum::<f32>() / parts.len() as f32 }
            }
        }
    }

    /// Objective mode: each team's own tickets back (`setTickets`: not below 0) at the start of
    /// `gameStatusPlaying`, before the bleed.
    pub fn objective_restore(&mut self) {
        if let Some(c) = self.modes.conquest.as_mut()
            && let Some(n) = c.normal
        {
            c.tickets = n.map(|t| t.max(0));
        }
    }

    /// The scaled tickets of `gameStatusPlaying` (mode 5): each team's own kept, and in
    /// `tickets` trunc(own × (1 − completion of the other team's root)), 0 with no root.
    pub fn objective_tickets(&mut self) {
        let Some(os) = self.modes.objectives.as_ref() else { return };
        let root = |t: u32| {
            let (_, name) = os.roots.iter().rev().find(|(r, _)| *r == t)?;
            os.list.iter().position(|o| o.name.eq_ignore_ascii_case(name))
        };
        let c = [2, 1].map(|t| root(t).map_or(0.0, |i| self.completion(i)));
        let Some(cq) = self.modes.conquest.as_mut() else { return };
        let own = cq.tickets;
        cq.normal = Some(own);
        for t in 0..2 {
            // (in the FPU's long double, truncated, as the bleed's)
            cq.tickets[t] = ((own[t] as f64 * (1.0 - c[t] as f64)) as i32).max(0);
        }
    }

    /// `evaluate`.
    fn objective_met(&mut self, i: usize, dt: f32) -> bool {
        let os = self.modes.objectives.as_mut().unwrap();
        match &mut os.list[i].goal {
            Goal::Timer { limit, clock } => {
                *clock += dt;
                ATTACKER_TICKETS_MOD as f32 * *limit * 0.01 < *clock
            }
            Goal::All(names) => {
                let names = names.clone();
                names.iter().all(|n| os.list.iter().any(|o| o.name.eq_ignore_ascii_case(n) && o.done))
            }
            Goal::Destroy { target } => {
                let target = target.clone();
                self.spawner_object(&target).and_then(|v| self.vehicles[v].armor.as_ref()).is_some_and(|a| a.destroyed)
            }
        }
    }

    /// `triggerImmediateAward`: a target's destroyer gets his points (the player award's give or
    /// `givePenalty`); the others only speak (`giveVocal`).
    fn objective_immediate_award(&mut self, i: usize) {
        let step = self.clock.steps;
        let o = &self.modes.objectives.as_ref().unwrap().list[i];
        let mut by = None;
        if let Goal::Destroy { target } = &o.goal {
            let team = o.team;
            let killer = self.spawner_object(&target.clone()).and_then(|v| self.vehicles[v].armor.as_ref()).and_then(|a| a.killed_by);
            by = killer.and_then(|by| (0..self.players.len()).find(|&j| self.players[j].soldier.is_some_and(|s| s == by || self.vehicles.get(s).is_some_and(|v| v.entry.inside == Some(by)))));
            if let Some(j) = by {
                self.score(j, if self.players[j].team == team { Score::Objective } else { Score::ObjectiveTk });
            }
        }
        self.modes.objectives.as_mut().unwrap().log.push((step, i, ObjectiveEvent::Met(by)));
    }

    /// `triggerAward`: a timer's or a composite's team wins (the team award's give: the winner,
    /// then the round's end).
    fn objective_award(&mut self, i: usize) {
        let step = self.clock.steps;
        let os = self.modes.objectives.as_mut().unwrap();
        os.log.push((step, i, ObjectiveEvent::Done));
        let o = &os.list[i];
        if matches!(o.goal, Goal::Destroy { .. }) {
            return;
        }
        let team = o.team as u8;
        if let Some(c) = self.modes.conquest.as_mut().filter(|c| c.winner.is_none()) {
            c.winner = Some(team);
        }
    }
}

#[cfg(test)]
mod tests {
    fn game() -> Option<std::path::PathBuf> {
        let p = bytefield_testdata::game();
        p.join("Mods/bf1942").exists().then_some(p)
    }

    #[test]
    fn battle_of_britain_s_objectives() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "Battle_of_Britain", "objectivemode").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let o = s.modes.objectives.as_ref().unwrap();
        assert_eq!(o.defender, 2);
        assert_eq!(o.list.len(), 7);
        assert_eq!(o.roots, vec![(2, "ALLIED".into()), (1, "AXIS".into())]);
        // the defenders' tickets are 100 whatever the level says (it gives them none)
        assert_eq!(s.modes.conquest.as_ref().unwrap().tickets[1], 100);
        // the five targets stand, made by their spawners
        for k in 0..5 {
            let super::Goal::Destroy { target } = &o.list[k].goal else { panic!() };
            assert!(s.spawner_object(&target.clone()).is_some(), "{target}");
        }
        // destroy them all: the composite is met, and 3 s later the axis wins
        for k in 0..5 {
            let super::Goal::Destroy { target } = s.modes.objectives.as_ref().unwrap().list[k].goal.clone() else { panic!() };
            let v = s.spawner_object(&target).unwrap();
            s.give_damage(v, 1.0e6, crate::damage::Source::Kill);
        }
        let mut t = 0.0;
        while s.modes.conquest.as_ref().unwrap().winner.is_none() && t < 10.0 {
            s.objective_frame(1.0 / 30.0);
            t += 1.0 / 30.0;
        }
        assert_eq!(s.modes.conquest.as_ref().unwrap().winner, Some(1));
        // 1 s for the targets, 3 for the composite (and a frame each to be met)
        assert!((4.0..4.5).contains(&t), "{t}");
    }

    /// The attackers' tickets on the empty server: the
    /// Allies' timer scales them, trunc(200 × (1 − clock / 900)), the exact ones a ticket lower by
    /// the f32 clock; a half-dead target takes a tenth of the defenders' 100 (and a ticket more, by f32); a death comes off
    /// the attackers' own count, and none left does not end the round.
    #[test]
    fn the_tickets_are_scaled_by_the_other_root() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "Battle_of_Britain", "objectivemode").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let original = [(60, 199), (540, 195), (1080, 192), (1620, 188), (2160, 184), (2220, 183), (2340, 182)];
        for f in 1..=2340 {
            s.objective_restore();
            s.objective_frame(1.0 / 30.0);
            s.objective_tickets();
            let c = s.modes.conquest.as_ref().unwrap();
            assert_eq!(c.normal, Some([200, 100]));
            if let Some(&(_, n)) = original.iter().find(|(k, _)| *k == f) {
                assert_eq!(c.tickets, [n, 100], "frame {f}");
            }
        }
        let super::Goal::Destroy { target } = s.modes.objectives.as_ref().unwrap().list[0].goal.clone() else { panic!() };
        let v = s.spawner_object(&target).unwrap();
        let a = s.vehicles[v].armor.as_mut().unwrap();
        a.hp = a.max_hp * 0.5;
        s.objective_tickets();
        // (0.1 in f32 is a hair over: 89.99…, truncated)
        assert_eq!(s.modes.conquest.as_ref().unwrap().tickets[1], 89);
        s.modes.conquest.as_mut().unwrap().normal = Some([0, 100]);
        s.objective_restore();
        s.objective_tickets();
        s.end_of_round(1.0 / 30.0);
        assert_eq!(s.modes.conquest.as_ref().unwrap().tickets[0], 0);
        assert!(s.round.round_end.is_none());
    }

    #[test]
    fn the_defenders_win_on_time() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "Battle_of_Britain", "objectivemode").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let mut t = 0.0;
        while s.modes.conquest.as_ref().unwrap().winner.is_none() && t < 1000.0 {
            s.objective_frame(0.5);
            t += 0.5;
        }
        assert_eq!(s.modes.conquest.as_ref().unwrap().winner, Some(2));
        // 900 s, then the 3 s delay
        assert!((903.0..905.0).contains(&t), "{t}");
    }
}
