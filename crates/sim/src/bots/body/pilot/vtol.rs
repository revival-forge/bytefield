//! Taking off and landing straight up: the climb out of a spot with obstacles, its legs and the way out.
use super::*;

impl Pilot {
    /// At the controls of a helicopter: the throttle raises and lowers the collective little by little, so vertical speed is commanded;
    /// to advance the nose is tilted. First of all, not crashing.
    /// `land`: if present, it goes to that point instead of to a flag, and on arrival (closer than [`LAND_M`]) it descends to what is below
    /// (the ground, or a dock or deck it is over: [`BotView::floor`]) and
    /// stays (bots: whoever carries passengers drops them near the flag; whoever waits for them does not take off). Without it, as usual.
    #[allow(clippy::too_many_arguments)]
    // (B11/B23: a plane with bombs or rockets attacks what goes by land or sea in runs, [`FlightEnd::Pass`]; `BYTEFIELD_BOTS_AIR=1`,
    // with a line per run's end, shows it.)
    /// bots: a VTOL takeoff ([`Pilot::vtol`]), for a craft its throttle lifts straight up ([`crate::bot_api::Inside::lift`]: GC's
    /// fighters, the Falcon). It needs no runway: it rises a few metres off where it stands (`VTOL_SLIDE_HEIGHT`, under the ceiling
    /// if there is one), slides out at that height towards the nearest open sky (sliding, not turning: hanging still, the yaw does not
    /// turn it, while the pitch and roll sticks tilt the lift racks and move it along and across), goes straight up under the open sky
    /// and then puts the throttle forward. The walls are what a craft hits ([`BotView::craft_clear`]: `hidden` does not see a
    /// hangar's walls from inside). What it looks at it looks at once, when it rises (16 ways out, about 500 rays), and then the sky
    /// overhead four times a second. Stuck for `VTOL_GIVE_UP_S`, or with no way out, it takes the runway as a plane does.
    /// TODO: the Falcon in GC_Hoth's hangar (`BYTEFIELD_VTOL_FINE=1 BYTEFIELD_BOTS_AIR=1 examples/air_aim data/game GCMOD GC_Hoth 52
    /// --seat all`, player 24) gets out in ~35 s scraping the walls all the way (it is 16 m to each side under an 11.8 m ceiling, and
    /// what lifts it pushes it ahead), then banks over in the climb with the roll stick hard over and crashes: its slide and its
    /// level-holding are guesses (`air_envelope … milleniumfalcon --hover`: the pitch stick drives it along, the roll stick barely
    /// moves it); it crashed before this too.
    pub(super) fn vtol_takeoff(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, a: &Attitude, player: usize) {
        let Attitude { pos, vel, f, r, pit, kind, dpit, drol, .. } = *a;
        let up = i.lift;
        let sky = |p: [f32; 3]| v.craft_clear(p, [p[0], p[1] + VTOL_SKY_M, p[2]]) >= VTOL_SKY_M - 0.5;
        let span = Pilot::vtol_half_span(v, i);
        // (open over all of it: its middle, its wing tips and its tail, as it slides along `dir`)
        let sky_over = |dir: [f32; 2]| {
            let side = [dir[1], -dir[0]];
            sky(pos)
                && sky([pos[0] - dir[0] * VTOL_TAIL_M, pos[1], pos[2] - dir[1] * VTOL_TAIL_M])
                && [-span, span].iter().all(|w| sky([pos[0] + side[0] * w, pos[1], pos[2] + side[1] * w]))
        };
        let st = *self.takeoff.get_or_insert_with(|| {
            let overhead = v.craft_clear([pos[0], pos[1] + 1.0, pos[2]], [pos[0], pos[1] + 1.0 + VTOL_SKY_M, pos[2]]);
            let phase = if overhead >= VTOL_SKY_M - 0.5 { VtolPhase::Climb { dir: [f[0], f[2]] } } else { VtolPhase::Rise };
            if trace_air() {
                eprintln!("air t={:.0} {player} vtol start {} #{} at {:.0},{:.0},{:.0} overhead {overhead:.1} m, half span {:.1}: {phase:?}", v.time, i.template, i.vehicle, pos[0], pos[1], pos[2], Pilot::vtol_half_span(v, i));
            }
            let rest = (pos[1] - v.floor(pos[0], pos[2], pos[1]).unwrap_or(pos[1])).clamp(0.0, 4.0);
            Vtol { phase, floor: pos[1], slide: (0.5 * overhead).clamp(1.5, VTOL_SLIDE_HEIGHT), start: [pos[0], pos[2]], best: 0.0, best_at: v.time, look_sky: 0.0, under: pos[1], rest, blocked_since: None, tries: 0, higher: 0.0 }
        });
        // hanging at a height by the throttle, all or nothing (half of it may not lift at all)
        let hold = |y: f32| if pos[1] + 0.8 * vel[1] < y { up } else { 0.0 };
        // sliding along `dir` at `speed` (0: holding still), as a helicopter: hanging, the pitch stick puts the nose down and it goes
        // ahead, the roll stick banks it and it goes to that side (the racks tilt with them); left alone it pitches up by itself
        let slide = |axes: &mut Vec<(usize, f32)>, dir: [f32; 2], speed: f32| {
            let n = |x: f32, z: f32| {
                let l = x.hypot(z).max(1e-3);
                [x / l, z / l]
            };
            let (fwd, side) = (n(f[0], f[2]), n(r[0], r[2]));
            let err = [dir[0] * speed - vel[0], dir[1] * speed - vel[2]];
            let (e_fwd, e_side) = (err[0] * fwd[0] + err[1] * fwd[1], err[0] * side[0] + err[1] * side[1]);
            if up > 0.0 {
                // (what the throttle pushed lifts, the Falcon: the pitch stick drives it along without tilting it much; it is held level)
                axes.push((PITCH, (0.3 * e_fwd + PITCH_SIGN * (0.06 * -pit - 0.03 * dpit)).clamp(-1.0, 1.0)));
                axes.push((BANK, (0.3 * e_side + 0.06 * kind + 0.02 * drol).clamp(-1.0, 1.0)));
                return;
            }
            let pit_d = (-1.5 * e_fwd).clamp(-VTOL_TILT, VTOL_TILT);
            let bank_d = (1.5 * e_side).clamp(-VTOL_TILT, VTOL_TILT);
            axes.push((PITCH, PITCH_SIGN * (0.06 * (pit_d - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
            axes.push((BANK, (0.06 * (kind + bank_d) + 0.02 * drol).clamp(-1.0, 1.0)));
        };
        let trace = |what: &str| {
            if trace_air() {
                eprintln!("air t={:.0} {player} vtol {what} {} h {:.1} v {:.0}", v.time, i.template, pos[1] - st.floor, (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt());
            }
        };
        let mut next = st;
        match st.phase {
            VtolPhase::Rise => {
                axes.push((axis::THROTTLE, hold(st.floor + st.slide)));
                slide(axes, [0.0, 0.0], 0.0);
                if pos[1] >= st.floor + st.slide - 0.5 {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, 0, &mut next.tries);
                    (next.start, next.best, next.best_at) = ([pos[0], pos[2]], 0.0, v.time);
                    trace(&format!("{:?}", next.phase));
                } else if v.time - st.best_at > VTOL_GIVE_UP_S {
                    next.phase = VtolPhase::Failed;
                    trace("cannot rise");
                }
            }
            VtolPhase::Out { dir, until, leg } => {
                axes.push((axis::THROTTLE, hold(st.floor.max(st.under) + st.slide)));
                slide(axes, dir, if st.blocked_since.is_some() { 0.0 } else { VTOL_SLIDE });
                let gone = (pos[0] - st.start[0]) * dir[0] + (pos[2] - st.start[1]) * dir[1];
                if gone > st.best + 1.0 {
                    (next.best, next.best_at) = (gone, v.time);
                }
                if v.time >= st.look_sky {
                    next.look_sky = v.time + 0.25;
                    // (what it stands over: the floor as it rose, then what is below it and 4 m ahead, as high as it is)
                    let top = pos[1] + 1.0;
                    let ahead = [pos[0] + dir[0] * 4.0, pos[2] + dir[1] * 4.0];
                    next.under = v.floor(pos[0], pos[2], top).into_iter().chain(v.floor(ahead[0], ahead[1], top)).fold(f32::MIN, f32::max) + st.rest;
                    // a craft with someone at the controls ahead in its way (one parked was left out of the way out)
                    let side = [dir[1], -dir[0]];
                    let in_the_way = v.vehicles().iter().any(|o| {
                        let a = [o.pos[0] - pos[0], o.pos[2] - pos[2]];
                        let (t, w, r) = (a[0] * dir[0] + a[1] * dir[1], (a[0] * side[0] + a[1] * side[1]).abs(), o.radius.min(15.0));
                        // (of two crafts in each other's way, the one with the higher number waits)
                        o.vehicle < i.vehicle && !o.driver_free && !o.fixed && o.class.flies() && t > -r && t < 10.0 + r && w < span + r && (o.pos[1] - pos[1]).abs() < 6.0 + r
                    });
                    next.blocked_since = if in_the_way { st.blocked_since.or(Some(v.time)) } else { None };
                    if in_the_way {
                        next.best_at = v.time;
                    }
                    if sky_over(dir) {
                        next.phase = VtolPhase::Climb { dir };
                        trace("open sky");
                    }
                }
                // waited long enough: another way out from here
                if next.phase == st.phase && next.blocked_since.is_some_and(|b| v.time - b > VTOL_WAIT_S) {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, leg, &mut next.tries);
                    (next.start, next.best, next.best_at, next.blocked_since) = ([pos[0], pos[2]], 0.0, v.time, None);
                    trace(&format!("waited; {:?}", next.phase));
                }
                // the end of a leg that does not reach the sky: the way out again from here
                if next.phase == st.phase && until.is_some_and(|u| gone >= u) {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, leg, &mut next.tries);
                    (next.start, next.best, next.best_at) = ([pos[0], pos[2]], 0.0, v.time);
                    trace(&format!("{:?}", next.phase));
                }
                if next.phase == st.phase && v.time - next.best_at > VTOL_GIVE_UP_S {
                    next.phase = VtolPhase::Failed;
                    trace("stuck sliding out");
                }
            }
            VtolPhase::Climb { dir } => {
                // (over the ground where it is and where the nose will take it once the throttle is forward)
                let ahead = [50.0f32, 100.0, 150.0].iter().map(|d| Pilot::ground(v, pos[0] + f[0] * d, pos[2] + f[2] * d) - 0.2 * d).fold(f32::MIN, f32::max);
                let top = Pilot::ground(v, pos[0], pos[2]).max(st.floor).max(ahead) + VTOL_CLIMB_M;
                axes.push((axis::THROTTLE, hold(top + st.higher)));
                slide(axes, dir, VTOL_SLIDE);
                // (and the way ahead of the nose clear for the run, counting what it sinks before its wings hold it; if not, higher)
                // (a craft that flies hanging flies from here: it never puts the throttle forward for its wings)
                if i.hangs && pos[1] >= top - 2.0 {
                    (self.in_the_air, self.hover.on) = (true, true);
                    trace("hanging");
                } else if pos[1] >= top - 2.0 + st.higher && v.time >= st.look_sky {
                    next.look_sky = v.time + 0.25;
                    let l = f[0].hypot(f[2]).max(1e-3);
                    let run = [pos[0] + f[0] / l * VTOL_RUN_M, pos[1] - VTOL_SINK_M, pos[2] + f[2] / l * VTOL_RUN_M];
                    if v.craft_clear(pos, run) >= VTOL_RUN_M - 1.0 || st.higher >= VTOL_HIGHER_M {
                        (next.phase, next.best_at) = (VtolPhase::Go, v.time);
                        trace("go");
                    } else {
                        next.higher = st.higher + 10.0;
                    }
                }
            }
            VtolPhase::Go => {
                let s = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
                let h = pos[1] - Pilot::ground(v, pos[0], pos[2]).max(st.floor);
                // sinking low before the wings hold it: the lift again for a moment (the throttle back is the lift, or forward is both)
                let sinking = up < 0.0 && h < 15.0 && vel[1] < -2.0;
                axes.push((axis::THROTTLE, if sinking { up } else { 1.0 }));
                // (something in its way in the next 3 s: it pulls up)
                let reach = [pos[0] + vel[0] * 3.0, pos[1] + vel[1] * 3.0, pos[2] + vel[2] * 3.0];
                let nose = if s > 10.0 && v.craft_clear(pos, reach) < 3.0 * s - 1.0 { 25.0 } else { 5.0 };
                axes.push((PITCH, PITCH_SIGN * (0.08 * (nose - pit) - 0.03 * dpit).clamp(-1.0, 1.0)));
                axes.push((BANK, (0.03 * kind + 0.01 * drol).clamp(-1.0, 1.0)));
                // (what the throttle pushed lifts climbs as long as it is pushed, and a gunship never goes much faster than 20 m/s)
                let flies = if up > 0.0 { s > 15.0 || v.time - st.best_at > 3.0 } else { s > V_LOST + 10.0 && vel[1] > -2.0 };
                if flies || v.time - st.best_at > 2.0 * VTOL_GIVE_UP_S {
                    self.in_the_air = true;
                    trace("flying");
                }
            }
            VtolPhase::Hold { until } => {
                axes.push((axis::THROTTLE, hold(st.floor.max(st.under) + st.slide)));
                slide(axes, [0.0, 0.0], 0.0);
                if v.time >= until {
                    next.phase = self.vtol_next_leg(v, i, st.floor + st.slide, 0, &mut next.tries);
                    (next.start, next.best, next.best_at, next.blocked_since) = ([pos[0], pos[2]], 0.0, v.time, None);
                    trace(&format!("again; {:?}", next.phase));
                }
            }
            VtolPhase::Failed => {}
        }
        if crate::debug::options().bots_air && crate::debug::options().bots_vtol_fine {
            let get = |k: usize| axes.iter().rev().find(|x| x.0 == k).map_or(0.0, |x| x.1);
            eprintln!("vtol t={:.2} {player} {} {:?} h {:.1} vy {:+.1} vh {:.1} pitch {:+.0} bank {:+.0} thr {:+.1} p {:+.2} b {:+.2}", v.time, i.template, st.phase, pos[1] - st.floor, vel[1], vel[0].hypot(vel[2]), pit, -kind, get(axis::THROTTLE), get(PITCH), get(BANK));
        }
        self.takeoff = Some(next);
    }

    /// Half the room a craft needs to each side when it slides, m.
    pub(super) fn vtol_half_span(v: &BotView, i: &Inside) -> f32 {
        v.vehicle(i.vehicle).map_or(VTOL_HALF_SPAN, |x| (VTOL_SPAN_OF_RADIUS * x.radius).max(VTOL_HALF_SPAN))
    }

    /// The next leg out after `leg` legs, at the height `y`: the way out ([`Pilot::vtol_way_out`]); with none, or with all its legs
    /// gone, it hangs a while and looks again from leg 1, and after `VTOL_TRIES` looks it gives up.
    pub(super) fn vtol_next_leg(&self, v: &BotView, i: &Inside, y: f32, leg: u8, tries: &mut u8) -> VtolPhase {
        *tries += 1;
        match self.vtol_way_out(v, i, y).filter(|_| leg < VTOL_LEGS) {
            Some((dir, until)) => VtolPhase::Out { dir, until, leg: leg + 1 },
            None if *tries < VTOL_TRIES => VtolPhase::Hold { until: v.time + VTOL_HOLD_S },
            None => VtolPhase::Failed,
        }
    }

    /// The way out of where a VTOL craft stands to the nearest open sky, at the height `y` it slides at (x, z): of 16 ways around
    /// it, the one that reaches open sky soonest, with room for its wings and no craft parked in the way. When none reaches it, the
    /// one with most room, as a leg to look again from (with how far to go along it); `None` with nowhere to go.
    pub(super) fn vtol_way_out(&self, v: &BotView, i: &Inside, y: f32) -> Option<([f32; 2], Option<f32>)> {
        let (pos, f) = (i.pos, i.rows[2]);
        let l = f[0].hypot(f[2]).max(1e-3);
        let (c, sn) = (f[2] / l, f[0] / l);
        let parked = v.vehicles();
        let mut best: Option<(f32, f32, [f32; 2])> = None;
        let mut roomiest: Option<(f32, [f32; 2])> = None;
        let span = Pilot::vtol_half_span(v, i);
        for k in 0..16 {
            let g = (k as f32 * 22.5).to_radians();
            let (gs, gc) = g.sin_cos();
            let dir = [sn * gc + c * gs, c * gc - sn * gs];
            let side = [dir[1], -dir[0]];
            let mut room = VTOL_OUT_M;
            for w in [-span, 0.0, span] {
                let a = [pos[0] + side[0] * w, y, pos[2] + side[1] * w];
                room = room.min(v.craft_clear(a, [a[0] + dir[0] * VTOL_OUT_M, y, a[2] + dir[1] * VTOL_OUT_M]));
            }
            for o in &parked {
                let a = [o.pos[0] - pos[0], o.pos[2] - pos[2]];
                let t = a[0] * dir[0] + a[1] * dir[1];
                if o.vehicle != i.vehicle && o.radius < 25.0 && t > 0.0 && t < room && (a[0] * dir[1] - a[1] * dir[0]).abs() < span + 0.5 * o.radius.min(15.0) && o.pos[1] + 0.4 * o.radius.min(15.0) > y - 2.0 {
                    room = t - 0.5 * o.radius.min(15.0);
                }
            }
            if crate::debug::options().bots_air && crate::debug::options().bots_vtol_fine {
                eprintln!("vtol way {} at {:.0},{:.0},{:.0} way {k} dir {:.2},{:.2} room {room:.1}", i.template, pos[0], y, pos[2], dir[0], dir[1]);
            }
            // (a leg back the way it came is no leg: within 45 degrees of where it came from, a craft goes back and forth)
            let back = (i.velocity[0] * dir[0] + i.velocity[2] * dir[1]) < -0.7 * i.velocity[0].hypot(i.velocity[2]) && i.velocity[0].hypot(i.velocity[2]) > 1.0;
            if !back && room >= 2.0 * VTOL_OUT_STEP && roomiest.is_none_or(|b| room > b.0) {
                roomiest = Some((room, dir));
            }
            let mut t = VTOL_OUT_STEP;
            while t < room {
                let p = [pos[0] + dir[0] * t, y, pos[2] + dir[1] * t];
                if v.craft_clear(p, [p[0], p[1] + VTOL_SKY_M, p[2]]) >= VTOL_SKY_M - 0.5 {
                    // (the nearest, and of two as near, the one closer to the nose)
                    let off = (k as f32 * 22.5).min(360.0 - k as f32 * 22.5);
                    if best.is_none_or(|b| t < b.0 || t == b.0 && off < b.1) {
                        best = Some((t, off, dir));
                    }
                    break;
                }
                t += VTOL_OUT_STEP;
            }
        }
        best.map(|b| (b.2, None)).or(roomiest.map(|(room, dir)| (dir, Some(room - VTOL_OUT_STEP))))
    }
}
