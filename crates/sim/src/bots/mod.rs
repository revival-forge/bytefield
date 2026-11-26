//! bots: team bots, designed from what a player does.
//!
//! The AI is versioned ([`VERSION`], semantic): a major number changes with a change of foundation (a new navigation, say), the
//! minor one when an iteration of work closes, and the patch one with each change of behaviour. It goes into the log of a round
//! and into what the measuring tools write (`bots_version`), so two measurements can be told to come from different bots.
//!
//! Three layers over the body (the hand-and-foot skills, [`body`]):
//! - [`team`]: what a whole side decides: which flag each one goes to, with whom, in which vehicle, with which kit;
//! - [`squad`]: three to five players with a leader who go together;
//! - [`individual`]: each player chooses at each moment what to do (fight, take cover, heal, board a vehicle...) according to what it sees
//!   and what it has been ordered, with a difficulty ([`profile`]).
//!
//! v0: the layers exist, with their contracts, but decide nothing; the individual plays exactly like the plain bot
//! (its body is the same), so the same seed gives the same round.
/// The bots' version (see the module's docs).
pub const VERSION: &str = "1.24.0";

pub mod body;
pub mod flags;
pub mod life;
pub mod team;
pub mod squad;
pub mod trip;
pub mod individual;
pub mod intel;
pub mod memory;
pub mod mods;
pub mod profile;
pub mod radio;
pub mod tactics;
pub mod fire_trace;
pub mod spot_trace;
pub mod streetwise;
pub mod params;

use crate::bot_api::{BotView, DeadView, Mind, MindMaker, VehicleClass};
use body::{Maps, MapProfile};
use team::{Team, Role, Parts, Kind, TeamView};
use squad::Levers;
use individual::{Settings, Ensemble, Individual, Command};
use profile::Profile;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A bots bot: its side's shared head and its own individual.
pub struct Bots {
    team: Arc<Mutex<Team>>,
    individual: Individual,
    radio: radio::Radio,
    /// B15: what it could say this tick (worked out while thinking, said in `speak`), and where its enemy target is
    causes: radio::Causes,
    spotted_at: [f32; 3],
    /// B15: the call it answers now (for the trace)
    answering: (Option<usize>, Option<usize>),
    /// map of a mod: its profile must still be chosen from what is seen on starting
    pending_mod: bool,
    /// M2: why it stood still on purpose at its last think ([`Mind::intent`])
    waits: Option<crate::bot_api::Wait>,
    /// the round's path maps (the team asks them whether a flag is reached by land)
    maps: Arc<Maps>,
}

/// The map as the team looks at it for the squads' trips ([`trip::Land`]): the foot path map's zones and the water.
struct MapLand<'a, 'b, 'w> {
    v: &'a BotView<'b, 'w>,
    maps: &'a Maps,
}

impl trip::Land for MapLand<'_, '_, '_> {
    fn reach(&self, from: [f32; 3], to: [f32; 3], r: f32) -> trip::Reach {
        let Some(g) = self.maps.foot(self.v) else { return trip::Reach::Unknown };
        // (a pocket the map draws closed, a depot with a door or a bunker, says nothing: the walker finds the way out or in)
        let big = |z: &u32| *z != 0 && g.zone_size(*z) > body::walker::POCKET_CELLS;
        let Some(z) = g.zone_at(from[0], from[2]).filter(big) else { return trip::Reach::Unknown };
        let near: Vec<u32> = g.zones_near(to[0], to[2], r).into_iter().filter(big).collect();
        if near.is_empty() {
            trip::Reach::Unknown
        } else if near.contains(&z) {
            trip::Reach::Path
        } else {
            trip::Reach::NoPath
        }
    }

    fn wet(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        let l = ((b[0] - a[0]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        let n = (l / 4.0).ceil().max(1.0) as usize;
        let w = self.v.water();
        (1..n).any(|i| {
            let t = i as f32 / n as f32;
            self.v.ground(a[0] + (b[0] - a[0]) * t, a[2] + (b[2] - a[2]) * t).is_some_and(|h| h < w - 1.0)
        })
    }

    fn below(&self, x: f32, z: f32) -> f32 {
        self.v.ground(x, z).unwrap_or(f32::MIN).max(self.v.water())
    }

    fn water(&self) -> f32 {
        self.v.water()
    }

    fn way_m(&self, from: [f32; 3], to: [f32; 3], r: f32) -> Option<f32> {
        self.maps.way_m(self.v, [from[0], from[2]], [to[0], to[2]], r)
    }

    fn way_point(&self, from: [f32; 3], to: [f32; 3], r: f32, left_m: f32) -> Option<[f32; 2]> {
        self.maps.way_point(self.v, [from[0], from[2]], [to[0], to[2]], r, left_m)
    }

    fn clear(&self, x: f32, z: f32, up: f32) -> bool {
        let Some(g) = self.v.ground(x, z) else { return false };
        // (the terrain test of `hidden` counts what is within 0.2 m of the ground)
        !self.v.hidden([x, g + up, z], [x, g + 0.4, z])
    }
}

impl Mind for Bots {
    fn think(&mut self, v: &BotView) -> Vec<(usize, f32)> {
        if self.pending_mod {
            self.pending_mod = false;
            let r = mods::Summary::de(v);
            let c = mods::classify(&r);
            if trace_mods() {
                eprintln!("mods: {r:?} -> {c:?}");
            }
            self.individual.set_map_profile(mods::profile(c));
            self.team.lock().unwrap_or_else(|e| e.into_inner()).set_levers(mods::levers(c));
        }
        // B15: what it heard up to the last tick (what is said in this one enters the log once all have thought)
        self.radio.hear(v);
        // Each one reports what it sees; the first bot each tick that thinks updates the team when due (the order they think in is
        // fixed, so it is deterministic).
        let at_rally;
        let command = {
            let team = self.team.clone();
            let mut e = team.lock().unwrap_or_else(|e| e.into_inner());
            e.report(v, self.individual.target().map(|b| (b.0, b.1)));
            if self.individual.cover_flank_on() {
                let (at_spot, flank) = self.individual.cover_state(v);
                e.note_cover(v.me.player, v.time, at_spot, flank, v.me.pos);
            }
            // B26: a fighter pilot in flight tells the side which enemy aircraft it sees and which it has fixed on, and is told which to
            // go after and which its mates already are after
            let air = self.individual.flies_a_fighter(v).then(|| {
                let (seen, lock) = self.individual.air_picture(v.time);
                for (j, pos) in seen {
                    if pos[1] - v.ground(pos[0], pos[2]).unwrap_or(f32::MIN).max(v.water()) > team::FLYING_M {
                        e.air_seen(v.time, j, pos);
                    }
                }
                if let Some(j) = lock {
                    e.air_locked(v.time, v.me.player, j);
                }
                let taking: Vec<[f32; 3]> = v.flags().iter().filter(|f| f.team == v.me.team && f.contested).map(|f| f.pos).collect();
                (e.engage(v.time, v.me.player, v.me.pos, &taking), e.air_taken(v.me.player))
            });
            // B23: a pilot with bombs or rockets is given the target of its attack runs
            let pass = v.me.inside.as_ref().filter(|i| e.parts.pass && i.seat == 0 && i.class.flies() && body::pilot::Pilot::ordnance(i)).and_then(|i| {
                let taking: Vec<[f32; 3]> = v.flags().iter().filter(|f| f.team == v.me.team && f.contested).map(|f| f.pos).collect();
                e.air_pass(v.time, v.me.player, i.pos, &taking, |p| {
                    // (a ship's centre is well above the water: a carrier's, some 15 m)
                    let g = v.ground(p[0], p[2]);
                    let afloat = g.is_none_or(|g| g < v.water());
                    (p[1] - g.unwrap_or(f32::MIN).max(v.water()) < if afloat { team::PASS_SHIP_UP_M } else { 8.0 }).then_some(afloat)
                })
            });
            if v.me.inside.is_none() && e.stuck_on_foot(v.me.player, v.time) {
                self.individual.break_out(v);
            }
            e.casualty_report(v);
            if self.individual.streetwise_on() && self.individual.streetwise().is_none() {
                let x = e.traits.of(v.me.team, v.me.player);
                self.individual.set_streetwise(x, v.me.player);
            }
            self.heed(v, &mut e);
            if e.due(v.time) && e.split_due(v.time) {
                e.think_on(v.time, &TeamView::de(v), &MapLand { v, maps: &self.maps });
            }
            let order = e.order(v.me.player);
            at_rally = e.parts.regroup_way && v.me.inside.is_none() && e.waits_at_rally(v.me.player, v.me.pos);
            // at the wheel: the passengers it was given and that have not boarded yet
            let outside = match (&v.me.inside, order.role) {
                (Some(ins), Role::Drive(_) | Role::Leader) if ins.seat == 0 && !e.passengers(ins.vehicle).is_empty() => {
                    let others = v.others();
                    e.passengers(ins.vehicle).iter().filter(|&&j| others.iter().any(|o| o.player == j && !(o.in_vehicle && (0..3).all(|k| (o.pos[k] - ins.pos[k]).abs() < 0.01)))).count()
                }
                _ => 0,
            };
            // the medics and engineers alive on its side (what the icons show and the radio counts)
            let (mut medics, mut engineers) = (Ensemble::default(), Ensemble::default());
            if !v.me.inside.is_some() || v.me.inside.as_ref().is_some_and(|i| i.seat == 0) {
                for o in v.others().iter().filter(|o| o.team == v.me.team && !o.in_vehicle) {
                    match e.kind(o.player) {
                        Some(Kind::Medic) => medics.put(o.player),
                        Some(Kind::Engineer) => engineers.put(o.player),
                        _ => {}
                    }
                }
            }
            let calls = self.calls(v, &e);
            if radio::trace() && calls != self.answering {
                eprintln!("radio: t={:.1} player {} answers medic {:?} repairs {:?}", v.time, v.me.player, calls.0, calls.1);
            }
            self.answering = calls;
            self.causes = self.causes_now(v, &order, outside > 0 && !e.urgent(v.me.player));
            Command::new(order, e.urgent(v.me.player), outside, e.accompanied(v.me.player)).with_leg_vehicle(e.leg_vehicle(v.me.player)).with_wing_vehicle(e.wing_vehicle(v.me.player)).with_leg_landing(e.leg_landing(v.me.player)).with_leg_drop(e.leg_drop(v.me.player)).with_calls(calls.0, calls.1).with_person_call(calls.0.is_some_and(|j| !e.is_bot(j))).with_trades(medics, engineers).with_dangers(e.dangers(v.time)).with_flank(e.flank(v.me.player, v.time)).with_habit(if self.individual.streetwise().is_some() { self.habit(v, &e) } else { None }).with_mines_at(e.mines_at.filter(|_| self.individual.streetwise().is_some())).with_busy(if self.individual.streetwise().is_some() || self.individual.opportunist_all_on() { e.busy(v.me.player, v.time) } else { [usize::MAX; 8] }).with_cover(self.individual.cover_flank_on() && e.covered(v.me.player, v.time), if self.individual.cover_flank_on() { e.cover_for(v.me.player, v.time) } else { None }).with_enemy_vehicles(if self.individual.steal_flank_on() { e.enemy_vehicles(v.time) } else { [usize::MAX; 8] }).with_reserved(e.reserved(v.me.player)).with_escort(v.me.inside.as_ref().filter(|i| i.seat == 0 && matches!(i.class, VehicleClass::Tank | VehicleClass::Walker)).and_then(|i| e.escort(v.me.player, i.pos, &v.others()))).with_air_engage(air.and_then(|a| a.0), air.map_or([usize::MAX; 6], |a| a.1)).with_air_pass(pass.map(|p| p.1)).with_air_target(v.me.inside.as_ref().filter(|i| i.seat == 0 && i.class.flies()).and_then(|i| e.air_target2(v.time, i.pos, |p| v.ground(p[0], p[2]).is_some_and(|g| g > v.water() && p[1] - g < 8.0))))
        };
        let axes = self.individual.think(v, &command);
        self.waits = if at_rally { Some(crate::bot_api::Wait::Rally) } else { self.individual.waits() };
        axes
    }

    fn intent(&self) -> crate::bot_api::Intent {
        crate::bot_api::Intent { waits: self.waits }
    }

    fn choose_spawn(&mut self, d: &DeadView) -> Option<(Option<i64>, usize)> {
        let c = self.team.lock().unwrap_or_else(|e| e.into_inner()).spawning(d);
        // (the filmed player keeps the kit the take asked for, [`film_kit`]; the side's head picks where it spawns)
        match (c, filmed_kit().filter(|_| filmed() == Some(d.player))) {
            (Some((g, _)), Some(k)) => Some((g, k)),
            (c, _) => c,
        }
    }

    fn speak(&mut self, v: &BotView) -> Option<(crate::radio::Msg, Option<usize>)> {
        let mut c = self.causes;
        let team = self.team.clone();
        let mut e = team.lock().unwrap_or_else(|e| e.into_inner());
        // a sighting, once for the whole team
        if let Some(m) = c.spotted.and_then(radio::spotted_msg)
            && !e.spotting_new(m, self.spotted_at, v.time)
        {
            c.spotted = None;
        }
        c.answering = self.answering.0;
        if self.individual.dialogue_on() {
            c.st = self.streetwise_line(v, &e);
        }
        // hurt with no medic of its side on foot near: it does not call (nobody would come); it heads for a medic it sees instead
        if v.me.health > 0.0 && v.me.health < radio::HURT && v.me.inside.is_none() && e.kind(v.me.player) != Some(Kind::Medic) {
            let near = |p: [f32; 3]| ((p[0] - v.me.pos[0]).powi(2) + (p[2] - v.me.pos[2]).powi(2)).sqrt() < radio::DISPATCH_M;
            c.no_medic = !v.others().iter().any(|o| o.team == v.me.team && !o.in_vehicle && e.kind(o.player) == Some(Kind::Medic) && near(o.pos));
        }
        let said = self.radio.speak(v, &c);
        if let Some((m, _)) = said
            && Some(m) == c.spotted.and_then(radio::spotted_msg)
        {
            e.spotting_said(m, self.spotted_at, v.time);
        }
        if let Some((m, p)) = said.filter(|_| radio::trace()) {
            eprintln!("radio: t={:.1} player {} says {} {p:?} health {:.2}", v.time, v.me.player, m.key, v.me.health);
        }
        said
    }
}

impl Bots {
    /// B15: what the team does with what this bot heard: a person's sighting goes into its reports; a person's "get in" from the
    /// wheel puts the bots on foot nearby in the vehicle's free seats. And whether it rides the seat a "get in" gave it.
    fn heed(&mut self, v: &BotView, e: &mut Team) {
        let me = &v.me;
        let t = v.time;
        if e.get_in_seat(me.player).is_some() {
            let riding = me.inside.as_ref().is_some_and(|i| Some(i.vehicle) == e.get_in_seat(me.player).map(|s| s.0));
            e.get_in_riding(me.player, t, riding);
        }
        if self.radio.heard.is_empty() {
            return;
        }
        let heard: Vec<radio::Request> = self.radio.heard.clone();
        let others = std::cell::OnceCell::new();
        let others = || others.get_or_init(|| v.others());
        for h in &heard {
            match *h {
                radio::Request::Spotted { player, class, at, age } => {
                    if let Some(m) = radio::spotted_msg(class) {
                        e.spotting_said(m, at, t - age as f64);
                    }
                    if !e.is_bot(player) {
                        e.heard_spotted(player, at, t - age as f64);
                    }
                }
                // B25: a person who calls "medic!": the team sends the nearest medic on foot within DISPATCH_M of the caller that is not
                // busy, hurt or under fire. Not for a bot's call: between bots it cost reactions to fire (B14, 86 -> 79 %) and bought
                // nothing (the callers that live 20 s were already attended 93 % of the time; most callers die before)
                radio::Request::Medic { player, .. } if !e.is_bot(player) => {
                    let Some(caller) = others().iter().find(|o| o.player == player && o.team == me.team && !o.in_vehicle) else { continue };
                    if !caller.health.is_some_and(|h| h < radio::HURT + 0.3) {
                        continue;
                    }
                    let flat = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
                    let enemies: Vec<[f32; 3]> = others().iter().filter(|o| o.team != me.team && o.team != 0 && !v.hidden(me.eye, [o.pos[0], o.pos[1] + 0.3, o.pos[2]])).map(|o| o.pos).collect();
                    let mut cand: Vec<(f32, usize)> = others()
                        .iter()
                        .filter(|o| o.team == me.team && !o.in_vehicle && e.kind(o.player) == Some(Kind::Medic) && o.health.is_none_or(|h| h >= radio::MEDIC_MIN_HP))
                        .map(|o| (o.player, o.pos))
                        .chain((me.inside.is_none() && e.kind(me.player) == Some(Kind::Medic) && me.health >= radio::MEDIC_MIN_HP).then_some((me.player, me.pos)))
                        .filter(|&(j, pos)| flat(pos, caller.pos) < radio::DISPATCH_M && !e.urgent(j) && !enemies.iter().any(|&q| flat(q, pos) < radio::UNDER_FIRE_M))
                        .map(|(j, pos)| (flat(pos, caller.pos), j))
                        .collect();
                    cand.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    let cand: Vec<usize> = cand.into_iter().map(|c| c.1).collect();
                    if e.ask_medic(t, player, &cand) && radio::trace() {
                        eprintln!("radio: t={t:.1} player {player} asked for a medic: {:?}", cand.first());
                    }
                }
                radio::Request::GetIn { player, .. } if !e.is_bot(player) => {
                    let Some(k) = others().iter().find(|o| o.player == player && o.team == me.team).and_then(|o| o.vehicle) else { continue };
                    let Some(x) = v.vehicle(k) else { continue };
                    let seats: Vec<usize> = x.free_seats.iter().copied().filter(|&s| s != 0).collect();
                    if seats.is_empty() {
                        continue;
                    }
                    let near = |p: [f32; 3]| ((p[0] - x.pos[0]).powi(2) + (p[2] - x.pos[2]).powi(2)).sqrt();
                    let free = |j: usize| !e.urgent(j) && !matches!(e.order(j).role, team::Role::Drive(_) | team::Role::Passenger(..) | team::Role::Free);
                    let mut cand: Vec<(f32, usize)> = others().iter().filter(|o| o.team == me.team && !o.in_vehicle && e.is_bot(o.player)).map(|o| (near(o.pos), o.player)).collect();
                    if me.inside.is_none() {
                        cand.push((near(me.pos), me.player));
                    }
                    cand.retain(|c| c.0 < radio::GET_IN_M && free(c.1));
                    cand.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    let cand: Vec<usize> = cand.into_iter().map(|c| c.1).collect();
                    if e.ask_get_in(t, k, &seats, &cand) && radio::trace() {
                        eprintln!("radio: t={t:.1} player {player} asked to get in {k}: {:?}", e.get_in_seat_all(k));
                    }
                }
                _ => {}
            }
        }
    }

    /// B15: the calls it answers: as a medic, the "medic!" no other medic of its team is nearer to; as an engineer, the "need repairs"
    /// (the vehicle the caller rides).
    fn calls(&self, v: &BotView, e: &Team) -> (Option<usize>, Option<usize>) {
        let me = &v.me;
        let kind = team::Kind::de(&me.items);
        // (a medic the team sent to a caller goes even if it did not hear the call)
        let sent = if kind == Kind::Medic { e.medic_call(me.player, v.time) } else { None };
        if me.inside.is_some() || (sent.is_none() && !self.radio.heard.iter().any(|h| matches!(h, radio::Request::Medic { .. } | radio::Request::Repairs { .. }))) {
            return (None, None);
        }
        let others = v.others();
        let helpers = |r: Kind| -> Vec<(usize, [f32; 3])> { others.iter().filter(|o| o.team == me.team && !o.in_vehicle && e.kind(o.player) == Some(r)).map(|o| (o.player, o.pos)).chain([(me.player, me.pos)]).collect() };
        match kind {
            Kind::Medic => (self.radio.answer((me.player, me.pos), true, &helpers(Kind::Medic)).map(|c| c.0).or(sent), None),
            Kind::Engineer => {
                let j = self.radio.answer((me.player, me.pos), false, &helpers(Kind::Engineer)).map(|c| c.0);
                (None, j.and_then(|j| others.iter().find(|o| o.player == j && o.team == me.team).and_then(|o| o.vehicle)))
            }
            _ => (None, None),
        }
    }

    /// B15: what it could say now: at the wheel waiting for passengers, its enemy target in a vehicle near, a leader near its flag.
    fn causes_now(&mut self, v: &BotView, order: &team::Order, waiting: bool) -> radio::Causes {
        let me = &v.me;
        let wheel = me.inside.as_ref().filter(|i| i.seat == 0);
        let waiting = waiting && wheel.and_then(|i| v.vehicle(i.vehicle)).is_some_and(|x| x.free_seats.iter().any(|&s| s != 0));
        let spotted = self.individual.target().filter(|b| b.2 < radio::SPOT_M).and_then(|b| {
            self.spotted_at = b.1;
            v.others().into_iter().find(|o| o.player == b.0).and_then(|o| o.vehicle).and_then(|k| v.vehicle(k)).map(|x| x.class)
        });
        let point = match (order.role, order.goal) {
            (team::Role::Leader, team::Goal::Flag(i)) => v.flags().get(i).filter(|f| ((f.pos[0] - me.pos[0]).powi(2) + (f.pos[2] - me.pos[2]).powi(2)).sqrt() < radio::POINT_M).map(|f| (i, f.team != me.team)),
            _ => None,
        };
        radio::Causes { waiting, spotted, point, no_medic: false, answering: None, st: None }
    }

    /// `bots:streetwise`: the cell where an enemy on foot passes again and again; with `bots:ambush-own`, only one nearer the side's
    /// flags than the enemy's.
    fn habit(&self, v: &BotView, e: &Team) -> Option<[f32; 3]> {
        if !self.individual.ambush_own_on() {
            return e.intel.habit(v.time).map(|h| h.0);
        }
        let flags = v.flags();
        let me = v.me.team;
        let near = |p: [f32; 3], own: bool| flags.iter().filter(|f| if own { f.team == me } else { f.team != 0 && f.team != me }).map(|f| (f.pos[0] - p[0]).hypot(f.pos[2] - p[2])).fold(f32::MAX, f32::min);
        e.intel.habit_where(v.time, |p| near(p, true) < near(p, false)).map(|h| h.0)
    }

    /// `bots:streetwise`: the line of an exchange it could say now (the first that holds): answer a mate's "cover me", "cover me" as it
    /// sets off to flank, "check your six" to a mate, "reinforcements" as the last of its squad.
    fn streetwise_line(&mut self, v: &BotView, e: &Team) -> Option<radio::St> {
        // (what it does that a mate should know: about to mine, going to steal, at the wheel of what it stole, firing with a mate)
        if let Some(x) = self.individual.act_line(v, &e.busy(v.me.player, v.time)) {
            return Some(x);
        }
        let me = &v.me;
        let flat = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
        let started = self.individual.flank_started();
        let others = v.others();
        let mates: Vec<&crate::bot_api::Other> = others.iter().filter(|o| o.team == me.team).collect();
        // a "cover me" of a mate within voice: the nearest of its mates on foot answers
        if me.inside.is_none() {
            let heard = v.radio(radio::COVER_ANSWER_S);
            if let Some(h) = heard.iter().find(|h| h.msg == crate::radio::LOCAL_COVER_ME && h.team == me.team && h.player != me.player && flat(h.at, me.pos) <= crate::bot_api::VOICE_M) {
                let mine = flat(h.at, me.pos);
                if !mates.iter().any(|o| o.player != h.player && !o.in_vehicle && flat(o.pos, h.at) < mine) {
                    return Some(radio::St::Roger(h.player));
                }
            }
        }
        if started && mates.iter().any(|o| flat(o.pos, me.pos) <= crate::bot_api::VOICE_M) {
            return Some(radio::St::CoverMe);
        }
        if let Some(b) = self.individual.target()
            && mates.iter().any(|o| !o.in_vehicle && flat(o.pos, b.1) < radio::SIX_M && flat(o.pos, me.pos) > radio::SIX_FROM_M && flat(o.pos, me.pos) <= crate::bot_api::VOICE_M && flat(o.pos, b.1) < flat(me.pos, b.1))
        {
            return Some(radio::St::CheckSix);
        }
        let sq = e.squads.iter().find(|q| q.leader == me.player || q.members.contains(&me.player))?;
        let alive = |k: usize| k == me.player || mates.iter().any(|o| o.player == k);
        (sq.members.len() + 1 >= 4 && std::iter::once(sq.leader).chain(sq.members.iter().copied()).filter(|&k| alive(k)).count() == 1).then_some(radio::St::Reinforcement)
    }
}

/// The bots of a side on `map`.
pub fn maker(map: &str, seed: u64) -> MindMaker {
    maker_with(map, seed, Arc::new(Maps::default()))
}

/// The same, with the path maps already built ([`Maps::prepare`]). Difficulty, from `BYTEFIELD_BOTS_PROFILE`
/// (`rookie`, `normal`, `veteran`; normal by default): this is how `--bot-ai bots` picks it in the client and the server.
pub fn maker_with(map: &str, seed: u64, maps: Arc<Maps>) -> MindMaker {
    let mut a = Settings { params: params::BotParams::layered(map, &[]), ..Settings::default() };
    if let Some(p) = crate::debug::options().bots_profile.as_deref().and_then(Profile::by_name) {
        a.profile = p;
    }
    maker_parts(map, seed, maps, Parts::default(), a, Vec::new())
}

/// A variant without some parts, to measure what each contributes: `no-assign` (each one chooses its flag, as on its own),
/// `no-squad` (assignment without squads or shared vehicles), `no-spawn` (spawns wherever it comes, with its kit), `no-heli`
/// (flies only with no way by land, not also when the flight is much faster; `with-heli` is the default), `no-reach` (also attacks the flags that cannot be taken on foot), `no-wing` (no pilots sent to the side's fighting aircraft), `no-pass` (no attack runs on a target given by the side), and of the
/// individual `no-line` (fires even if a wall hides the target), `no-weapon` (does not choose the weapon by target), `no-flank`
/// (enters where its mates have fallen), `no-escort` (the tank does not wait for its escort on foot), `no-hearing` (does not turn to the shots it hears), `no-fixed` (does not take a fixed gun for an enemy in reach), `no-drive` (drives as the plain bot: a car stops to turn and brakes for any mate ahead), `no-body` (from the air sees only an enemy's chest, as on its own), `no-prone-fight` (does not lie down to fight
/// an enemy far, as up to 1.18.0, [`life::FightTweaks`]); or with another difficulty: `rookie`, `normal`, `veteran`; or with a team-head lever
/// forced, `key=value` ([`Levers::force`]: `size=5`, `guard=0`, `targets=1`...), over whatever the map type calls for; or a named
/// parameter, `key=value` too ([`params::BotParams`]: `rally_m=140`), over the layers. Experimental, off by
/// default: `cover-fight` (goes to a firing spot a few metres away before stopping to fight, [`tactics`]), `cover-points` (its
/// position queries take the level's cover points first, filtered by the sectors they hide, [`crate::cover`]), with `cover-fight`:
/// `cover-near` (no firing spot farther than a few metres, nor when already hidden where it stands), `cover-keep` (the spot is not
/// dropped for a flickering target or the coin of stopping), `cover-path` (no spot whose straight way is walled) ([`life::FightTweaks`]), `strength` (the attack
/// by what the side knows of the enemy, [`team::Parts::strength`]), `flank2` (one flanker per squad in contact, round the side the
/// enemy does not face, [`team::Parts::flank2`]), `hunt` (with no flag that changes hands, where the enemy passes,
/// [`team::Parts::hunt`]), `escort2` (on foot, behind its leader's tank), `streetwise` (one bot in four with the streetwise trait, [`streetwise`]), `dialogue` (with `streetwise`: two-line exchanges on the
/// radio about what they do, [`radio::St`]); with `streetwise` too, the levers that did not pass alone, to be tried together:
/// `opportunist`, `steal`, `ambush`, `traffic-mines`; `regroup` (the squads wait short of an enemy flag and go in together,
/// [`team::Team::regroup`]), `regroup-way` (`regroup` with each squad's point on its own way and all its people held, R1,
/// [`team::Parts::regroup_way`]), `flank-shuffle` (the flank side drawn without streaks for all, [`Settings::flank_shuffle`]),
/// `opportunist-all` (every bot weighs how vulnerable an enemy is, [`Settings::opportunist_all`]), `steal-flank` (with
/// `streetwise`: only the flanker steals, and only what its side saw an enemy drive, [`Settings::steal_flank`]),
/// `traffic-mines-spread` (`traffic-mines` with the mines along the traffic, [`Settings::traffic_spread`]), `ambush-own` (`ambush`
/// only on its own side of the map, [`Settings::ambush_own`]), `cover-flank` (covering fire for the squad's flanker, which waits for
/// it, [`Settings::cover_flank`]), `retreat-mate` (with `streetwise`: the high trait takes cover by a mate who watches its way,
/// [`Settings::retreat_mate`]). On by default since 1.20.0, how it fires on foot ([`body::shooter::FpsTweaks`]; `no-fps` turns the
/// five off, as up to 1.19.0, and `no-` before one's name turns that one off): `fire-rhythm` (longer bursts and shorter
/// pauses the nearer the enemy, a one-shot weapon at its own cycle), `friend-gate` (a mate blocks the line only close to it, and it
/// steps aside), `look-all` (its look tries more than the three nearest, and one with no mate in the line first), `quick-react` (a
/// reaction without the long tail; no grenade with a mate in the line or near), `fire-first` (walking, it stops for each burst), and since 1.21.0
/// `pistol-close` (the pistol only with the main gun dry and the enemy near, or to finish one point-blank with the magazine
/// empty; not by the default's coin), and since 1.22.0 `armour` (on foot, no rounds at an enemy inside armour that its weapons do
/// not hurt, by the `.con` damage table; grenades from close, and the gunner on a seat that shows him is a target), `close-fight`
/// (with a target near, no flight to get unstuck: it fights) and `grenade-throw` (it lets go of the fire key before going back to
/// the gun, so the grenade leaves), and since 1.23.0 `heard-walk` (turned to a shot heard with nobody in sight, it walks on while it
/// looks, rather than stand), and since 1.24.0 `fov` (on foot, a target only within its field of view, near it or the one it
/// follows) and `recall` (a target lost from sight is watched for a few seconds where he was). Off by default, to measure:
/// `tank-cover` (with no target, it hides from an enemy tank near that it cannot hurt) and `flight-aim` (a flight to get unstuck
/// keeps its aim and fire when it has a target) and `fire-ready` (`fire-first` stops only when it can fire: not reloading,
/// switching weapon or with nothing to squeeze).
pub fn maker_variant(map: &str, seed: u64, sin: &[&str]) -> MindMaker {
    let (p, a, forced) = parse_variant(sin, map);
    maker_parts(map, seed, Arc::new(Maps::default()), p, a, forced)
}

/// What a list of variants switches on `level`: the head's parts, the individual's settings (with the named parameters, layered with
/// those the list forces) and the levers forced. A `key=value` is a lever if there is one by that name, else a named parameter.
fn parse_variant(sin: &[&str], level: &str) -> (Parts, Settings, Vec<(String, f32)>) {
    let mut p = Parts::default();
    let mut a = Settings::default();
    let mut forced = Vec::new();
    let mut forced_params = Vec::new();
    for s in sin {
        match *s {
            "no-assign" => p.split = false,
            "no-squad" => p.squads = false,
            "no-spawn" => p.spawning = false,
            "with-heli" => p.heli = true,
            "no-heli" => p.heli = false,
            "no-mods" => p.mods = false,
            "no-reach" => p.reach = false,
            "no-wing" => p.wing = false,
            "no-pass" => p.pass = false,
            "strength" => p.strength = true,
            "hunt" => p.hunt = true,
            "escort2" => a.escort2 = true,
            "streetwise" => a.streetwise = true,
            "dialogue" => a.dialogue = true,
            "opportunist" => a.opportunist = true,
            "opportunist-all" => a.opportunist_all = true,
            "steal" => a.steal = true,
            "steal-flank" => {
                a.steal = true;
                a.steal_flank = true;
            }
            "ambush" => a.ambush = true,
            "ambush-own" => {
                a.ambush = true;
                a.ambush_own = true;
            }
            "traffic-mines" => p.traffic_mines = true,
            "traffic-mines-spread" => {
                p.traffic_mines = true;
                a.traffic_spread = true;
            }
            "regroup" => p.regroup = true,
            "regroup-way" => {
                p.regroup = true;
                p.regroup_way = true;
            }
            "cover-flank" => a.cover_flank = true,
            "retreat-mate" => a.retreat_mate = true,
            "flank-shuffle" => a.flank_shuffle = true,
            "flank2" => {
                p.flank2 = true;
                a.flank2 = true;
            }
            "no-line" => a.line = false,
            "no-weapon" => a.melee_weapon = false,
            "no-flank" => a.flanking = false,
            "no-escort" => a.escort = false,
            "no-hearing" => a.heard = false,
            "no-fixed" => a.fixed = false,
            "no-drive" => a.gear = false,
            "no-body" => a.body = false,
            "no-dogfight" => a.dogfight = false,
            "no-vtol" => a.vtol = false,
            "no-swim" => a.swim = false,
            "no-noway" => a.no_way = false,
            "no-jam" => a.jam = false,
            "no-sight" => a.sight = false,
            "air-task" => a.air_task = true,
            "cover-fight" => a.cover_fight = true,
            "cover-points" => a.cover_points = true,
            "memory" => p.memory = true,
            "cover-near" => a.fight_tweaks.near = true,
            "cover-keep" => a.fight_tweaks.keep = true,
            "cover-path" => a.fight_tweaks.path = true,
            "prone-fight" => a.fight_tweaks.prone = true,
            "no-prone-fight" => a.fight_tweaks.prone = false,
            "fire-rhythm" => a.fps.rhythm = true,
            "friend-gate" => a.fps.friend = true,
            "look-all" => a.fps.look = true,
            "quick-react" => a.fps.react = true,
            "fire-first" => a.fps.stand = true,
            "pistol-close" => a.fps.pistol = true,
            "no-pistol-close" => a.fps.pistol = false,
            "armour" => a.fps.armour = true,
            "no-armour" => a.fps.armour = false,
            "close-fight" => a.fps.close = true,
            "no-close-fight" => a.fps.close = false,
            "grenade-throw" => a.fps.throw = true,
            "no-grenade-throw" => a.fps.throw = false,
            "fov" => a.fps.fov = true,
            "no-fov" => a.fps.fov = false,
            "recall" => a.fps.recall = true,
            "no-recall" => a.fps.recall = false,
            "heard-walk" => a.fps.heard_walk = true,
            "no-heard-walk" => a.fps.heard_walk = false,
            "tank-cover" => a.fps.tank_cover = true,
            "flight-aim" => a.fps.flight_aim = true,
            "fire-ready" => a.fps.ready = true,
            "no-fire-ready" => a.fps.ready = false,
            "no-fps" => a.fps = body::shooter::FpsTweaks::OFF,
            "no-fire-rhythm" => a.fps.rhythm = false,
            "no-friend-gate" => a.fps.friend = false,
            "no-look-all" => a.fps.look = false,
            "no-quick-react" => a.fps.react = false,
            "no-fire-first" => a.fps.stand = false,
            "" => {}
            other if other.contains('=') => {
                let (k, x) = other.split_once('=').unwrap();
                let x: f64 = x.parse().unwrap_or_else(|_| panic!("bots: {other}: the value is not a number"));
                if Levers::default().force(k, x as f32) {
                    forced.push((k.to_string(), x as f32));
                } else {
                    assert!(params::spec(k).is_some(), "bots has no lever or parameter {k}");
                    forced_params.push((k.to_string(), x));
                }
            }
            other => match Profile::by_name(other) {
                Some(x) => a.profile = x,
                None => panic!("bots has no variant {other}"),
            },
        }
    }
    a.params = params::BotParams::layered(level, &forced_params);
    // (a few bytes per mind made, and only while tracing)
    if fire_trace::on() {
        a.label = Box::leak(format!("bots:{}", sin.join("+")).into_boxed_str());
    }
    (p, a, forced)
}

/// The variants after `bots:` in a mind's name (none for `bots`).
fn variants_of(name: &str) -> Vec<&str> {
    name.split_once(':').map_or(Vec::new(), |(_, v)| v.split('+').collect())
}

/// One side split in two inside the same head (the A/B test within a side, `tournament3 --split`): the players `in_b` says play with
/// `name_b`'s settings (and their own draws from `seed_b`), the others with `name_a`'s. Only for what each bot does on its own: the two
/// names must leave the head the same (parts and levers), or this is `Err` with what differs.
pub fn mind_split(name_a: &str, name_b: &str, level: &str, seed: u64, seed_b: u64, in_b: Arc<dyn Fn(usize) -> bool + Send + Sync>) -> Result<MindMaker, String> {
    let (pa, sa, fa) = parse_variant(&variants_of(name_a), level);
    let (pb, sb, fb) = parse_variant(&variants_of(name_b), level);
    if pa != pb || fa != fb || !sa.params.same_for_team(&sb.params) {
        return Err(format!("{name_a} and {name_b} differ in the side's head (parts {pa:?} against {pb:?}, levers {fa:?} against {fb:?}, or a parameter the head reads): they are measured side against side"));
    }
    let settings: Arc<dyn Fn(usize) -> (Settings, u64) + Send + Sync> = Arc::new(move |player| if in_b(player) { (sb, seed_b) } else { (sa, seed) });
    Ok(maker_split_parts(level, Arc::new(Maps::default()), pa, settings, fa))
}

fn maker_parts(map: &str, seed: u64, maps: Arc<Maps>, parts: Parts, settings: Settings, forced: Vec<(String, f32)>) -> MindMaker {
    maker_split_parts(map, maps, parts, Arc::new(move |_| (settings, seed)), forced)
}

/// A side's head, with its parts, the levers its name forces and its named parameters.
fn head(parts: Parts, forced: Vec<(String, f32)>, params: params::BotParams) -> Team {
    let mut e = Team::con(parts);
    e.forced = forced;
    e.params = params;
    e.set_levers(Levers::default());
    e
}

/// The head a `bots:`-style name makes (for tests).
#[cfg(test)]
pub(crate) fn head_of(name: &str, level: &str) -> Team {
    let (p, a, forced) = parse_variant(&variants_of(name), level);
    head(p, forced, a.params)
}

/// The maker, with each player's settings and seed.
fn maker_split_parts(map: &str, maps: Arc<Maps>, parts: Parts, settings: Arc<dyn Fn(usize) -> (Settings, u64) + Send + Sync>, forced: Vec<(String, f32)>) -> MindMaker {
    let profile = MapProfile::de(map);
    let name = map.to_string();
    let con_mods = parts.mods;
    // (the head reads the side's parameters: with `--split` both halves have the same ones of scope `Team`, [`mind_split`])
    if parts.memory {
        maps.want_memory(settings(0).0.params.memory_death_m);
    }
    let team = Arc::new(Mutex::new(head(parts, forced, settings(0).0.params)));
    Box::new(move |player| {
        let (settings, seed) = settings(player);
        Box::new(Bots { pending_mod: con_mods && mods::is_mod_level(&name), team: team.clone(), individual: Individual::new(profile, maps.clone(), player, seed).with_settings(settings), maps: maps.clone(), radio: radio::Radio::default(), causes: Default::default(), spotted_at: [0.0; 3], answering: (None, None), waits: None })
    })
}

static BY_DEFAULT: AtomicBool = AtomicBool::new(false);
static FILMED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// The player whose bot a camera follows (the client's `BYTEFIELD_PLAYER_BOT`, to film a round): at the wheel it sets off at once,
/// without waiting for its passengers or its escort on foot (a take of a tank standing 20 s at the spawn is lost).
pub fn film(player: Option<usize>) {
    FILMED.store(player.unwrap_or(usize::MAX), Ordering::Relaxed);
}

fn filmed() -> Option<usize> {
    Some(FILMED.load(Ordering::Relaxed)).filter(|&p| p != usize::MAX)
}

static FILMED_KIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// The kit the filmed player spawns with (the client's `--kit` with `BYTEFIELD_PLAYER_BOT`), over the one its side's head would give
/// it at each spawn; `None`: the head's.
pub fn film_kit(kit: Option<usize>) {
    FILMED_KIT.store(kit.unwrap_or(usize::MAX), Ordering::Relaxed);
}

fn filmed_kit() -> Option<usize> {
    Some(FILMED_KIT.load(Ordering::Relaxed)).filter(|&k| k != usize::MAX)
}

/// Makes `Sim::enable_bots` use bots instead of the plain bot. False in the library (tests and loose tools stay on
/// the plain bot); client, server and examples set it to true except when `--bot-ai` asks for another AI.
pub fn use_by_default(si: bool) {
    BY_DEFAULT.store(si, Ordering::Relaxed);
}

/// Whether `--bot-ai`'s value names these bots (`bots`, or the older `bots3`).
pub fn is_name(v: &str) -> bool {
    v == "bots" || v == "bots3" || v.starts_with("bots:") || v.starts_with("bots3:")
}

/// The bots a `--mind`-style name asks for (see [`is_name`]): `bots`, or `bots:` and variants joined with `+` (`bots:no-squad+rookie`).
pub fn mind(name: &str, level: &str, seed: u64) -> MindMaker {
    match name.split_once(':') {
        Some((_, variants)) => maker_variant(level, seed, &variants.split('+').collect::<Vec<_>>()),
        None => maker(level, seed),
    }
}

/// Whether bots was asked for by default.
pub fn is_default() -> bool {
    BY_DEFAULT.load(Ordering::Relaxed)
}

impl crate::Sim<'_> {
    /// The computer's players play with bots, on both sides.
    pub fn enable_team_bots(&mut self) {
        static SAID: AtomicBool = AtomicBool::new(false);
        if !SAID.swap(true, Ordering::Relaxed) {
            eprintln!("bots {VERSION}: team bots on both sides");
        }
        // (the path maps, now and not at the first bot that asks, as in `enable_bots`)
        let maps = Maps::prepare(self.world);
        let level = self.world.level.name.clone();
        for team in [1, 2] {
            self.set_minds(team, maker_with(&level, 0, maps.clone()));
        }
    }
}

/// `BYTEFIELD_BOTS_MODS=1`: what player 0 sees on starting and the kind of map it yields (stderr, mod maps).
fn trace_mods() -> bool {
    crate::debug::options().bots_mods
}

/// `BYTEFIELD_BOTS_TRACE=1`: the team writes its squads and orders on every split (stderr).
fn trace_team() -> bool {
    crate::debug::options().bots_team
}

/// `BYTEFIELD_BOTS_ME=<player>`: that player writes its order, seat and axes twice a second (stderr).
fn trace_me() -> Option<usize> {
    crate::debug::options().bots_me
}

/// `BYTEFIELD_BOTS_ACTIONS=1`: each action change of each player, with what decided it (stderr).
fn trace_actions() -> bool {
    crate::debug::options().bots_actions
}

/// `BYTEFIELD_BOTS_WOUNDED=1`: each low-health player writes once a second its action and what it sees (stderr).
fn trace_wounded() -> bool {
    crate::debug::options().bots_wounded
}

/// `BYTEFIELD_BOTS_SHOT=1`: each tick in which a player on foot presses the trigger, its action, weapon, target and whether a wall hides the target
/// from the eye and from somewhat lower down (stderr; to cross-check with `dumb`'s `blocked`).
fn trace_shots() -> bool {
    crate::debug::options().bots_shots
}

#[cfg(test)]
mod tests;
