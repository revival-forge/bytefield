//! The side's turn: the split of its people among the flags, the trips and the wing of aircraft.
use super::*;

impl Team {
    /// Decides with what the side knows at instant `t`, on a map it knows nothing of (every trip is one ground leg).
    pub fn think(&mut self, t: f64, view: &TeamView) {
        self.think_on(t, view, &trip::Unknown)
    }

    /// Decides with what the side knows at instant `t`, looking at the map (`land`) for the squads' trips.
    pub fn think_on(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        self.next = t + self.params.split_period_s;
        self.rng.get_or_insert_with(|| Rng::new(0xB0_75_3E_00 ^ view.team as u64));
        self.reports.retain(|p| t - p.t < 3.0 * self.params.report_s);
        let fl = &view.flags;
        self.intel_flags = fl.iter().map(|f| f.pos).collect();
        self.intel.set_flags(&self.intel_flags);
        self.intel.age(t);
        crate::bots::intel::note(t, view.team, &self.intel);
        if (TRAFFIC_MINES_ON || self.parts.traffic_mines) && self.traits.high() > 0 {
            self.mines_at = self.mine_spot(view);
        }
        if self.owners.len() != fl.len() {
            self.owners = fl.iter().map(|f| f.owner).collect();
            self.lost = vec![None; fl.len()];
            self.footsteps = vec![false; fl.len()];
        }
        // TODO: a flag up in the air on the deck the side stands on is not reached on foot until a mate has been in it: on GC_Judicator
        // every flag is inside the star destroyer (y = 341, ground ~150) and team 2 spawns 75 m from the neutral flag 3, but stands
        // still (86 % at its spawn at 60 s). A straight line of floor does not join them (the hangar between is open below, nothing
        // down to y = 180; the way is round by the halls), and the walker, with the flat path map, would not find it either: it needs
        // a path map per deck (`navgen` at the deck's height?). See `examples/obstacles_near data/game GCMOD GC_Judicator 540 890 60`.
        for (i, f) in fl.iter().enumerate() {
            let inside = |p: [f32; 3]| (0..3).map(|k| (p[k] - f.pos[k]).powi(2)).sum::<f32>() <= f.radio * f.radio;
            self.footsteps[i] |= view.mates.iter().any(|c| inside(c.pos));
        }
        let with_footsteps;
        let view = if fl.iter().zip(&self.footsteps).any(|(f, &p)| (p || !self.parts.reach) && !f.on_foot) {
            let mut v = view.clone();
            for (f, &p) in v.flags.iter_mut().zip(&self.footsteps) {
                f.on_foot |= p || !self.parts.reach;
            }
            with_footsteps = v;
            &with_footsteps
        } else {
            view
        };
        let fl = &view.flags;
        for (i, f) in fl.iter().enumerate() {
            if self.owners[i] == view.team && f.owner != view.team {
                self.lost[i] = Some(t);
            }
            self.owners[i] = f.owner;
        }
        self.urgent_ones.clear();
        self.accompanied_ones.clear();
        self.passengers.clear();
        let previous = |o: Option<&Order>| o.and_then(|o| if let Goal::Flag(i) = o.goal { Some(i) } else { None });
        if self.parts.squads {
            squad::keep(&mut self.squads, view, self.levers.size);
            let pos = |j: usize| view.mates.iter().find(|c| c.player == j).map(|c| c.pos);
            let units: Vec<Unit> = self
                .squads
                .iter()
                .map(|e| Unit { pos: e.alive(view).into_iter().filter_map(pos).collect(), previous: if let Goal::Flag(i) = e.goal { Some(i) } else { self.held.get(&e.leader).copied() }, avoid: e.avoid.iter().filter(|a| t < a.1).map(|a| a.0).collect() })
                .collect();
            let intel = self.parts.strength.then_some(&self.intel);
            let destinations = split_up_with(t, view, &self.reports, &self.lost, &units, &self.levers, &self.params, intel);
            for (e, d) in self.squads.iter_mut().zip(destinations) {
                let Some(d) = d else { continue };
                // (in the air on a plane its people jump from, it stays with its flag while it is there to take: given another
                // every few seconds, it gets to none)
                let flying_to = e.trip.as_ref().filter(|tr| tr.now().end == trip::End::Drop).filter(|tr| {
                    let v = tr.now().mode.vehicle();
                    v.is_some() && e.all().any(|j| view.mates.iter().any(|c| c.player == j && c.vehicle == v)) && fl.get(tr.flag).is_some_and(|f| f.takeable && f.owner != view.team)
                });
                if let Some(tr) = flying_to {
                    e.goal = Goal::Flag(tr.flag);
                    continue;
                }
                // (`bots:regroup-way`: a squad held at its point keeps its flag while it waits, unless one of the side's is being lost)
                if let Some(i) = self.parts.regroup_way.then(|| held_flag(&self.held, &self.rallies, e.leader, view)).flatten().filter(|_| !d.urgent) {
                    e.goal = Goal::Flag(i);
                    continue;
                }
                e.goal = Goal::Flag(d.flag);
                if d.urgent {
                    self.urgent_ones.extend(e.alive(view));
                }
            }
            if self.parts.hunt && !fl.is_empty() && fl.iter().all(|f| !f.takeable) {
                self.hunt = self.hunt_point(view, land);
                if let Some(p) = self.hunt {
                    for e in self.squads.iter_mut() {
                        if let Goal::Flag(i) = e.goal
                            && fl.get(i).is_some_and(|f| f.owner != view.team)
                        {
                            e.goal = Goal::Point([p[0], p[2]]);
                        }
                    }
                }
            }
            if self.parts.regroup_way {
                self.regroup_way(t, view, land);
            } else if self.parts.regroup {
                self.regroup(t, view, land);
            }
            self.trips(t, view, land);
            if self.parts.flank2 {
                self.flankers(t, view);
            }
            for e in &self.squads {
                let Some(jp) = view.mates.iter().find(|c| c.player == e.leader && c.vehicle.is_none()).map(|c| c.pos) else { continue };
                if !self.urgent_ones.contains(&e.leader) && e.members.iter().any(|&m| view.mates.iter().any(|c| c.player == m && c.vehicle.is_none() && flat(c.pos, jp) < squad::TOGETHER_M)) {
                    self.accompanied_ones.insert(e.leader);
                }
            }
            let (orders, passengers) = squad::orders(&mut self.squads, view, t, &self.levers);
            self.orders.extend(orders);
            self.passengers = passengers;
            if self.parts.regroup_way {
                self.hold_orders(view);
            }
            if self.parts.wing {
                self.wing(t, view, land);
            }
        } else {
            let units: Vec<Unit> = view.mates.iter().map(|c| Unit { pos: vec![c.pos], previous: previous(self.orders.get(&c.player)), avoid: Vec::new() }).collect();
            let intel = self.parts.strength.then_some(&self.intel);
            let destinations = split_up_with(t, view, &self.reports, &self.lost, &units, &self.levers, &self.params, intel);
            for (c, d) in view.mates.iter().zip(destinations) {
                let Some(d) = d else { continue };
                self.orders.insert(c.player, Order { goal: Goal::Flag(d.flag), role: Role::Leader, kit: 0 });
                if d.urgent {
                    self.urgent_ones.insert(c.player);
                }
            }
        }
        for (j, o) in self.orders.iter_mut() {
            o.kit = self.kit_de.get(j).copied().unwrap_or(0);
        }
        if crate::bots::trace_team() {
            let role = |o: &Order| format!("{:?}", o.role);
            if self.last_trace.is_none() {
                eprintln!("t={t:.0} team {} flags {:?}", view.team, fl.iter().map(|f| ([f.pos[0] as i32, f.pos[2] as i32], f.takeable, f.on_foot)).collect::<Vec<_>>());
                eprintln!("t={t:.0} team {} vehicles {:?}", view.team, view.vehicles.iter().map(|c| (c.vehicle, c.class, c.seats.len(), [c.pos[0] as i32, c.pos[1] as i32, c.pos[2] as i32])).collect::<Vec<_>>());
            }
            self.last_trace = Some(t);
            eprintln!("t={t:.0} team {} alive {} owners {:?} sq {:?} pas {:?}", view.team, view.mates.len(), self.owners, self.squads.iter().map(|e| (e.leader, e.members.len(), e.goal)).collect::<Vec<_>>(), self.passengers);
            eprintln!("t={t:.0} team {} presence {:?}", view.team, self.intel.presences().iter().map(|p| (p * 10.0).round() / 10.0).collect::<Vec<_>>());
            for e in self.squads.iter().filter(|e| e.trip.as_ref().is_some_and(|tr| !tr.plain()) || !e.avoid.is_empty()) {
                let to = e.trip.as_ref().map(|tr| tr.now().to.map(|x| x as i32));
                let veh = e.leg_vehicle().and_then(|v| view.vehicles.iter().find(|c| c.vehicle == v)).map(|c| c.pos.map(|x| x as i32));
                eprintln!("   trip {} {:?} leg {:?} to {to:?} vehicle at {veh:?} boarded {:?} avoid {:?}", e.leader, e.trip.as_ref().map(|tr| tr.legs.iter().map(|l| l.mode).collect::<Vec<_>>()), e.trip.as_ref().map(|tr| tr.leg), e.trip.as_ref().and_then(|tr| tr.boarded), e.avoid);
            }
            for pl in self.squads.iter().flat_map(|e| e.planes.iter()) {
                eprintln!("   plan {} {:?} driver {} passengers {:?} since {:.0}", pl.vehicle, view.vehicles.iter().find(|c| c.vehicle == pl.vehicle).map(|c| c.class), pl.driver, pl.passengers, pl.since);
            }
            for c in &view.mates {
                eprintln!("   {} {:?} veh {:?} {}", c.player, c.pos.map(|x| x as i32), c.vehicle, self.orders.get(&c.player).map_or("-".into(), role));
            }
        }
    }

    /// Each squad's trip to its flag ([`trip`]): chosen when the flag changes, upgraded from a plain ground leg when the map shows
    /// no way by land, and tested for progress; what fails is not tried again for a while, and with no way left the squad avoids that
    /// flag in the next splits.
    pub(super) fn trips(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        let avoid = |e: &mut Squad, f: usize, t: f64| {
            e.avoid.retain(|a| a.0 != f);
            e.avoid.push((f, t + trip::RETRY_S));
        };
        let mate = |j: usize| view.mates.iter().find(|c| c.player == j);
        let reports = &self.reports;
        let report_s = self.params.report_s;
        let enemies: Vec<[f32; 3]> = reports.iter().filter(|r| t - r.t < report_s).map(|r| r.pos).collect();
        let air_if_faster = self.parts.heli;
        for k in 0..self.squads.len() {
            let taken: Vec<usize> = self.squads.iter().enumerate().filter(|&(i, _)| i != k).filter_map(|(_, e)| e.leg_vehicle()).collect();
            let e = &mut self.squads[k];
            e.failed.retain(|f| t < f.until);
            e.avoid.retain(|a| t < a.1);
            let (Goal::Flag(f), Some(l)) = (e.goal, mate(e.leader)) else {
                e.trip = None;
                continue;
            };
            let Some(fs) = view.flags.get(f) else { continue };
            let on_foot = l.vehicle.is_none();
            // (in a vehicle already, it goes on by land; the boats and aircraft are chosen on foot)
            let choose = |failed: &[trip::Failed]| {
                let ask = trip::Ask { t, from: l.pos, flag: f, f: fs, vehicles: &view.vehicles, taken: &taken, failed, enemies: &enemies, air_if_faster };
                if on_foot { trip::choose(&ask, land) } else if failed.iter().any(|x| x.flag == f && x.kind == trip::Kind::Ground) { None } else { Some(Trip::ground(f, fs.pos, t)) }
            };
            // (on a plane its people jump from, already aboard, and the side gives it another flag: the same flight, over the new one;
            // a plane does not go back for a new plan)
            let dropping = e.trip.as_ref().is_some_and(|tr| {
                let leg = tr.now();
                tr.flag != f && leg.end == trip::End::Drop && leg.mode.vehicle().is_some_and(|v| e.all().any(|j| mate(j).is_some_and(|c| c.vehicle == Some(v))))
            });
            if dropping && let Some(tr) = e.trip.as_mut() {
                tr.flag = f;
                for leg in tr.legs.iter_mut().skip(tr.leg) {
                    leg.to = fs.pos;
                }
            }
            match &e.trip {
                Some(tr) if tr.flag == f && !(tr.plain() && on_foot) => {}
                Some(tr) if tr.flag == f => {
                    // a plain ground leg that the map now says has no way (the squad respawned across the water): the new trip
                    if let Some(n) = choose(&e.failed).filter(|n| !n.plain()) {
                        e.trip = Some(n);
                    }
                }
                _ => e.trip = choose(&e.failed),
            }
            let Some(leg) = e.trip.as_ref().map(|tr| tr.now()) else {
                avoid(e, f, t);
                continue;
            };
            let vehicle = leg.mode.vehicle().and_then(|v| view.vehicles.iter().find(|c| c.vehicle == v)).filter(|c| c.health > 0.0).map(|c| c.pos);
            let aboard = leg.mode.vehicle().is_some_and(|v| e.all().any(|j| mate(j).is_some_and(|c| c.vehicle == Some(v))));
            let fighting = reports.iter().any(|r| t - r.t < report_s && flat(r.pos, l.pos) < trip::FIGHT_M);
            let sample = trip::Sample { t, leader: e.leader, leader_pos: l.pos, aboard, vehicle, fighting, boarding: !e.planes.is_empty(), below: vehicle.map_or(f32::MIN, |p| land.below(p[0], p[2])) };
            let tr = e.trip.as_mut().unwrap();
            // in the air towards a landing spot with enemies at it: another, once (on the flag's ground, not on a platform)
            if let (trip::Mode::Air(_), Some(p), true) = (leg.mode, vehicle, aboard)
                && leg.end == trip::End::Land
                && !tr.moved
                && fs.on_foot
                && flat(p, leg.to) < trip::LAND_RETHINK_M
                && enemies.iter().any(|q| flat(*q, leg.to) < 0.5 * trip::LAND_ENEMY_M)
            {
                tr.moved = true;
                let to = trip::landing_spot(fs.pos, p, &enemies, land);
                tr.legs[tr.leg].to = to;
                if crate::bots::trace_team() {
                    eprintln!("t={t:.0} team {} squad {} lands elsewhere: {:?} -> {:?}", view.team, e.leader, leg.to.map(|x| x as i32), to.map(|x| x as i32));
                }
            }
            if let trip::Step::Failed(kind) = tr.check(&sample) {
                if crate::bots::trace_team() {
                    eprintln!("t={t:.0} team {} squad {} trip to {f} failed: {:?} leg {} of {:?}", view.team, e.leader, kind, tr.leg, tr.legs.iter().map(|l| l.mode).collect::<Vec<_>>());
                }
                if kind == trip::Kind::Ground && on_foot {
                    self.stuck.insert(e.leader, t);
                }
                let gone = leg.mode.vehicle();
                e.planes.retain(|p| Some(p.vehicle) != gone);
                e.failed.push(trip::Failed { flag: f, kind, until: t + trip::RETRY_S });
                e.trip = choose(&e.failed);
                if e.trip.is_none() {
                    avoid(e, f, t);
                }
            }
        }
    }

    /// B23: the side's fighting aircraft get a pilot, as players run to the planes at the start: each free one (a plane, or a
    /// helicopter that does not carry a squad), with the pilot's seat free and nobody's trip, goes to the nearest bot on foot within
    /// [`WING_NEAR_M`] that only follows its leader (not a leader, a driver or one with an urgent order), one pilot per [`WING_SHARE`]
    /// bots of the side at most. It keeps it while it flies it; the order to get in lasts [`WING_BOARD_S`]. Once in, it flies as
    /// any pilot of the side (to the target the side reports, else from flag to flag).
    pub(super) fn wing(&mut self, t: f64, view: &TeamView, land: &dyn trip::Land) {
        let mate = |j: usize| view.mates.iter().find(|c| c.player == j);
        let legs: BTreeSet<usize> = self.squads.iter().filter_map(|e| e.leg_vehicle()).collect();
        let planned: BTreeSet<usize> = self.squads.iter().flat_map(|e| e.planes.iter().map(|p| p.vehicle)).collect();
        // what was given and still holds: the pilot alive and in it, or on its way in time and the seat still free
        // (one sent from the spawn screen is not alive yet)
        let kits = &self.kit_de;
        self.wing.retain(|v, (j, until)| {
            let Some(c) = mate(*j) else { return t < *until && kits.contains_key(j) };
            let craft = view.vehicles.iter().find(|x| x.vehicle == *v);
            c.vehicle == Some(*v) || (c.vehicle.is_none() && t < *until && craft.is_some_and(|x| x.seats.contains(&0) && x.health > 0.3))
        });
        let bots = view.mates.iter().filter(|c| self.is_bot(c.player)).count();
        let most = bots.div_ceil(WING_SHARE);
        let fights = |x: &VehicleSighting| (x.class == VehicleClass::Plane || (x.class == VehicleClass::Helicopter && !x.transport_helicopter())) && !x.floats;
        // (a plane up on something that is not a ship does not take off with the bots' pilot: the runway logic of [`Pilot::fly`]
        // looks at the terrain, not at what it stands on; one that lifts straight up does: GC_Bespin's cloud cars on the platforms)
        let raised = |x: &VehicleSighting| {
            let under = land.below(x.pos[0], x.pos[2]);
            x.class == VehicleClass::Plane && !x.vtol && !x.afloat && under > f32::MIN && x.pos[1] - under > WING_RAISED_M
        };
        let mut free: Vec<&VehicleSighting> = view.vehicles.iter().filter(|x| fights(x) && !raised(x) && x.seats.contains(&0) && x.health > 0.5 && !legs.contains(&x.vehicle) && !planned.contains(&x.vehicle) && !self.wing.contains_key(&x.vehicle)).collect();
        free.sort_by_key(|x| x.vehicle);
        self.wing_most = most;
        self.wing_free.clear();
        for x in free {
            if self.wing.len() >= most {
                break;
            }
            let pilot = view
                .mates
                .iter()
                .filter(|c| c.vehicle.is_none() && self.is_bot(c.player) && flat(c.pos, x.pos) < if x.afloat { WING_DECK_M } else { WING_NEAR_M } && !self.urgent_ones.contains(&c.player))
                .filter(|c| matches!(self.orders.get(&c.player).map(|o| o.role), Some(Role::Follow(_))) && !self.wing.values().any(|w| w.0 == c.player))
                // (one it can walk to: with a way on the path map, or, where the map does not say, at about its height; on GC_Bespin's
                // platforms they walked off the edge towards an aircraft on another)
                .filter(|c| match land.reach(c.pos, x.pos, x.radius + 4.0) {
                    trip::Reach::Path => true,
                    trip::Reach::NoPath => false,
                    _ => (c.pos[1] - x.pos[1]).abs() < WING_STEP_M,
                })
                .min_by(|a, b| flat(a.pos, x.pos).total_cmp(&flat(b.pos, x.pos)).then(a.player.cmp(&b.player)));
            if crate::bots::trace_team() {
                let near: Vec<(usize, i32, Option<Role>)> = view.mates.iter().filter(|c| c.vehicle.is_none()).map(|c| (c.player, flat(c.pos, x.pos) as i32, self.orders.get(&c.player).map(|o| o.role))).filter(|m| m.1 < 2 * WING_NEAR_M as i32).collect();
                eprintln!("t={t:.0} team {} wing: aircraft {} {:?} at {:?} -> {:?}; on foot near {near:?}", view.team, x.vehicle, x.class, x.pos.map(|q| q as i32), pilot.map(|c| c.player));
            }
            match pilot {
                Some(c) => {
                    self.wing.insert(x.vehicle, (c.player, t + WING_BOARD_S + (flat(c.pos, x.pos) / WING_WALK) as f64));
                }
                None => self.wing_free.push((x.vehicle, x.pos)),
            }
        }
        for (&v, &(j, _)) in &self.wing {
            if let Some(o) = self.orders.get_mut(&j) {
                o.role = Role::Drive(v);
            }
        }
    }

    /// B23: on the spawn screen, the aircraft `d`'s player is sent to fly (the one it had, or a free one beside a group it can spawn at,
    /// if the side may send another pilot and it only follows its leader): where it is.
    pub(super) fn wing_spawn(&mut self, d: &DeadView, role: Role) -> Option<[f32; 3]> {
        if !self.parts.wing {
            return None;
        }
        let at = |v: usize| d.vehicles.iter().find(|x| x.vehicle == v).map(|x| x.pos);
        if let Some((&v, _)) = self.wing.iter().find(|(_, w)| w.0 == d.player) {
            return at(v);
        }
        if !matches!(role, Role::Follow(_)) || self.wing.len() >= self.wing_most {
            return None;
        }
        // (an empty aircraft is nobody's: the enemy's carrier's too, which none of its groups is near)
        let near_group = |p: [f32; 3]| d.groups.iter().any(|g| g.active && flat(g.pos, p) < WING_NEAR_M);
        let (v, pos) = self.wing_free.iter().copied().find(|&(v, p)| !self.wing.contains_key(&v) && near_group(p))?;
        self.wing.insert(v, (d.player, d.time + WING_BOARD_S + WING_SPAWN_S + (WING_NEAR_M / WING_WALK) as f64));
        Some(pos)
    }
}
