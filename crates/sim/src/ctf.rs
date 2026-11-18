//! Capture the flag, as the original game plays it (`Flag`, `FlagBase`,
//! and the server's pickup and score handling):
//!
//! - Each `FlagBase` of the level (`object.create UKbase` in `<level>/Ctf.con`: team,
//!   `flagTemplate`, `radius`, `setFlagLocation`) makes its flag at its first update, at the base
//!   plus the flag's place, and keeps it "at home" until someone takes it.
//! - With its flag at home, a base looks at the players of its team that carry a flag: one on foot
//!   within its radius drops it there, which is the capture (`FlagCapture` to him, the flag back to
//!   its own base). Then the nearest enemy on foot within the radius takes its flag (`Attack`).
//! - A flag away from home: carried, its clock is set to its `TimeToReSpawn`; on the ground the
//!   clock runs and at 0 the flag goes home. Before that, the nearest living soldier of its team
//!   on foot within its radius sends it home (`Defence` to him); if none, the nearest enemy takes
//!   it (`Attack`).
//! - A carrier that dies drops it where he was, on the terrain + 1.5 m.
//! - A team wins when its captures reach the score limit (`game.serverScoreLimit`; 0: never).
//!
//! Not modeled: the flag's orientation to the terrain, the events to the clients.

use serde::Serialize;

use crate::players::Score;

use crate::vecmath::V3;

/// A flag's base and its flag (one each).
#[derive(Debug, Clone, Serialize)]
pub struct Base {
    pub template: String,
    pub team: u32,
    pub pos: V3,
    pub radius: f32,
    /// the flag's template and where it stands on the base
    pub flag_template: String,
    pub flag_at: V3,
    pub flag: Flag,
}

#[derive(Debug, Clone, Serialize)]
pub struct Flag {
    pub team: u32,
    pub radius: f32,
    /// `TimeToReSpawn` (−1 by default: home at once once dropped)
    pub respawn: f32,
    /// its clock on the ground
    pub timer: f32,
    pub pos: V3,
    /// at its base
    pub home: bool,
    /// the player carrying it
    pub carrier: Option<usize>,
}

/// What happened to a flag (the log: the base's index).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum FlagEvent {
    /// taken by this player (from home or from the ground)
    Taken(usize),
    /// dropped where its carrier died
    Dropped,
    /// sent home by this player of its team
    Returned(usize),
    /// home on its own (its clock ran out)
    Home,
    /// this player captured it at his base
    Captured(usize),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Ctf {
    pub bases: Vec<Base>,
    /// each team's captures (`TeamScore` +8)
    pub captures: [u32; 2],
    pub log: Vec<(u64, usize, FlagEvent)>,
}

fn dist(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn first(t: &bytefield_con::templates::Template, n: &str) -> Option<String> {
    t.get(n).and_then(|a| a.first()).map(|s| s.trim().trim_matches('"').to_string())
}

impl Ctf {
    /// The level's flag bases (the `FlagBase` instances) and their flags (`flagTemplate`).
    pub fn from_db(db: &bytefield_con::templates::TemplateDb) -> Ctf {
        let mut bases = Vec::new();
        for i in &db.instances {
            let Some(t) = db.object_template(&i.template).filter(|t| t.kind.eq_ignore_ascii_case("flagbase")) else { continue };
            let Some(pos) = i.props.iter().rev().find(|p| p.name == "absoluteposition").and_then(|p| p.args.first()).and_then(|v| bytefield_world::parse_vec(v)).filter(|v| v.len() == 3) else { continue };
            let num = |t: &bytefield_con::templates::Template, n: &str, d: f32| first(t, n).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
            let flag_template = first(t, "flagtemplate").unwrap_or_default();
            let Some(f) = db.object_template(&flag_template).filter(|f| f.kind.eq_ignore_ascii_case("flag")) else { continue };
            // `setFlagLocation <pos> <rotation>`
            let flag_at = t.get("setflaglocation").and_then(|a| a.first()).and_then(|v| bytefield_world::parse_vec(v)).filter(|v| v.len() == 3).map_or([0.0; 3], |v| [v[0] as f32, v[1] as f32, v[2] as f32]);
            let pos = [pos[0] as f32, pos[1] as f32, pos[2] as f32];
            let team = num(t, "team", 0.0) as u32;
            let respawn = num(f, "timetorespawn", -1.0);
            let home_at = [pos[0] + flag_at[0], pos[1] + flag_at[1], pos[2] + flag_at[2]];
            let flag = Flag { team: num(f, "team", team as f32) as u32, radius: num(f, "radius", 0.0), respawn, timer: respawn, pos: home_at, home: true, carrier: None };
            bases.push(Base { template: t.name.clone(), team, pos, radius: num(t, "radius", 0.0), flag_template, flag_at, flag });
        }
        Ctf { bases, ..Ctf::default() }
    }

    /// The flag of base `b` home.
    fn home(&mut self, b: usize) {
        let base = &mut self.bases[b];
        base.flag.pos = [base.pos[0] + base.flag_at[0], base.pos[1] + base.flag_at[1], base.pos[2] + base.flag_at[2]];
        base.flag.home = true;
        base.flag.carrier = None;
        base.flag.timer = base.flag.respawn;
    }

    /// Whether player `k` carries a flag.
    pub fn carries(&self, k: usize) -> bool {
        self.bases.iter().any(|b| b.flag.carrier == Some(k))
    }
}

impl crate::Sim<'_> {
    /// The flags' frame (each base's `handleUpdate`, then each flag's), with the round's end at
    /// the score limit (`handleScore`).
    pub(crate) fn ctf_frame(&mut self, dt: f32) {
        let Some(mut c) = self.modes.ctf.take() else { return };
        let step = self.clock.steps;
        // each player: team, alive soldier's place, on foot
        let who: Vec<Option<(u32, V3, bool)>> = self
            .players
            .iter()
            .map(|p| {
                let s = p.soldier.filter(|_| p.alive).and_then(|s| self.vehicles.get(s))?;
                Some((p.team, s.craft.body.pos, s.entry.inside.is_none()))
            })
            .collect();
        let nearest = |at: V3, radius: f32, pick: &dyn Fn(u32) -> bool| -> Option<usize> {
            let mut best = (radius, None);
            for (k, w) in who.iter().enumerate() {
                if let Some((team, pos, on_foot)) = *w
                    && on_foot
                    && pick(team)
                {
                    let d = dist(pos, at);
                    if d < best.0 {
                        best = (d, Some(k));
                    }
                }
            }
            best.1
        };
        let mut scores = Vec::new();
        for b in 0..c.bases.len() {
            if !c.bases[b].flag.home {
                continue;
            }
            let (team, pos, radius) = (c.bases[b].team, c.bases[b].pos, c.bases[b].radius);
            // the captures: its team's carriers on foot within the radius
            for (k, w) in who.iter().enumerate() {
                let Some((t, at, true)) = *w else { continue };
                if t != team || dist(at, pos) >= radius {
                    continue;
                }
                if let Some(other) = c.bases.iter().position(|o| o.flag.carrier == Some(k)) {
                    c.home(other);
                    c.captures[team as usize - 1] += 1;
                    c.log.push((step, other, FlagEvent::Captured(k)));
                    scores.push((k, Score::Capture));
                }
            }
            // the nearest enemy takes it
            if let Some(k) = nearest(pos, radius, &|t| t != team) {
                let f = &mut c.bases[b].flag;
                (f.home, f.carrier) = (false, Some(k));
                c.log.push((step, b, FlagEvent::Taken(k)));
                scores.push((k, Score::Attack));
            }
        }
        for b in 0..c.bases.len() {
            let f = &mut c.bases[b].flag;
            if f.home {
                continue;
            }
            if let Some(k) = f.carrier {
                f.timer = f.respawn;
                if let Some(Some((_, at, _))) = who.get(k) {
                    f.pos = *at;
                }
                continue;
            }
            f.timer -= dt;
            if f.timer < 0.0 {
                c.home(b);
                c.log.push((step, b, FlagEvent::Home));
                continue;
            }
            let (team, pos, radius) = (f.team, f.pos, f.radius);
            if let Some(k) = nearest(pos, radius, &|t| t == team) {
                c.home(b);
                c.log.push((step, b, FlagEvent::Returned(k)));
                scores.push((k, Score::Defence));
            } else if let Some(k) = nearest(pos, radius, &|t| t != team) {
                let f = &mut c.bases[b].flag;
                f.carrier = Some(k);
                c.log.push((step, b, FlagEvent::Taken(k)));
                scores.push((k, Score::Attack));
            }
        }
        // the round's end: a team at the score limit (team 1 first)
        let limit = self.rules.score_limit;
        let won = (limit > 0).then(|| (0..2).find(|&t| c.captures[t] >= limit)).flatten();
        self.modes.ctf = Some(c);
        for (k, s) in scores {
            self.score(k, s);
        }
        if let Some(t) = won
            && let Some(q) = self.modes.conquest.as_mut()
            && q.winner.is_none()
        {
            q.winner = Some(t as u8 + 1);
        }
    }

    /// A carrier's death: his flag falls where he was, on the terrain
    /// + 1.5 m, its clock from its `TimeToReSpawn`.
    pub(crate) fn ctf_drop(&mut self, k: usize, at: V3) {
        let ground = self.world.height_at(at[0] as f64, at[2] as f64).map_or(at[1], |h| h as f32 + 1.5);
        let step = self.clock.steps;
        let Some(c) = self.modes.ctf.as_mut() else { return };
        for b in 0..c.bases.len() {
            let f = &mut c.bases[b].flag;
            if f.carrier == Some(k) {
                f.carrier = None;
                f.pos = [at[0], ground, at[2]];
                f.timer = f.respawn;
                c.log.push((step, b, FlagEvent::Dropped));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> Option<std::path::PathBuf> {
        let game = bytefield_testdata::game();
        game.is_dir().then_some(game)
    }

    /// A soldier of `team`, alive, and his vehicle index.
    fn soldier(s: &mut crate::Sim, team: u32, template: &str) -> (usize, usize) {
        let k = s.add_player(team, template, false);
        for _ in 0..30 * 30 {
            s.step();
            if s.players[k].alive {
                break;
            }
        }
        if s.players[k].soldier.is_none() {
            let c = s.modes.conquest.as_ref().unwrap();
            panic!("no spawn: groups {:?} tickets {:?} winner {:?} player {:?}", c.groups.iter().map(|(id, g)| (*id, g.team, g.spawns.len())).collect::<Vec<_>>(), c.tickets, c.winner, s.players[k]);
        }
        (k, s.players[k].soldier.unwrap())
    }

    fn put(s: &mut crate::Sim, v: usize, at: V3) {
        let h = s.world.height_at(at[0] as f64, at[2] as f64).unwrap() as f32;
        s.vehicles[v].craft.body.pos = [at[0], h + 1.0, at[2]];
        s.step();
    }

    /// El Alamein's CTF: a German takes the British flag at its base (`Attack`), brings it to his
    /// own (`FlagCapture`: 10 points in `ScoreManagerSettingsCTF.con`, a capture for team 1, the
    /// flag home), and with the score limit at 1 that ends the round. The tickets do not run down.
    #[test]
    fn a_flag_taken_and_brought_home_is_a_capture() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "ctf").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.rules.score_limit = 1;
        s.start_conquest();
        let c = s.modes.ctf.as_ref().unwrap();
        assert_eq!(c.bases.len(), 2);
        let uk = c.bases.iter().position(|b| b.team == 2).unwrap();
        let ge = c.bases.iter().position(|b| b.team == 1).unwrap();
        assert_eq!((c.bases[uk].flag.respawn, c.bases[uk].radius), (30.0, 5.0));
        let (k, v) = soldier(&mut s, 1, "GermanSoldier");
        let tickets = s.modes.conquest.as_ref().unwrap().tickets;
        let score = s.players[k].score;
        let at = s.modes.ctf.as_ref().unwrap().bases[uk].pos;
        put(&mut s, v, at);
        let c = s.modes.ctf.as_ref().unwrap();
        assert_eq!(c.bases[uk].flag.carrier, Some(k));
        assert!(!c.bases[uk].flag.home);
        assert_eq!(s.players[k].score, score + s.rules.scores.attack);
        let at = s.modes.ctf.as_ref().unwrap().bases[ge].pos;
        put(&mut s, v, at);
        let c = s.modes.ctf.as_ref().unwrap();
        assert_eq!(c.captures, [1, 0]);
        assert!(c.bases[uk].flag.home && c.bases[uk].flag.carrier.is_none());
        assert_eq!(s.players[k].score, score + s.rules.scores.attack + s.rules.scores.capture);
        assert_eq!(s.rules.scores.capture, 10);
        assert_eq!(s.modes.conquest.as_ref().unwrap().winner, Some(1));
        assert_eq!(s.modes.conquest.as_ref().unwrap().tickets, tickets);
    }

    /// The flags go by the network: a flag taken on the server is carried in the
    /// copy that gets the snapshot, and the captures with it.
    #[test]
    fn a_flag_goes_by_the_network() {
        use crate::replica::{Delta, Snapshot};
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "ctf").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let uk = s.modes.ctf.as_ref().unwrap().bases.iter().position(|b| b.team == 2).unwrap();
        let (k, v) = soldier(&mut s, 1, "GermanSoldier");
        let at = s.modes.ctf.as_ref().unwrap().bases[uk].pos;
        put(&mut s, v, at);
        assert_eq!(s.modes.ctf.as_ref().unwrap().bases[uk].flag.carrier, Some(k));
        let d = Delta::decode(&Snapshot::of(&s).delta(None).encode()).unwrap();
        let mut copy = crate::Sim::new(&w, 1.0 / 30.0);
        copy.start_conquest();
        Snapshot::apply(None, &d).write(&mut copy);
        let (a, b) = (&s.modes.ctf.as_ref().unwrap().bases[uk].flag, &copy.modes.ctf.as_ref().unwrap().bases[uk].flag);
        assert_eq!((b.carrier, b.home, b.pos), (a.carrier, a.home, a.pos));
        // unchanged, it does not go again
        let snap = Snapshot::of(&s);
        assert!(snap.delta(Some(&snap)).modes.is_none());
    }

    /// A carrier killed drops the flag on the ground; a soldier of the flag's team who reaches it
    /// sends it home (`Defence`); left alone it goes home after its 30 s.
    #[test]
    fn a_dropped_flag_goes_home() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "ctf").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let uk = s.modes.ctf.as_ref().unwrap().bases.iter().position(|b| b.team == 2).unwrap();
        let (k, v) = soldier(&mut s, 1, "GermanSoldier");
        let base = s.modes.ctf.as_ref().unwrap().bases[uk].pos;
        put(&mut s, v, base);
        assert_eq!(s.modes.ctf.as_ref().unwrap().bases[uk].flag.carrier, Some(k));
        // away from the base, killed
        put(&mut s, v, [base[0] + 40.0, 0.0, base[2]]);
        s.give_damage(v, 1000.0, crate::damage::Source::Hit);
        s.step();
        let f = &s.modes.ctf.as_ref().unwrap().bases[uk].flag;
        assert!(f.carrier.is_none() && !f.home);
        let ground = s.world.height_at(f.pos[0] as f64, f.pos[2] as f64).unwrap() as f32;
        assert!((f.pos[1] - ground - 1.5).abs() < 1e-3, "{:?} {ground}", f.pos);
        // a Briton picks it up: home, and his points
        let dropped = f.pos;
        let (b, bv) = soldier(&mut s, 2, "BritishSoldier");
        let score = s.players[b].score;
        put(&mut s, bv, dropped);
        let f = &s.modes.ctf.as_ref().unwrap().bases[uk].flag;
        assert!(f.home, "{f:?}");
        assert_eq!(s.players[b].score, score + s.rules.scores.defence);
        assert!(s.modes.ctf.as_ref().unwrap().log.iter().any(|e| e.2 == FlagEvent::Returned(b)));
    }
}
