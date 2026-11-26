//! What the medic and the engineer can do: heal a wounded mate and repair a friendly vehicle.
//! The specification leaves it as an improvement, with no figure (§3), so it only happens with nobody in sight and for a short time.
use super::walker::Walker;
use super::shooter::Shooter;
use super::{TURN, CHEST, MapProfile, weapons, pitch, dist3, angle_between, no_fall};
use crate::bot_api::{BotView, Other};
use crate::soldier::axis;

/// How far it looks for a wounded mate, m: the medkit reaches 10.
const WOUNDED_RADIUS: f32 = 30.0;
/// How close it gets to heal, m.
const NEAR_WOUNDED: f32 = 6.0;
/// How far it looks for a damaged vehicle, m.
const DAMAGED_RADIUS: f32 = 70.0;
/// Health of a mate below which it goes to heal them.
const HEALTH_WOUNDED: f32 = 0.8;
/// Own health below which the medic heals itself.
const HEALTH_OWN: f32 = 0.6;
/// Health of a vehicle below which it goes to repair it.
const HEALTH_DAMAGED: f32 = 0.9;
/// Seconds it spends on a task, and how long it waits before looking for another.
const TASK_TIME: f64 = 25.0;
const TASK_REST: f64 = 6.0;

/// What fraction of those radii it looks at where the fighting is close.
const CITY_SCALE: f32 = 0.6;

/// Whom it attends to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Task {
    Wounded(usize),
    Yo,
    VehicleKind(usize),
}

/// The key for the item it carries with that name.
fn key(v: &BotView, name: &str) -> Option<usize> {
    v.me.items.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|x| x.0).filter(|k| (1..=9).contains(k))
}

/// What it carries from one tick to the next.
#[derive(Debug, Default)]
pub struct Trade {
    pub task: Option<Task>,
    pub task_since: f64,
    pub find_task: f64,
    /// who it has been asked to heal (bots: the wounded mate it is heading to), before anyone else; if closer than [`REQUEST_RADIUS`]
    pub heal: Option<usize>,
    /// the vehicle it has been asked to repair (bots: one with a crew and little health), before any other
    pub repair: Option<usize>,
}

/// How far it goes straight to whoever it was asked to heal or to the vehicle it was asked to repair, m (farther away, they are brought closer).
pub const REQUEST_RADIUS: f32 = 25.0;

impl Trade {
    /// If there is something to heal or repair: the axes to do it.
    pub fn trade(&mut self, v: &BotView, others: &[Other], profile: &MapProfile, vetoed: &mut Vec<usize>, cam: &mut Walker, shooter: &mut Shooter) -> Option<Vec<(usize, f32)>> {
        if shooter.target.is_some() {
            self.task = None;
            return None;
        }
        let medkit = key(v, "MedPack");
        let wrench = key(v, "RepairPack");
        if medkit.is_none() && wrench.is_none() {
            return None;
        }
        let me = &v.me;
        if self.task.is_none() {
            if v.time < self.find_task {
                return None;
            }
            self.find_task = v.time + 1.0;
            // (in a city the fighting is close and there is no time: only whoever is beside it)
            let scale = if profile.insists { 1.0 } else { CITY_SCALE };
            let near = |p: [f32; 3], r: f32| dist3(p, me.pos) < r * scale;
            let wounded = medkit.and_then(|_| others.iter().filter(|o| o.team == me.team && !o.in_vehicle && o.health.is_some_and(|h| h < HEALTH_WOUNDED) && near(o.pos, WOUNDED_RADIUS)).min_by(|a, b| dist3(a.pos, me.pos).total_cmp(&dist3(b.pos, me.pos)).then(a.player.cmp(&b.player))).filter(|o| !v.hidden(me.eye, [o.pos[0], o.pos[1] + CHEST, o.pos[2]])).map(|o| Task::Wounded(o.player)));
            let own_one = medkit.filter(|_| me.health < HEALTH_OWN).map(|_| Task::Yo);
            let vehicle = wrench.and_then(|_| {
                v.vehicles()
                    .into_iter()
                    // (a big ship cannot be reached from the ground; and one out of sight is not gone to: the way there is straight,
                    // without a path, and GC_Judicator_Push's engineers pushed against the hangar's walls 25 s for each damaged ship)
                    .filter(|c| (c.team == 0 || c.team == me.team) && c.health > 0.0 && c.health < HEALTH_DAMAGED && c.radius < 25.0 && near(c.pos, DAMAGED_RADIUS) && !vetoed.contains(&c.vehicle) && !v.hidden(me.eye, [c.pos[0], c.pos[1] + 1.0, c.pos[2]]))
                    .min_by(|a, b| dist3(a.pos, me.pos).total_cmp(&dist3(b.pos, me.pos)).then(a.vehicle.cmp(&b.vehicle)))
                    .map(|c| Task::VehicleKind(c.vehicle))
            });
            // what it has been asked for goes first
            let request_h = self.heal.zip(medkit).and_then(|(j, _)| others.iter().find(|o| o.player == j && o.team == me.team && !o.in_vehicle && o.health.is_some_and(|h| h < HEALTH_WOUNDED) && dist3(o.pos, me.pos) < REQUEST_RADIUS)).filter(|o| !v.hidden(me.eye, [o.pos[0], o.pos[1] + CHEST, o.pos[2]])).map(|o| Task::Wounded(o.player));
            let request_v = self.repair.zip(wrench).and_then(|(j, _)| v.vehicle(j)).filter(|c| (c.team == 0 || c.team == me.team) && c.health > 0.0 && c.health < HEALTH_DAMAGED && dist3(c.pos, me.pos) < REQUEST_RADIUS + c.radius).map(|c| Task::VehicleKind(c.vehicle));
            self.task = request_h.or(request_v).or(wounded).or(own_one).or(vehicle);
            self.task?;
            self.task_since = v.time;
        }
        let e = self.task?;
        if v.time - self.task_since > TASK_TIME {
            if let Task::VehicleKind(j) = e {
                vetoed.push(j);
            }
            return self.finish_task(v, shooter);
        }
        // where it has to get to, how close, and whether it is already done
        let (spot, a, done, name, k) = match e {
            Task::Wounded(j) => {
                let Some(o) = others.iter().find(|o| o.player == j && !o.in_vehicle) else { return self.finish_task(v, shooter) };
                (o.pos, NEAR_WOUNDED, o.health.is_none_or(|h| h > 0.97), "MedPack", medkit?)
            }
            Task::Yo => (me.pos, f32::MAX, me.health > 0.95, "MedPack", medkit?),
            Task::VehicleKind(j) => {
                let Some(c) = v.vehicles().into_iter().find(|c| c.vehicle == j) else { return self.finish_task(v, shooter) };
                (c.pos, c.radius + 1.2, c.health <= 0.0 || c.health > 0.97 || !(c.team == 0 || c.team == me.team), "RepairPack", wrench?)
            }
        };
        if done {
            return self.finish_task(v, shooter);
        }
        let mut axes = Vec::with_capacity(6);
        let d = [spot[0] - me.pos[0], spot[2] - me.pos[2]];
        let g = angle_between(me.view[2], d);
        axes.push((axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)));
        axes.push((axis::MOUSE_Y, (pitch(me.view[2]) / 2.5).clamp(-4.0, 4.0)));
        if dist3(spot, me.pos) > a {
            let walks = g.abs() < 60.0 && no_fall(v, me.pos, [me.pos[0] + d[0].clamp(-8.0, 8.0), me.pos[2] + d[1].clamp(-8.0, 8.0)]);
            axes.push((axis::THROTTLE, walks as i32 as f32));
            if cam.vehicle_jam(v, me.pos) > 2.0 {
                axes.push((axis::ACTION, 1.0));
            }
            return Some(axes);
        }
        // done: takes out its kit and uses it
        if me.weapon.as_deref().is_some_and(|w| w.eq_ignore_ascii_case(name)) {
            axes.push((axis::FIRE, 1.0));
            if me.rounds.0 <= 1 && me.rounds.1 <= 0 {
                return self.finish_task(v, shooter);
            }
        } else {
            axes.push((axis::WEAPON1 + k - 1, 1.0));
        }
        Some(axes)
    }

    /// Puts it away, and goes back to its weapon.
    fn finish_task(&mut self, v: &BotView, shooter: &mut Shooter) -> Option<Vec<(usize, f32)>> {
        self.task = None;
        self.find_task = v.time + TASK_REST;
        shooter.requested_weapon = Some((weapons::PRIMARY_SLOT, v.time + 0.1));
        None
    }
}
