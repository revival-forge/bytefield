//! The radio: what a player says with the F1–F8 menus (`menu/RadioMenu`), to the team
//! (`menu/MenuRadioSound.ssc`, heard by its whole team) or out loud (`SoldierVoice.ssc`, heard
//! around the soldier). The messages are the executable's `RM*` and the lexicon's `RADIO_*`.

use serde::{Deserialize, Serialize};

use crate::Sim;

/// A radio message: its lexicon key (`RADIO_<key>`, the text; `RADIO_HEADING_<key>` the menu's),
/// whether it is said out loud, and its patch in that script (from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Msg {
    pub key: &'static str,
    pub local: bool,
    pub patch: usize,
}

const fn team(key: &'static str, patch: usize) -> Msg {
    Msg { key, local: false, patch }
}
const fn local(key: &'static str, patch: usize) -> Msg {
    Msg { key, local: true, patch }
}

// the team's, in the order of `MenuRadioSound*.ssc`
pub const ROGER: Msg = team("ROGER", 0);
pub const NEGATIVE: Msg = team("NEGATIVE", 1);
pub const GO_FOR_ENEMY_FLAG: Msg = team("LOCAL_GO_FOR_ENEMY_FLAG", 2);
pub const PROTECT_FLAG: Msg = team("LOCAL_PROTECT_FLAG", 3);
pub const PROTECT_FLAGCARRIER: Msg = team("LOCAL_PROTECT_FLAGCARRIER", 4);
pub const GET_FLAG_BACK: Msg = team("LOCAL_GET_FLAG_BACK", 5);
pub const PICKUP: Msg = team("PICKUP", 6);
pub const REINFORCEMENT: Msg = team("REINFORCEMENT", 7);
pub const ANTITANK_SUPPORT: Msg = team("ANTITANK_SUPPORT", 8);
pub const NAVAL_SUPPORT: Msg = team("NAVAL_SUPPORT", 9);
pub const AIR_SUPPORT: Msg = team("AIR_SUPPORT", 10);
pub const ARTILLERY_READY: Msg = team("ARTILLERY_READY", 11);
pub const ARTILLERY_SUPPORT: Msg = team("ARTILLERY_SUPPORT", 12);
pub const APC_SUPPORT: Msg = team("APC_SUPPORT", 13);
pub const ARMOR_SPOTTED: Msg = team("ARMOR_SPOTTED", 14);
pub const INFANTRY_SPOTTED: Msg = team("INFANTRY_SPOTTED", 15);
pub const UNIT_SPOTTED: Msg = team("UNIT_SPOTTED", 16);
pub const SHIP_SPOTTED: Msg = team("SHIP_SPOTTED", 17);
pub const SUBMARINE_SPOTTED: Msg = team("SUBMARINE_SPOTTED", 18);
pub const AIRPLANE_SPOTTED: Msg = team("AIRPLANE_SPOTTED", 19);
pub const SCOUT_SPOTTED: Msg = team("SCOUT_SPOTTED", 20);
pub const ATTACK: Msg = team("ATTACK", 21);
pub const DEFEND: Msg = team("DEFEND", 22);

// out loud, in the order of `SoldierVoice.ssc`
pub const LOCAL_DEFENDING: Msg = local("LOCAL_DEFENDING", 0);
pub const LOCAL_ATTACKING: Msg = local("LOCAL_ATTACKING", 1);
pub const LOCAL_NEGATIVE: Msg = local("LOCAL_NEGATIVE", 2);
pub const LOCAL_ROGER: Msg = local("LOCAL_ROGER", 3);
pub const LOCAL_WAIT: Msg = local("LOCAL_WAIT", 4);
pub const LOCAL_GO: Msg = local("LOCAL_GO", 5);
pub const LOCAL_FOLLOW_ME: Msg = local("LOCAL_FOLLOW_ME", 6);
pub const LOCAL_STICK_TOGETHER: Msg = local("LOCAL_STICK_TOGETHER", 7);
pub const LOCAL_GET_IN: Msg = local("LOCAL_GET_IN", 8);
pub const LOCAL_FIRE_IN_HOLE: Msg = local("LOCAL_FIRE_IN_HOLE", 9);
pub const LOCAL_TAKE_COVER: Msg = local("LOCAL_TAKE_COVER", 10);
pub const LOCAL_CHECK_YOUR_SIX: Msg = local("LOCAL_CHECK_YOUR_SIX", 11);
pub const LOCAL_BAIL_OUT: Msg = local("LOCAL_BAIL_OUT", 12);
pub const LOCAL_ABANDON_SHIP: Msg = local("LOCAL_ABANDON_SHIP", 13);
pub const LOCAL_COVER_ME: Msg = local("LOCAL_COVER_ME", 14);
pub const LOCAL_HOLD_POSITION: Msg = local("LOCAL_HOLD_POSITION", 15);
pub const LOCAL_GO_FOR_THE_FLAG: Msg = local("LOCAL_GO_FOR_ENEMY_FLAG", 16);
pub const LOCAL_FALL_BACK: Msg = local("LOCAL_FALL_BACK", 17);
pub const LOCAL_HOLD_FIRE: Msg = local("LOCAL_HOLD_FIRE", 18);
pub const LOCAL_FIRE: Msg = local("LOCAL_FIRE", 19);
pub const LOCAL_MEDIC: Msg = local("LOCAL_MEDIC", 20);
pub const LOCAL_NEED_REPAIRS: Msg = local("LOCAL_NEED_REPAIRS", 21);

/// The team's messages by patch.
pub const TEAM: [Msg; 23] = [
    ROGER, NEGATIVE, GO_FOR_ENEMY_FLAG, PROTECT_FLAG, PROTECT_FLAGCARRIER, GET_FLAG_BACK, PICKUP, REINFORCEMENT, ANTITANK_SUPPORT, NAVAL_SUPPORT, AIR_SUPPORT, ARTILLERY_READY,
    ARTILLERY_SUPPORT, APC_SUPPORT, ARMOR_SPOTTED, INFANTRY_SPOTTED, UNIT_SPOTTED, SHIP_SPOTTED, SUBMARINE_SPOTTED, AIRPLANE_SPOTTED, SCOUT_SPOTTED, ATTACK, DEFEND,
];
/// Those out loud by patch.
pub const LOCAL: [Msg; 22] = [
    LOCAL_DEFENDING, LOCAL_ATTACKING, LOCAL_NEGATIVE, LOCAL_ROGER, LOCAL_WAIT, LOCAL_GO, LOCAL_FOLLOW_ME, LOCAL_STICK_TOGETHER, LOCAL_GET_IN, LOCAL_FIRE_IN_HOLE, LOCAL_TAKE_COVER,
    LOCAL_CHECK_YOUR_SIX, LOCAL_BAIL_OUT, LOCAL_ABANDON_SHIP, LOCAL_COVER_ME, LOCAL_HOLD_POSITION, LOCAL_GO_FOR_THE_FLAG, LOCAL_FALL_BACK, LOCAL_HOLD_FIRE, LOCAL_FIRE, LOCAL_MEDIC,
    LOCAL_NEED_REPAIRS,
];

/// The message of `patch` in the team's script (`local` false) or out loud.
pub fn msg(local: bool, patch: usize) -> Option<Msg> {
    if local { LOCAL.get(patch) } else { TEAM.get(patch) }.copied()
}

/// What the soldier is in decides some entries of the menus (each has two at one key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Riding {
    Foot,
    Vehicle,
    Plane,
    Ship,
}

/// An entry of a menu: the message, or (F4) a control point to attack or defend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Say(Msg),
    /// the conquest's point n (from 0); `None`: the closest
    Point(Option<usize>),
}

/// Menu `f` (1–8, F1–F8) at key `k` (1–7; 8 cancels), as `menu/RadioMenu` places them. Where it
/// shows one of two at a key (bail out or abandon ship, medic or need repairs, …), which one
/// depends on what the soldier rides: our choice, the file only culls by a variable.
pub fn entry(f: u32, k: u32, riding: Riding) -> Option<Entry> {
    let in_vehicle = riding != Riding::Foot;
    let m = match (f, k) {
        (1, 1) => ROGER,
        (1, 2) => NEGATIVE,
        (2, 1) => PICKUP,
        (2, 2) => REINFORCEMENT,
        (2, 3) => ANTITANK_SUPPORT,
        (2, 4) => NAVAL_SUPPORT,
        (2, 5) => AIR_SUPPORT,
        (2, 6) => ARTILLERY_READY,
        (2, 7) => APC_SUPPORT,
        (3, 1) => ARMOR_SPOTTED,
        (3, 2) => INFANTRY_SPOTTED,
        (3, 3) => UNIT_SPOTTED,
        (3, 4) => SHIP_SPOTTED,
        (3, 5) => SUBMARINE_SPOTTED,
        (3, 6) => AIRPLANE_SPOTTED,
        (3, 7) => SCOUT_SPOTTED,
        (4, 4) => return Some(Entry::Point(None)),
        (4, 1..=7) => return Some(Entry::Point(Some(k as usize - 1))),
        (5, 3) => LOCAL_DEFENDING,
        (5, 4) => LOCAL_ATTACKING,
        (5, 5) => LOCAL_ROGER,
        (5, 6) => LOCAL_NEGATIVE,
        (6, 1) => LOCAL_WAIT,
        (6, 2) => LOCAL_FIRE,
        (6, 3) => LOCAL_HOLD_FIRE,
        (6, 4) => LOCAL_FIRE_IN_HOLE,
        (6, 5) if in_vehicle => LOCAL_NEED_REPAIRS,
        (6, 5) => LOCAL_MEDIC,
        (6, 6) if riding == Riding::Plane => LOCAL_CHECK_YOUR_SIX,
        (6, 6) => LOCAL_TAKE_COVER,
        (6, 7) if riding == Riding::Ship => LOCAL_ABANDON_SHIP,
        (6, 7) => LOCAL_BAIL_OUT,
        (7, 1) => LOCAL_COVER_ME,
        (7, 2) => LOCAL_HOLD_POSITION,
        (7, 3) => LOCAL_GO_FOR_THE_FLAG,
        (7, 4) => LOCAL_FALL_BACK,
        (7, 5) if in_vehicle => LOCAL_GET_IN,
        (7, 5) => LOCAL_STICK_TOGETHER,
        (7, 6) => LOCAL_FOLLOW_ME,
        (7, 7) => LOCAL_GO,
        _ => return None,
    };
    Some(Entry::Say(m))
}

/// What was said: by whom (player and soldier), to which team, where (the soldier's position), and
/// the control point it names.
#[derive(Debug, Clone)]
pub struct RadioEvent {
    pub step: u64,
    pub player: usize,
    pub soldier: Option<usize>,
    pub team: u32,
    pub msg: Msg,
    pub at: [f32; 3],
    pub point: Option<usize>,
}

/// A [`RadioEvent`] as it goes on the network: the message by its script and patch.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Said {
    pub step: u64,
    pub player: u32,
    pub soldier: Option<u32>,
    pub team: u32,
    pub local: bool,
    pub patch: u8,
    pub at: [f32; 3],
    pub point: Option<u32>,
}

impl RadioEvent {
    pub fn said(&self) -> Said {
        let (player, soldier, point) = (self.player as u32, self.soldier.map(|s| s as u32), self.point.map(|p| p as u32));
        Said { step: self.step, player, soldier, team: self.team, local: self.msg.local, patch: self.msg.patch as u8, at: self.at, point }
    }
}

impl Said {
    pub fn event(&self) -> Option<RadioEvent> {
        let msg = msg(self.local, self.patch as usize)?;
        let (player, soldier, point) = (self.player as usize, self.soldier.map(|s| s as usize), self.point.map(|p| p as usize));
        Some(RadioEvent { step: self.step, player, soldier, team: self.team, msg, at: self.at, point })
    }
}

impl Sim<'_> {
    /// Player `k` picks key `d` of menu `f`.
    pub fn radio_pick(&mut self, k: usize, f: u32, d: u32) -> bool {
        match entry(f, d, self.riding(k)) {
            Some(Entry::Say(m)) => self.say(k, m, None),
            Some(Entry::Point(p)) => self.say_point(k, p),
            None => false,
        }
    }

    /// Player `k` says `msg` (a dead player says nothing).
    pub fn say(&mut self, k: usize, msg: Msg, point: Option<usize>) -> bool {
        let Some(p) = self.players.get(k) else { return false };
        let Some(s) = p.soldier.filter(|_| p.alive) else { return false };
        let at = self.vehicles[s].craft.body.pos;
        self.journal.radio_log.push(RadioEvent { step: self.clock.steps, player: k, soldier: Some(s), team: p.team, msg, at, point });
        true
    }

    /// F4 at a control point: attack it if it is not the team's, defend it if it is. `None`, the
    /// closest to the soldier.
    pub fn say_point(&mut self, k: usize, point: Option<usize>) -> bool {
        let Some(p) = self.players.get(k) else { return false };
        let (team, sol) = (p.team, p.soldier);
        let Some(c) = self.modes.conquest.as_ref() else { return false };
        let i = match point {
            Some(i) => i,
            None => {
                let Some(s) = sol else { return false };
                let at = self.vehicles[s].craft.body.pos;
                let d = |q: &[f32; 3]| (0..3).map(|j| (q[j] - at[j]).powi(2)).sum::<f32>();
                let Some(i) = (0..c.points.len()).min_by(|&a, &b| d(&c.points[a].pos).total_cmp(&d(&c.points[b].pos))) else { return false };
                i
            }
        };
        let Some(cp) = c.points.get(i) else { return false };
        let msg = if cp.team == team { DEFEND } else { ATTACK };
        self.say(k, msg, Some(i))
    }

    /// What player `k`'s soldier rides now (the menus' choice).
    pub fn riding(&self, k: usize) -> Riding {
        let Some(s) = self.players.get(k).and_then(|p| p.soldier) else { return Riding::Foot };
        let Some(v) = self.vehicles.get(s).and_then(|v| v.entry.inside).and_then(|i| self.vehicles.get(i)) else { return Riding::Foot };
        let kinds = || v.craft.parts.iter().map(|p| &p.kind);
        if kinds().any(|k| matches!(k, crate::craft::Kind::Wing(_))) {
            Riding::Plane
        } else if kinds().any(|k| matches!(k, crate::craft::Kind::Floating(_))) {
            Riding::Ship
        } else {
            Riding::Vehicle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menus_place_their_messages() {
        assert_eq!(entry(1, 1, Riding::Foot), Some(Entry::Say(ROGER)));
        assert_eq!(entry(3, 7, Riding::Foot), Some(Entry::Say(SCOUT_SPOTTED)));
        assert_eq!(entry(6, 5, Riding::Foot), Some(Entry::Say(LOCAL_MEDIC)));
        assert_eq!(entry(6, 5, Riding::Vehicle), Some(Entry::Say(LOCAL_NEED_REPAIRS)));
        assert_eq!(entry(6, 7, Riding::Ship), Some(Entry::Say(LOCAL_ABANDON_SHIP)));
        assert_eq!(entry(4, 4, Riding::Foot), Some(Entry::Point(None)));
        assert_eq!(entry(5, 1, Riding::Foot), None);
        assert_eq!(entry(2, 8, Riding::Foot), None, "cancel");
        assert!(TEAM.iter().enumerate().all(|(i, m)| m.patch == i && !m.local));
        assert!(LOCAL.iter().enumerate().all(|(i, m)| m.patch == i && m.local));
    }
}
