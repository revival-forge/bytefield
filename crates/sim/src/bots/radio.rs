//! The radio of a bots bot: what it says and when, and what it does with what it hears, from a bot or from a person.
//!
//! What a bot says goes through [`crate::bot_api::Mind::speak`] and [`crate::Sim::say`], as a person's F1–F8 menus; what it hears
//! comes from [`crate::bot_api::BotView::radio`] (its team's messages, and shouts within earshot). The team already knows its medics,
//! engineers and wounded from the icons ([`super::team`]): the radio must not duplicate that. What it adds: (1) a person's request,
//! which no bot heeds without it, (2) the person hears the bots ask and warn, (3) sightings that do not travel otherwise (armour or a
//! plane spotted, said by a person).
//!
//! Five messages, each with a cause anyone could check from outside at that instant, and few of them: [`MAX_PER_LIFE`] a life, the
//! same one at most every [`SAME_MSG_S`] and any one every [`ANY_MSG_S`]:
//! - "medic!" (out loud), on foot with less than [`HURT`] health and a mate within earshot;
//! - "need repairs" (out loud), at the wheel of a land vehicle with less than [`HURT`] health and a mate within earshot;
//! - "get in" (out loud), at the wheel waiting for the passengers the team gave it, a free seat and a mate on foot within earshot;
//! - armour, plane or ship spotted (team), its enemy target in one, within [`SPOT_M`]; once per sighting for the whole team;
//! - attack or defend (team, naming the flag), a squad leader within [`POINT_M`] of a flag it was just sent to.
//!
//! What it heeds, always as a weight in the individual's or the team's choice, never as an order: a "medic!" or "need repairs" brings
//! the nearest medic or engineer of the team (only that one, so two do not run to the same call); "get in" or "pickup" from a person at
//! the wheel puts the bots on foot nearby in its free seats ([`super::team::Team::pedir_subir`]); a sighting said by a person goes
//! into the team's reports of enemies, as one by a bot does ([`super::team::Team::oido_avistado`]).

use super::team::Kind;
use crate::bot_api::{BotView, RadioHeard, VOICE_M, VehicleClass};
use crate::radio::{self as r, Msg};

/// How long a bot remembers what it heard, s.
pub const MEMORY_S: f32 = 20.0;
/// Less health than this (the bot's own, or its vehicle's) is a cause to call a medic or an engineer.
pub const HURT: f32 = 0.5;
/// At most this many messages a life...
pub const MAX_PER_LIFE: usize = 3;
/// …the same one at most once every this, s…
pub const SAME_MSG_S: f64 = 30.0;
/// …and any one at most once every this, s.
pub const ANY_MSG_S: f64 = 8.0;
/// A sighting is said when the enemy vehicle is within this, m...
pub const SPOT_M: f32 = 150.0;
/// …and not if the team said the same kind within this of it in the last [`SPOT_AGAIN_S`].
pub const SPOT_AGAIN_M: f32 = 100.0;
pub const SPOT_AGAIN_S: f64 = 30.0;
/// A leader names the flag it goes to within this of it, m.
pub const POINT_M: f32 = 200.0;
/// A medic or an engineer answers a call from this far, m (the shout carries [`VOICE_M`]; the caller may have moved since).
pub const ANSWER_M: f32 = 70.0;
/// The team sends a medic to a caller from this far, m; a hurt bot with no medic on foot within this does not call.
pub const DISPATCH_M: f32 = 150.0;
/// A medic with an enemy this near is under fire and is not sent to a caller, m.
pub const UNDER_FIRE_M: f32 = 20.0;
/// A medic below this health is not sent to a caller (a medic hurt more heals itself first), 0 to 1.
pub const MEDIC_MIN_HP: f32 = 0.3;
/// "get in" from a person at the wheel: the bots on foot within this of the vehicle get in, m.
pub const GET_IN_M: f32 = 40.0;

/// Something heard on the radio that may change what a bot does: who asked or warned, where, and how long ago (s).
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// "medic!" said out loud by `player` at `at`
    Medic { player: usize, at: [f32; 3], age: f32 },
    /// "need repairs" said out loud from a vehicle
    Repairs { player: usize, at: [f32; 3], age: f32 },
    /// "get in" (out loud) or "pickup" (to the team) from someone at the wheel
    GetIn { player: usize, at: [f32; 3], age: f32 },
    /// an enemy of that class spotted near `at` (where the one who said it was)
    Spotted { player: usize, class: VehicleClass, at: [f32; 3], age: f32 },
    /// attack or defend the control point `point`
    Point { player: usize, point: usize, attack: bool, age: f32 },
}

/// What a bot could say now, worked out by [`super::Bots`] from what it sees and what its team told it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Causes {
    /// at the wheel, waiting for the passengers its team gave it, with a free seat
    pub waiting: bool,
    /// its enemy target rides a vehicle of this class, this near (the team has not said it yet)
    pub spotted: Option<VehicleClass>,
    /// a squad leader near the flag it was sent to: the flag, and whether it attacks it
    pub point: Option<(usize, bool)>,
    /// hurt, and no medic of its side on foot within [`DISPATCH_M`]: nobody would come, so it does not call
    pub no_medic: bool,
    /// a medic that has begun to answer this caller's "medic!"
    pub answering: Option<usize>,
    /// `bots:streetwise`: a line of a two-line exchange ([`St`])
    pub st: Option<St>,
}

/// `bots:streetwise`: what the bot says about what it does (the combat dialogue of the game AI books: the doer says it, another
/// answers), only with the game's own lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum St {
    /// it sets off to flank: "cover me"
    CoverMe,
    /// the nearest mate of one that said "cover me" (this player): "roger"
    Roger(usize),
    /// the last alive of a squad of four or more: "reinforcements"
    Reinforcement,
    /// its target is near the back of a mate: "check your six"
    CheckSix,
    /// it is about to lay a mine, the mine in hand: "fire in the hole"
    Mine,
    /// on its way to steal an enemy's vehicle: "cover me"
    Steal,
    /// at the wheel of the vehicle it stole, with a free seat and a mate on foot near: "get in"
    Stolen,
    /// it fires at an enemy a mate is already firing at: "fire!"
    Finish,
}

impl St {
    /// Whether it is about a streetwise act (mine, steal, stolen vehicle, firing with a mate), not an exchange.
    pub fn act(self) -> bool {
        matches!(self, St::Mine | St::Steal | St::Stolen | St::Finish)
    }

    pub fn msg(self) -> Msg {
        match self {
            St::CoverMe => r::LOCAL_COVER_ME,
            St::Roger(_) => r::LOCAL_ROGER,
            St::Reinforcement => r::REINFORCEMENT,
            St::CheckSix => r::LOCAL_CHECK_YOUR_SIX,
            St::Mine => r::LOCAL_FIRE_IN_HOLE,
            St::Steal => r::LOCAL_COVER_ME,
            St::Stolen => r::LOCAL_GET_IN,
            St::Finish => r::LOCAL_FIRE,
        }
    }
}

/// `bots:streetwise`: a mate this near the back of another (m) and that other this far from the one who sees it (m) is told to check
/// its six; a "cover me" heard this recently is answered (s).
pub const SIX_M: f32 = 30.0;
pub const SIX_FROM_M: f32 = 12.0;
pub const COVER_ANSWER_S: f32 = 3.0;

/// One bot's radio: what it has said this life (for its cooldowns) and what it heard. A mind is made anew for each life, and so is
/// its radio.
#[derive(Clone, Debug, Default)]
pub struct Radio {
    /// what it said, and when (seconds since the round began)
    pub said: Vec<(Msg, f64)>,
    /// the flags it has named this life
    pub named: Vec<usize>,
    /// the callers it has told "roger" this life
    pub acked: Vec<usize>,
    /// what it heard that still matters
    pub heard: Vec<Request>,
    /// `bots:dialogue`: when it said a line about a streetwise act ([`St::act`]): a "medic!" does not wait [`ANY_MSG_S`] after one
    pub acts: Vec<f64>,
}

impl Radio {
    /// Once a tick: what it heard in the last [`MEMORY_S`] s that may change what it does.
    pub fn hear(&mut self, v: &BotView) {
        self.heard = v.radio(MEMORY_S).iter().filter_map(|h| request(h, v.me.team)).collect();
    }

    /// Whether it may say `m` at `t`: not over [`MAX_PER_LIFE`], nor the same within [`SAME_MSG_S`], nor any within [`ANY_MSG_S`].
    pub fn may(&self, m: Msg, t: f64) -> bool {
        let after_act = |s: &(Msg, f64)| m == r::LOCAL_MEDIC && self.acts.contains(&s.1);
        self.said.len() < MAX_PER_LIFE && self.said.iter().all(|s| (t - s.1 >= ANY_MSG_S || after_act(s)) && (s.0 != m || t - s.1 >= SAME_MSG_S))
    }

    /// Once a tick, after thinking: what to say now (a message and the control point it names), or nothing. The first cause that
    /// holds, in order of need: a medic, repairs, a sighting, "get in", the flag.
    pub fn speak(&mut self, v: &BotView, c: &Causes) -> Option<(Msg, Option<usize>)> {
        let me = &v.me;
        let t = v.time;
        // (who is around, only when a cause holds: `others` looks along lines of sight)
        let others = std::cell::OnceCell::new();
        let mates = |on_foot: bool| others.get_or_init(|| v.others()).iter().any(|o| o.team == me.team && (!on_foot || !o.in_vehicle) && dist3(o.pos, me.pos) <= VOICE_M);
        let wheel = me.inside.as_ref().filter(|i| i.seat == 0);
        let land = wheel.is_some_and(|i| matches!(i.class, VehicleClass::Tank | VehicleClass::Car | VehicleClass::Walker));
        // a medic that starts answering a call says "roger" (it is an acknowledgement: neither counted in the cap nor a message with a cause)
        if let Some(j) = c.answering.filter(|j| !self.acked.contains(j)) {
            self.acked.push(j);
            return Some((r::LOCAL_ROGER, None));
        }
        let mut out = None;
        if me.inside.is_none() && me.health > 0.0 && me.health < HURT && !c.no_medic && Kind::de(&me.items) != Kind::Medic && mates(true) {
            out = Some((r::LOCAL_MEDIC, None));
        }
        if out.is_none()
            && land
            && let Some(x) = wheel.and_then(|i| v.vehicle(i.vehicle))
            && x.health > 0.0
            && x.health < HURT
            && mates(false)
        {
            out = Some((r::LOCAL_NEED_REPAIRS, None));
        }
        // (the streetwise lines go before the sightings and the flags: when the cap is tight, they are what a person notices)
        if out.is_none()
            && let Some(x) = c.st.filter(|x| !matches!(x, St::Roger(j) if self.acked.contains(j)))
            // (one place of the cap is kept for "medic!": spending it on these halved the medic calls)
            && self.said.len() + 1 < MAX_PER_LIFE
        {
            out = Some((x.msg(), None));
        }
        if out.is_none() {
            out = c.spotted.and_then(spotted_msg).map(|m| (m, None));
        }
        if out.is_none() && c.waiting && wheel.is_some() && mates(true) {
            out = Some((r::LOCAL_GET_IN, None));
        }
        if out.is_none()
            && let Some((p, attack)) = c.point.filter(|p| !self.named.contains(&p.0))
        {
            out = Some((if attack { r::ATTACK } else { r::DEFEND }, Some(p)));
        }
        let (m, p) = out.filter(|o| self.may(o.0, t))?;
        self.said.push((m, t));
        if c.st.is_some_and(|x| x.act() && x.msg() == m) {
            self.acts.push(t);
        }
        if let Some(St::Roger(j)) = c.st.filter(|x| x.msg() == m) {
            self.acked.push(j);
        }
        if let Some(p) = p {
            self.named.push(p);
        }
        Some((m, p))
    }

    /// The call of a medic or an engineer it should answer: the one asking (player), heard in the last [`MEMORY_S`], from within
    /// [`ANSWER_M`] of `me`, and no other of `helpers` (same trade, of its team: `(player, pos)`) nearer to the caller. `medic`: a
    /// "medic!" call; otherwise "need repairs". The nearest goes, so two do not answer the same call.
    pub fn answer(&self, me: (usize, [f32; 3]), medic: bool, helpers: &[(usize, [f32; 3])]) -> Option<(usize, [f32; 3])> {
        self.heard
            .iter()
            .filter_map(|h| match *h {
                Request::Medic { player, at, .. } if medic => Some((player, at)),
                Request::Repairs { player, at, .. } if !medic => Some((player, at)),
                _ => None,
            })
            .filter(|&(j, at)| j != me.0 && dist3(at, me.1) <= ANSWER_M && !helpers.iter().any(|h| h.0 != me.0 && h.0 != j && closer(h, me, at)))
            .min_by(|a, b| dist3(a.1, me.1).total_cmp(&dist3(b.1, me.1)).then(a.0.cmp(&b.0)))
    }

    /// The "get in" calls heard: who asked and how long ago.
    pub fn get_in(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.heard.iter().filter_map(|h| if let Request::GetIn { player, age, .. } = *h { Some((player, age)) } else { None })
    }

    /// The sightings heard: who said it, of what, where and how long ago.
    pub fn spotted(&self) -> impl Iterator<Item = (usize, VehicleClass, [f32; 3], f32)> + '_ {
        self.heard.iter().filter_map(|h| if let Request::Spotted { player, class, at, age } = *h { Some((player, class, at, age)) } else { None })
    }
}

/// `h` is nearer to `at` than `me` (ties go to the lower player number).
fn closer(h: &(usize, [f32; 3]), me: (usize, [f32; 3]), at: [f32; 3]) -> bool {
    let (a, b) = (dist3(h.1, at), dist3(me.1, at));
    a < b || (a == b && h.0 < me.0)
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt()
}

/// The message that warns of an enemy in a vehicle of this class, if any: armour (tanks and walkers), planes (and helicopters),
/// ships (and submarines).
pub fn spotted_msg(c: VehicleClass) -> Option<Msg> {
    match c {
        VehicleClass::Tank | VehicleClass::Walker => Some(r::ARMOR_SPOTTED),
        VehicleClass::Plane | VehicleClass::Helicopter | VehicleClass::Airship => Some(r::AIRPLANE_SPOTTED),
        VehicleClass::Ship => Some(r::SHIP_SPOTTED),
        VehicleClass::Submarine => Some(r::SUBMARINE_SPOTTED),
        _ => None,
    }
}

/// What a message heard asks for, if a bots bot of `team` heeds it. Only from its own team: an enemy's shout heard is no request.
pub fn request(h: &RadioHeard, team: u32) -> Option<Request> {
    let (player, at, age) = (h.player, h.at, h.age);
    let m = h.msg;
    Some(if m == r::LOCAL_MEDIC {
        Request::Medic { player, at, age }
    } else if m == r::LOCAL_NEED_REPAIRS {
        Request::Repairs { player, at, age }
    } else if m == r::LOCAL_GET_IN || m == r::PICKUP {
        Request::GetIn { player, at, age }
    } else if m == r::ARMOR_SPOTTED {
        Request::Spotted { player, class: VehicleClass::Tank, at, age }
    } else if m == r::AIRPLANE_SPOTTED {
        Request::Spotted { player, class: VehicleClass::Plane, at, age }
    } else if m == r::SHIP_SPOTTED {
        Request::Spotted { player, class: VehicleClass::Ship, at, age }
    } else if m == r::ATTACK || m == r::DEFEND {
        Request::Point { player, point: h.point?, attack: m == r::ATTACK, age }
    } else {
        return None;
    })
    .filter(|_| h.team == team)
}

/// `BYTEFIELD_BOTS_RADIO=1`: each message a bots bot says and each request it heeds (stderr).
pub fn trace() -> bool {
    crate::debug::options().bots_radio
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_helper_answers_a_call() {
        let mut r = Radio::default();
        r.heard = vec![Request::Medic { player: 7, at: [0.0, 0.0, 0.0], age: 1.0 }];
        let a = (1, [30.0, 0.0, 0.0]);
        let b = (2, [20.0, 0.0, 0.0]);
        assert_eq!(r.answer(a, true, &[a, b]), None, "b is nearer");
        assert_eq!(r.answer(b, true, &[a, b]), Some((7, [0.0, 0.0, 0.0])));
        assert_eq!(r.answer(b, false, &[a, b]), None, "it was no call for repairs");
        assert_eq!(r.answer((3, [80.0, 0.0, 0.0]), true, &[]), None, "too far");
        assert_eq!(r.answer((7, [0.0, 0.0, 0.0]), true, &[]), None, "not its own call");
    }

    #[test]
    fn it_does_not_spam() {
        let mut r = Radio::default();
        assert!(r.may(r::LOCAL_MEDIC, 0.0));
        r.said.push((r::LOCAL_MEDIC, 0.0));
        assert!(!r.may(r::ARMOR_SPOTTED, 5.0), "any message, once every {ANY_MSG_S} s");
        assert!(r.may(r::ARMOR_SPOTTED, 9.0));
        assert!(!r.may(r::LOCAL_MEDIC, 20.0), "the same, once every {SAME_MSG_S} s");
        r.said.push((r::ARMOR_SPOTTED, 9.0));
        r.said.push((r::LOCAL_GET_IN, 40.0));
        assert!(!r.may(r::ATTACK, 100.0), "{MAX_PER_LIFE} a life");
    }
}
