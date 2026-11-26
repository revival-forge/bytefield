//! On the ground: the runway it picks (wherever it can run without crashing), the run and the rotation; it gives up stuck.
use super::*;

impl Pilot {
    /// On the ground (or a deck), the run to take off. Returns whether it is advisable to jump (stuck for a while).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn taxi(&mut self, v: &BotView, i: &Inside, axes: &mut Vec<(usize, f32)>, maps: &Arc<Maps>, a: &Attitude) -> bool {
        let Attitude { pos, vel, s, f, pit, kind, dpit, drol, below, .. } = *a;
        // the runway: wherever you can run without crashing, as close as possible to where it is looking
        // (a craft that lifts off by itself does not come here with bots: [`Pilot::vtol_takeoff`]; a run out of a hangar does not
        // work: from GC_Hoth's every way out is shut within 40-95 m at the belly's height and the run needs ~45 m to rotate)
        let eye = [pos[0], pos[1] + 2.0, pos[2]];
        if self.runway.is_none() {
            let (c, sn) = (f[2] / f[0].hypot(f[2]).max(1e-3), f[0] / f[0].hypot(f[2]).max(1e-3));
            let seen_ones = v.vehicles();
            let maps = maps.clone();
            // on a deck the map says nothing: what is below is not where it goes
            let under_cover_now = pos[1] - below > 5.0;
            let grid = if under_cover_now { None } else { maps.tank(v) };
            // the metres that can be run in that direction without crashing
            let free = |g: f32| -> f32 {
                let (gs, gc) = g.to_radians().sin_cos();
                let dir = [sn * gc + c * gs, c * gc - sn * gs];
                let side_ = [dir[1], -dir[0]];
                let mut l = RUNWAY_LENGTH;
                if let Some(m) = grid {
                    let mut t = 12.0;
                    while t < RUNWAY_LENGTH {
                        let mut with_nothing = true;
                        for w in [-4.0f32, 0.0, 4.0] {
                            let (x, z) = (pos[0] + dir[0] * t + side_[0] * w, pos[2] + dir[1] * t + side_[1] * w);
                            // (what is under water is blocked on the map, but you fly over it)
                            with_nothing &= m.clear(x, z) || Pilot::ground(v, x, z) < pos[1] - 6.0;
                        }
                        if !with_nothing {
                            l = t;
                            break;
                        }
                        t += 4.0;
                    }
                }
                // (what the map does not say, the view does; also to the sides, which the wings take up)
                // (on a deck it leaves straight ahead, and to the sides is the ship itself)
                let wides: &[f32] = if under_cover_now { &[0.0] } else { &[0.0, -WING_SPAN, WING_SPAN] };
                for &w in wides {
                    let a = [eye[0] + side_[0] * w, eye[1], eye[2] + side_[1] * w];
                    if v.hidden(a, [a[0] + dir[0] * l, a[1], a[2] + dir[1] * l]) {
                        l = l.min(40.0);
                    }
                }
                // neither the map nor other vehicles in between (parked ones are not reported by `hidden`)
                for o in &seen_ones {
                    let a = [o.pos[0] - pos[0], o.pos[2] - pos[2]];
                    let t = a[0] * dir[0] + a[1] * dir[1];
                    if o.vehicle != i.vehicle && o.radius < 25.0 && (6.0..l).contains(&t) && (a[0] * dir[1] - a[1] * dir[0]).abs() < 4.0 + 0.5 * o.radius.min(15.0) && (o.pos[1] - pos[1]).abs() < 8.0 {
                        l = t;
                    }
                }
                l
            };
            let candidates = [0.0f32, 15.0, -15.0, 30.0, -30.0, 45.0, -45.0, 60.0, -60.0, 90.0, -90.0, 120.0, -120.0, 150.0, -150.0, 180.0];
            let mut best = (0.0f32, f32::MIN);
            for g in candidates {
                let l = free(g);
                if l >= RUNWAY_LENGTH {
                    best = (g, l);
                    break;
                }
                if l > best.1 + 1.0 {
                    best = (g, l);
                }
            }
            let (gs, gc) = best.0.to_radians().sin_cos();
            self.runway = Some([sn * gc + c * gs, c * gc - sn * gs]);
        }
        let runway = self.runway.unwrap_or([f[0], f[2]]);
        let e = angle_between(f, runway);
        // (some tail wheels turn the opposite way to the rudder: it watches what it does and corrects)
        if v.time >= self.look_wheel {
            if let Some((t, before)) = self.wheel
                && v.time > t
                && s > 2.0
                && e.abs() > before.abs() + 0.5
                && e.abs() > 3.0
            {
                self.wheel_sign = -self.wheel_sign;
            }
            self.wheel = Some((v.time, e));
            self.look_wheel = v.time + 0.5;
        }
        let sign = if s < 20.0 { self.wheel_sign } else { 1.0 };
        axes.push((axis::YAW, sign * (e / 15.0).clamp(-1.0, 1.0)));
        // with something ahead it does not manage to take off: better to brake to turn
        if e.abs() > 25.0 {
            axes.retain(|x| x.0 != axis::THROTTLE);
            axes.push((axis::THROTTLE, if s > 8.0 { 0.0 } else { 0.5 }));
        }
        // an obstacle or a dead end: after a while it gives up
        if s < 2.0 {
            let since = *self.stopped_since.get_or_insert(v.time);
            if v.time - since > 25.0 {
                return true;
            }
        } else {
            self.stopped_since = None;
        }
        let ahead_obstacle = s > 12.0 && v.hidden(eye, [eye[0] + vel[0] * 2.5, eye[1] + vel[1] * 2.5, eye[2] + vel[2] * 2.5]);
        let throws = s > V_ROTATE || (ahead_obstacle && s > V_LOST);
        let cab = if throws { 0.08 * ((if ahead_obstacle { 25.0 } else { 15.0 }) - pit) - 0.03 * dpit } else { 0.0 };
        axes.push((PITCH, PITCH_SIGN * cab.clamp(-1.0, 1.0)));
        axes.push((BANK, (0.03 * kind + 0.01 * drol).clamp(-1.0, 1.0)));
        return false;
    }
}
