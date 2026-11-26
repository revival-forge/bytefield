//! What it adds to the body's skills: fixed guns, the weapon by target, the places it avoids, the flank and going round its vehicle.
use super::*;

impl Individual {
    /// B14: the fixed gun it goes to now, if any: a free one of its team (or nobody's) within [`FIXED_M`], not up high, with an enemy
    /// plane within [`FIXED_PLANE_M`] of it, or, the gun guarding one of its team's flags (`own`, within [`FIXED_FLAG_M`]), an
    /// enemy soldier in sight within [`FIXED_TARGET_M`]. An attacker does not stop at a machine gun on the way: it slows the attack down (the
    /// tournament lost ~8 tickets with any gun at hand). It keeps going to it for [`FIXED_GO_S`]; one it did not get into is not for this life.
    // TODO: bot_caps counts 33 boardings and 3 kills on the base-game census (its gate asks 50 boardings). Ways: an AA gun with an
    // enemy plane in sight farther than FIXED_PLANE_M, or a guard that waits at the gun without seeing anyone (B16 «still» would show it).
    pub(super) fn fixed_weapon(&mut self, v: &BotView, own: &[[f32; 3]]) -> Option<usize> {
        let me = &v.me;
        let t = v.time;
        let free = |j: usize| v.vehicle(j).filter(|x| x.fixed && x.driver_free && (x.team == 0 || x.team == me.team) && x.health > 0.3);
        if let Some((j, since)) = self.fixed {
            if t - since < FIXED_GO_S && free(j).is_some() {
                return Some(j);
            }
            self.fixed = None;
            self.fixed_vetoed.push(j);
        }
        if t < self.fixed_look_t {
            return None;
        }
        self.fixed_look_t = t + 1.0;
        let high = |x: &crate::bot_api::VehicleSeen| v.ground(x.pos[0], x.pos[2]).is_none_or(|g| x.pos[1] - g > crate::bots::body::driver::FIXED_HEIGHT);
        let weapons: Vec<crate::bot_api::VehicleSeen> = v.vehicles().into_iter().filter(|x| x.fixed && x.driver_free && (x.team == 0 || x.team == me.team) && x.health > 0.3 && !self.fixed_vetoed.contains(&x.vehicle) && flat(me.pos, [x.pos[0], x.pos[2]]) < FIXED_M && !high(x)).collect();
        let g = weapons.iter().min_by(|a, b| flat(me.pos, [a.pos[0], a.pos[2]]).total_cmp(&flat(me.pos, [b.pos[0], b.pos[2]])).then(a.vehicle.cmp(&b.vehicle)))?;
        let plane = v.others().iter().filter(|o| o.team != me.team).filter_map(|o| o.vehicle).filter_map(|k| v.vehicle(k)).any(|x| x.class.flies() && flat(g.pos, [x.pos[0], x.pos[2]]) < FIXED_PLANE_M);
        let keeps = own.iter().any(|f| flat(g.pos, [f[0], f[2]]) < FIXED_FLAG_M);
        // (the enemy in sight from the gun too, not only from where it stands: from a nest behind a wall it would shoot the wall)
        let from_weapon = [g.pos[0], g.pos[1] + FIXED_EYE, g.pos[2]];
        if !plane && !(keeps && enemy_visible(v, FIXED_TARGET_M).is_some_and(|e| !v.hidden(from_weapon, [e[0], e[1] + CHEST, e[2]]))) {
            return None;
        }
        self.fixed = Some((g.vehicle, t));
        Some(g.vehicle)
    }

    /// The weapon according to target. The anti-tank looks first at the ground vehicles in range and fires rockets at them; at a
    /// soldier on foot it fires with the pistol (the rocket, only if nothing else is left). Whoever carries grenades throws them more often at a
    /// vehicle nearby. Changing weapon costs: the wish must last [`WEAPON_WISH_S`] and what was asked is kept [`WEAPON_KEEP_S`].
    // TODO(perf, low value): the weapon classes come from string searches on every call (here, `is_anti_tank`, `team::kind`);
    // keeping them per kit template would save 3-5 % of a step on GC_Endor (~0.06 ms), not worth it unless they grow
    pub(super) fn choose_weapon(&mut self, v: &BotView) {
        let me = &v.me;
        let t = v.time;
        let shooter = self.body.shooter_mut();
        if !self.settings.melee_weapon {
            shooter.prefers_vehicle_m = 0.0;
            shooter.wanted_slot = None;
            shooter.p_grenade = crate::bots::body::shooter::P_GRENADE;
            return;
        }
        let at = me.items.iter().find(|i| weapons::is_anti_tank(&i.1)).map(|i| i.0);
        let pistol = me.items.iter().find(|i| weapons::is_pistol(&i.1)).map(|i| i.0);
        let in_hand = crate::bots::body::shooter::slot_in_hand(me);
        if in_hand.is_some() && in_hand == pistol && me.rounds.0 + me.rounds.1 <= 0 {
            self.pistol_empty_until = t + 60.0;
        }
        shooter.prefers_vehicle_m = if at.is_some() { VEHICLE_AT_M } else { 0.0 };
        let target = shooter.target;
        let on_ground = |o: &Other| v.ground(o.pos[0], o.pos[2]).is_none_or(|h| o.pos[1] - h < 6.0);
        let vehicle = target.and_then(|b| v.others().into_iter().find(|o| o.player == b.0)).is_some_and(|o| o.in_vehicle && on_ground(&o));
        shooter.p_grenade = if vehicle && at.is_none() { P_GRENADE_VEHICLE } else { crate::bots::body::shooter::P_GRENADE };
        // (from the rocket to the pistol against a soldier, at once: it is the first thing whoever sees a soldier with a rocket in hand does)
        let wish = match target {
            Some(_) if vehicle => at,
            Some(_) if at.is_some() && in_hand == at && t >= self.pistol_empty_until => pistol,
            // (with the pistol in hand and a soldier ahead, it carries on with it)
            Some(_) if at.is_some() && in_hand == pistol && t >= self.pistol_empty_until => pistol,
            _ => None,
        };
        if wish != self.weapon_wish.0 {
            self.weapon_wish = (wish, t);
        }
        let firm = t - self.weapon_wish.1 >= WEAPON_WISH_S as f64 || (self.weapon_wish.0.is_some() && self.weapon_wish.0 == pistol && in_hand == at);
        if firm && self.weapon_wish.0 != self.requested_weapon.0 && (self.requested_weapon.0.is_none() || t - self.requested_weapon.1 >= WEAPON_KEEP_S as f64) {
            self.requested_weapon = (self.weapon_wish.0, t);
        }
        shooter.wanted_slot = self.requested_weapon.0;
        // with nobody ahead, back to the rocket (DC's pistol is not known by the body as a pistol)
        if target.is_none() && self.requested_weapon.0.is_none() && at.is_some() && in_hand.is_some() && in_hand == pistol && !shooter.requesting_weapon(v) && (t * 2.0).fract() < 0.05 {
            shooter.request(v, at.unwrap_or(weapons::PRIMARY_SLOT));
        }
    }

    /// Does not enter where two or more of its own have just fallen: the path avoids those places (except one at its goal or
    /// where it already is). If one appears near the path it follows, it thinks it again.
    pub(super) fn avoid(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>) {
        let me = &v.me;
        let mut now: [Option<[f32; 2]>; 4] = [None; 4];
        if self.settings.flanking {
            let mut n = 0;
            for p in command.dangers.iter().flatten() {
                let at_goal = meta.is_some_and(|m| (m[0] - p[0]).hypot(m[2] - p[1]) < AVOID_FREE_M);
                if !at_goal && flat(me.pos, *p) > AVOID_FREE_M {
                    now[n] = Some(*p);
                    n += 1;
                }
            }
        }
        if now == self.avoiding {
            return;
        }
        let cam = self.body.walker_mut();
        let new_on_the_way = now.iter().flatten().any(|p| !self.avoiding.contains(&Some(*p)) && cam.path.iter().any(|c| (c[0] - p[0]).hypot(c[2] - p[1]) < 12.0));
        cam.avoid = now.iter().flatten().copied().collect();
        if new_on_the_way {
            cam.replan();
        }
        self.avoiding = now;
    }

    /// If it is one of those who follow the leader (half the squad, by number), and the squad goes for an enemy flag closer than
    /// [`FLANK_START_M`], it goes first to a point [`FLANK_M`] from the flag, at ±[`FLANK_DEGREES`] from the side the
    /// leader arrives by; once past it, it enters the flag. Returns the point while it goes to it.
    pub(super) fn flank_move(&mut self, v: &BotView, command: &Command) -> Option<[f32; 3]> {
        let me = &v.me;
        let (Role::Follow(j), Goal::Flag(i)) = (command.order.role, command.order.goal) else {
            self.flank = None;
            return None;
        };
        let flags = v.flags();
        let f = flags.get(i)?;
        let far = flat(me.pos, [f.pos[0], f.pos[2]]);
        // (`bots:flank2`: the flanker its side names flanks by its own spot; the half by number still flanks as before: taking that half
        // away lost 3 of 16 rounds, -86 tickets on DC_Al_Nas)
        let named = self.settings.flank2 && command.flank.is_some_and(|x| x.flag == i);
        let parity = !named && me.player % 2 == 0;
        if !self.settings.flanking || f.team == me.team || me.inside.is_some() || parity || command.urgent || far > FLANK_START_M + 60.0 {
            self.flank = None;
            return None;
        }
        if self.flank.is_none_or(|x| x.0 != i) {
            if far > FLANK_START_M {
                return None;
            }
            let leader = v.others().into_iter().find(|x| x.player == j && x.team == me.team)?;
            if named {
                // (no flank spot: it does not flank, it goes with its squad)
                let p = command.flank.and_then(|fk| flank2_point(v, self.body.maps().foot(v), f.pos, leader.pos, &fk));
                self.flank = Some((i, p.unwrap_or(f.pos), p.is_none()));
                self.flank_t = v.time;
                let (_, p, done) = self.flank?;
                return (!done).then_some(p);
            }
            let a = (leader.pos[2] - f.pos[2]).atan2(leader.pos[0] - f.pos[0]);
            if self.settings.flank_shuffle && self.sw.is_none() && self.shuffle.is_none() {
                self.shuffle = Some((crate::bots::body::rng::Rng::new(0x5348 ^ me.player as u64), crate::bots::streetwise::Filtered::default()));
            }
            let side_ = match (self.sw.as_mut(), self.shuffle.as_mut()) {
                (Some((_, rng, f)), _) | (None, Some((rng, f))) => if f.coin(rng) { 1.0 } else { -1.0 },
                (None, None) => if (me.player / 2) % 2 == 0 { 1.0 } else { -1.0 },
            };
            let water = v.water();
            let point = [1.0f32, 0.7].iter().find_map(|k| {
                let b = a + side_ * k * FLANK_DEGREES.to_radians();
                let (x, z) = (f.pos[0] + FLANK_M * b.cos(), f.pos[2] + FLANK_M * b.sin());
                v.ground(x, z).filter(|h| *h > water + 0.3).map(|h| [x, h, z])
            });
            let Some(p) = point else {
                // (no room on that side: with the leader)
                self.flank = Some((i, f.pos, true));
                return None;
            };
            self.flank = Some((i, p, false));
            self.flank_new = true;
            self.flank_t = v.time;
        }
        let (_, p, done) = self.flank?;
        if done {
            return None;
        }
        // (`bots:cover-flank`: it waits where it is for a mate of its squad at a firing spot, or a while)
        if self.settings.cover_flank && !command.covered && v.time - self.flank_t < COVER_WAIT_S {
            if cover_trace() && (v.time * 30.0).round() as i64 % 30 == 0 {
                eprintln!("cover: t={:.0} player {} waits to flank flag {i} ({:.0} s)", v.time, me.player, v.time - self.flank_t);
            }
            self.waits = Some(crate::bot_api::Wait::Cover);
            return Some(me.pos);
        }
        if cover_trace() && self.settings.cover_flank && (v.time * 30.0).round() as i64 % 30 == 0 {
            eprintln!("cover: t={:.0} player {} flanks flag {i}, covered {}", v.time, me.player, command.covered);
        }
        if flat(me.pos, [p[0], p[2]]) < 10.0 || self.body.target().is_some_and(|b| b.2 < 25.0) {
            self.flank = Some((i, p, true));
            return None;
        }
        Some(p)
    }

    /// A shot with no target: looks towards the enemy it sees or, if it sees nobody, steps away running.
    /// Going to its seat, stuck close to the vehicle and not advancing (the entrance is on the other side and it pushes against the hull): slides
    /// sideways without ceasing to look at the entrance, away from the vehicle's centre, and if in a few seconds it does not manage, to the other side.
    /// The same against another vehicle in its way.
    pub(super) fn go_round(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>) {
        let me = &v.me;
        let seat_spot = self.body.seat_spot.filter(|_| me.inside.is_none() && self.body.target().is_none());
        let Some(x) = seat_spot.and_then(|(j, _)| v.vehicle(j)) else {
            self.detour = None;
            return;
        };
        let still = v.time - self.last_step.0 > GO_ROUND_STILL_S;
        let pushes = axes.iter().any(|e| e.0 == axis::THROTTLE && e.1 > 0.0);
        // what it pushes against: its own vehicle or, on the way to it, another one in front (tanks parked side by side at a spawn)
        let near = |y: &VehicleSeen| flat(me.pos, [y.pos[0], y.pos[2]]) < y.radius + GO_ROUND_M;
        let obstacle = if near(&x) {
            Some(x.pos)
        } else if (still && pushes) || self.detour.is_some() {
            let heading = [x.pos[0] - me.pos[0], x.pos[2] - me.pos[2]];
            v.vehicles()
                .into_iter()
                .filter(|y| y.vehicle != x.vehicle && near(y) && (y.pos[0] - me.pos[0]) * heading[0] + (y.pos[2] - me.pos[2]) * heading[1] > 0.0)
                .min_by(|a, b| flat(me.pos, [a.pos[0], a.pos[2]]).total_cmp(&flat(me.pos, [b.pos[0], b.pos[2]])))
                .map(|y| y.pos)
        } else {
            None
        };
        let stuck_close = obstacle.is_some();
        let t = v.time;
        match self.detour {
            None if stuck_close && still && pushes => self.detour = Some((t, 0.0)),
            Some((since, 0.0)) if t - since > GO_ROUND_WAIT_S => {
                let e = obstacle.unwrap_or(x.pos);
                let centre = angle_between(me.view[2], [e[0] - me.pos[0], e[2] - me.pos[2]]);
                self.detour = Some((t, if centre > 0.0 { -1.0 } else { 1.0 }));
            }
            Some((since, l)) if l != 0.0 && t - since > GO_ROUND_SIDE_S => self.detour = Some((t, -l)),
            Some(_) if !stuck_close => self.detour = None,
            _ => {}
        }
        if let Some((_, l)) = self.detour.filter(|r| r.1 != 0.0) {
            axes.retain(|e| e.0 != axis::YAW);
            axes.push((axis::YAW, l));
        }
    }
}
