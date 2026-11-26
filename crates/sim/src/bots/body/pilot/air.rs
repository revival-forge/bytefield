//! A plane in the air, by phases: the fight with what flies, the attack run, the ring over the goal, what it looks at, the height
//! it wants, aiming and firing, what is ahead of it, and the controls. Each phase reads the craft's [`Attitude`] and leaves what
//! the next ones need; they run in this order every tick.
use super::*;

impl Pilot {
    /// In the air ([`Pilot::fly`], once off the ground).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fly_air(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, a: &Attitude, player: usize) {
        if self.dogfight_takes_over(v, i, axes, others, rng, shooter, a, player) {
            return;
        }
        // B23: an attack run: pulling away (straight, climbing) or coming in on the target (what it aims at is fixed for the run)
        let pass = if let Some(FlightOrder { end: FlightEnd::Pass { target, .. }, .. }) = self.flight { Some(target) } else { self.run = None; None };
        let bombs = pass.is_some() && i.alt_fire == AltFire::Drops;
        let mut meta = self.air_goal(v, rng, a.pos, a.f);
        let forced = match pass {
            Some(tg) => self.attack_run(v, a, others, tg, bombs, &mut meta, player),
            None => None,
        };
        let in_run = matches!(self.run, Some(Run { phase: RunPhase::In, .. }));
        let mut heading_d = self.heading_to(a, meta, pass.is_some(), player);
        self.look(v, a, others, rng, shooter, forced, player);
        let mut goal_alt = self.goal_altitude(a, axes, shooter, bombs && in_run);
        let (diving, lead) = self.aim_and_fire(v, a, axes, others, shooter, bombs && in_run, in_run, &mut heading_d, &mut goal_alt, player);
        let boards = self.clearance(v, a, &mut heading_d);
        self.controls(v, a, axes, shooter, diving, bombs && in_run, goal_alt, boards, heading_d, lead);
    }

    /// An enemy aircraft in sight, or one on its tail: the fight rules (not while just off the runway). Whether it took the
    /// controls this tick.
    #[allow(clippy::too_many_arguments)]
    fn dogfight_takes_over(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, a: &Attitude, player: usize) -> bool {
        let Attitude { pos, s, h, .. } = *a;
        if self.dogfight && h > MIN_HEIGHT {
            // (where its own are matters only to what cannot fight back)
            let refuge = if super::super::dogfight::fighter(i) { None } else { v.flags().iter().filter(|b| b.team == v.me.team).map(|b| b.pos).min_by(|a, b| flat(pos, [a[0], a[2]]).total_cmp(&flat(pos, [b[0], b[2]]))) };
            let ordered = self.flight.and_then(|o| if let FlightEnd::Engage { player } = o.end { Some(player) } else { None });
            let skill = super::super::dogfight::Skill { error_degrees: shooter.error_degrees, reaction_s: shooter.median_reaction, ordered, taken: &self.taken, refuge };
            if let Some(fight) = self.fight.fly(v, i, others, rng, &mut self.lock, &skill, &|x, z| Pilot::ground(v, x, z), player) {
                if trace_air() && v.time >= self.trace_t + 1.0 {
                    self.trace_t = v.time;
                    eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} dogfight lock {:?}", v.time, self.lock.map(|l| l.player));
                }
                axes.retain(|x| x.0 != axis::THROTTLE);
                axes.extend(fight);
                return true;
            }
        } else {
            self.lock = None;
        }
        false
    }

    /// B23: the attack run on `tg` in course: where it heads (`meta`) and what it aims at, which rules over what it sees (`Some`).
    #[allow(clippy::too_many_arguments)]
    fn attack_run(&mut self, v: &BotView, a: &Attitude, others: &[Other], tg: [f32; 3], bombs: bool, meta: &mut [f32; 2], player: usize) -> Option<Option<Target>> {
        let Attitude { pos, vel, s, f, h, .. } = *a;
        let forced;
        let aim = others.iter().filter(|o| o.team != v.me.team && flat(o.pos, [tg[0], tg[2]]) < PASS_FIND_M).min_by(|a, b| (!a.in_vehicle).cmp(&!b.in_vehicle).then(flat(a.pos, [tg[0], tg[2]]).total_cmp(&flat(b.pos, [tg[0], tg[2]]))));
        let at = aim.map_or(tg, |o| o.pos);
        let d = flat(pos, [at[0], at[2]]);
        let ahead_deg = angle_between(f, [at[0] - pos[0], at[2] - pos[2]]);
        // where it pulls away to: straight on if that stays inside the play area with room to turn back, else the nearest heading
        // that does (Kursk's planes flew out of it and died)
        let area = v.combat_area();
        let inside = |p: [f32; 2], m: f32| p[0] > area[0] + m && p[0] < area[0] + area[2] - m && p[1] > area[1] + m && p[1] < area[1] + area[3] - m;
        let away = |vel: [f32; 3]| {
            let n = vel[0].hypot(vel[2]).max(1.0);
            let d0 = [vel[0] / n, vel[2] / n];
            let turned = |g: f32| {
                let (sn, c) = g.to_radians().sin_cos();
                [d0[0] * c - d0[1] * sn, d0[0] * sn + d0[1] * c]
            };
            let ends = |d: [f32; 2]| [at[0] + d[0] * PASS_OUT_M, at[2] + d[1] * PASS_OUT_M];
            [0.0f32, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0, 120.0, -120.0, 150.0, -150.0, 180.0].iter().map(|&g| turned(g)).find(|&d| inside(ends(d), PASS_EDGE_M)).unwrap_or(d0)
        };
        let run = match self.run {
            Some(r) if flat(r.target, [tg[0], tg[2]]) < 300.0 => self.run.insert(Run { target: tg, ..r }),
            _ => self.run.insert(Run { target: tg, phase: if d > 0.7 * PASS_OUT_M { RunPhase::In } else { RunPhase::Out { dir: away(vel) } }, since: v.time, dropped: f64::MIN, aligned: false, best_miss: f32::MAX }),
        };
        match run.phase {
            RunPhase::Out { dir } => {
                *meta = [pos[0] + dir[0] * 1000.0, pos[2] + dir[1] * 1000.0];
                forced = Some(None);
                let gone = v.time - run.since;
                let edge = !inside([pos[0], pos[2]], PASS_EDGE_M);
                if (gone > PASS_OUT_LEAST_S && (d > PASS_OUT_M || edge)) || gone > PASS_OUT_MOST_S {
                    *run = Run { phase: RunPhase::In, since: v.time, aligned: false, best_miss: f32::MAX, ..*run };
                }
            }
            RunPhase::In => {
                *meta = [at[0], at[2]];
                run.aligned |= ahead_deg.abs() < PASS_ALIGNED;
                let done = d < PASS_OVER_M || (run.aligned && ahead_deg.abs() > PASS_BEHIND && d < PASS_BEHIND_M) || (run.dropped > run.since && v.time - run.dropped > PASS_AFTER_DROP_S);
                if done {
                    if trace_air() {
                        eprintln!("air t={:.0} {player} run ends: {} at {d:.0} m off {ahead_deg:.0} h {h:.0}", v.time, if run.dropped > run.since { "dropped" } else if d < PASS_OVER_M { "over" } else { "behind" });
                    }
                    *run = Run { phase: RunPhase::Out { dir: away(vel) }, since: v.time, ..*run };
                    forced = Some(None);
                } else {
                    let d3 = (0..3).map(|k| (at[k] - pos[k]).powi(2)).sum::<f32>().sqrt();
                    forced = Some(Some((aim.map_or(usize::MAX, |o| o.player), at, d3)));
                }
            }
        }
        if trace_air() && v.time >= self.trace_t + 1.0 {
            self.trace_t = v.time;
            eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} pass {:?} target {d:.0} m off {ahead_deg:.0} seen {} bombs {bombs}", v.time, run.phase, aim.is_some());
        }
        forced
    }

    /// The turn it wants towards `meta`, degrees.
    fn heading_to(&self, a: &Attitude, meta: [f32; 2], pass: bool, player: usize) -> f32 {
        let Attitude { pos, f, .. } = *a;
        let d_goal = flat(pos, meta);
        let mut d_xz = [meta[0] - pos[0], meta[1] - pos[2]];
        // over the flag it circles: heading for a point on a ring ahead. Ordered over a point, straight at it, again and again; but
        // while it is too far below it to climb what is left on the way in, it climbs on a wide ring around it (what holds a flag up
        // in the air is in the way of whoever comes in below it)
        let over = if let Some(FlightOrder { to, end: FlightEnd::Over, .. }) = self.flight { Some(to[1] - pos[1]) } else { None };
        let ring = match over {
            None if pass => None,
            None => (d_goal < 300.0).then_some(250.0),
            Some(up) => (up > OVER_LOW_M && d_goal < OVER_CLIMB * up).then_some(OVER_RING_M),
        };
        if let Some(r) = ring {
            let t = [-d_xz[1] / d_goal.max(1.0), d_xz[0] / d_goal.max(1.0)];
            let g = if player.is_multiple_of(2) { 1.0 } else { -1.0 };
            d_xz = [t[0] * g * r + d_xz[0] * (d_goal - r) / d_goal.max(1.0), t[1] * g * r + d_xz[1] * (d_goal - r) / d_goal.max(1.0)];
        }
        angle_between(f, d_xz)
    }

    /// What it sees: the nearest enemy of what is ahead of it, unless the attack run fixed what it aims at (`forced`).
    #[allow(clippy::too_many_arguments)]
    fn look(&mut self, v: &BotView, a: &Attitude, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, forced: Option<Option<Target>>, player: usize) {
        let Attitude { pos, s, f, h, .. } = *a;
        // (what is behind or right below cannot be attacked: if it looked at it, it would not see what it does have ahead)
        let ahead: Vec<Other> = others.iter().filter(|o| o.team == v.me.team || (angle_between(f, [o.pos[0] - pos[0], o.pos[2] - pos[2]]).abs() < AIR_CONE && flat(o.pos, [pos[0], pos[2]]) > 60.0)).cloned().collect();
        shooter.see(v, rng, [pos[0], pos[1] - 1.0, pos[2]], AIR_RANGE, &ahead);
        if let Some(t) = forced {
            shooter.target = t;
        }
        if trace_air() && shooter.target.is_none() && v.time >= self.trace_t + 1.0 {
            self.trace_t = v.time;
            let in_range = |o: &&Other| o.team != v.me.team && (0..3).map(|k| (o.pos[k] - pos[k]).powi(2)).sum::<f32>() < SHOT_RANGE * SHOT_RANGE;
            eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} no target: enemies in range {}, ahead {}", v.time, others.iter().filter(in_range).count(), ahead.iter().filter(in_range).count());
        }
    }

    /// The height it wants: its cruise over what is below; ordered over a point, that point's if higher; coming in with bombs
    /// (`bomb_run`), level over the target.
    fn goal_altitude(&self, a: &Attitude, axes: &mut Vec<(usize, f32)>, shooter: &Shooter, bomb_run: bool) -> f32 {
        let Attitude { pos, s, below, .. } = *a;
        let mut goal_alt = below + CRUISE_HEIGHT;
        // (ordered over a point: at its height, if that is higher: a platform up in the air)
        if let Some(FlightOrder { to, end: FlightEnd::Over, .. }) = self.flight {
            goal_alt = goal_alt.max(to[1]);
            // (and slowly as it gets there: whoever jumps over it keeps the plane's speed)
            if flat(pos, [to[0], to[2]]) < OVER_SLOW_M && s > OVER_SLOW * V_LOST {
                axes.retain(|x| x.0 != axis::THROTTLE);
                axes.push((axis::THROTTLE, 0.0));
            }
        }
        // (pulling away it climbs back to its cruise; coming in with bombs, level over the target)
        if bomb_run {
            goal_alt = shooter.target.map_or(goal_alt, |b| (b.1[1] + PASS_BOMB_UP).max(below + MIN_HEIGHT + 10.0));
        }
        goal_alt
    }

    /// With a target: how it moves, the lead its shot needs, diving on it (the turn and height it wants), and the guns, missiles,
    /// rockets and bombs. Whether it dives at it, and the lead.
    #[allow(clippy::too_many_arguments)]
    fn aim_and_fire(&mut self, v: &BotView, a: &Attitude, axes: &mut Vec<(usize, f32)>, others: &[Other], shooter: &Shooter, bomb_run: bool, in_run: bool, heading_d: &mut f32, goal_alt: &mut f32, player: usize) -> (bool, [f32; 3]) {
        let Attitude { pos, vel, s, f, pit, h, .. } = *a;
        let mut diving = false;
        // where what it looks at is moving, seen from one time to the next (as anyone sees it); and how far ahead of it the shot must lead
        if let Some((p, at, _)) = shooter.target {
            match self.seen_target {
                Some((q, before, t)) if q == p && v.time - t >= 0.2 => {
                    let dt = (v.time - t) as f32;
                    self.target_speed = std::array::from_fn(|k| (at[k] - before[k]) / dt);
                    self.seen_target = Some((p, at, v.time));
                }
                Some((q, ..)) if q == p => {}
                _ => {
                    self.target_speed = [0.0; 3];
                    self.seen_target = Some((p, at, v.time));
                }
            }
        } else {
            self.seen_target = None;
        }
        let lead: [f32; 3] = shooter.target.map_or([0.0; 3], |b| std::array::from_fn(|k| (self.target_speed[k] - vel[k]) * b.2.min(SHOT_RANGE) / BULLET_SPEED));
        if let Some((_, at, d)) = shooter.target {
            let a: [f32; 3] = std::array::from_fn(|k| at[k] + lead[k] - pos[k]);
            let slope = a[1].atan2((a[0] * a[0] + a[2] * a[2]).sqrt()).to_degrees();
            let g_at = angle_between(f, [a[0], a[2]]);
            // it has it more or less ahead and can descend
            if bomb_run {
                // (the bomb keeps the plane's speed: it heads for where the target will be when it lands)
                let fall = ((2.0 * (pos[1] - at[1]).max(1.0)) / crate::GRAVITY as f32).sqrt();
                *heading_d = angle_between(f, [at[0] + self.target_speed[0] * fall - pos[0], at[2] + self.target_speed[2] * fall - pos[2]]);
            } else if g_at.abs() < 70.0 && slope > -55.0 && h > MIN_HEIGHT && v.time > self.dive_out {
                *heading_d = g_at;
                diving = true;
                *goal_alt = at[1];
            }
            // (it fires if the burst passes near it, not if it goes more or less that way)
            let cone_ang = (BURST_RADIUS / d.max(1.0)).atan().to_degrees().max(1.5);
            let cone = g_at.abs() < cone_ang && (slope - pit).abs() < cone_ang + 2.0;
            if trace_air() && v.time >= self.trace_t + 1.0 {
                self.trace_t = v.time;
                eprintln!("air t={:.0} {player} h {h:.0} v {s:.0} target {d:.0} m turn {g_at:.0} pitch {:.0} (slope {slope:.0} nose {pit:.0}) cone {cone_ang:.1} ready {:.1} diving {}", v.time, slope - pit, (shooter.ready_at - v.time).max(0.0), v.time > self.dive_out);
            }
            if cone && d < SHOT_RANGE && v.time >= shooter.ready_at {
                self.burst_until = v.time + 0.4;
            }
            // missiles at what flies, if it carries any: what the secondary key carries is used from afar if it is not a bomb
            let in_the_air_the = Pilot::target_flies(v, others, shooter.target);
            // (only if it fires them: with bombs on that key, a bomber in a dogfight dropped its whole load into the sea)
            let alt = v.me.inside.as_ref().map_or(AltFire::None, |i| i.alt_fire);
            // rockets at what goes by land, from closer and with the nose on it (they fly straight, as the machine guns)
            let reach = if in_the_air_the { 800.0 } else { 500.0 };
            if alt == AltFire::Fires && g_at.abs() < 8.0 && (slope - pit).abs() < 12.0 && d < reach && v.time >= self.missile_at {
                axes.push((axis::ALT_FIRE, 1.0));
                self.missile_at = v.time + if in_run { 0.5 } else { 3.0 };
            }
            // bombs: a bomb keeps moving forward as it falls, so it is dropped when where it would land is where the target is
            let drop = pos[1] - at[1];
            if alt == AltFire::Drops && !in_the_air_the && drop > 30.0 {
                // (the world's gravity, not the Earth's: with 9.81 every bomb fell ~18 % short of the target, 40-60 m)
                let (low, g) = (-vel[1], crate::GRAVITY as f32);
                let t = (low + (low * low + 2.0 * g * drop).sqrt()) / g;
                let impact = [pos[0] + vel[0] * t, pos[2] + vel[2] * t];
                let friend_near = others.iter().any(|o| o.team == v.me.team && flat(o.pos, impact) < FRIEND_BOMB_RADIUS);
                // (where the target will be when the bomb lands: a ship at 10 m/s goes 40 m in a bomb's fall)
                let there = [at[0] + self.target_speed[0] * t, at[1], at[2] + self.target_speed[2] * t];
                let miss = flat(there, impact);
                // (coming in, the nearest it gets, if near enough)
                let passed = match self.run.as_mut().filter(|r| r.phase == RunPhase::In && r.dropped < r.since) {
                    Some(r) => {
                        let gone_by = miss > r.best_miss + 2.0 && r.best_miss < PASS_LOOSE_M;
                        r.best_miss = r.best_miss.min(miss);
                        gone_by
                    }
                    None => false,
                };
                if (miss < BOMB_RADIUS || passed) && !friend_near {
                    axes.push((axis::ALT_FIRE, 1.0));
                    if let Some(r) = self.run.as_mut().filter(|r| r.phase == RunPhase::In) {
                        if r.dropped < r.since {
                            r.dropped = v.time;
                        }
                    }
                }
            }
        }
        if v.time < self.burst_until {
            axes.push((axis::FIRE, 1.0));
        }
        (diving, lead)
    }

    /// What is ahead rules over height: the angle it must climb at to clear each point ahead (it turns 45° aside when that is too
    /// steep, `heading_d`), and something on the map ahead ([`Pilot::obstacle`]).
    fn clearance(&mut self, v: &BotView, a: &Attitude, heading_d: &mut f32) -> f32 {
        let Attitude { pos, vel, s, .. } = *a;
        let ahead = |t: f32, turn_val: f32| {
            let (gs, gc) = turn_val.to_radians().sin_cos();
            let dir = [vel[0] * gc + vel[2] * gs, vel[2] * gc - vel[0] * gs];
            Pilot::ground(v, pos[0] + dir[0] * t, pos[2] + dir[1] * t)
        };
        let is_needed = |turn_val: f32| {
            [1.0f32, 2.0, 3.0, 5.0, 8.0, 11.0].iter().map(|t| (ahead(*t, turn_val) + MIN_HEIGHT - pos[1]).atan2(s.max(20.0) * t).to_degrees()).fold(f32::MIN, f32::max)
        };
        let mut boards = is_needed(0.0);
        // too steep: better to one side
        if boards > 22.0 {
            let (a, b) = (is_needed(45.0), is_needed(-45.0));
            if a < boards.min(b) {
                *heading_d = 45.0;
                boards = a;
            } else if b < boards {
                *heading_d = -45.0;
                boards = b;
            }
        }
        // something on the map ahead, seen without more ado
        if v.time >= self.look_ahead {
            self.look_ahead = v.time + 0.2;
            self.obstacle = v.hidden(pos, [pos[0] + vel[0] * 3.0, pos[1] + vel[1] * 3.0, pos[2] + vel[2] * 3.0]);
        }
        boards
    }

    /// The sticks: the pitch and bank it wants from the height and turn it wants, what is ahead and its speed, and a law that
    /// corrects by watching what the plane does.
    #[allow(clippy::too_many_arguments)]
    fn controls(&mut self, v: &BotView, a: &Attitude, axes: &mut Vec<(usize, f32)>, shooter: &Shooter, diving: bool, bomb_run: bool, mut goal_alt: f32, boards: f32, heading_d: f32, lead: [f32; 3]) {
        let Attitude { pos, s, pit, kind, dpit, drol, below, h, .. } = *a;
        let diving = diving && boards < -3.0 && !self.obstacle;
        if !diving && shooter.target.is_some() && !bomb_run && pos[1] < goal_alt.max(below + MIN_HEIGHT) {
            self.dive_out = v.time + 6.0;
            goal_alt = below + CRUISE_HEIGHT;
        }
        let alt_d = if diving { goal_alt } else { goal_alt.max(pos[1] + 0.0_f32.min(boards)) };
        let err_alt = alt_d - pos[1];
        let mut pit_d = if diving {
            let a = shooter.target.map_or([0.0; 3], |b| std::array::from_fn(|k| b.1[k] + lead[k] - pos[k]));
            a[1].atan2((a[0] * a[0] + a[2] * a[2]).sqrt()).to_degrees().clamp(-45.0, 20.0)
        } else {
            (err_alt * 0.25).clamp(-12.0, 22.0)
        };
        let mut kind_d = -(heading_d * 1.2).clamp(-BANK_MAX, BANK_MAX);
        // hugging what is ahead: up, with the wings level
        if boards > 0.0 || self.obstacle {
            pit_d = pit_d.max((boards + 6.0).min(35.0));
            if boards > 15.0 || self.obstacle {
                pit_d = pit_d.max(30.0);
                kind_d = kind_d.clamp(-20.0, 20.0);
            }
        }
        // near the ground, and more the faster it runs, it does not turn tightly: a tight turn loses height and does not let it climb
        let just = 40.0 + 1.2 * s;
        if h < just {
            let cap = (BANK_MAX * (h / just).max(0.4)).max(20.0);
            kind_d = kind_d.clamp(-cap, cap);
        }
        // low speed: nose down, and wings level
        if s < V_LOST {
            pit_d = pit_d.min(-8.0);
            kind_d = kind_d.clamp(-15.0, 15.0);
        }
        // the controls' authority grows with the square of the speed
        let k = (45.0 / s.max(20.0)).powi(2).clamp(0.3, 3.0);
        // turning, you have to pull more not to lose height
        let extra = 8.0 * (1.0 - kind.to_radians().cos().abs()) * (kind.to_radians().cos() > 0.0) as i32 as f32;
        let cab = k * (0.05 * (pit_d - pit + extra) - 0.02 * dpit);
        let ala = k * (0.02 * (kind - kind_d) + 0.006 * drol);
        axes.push((PITCH, PITCH_SIGN * cab.clamp(-1.0, 1.0)));
        axes.push((BANK, ala.clamp(-1.0, 1.0)));
        axes.push((axis::YAW, (0.03 * heading_d).clamp(-0.4, 0.4)));
    }
}
