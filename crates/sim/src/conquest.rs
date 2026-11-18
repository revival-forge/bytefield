//! Conquest, read from the original game: control points that
//! change hands, what they switch (`CPEnable`/`CPDisable`:
//! the team of their spawn groups and object spawners), and the tickets (`restartMap`,
//! `gameStatusPlaying`).
//!
//! "Players" are the bench's soldiers with a team: a control point sees the alive ones inside its
//! radius.

use std::collections::BTreeMap;

use bytefield_con::templates::TemplateDb;
use serde::Serialize;

use crate::vecmath::V3;

/// A `ControlPointTemplate`'s numbers (the engine's defaults).
#[derive(Debug, Clone, Serialize)]
pub struct ControlPointTemplate {
    pub name: String,
    /// `radius` (10): a player counts within this (3D) distance of its position
    pub radius: f32,
    /// `team` (0: neutral) at the start
    pub team: u32,
    /// `timeToGetControl`, `timeToLoseControl` (5 s)
    pub time_to_get: f32,
    pub time_to_lose: f32,
    pub disable_if_enemy_inside: bool,
    pub disable_when_losing: bool,
    /// `loseControlWhenEnemyClose` (1), `loseControlWhenNotClose` (0)
    pub lose_when_enemy_close: bool,
    pub lose_when_not_close: bool,
    pub unable_to_change_team: bool,
    /// `onlyTakeableByTeam` (0: any)
    pub only_takeable_by: u32,
    /// `minNrToTakeControl` (1)
    pub min_to_take: u32,
    /// `spawnGroupId`, `secondSpawnGroupId` (team 2's when set), `objectSpawnerId` (−1: none)
    pub spawn_group: i64,
    pub second_spawn_group: i64,
    pub object_spawner: i64,
    /// `areaValueTeam1/2` (`areaValue` sets both): what holding it weighs in the tickets
    pub area_value: [u32; 2],
}

/// The control point's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CpStatus {
    /// 1: nobody's
    Neutral,
    /// 2: its team is losing it
    Losing,
    /// 3: a team is taking it
    Taking,
    /// 4: held (or neutral and quiet)
    Held,
}

/// A control point in the sim.
#[derive(Debug, Clone, Serialize)]
pub struct ControlPoint {
    pub template: ControlPointTemplate,
    pub pos: V3,
    /// the team that holds it, its status, the time left to lose it, the time to take it and who takes
    /// it, and whether it is enabled (its spawn groups and spawners are its team's)
    pub team: u32,
    pub status: CpStatus,
    pub lose: f32,
    pub get: f32,
    pub taker: u32,
    pub enabled: bool,
}

/// A `BFSpawnGroup`: its team (−1 at first; `spawnPointManager.groupTeam`), whether a control
/// point may change it (`groupEnableToChangeTeam`, 1), and how many soldier spawn points it has
/// (a group without any does not count for a team).
#[derive(Debug, Clone, Serialize)]
pub struct SpawnGroup {
    pub team: i64,
    pub changeable: bool,
    pub points: usize,
    /// its soldier spawn points, in the level's order
    pub spawns: Vec<SpawnPoint>,
    /// `onlyForAI` and `onlyForHuman`: the spawn group's active lookup reads
    /// the one that shuts the group to a person or to a bot (Battle of Britain, Coral Sea)
    pub only_ai: bool,
    pub only_human: bool,
}

impl Default for SpawnGroup {
    fn default() -> SpawnGroup {
        SpawnGroup { team: -1, changeable: true, points: 0, spawns: Vec::new(), only_ai: false, only_human: false }
    }
}

impl SpawnGroup {
    /// Whether a bot (`ai`) or a person may spawn here (`getActive`'s flag; the points apart).
    pub fn open_to(&self, ai: bool) -> bool {
        !if ai { self.only_human } else { self.only_ai }
    }
}

/// A soldier spawn point (`SpawnPoint`): where and turned how (yaw, pitch, roll), and its
/// template's `setEnterOnSpawn` (the soldier gets into the vehicle there; the point
/// counts only with an entry point within 5 m), `setAIEnterOnSpawn` (bots, within 15
/// m), `setSpawnAsParaTroper` (falling at 3 m/s with its parachute open) and
/// `setMinSpawnHeight` (−1: none; the point counts only that high over the ground or
/// the water).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SpawnPoint {
    pub pos: V3,
    pub rot: V3,
    pub enter_on_spawn: bool,
    pub ai_enter_on_spawn: bool,
    pub paratrooper: bool,
    pub min_height: f32,
    /// on a vehicle (its index, and the point's place in its frame): it goes where the
    /// vehicle goes, and away with it
    pub on: Option<(usize, V3)>,
}

/// A template's first argument of `n`.
fn first(t: &bytefield_con::templates::Template, n: &str) -> Option<String> {
    t.get(n).and_then(|a| a.first()).map(|v| v.trim().to_string())
}

impl SpawnPoint {
    /// A point of `SpawnPoint` template `t` at `pos`, turned `rot`.
    pub fn of(t: &bytefield_con::templates::Template, pos: V3, rot: V3) -> SpawnPoint {
        let flag = |n: &str| first(t, n).is_some_and(|v| v != "0");
        SpawnPoint {
            pos,
            rot,
            enter_on_spawn: flag("setenteronspawn"),
            ai_enter_on_spawn: flag("setaienteronspawn"),
            paratrooper: flag("setspawnasparatroper"),
            min_height: first(t, "setminspawnheight").and_then(|v| v.parse().ok()).unwrap_or(-1.0),
            on: None,
        }
    }

    /// Its group (`setGroup`).
    pub fn group_of(t: &bytefield_con::templates::Template) -> Option<i64> {
        first(t, "setgroup").or_else(|| first(t, "group")).and_then(|v| v.parse::<i64>().ok())
    }
}

/// The players' teams (`game.setTeamSkin <team> <soldier>`, `game.setKit <team> <i> <kit>`):
/// the soldier each team spawns as and the kits it can choose.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Teams {
    pub skins: [Option<String>; 2],
    pub kits: [Vec<Option<String>>; 2],
}

/// The tickets' settings (`Game.setNumberOfTickets`, `setTicketLostPerMin`, the server's
/// `serverMaxPlayers` and `serverTicketRatio`; `game.assaultTeam`).
#[derive(Debug, Clone, Serialize)]
pub struct TicketRules {
    pub tickets: [u32; 2],
    pub lost_per_min: [f32; 2],
    pub lost_at_end_per_min: f32,
    pub max_players: u32,
    pub ticket_ratio: f32,
    /// `game.assaultTeam` (`setAssultTeam`; 0 in the constructor): the team
    /// that attacks; when the time limit ends a round, the other one wins (`gameStatusPlaying`)
    #[serde(default)]
    pub assault_team: u8,
    /// `game.timeToNextWave` (`setTimeToNextWave`: the long part of the spawn waves'
    /// cycle, [`crate::players::Waves::add`]): the level's runs after the server's `serverSpawnTime` (original, Cassino
    /// in coop: 20 from `startHostGame`, then 0 twice from the level), so it wins
    #[serde(default)]
    pub time_to_next_wave: Option<f32>,
}

impl Default for TicketRules {
    fn default() -> TicketRules {
        TicketRules { tickets: [0, 0], lost_per_min: [0.0, 0.0], lost_at_end_per_min: 500.0, max_players: 32, ticket_ratio: 1.0, assault_team: 0, time_to_next_wave: None }
    }
}

/// What a control point did (the bench's log).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum CpEvent {
    /// it is now this team's
    Taken(u32),
    /// it is now nobody's (it was this team's)
    Lost(u32),
}

/// The game's conquest state.
#[derive(Debug, Clone, Serialize)]
pub struct Conquest {
    /// the teams' soldiers and kits
    pub teams: Teams,
    pub points: Vec<ControlPoint>,
    pub groups: BTreeMap<i64, SpawnGroup>,
    pub rules: TicketRules,
    pub tickets: [i32; 2],
    /// seconds until each team's next ticket goes
    pub timers: [f32; 2],
    pub log: Vec<(u64, usize, CpEvent)>,
    /// the round is over (`gameStatusPlaying`'s end of round): who won, 1 or 2, or 0 a draw
    pub winner: Option<u8>,
    /// the tickets run down (`gameStatusPlaying`: not in CTF nor TDM)
    pub bleed: bool,
    /// objective mode: each team's own tickets (`normalTicketCount` and the next static, set in
    /// `gamaStatusFirstPreGame`); `tickets` is then what `TeamScore` holds between frames, these
    /// scaled by the other team's root objective (`Sim::objective_tickets`)
    pub normal: Option<[i32; 2]>,
    /// the vehicles whose spawn points (if any) are in the groups
    #[serde(skip)]
    pub carried: std::collections::BTreeSet<usize>,
}

/// One "player": a team and where its soldier is (alive).
pub type Player = (u32, V3);

fn dist(a: V3, b: V3) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    (d[2] * d[2] + d[0] * d[0] + d[1] * d[1]).sqrt()
}

/// What a control point's frame changes about the spawners of its `objectSpawnerId`: their team
/// and whether they are active (`CPEnable`/`CPDisable`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SpawnerSwitch {
    pub os_id: i64,
    pub team: u32,
    pub active: bool,
}

impl ControlPoint {
    /// `CPEnable` / `CPDisable`: the spawn group(s) take its team (or 0), its spawners its team
    /// and are switched on (off).
    fn enable(&mut self, on: bool, groups: &mut BTreeMap<i64, SpawnGroup>, out: &mut Vec<SpawnerSwitch>) {
        if self.enabled == on {
            return;
        }
        self.enabled = on;
        let t = &self.template;
        let mut set = |id: i64, team: i64| {
            if let Some(g) = groups.get_mut(&id).filter(|g| g.changeable) {
                g.team = team;
            }
        };
        if on {
            if t.spawn_group != -1 {
                if t.second_spawn_group == -1 || self.team == 1 {
                    set(t.spawn_group, self.team as i64);
                } else if self.team == 2 {
                    set(t.second_spawn_group, self.team as i64);
                }
            }
        } else {
            if t.spawn_group != -1 {
                set(t.spawn_group, 0);
            }
            if t.second_spawn_group != -1 {
                set(t.second_spawn_group, 0);
            }
        }
        if t.object_spawner != -1 {
            out.push(SpawnerSwitch { os_id: t.object_spawner, team: self.team, active: on });
        }
    }

    fn set_team(&mut self, team: u32) {
        if !self.template.unable_to_change_team {
            self.team = team;
        }
    }

    fn takeable_by(&self, team: i64) -> bool {
        self.template.only_takeable_by == 0 || self.template.only_takeable_by as i64 == team
    }

    fn reset_timers(&mut self) {
        self.get = self.template.time_to_get;
        self.lose = self.template.time_to_lose;
    }

    /// [`ControlPoint::init`], as the level loads: off (the constructor's), and one with a team takes
    /// control of itself (`control(0)`: `CPEnable`); a neutral one leaves its spawn groups and
    /// spawners as they are (150n: El_Alamein's coop `AAGunSpawner` with `setTeam 1` in
    /// `SouthOpenBase` makes its flak38).
    pub fn init(&mut self, groups: &mut BTreeMap<i64, SpawnGroup>, out: &mut Vec<SpawnerSwitch>) {
        self.reset_timers();
        self.team = self.template.team;
        self.taker = 0;
        self.status = CpStatus::Neutral;
        self.enabled = false;
        if self.team != 0 {
            self.status = CpStatus::Held;
            self.enable(true, groups, out);
        }
    }

    /// [`ControlPoint::reset`] (`restartMap`).
    pub fn reset(&mut self, groups: &mut BTreeMap<i64, SpawnGroup>, out: &mut Vec<SpawnerSwitch>) {
        self.reset_timers();
        self.team = self.template.team;
        self.taker = 0;
        // `reset` sets the flag to the opposite first, so the switch always happens
        if self.team == 0 {
            self.status = CpStatus::Neutral;
            self.enabled = true;
            self.enable(false, groups, out);
        } else {
            self.status = CpStatus::Held;
            self.enabled = false;
            self.enable(true, groups, out);
        }
    }

    /// The control point's frame update handling with the alive players: a team alone inside takes a
    /// neutral point in `timeToGetControl`, an enemy inside makes its holders lose it in
    /// `timeToLoseControl`; two enemy teams inside: nothing moves.
    pub fn frame(&mut self, dt: f32, players: &[Player], groups: &mut BTreeMap<i64, SpawnGroup>, out: &mut Vec<SpawnerSwitch>) -> Option<CpEvent> {
        let mut friendly = false;
        let (mut enemy, mut count) = (0u32, 0u32);
        for &(team, pos) in players {
            if dist(pos, self.pos) > self.template.radius {
                continue;
            }
            if team == self.team {
                friendly = true;
            } else {
                if count > 0 && enemy != team {
                    return None;
                }
                count += 1;
                enemy = team;
            }
        }
        let t = self.template.clone();
        let (lose, lost) = (|s: &mut ControlPoint, by: i64, dt: f32, groups: &mut BTreeMap<i64, SpawnGroup>, out: &mut Vec<SpawnerSwitch>| {
            // `losingControl`
            if !s.takeable_by(by) {
                return;
            }
            s.status = CpStatus::Losing;
            s.lose -= dt;
            s.enable(!s.template.disable_when_losing, groups, out);
        }, |s: &mut ControlPoint, by: i64, groups: &mut BTreeMap<i64, SpawnGroup>, out: &mut Vec<SpawnerSwitch>| {
            // `lostControl`
            if !s.takeable_by(by) {
                return None;
            }
            let was = s.team;
            s.status = CpStatus::Neutral;
            s.set_team(0);
            s.enable(false, groups, out);
            Some(CpEvent::Lost(was))
        });
        // `control(team)`: held and quiet; disabled while an enemy is inside (`disableIfEnemyInsideRadius`)
        let control = |s: &mut ControlPoint, team: u32, groups: &mut BTreeMap<i64, SpawnGroup>, out: &mut Vec<SpawnerSwitch>| {
            s.status = CpStatus::Held;
            s.reset_timers();
            s.enable(team == 0, groups, out);
        };
        if friendly {
            if enemy != 0 {
                if t.lose_when_enemy_close {
                    if self.lose > 0.0 {
                        lose(self, enemy as i64, dt, groups, out);
                        return None;
                    }
                    if self.status != CpStatus::Losing {
                        return None;
                    }
                    return lost(self, enemy as i64, groups, out);
                }
                if t.disable_if_enemy_inside {
                    control(self, enemy, groups, out);
                    return None;
                }
            }
            control(self, 0, groups, out);
            return None;
        }
        if enemy != 0 {
            if self.team != 0 {
                if self.lose > 0.0 {
                    lose(self, enemy as i64, dt, groups, out);
                    return None;
                }
                return lost(self, enemy as i64, groups, out);
            }
            if (self.taker == 0 || self.taker == enemy) && t.min_to_take <= count {
                if self.get > 0.0 {
                    // `gettingControl`
                    if self.takeable_by(enemy as i64) {
                        self.status = CpStatus::Taking;
                        self.taker = enemy;
                        self.get -= dt;
                    }
                    return None;
                }
                // `gotControl`
                if !self.takeable_by(enemy as i64) {
                    return None;
                }
                self.status = CpStatus::Held;
                self.reset_timers();
                self.set_team(enemy);
                self.enable(true, groups, out);
                return Some(CpEvent::Taken(self.team));
            }
            self.taker = 0;
            self.reset_timers();
            return None;
        }
        if self.team == 0 {
            // `faildGettingControl`
            self.taker = 0;
            self.reset_timers();
            return None;
        }
        if t.lose_when_not_close {
            if self.lose > 0.0 {
                lose(self, -1, dt, groups, out);
                return None;
            }
            if self.status != CpStatus::Losing {
                return None;
            }
            return lost(self, -1, groups, out);
        }
        control(self, 0, groups, out);
        None
    }
}

impl Conquest {
    /// The level's control points, spawn groups and ticket rules (from the loaded scripts).
    pub fn from_db(db: &TemplateDb) -> Conquest {
        let mut points = Vec::new();
        for i in &db.instances {
            let Some(t) = db.object_template(&i.template).filter(|t| t.kind.eq_ignore_ascii_case("controlpoint")) else { continue };
            let num = |n: &str, d: f32| first(t, n).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
            let int = |n: &str, d: i64| first(t, n).and_then(|v| v.parse::<i64>().ok()).unwrap_or(d);
            let flag = |n: &str, d: bool| first(t, n).map_or(d, |v| v != "0");
            let Some(pos) = i.props.iter().rev().find(|p| p.name == "absoluteposition").and_then(|p| p.args.first()).and_then(|v| bytefield_world::parse_vec(v)).filter(|v| v.len() == 3)
            else {
                continue;
            };
            // `areaValue a` sets both; `areaValueTeam1/2` one each; the last one wins
            let mut area = [0u32; 2];
            for p in &t.props {
                let v = p.args.first().and_then(|v| v.trim().parse::<u32>().ok());
                match (p.name.as_str(), v) {
                    ("areavalue" | "setareavalue", Some(v)) => area = [v, v],
                    ("areavalueteam1", Some(v)) => area[0] = v,
                    ("areavalueteam2", Some(v)) => area[1] = v,
                    _ => {}
                }
            }
            // ([`ControlPoint::reset`] takes the template's team: an instance's `Object.setTeam` only
            // sets the current one, which the round's start overwrites; GC's levels carry
            // garbage there, `Object.setTeam 7627091`)
            let team = int("team", 0) as u32;
            let template = ControlPointTemplate {
                name: t.name.clone(),
                radius: num("radius", 10.0),
                team,
                time_to_get: num("timetogetcontrol", 5.0),
                time_to_lose: num("timetolosecontrol", 5.0),
                disable_if_enemy_inside: flag("disableifenemyinsideradius", false),
                disable_when_losing: flag("disablewhenlosingcontrol", false),
                lose_when_enemy_close: flag("losecontrolwhenenemyclose", true),
                lose_when_not_close: flag("losecontrolwhennotclose", false),
                unable_to_change_team: flag("unabletochangeteam", false),
                only_takeable_by: int("onlytakeablebyteam", 0) as u32,
                min_to_take: int("minnrtotakecontrol", 1) as u32,
                spawn_group: int("spawngroupid", -1),
                second_spawn_group: int("secondspawngroupid", -1),
                object_spawner: int("objectspawnerid", -1),
                area_value: area,
            };
            points.push(ControlPoint {
                template,
                pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                team,
                status: CpStatus::Held,
                lose: 0.0,
                get: 0.0,
                taker: 0,
                enabled: false,
            });
        }
        let mut groups: BTreeMap<i64, SpawnGroup> = BTreeMap::new();
        let mut rules = TicketRules::default();
        let mut teams = Teams::default();
        let mut cur: Option<i64> = None;
        for c in &db.globals {
            let args = &c.args;
            let a0 = args.first().map(|v| v.trim());
            let int = |i: usize| args.get(i).and_then(|v| v.trim().parse::<i64>().ok());
            let num = |i: usize| args.get(i).and_then(|v| v.trim().parse::<f32>().ok());
            match c.command.to_ascii_lowercase().as_str() {
                "spawnpointmanager.group" => {
                    cur = int(0);
                    if let Some(g) = cur {
                        groups.entry(g).or_default();
                    }
                }
                "spawnpointmanager.groupteam" => {
                    if let (Some(g), Some(t)) = (cur, int(0)) {
                        groups.entry(g).or_default().team = t;
                    }
                }
                "spawnpointmanager.groupenabletochangeteam" => {
                    if let (Some(g), Some(v)) = (cur, a0) {
                        groups.entry(g).or_default().changeable = v != "0";
                    }
                }
                "spawnpointmanager.onlyforai" => {
                    if let (Some(g), Some(v)) = (cur, a0) {
                        groups.entry(g).or_default().only_ai = v != "0";
                    }
                }
                "spawnpointmanager.onlyforhuman" => {
                    if let (Some(g), Some(v)) = (cur, a0) {
                        groups.entry(g).or_default().only_human = v != "0";
                    }
                }
                "game.setnumberoftickets" => {
                    if let (Some(t @ 1..=2), Some(n)) = (int(0), int(1)) {
                        rules.tickets[t as usize - 1] = n as u32;
                    }
                }
                "game.setticketlostpermin" => {
                    if let (Some(t @ 1..=2), Some(n)) = (int(0), num(1)) {
                        rules.lost_per_min[t as usize - 1] = n;
                    }
                }
                "game.setteamskin" => {
                    if let (Some(t @ 1..=2), Some(n)) = (int(0), args.get(1)) {
                        teams.skins[t as usize - 1] = Some(n.trim().to_string());
                    }
                }
                "game.setkit" => {
                    if let (Some(t @ 1..=2), Some(i @ 0..=15), Some(n)) = (int(0), int(1), args.get(2)) {
                        let kits = &mut teams.kits[t as usize - 1];
                        if kits.len() <= i as usize {
                            kits.resize(i as usize + 1, None);
                        }
                        kits[i as usize] = Some(n.trim().to_string());
                    }
                }
                "game.timetonextwave" => {
                    if let Some(n) = num(0) {
                        rules.time_to_next_wave = Some(n);
                    }
                }
                "game.assaultteam" => {
                    if let Some(n) = int(0) {
                        rules.assault_team = n as u8;
                    }
                }
                "game.setticketlostatendpermin" => {
                    if let Some(n) = num(0) {
                        rules.lost_at_end_per_min = n;
                    }
                }
                _ => {}
            }
        }
        // soldier spawn points: each instance of a `SpawnPoint` template counts for its group
        for i in &db.instances {
            let Some(t) = db.object_template(&i.template).filter(|t| t.kind.eq_ignore_ascii_case("spawnpoint")) else { continue };
            if let Some(g) = first(t, "setgroup").or_else(|| first(t, "group")).and_then(|v| v.parse::<i64>().ok()) {
                let vec3 = |name: &str| {
                    i.props.iter().rev().find(|p| p.name == name).and_then(|p| p.args.first()).and_then(|v| bytefield_world::parse_vec(v)).filter(|v| v.len() == 3).map(|v| [v[0] as f32, v[1] as f32, v[2] as f32])
                };
                let e = groups.entry(g).or_default();
                e.points += 1;
                if let Some(pos) = vec3("absoluteposition") {
                    e.spawns.push(SpawnPoint::of(t, pos, vec3("rotation").unwrap_or([0.0; 3])));
                }
            }
        }
        // and those in a placed object's tree (GC_Bespin's `echo_chair_imp`: three of global group
        // 70, team 1; the original counts them, gc_bespin_branch). A vehicle's go with it
        // (`Sim::sync_carried_spawn_points`).
        for i in &db.instances {
            if db.object_template(&i.template).is_none_or(|t| ["spawnpoint", "playercontrolobject"].iter().any(|k| t.kind.eq_ignore_ascii_case(k))) {
                continue;
            }
            let found = bytefield_world::child_spawn_points(db, &i.template);
            if found.is_empty() {
                continue;
            }
            let vec3 = |name: &str| {
                i.props.iter().rev().find(|p| p.name == name).and_then(|p| p.args.first()).and_then(|v| bytefield_world::parse_vec(v)).filter(|v| v.len() == 3).map(|v| [v[0] as f32, v[1] as f32, v[2] as f32])
            };
            let Some(pos) = vec3("absoluteposition") else { continue };
            let rot = vec3("rotation").unwrap_or([0.0; 3]);
            let m = crate::ref2::placed(pos.map(f64::from), rot.map(f64::from));
            for (name, local) in found {
                let Some(t) = db.object_template(&name) else { continue };
                let Some(g) = SpawnPoint::group_of(t) else { continue };
                let lp = [local.0[3][0] as f32, local.0[3][1] as f32, local.0[3][2] as f32];
                let p = [0, 1, 2].map(|j| m[12 + j] + lp[0] * m[j] + lp[1] * m[4 + j] + lp[2] * m[8 + j]);
                let e = groups.entry(g).or_default();
                e.points += 1;
                e.spawns.push(SpawnPoint::of(t, p, rot));
            }
        }
        let mut c = Conquest { teams, points, groups, rules, tickets: [0, 0], timers: [0.0, 0.0], log: Vec::new(), winner: None, bleed: true, normal: None, carried: Default::default() };
        c.restart(&mut Vec::new());
        c
    }

    /// `ticketLostPerMin` × `serverMaxPlayers` / 16: tickets a minute, and at the
    /// end.
    pub fn rate(&self, team: usize) -> f32 {
        self.rules.max_players as f32 * 0.0625 * self.rules.lost_per_min[team]
    }

    pub fn rate_at_end(&self) -> f32 {
        self.rules.lost_at_end_per_min * (self.rules.max_players as f32 * 0.0625)
    }

    /// The round's start: the tickets and timers of `restartMap` (n × `ticketRatio` × max players
    /// / 16, rounded) and the control points' `init` (the load's; `restartMap` runs their `reset`,
    /// and then the spawner's reset gives the spawners back their own team outside the pre-game:
    /// not needed: here a new round is a new `Sim`).
    pub fn restart(&mut self, out: &mut Vec<SpawnerSwitch>) {
        let k = self.rules.ticket_ratio * self.rules.max_players as f32 * 0.0625;
        for t in 0..2 {
            self.tickets[t] = (self.rules.tickets[t] as f32 * k).round_ties_even() as i32;
            self.timers[t] = 60.0 / self.rate(t);
        }
        for p in self.points.iter_mut() {
            p.init(&mut self.groups, out);
        }
    }

    /// Groups of a team with spawn points (`getGroupsForTeam`).
    pub fn groups_of(&self, team: i64) -> usize {
        self.groups.values().filter(|g| g.team == team && g.points > 0).count()
    }

    /// `canTeamGetAnySpawnGroup`: another team's group with spawn points that may change hands.
    pub fn can_get_a_group(&self, team: i64) -> bool {
        self.groups.values().any(|g| g.team != team && g.points > 0 && g.changeable)
    }

    /// One frame: the control points with these players, then the tickets
    /// (`gameStatusPlaying`). Returns the spawners to switch.
    pub fn frame(&mut self, step: u64, dt: f32, players: &[Player]) -> Vec<SpawnerSwitch> {
        let out = self.points_frame(step, dt, players);
        if self.bleed {
            self.tickets_frame(dt, players);
        }
        out
    }

    /// The control points' part of a frame (the control point's frame update handling, on the frame's
    /// clock: the server's real one).
    pub fn points_frame(&mut self, step: u64, dt: f32, players: &[Player]) -> Vec<SpawnerSwitch> {
        let mut out = Vec::new();
        for (k, p) in self.points.iter_mut().enumerate() {
            if let Some(e) = p.frame(dt, players, &mut self.groups, &mut out) {
                self.log.push((step, k, e));
            }
        }
        out
    }

    /// The ticket bleed of `gameStatusPlaying` (conquest, coop, objective mode). For each team
    /// (team 2 first): with spawn groups, the other's area value (of the points it holds) over 99
    /// makes its timer run (by dt; by area/100 × dt only when it was looked for alive players and
    /// some were found), else it is reset to 60 / rate; with no group left and nobody alive (or
    /// no group it can get), the timer runs and the tickets go at the end rate. When the timer
    /// passes 0, the tickets it covers go at once.
    fn tickets_frame(&mut self, dt: f32, players: &[Player]) {
        let mut area = [0u32; 2];
        for p in &self.points {
            match p.team {
                1 => area[0] += p.template.area_value[0],
                2 => area[1] += p.template.area_value[1],
                _ => {}
            }
        }
        let g = [self.groups_of(1), self.groups_of(2)];
        let no_spawn = [g[0] == 0 && !self.can_get_a_group(1), g[1] == 0 && !self.can_get_a_group(2)];
        // "nobody alive": only looked at when a team has no group (else it stays true)
        let mut nobody = [true, true];
        if g[0] == 0 || g[1] == 0 {
            for &(t, _) in players {
                if (1..=2).contains(&t) {
                    nobody[t as usize - 1] = false;
                }
            }
        }
        enum Run {
            Full,
            Area,
            Reset,
        }
        for k in [1usize, 0] {
            let a = area[1 - k];
            let d05 = if nobody[k] || no_spawn[k] { Run::Full } else { Run::Area };
            let cf8 = |d05: Run| if a > 99 { d05 } else { Run::Reset };
            let run = if g[k] == 0 {
                if nobody[k] { Run::Full } else if no_spawn[k] { d05 } else { cf8(d05) }
            } else {
                cf8(d05)
            };
            let rate = self.rate(k);
            match run {
                Run::Reset => {
                    self.timers[k] = 60.0 / rate;
                    continue;
                }
                Run::Full => self.timers[k] -= dt,
                Run::Area => self.timers[k] -= (a as f32 * dt) / 100.0,
            }
            if self.timers[k] < 0.0 {
                let r = if g[k] == 0 && (nobody[k] || no_spawn[k]) { self.rate_at_end() } else { rate };
                let per = 60.0 / r;
                // the FPU's truncation (control word 0xc)
                let n = (-self.timers[k] / per) as i32 + 1;
                self.tickets[k] = (self.tickets[k] - n).max(0);
                self.timers[k] += per * n as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(team: u32) -> ControlPoint {
        let template = ControlPointTemplate {
            name: "cp".into(),
            radius: 10.0,
            team,
            time_to_get: 1.0,
            time_to_lose: 1.0,
            disable_if_enemy_inside: false,
            disable_when_losing: false,
            lose_when_enemy_close: true,
            lose_when_not_close: false,
            unable_to_change_team: false,
            only_takeable_by: 0,
            min_to_take: 1,
            spawn_group: 7,
            second_spawn_group: -1,
            object_spawner: 3,
            area_value: [50, 50],
        };
        let mut p = ControlPoint { template, pos: [0.0; 3], team, status: CpStatus::Held, lose: 0.0, get: 0.0, taker: 0, enabled: false };
        p.reset(&mut BTreeMap::new(), &mut Vec::new());
        p
    }

    fn run(p: &mut ControlPoint, players: &[Player], frames: usize, groups: &mut BTreeMap<i64, SpawnGroup>) -> (Vec<CpEvent>, Vec<SpawnerSwitch>) {
        let (mut ev, mut sw) = (Vec::new(), Vec::new());
        for _ in 0..frames {
            ev.extend(p.frame(0.1, players, groups, &mut sw));
        }
        (ev, sw)
    }

    #[test]
    fn a_soldier_alone_takes_a_neutral_point_and_an_enemy_takes_it_away() {
        let mut groups = BTreeMap::from([(7, SpawnGroup { team: 0, points: 3, ..SpawnGroup::default() })]);
        let mut p = point(0);
        // inside: 1 s to take it (the frame that finds the timer at 0 takes it)
        let (ev, sw) = run(&mut p, &[(1, [3.0, 0.0, 0.0])], 12, &mut groups);
        assert_eq!(ev, vec![CpEvent::Taken(1)]);
        assert_eq!((p.team, p.status, p.enabled), (1, CpStatus::Held, true));
        assert_eq!(groups[&7].team, 1);
        assert_eq!(sw.last(), Some(&SpawnerSwitch { os_id: 3, team: 1, active: true }));
        // two teams inside: nothing moves
        let before = p.lose;
        run(&mut p, &[(2, [1.0, 0.0, 0.0]), (3, [1.0, 0.0, 0.0])], 5, &mut groups);
        assert_eq!(p.lose, before);
        // an enemy alone: losing, then lost (neutral), then taken by it
        let (ev, _) = run(&mut p, &[(2, [1.0, 0.0, 0.0])], 30, &mut groups);
        assert_eq!(ev, vec![CpEvent::Lost(1), CpEvent::Taken(2)]);
        assert_eq!(groups[&7].team, 2);
        // an enemy inside with its holder still makes it lose it (`loseControlWhenEnemyClose`);
        // neutral with both teams inside: nobody takes it
        let (ev, _) = run(&mut p, &[(2, [1.0, 0.0, 0.0]), (1, [2.0, 0.0, 0.0])], 30, &mut groups);
        assert_eq!(ev, vec![CpEvent::Lost(2)]);
        assert_eq!((p.team, p.status), (0, CpStatus::Neutral));
    }

    #[test]
    fn nobody_near_leaves_it_as_it_is() {
        let mut p = point(1);
        let (ev, _) = run(&mut p, &[(2, [50.0, 0.0, 0.0])], 50, &mut BTreeMap::new());
        assert!(ev.is_empty());
        assert_eq!((p.team, p.status), (1, CpStatus::Held));
    }
}
