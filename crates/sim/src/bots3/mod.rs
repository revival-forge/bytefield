//! bots3: team bots, designed from what a player does.
//!
//! Three layers over the body (the hand-and-foot skills, [`body`]):
//! - [`team`]: what a whole side decides: which flag each one goes to, with whom, in which vehicle, with which kit;
//! - [`squad`]: three to five players with a leader who go together;
//! - [`individual`]: each player chooses at each moment what to do (fight, take cover, heal, board a vehicle...) according to what it sees
//!   and what it has been ordered, with a difficulty ([`profile`]).
//!
//! v0: the layers exist, with their contracts, but decide nothing; the individual plays exactly like `bots2`
//! (its body is the same), so the same seed gives the same round.
pub mod body;
pub mod team;
pub mod squad;
pub mod trip;
pub mod individual;
pub mod mods;
pub mod profile;
pub mod radio;

use crate::bot_api::{BotView, DeadView, Mind, MindMaker, VehicleClass};
use body::{Maps, MapProfile};
use team::{Team, Role, Parts, Kind, TeamView};
use squad::Levers;
use individual::{Settings, Ensemble, Individual, Command};
use profile::Profile;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A bots3 bot: its side's shared head and its own individual.
pub struct Bots3 {
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

    fn clear(&self, x: f32, z: f32, up: f32) -> bool {
        let Some(g) = self.v.ground(x, z) else { return false };
        // (the terrain test of `hidden` counts what is within 0.2 m of the ground)
        !self.v.hidden([x, g + up, z], [x, g + 0.4, z])
    }
}

impl Mind for Bots3 {
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
        let command = {
            let team = self.team.clone();
            let mut e = team.lock().unwrap_or_else(|e| e.into_inner());
            e.report(v, self.individual.target().map(|b| (b.0, b.1)));
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
            self.heed(v, &mut e);
            if e.due(v.time) && e.split_due(v.time) {
                e.think_on(v.time, &TeamView::de(v), &MapLand { v, maps: &self.maps });
            }
            let order = e.order(v.me.player);
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
            Command::new(order, e.urgent(v.me.player), outside, e.accompanied(v.me.player)).with_leg_vehicle(e.leg_vehicle(v.me.player)).with_wing_vehicle(e.wing_vehicle(v.me.player)).with_leg_landing(e.leg_landing(v.me.player)).with_leg_drop(e.leg_drop(v.me.player)).with_calls(calls.0, calls.1).with_person_call(calls.0.is_some_and(|j| !e.is_bot(j))).with_trades(medics, engineers).with_dangers(e.dangers(v.time)).with_reserved(e.reserved(v.me.player)).with_escort(v.me.inside.as_ref().filter(|i| i.seat == 0 && matches!(i.class, VehicleClass::Tank | VehicleClass::Walker)).and_then(|i| e.escort(v.me.player, i.pos, &v.others()))).with_air_engage(air.and_then(|a| a.0), air.map_or([usize::MAX; 6], |a| a.1)).with_air_pass(pass.map(|p| p.1)).with_air_target(v.me.inside.as_ref().filter(|i| i.seat == 0 && i.class.flies()).and_then(|i| e.air_target2(v.time, i.pos, |p| v.ground(p[0], p[2]).is_some_and(|g| g > v.water() && p[1] - g < 8.0))))
        };
        self.individual.think(v, &command)
    }

    fn choose_spawn(&mut self, d: &DeadView) -> Option<(Option<i64>, usize)> {
        self.team.lock().unwrap_or_else(|e| e.into_inner()).spawning(d)
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

impl Bots3 {
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
        radio::Causes { waiting, spotted, point, no_medic: false, answering: None }
    }
}

/// The bots of a side on `map`.
pub fn maker(map: &str, seed: u64) -> MindMaker {
    maker_with(map, seed, Arc::new(Maps::default()))
}

/// The same, with the path maps already built ([`Maps::prepare`]). Difficulty, from `BYTEFIELD_BOTS3_PROFILE`
/// (`rookie`, `normal`, `veteran`; normal by default): this is how `--bot-ai bots3` picks it in the client and the server.
pub fn maker_with(map: &str, seed: u64, maps: Arc<Maps>) -> MindMaker {
    let mut a = Settings::default();
    if let Some(p) = crate::debug::options().bots3_profile.as_deref().and_then(Profile::by_name) {
        a.profile = p;
    }
    maker_parts(map, seed, maps, Parts::default(), a, Vec::new())
}

/// A variant without some parts, to measure what each contributes: `no-assign` (each one chooses its flag, as bots2),
/// `no-squad` (assignment without squads or shared vehicles), `no-spawn` (spawns wherever it comes, with its kit), `no-heli`
/// (flies only with no way by land, not also when the flight is much faster; `with-heli` is the default), `no-reach` (also attacks the flags that cannot be taken on foot), `no-wing` (no pilots sent to the side's fighting aircraft), `no-pass` (no attack runs on a target given by the side), and of the
/// individual `no-line` (fires even if a wall hides the target), `no-weapon` (does not choose the weapon by target), `no-flank`
/// (enters where its mates have fallen), `no-escort` (the tank does not wait for its escort on foot), `no-hearing` (does not turn to the shots it hears), `no-fixed` (does not take a fixed gun for an enemy in reach), `no-drive` (drives as bots2: a car stops to turn and brakes for any mate ahead), `no-body` (from the air sees only an enemy's chest, as bots2); or with another difficulty: `rookie`, `normal`, `veteran`; or with a team-head lever
/// forced, `key=value` ([`Levers::force`]: `size=5`, `guard=0`, `targets=1`...), over whatever the map type calls for.
pub fn maker_variant(map: &str, seed: u64, sin: &[&str]) -> MindMaker {
    let mut p = Parts::default();
    let mut a = Settings::default();
    let mut forced = Vec::new();
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
            "air-task" => a.air_task = true,
            "" => {}
            other if other.contains('=') => {
                let (k, x) = other.split_once('=').unwrap();
                                let x: f32 = x.parse().unwrap_or_else(|_| panic!("bots3: {other}: the value is not a number"));
                                assert!(Levers::default().force(k, x), "bots3 has no lever {k}");
                forced.push((k.to_string(), x));
            }
            other => match Profile::by_name(other) {
                Some(x) => a.profile = x,
                                None => panic!("bots3 has no variant {other}"),
            },
        }
    }
    maker_parts(map, seed, Arc::new(Maps::default()), p, a, forced)
}

fn maker_parts(map: &str, seed: u64, maps: Arc<Maps>, parts: Parts, settings: Settings, forced: Vec<(String, f32)>) -> MindMaker {
    let profile = MapProfile::de(map);
    let name = map.to_string();
    let con_mods = parts.mods;
    let mut e = Team::con(parts);
    e.forced = forced;
    e.set_levers(Levers::default());
    let team = Arc::new(Mutex::new(e));
    Box::new(move |player| Box::new(Bots3 { pending_mod: con_mods && mods::is_mod_level(&name), team: team.clone(), individual: Individual::new(profile, maps.clone(), player, seed).with_settings(settings), maps: maps.clone(), radio: radio::Radio::default(), causes: Default::default(), spotted_at: [0.0; 3], answering: (None, None) }))
}

static BY_DEFAULT: AtomicBool = AtomicBool::new(false);

/// Makes `Sim::enable_bots` use bots3 instead of bots2. False in the library (tests and loose tools stay on
/// bots2); client, server and examples set it to true except with `--bot-ai bots2` / `--mind bots2`.
pub fn use_by_default(si: bool) {
    BY_DEFAULT.store(si, Ordering::Relaxed);
}

/// Whether bots3 was asked for by default.
pub fn is_default() -> bool {
    BY_DEFAULT.load(Ordering::Relaxed)
}

impl crate::Sim<'_> {
    /// The computer's players play with bots3, on both sides.
    pub fn enable_bots3(&mut self) {
        // (the path maps, now and not at the first bot that asks, as in `enable_bots`)
        let maps = Maps::prepare(self.world);
        let level = self.world.level.name.clone();
        for team in [1, 2] {
            self.set_minds(team, maker_with(&level, 0, maps.clone()));
        }
    }
}

/// `BYTEFIELD_BOTS3_MODS=1`: what player 0 sees on starting and the kind of map it yields (stderr, mod maps).
fn trace_mods() -> bool {
    crate::debug::options().bots3_mods
}

/// `BYTEFIELD_BOTS3_TRACE=1`: the team writes its squads and orders on every split (stderr).
fn trace_team() -> bool {
    crate::debug::options().bots3_team
}

/// `BYTEFIELD_BOTS3_ME=<player>`: that player writes its order, seat and axes twice a second (stderr).
fn trace_me() -> Option<usize> {
    crate::debug::options().bots3_me
}

/// `BYTEFIELD_BOTS3_ACTIONS=1`: each action change of each player, with what decided it (stderr).
fn trace_actions() -> bool {
    crate::debug::options().bots3_actions
}

/// `BYTEFIELD_BOTS3_WOUNDED=1`: each low-health player writes once a second its action and what it sees (stderr).
fn trace_wounded() -> bool {
    crate::debug::options().bots3_wounded
}

/// `BYTEFIELD_BOTS3_SHOT=1`: each tick in which a player on foot presses the trigger, its action, weapon, target and whether a wall hides the target
/// from the eye and from somewhat lower down (stderr; to cross-check with `dumb`'s `blocked`).
fn trace_shots() -> bool {
    crate::debug::options().bots3_shots
}

#[cfg(test)]
mod tests;
