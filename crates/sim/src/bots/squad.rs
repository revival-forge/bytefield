//! The squad: three to five players with a leader, like those who team up in a game to go to a flag. They go together (about
//! 20 m from the leader when there is no combat), wait a few seconds for whoever has not boarded before setting off with a vehicle, spawn
//! near their own and, if the leader falls, another takes over.
//!
//! How they are formed: at the start, those who are together, four by four; whoever arrives later (or respawns and
//! had none) joins the nearest squad that is not full. The squad lasts the whole round: its dead stay in it and
//! respawn with it. The team gives the whole squad a goal; the leader goes to it and the rest follow. If
//! the flag is far and there is a ground vehicle with seats beside the leader, it is filled: the leader at the wheel and the rest as
//! passengers; if the leader already drives, its own board with it.
use super::team::{Goal, Order, Role, VehicleSighting, TeamView};
use super::trip::{Failed, Trip};
use std::collections::{BTreeMap, BTreeSet};

/// A few players with a leader and a common goal.
#[derive(Clone, Debug, PartialEq)]
pub struct Squad {
    pub leader: usize,
    /// the other players (without the leader), alive or waiting to respawn
    pub members: Vec<usize>,
    pub goal: Goal,
    /// the vehicle it has been given
    pub vehicle: Option<usize>,
    /// the plans of its vehicles
    pub planes: Vec<Plan>,
    /// how it gets to its goal ([`super::trip`]); `None` when its goal is not a flag
    pub trip: Option<Trip>,
    /// the ways to a flag it tried and failed, not tried again for a while
    pub failed: Vec<Failed>,
    /// the flags it found no way to: the split gives it another until the instant
    pub avoid: Vec<(usize, f64)>,
}

impl Squad {
    pub fn new(leader: usize, members: Vec<usize>, goal: Goal) -> Squad {
        Squad { leader, members, goal, vehicle: None, planes: Vec::new(), trip: None, failed: Vec::new(), avoid: Vec::new() }
    }

    /// The vehicle of the leg it is on, if it is a vehicle leg (a boat, an aircraft) whose people have not got off yet.
    pub fn leg_vehicle(&self) -> Option<usize> {
        self.trip.as_ref().and_then(|tr| tr.now().mode.vehicle())
    }

    /// Its own that are alive, the leader first.
    pub fn alive(&self, view: &TeamView) -> Vec<usize> {
        self.all().filter(|&j| view.mates.iter().any(|c| c.player == j)).collect()
    }

    /// All its own, the leader first.
    pub fn all(&self) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(self.leader).chain(self.members.iter().copied())
    }
}

/// Size of a squad when formed (those who join later do not go over this).
pub const SIZE: usize = 4;
/// Whoever is closer than this joins the leader, m.
pub const TOGETHER_M: f32 = 80.0;
/// A vehicle is taken to go to a flag farther than this, m...
pub const VEHICLE_IF_FAR: f32 = 150.0;
/// A ground vehicle is taken if it is closer than this to the leader, m (a transport helicopter is not a squad vehicle: the trip
/// takes it, as an air leg, [`super::trip::choose`]).
pub const VEHICLE_NEAR: f32 = 50.0;
/// Whoever is on foot boards its leader's vehicle if closer than this, m.
pub const BOARD_NEAR: f32 = 60.0;

/// The head levers that depend on the map type ([`super::mods::levers`]): by default, the constants above
/// and the usual split. Each can be forced to measure it: `bots:size=5`, `bots:guard=0`...
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levers {
    /// size of a squad when formed ([`SIZE`])
    pub size: usize,
    /// a vehicle is taken to go to a flag farther than this, m ([`VEHICLE_IF_FAR`])
    pub vehicle_if_far: f32,
    /// … if it is closer than this to the leader, m ([`VEHICLE_NEAR`])
    pub vehicle_near: f32,
    /// the own front flag keeps a small unit even if nobody threatens it (step 3 of the split)
    pub guard: bool,
    /// how many flags it attacks at once at most (0: two, or three if there are plenty of people)
    pub targets: usize,
}

impl Default for Levers {
    fn default() -> Levers {
        Levers { size: SIZE, vehicle_if_far: VEHICLE_IF_FAR, vehicle_near: VEHICLE_NEAR, guard: true, targets: 0 }
    }
}

impl Levers {
    /// Forces a lever by its name (`size`, `vehicle_if_far`, `vehicle_near`, `guard`, `targets`); false if it does not exist.
    pub fn force(&mut self, name: &str, valor: f32) -> bool {
        match name {
            "size" => self.size = (valor as usize).max(1),
            "vehicle_if_far" => self.vehicle_if_far = valor,
            "vehicle_near" => self.vehicle_near = valor,
            "guard" => self.guard = valor != 0.0,
            "targets" => self.targets = valor as usize,
            _ => return false,
        }
        true
    }
}

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Takeover from a dead leader (the living one of its own nearest the goal) and those with no squad, one by one.
pub fn keep(squads: &mut Vec<Squad>, view: &TeamView, size: usize) {
    let pos = |j: usize| view.mates.iter().find(|c| c.player == j).map(|c| c.pos);
    let meta = |o: Goal| match o {
        Goal::Flag(i) => view.flags.get(i).map(|b| b.pos),
        Goal::Point(p) => Some([p[0], 0.0, p[1]]),
    };
    for e in squads.iter_mut() {
        if pos(e.leader).is_some() {
            continue;
        }
        let m = meta(e.goal);
        let far = |p: [f32; 3]| m.map_or(0.0, |m| flat(p, m));
        let new = e.members.iter().copied().filter_map(|j| pos(j).map(|p| (j, p))).min_by(|a, b| far(a.1).total_cmp(&far(b.1)).then(a.0.cmp(&b.0)));
        if let Some((j, _)) = new {
            e.members.retain(|&x| x != j);
            e.members.push(e.leader);
            e.leader = j;
            e.vehicle = None;
            e.planes.clear();
        }
    }
    let con: BTreeSet<usize> = squads.iter().flat_map(|e| e.all().collect::<Vec<_>>()).collect();
    let mut loose: Vec<(usize, [f32; 3])> = view.mates.iter().filter(|c| !con.contains(&c.player)).map(|c| (c.player, c.pos)).collect();
    while !loose.is_empty() {
        let (j, p) = loose.remove(0);
        // to the nearest squad that is not full (by its living leader)
        let near = |e: &Squad| pos(e.leader).map_or(f32::MAX, |q| flat(p, q));
        if let Some(e) = squads.iter_mut().filter(|e| e.members.len() + 1 < size && near(e) < 2.0 * TOGETHER_M).min_by(|a, b| near(a).total_cmp(&near(b)).then(a.leader.cmp(&b.leader))) {
            e.members.push(j);
            continue;
        }
        // otherwise, a new one with the loose ones nearby
        let mut members: Vec<usize> = Vec::new();
        loose.sort_by(|a, b| flat(a.1, p).total_cmp(&flat(b.1, p)).then(a.0.cmp(&b.0)));
        while members.len() + 1 < size && loose.first().is_some_and(|x| flat(x.1, p) < TOGETHER_M) {
            members.push(loose.remove(0).0);
        }
        loose.sort_by_key(|x| x.0);
        squads.push(Squad::new(j, members, Goal::Point([p[0], p[2]])));
    }
}

/// A squad vehicle's plan: who drives and who rides in each seat, since when. It is kept between splits
/// (like the player who says «get in, I'm leaving»: it does not change car every moment), until [`PLAN_S`] or until the vehicle or the
/// driver is lost.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub vehicle: usize,
    pub driver: usize,
    /// (player, seat)
    pub passengers: Vec<(usize, usize)>,
    pub since: f64,
    /// the plan lasts until then ([`PLAN_S`] after it was made; a trip's leg, until its boarding deadline)
    pub until: f64,
    /// where the vehicle was when the plan was made (the plan ends when it leaves there)
    pub start: [f32; 3],
}

/// How long a vehicle plan lasts, s: how long the driver takes to arrive, go round it and wait for its own.
pub const PLAN_S: f64 = 30.0;
/// With someone already inside, the plan lasts until the vehicle moves this far from where it was, m (it has left: those who have not boarded are
/// left on foot).
pub const PLAN_EXIT_M: f32 = 60.0;

/// The order of each living member of the squads and the passengers given to each vehicle, at instant `t`.
pub fn orders(squads: &mut [Squad], view: &TeamView, t: f64, levers: &Levers) -> (BTreeMap<usize, Order>, BTreeMap<usize, Vec<usize>>) {
    let mut orders = BTreeMap::new();
    let mut passengers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut used: BTreeSet<usize> = BTreeSet::new();
    let mate = |j: usize| view.mates.iter().find(|c| c.player == j);
    let veh = |v: usize| view.vehicles.iter().find(|c| c.vehicle == v);
    let pos = |j: usize| mate(j).map_or([0.0; 3], |c| c.pos);
    let near_m = levers.vehicle_near;
    for e in squads.iter_mut() {
        let goal = e.goal;
        let Goal::Flag(f) = goal else {
            e.planes.clear();
            continue;
        };
        let Some(fpos) = view.flags.get(f).map(|b| b.pos) else { continue };
        let leader = e.leader;
        if mate(leader).is_none() {
            e.planes.clear();
            continue;
        }
        let alive: Vec<usize> = e.alive(view).into_iter().filter(|&j| j != leader).collect();
        orders.insert(leader, Order { goal, role: Role::Leader, kit: 0 });
        for &m in &alive {
            orders.insert(m, Order { goal, role: Role::Follow(leader), kit: 0 });
        }
        let jp = pos(leader);
        let on_foot: Vec<usize> = alive.iter().copied().filter(|&m| mate(m).is_some_and(|c| c.vehicle.is_none())).collect();
        // the plans still valid: in time, the vehicle in sight (and the wheel free, or with its driver inside) and the
        // driver alive and on foot or in it
        // the driver lost (dead, or in another vehicle) with some of the passengers already inside: that one drives (takes the wheel)
        let en = |j: usize| mate(j).map(|c| c.vehicle);
        for p in e.planes.iter_mut() {
            if !en(p.driver).is_some_and(|x| x.is_none() || x == Some(p.vehicle))
                && let Some(k) = p.passengers.iter().position(|&(m, _)| en(m) == Some(Some(p.vehicle)))
            {
                p.driver = p.passengers.remove(k).0;
            }
        }
        // a boat or aircraft leg of the trip not yet boarded: its vehicle, before any other plan nobody has boarded yet (a car the
        // squad was filling must not eat the leg's boarding deadline: Bragg)
        let leg_v = e.trip.as_ref().filter(|tr| tr.boarded.is_none()).and_then(|tr| tr.now().mode.vehicle());
        // the plans still valid: in time (or with someone inside and not yet out), the vehicle in sight (and the wheel free,
        // or with one of its own inside) and the driver alive and on foot or in it
        e.planes.retain(|p| {
            let Some(c) = veh(p.vehicle) else { return false };
            let inside = en(p.driver) == Some(Some(p.vehicle)) || p.passengers.iter().any(|&(m, _)| en(m) == Some(Some(p.vehicle)));
            let in_time = t < p.until || (inside && flat(c.pos, p.start) < PLAN_EXIT_M);
            let not_the_leg = leg_v.is_some_and(|v| v != p.vehicle) && !inside;
            in_time && !not_the_leg && !used.contains(&p.vehicle) && (c.seats.contains(&0) || inside) && en(p.driver).is_some_and(|x| x.is_none() || x == Some(p.vehicle))
        });
        // the leg's plan takes whoever of its own is on foot and has no seat yet (the plan was made with the squad half spawned: at
        // t=0 the leader alone, and it flew off alone)
        if let Some(v) = leg_v
            && let Some(c) = veh(v)
            && let Some(p) = e.planes.iter_mut().find(|p| p.vehicle == v)
        {
            for &m in &on_foot {
                if m == p.driver || p.passengers.iter().any(|x| x.0 == m) {
                    continue;
                }
                let Some(s) = c.seats.iter().copied().find(|&s| s != 0 && !p.passengers.iter().any(|x| x.1 == s)) else { break };
                p.passengers.push((m, s));
            }
        }
        // a boat or aircraft leg of the trip not yet boarded, with its vehicle in sight, the helm free and nobody else's
        let leg = e.trip.as_ref().filter(|tr| tr.boarded.is_none()).and_then(|tr| {
            let l = tr.now();
            l.mode.vehicle().and_then(veh).filter(|c| c.seats.contains(&0) && !used.contains(&c.vehicle)).map(|c| (c, l.deadline))
        });
        let leader_at = mate(leader).and_then(|c| c.vehicle).filter(|v| veh(*v).is_some_and(|c| !c.seats.contains(&0)));
        if let Some(v) = leader_at {
            // the leader already rides one and someone drives: its own who walk nearby board with it, if they have no plan yet
            e.vehicle = Some(v);
            if !e.planes.iter().any(|p| p.vehicle == v) {
                let c = veh(v).unwrap();
                let driver = alive.iter().copied().chain(std::iter::once(leader)).find(|&j| mate(j).is_some_and(|x| x.vehicle == Some(v))).unwrap_or(leader);
                let mut free_ones = c.seats.iter().copied().filter(|&s| s != 0);
                let mut plan = Plan { vehicle: v, driver, passengers: Vec::new(), since: t, until: t + PLAN_S, start: c.pos };
                for &m in &on_foot {
                    if flat(pos(m), c.pos) > BOARD_NEAR {
                        continue;
                    }
                    let Some(s) = free_ones.next() else { break };
                    plan.passengers.push((m, s));
                }
                if !plan.passengers.is_empty() {
                    e.planes.push(plan);
                }
            }
            for &m in &alive {
                if mate(m).is_some_and(|x| x.vehicle == Some(v)) {
                    orders.insert(m, Order { goal, role: Role::Passenger(v, 1), kit: 0 });
                }
            }
            used.insert(v);
        } else if let Some((c, deadline)) = leg.filter(|_| e.planes.is_empty() && mate(leader).is_some_and(|x| x.vehicle.is_none())) {
            // the trip's leg is in a boat or an aircraft: the leader takes the helm and its own on foot the seats, wherever it is
            // (the squad walks to it); the plan lasts until the leg's boarding deadline
            e.vehicle = Some(c.vehicle);
            let mut free_ones = c.seats.iter().copied().filter(|&s| s != 0);
            let mut plan = Plan { vehicle: c.vehicle, driver: leader, passengers: Vec::new(), since: t, until: deadline, start: c.pos };
            for &m in &on_foot {
                let Some(s) = free_ones.next() else { break };
                plan.passengers.push((m, s));
            }
            e.planes.push(plan);
        } else if e.planes.is_empty() && flat(jp, fpos) > levers.vehicle_if_far && !on_foot.is_empty() {
            // far and on foot: a vehicle among those beside the leader is filled (the leader drives the first; the one it already had, before)
            let mut left: Vec<usize> = std::iter::once(leader).chain(on_foot.iter().copied()).filter(|&j| mate(j).is_some_and(|c| c.vehicle.is_none())).collect();
            let serves = |c: &VehicleSighting, r: f32| c.ground_with_seats() && c.seats.contains(&0) && flat(c.pos, jp) < r;
            let theirs = e.vehicle;
            let mut free_ones: Vec<&VehicleSighting> = view.vehicles.iter().filter(|c| !used.contains(&c.vehicle) && (serves(c, near_m) || (Some(c.vehicle) == theirs && serves(c, 1.6 * near_m)))).collect();
            free_ones.sort_by(|a, b| (Some(a.vehicle) != theirs).cmp(&(Some(b.vehicle) != theirs)).then(b.seats.len().cmp(&a.seats.len())).then(flat(a.pos, jp).total_cmp(&flat(b.pos, jp))).then(a.vehicle.cmp(&b.vehicle)));
            e.vehicle = None;
            for c in free_ones {
                if left.len() < 2 {
                    break;
                }
                let driver = if left[0] == leader { leader } else { *left.iter().min_by(|&&a, &&b| flat(pos(a), c.pos).total_cmp(&flat(pos(b), c.pos)).then(a.cmp(&b))).unwrap() };
                left.retain(|&j| j != driver);
                e.vehicle.get_or_insert(c.vehicle);
                left.sort_by(|&a, &b| flat(pos(a), c.pos).total_cmp(&flat(pos(b), c.pos)).then(a.cmp(&b)));
                let mut plan = Plan { vehicle: c.vehicle, driver, passengers: Vec::new(), since: t, until: t + PLAN_S, start: c.pos };
                for s in c.seats.iter().copied().filter(|&s| s != 0) {
                    if left.is_empty() {
                        break;
                    }
                    plan.passengers.push((left.remove(0), s));
                }
                e.planes.push(plan);
            }
        } else if e.planes.is_empty() {
            e.vehicle = None;
        }
        // the plans' orders: the driver to its wheel, each passenger to its seat (while alive and on foot or in it)
        for p in &e.planes {
            used.insert(p.vehicle);
            e.vehicle.get_or_insert(p.vehicle);
            if p.driver != leader || leader_at.is_none() {
                orders.insert(p.driver, Order { goal, role: Role::Drive(p.vehicle), kit: 0 });
            }
            for &(m, s) in &p.passengers {
                if mate(m).is_some_and(|c| c.vehicle.is_none() || c.vehicle == Some(p.vehicle)) {
                    orders.insert(m, Order { goal, role: Role::Passenger(p.vehicle, s), kit: 0 });
                    passengers.entry(p.vehicle).or_default().push(m);
                }
            }
        }
    }
    (orders, passengers)
}

#[cfg(test)]
mod tests {
    use super::super::team::{FlagSighting, Mate, VehicleSighting};
    use super::*;
    use crate::bot_api::VehicleClass;

    fn view(n: usize, x0: f32) -> TeamView {
        TeamView {
            team: 1,
            mates: (0..n).map(|j| Mate { player: j, pos: [x0 + j as f32 * 3.0, 0.0, 0.0], vehicle: None }).collect(),
            flags: vec![FlagSighting { pos: [600.0, 0.0, 0.0], owner: 2, takeable: true, radio: 10.0, on_foot: true }],
            vehicles: vec![VehicleSighting { vehicle: 7, pos: [x0, 0.0, 10.0], team: 1, free_seats: 3, class: VehicleClass::Car, health: 1.0, floats: false, seats: vec![0, 1, 2], radius: 3.0, afloat: false, vtol: false }],
        }
    }

    /// Nine together make three squads of four at most; if the leader falls, another takes over and the dead one stays in it.
    #[test]
    fn b02_squads_with_leader_and_takeover() {
        let mut v = view(9, 0.0);
        let mut sq = Vec::new();
        keep(&mut sq, &v, SIZE);
        assert_eq!(sq.len(), 3);
        assert!(sq.iter().all(|e| e.members.len() < SIZE));
        let leader = sq[0].leader;
        v.mates.retain(|c| c.player != leader);
        keep(&mut sq, &v, SIZE);
        assert_ne!(sq[0].leader, leader);
                assert!(sq[0].members.contains(&leader), "the dead one stays in its squad");
        for e in sq.iter_mut() {
            e.goal = Goal::Flag(0);
        }
        v.vehicles.clear();
        let (ord, _) = orders(&mut sq, &v, 10.0, &Levers::default());
        assert_eq!(ord[&sq[0].leader].role, Role::Leader);
        assert!(sq[0].members.iter().filter(|m| **m != leader).all(|m| ord[m].role == Role::Follow(sq[0].leader)));
    }

    /// The flag is far and there is a three-seat car beside the leader: it drives and two board; the one left follows it.
    #[test]
    fn b03_the_squad_vehicle_fills_up() {
        let v = view(4, 0.0);
        let mut sq = Vec::new();
        keep(&mut sq, &v, SIZE);
        sq[0].goal = Goal::Flag(0);
        let leader = sq[0].leader;
        let (ord, pas) = orders(&mut sq, &v, 10.0, &Levers::default());
        assert_eq!(ord[&leader].role, Role::Drive(7));
        assert_eq!(pas[&7].len(), 2);
        assert_eq!(ord.values().filter(|o| matches!(o.role, Role::Follow(j) if j == leader)).count(), 1);
        assert_eq!(sq[0].vehicle, Some(7));
    }

    /// The vehicle plan is kept between splits even if the leader walks away (on foot to it), each one keeps its seat, and
    /// expires at `PLAN_S`; the team reserves the vehicle for its own.
    #[test]
    fn b03_the_vehicle_plan_holds_and_expires() {
        let mut v = view(4, 0.0);
        let mut sq = Vec::new();
        keep(&mut sq, &v, SIZE);
        sq[0].goal = Goal::Flag(0);
        let leader = sq[0].leader;
        let (_, pas) = orders(&mut sq, &v, 10.0, &Levers::default());
        let plan = sq[0].planes.clone();
        assert_eq!(plan.len(), 1);
        // the leader has gone 100 m away (outside the radius in which a vehicle is taken) and the car stays there: the plan holds anyway
        v.mates.iter_mut().find(|c| c.player == leader).unwrap().pos = [100.0, 0.0, 0.0];
        let (ord, pas2) = orders(&mut sq, &v, 14.0, &Levers::default());
        assert_eq!(sq[0].planes, plan);
        assert_eq!(pas, pas2);
        assert_eq!(ord[&leader].role, Role::Drive(7));
        // expired, and far from the car: no plan any more
        let _ = orders(&mut sq, &v, 10.0 + PLAN_S + 1.0, &Levers::default());
        assert!(sq[0].planes.is_empty());
    }

    /// A transport helicopter beside the leader is not a squad vehicle: by land the squad goes on foot (the trip takes the helicopter
    /// as an air leg, [`super::super::trip::choose`]); with an air leg, the squad boards it even with a car at hand.
    #[test]
    fn b12_the_helicopter_is_the_trips() {
        let mut v = view(4, 0.0);
        v.vehicles[0].class = VehicleClass::Helicopter;
        v.vehicles[0].seats = vec![0, 1, 2, 3];
        let mut sq = Vec::new();
        keep(&mut sq, &v, SIZE);
        sq[0].goal = Goal::Flag(0);
        let (ord, _) = orders(&mut sq, &v, 10.0, &Levers::default());
        assert_eq!(ord[&sq[0].leader].role, Role::Leader, "flag at 600 m, by land: on foot");
        // a car at hand and a plan in it; then the trip's air leg in the helicopter: the car plan, nobody in it, goes
        v.vehicles.push(VehicleSighting { vehicle: 8, pos: [0.0, 0.0, 5.0], team: 1, free_seats: 3, class: VehicleClass::Car, health: 1.0, floats: false, seats: vec![0, 1, 2], radius: 3.0, afloat: false, vtol: false });
        let (ord, _) = orders(&mut sq, &v, 10.0, &Levers::default());
        assert_eq!(ord[&sq[0].leader].role, Role::Drive(8));
        let leg = crate::bots::trip::Leg { mode: crate::bots::trip::Mode::Air(7), to: [560.0, 0.0, 0.0], end: crate::bots::trip::End::Land, deadline: 60.0 };
        let mut tr = crate::bots::trip::Trip::ground(0, [600.0, 0.0, 0.0], 10.0);
        tr.legs.insert(0, leg);
        sq[0].trip = Some(tr);
        let (ord, _) = orders(&mut sq, &v, 12.0, &Levers::default());
        assert_eq!(ord[&sq[0].leader].role, Role::Drive(7));
    }
}
