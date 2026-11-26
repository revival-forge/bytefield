//! [`Individual::think`] by phases: what it tunes in the body, where to, with whom, in which seat, aboard, and what the body does.
use super::*;
use crate::bot_api::Inside;

impl Individual {
    /// One tick: the axes it presses. The order is taken to the body (where to, with whom, in which vehicle); the how is the body's.
    /// [`Role::Free`]: goes on its own, like a player with no squad (as the plain bot).
    pub fn think(&mut self, v: &BotView, command: &Command) -> Vec<(usize, f32)> {
        self.waits = None;
        let o = command.order;
        let ambush_point = self.parked_tick(v, command);
        if v.me.inside.is_some() || flat(v.me.pos, [self.last_step.1[0], self.last_step.1[2]]) > SEAT_STEP_M {
            self.last_step = (v.time, v.me.pos);
        }
        if self.kind.is_none() && !v.me.items.is_empty() {
            self.kind = Some(Kind::de(&v.me.items));
        }
        self.tune_body(v, o);
        if o.role == Role::Free {
            return self.think_free(v, command);
        }
        let me = &v.me;
        let (mut point, meta) = self.where_to(v, command);
        self.with_whom(v, command, meta, &mut point);
        // ---- what to do (survival): by utility, and what is neither going to the goal nor fighting changes where it goes
        let action = if me.inside.is_none() { self.decide(v, command, meta) } else { Action::GoToGoal };
        if me.inside.is_some() {
            self.action = Action::GoToGoal;
            self.reaction = None;
        }
        let about_its_business = matches!(action, Action::GoToGoal | Action::Fight);
        if let Some(p) = self.execute(v, action) {
            point = Some(p);
        }
        if let Some(p) = self.tank_cover(v, about_its_business) {
            point = Some(p);
        }
        self.seat(v, command, about_its_business, &mut point);
        let c = &mut self.body;
        if let Some(a) = ambush_point {
            point = Some(a);
            c.no_own_vehicle = true;
            self.following = false;
        }
        c.impose_point(point);
        self.aboard(v, command, meta);
        self.finish(v, command, meta)
    }

    /// What its order and its kit set in the body each tick: what it checks before firing, how it aims, how it drives (on its own
    /// ([`Role::Free`]), as the plain bot).
    fn tune_body(&mut self, v: &BotView, o: Order) {
        let c = &mut self.body;
        // does not fire at a wall (on its own: as the plain bot, nothing more)
        c.shooter_mut().check_line = o.role != Role::Free && self.settings.line;
        // from the air it sees a head or a shoulder, as a person does, not only the chest
        c.shooter_mut().see_body = o.role != Role::Free && self.settings.body && v.me.inside.as_ref().is_some_and(|i| i.class.flies());
        // from a gunner's seat of an aircraft: what flies, led (not on its own: as the plain bot)
        c.shooter_mut().gunner = (o.role != Role::Free && self.settings.dogfight && v.me.inside.as_ref().is_some_and(|i| i.class.flies() && i.seat != 0)).then(|| c.shooter_mut().gunner.take().unwrap_or_default());
        // difficulty (on its own: as the plain bot)
        let (reaction, error) = if o.role == Role::Free { (crate::bots::body::shooter::MEDIAN_REACTION, crate::bots::body::shooter::AIM_ERROR) } else { (self.profile.reaction_s(), self.profile.error_degrees()) };
        c.shooter_mut().median_reaction = reaction;
        c.shooter_mut().error_degrees = error;
        c.break_contact = false;
        c.driver_mut().drive_fine = self.settings.gear && o.role != Role::Free;
        c.driver_mut().swim = self.settings.swim && o.role != Role::Free;
        c.driver_mut().sight = self.settings.sight && o.role != Role::Free;
        // goes round the hull to a seat (the plain bot, which a free player plays as, walks straight at it)
        c.go_round_hull = o.role != Role::Free;
        c.driver_mut().go_round_hull = o.role != Role::Free;
        // B20: shut in by the path map (a depot with one door), it looks for the way out
        c.walker_mut().seeks_way_out = o.role != Role::Free;
        c.seat_bound = matches!(o.role, Role::Drive(_) | Role::Passenger(..));
        // when it goes into a flag: its named parameters (on its own, as the plain bot)
        c.enter = if o.role == Role::Free { crate::bots::life::Enter::default() } else { crate::bots::life::Enter::of(&self.settings.params) };
        c.trade.heal = None;
        c.trade.repair = None;
    }

    /// [`Role::Free`]: no imposed goal, seat or vehicle; whoever flies goes where the radio tells it.
    fn think_free(&mut self, v: &BotView, command: &Command) -> Vec<(usize, f32)> {
        let c = &mut self.body;
        let shooter = c.shooter_mut();
        shooter.prefers_vehicle_m = 0.0;
        shooter.wanted_slot = None;
        shooter.p_grenade = crate::bots::body::shooter::P_GRENADE;
        c.imposed = None;
        c.impose_point(None);
        c.seat_spot = None;
        c.wait_until = 0.0;
        c.no_own_vehicle = false;
        c.get_off_now = false;
        c.land_at = None;
        c.pilot_mut().flight = None;
        c.reserved_mut().clear();
        // even with no orders (maps with no flags to take, like the air-only or sea-only ones), whoever flies goes where the radio tells it
        let flies = v.me.inside.as_ref().is_some_and(|i| i.seat == 0 && i.class.flies());
        c.pilot_mut().imposed_goal = command.air_target.filter(|_| flies).map(|b| [b[0], b[2]]);
        c.pilot_mut().attack_run = true;
        c.pilot_mut().dogfight = self.settings.dogfight;
        c.pilot_mut().vtol = self.settings.vtol;
        engage(c.pilot_mut(), command);
        pass(c.pilot_mut(), command, v);
        return c.think(v);
    }

    /// Where to: the flag or point of its order (imposed on the body), and the point it goes to instead, if any.
    fn where_to(&mut self, v: &BotView, command: &Command) -> (Option<([f32; 3], f32)>, Option<[f32; 3]>) {
        let o = command.order;
        let c = &mut self.body;
        // what the side has given to others it does not take
        let r = c.reserved_mut();
        r.clear();
        r.extend(command.reserved.iter().copied().filter(|&x| x != usize::MAX));
        let me = &v.me;
        let t = v.time;
        let flags = v.flags();
        // ---- where to
        let mut point = None;
        let meta: Option<[f32; 3]> = match o.goal {
            Goal::Flag(i) if i < flags.len() => {
                c.imposed = Some(Choice { flag: i, attacks: flags[i].team != me.team });
                Some(flags[i].pos)
            }
            Goal::Flag(_) => {
                c.imposed = None;
                None
            }
            Goal::Point(p) => {
                c.imposed = None;
                let y = v.ground(p[0], p[1]).unwrap_or(me.pos[1]);
                point = Some(([p[0], y, p[1]], 12.0));
                point.map(|x| x.0)
            }
        };
        // (in the air on foot out of an aircraft in flight, it comes down towards its goal if it is near: it jumped over it)
        let jumped = me.inside.is_none() && self.aloft_at.is_some_and(|a| t - a < FALL_GRACE_S || Pilot::in_the_air(v));
        if me.inside.is_none() && !jumped {
            self.aloft_at = None;
        }
        c.fall_towards = meta.filter(|p| jumped && flat(me.pos, [p[0], p[2]]) < FALL_STEER_M);
        (point, meta)
    }

    fn with_whom(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>, point: &mut Option<([f32; 3], f32)>) {
        let o = command.order;
        let me = &v.me;
        let c = &mut self.body;
        // ---- with whom: on foot, behind the leader who goes on foot
        // (unless it cannot get there on foot: its path has just run off the edge of what it stands on, a platform far above the
        // ground like GC_Bespin's bases; then it takes a vehicle of its own, one that flies too, and does not stay with the leader)
        let at_edge_on_foot = me.inside.is_none() && v.time - c.walker_mut().edge_t < EDGE_S;
        c.no_own_vehicle = o.role == Role::Leader && command.accompanied && !at_edge_on_foot;
        // half the squad enters the flag it attacks from another side than the leader
        let flank = self.flank_move(v, command);
        let c = &mut self.body;
        if let Some(p) = flank {
            self.following = false;
            // (`bots:steal-flank`: on its way round it may go for an enemy's vehicle, and only for that)
            let steal = self.settings.steal_flank && c.driver().steals;
            c.no_own_vehicle = !steal;
            c.driver_mut().only_steal = steal;
            *point = Some((p, 8.0));
        } else if at_edge_on_foot {
            self.following = false;
        } else if let Role::Follow(j) = o.role
            && !self.flank.is_some_and(|f| f.2)
        {
            match v.others().into_iter().find(|x| x.player == j && x.team == me.team) {
                Some(x)
                    if self.settings.escort2
                        && me.inside.is_none()
                        && x.in_vehicle
                        && x.vehicle.and_then(|k| v.vehicle(k)).is_some_and(|k| matches!(k.class, VehicleClass::Tank | VehicleClass::Walker))
                        && meta.is_some_and(|m| flat(x.pos, [m[0], m[2]]) > ESCORT_FLAG_M) =>
                {
                    let m = meta.unwrap();
                    let (dx, dz) = (x.pos[0] - m[0], x.pos[2] - m[2]);
                    let l = dx.hypot(dz).max(0.1);
                    let behind = [x.pos[0] + dx / l * ESCORT_BEHIND_M, x.pos[1], x.pos[2] + dz / l * ESCORT_BEHIND_M];
                    let d = flat(me.pos, [behind[0], behind[2]]);
                    if d > ESCORT_SLACK_M * 2.0 {
                        self.following = true;
                    } else if d < ESCORT_SLACK_M {
                        self.following = false;
                    }
                    c.no_own_vehicle = true;
                    if self.following {
                        *point = Some((behind, ESCORT_SLACK_M / 2.0));
                    }
                }
                Some(x) if me.inside.is_none() && (!x.in_vehicle || x.vehicle.and_then(|k| v.vehicle(k)).is_some_and(|k| matches!(k.class, VehicleClass::Tank | VehicleClass::Walker))) => {
                    // (behind the leader's tank, somewhat farther: so it does not run it over and covers it)
                    let (far, near, r) = if x.in_vehicle { (TANK_FAR, TANK_NEAR, 10.0) } else { (FOLLOW_FAR, FOLLOW_NEAR, 6.0) };
                    let d = flat(me.pos, [x.pos[0], x.pos[2]]);
                    if d > far {
                        self.following = true;
                    } else if d < near {
                        self.following = false;
                    }
                    c.no_own_vehicle = true;
                    if self.following {
                        *point = Some((x.pos, r));
                    }
                }
                _ => self.following = false,
            }
        }
    }

    fn seat(&mut self, v: &BotView, command: &Command, about_its_business: bool, point: &mut Option<([f32; 3], f32)>) {
        let o = command.order;
        let me = &v.me;
        let t = v.time;
        let flags = v.flags();
        let c = &mut self.body;
        // (imposed once, below, after the walk to its seat: imposing this and then the vehicle every tick made the walker drop its
        // path and ask for a new one each tick)
        // ---- in which vehicle
        let seat_spot = match o.role {
            Role::Drive(j) => Some((j, 0)),
            Role::Passenger(j, s) => Some((j, s)),
            _ => None,
        }
        .filter(|(j, _)| self.gives_up != Some(*j));
        match (&me.inside, seat_spot) {
            (None, Some((j, s))) => {
                // (the trip's boat or aircraft may be hundreds of metres away: the patience counts once beside it)
                let far_leg = (command.leg_vehicle == Some(j) || command.wing_vehicle == Some(j)) && v.vehicle(j).is_some_and(|x| flat(me.pos, [x.pos[0], x.pos[2]]) > x.radius + SEAT_WALKING_M);
                if self.seat_requested.is_none_or(|p| p.0 != j) || far_leg {
                    self.seat_requested = Some((j, t));
                }
                // (its seat taken by one of its own, the other left free: it goes anyway; on boarding it falls into whichever remains and swaps
                // to its own when the other moves to the one it is due, like the passenger who has fallen to the wheel)
                let seat = v.vehicle(j).filter(|x| x.health > 0.1 && (x.team == 0 || x.team == me.team)).and_then(|x| {
                    if x.free_seats.contains(&s) {
                        Some(s)
                    } else if s != 0 {
                        x.free_seats.iter().copied().find(|&q| q != 0)
                    } else if !x.free_seats.is_empty() {
                        Some(s)
                    } else {
                        None
                    }
                });
                if seat.is_none() || self.seat_requested.is_some_and(|p| t - p.1 > SEAT_PATIENCE) {
                    self.gives_up = Some(j);
                    c.seat_spot = None;
                } else {
                    c.seat_spot = seat.map(|s| (j, s));
                    c.no_own_vehicle = true;
                }
            }
            (Some(ins), _) => c.seat_spot = seat_spot.filter(|p| p.0 == ins.vehicle),
            (None, None) => c.seat_spot = None,
        }
        // what it gave up it does not take again on its own either (the body's search for a vehicle at hand): parked beside the flag
        // it attacks, it got off, and climbed back into the same landspeeder 4 s later to sit there under fire (GC_Tatooine, the filmed bot)
        if self.vetoed_given_up != self.gives_up {
            let vetoed = &mut c.driver_mut().vetoed;
            if let Some(old) = self.vetoed_given_up
                && let Some(k) = vetoed.iter().position(|&x| x == old)
            {
                vetoed.remove(k);
            }
            if let Some(new) = self.gives_up.filter(|j| !vetoed.contains(j)) {
                vetoed.push(new);
                self.vetoed_given_up = Some(new);
            } else {
                self.vetoed_given_up = None;
            }
        }
        if !about_its_business && me.inside.is_none() {
            // (healing, going to the medic or under cover it does not go to its seat)
            c.seat_spot = None;
        }
        // B14: a fixed gun at hand with an enemy in its reach: it takes it (the body fires from it and gets out after a while)
        let own: Vec<[f32; 3]> = flags.iter().filter(|f| f.team == me.team).map(|f| f.pos).collect();
        let fixed = if me.inside.is_none() && about_its_business && self.body.seat_spot.is_none() && !command.urgent && self.settings.fixed { self.fixed_weapon(v, &own) } else { None };
        let c = &mut self.body;
        if let Some(g) = fixed {
            c.seat_spot = Some((g, 0));
            c.no_own_vehicle = true;
        }
        // repaired (or time up) the vehicle it got off, it boards again if still free
        if let Some((j, since)) = self.repair_own {
            match &me.inside {
                Some(_) => self.repair_own = None,
                None if t - since > REPAIR_OWN_S + SEAT_PATIENCE => self.repair_own = None,
                None if about_its_business => {
                    if v.vehicle(j).is_some_and(|x| x.driver_free && x.health > 0.2 && (x.team == 0 || x.team == me.team)) {
                        c.seat_spot = Some((j, 0));
                        c.no_own_vehicle = true;
                    }
                }
                None => {}
            }
        }
        // far from its seat, it walks by the paths to the vehicle (in a straight line it stays against a wall or a house); on the
        // side, to the entrance
        // (if by the paths it takes no step in a while, because there is no path to the vehicle's spot, it goes straight)
        // (it is the distance moved that counts, not the speed: pushing against something or turning on the spot the speed jitters
        // over and under any threshold and reset the count, so it stood 14 s at 12 m from the vehicle)
        // (and if straight it takes no step either, it is shut in: on DC_Coastal_Hammer the helicopter's pilot appears in a barracks
        // beside it and pushed against its wall until a passenger took the controls; it gets out as from a place the map shuts)
        if me.inside.is_none()
            && let Some((j, _)) = c.seat_spot
            && self.straight_seat == Some(j)
            && t - self.last_step.0 > SEAT_SHUT_IN_S
            && v.vehicle(j).is_some_and(|x| flat(me.pos, [x.pos[0], x.pos[2]]) > x.radius + SEAT_WALKING_M)
        {
            self.straight_seat = None;
            self.walking_still = None;
            c.break_out(v);
        }
        // (nearer, with a wall or a fence between it and the vehicle, it keeps to the paths too, to the vehicle itself: on
        // DC_Medina_Ridge's flag, a triangle of rails with a BMP-2 parked outside it, three soldiers inside pushed into the rails
        // towards their seats for 18 s each)
        let fenced = |x: &VehicleSeen| c.maps().foot(v).is_some_and(|n| !n.line_clear_from(crate::bots::body::feet(me.pos), [x.pos[0], x.pos[2]], 0.5));
        if me.inside.is_none()
            && let Some((j, _)) = c.seat_spot
            && let Some(x) = v.vehicle(j)
            && let near = flat(me.pos, [x.pos[0], x.pos[2]]) <= x.radius + SEAT_WALKING_M
            && (!near || fenced(&x))
            && self.straight_seat != Some(j)
        {
            match self.walking_still {
                Some(k) if k == j => {
                    if t - self.last_step.0 > SEAT_NO_WAY_S {
                        self.straight_seat = Some(j);
                    }
                }
                _ => {
                    self.walking_still = Some(j);
                    self.last_step = (t, me.pos);
                }
            }
            if self.straight_seat != Some(j) {
                c.seat_spot = None;
                c.no_own_vehicle = true;
                *point = Some((x.pos, if near { 2.0 } else { x.radius + 2.0 }));
            }
        }
    }

    /// Inside: the driver waits for its own; the passenger gets off on arrival.
    fn aboard(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>) {
        let me = &v.me;
        let c = &mut self.body;
        c.wait_until = 0.0;
        c.get_off_now = false;
        if me.inside.is_none() {
            self.parked_since = None;
            self.jammed = None;
        }
        match &me.inside {
            Some(ins) => self.aboard_in(v, command, meta, ins),
            None => self.boarded = None,
        }
    }

    fn aboard_in(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>, ins: &Inside) {
        let me = &v.me;
        let t = v.time;
        let c = &mut self.body;
        if self.boarded.is_none_or(|s| s.0 != ins.vehicle) {
            self.boarded = Some((ins.vehicle, t));
            self.without_driver_since = None;
        }
        let since = self.boarded.map_or(t, |s| s.1);
        // (B01, the first flag: the cars that stood 20-30 s at the spawn were not waiting but jammed: stopped to turn, which
        // a car cannot do, and braking for the squad walking beside it; [`Driver::drive_fine`]. What is left is no jam:
        // El_Alamein's nearest flag is ~680 m away, ~75 s at the driver's top speed with boarding and taking it; in Kursk and
        // Basrahs both teams meet at the middle flag and fight there; Medina_Ridge has no neutral flag)
        // TODO: passengers that cannot reach their seat of an airship (its door over the void at the platform's edge) stand on
        // its hull while the pilot waits; when the wait runs out it lifts off and they fall 260 m (GC_Bespin, 1–2 a round of the
        // 2.3 left). The pilot should not lift off with a mate standing on it, or the passenger step off to the floor. How:
        // `BYTEFIELD_BOTS_TRACE=1 examples/deaths data/game GCMOD GC_Bespin --a bots --b plain --minutes 2 --seed 3`
        // (players 23 and 31: `Passenger(7, …)` at the airship's own position, y 282, then `Follow` and the fall).
        let filmed = crate::bots::filmed() == Some(me.player);
        if ins.seat == 0 && command.passengers_out > 0 && !command.urgent && !filmed {
            c.wait_until = since + self.profile.passengers_wait_s() as f64;
        }
        if c.target().is_some() {
            self.contact_t = t;
        }
        self.ground_crew(v, command, ins, since, filmed);
        self.air_crew(v, command, meta, ins);
    }

    /// At the wheel or in a ground vehicle: getting off to repair, parked, with no way, at an idle fixed gun, waiting for its escort or
    /// its engineer, and jammed.
    fn ground_crew(&mut self, v: &BotView, command: &Command, ins: &Inside, since: f64, filmed: bool) {
        let o = command.order;
        let me = &v.me;
        let t = v.time;
        let c = &mut self.body;
        let flags = v.flags();
        // the engineer gets off to repair its damaged vehicle if nobody shoots at it (and stops it itself, if driving)
        let on_ground = matches!(ins.class, VehicleClass::Tank | VehicleClass::Car | VehicleClass::Walker);
        let stopped = (ins.velocity[0].powi(2) + ins.velocity[2].powi(2)).sqrt() < 2.0;
        if self.kind == Some(Kind::Engineer) && on_ground && !command.urgent && t - self.contact_t > REPAIR_CALM_S && (ins.seat == 0 || stopped) && v.vehicle(ins.vehicle).is_some_and(|x| x.health > 0.2 && x.health < HEALTH_GET_OFF_TO_REPAIR && !x.fixed) {
            self.repair_own = Some((ins.vehicle, t));
        }
        if self.repair_own.is_some_and(|r| r.0 == ins.vehicle) {
            c.get_off_now = true;
        }
        // B21: its ground vehicle stands beside the flag it attacks and not inside it, with nobody to shoot at (what is in
        // the way of a vehicle, a camp or a wreck around the flag, is not in the way of a soldier): on foot from there.
        // Not where the driver waits outside the flag on purpose: on Berlin it cost the side its tanks' guns (bots
        // against the plain bot, four rounds of four lower, 11 to 39 tickets)
        let attacked = if let Goal::Flag(i) = o.goal { flags.get(i).filter(|f| f.team != me.team) } else { None };
        let parked = on_ground && stopped && c.enters_at_once() && c.target().is_none() && attacked.is_some_and(|f| (PARKED_IN * f.radius..PARKED_M).contains(&flat(ins.pos, [f.pos[0], f.pos[2]])));
        self.parked_since = if parked { self.parked_since.or(Some(t)) } else { None };
        if self.parked_since.is_some_and(|s| t - s > PARKED_S) {
            c.get_off_now = true;
            self.gives_up = Some(ins.vehicle);
        }
        if ins.seat == 0 && on_ground && self.settings.no_way && o.role != Role::Free && c.driver_mut().no_way.is_some_and(|s| t - s > NO_WAY_S) {
            c.driver_mut().no_way = None;
            c.get_off_now = true;
            self.gives_up = Some(ins.vehicle);
        }
        // B14/B16: at a fixed gun with nobody to shoot at for a while, it gets off and goes on (sitting there is standing still)
        if ins.seat == 0 && ins.class == VehicleClass::Fixed && t - since > FIXED_NO_TARGET_S && t - self.contact_t > FIXED_NO_TARGET_S {
            c.get_off_now = true;
        }
        // at the wheel of a tank, outside combat, it does not leave its escort on foot behind
        let escort_wait = !filmed && ins.seat == 0 && matches!(ins.class, VehicleClass::Tank | VehicleClass::Walker) && self.settings.escort && !command.urgent && c.target().is_none() && command.escort_m.is_some_and(|d| d > ESCORT_WAIT_M && d < ESCORT_FAR_M);
        match (escort_wait, self.escort) {
            (true, None) => self.escort = Some((t, false)),
            (true, Some((since, false))) if t - since > ESCORT_WAIT_S => self.escort = Some((t, true)),
            (_, Some((since, true))) if t - since > ESCORT_REST_S => self.escort = None,
            (false, Some((_, false))) => self.escort = None,
            _ => {}
        }
        if escort_wait && self.escort.is_some_and(|e| !e.1) {
            c.wait_until = c.wait_until.max(t + 0.5);
        }
        // with the vehicle damaged and an engineer of its own at hand, it stops for it to be fixed
        if ins.seat == 0 && !command.urgent && c.target().is_none() {
            let touched = v.vehicle(ins.vehicle).is_some_and(|x| x.health > 0.2 && x.health < HEALTH_REPAIR && !x.fixed);
            if touched && v.others().iter().any(|x| x.team == me.team && !x.in_vehicle && command.engineers.has(x.player) && flat(ins.pos, [x.pos[0], x.pos[2]]) < 40.0) {
                c.wait_until = c.wait_until.max(t + 0.5);
            }
        }
        // at the wheel of a ground vehicle with somewhere to go, nobody to shoot at and not waiting, it stays near one spot:
        // jammed. The driver's own unjamming (back up 2 s and go again) never got GC_Tatooine's AT-ST past the fence at the
        // Imperial base (a whole round on the spot); off and on foot, as on its own after its 25 s
        // The clock only runs while it tries, and stands still (not back to zero) while it shoots or waits near the same spot:
        // GC_Dantooine's T-3B sat 140 s against a hill with an enemy in sight every 10-20 s, and each one started the count again
        // (nor while its path is asked again: for that tick it has none, and DC_Operation_Bragg's BRDM-2 against the far bank,
        // asking every few seconds, never got off)
        let wheel = self.settings.jam && ins.seat == 0 && on_ground;
        let trying = wheel && (!c.walker().path.is_empty() || c.driver().swimming) && c.target().is_none() && c.wait_until <= t;
        let here = [ins.pos[0], ins.pos[2]];
        self.jammed = match self.jammed {
            Some((p, s, last)) if wheel && flat(ins.pos, p) < JAM_M => Some((p, if trying { s + (t - last) } else { s }, t)),
            _ if trying => Some((here, 0.0, t)),
            _ => None,
        };
        if self.jammed.is_some_and(|(_, s, _)| s > JAM_S) {
            self.jammed = None;
            c.get_off_now = true;
            self.gives_up = Some(ins.vehicle);
        }
    }

    /// In something that flies: the pilot's flight (landing, the drop), and getting off a landed helicopter or a vehicle with no driver.
    fn air_crew(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>, ins: &Inside) {
        let o = command.order;
        let t = v.time;
        let c = &mut self.body;
        // in a helicopter, landed (near the ground and almost still)
        let flies = ins.class.flies();
        let height = ins.pos[1] - v.ground(ins.pos[0], ins.pos[2]).unwrap_or(f32::MIN).max(v.water());
        // (still and high over the ground: perhaps on a dock or a deck; only then the line tests of `floor`)
        let still = ins.velocity.iter().map(|x| x * x).sum::<f32>() < 9.0;
        let landed = still && (height < LANDED_M || (flies && v.floor(ins.pos[0], ins.pos[2], ins.pos[1]).is_some_and(|f| ins.pos[1] - f < LANDED_M)));
        if flies && height > DROP_LEAST_UP_M {
            self.aloft_at = Some(t);
        }
        // in command of a transport helicopter: waits landed for its own, carries them and lands near the flag; once
        // empty and landed there, it gets off too and follows on foot with them
        c.land_at = None;
        c.pilot_mut().flight = None;
        // at the controls of something that flies, to the enemy the side has reported (if not, from flag to flag)
        c.pilot_mut().imposed_goal = command.air_target.filter(|_| flies && ins.seat == 0).map(|b| [b[0], b[2]]);
        c.pilot_mut().attack_run = true;
        c.pilot_mut().dogfight = self.settings.dogfight;
        c.pilot_mut().airship = true;
        c.pilot_mut().vtol = self.settings.vtol;
        engage(c.pilot_mut(), command);
        pass(c.pilot_mut(), command, v);
        // (also a follower that took the controls of its squad's leg aircraft: flying on its own it went off to another flag)
        if ins.class.hovers() && ins.seat == 0 && (matches!(o.role, Role::Drive(_) | Role::Leader) || command.leg_vehicle == Some(ins.vehicle)) && !command.urgent {
            let x = v.vehicle(ins.vehicle);
            let carrying = x.as_ref().is_some_and(|x| x.free_seats.len() + 1 < x.seats);
            if t < c.wait_until {
                c.pilot_mut().flight = Some(FlightOrder { to: ins.pos, end: FlightEnd::Land, hold: true });
            } else if let Some(m) = meta {
                let d = flat(ins.pos, [m[0], m[2]]);
                // the trip's air leg lands where it was chosen (before taking off: beside the flag, not on it), with or without
                // people; without a trip, whoever carries people lands on the straight line, short of the flag
                let leg = command.leg_landing.filter(|_| command.leg_vehicle == Some(ins.vehicle));
                if carrying || leg.is_some() {
                    let k = (HELI_LAND_M / d.max(1.0)).min(1.0);
                    let to = leg.unwrap_or([m[0] + (ins.pos[0] - m[0]) * k, m[1], m[2] + (ins.pos[2] - m[2]) * k]);
                    c.pilot_mut().flight = Some(FlightOrder { to, end: FlightEnd::Land, hold: false });
                }
                // TODO: once its people are off the pilot gets off too and follows them on foot; a player would fly back for
                // another squad or stay over them in support (the helicopter waits by the flag with nobody to use it)
                if !carrying && landed && d < HELI_GET_OFF_M && self.boarded.is_some_and(|s| t - s.1 > 20.0) {
                    c.get_off_now = true;
                    self.gives_up = Some(ins.vehicle);
                }
            }
        }
        // the trip's air leg in a plane: it flies over the point, above it, and everyone in it jumps there (the pilot too: its
        // squad goes on on foot, and a plane does not wait)
        if let Some(to) = command.leg_drop.filter(|_| ins.class == VehicleClass::Plane && command.leg_vehicle == Some(ins.vehicle)) {
            if ins.seat == 0 {
                let p = c.pilot_mut();
                p.flight = Some(FlightOrder { to: [to[0], to[1] + DROP_UP_M, to[2]], end: FlightEnd::Over, hold: false });
                p.imposed_goal = None;
                p.attack_run = false;
                p.dogfight = false;
            }
            let speed = ins.velocity[0].hypot(ins.velocity[2]);
            if flat(ins.pos, [to[0], to[2]]) < DROP_M + DROP_LEAD_S * speed && ins.pos[1] - to[1] > DROP_LEAST_UP_M {
                c.get_off_now = true;
                self.gives_up = Some(ins.vehicle);
            }
        }
        // from a helicopter landed near its goal anyone who does not pilot it gets off (even outside its seat)
        if ins.class.hovers() && ins.seat != 0 && landed && meta.is_some_and(|p| flat(ins.pos, [p[0], p[2]]) < HELI_GET_OFF_M) {
            c.get_off_now = true;
            self.gives_up = Some(ins.vehicle);
        }
        if ins.seat != 0 && c.seat_spot.is_some() {
            let sin = v.vehicle(ins.vehicle).is_some_and(|x| x.driver_free);
            if !sin {
                self.without_driver_since = None;
            } else if self.without_driver_since.is_none() {
                self.without_driver_since = Some(t);
            }
            // (from a helicopter it gets off when landed, near: in the air, jumping is killing yourself)
            let arrived = if flies { landed && meta.is_some_and(|p| flat(ins.pos, [p[0], p[2]]) < HELI_GET_OFF_M) } else { meta.is_some_and(|p| flat(ins.pos, [p[0], p[2]]) < GET_OFF_NEAR) };
            // (whoever boards without a driver waits for one to arrive, but not forever)
            let alone = self.without_driver_since.is_some_and(|s| t - s > 8.0) && (!flies || landed);
            if arrived || alone {
                c.get_off_now = true;
                self.gives_up = Some(ins.vehicle);
            }
        }
    }

    /// Off the vehicle it picks its weapon and avoids; then the body thinks; and it moves to its own seat if it fell in another.
    fn finish(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>) -> Vec<(usize, f32)> {
        let me = &v.me;
        let t = v.time;
        if me.inside.is_none() {
            self.choose_weapon(v);
            self.avoid(v, command, meta);
        }
        // boarded in another seat than its own (on entering you take the one nearest to hand): the passenger who has fallen to the wheel does not
        // set off and moves to its seat; the driver who has fallen in the back moves to the wheel
        let c = &mut self.body;
        let change = match (&me.inside, c.seat_spot) {
            (Some(ins), Some((j, s))) if j == ins.vehicle && s != ins.seat && (ins.seat == 0 || s == 0) => {
                // (at the wheel without being the driver: to its seat or, if taken, to another passenger one; if none is left, it drives itself)
                v.vehicle(j).and_then(|x| if x.free_seats.contains(&s) { Some(s) } else if s != 0 { x.free_seats.iter().copied().find(|&q| q != 0) } else { None })
            }
            _ => None,
        };
        if change.is_some_and(|s| s != 0) {
            c.wait_until = c.wait_until.max(t + 0.5);
        }
        let mut axes = c.think(v);
        if let Some(s) = change {
            axes.retain(|e| !(crate::entry::MENU_SELECT1..crate::entry::MENU_SELECT1 + 9).contains(&e.0) && e.0 != crate::entry::USE);
            axes.push((crate::entry::MENU_SELECT1 + s, 1.0));
        }
        self.go_round(v, &mut axes);
        self.react(v, &mut axes);
        self.trace(v, command, &axes);
        if crate::bots::fire_trace::on() {
            // (and the reaction turning it, if any: to a shot heard, to a hit)
            let action = match self.reaction.filter(|r| v.time < r.0) {
                Some(r) => format!("{:?}+{:?}", self.action, r.3),
                None => format!("{:?}", self.action),
            };
            self.body.trace_after(v, &action, &axes);
        }
        axes
    }

    fn trace(&self, v: &BotView, command: &Command, axes: &[(usize, f32)]) {
        let o = command.order;
        let me = &v.me;
        let t = v.time;
        if crate::bots::trace_shots() && me.inside.is_none() && axes.iter().any(|e| e.0 == axis::FIRE && e.1 > 0.0) {
            let b = self.body.target();
            let crouched = axes.iter().any(|e| e.0 == axis::CROUCH && e.1 > 0.0);
            let h = b.map(|b| (v.hidden(me.eye, b.1), v.hidden([me.eye[0], me.eye[1] - 0.4, me.eye[2]], b.1)));
            eprintln!("shot t={t:.2} p={} {:?} crouched {crouched} weapon {:?} d {:?} eye {:?} pos {:?} hidden {:?} cover {:?} following {} carries {:?} target_in_vehicle {:?} bullets {:?}", me.player, self.action, me.weapon, b.map(|b| b.2 as i32), me.eye.map(|x| (x * 10.0).round() / 10.0), me.pos.map(|x| (x * 10.0).round() / 10.0), h, self.under_cover.is_some(), self.following, me.items.iter().map(|i| i.1.as_str()).collect::<Vec<_>>(), b.and_then(|b| v.others().into_iter().find(|o| o.player == b.0)).map(|o| o.in_vehicle), me.rounds);
        }
        let c = &self.body;
        if crate::bots::trace_wounded() && me.health < HEALTH_LOW && (t * 30.0).round() as i64 % 30 == 0
            && let Some(s) = self.last
        {
            eprintln!("wounded t={t:.0} p={} {:?} health {:.2} urgent {} enemy {:?} since {:.0} medic {:?} cover {:?} break {} inside {}", me.player, self.action, me.health, s.urgent, s.enemy_m.map(|x| x as i32), s.since_enemy, s.medic_m.map(|x| x as i32), self.cover.map(|p| flat(me.pos, [p[0], p[2]]) as i32), c.break_contact, me.inside.is_some());
        }
        if crate::bots::trace_me() == Some(me.player) && (t * 30.0).round() as i64 % 15 == 0 {
            let pv = match o.role { Role::Drive(j) | Role::Passenger(j, _) => v.vehicle(j).map(|x| (x.template, x.pos.map(|q| q as i32), x.radius as i32, x.free_seats)), _ => None };
            eprintln!("me t={t:.1} pos {:?} own vehicle {:?} {:?} {:?} health {:.2} seat {:?} inside {:?} gives_up {:?} go_round {:?} veh {:?} wait {:.1} outside {} escort {:?} step {:?} path {} next {:?} run {:?} flight {:?} axes {:?}", me.pos.map(|x| x as i32), c.driver().vehicle.and_then(|j| v.vehicle(j).map(|x| (j, x.pos.map(|q| q as i32)))), o.role, self.action, me.health, c.seat_spot, me.inside.as_ref().map(|i| (i.vehicle, i.seat)), self.gives_up, self.detour, pv, (c.wait_until - t).max(0.0), command.passengers_out, self.escort, c.walker().point.map(|p| p.map(|x| x as i32)), c.walker().path.len(), c.walker().path.first().map(|p| p.map(|x| x as i32)), (c.walker().flight_until > t).then(|| (c.walker().flight.map(|x| (x * 100.0) as i32), (c.walker().flight_until - t) as f32)), c.pilot().flight.map(|f| (f.to.map(|x| x as i32), f.end)), axes);
            if let Some(s) = self.last.filter(|s| s.health < self.settings.params.health_medic) {
                eprintln!("   sit urgent {} enemy {:?} since {:.1} no_cover {} medic {:?} cover {:?} retreat {:?}", s.urgent, s.enemy_m, s.since_enemy, s.no_cover, s.medic_m, self.cover.map(|p| p.map(|x| x as i32)), self.retreat.map(|p| p.0.map(|x| x as i32)));
            }
        }
    }
}
