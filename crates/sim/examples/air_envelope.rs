//! How an aircraft answers its stick, measured from outside: one aircraft already in the air over the middle of the map, flown by
//! a script through the same axes a player has. The craft is put back over the middle every frame (the sea is the same everywhere),
//! so a run can last as long as it needs.
//!   air_envelope <game> <mod> <level> <template>... [--chase] [--hover] [--verbose]
//! Per template: the level speed at full, half and no throttle; the roll rate with the stick hard over; the sustained level turn (the
//! sharpest bank in 30..82° that holds its height, at full and at half throttle; `--verbose`: every bank); the climb with the nose
//! pulled to 20° at full throttle (the best rise over 4 s, and what it keeps over the last 5 s).
//! `--chase`: can it be pointed anywhere? A pursuit that rolls until the target is over the nose and pulls, against a target flying
//! a level circle (or straight): how long until the guns bear, and how much of the time they keep bearing.
//! `--hover`: does it lift off from rest by itself, and with what throttle (a VTOL engine may answer the throttle pulled back), and
//! what pitch, roll and yaw do to it while it hangs over the ground.
//! `--cruise <throttle>`: a craft that only hangs by the throttle (the cloud car): held at 40 m by that throttle, all or nothing, with
//! the nose held down 10..45°, how fast it goes; then, going along, what turns it: the yaw held, a bank held, and both.
use bytefield_sim::soldier::axis;

type V3 = [f32; 3];
const PITCH: usize = 1;
const BANK: usize = 2;
/// Height the runs start at, over the ground at the middle of the map, m; below `FLOOR` over it a run is over.
const START: f32 = 900.0;
const FLOOR: f32 = 120.0;
const DT: f32 = 1.0 / 30.0;

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let l = len(a).max(1e-6);
    [a[0] / l, a[1] / l, a[2] / l]
}
fn angle(a: V3, b: V3) -> f32 {
    dot(unit(a), unit(b)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// One aircraft in the air with someone at its controls.
struct Rig<'w> {
    s: bytefield_sim::Sim<'w>,
    craft: usize,
    soldier: usize,
    ground: f32,
    before: [V3; 3],
    /// pitch, yaw and roll rates seen from one frame to the next, °/s: nose up, nose right, right wing down
    rates: V3,
    /// the stick it has learnt to hold to keep the nose where it wants it
    trim: f32,
    /// a fighting pilot's hands
    stick: bytefield_sim::bots::body::dogfight::Stick,
    t: f32,
}

impl<'w> Rig<'w> {
    fn new(w: &'w bytefield_sim::World, template: &str, speed: f32) -> Option<Rig<'w>> {
        let mut s = bytefield_sim::Sim::new(w, DT as f64);
        s.round.playing = true;
        let ground = w.height_at(1024.0, 1024.0)? as f32;
        let at = [1024.0, ground + START, 1024.0];
        let craft = s.spawn_vehicle(template, at, [0.0; 3])?;
        let soldier = s.spawn_vehicle("USSoldier", [at[0] + 30.0, at[1], at[2]], [0.0; 3])?;
        s.vehicles[soldier].team = 1;
        s.enter(soldier, craft, 0);
        let b = &mut s.vehicles[craft].craft.body;
        b.vel = [0.0, 0.0, speed];
        let before = b.rows;
        Some(Rig { s, craft, soldier, ground, before, rates: [0.0; 3], trim: 0.0, stick: Default::default(), t: 0.0 })
    }

    fn pos(&self) -> V3 {
        self.s.vehicles[self.craft].craft.body.pos
    }
    fn rows(&self) -> [V3; 3] {
        self.s.vehicles[self.craft].craft.body.rows
    }
    fn vel(&self) -> V3 {
        self.s.vehicles[self.craft].craft.body.vel
    }
    fn speed(&self) -> f32 {
        len(self.vel())
    }
    fn height(&self) -> f32 {
        self.pos()[1] - self.ground
    }
    fn alive(&self) -> bool {
        self.height() > FLOOR && self.s.vehicles[self.craft].armor.as_ref().is_none_or(|a| !a.destroyed)
    }

    /// One frame with these axes held. Returns how far it went over the map: it is put back over the middle.
    fn step(&mut self, axes: &[(usize, f32)]) -> [f32; 2] {
        self.s.vehicles[self.soldier].soldier_mut().unwrap().set_input(axes);
        self.s.step();
        self.t += DT;
        let b = &mut self.s.vehicles[self.craft].craft.body;
        let moved = [b.pos[0] - 1024.0, b.pos[2] - 1024.0];
        (b.pos[0], b.pos[2]) = (1024.0, 1024.0);
        let now = b.rows;
        let [r0, u0, _] = self.before;
        self.rates = [dot(now[2], u0), dot(now[2], r0), -dot(now[0], u0)].map(|x| x.clamp(-1.0, 1.0).asin().to_degrees() / DT);
        self.before = now;
        moved
    }

    /// The stick's reach grows with the square of the speed: less of it the faster it goes.
    fn gain(&self) -> f32 {
        (45.0 / self.speed().max(20.0)).powi(2).clamp(0.3, 3.0)
    }

    /// Nose up, in degrees over the horizon; and the bank, right wing down.
    fn attitude(&self) -> (f32, f32) {
        let [r, u, f] = self.rows();
        (f[1].clamp(-1.0, 1.0).asin().to_degrees(), -r[1].atan2(u[1]).to_degrees())
    }

    /// The stick that holds that pitch and that bank.
    fn hold(&self, pitch: f32, bank: f32) -> (f32, f32) {
        let (p, b) = self.attitude();
        let k = self.gain();
        ((k * (0.05 * (pitch - p) - 0.02 * self.rates[0])).clamp(-1.0, 1.0), (k * (0.02 * (bank - b) - 0.006 * self.rates[2])).clamp(-1.0, 1.0))
    }

    /// Wings level at the height it started at, for `seconds`.
    fn level(&mut self, throttle: f32, seconds: f32) {
        let end = self.t + seconds;
        while self.t < end && self.alive() {
            let want = ((START - self.height()) * 0.25).clamp(-12.0, 22.0);
            let (p, b) = self.hold(want, 0.0);
            self.step(&[(axis::THROTTLE, throttle), (PITCH, -p), (BANK, b)]);
        }
    }

    /// The stick that brings the nose onto `dir` and keeps it there while the line to the target turns at `turn`: a fighting pilot's.
    fn point(&mut self, dir: V3, turn: V3) -> (f32, f32, f32) {
        self.stick.watch(self.rows(), self.t as f64);
        self.stick.point(self.rows(), self.vel(), dir, turn, DT)
    }
}

/// [`Rig::hold`] with a firmer pitch stick: its gain is tuned for ~110 m/s and at 135 m/s cannot hold the nose 20° up.
fn firm_hold(rig: &mut Rig, pitch: f32, bank: f32) -> (f32, f32) {
    let (_, b) = rig.hold(pitch, bank);
    let k = (90.0 / rig.speed().max(20.0)).powi(2).clamp(0.5, 4.0);
    // (the stick also learns what it takes to hold the nose there: a plane pulled up and let go goes back to where the lift holds it)
    rig.trim = (rig.trim + 0.003 * (pitch - rig.attitude().0).clamp(-10.0, 10.0) * DT).clamp(-0.7, 0.7);
    ((k * (0.05 * (pitch - rig.attitude().0) - 0.06 * rig.rates[0]) + rig.trim).clamp(-1.0, 1.0), b)
}

fn envelope(w: &bytefield_sim::World, template: &str, verbose: bool) {
    // level speed by throttle
    let mut cruise = [0.0f32; 3];
    for (k, throttle) in [1.0f32, 0.5, 0.0].into_iter().enumerate() {
        let Some(mut rig) = Rig::new(w, template, 90.0) else {
            println!("{template}: no such aircraft");
            return;
        };
        rig.level(throttle, 40.0);
        cruise[k] = if rig.alive() { rig.speed() } else { f32::NAN };
    }
    println!("{template}: level speed at throttle 1 / 0.5 / 0: {:.0} / {:.0} / {:.0} m/s", cruise[0], cruise[1], cruise[2]);
    // roll rate
    let mut rig = Rig::new(w, template, cruise[0].max(60.0)).unwrap();
    rig.level(1.0, 6.0);
    let mut roll = 0.0f32;
    for _ in 0..45 {
        rig.step(&[(axis::THROTTLE, 1.0), (BANK, 1.0)]);
        roll = roll.max(rig.rates[2].abs());
    }
    println!("{template}: roll rate, stick hard over: {roll:.0} °/s");
    // the sustained turn: level (the height is kept by the stick, the speed is what the throttle and the drag leave) at each bank; the
    // best is the sharpest turn that holds its height (loses less than 100 m in 20 s) over the last 8 s of the 20
    for throttle in [1.0f32, 0.5] {
        let mut best: Option<(f32, f32, f32, f32)> = None;
        let mut line = String::new();
        for bank in [30.0f32, 45.0, 55.0, 65.0, 75.0, 82.0] {
            let mut rig = Rig::new(w, template, cruise[0].max(60.0)).unwrap();
            rig.level(1.0, 6.0);
            let start = rig.t;
            let mut mark = (rig.vel(), rig.t, 0.0f32);
            // the stick that holds the height: pitch by the height error and the climb, and the bank
            while rig.t < start + 20.0 && rig.alive() {
                let want = ((START - rig.height()) * 0.4 - 0.5 * rig.vel()[1] + 0.1 * bank).clamp(-12.0, 30.0);
                let (p, b) = firm_hold(&mut rig, want, bank);
                rig.step(&[(axis::THROTTLE, throttle), (PITCH, -p), (BANK, b)]);
                if rig.t > start + 12.0 && mark.2 == 0.0 {
                    mark = (rig.vel(), rig.t, 1.0);
                }
            }
            if !rig.alive() {
                line += &format!(" {bank:.0}:crashed");
                continue;
            }
            let rate = angle(mark.0, rig.vel()) / (rig.t - mark.1).max(0.1);
            let lost = START - rig.height();
            line += &format!(" {bank:.0}°:{rate:.0}°/s@{:.0}m/s{}", rig.speed(), if lost < 100.0 { "" } else { "(sagging)" });
            if lost < 100.0 && best.is_none_or(|b| rate > b.0) {
                best = Some((rate, rig.speed(), bank, lost));
            }
        }
        match best {
            Some((rate, speed, bank, lost)) => println!("{template}: sustained turn, throttle {throttle}: {rate:.0} °/s at {speed:.0} m/s with {bank:.0}° of bank (a circle of {:.0} m); lost {lost:.0} m", speed / rate.to_radians().max(1e-3)),
            None => println!("{template}: sustained turn, throttle {throttle}: none holds its height"),
        }
        if verbose {
            println!("    bank:rate@speed for each bank:{line}");
        }
    }
    // the climb: the nose pulled to 20° over the horizon at full throttle for 15 s. The nose does not stay there (the plane goes back to
    // where its lift holds it: at 130-140 m/s the stick cannot keep 20°), so two figures: the best rise over any 4 s (the zoom) and the
    // rate over the last 5 s (what it keeps)
    let mut rig = Rig::new(w, template, cruise[0].max(60.0)).unwrap();
    rig.level(1.0, 6.0);
    let start = rig.t;
    let mut heights = vec![rig.height()];
    while rig.t < start + 15.0 && rig.alive() {
        let (p, b) = firm_hold(&mut rig, 20.0, 0.0);
        rig.step(&[(axis::THROTTLE, 1.0), (PITCH, -p), (BANK, b)]);
        if (rig.t - start) * 30.0 % 30.0 < 0.5 || ((rig.t - start) * 30.0).round() as u32 % 30 == 0 {
            heights.push(rig.height());
            if verbose {
                println!("    climb t {:.0} pitch {:.1} height {:.0} speed {:.0} stick {p:.2}", rig.t - start, rig.attitude().0, rig.height(), rig.speed());
            }
        }
    }
    let zoom = heights.windows(5).map(|h| (h[4] - h[0]) / 4.0).fold(f32::MIN, f32::max);
    let kept = heights.len().checked_sub(6).map_or(f32::NAN, |i| (heights[heights.len() - 1] - heights[i]) / 5.0);
    println!("{template}: climb, nose pulled to 20°, throttle 1: best over 4 s {zoom:.0} m/s, kept over the last 5 s {kept:.0} m/s (speed {:.0} m/s){}", rig.speed(), if rig.alive() { "" } else { " (ran out of height)" });
}

/// The target of a chase: a level circle of `radius` m (0: straight) at `speed`, `above` m over the chaser's starting height.
fn chase(w: &bytefield_sim::World, template: &str, radius: f32, speed: f32, above: f32, bullet: f32, verbose: bool) {
    let Some(mut rig) = Rig::new(w, template, 110.0) else { return };
    rig.level(1.0, 4.0);
    // the target starts 500 m ahead and 150 m to the right, flying the same way; its circle turns left
    let p0 = rig.pos();
    let mut centre = [p0[0] + 150.0 - radius, p0[1] + above, p0[2] + 500.0];
    let mut phase = 0.0f32;
    let mut straight = [p0[0] + 150.0, p0[1] + above, p0[2] + 500.0];
    let (mut first, mut on1, mut on3, mut on_range, mut n) = (None::<f32>, 0u32, 0u32, 0u32, 0u32);
    let (mut nearest, mut lowest) = (f32::MAX, f32::MAX);
    let start = rig.t;
    let mut mark = rig.t;
    while rig.t < start + 60.0 && rig.alive() {
        let (tp, tv) = if radius > 0.0 {
            ([centre[0] + radius * phase.cos(), centre[1], centre[2] + radius * phase.sin()], [-speed * phase.sin(), 0.0, speed * phase.cos()])
        } else {
            (straight, [0.0, 0.0, speed])
        };
        let (pos, vel) = (rig.pos(), rig.vel());
        let to = sub(tp, pos);
        let range = len(to);
        // where the burst must go to meet it: the target's motion as the shooter sees it, for the time the shot takes
        let tof = range / bullet;
        let aim: V3 = std::array::from_fn(|k| to[k] + (tv[k] - vel[k]) * tof);
        let off = angle(rig.rows()[2], aim);
        // near the floor, the floor rules
        let low = rig.height() < FLOOR + 130.0;
        let f = rig.rows()[2];
        // how the line to it turns
        let rel = sub(tv, vel);
        let turn: V3 = [to[1] * rel[2] - to[2] * rel[1], to[2] * rel[0] - to[0] * rel[2], to[0] * rel[1] - to[1] * rel[0]].map(|c| c / (range * range).max(1.0));
        let up = bytefield_sim::bots::body::dogfight::climbable(aim, rig.speed());
        let climbing = up != aim;
        let (p, b, r) = if low { rig.point([f[0], 0.35, f[2]], [0.0; 3]) } else { rig.point(up, if climbing { [0.0; 3] } else { turn }) };
        // the throttle: not to run past it
        let closing = -dot(sub(tv, vel), unit(to));
        let throttle = if low || climbing { 1.0 } else { (0.5 + (range - 220.0) / 200.0 - closing / 40.0).clamp(0.0, 1.0) };
        let moved = rig.step(&[(axis::THROTTLE, throttle), (PITCH, -p), (BANK, b), (axis::YAW, r)]);
        // the target moves on, and the map slides back under both
        phase += speed / radius.max(1.0) * DT;
        straight[2] += speed * DT;
        for c in [&mut centre, &mut straight] {
            c[0] -= moved[0];
            c[2] -= moved[1];
        }
        n += 1;
        on1 += (off < 1.0 && range < 450.0) as u32;
        on3 += (off < 3.0 && range < 450.0) as u32;
        on_range += (range < 450.0) as u32;
        if first.is_none() && off < 1.0 && range < 450.0 {
            first = Some(rig.t - start);
        }
        nearest = nearest.min(range);
        lowest = lowest.min(rig.height());
        if verbose && rig.t >= mark + 2.0 {
            mark = rig.t;
            println!("    t {:>4.0} range {range:>5.0} off {off:>5.1} speed {:>4.0} height {:>4.0} throttle {throttle:.1} bank {:>4.0}", rig.t - start, rig.speed(), rig.height(), rig.attitude().1);
        }
    }
    let pct = |a: u32| 100.0 * a as f32 / n.max(1) as f32;
    let what = if radius > 0.0 { format!("a circle of {radius:.0} m at {speed:.0} m/s ({:.0} °/s)", (speed / radius).to_degrees()) } else { format!("straight at {speed:.0} m/s") };
    println!(
        "{template} chases {what}, {above:+.0} m: guns bear after {}; within 450 m {:.0} % of the time, bearing within 1° {:.0} %, within 3° {:.0} %; nearest {nearest:.0} m, lowest {lowest:.0} m{}",
        first.map_or("never".into(), |t| format!("{t:.0} s")),
        pct(on_range),
        pct(on1),
        pct(on3),
        if rig.alive() { "" } else { " (CRASHED or under the floor)" }
    );
}

/// What the stick does: from level flight at that throttle, one axis is held at `amount` for 3 s and the body rates and the angle between
/// the nose and where it is going are printed every 0.2 s.
fn step_response(w: &bytefield_sim::World, template: &str, throttle: f32, which: usize, amount: f32) {
    let Some(mut rig) = Rig::new(w, template, 110.0) else { return };
    rig.level(throttle, 12.0);
    println!("{template}: throttle {throttle}, axis {which} held at {amount}, from {:.0} m/s", rig.speed());
    let start = rig.t;
    let mut k = 0;
    while rig.t < start + 3.0 && rig.alive() {
        rig.step(&[(axis::THROTTLE, throttle), (which, amount)]);
        k += 1;
        if k % 6 == 0 {
            let [r, u, f] = rig.rows();
            let v = rig.vel();
            println!("    t {:.1} rates pitch {:>6.1} yaw {:>6.1} roll {:>6.1}  aoa {:>5.1} slip {:>5.1} speed {:>4.0} pitch {:>5.1} bank {:>6.1}", rig.t - start, rig.rates[0], rig.rates[1], rig.rates[2], -dot(v, u).atan2(dot(v, f)).to_degrees(), dot(v, r).atan2(dot(v, f)).to_degrees(), rig.speed(), rig.attitude().0, rig.attitude().1);
        }
    }
}

/// Does it lift off by itself? From rest on the ground, each throttle in turn is held for 8 s (the height and the climb rate every
/// second; a VTOL engine may be driven by pulling the throttle back); then, hanging at ~10 m by the throttle that lifted it, each of pitch, roll and
/// yaw is held hard for 4 s, and what the craft does is printed (speed over the ground along it and to its side, the climb, the turn).
fn hover(w: &bytefield_sim::World, template: &str) {
    let destroyed = |rig: &Rig| rig.s.vehicles[rig.craft].armor.as_ref().is_some_and(|a| a.destroyed);
    let height = |rig: &Rig| rig.pos()[1] - rig.ground;
    // the throttle that lifts it highest from rest
    let mut lifts = (0.0f32, f32::MIN);
    for throttle in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        let Some(mut rig) = Rig::new(w, template, 0.0) else { return };
        let b = &mut rig.s.vehicles[rig.craft].craft.body;
        b.pos[1] = rig.ground + 4.0;
        // settle on the gear with nothing held
        for _ in 0..(3.0 / DT) as usize {
            rig.step(&[]);
        }
        let h0 = height(&rig);
        let mut line = format!("{template}: throttle {throttle:+.1} from rest at {h0:.1} m:");
        for k in 1..=8 {
            let mut moved = [0.0f32; 2];
            for _ in 0..(1.0 / DT) as usize {
                let m = rig.step(&[(axis::THROTTLE, throttle)]);
                moved = [moved[0] + m[0], moved[1] + m[1]];
            }
            let (p, b) = rig.attitude();
            line += &format!(" {k}s h {:.1} vy {:+.1} run {:.0} att {p:+.0}/{b:+.0}", height(&rig), rig.vel()[1], moved[0].hypot(moved[1]));
            if destroyed(&rig) {
                line += " DESTROYED";
                break;
            }
        }
        println!("{line}");
        if height(&rig) - h0 > lifts.1 {
            lifts = (throttle, height(&rig) - h0);
        }
    }
    let up = if lifts.1 > 5.0 { lifts.0.signum() } else { 0.0 };
    // what each stick does to the attitude while it hangs: from 1 s hanging at 10 m, 1.5 s with the stick held
    if up != 0.0 {
        for (name, which) in [("pitch", PITCH), ("roll", BANK)] {
            let mut line = format!("{template}: hanging, {name} stick:");
            for amount in [-0.3f32, 0.0, 0.3] {
                let Some(mut rig) = Rig::new(w, template, 0.0) else { return };
                rig.s.vehicles[rig.craft].craft.body.pos[1] = rig.ground + 10.0;
                let lift = |rig: &Rig| if height(rig) + 0.8 * rig.vel()[1] < 10.0 { up } else { 0.0 };
                for _ in 0..(1.0 / DT) as usize {
                    let t = lift(&rig);
                    rig.step(&[(axis::THROTTLE, t)]);
                }
                let (p0, b0) = rig.attitude();
                let (mut along, mut side) = (0.0f32, 0.0f32);
                for _ in 0..(1.5 / DT) as usize {
                    let t = lift(&rig);
                    let m = rig.step(&[(axis::THROTTLE, t), (which, amount)]);
                    let [r, _, f] = rig.rows();
                    along += m[0] * f[0] + m[1] * f[2];
                    side += m[0] * r[0] + m[1] * r[2];
                }
                let (p1, b1) = rig.attitude();
                line += &format!(" {amount:+.1}: pitch {:+.1}° bank {:+.1}° ahead {along:+.1} right {side:+.1};", p1 - p0, b1 - b0);
            }
            println!("{line}");
        }
    }
    println!("{template}: lifts with the throttle {}", if up < 0.0 { "back" } else if up > 0.0 { "forward" } else { "neither way (no VTOL)" });
    if up != 0.0 {
        for start in [10.0, 30.0] {
            transition(w, template, up, start);
        }
    }
    // hanging at 10 m: the throttle holds the height (back is up, as measured above when it is so)
    for (name, which, amount) in [("pitch", PITCH, 1.0f32), ("pitch", PITCH, -1.0), ("roll", BANK, 1.0), ("yaw", axis::YAW, 1.0)] {
        let Some(mut rig) = Rig::new(w, template, 0.0) else { return };
        let b = &mut rig.s.vehicles[rig.craft].craft.body;
        b.pos[1] = rig.ground + 10.0;
        // (all or nothing: half of it may not lift at all)
        let lift = |rig: &Rig| if height(rig) + 0.8 * rig.vel()[1] < 10.0 { up } else { 0.0 };
        for _ in 0..(3.0 / DT) as usize {
            let t = lift(&rig);
            rig.step(&[(axis::THROTTLE, t)]);
        }
        let h0 = height(&rig);
        let yaw0 = rig.rows()[2];
        let (mut along, mut side) = (0.0f32, 0.0f32);
        for _ in 0..(4.0 / DT) as usize {
            let t = lift(&rig);
            let m = rig.step(&[(axis::THROTTLE, t), (which, amount)]);
            let [r, _, f] = rig.rows();
            along += m[0] * f[0] + m[1] * f[2];
            side += m[0] * r[0] + m[1] * r[2];
        }
        let f = rig.rows()[2];
        let turned = (yaw0[0] * f[2] - yaw0[2] * f[0]).atan2(yaw0[0] * f[0] + yaw0[2] * f[2]).to_degrees();
        let (p, bank) = rig.attitude();
        println!(
            "{template}: hanging at {h0:.1} m, {name} {amount:+.0} for 4 s: ahead {along:+.1} m, right {side:+.1} m, height {:.1} m, turned {turned:+.0}°, pitch {p:+.0}° bank {bank:+.0}°{}",
            height(&rig),
            if destroyed(&rig) { " DESTROYED" } else { "" }
        );
    }
}

/// From hanging at `start` m (lifted by the throttle `up`), drifting ahead with the pitch stick at 0.4 for 4 s, then the throttle full
/// forward and the nose held 5° up: the height and speed every second for 10 s (how much a VTOL craft sinks before it flies).
fn transition(w: &bytefield_sim::World, template: &str, up: f32, start: f32) {
    let Some(mut rig) = Rig::new(w, template, 0.0) else { return };
    let height = |rig: &Rig| rig.pos()[1] - rig.ground;
    rig.s.vehicles[rig.craft].craft.body.pos[1] = rig.ground + start;
    let lift = |rig: &Rig| if height(rig) + 0.8 * rig.vel()[1] < start { up } else { 0.0 };
    for _ in 0..(2.0 / DT) as usize {
        let t = lift(&rig);
        rig.step(&[(axis::THROTTLE, t)]);
    }
    for _ in 0..(4.0 / DT) as usize {
        let t = lift(&rig);
        rig.step(&[(axis::THROTTLE, t), (PITCH, 0.4)]);
    }
    let mut line = format!("{template}: from hanging at {start:.0} m and {:.0} m/s, full throttle:", rig.speed());
    let mut lowest = height(&rig);
    for k in 1..=10 {
        for _ in 0..(1.0 / DT) as usize {
            let (p, b) = rig.hold(5.0, 0.0);
            rig.step(&[(axis::THROTTLE, 1.0), (PITCH, -p), (BANK, b)]);
            lowest = lowest.min(height(&rig));
        }
        line += &format!(" {k}s h {:.0} v {:.0}", height(&rig), rig.speed());
    }
    println!("{line}; lowest {lowest:.1} m");
}

/// `--cruise`: see the header.
fn cruise(w: &bytefield_sim::World, template: &str, up: f32) {
    const AT: f32 = 40.0;
    let height = |rig: &Rig| rig.pos()[1] - rig.ground;
    let lift = |rig: &Rig| if height(rig) + 0.8 * rig.vel()[1] < AT { up } else { 0.0 };
    // the sticks that hold that attitude while hanging (a hover's gains: the plane's [`Rig::hold`] is tuned for 110 m/s)
    let hold = |rig: &Rig, pitch: f32, bank: f32| {
        let (p, b) = rig.attitude();
        ((-(0.06 * (pitch - p) - 0.03 * rig.rates[0])).clamp(-1.0, 1.0), (0.06 * (bank - b) - 0.02 * rig.rates[2]).clamp(-1.0, 1.0))
    };
    let heading = |rig: &Rig| {
        let f = rig.rows()[2];
        f[0].atan2(f[2]).to_degrees()
    };
    let setup = |tilt: f32| -> Option<Rig> {
        let mut rig = Rig::new(w, template, 0.0)?;
        rig.s.vehicles[rig.craft].craft.body.pos[1] = rig.ground + AT;
        for _ in 0..(8.0 / DT) as usize {
            let t = lift(&rig);
            let (p, b) = hold(&rig, -tilt, 0.0);
            rig.step(&[(axis::THROTTLE, t), (PITCH, p), (BANK, b)]);
        }
        Some(rig)
    };
    for tilt in [10.0f32, 20.0, 30.0, 45.0] {
        let Some(rig) = setup(tilt) else { return };
        let v = rig.vel();
        let (p, b) = rig.attitude();
        println!("{template}: hanging by {up:+.1}, nose {tilt:.0}° down for 8 s: {:.1} m/s along, vy {:+.1}, height {:.1}, pitch {p:+.0} bank {b:+.0}", v[0].hypot(v[2]), v[1], height(&rig));
    }
    // going along at 20° down, what turns it, over 4 s
    for (name, yaw, bank) in [("yaw +1", 1.0f32, 0.0f32), ("yaw -1", -1.0, 0.0), ("bank 20", 0.0, 20.0), ("bank 40", 0.0, 40.0), ("bank 20 + yaw", 1.0, 20.0)] {
        let Some(mut rig) = setup(20.0) else { return };
        let h0 = heading(&rig);
        let mut line = format!("{template}: going along, {name}:");
        for k in 1..=4 {
            for _ in 0..(1.0 / DT) as usize {
                let t = lift(&rig);
                let (p, b) = hold(&rig, -20.0, bank);
                rig.step(&[(axis::THROTTLE, t), (PITCH, p), (BANK, b), (axis::YAW, yaw)]);
            }
            let v = rig.vel();
            let mut d = heading(&rig) - h0;
            d = (d + 540.0).rem_euclid(360.0) - 180.0;
            let track = v[0].atan2(v[2]).to_degrees() - h0;
            let (p, b) = rig.attitude();
            line += &format!(" {k}s turned {d:+.0}° track {:+.0}° v {:.0} h {:.0} att {p:+.0}/{b:+.0};", (track + 540.0).rem_euclid(360.0) - 180.0, v[0].hypot(v[2]), height(&rig));
        }
        println!("{line}");
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("air_envelope <game> <mod> <level> <template>... [--chase] [--hover] [--verbose]");
        std::process::exit(2);
    }
    let (do_chase, verbose) = (a.iter().any(|x| x == "--chase"), a.iter().any(|x| x == "--verbose"));
    let w = bytefield_sim::World::load(std::path::Path::new(&a[1]), &a[2], &a[3], "conquest").expect("world");
    for template in a[4..].iter().take_while(|x| !x.starts_with("--")) {
        if a.iter().any(|x| x == "--hover") {
            hover(&w, template);
        } else if let Some(i) = a.iter().position(|x| x == "--cruise") {
            cruise(&w, template, a.get(i + 1).and_then(|x| x.parse().ok()).unwrap_or(-1.0));
        } else if let Some(i) = a.iter().position(|x| x == "--step") {
            let p = |k: usize| a[i + k].parse::<f32>().expect("--step <throttle> <axis> <amount>");
            step_response(&w, template, p(1), p(2) as usize, p(3));
        } else if do_chase {
            for (radius, speed, above) in [(0.0, 90.0, 0.0), (500.0, 90.0, 0.0), (300.0, 90.0, 0.0), (300.0, 90.0, 250.0), (200.0, 70.0, -200.0)] {
                chase(&w, template, radius, speed, above, 900.0, verbose);
            }
        } else {
            envelope(&w, template, verbose);
        }
    }
}
