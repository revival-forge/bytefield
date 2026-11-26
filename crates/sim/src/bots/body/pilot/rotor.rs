//! What flies on rotors or lift: hovering, the helicopter and the airship.
use super::*;

impl Pilot {
    /// bots: at the controls of a craft that flies hanging by its lift ([`crate::bot_api::Inside::hangs`]: GC_Bespin's cloud car,
    /// the gunships, the skiff), once up ([`Pilot::vtol_takeoff`]). It flies as a helicopter: the throttle holds its height, all or
    /// nothing (half of it may not lift at all); the pitch and roll sticks tilt it, a little, and it goes along or to the side; the yaw
    /// turns it (the cloud car's only while it goes along). It goes from flag to flag at [`HOVER_HEIGHT`] over what is below and
    /// ahead (a platform too: [`BotView::floor`]), climbs and stops before a wall, and with someone in sight faces them, closes to
    /// [`HOVER_FIRE_M`] keeping some speed, holds the target's height when it flies, and fires when the nose is on it.
    /// TODO: in GC_Bespin it fights but hardly downs anything (B26: 5 % of its shots hit, ~0 downed a round), and late in a round
    /// several cloud cars circle one target and crash into each other or the city (3 in 240 s): `AIR_AIM_DAMAGE=1
    /// BYTEFIELD_BOTS_AIR=1 examples/air_aim data/game GCMOD GC_Bespin 240 --seat all` (`hover … target … blocked` each second, the
    /// crashes as `dmg … source Crash`). The gunships and the skiff climb after fighters to their height (`fly_at`) and never reach
    /// them: what flies fast may be left to what flies fast.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fly_hover(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, a: &Attitude, player: usize) {
        let Attitude { pos, vel, f, r, pit, kind, dpit, drol, .. } = *a;
        let up = i.lift;
        let land = match self.flight {
            Some(FlightOrder { hold: true, .. }) => Some(pos),
            Some(FlightOrder { to, end: FlightEnd::Land, .. }) => Some(to),
            _ => None,
        };
        let meta = match land {
            Some(p) => [p[0], p[2]],
            None => self.air_goal(v, rng, pos, f),
        };
        shooter.see(v, rng, [pos[0], pos[1] - 1.0, pos[2]], AIR_RANGE.min(500.0), others);
        let target = shooter.target.filter(|_| land.is_none());
        // where it goes along the ground, how fast, and where its nose looks
        let (mut dir, mut speed, mut face) = {
            let d = [meta[0] - pos[0], meta[1] - pos[2]];
            let l = d[0].hypot(d[1]).max(1e-3);
            ([d[0] / l, d[1] / l], (0.25 * l).min(HOVER_SPEED), [d[0] / l, d[1] / l])
        };
        let mut fly_at: Option<f32> = None;
        if let Some((who, at, d)) = target {
            let a = [at[0] - pos[0], at[1] - pos[1], at[2] - pos[2]];
            let l = a[0].hypot(a[2]).max(1e-3);
            let to = [a[0] / l, a[2] / l];
            face = to;
            if d > HOVER_FIRE_M {
                (dir, speed) = (to, (0.2 * (d - HOVER_RING_M)).clamp(HOVER_KEEP, HOVER_SPEED));
            } else {
                // within range it circles it at `HOVER_RING_M`, nose on it, and never stops (the cloud car turns only while it goes
                // along) nor rams it: two that closed on each other to a few metres crashed into each other
                let side = if (who + player) % 2 == 0 { 1.0 } else { -1.0 };
                let k = ((l - HOVER_RING_M) / HOVER_RING_M).clamp(-1.0, 1.0) * 1.5;
                let w = [side * -to[1] + k * to[0], side * to[0] + k * to[1]];
                let n = w[0].hypot(w[1]).max(1e-3);
                (dir, speed) = ([w[0] / n, w[1] / n], HOVER_KEEP + 4.0);
            }
            if Pilot::target_flies(v, others, Some((who, at, d))) {
                fly_at = Some(at[1]);
            }
            let slope = a[1].atan2(l).to_degrees();
            if angle_between(f, [a[0], a[2]]).abs() < 6.0 && (slope - pit).abs() < 15.0 && d < SHOT_RANGE && v.time >= shooter.ready_at {
                self.burst_until = v.time + 0.3;
            }
        }
        if v.time < self.burst_until {
            axes.push((axis::FIRE, 1.0));
        }
        // ---- what is below and ahead, four times a second: the height it keeps and whether a wall stands in the way
        if v.time >= self.hover.look_at {
            self.hover.look_at = v.time + 0.25;
            let top = pos[1] + 2.0;
            self.hover.under = v.floor(pos[0], pos[2], top).unwrap_or_else(|| Pilot::ground(v, pos[0], pos[2]));
            let mut high = self.hover.under;
            for t in [1.0f32, 3.0, 6.0] {
                let (x, z) = (pos[0] + vel[0] * t, pos[2] + vel[2] * t);
                high = high.max(v.floor(x, z, top + 10.0).unwrap_or(f32::MIN)).max(Pilot::ground(v, x, z));
            }
            // (and not below where it goes: GC_Bespin's city stands on platforms over a void, and a flight between them
            // keeps to their height)
            high = high.max(v.floor(meta[0], meta[1], pos[1].max(high) + 100.0).unwrap_or(f32::MIN));
            let reach = [pos[0] + vel[0] * HOVER_AHEAD_S, pos[1] + vel[1].min(0.0) * HOVER_AHEAD_S, pos[2] + vel[2] * HOVER_AHEAD_S];
            let s = (vel[0] * vel[0] + vel[2] * vel[2]).sqrt();
            self.hover.blocked = s > 3.0 && v.craft_clear(pos, reach) < s * HOVER_AHEAD_S - 1.0;
            self.hover.alt = high + HOVER_HEIGHT + if self.hover.blocked { 15.0 } else { 0.0 };
            if trace_air() && v.time >= self.trace_t + 1.0 {
                self.trace_t = v.time;
                eprintln!("air t={:.0} {player} hover {} h {:.0} over {:.0} v {s:.0} target {:?} blocked {}", v.time, i.template, pos[1] - high, high, target.map(|t| (t.0, t.2 as i32)), self.hover.blocked);
            }
        }
        // (over what is below it, and up to what it fights in the air)
        let alt = fly_at.map_or(self.hover.alt, |y| y.max(self.hover.alt - HOVER_HEIGHT + 8.0));
        if self.hover.blocked {
            speed = 0.0;
        }
        if land.is_some_and(|p| flat(pos, [p[0], p[2]]) < LAND_M) {
            speed = 0.0;
        }
        axes.push((axis::THROTTLE, if pos[1] + 0.8 * vel[1] < alt { up } else { 0.0 }));
        // ---- the sticks: tilting it along `dir` at `speed`, as a helicopter
        let n = |x: f32, z: f32| {
            let l = x.hypot(z).max(1e-3);
            [x / l, z / l]
        };
        let (fwd, side) = (n(f[0], f[2]), n(r[0], r[2]));
        let err = [dir[0] * speed - vel[0], dir[1] * speed - vel[2]];
        let (e_fwd, e_side) = (err[0] * fwd[0] + err[1] * fwd[1], err[0] * side[0] + err[1] * side[1]);
        let pit_d = (-1.0 * e_fwd).clamp(-HOVER_TILT, HOVER_TILT);
        let bank_d = (1.0 * e_side).clamp(-HOVER_TILT, HOVER_TILT);
        axes.push((PITCH, PITCH_SIGN * (0.06 * (pit_d - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
        axes.push((BANK, (0.06 * (kind + bank_d) + 0.02 * drol).clamp(-1.0, 1.0)));
        // ---- the nose (the turn does not stop by itself: it is braked before arriving)
        let heading_d = angle_between(f, face);
        let rum = f[0].atan2(f[2]).to_degrees();
        let drum = self.previous_heading.map_or(0.0, |(t, q)| if v.time > t { wraps(rum - q) / (v.time - t) as f32 } else { 0.0 });
        self.previous_heading = Some((v.time, rum));
        axes.push((axis::YAW, (heading_d / 50.0 - drum / 40.0).clamp(-1.0, 1.0)));
        // landed (put down, or fallen onto something): it takes off again from there
        if pos[1] - self.hover.under < 4.0 && vel[0].hypot(vel[2]) < 3.0 && land.is_none() {
            (self.in_the_air, self.hover.on) = (false, false);
            self.takeoff = None;
        }
    }

    pub fn fly_heli(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, others: &[Other], rng: &mut Rng, shooter: &mut Shooter, land: Option<[f32; 3]>) {
        let Attitude { pos, vel, f, pit, kind, dpit, drol, below, h, .. } = self.attitude(v, i);
        // (an ordered landing, or waiting landed where it is)
        let land = land.or(match self.flight {
            Some(FlightOrder { hold: true, .. }) => Some(pos),
            Some(FlightOrder { to, end: FlightEnd::Land, .. }) => Some(to),
            _ => None,
        });
        let meta = match land {
            Some(p) => [p[0], p[2]],
            None => self.air_goal(v, rng, pos, f),
        };
        let d_goal = flat(pos, meta);
        let mut heading_d = angle_between(f, [meta[0] - pos[0], meta[1] - pos[2]]);
        let land_on = land.is_some() && d_goal < LAND_M;
        // what it would stand on, counting the level's objects (a dock, a deck, a platform): where it lands and what it takes off from
        let floor = if land_on || h < 60.0 { v.floor(pos[0], pos[2], pos[1]).unwrap_or(below) } else { below };
        // what is ahead rules over height, and so does a point to land on up on something (it comes in over it, not into it)
        let mut high = below;
        if let Some(p) = land.filter(|_| d_goal < 400.0) {
            high = high.max(p[1]);
        }
        for t in [1.0, 2.0, 4.0, 7.0, 10.0] {
            high = high.max(Pilot::ground(v, pos[0] + vel[0] * t, pos[2] + vel[2] * t));
        }
        for d in [40.0, 80.0] {
            high = high.max(Pilot::ground(v, pos[0] + f[0] * d, pos[2] + f[2] * d));
        }
        // what it sees and fires at: only what is in front of the nose
        shooter.see(v, rng, [pos[0], pos[1] - 1.0, pos[2]], AIR_RANGE.min(500.0), others);
        let mut halt = d_goal < if land.is_some() { 60.0 } else { 120.0 };
        let mut aim_nose: Option<f32> = None;
        if land_on {
            heading_d = 0.0;
        }
        if let Some((_, at, d)) = shooter.target.filter(|_| land.is_none()) {
            let a = [at[0] - pos[0], at[1] - pos[1], at[2] - pos[2]];
            let g_at = angle_between(f, [a[0], a[2]]);
            heading_d = g_at;
            halt = d < 200.0;
            let slope = a[1].atan2((a[0] * a[0] + a[2] * a[2]).sqrt()).to_degrees();
            if self.attack_run {
                // at firing distance it stops and points the nose (without dropping below the safety height: the collective controls that)
                halt = d < HELI_FIRE_M;
                aim_nose = Some(slope.clamp(-25.0, 5.0));
                let aligned = g_at.abs() < 8.0 && (slope - pit).abs() < 12.0 && d < HELI_FIRE_M + 100.0 && v.time >= shooter.ready_at;
                if aligned {
                    self.burst_until = v.time + 0.4;
                    let in_vehicle = others.iter().any(|o| shooter.target.is_some_and(|b| b.0 == o.player) && o.in_vehicle);
                    if in_vehicle && v.time >= self.missile_at {
                        axes.push((axis::ALT_FIRE, 1.0));
                        self.missile_at = v.time + 1.5;
                    }
                }
            } else if g_at.abs() < 6.0 && (slope - pit).abs() < 25.0 && d < SHOT_RANGE && v.time >= shooter.ready_at {
                self.burst_until = v.time + 0.3;
            }
        }
        if v.time < self.burst_until {
            axes.push((axis::FIRE, 1.0));
        }
        // ---- height
        let alt_d = if land_on { floor } else if self.attack_run { high + HELI_ATTACK_HEIGHT } else { high + HELI_HEIGHT };
        let vy_d = ((alt_d - pos[1]) * 0.35).clamp(-5.0, 12.0);
        // just taking off, straight up
        let taking_off = pos[1] - floor < 12.0 && !land_on;
        axes.push((axis::THROTTLE, ((vy_d - vel[1]) * 0.25).clamp(-1.0, 1.0)));
        // ---- actitud
        let va = vel[0].hypot(vel[2]);
        let pit_d = if land_on {
            // landing: brakes completely
            (va * 0.8).clamp(-4.0, 12.0)
        } else if taking_off || vel[1] < -6.0 || heading_d.abs() > 40.0 {
            0.0
        } else if let Some(m) = aim_nose.filter(|_| halt && va < 6.0) {
            // stopped facing the target: nose towards it
            m
        } else if halt {
            // brakes: nose up, according to how fast it runs
            (va * 0.5).clamp(0.0, 12.0)
        } else if va < HELI_SPEED {
            -HELI_NOSE
        } else {
            -2.0
        };
        let kind_d = if taking_off { 0.0 } else { -(heading_d * 0.35).clamp(-18.0, 18.0) };
        axes.push((PITCH, PITCH_SIGN * (0.04 * (pit_d - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
        axes.push((BANK, (0.03 * (kind - kind_d) + 0.02 * drol).clamp(-1.0, 1.0)));
        // (the turn does not stop by itself: it must be braked before arriving)
        let rum = f[0].atan2(f[2]).to_degrees();
        let drum = self.previous_heading.map_or(0.0, |(t, q)| if v.time > t { wraps(rum - q) / (v.time - t) as f32 } else { 0.0 });
        self.previous_heading = Some((v.time, rum));
        axes.push((axis::YAW, (heading_d / 50.0 - drum / 40.0).clamp(-1.0, 1.0)));
    }

    /// bots: at the controls of an airship ([`crate::bot_api::VehicleClass::Airship`]): it hangs by itself and does not bank. The nose stays level, the
    /// yaw turns it, the throttle pushes it along (backwards too: that is its brake) and the pitch stick takes it up and down
    /// (`examples/air_envelope … cloudbus --step`). It goes to the ordered point or to `land`, else from flag to flag like a plane, at
    /// a height over what is ahead and over the point; it lands on what is below the point ([`BotView::floor`]: a platform). It does not
    /// fire: GC_Bespin's carries a squad.
    pub fn fly_airship(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, rng: &mut Rng, land: Option<[f32; 3]>) {
        let (pos, vel, f) = (i.pos, i.velocity, i.rows[2]);
        let below = Pilot::ground(v, pos[0], pos[2]);
        let land = land.or(match self.flight {
            Some(FlightOrder { hold: true, .. }) => Some(pos),
            Some(FlightOrder { to, end: FlightEnd::Land, .. }) => Some(to),
            _ => None,
        });
        // (a target its side reported is for what fires)
        let imposed = self.imposed_goal.take();
        let meta = match land {
            Some(p) => [p[0], p[2]],
            None => self.air_goal(v, rng, pos, f),
        };
        self.imposed_goal = imposed;
        let d_goal = flat(pos, meta);
        let land_on = land.is_some() && d_goal < AIRSHIP_LAND_M;
        let heading_d = if land_on { 0.0 } else { angle_between(f, [meta[0] - pos[0], meta[1] - pos[2]]) };
        let floor = if land_on || pos[1] - below < 60.0 { v.floor(pos[0], pos[2], pos[1]).unwrap_or(below) } else { below };
        // ---- height: over what is ahead and, near it, over the point it goes to
        let mut high = below;
        for t in [2.0, 5.0, 10.0] {
            high = high.max(Pilot::ground(v, pos[0] + vel[0] * t, pos[2] + vel[2] * t));
        }
        // (what floats over a city high up, with the ground far below, keeps to the height of where it goes, or else to its own)
        high = high.max(land.map_or(pos[1] - AIRSHIP_HEIGHT, |p| p[1]));
        let mut alt_d = if land_on { floor.max(land.map_or(floor, |p| p[1])) } else { high + AIRSHIP_HEIGHT };
        // ---- speed along it: slowing down near the point, and barely while it turns
        let along = vel[0] * f[0] + vel[1] * f[1] + vel[2] * f[2];
        let mut speed_d = if land_on {
            0.0
        } else if heading_d.abs() > AIRSHIP_TURN {
            3.0
        } else if land.is_some() {
            (d_goal * AIRSHIP_BRAKE).min(AIRSHIP_SPEED)
        } else {
            AIRSHIP_SPEED
        };
        // something in the way (a building, the city): it stops and climbs over it
        let flat_f = [f[0], 0.0, f[2]];
        let n = (flat_f[0] * flat_f[0] + flat_f[2] * flat_f[2]).sqrt().max(1e-3);
        let look = (along.max(0.0) * AIRSHIP_LOOK_S).max(AIRSHIP_LOOK_M);
        let at = |m: f32| [pos[0] + flat_f[0] / n * m, pos[1], pos[2] + flat_f[2] / n * m];
        if !land_on && v.hidden(at(AIRSHIP_HULL_M), at(AIRSHIP_HULL_M + look)) {
            speed_d = speed_d.min(2.0);
            alt_d = alt_d.max(pos[1] + 15.0);
        }
        axes.push((axis::THROTTLE, ((speed_d - along) * 0.2).clamp(-1.0, 1.0)));
        // the pitch stick, positive: down
        let vy_d = ((alt_d - pos[1]) * 0.3).clamp(if land_on { -3.0 } else { -6.0 }, 10.0);
        axes.push((PITCH, (-(vy_d - vel[1]) * 0.3).clamp(-1.0, 1.0)));
        // (the turn does not stop by itself: it must be braked before arriving)
        let rum = f[0].atan2(f[2]).to_degrees();
        let drum = self.previous_heading.map_or(0.0, |(t, q)| if v.time > t { wraps(rum - q) / (v.time - t) as f32 } else { 0.0 });
        self.previous_heading = Some((v.time, rum));
        axes.push((axis::YAW, (heading_d / 30.0 - drum / 20.0).clamp(-1.0, 1.0)));
    }
}
