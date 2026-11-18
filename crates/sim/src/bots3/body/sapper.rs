//! What the engineer does with its explosives: mines laid across the path where enemy vehicles will come, and charges beside
//! a stopped enemy vehicle, set off once it has stepped away. Outside the specification, which does not mention them: the only
//! thing against it is its Berlin limit (no shot with the enemy beyond 60 m), which is why they are not used there ([`Profile::explosives`]).
use super::paths::Answer;
use super::{Choice, Surroundings, TURN, weapons, pitch, dist3, angle_between, flat, no_fall};
use super::sailor::BIG_SHIP_RADIUS;
use crate::bot_api::{BotView, VehicleClass};
use crate::soldier::axis;

/// Engineer lives that start by laying mines, instead of going straight for a vehicle.
const P_MINES: f32 = 0.7;
/// Engineer lives that, besides, do not board a vehicle while there is an enemy one to blow up.
const P_FIGHTER: f32 = 0.7;
/// Until when whoever is at this does not board a vehicle, s since spawning.
const WHILE_ON_FOOT: f64 = 100.0;
/// Spacing between the mines of a barrier, m: the fuse triggers 3 m from the vehicle's centre.
const MINE_SPACING: f32 = 5.5;
const MINES_BARRIER: usize = 4;
/// What an engineer carries.
const MINES_CARRIED: u32 = 4;
/// Distance from the barrier to the own flag along the path, m, and how it varies from one engineer to another.
const BARRIER_FROM: f32 = 40.0;
const BARRIER_STEP: f32 = 15.0;
/// Farther than this, you do not walk to lay mines if there is a vehicle, m; and how far from where it will lay them it gets off.
const FAR_ON_FOOT: f32 = 500.0;
const GET_OFF_NEAR: f32 = 70.0;
/// How far from its flag it leaves a trap on the approach, and how far along that approach, m.
const NEAR_FLAG: f32 = 60.0;
const ACCESS: f32 = 30.0;
/// Seconds without advancing towards where it was going to lay a mine after which it lays it where it stands.
const MINE_JAM: f64 = 6.0;
/// How far away it sees an enemy vehicle coming, m; and ahead of it, how far it goes to cut it off.
const SEES_COMING: f32 = 180.0;
const CUT_OFF: f32 = 90.0;
/// With enemies in sight it keeps mining if it has fewer than this many metres left, and if it has none closer than these others.
const ALMOST_THERE: f32 = 30.0;
const ENEMY_ON_TOP: f32 = 20.0;
/// How far it goes for a stopped enemy vehicle, m.
const FIGHTER_RADIUS: f32 = 250.0;
/// At what speed a vehicle still counts as stopped, m/s.
const STOPPED_SPEED: f32 = 2.5;
/// How many metres from the vehicle's edge it drops the charge.
const NEAR_CHARGE: f32 = 2.5;
/// Charges per vehicle, and how many it lays at most: of the four it carries it keeps one, because without it the detonator cannot be taken out.
const CHARGES_PER_VEHICLE: u32 = 2;
const CHARGES_MAX: u32 = 3;
/// At a trap, one: the rest, for a sure target.
const CHARGES_TRAP: u32 = 1;
/// What a charge reaches, m, and how far it steps away (its own also kills).
const CHARGE_RANGE: f32 = 12.0;
const ASIDE: f32 = 15.0;
/// At what distance from its charges it sees a vehicle approach and takes out the detonator, m.
const SEES_APPROACH: f32 = 60.0;
/// Seconds it waits aside for someone to blow up: after that, it carries on with its business with the charges laid, in case someone passes.
const DETONATE_WAIT: f64 = 30.0;
/// Seconds it spends at most getting to each thing.
const GO_TIME: f64 = 70.0;

/// What it is up to.
#[derive(Clone, Debug, Default)]
pub enum Chore {
    #[default]
    Nothing,
    /// wants to mine: it must know where
    FindRoute,
    /// lays mines, in order
    Mines { places: Vec<[f32; 2]>, since: f64 },
    /// is going to leave a charge on the path
    GoesToPath { place: [f32; 2], since: f64 },
    /// is going for this vehicle
    GoesToCharge { vehicle: usize, since: f64 },
    /// drops them here, beside a vehicle or on a path: `left` left to drop, and the bullets it saw last time
    Loading { vehicle: Option<usize>, left: u32, before: i32, since: f64 },
    /// steps away from where it left them and waits
    StepAside { spot: [f32; 3], since: f64 },
    /// presses the detonator for a few ticks
    Detonate { until: f64 },
}

#[derive(Clone, Debug)]
pub struct Sapper {
    pub chore: Chore,
    /// does not board a vehicle before this
    pub on_foot_until: f64,
    fighter: bool,
    no_mines: bool,
    /// when it looks again for an enemy vehicle to blow up
    look: f64,
    /// where the vehicles were the last time it looked
    before: Vec<(usize, [f32; 3])>,
    before_t: f64,
    charges: u32,
    mines_laid: u32,
    /// where it has charges not yet set off, which it will detonate if something enemy passes
    trap: Option<[f32; 3]>,
    /// where it would put a charge on the path, after the mines
    charge_path: Option<[f32; 2]>,
    look_trap: f64,
    /// presses the key one tick in two
    pulse: bool,
    requested: bool,
    /// the bullets it saw in the mines' magazine last time (-1: none)
    bullets: i32,
}

impl Default for Sapper {
    fn default() -> Sapper {
        Sapper { chore: Chore::Nothing, on_foot_until: 0.0, fighter: false, no_mines: false, look: 0.0, before: Vec::new(), before_t: 0.0, charges: 0, mines_laid: 0, trap: None, charge_path: None, look_trap: 0.0, pulse: false, requested: false, bullets: -1 }
    }
}

/// The key for the item it carries with that name.
fn key(v: &BotView, name: &str) -> Option<usize> {
    v.me.items.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|x| x.0).filter(|k| (1..=9).contains(k))
}

fn in_hand(v: &BotView, name: &str) -> bool {
    v.me.weapon.as_deref().is_some_and(|w| w.eq_ignore_ascii_case(name))
}

impl Sapper {
    /// On spawning: what it plans to do with the explosives in this life.
    pub fn plan_sapper(&mut self, v: &BotView, ctx: &mut Surroundings) {
        if !ctx.profile.explosives || key(v, "Landmine").is_none() {
            return;
        }
        let has_vehicles = v.vehicles().iter().any(|c| matches!(c.class, VehicleClass::Tank | VehicleClass::Car | VehicleClass::Walker));
        let mines = has_vehicles && ctx.rng.si(P_MINES);
        self.fighter = ctx.rng.si(P_FIGHTER);
        // whoever does not lay mines, if at this, leaves at least one charge on the path
        self.no_mines = !mines;
        if mines || (has_vehicles && self.fighter) {
            self.chore = Chore::FindRoute;
        }
        if self.fighter && has_vehicles {
            // whoever is going for them does not board one while it has charges left
            self.on_foot_until = f64::MAX;
        } else if mines {
            self.on_foot_until = v.time + WHILE_ON_FOOT;
        }
    }

    /// What it does with the explosives this tick, if anything. `combat`: it has someone to shoot, and that comes before going anywhere.
    pub fn sap(&mut self, v: &BotView, ctx: &mut Surroundings, combat: bool) -> Option<Vec<(usize, f32)>> {
        if !ctx.profile.explosives {
            return None;
        }
        match self.chore.clone() {
            Chore::Nothing => {
                self.look_trap(v, ctx);
                if !combat {
                    self.look_charges(v, ctx);
                }
                None
            }
            Chore::FindRoute => {
                if combat {
                    return None;
                }
                self.find_route(v, ctx);
                Some(vec![(axis::MOUSE_X, 0.0)])
            }
            Chore::Mines { places, since } => {
                // almost there and the enemy still far: finishes what it came to do
                let almost = places.first().is_some_and(|s| flat(v.me.pos, *s) < ALMOST_THERE) && ctx.shooter.target.is_none_or(|b| b.2 > ENEMY_ON_TOP);
                if combat && !almost {
                    return None;
                }
                self.lay_mines(v, ctx, places, since)
            }
            Chore::GoesToPath { place, since } => {
                if combat {
                    return None;
                }
                if v.time - since > GO_TIME {
                    return self.finish_chore(v, ctx);
                }
                let jammed = flat(v.me.pos, place) < ALMOST_THERE * 2.0 && ctx.cam.vehicle_jam(v, v.me.pos) > MINE_JAM;
                if !jammed && let Some(axes) = self.walk_to(v, ctx, place, 2.0) {
                    return Some(axes);
                }
                self.chore = Chore::Loading { vehicle: None, left: CHARGES_TRAP, before: -1, since: v.time };
                Some(Vec::new())
            }
            Chore::GoesToCharge { vehicle, since } => {
                if combat {
                    return None;
                }
                self.go_to_charge(v, ctx, vehicle, since)
            }
            Chore::Loading { vehicle, left, before, since } => self.load(v, ctx, vehicle, left, before, since),
            Chore::StepAside { spot, since } => self.step_aside(v, ctx, spot, since),
            Chore::Detonate { until } => {
                if v.time >= until {
                    self.chore = Chore::Nothing;
                    ctx.shooter.requested_weapon = Some((weapons::PRIMARY_SLOT, v.time + 0.2));
                    return None;
                }
                Some(vec![(axis::FIRE, 1.0)])
            }
        }
    }

    /// The axes for walking towards `p` however it can, if it has not arrived yet; without them while it looks for the path.
    fn walk_to(&mut self, v: &BotView, ctx: &mut Surroundings, p: [f32; 2], near: f32) -> Option<Vec<(usize, f32)>> {
        let me = &v.me;
        if flat(me.pos, p) < near {
            return None;
        }
        // (the path is good while the goal is the same)
        if self.requested || ctx.cam.point != Some(p) || ctx.cam.path.is_empty() {
            let maps = ctx.maps.clone();
            let (eye, pos) = (me.eye, me.pos);
            let se_ve = |c: [f32; 2]| (!v.hidden(eye, [c[0], eye[1], c[1]]) || v.ground(c[0], c[1]).is_some_and(|h| !v.hidden(eye, [c[0], h + 1.4, c[1]]))) && no_fall(v, pos, c);
            match maps.request_foot(v, [pos[0], pos[2]], p, &se_ve) {
                Answer::Path(c) => {
                    ctx.cam.path = c;
                    ctx.cam.path.push(p);
                    ctx.cam.point = Some(p);
                    self.requested = false;
                }
                Answer::Wait => {
                    self.requested = true;
                    return Some(vec![(axis::MOUSE_X, 0.0)]);
                }
                Answer::NoPath => {
                    ctx.cam.path = vec![p];
                    ctx.cam.point = Some(p);
                    self.requested = false;
                }
            }
        }
        while ctx.cam.path.len() > 1 && flat(me.pos, ctx.cam.path[0]) < 2.0 {
            ctx.cam.path.remove(0);
        }
        let step = ctx.cam.path[0];
        let g = angle_between(me.view[2], [step[0] - me.pos[0], step[1] - me.pos[2]]);
        let mut axes = vec![(axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)), (axis::MOUSE_Y, (pitch(me.view[2]) / 2.5).clamp(-4.0, 4.0))];
        let safe = no_fall(v, me.pos, [me.pos[0] + (step[0] - me.pos[0]).clamp(-8.0, 8.0), me.pos[2] + (step[1] - me.pos[2]).clamp(-8.0, 8.0)]);
        axes.push((axis::THROTTLE, (g.abs() < 60.0 && safe) as i32 as f32));
        if ctx.cam.vehicle_jam(v, me.pos) > 2.0 {
            axes.push((axis::ACTION, 1.0));
        }
        Some(axes)
    }

    // ---- mines

    /// Where they will come from: the vehicle path of the enemy flag nearest to the nearest own one. The barrier, across.
    fn find_route(&mut self, v: &BotView, ctx: &mut Surroundings) {
        let me = &v.me;
        let flags = v.flags();
        let flat_f = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
        let nearest = |de: [f32; 3], list: &mut dyn Iterator<Item = &crate::bot_api::Flag>| list.min_by(|x, y| flat_f(x.pos, de).total_cmp(&flat_f(y.pos, de))).map(|f| f.pos);
        // what the enemy will come to take: a flag that can be taken and is not its own, the nearest to its own
        let own_ones: Vec<[f32; 3]> = flags.iter().filter(|f| f.team != me.team && f.team != 0).map(|f| f.pos).collect();
        let to_own_ones = |p: [f32; 3]| own_ones.iter().map(|q| flat_f(*q, p)).fold(f32::MAX, f32::min);
        let Some(k_own) = (0..flags.len()).filter(|&k| flags[k].takeable && (flags[k].team == me.team || flags[k].team == 0)).min_by(|&x, &y| to_own_ones(flags[x].pos).total_cmp(&to_own_ones(flags[y].pos)).then(x.cmp(&y))) else {
            self.chore = Chore::Nothing;
            return;
        };
        let own_side = flags[k_own].pos;
        let Some(others_own) = nearest(own_side, &mut flags.iter().filter(|f| f.team != me.team && f.team != 0)) else {
            self.chore = Chore::Nothing;
            return;
        };
        let maps = ctx.maps.clone();
        let route = match maps.request_tank(v, [others_own[0], others_own[2]], [own_side[0], own_side[2]], &[], false) {
            Answer::Path(c) => c,
            Answer::Wait => return,
            Answer::NoPath => {
                self.chore = Chore::Nothing;
                return;
            }
        };
        // from the own flag backwards, so many metres along the path
        let mut points: Vec<[f32; 2]> = vec![[others_own[0], others_own[2]]];
        points.extend(route);
        let goal = BARRIER_FROM + BARRIER_STEP * (me.player % 4) as f32;
        let mut done = 0.0;
        let mut barrier = None;
        for w in points.windows(2).rev() {
            let (b, a) = (w[1], w[0]);
            let l = (a[0] - b[0]).hypot(a[1] - b[1]);
            if done + l >= goal && l > 0.0 {
                let t = (goal - done) / l;
                let centre = [b[0] + (a[0] - b[0]) * t, b[1] + (a[1] - b[1]) * t];
                barrier = Some((centre, [(a[0] - b[0]) / l, (a[1] - b[1]) / l]));
                break;
            }
            done += l;
        }
        let Some((centre, dir)) = barrier else {
            self.chore = Chore::Nothing;
            return;
        };
        self.charge_path = Some([centre[0] + dir[0] * 12.0, centre[1] + dir[1] * 12.0]);
        let water = v.water();
        let mut places: Vec<[f32; 2]> = (0..if self.no_mines { 0 } else { MINES_BARRIER })
            .map(|k| (k as f32 - (MINES_BARRIER as f32 - 1.0) * 0.5) * MINE_SPACING)
            .map(|d| [centre[0] - dir[1] * d, centre[1] + dir[0] * d])
            .filter(|s| v.ground(s[0], s[1]).is_some_and(|h| h > water + 0.5))
            .collect();
        // starts at the nearest end
        if places.first().zip(places.last()).is_some_and(|(a, b)| flat(me.pos, *b) < flat(me.pos, *a)) {
            places.reverse();
        }
        if !places.is_empty() || self.charge_path.is_some() {
            // if in a vehicle, it goes to that flag; flying does not lead to any road
            *ctx.flag = Some(Choice { flag: k_own, attacks: flags[k_own].team != me.team });
            *ctx.owner = flags[k_own].team;
            ctx.driver.wants_fly = false;
            ctx.driver.wants_fixed = false;
        }
        self.chore = Chore::Mines { places, since: v.time };
    }

    fn lay_mines(&mut self, v: &BotView, ctx: &mut Surroundings, mut places: Vec<[f32; 2]>, since: f64) -> Option<Vec<(usize, f32)>> {
        let Some(&place) = places.first() else {
            // once the mines are done, a charge in the middle of the path
            if let Some(c) = self.charge_path.take()
                && self.trap.is_none()
                && key(v, "ExpPack").is_some()
            {
                self.chore = Chore::GoesToPath { place: c, since: v.time };
                return Some(Vec::new());
            }
            return self.finish_chore(v, ctx);
        };
        if v.time - since > GO_TIME * 2.0 {
            return self.finish_chore(v, ctx);
        }
        // far: better in a vehicle, if there is one (on arrival it gets off: `dismount_to_mine`)
        if flat(v.me.pos, place) > FAR_ON_FOOT {
            ctx.driver.wants_board_at.get_or_insert(v.time);
            if let Some(axes) = ctx.driver.go_to_vehicle(v, ctx.profile, ctx.alternate) {
                return Some(axes);
            }
        }
        // (a cliff or a wall the map does not know: if stuck nearby, it lays it where it stands, which is also path)
        let jammed = flat(v.me.pos, place) < ALMOST_THERE * 2.0 && ctx.cam.vehicle_jam(v, v.me.pos) > MINE_JAM;
        if !jammed && let Some(axes) = self.walk_to(v, ctx, place, 1.5) {
            return Some(axes);
        }
        let Some(k) = key(v, "Landmine") else { return self.finish_chore(v, ctx) };
        if !in_hand(v, "Landmine") {
            return Some(vec![(axis::WEAPON1 + k - 1, 1.0)]);
        }
        if v.me.rounds.0 + v.me.rounds.1 <= 0 {
            return self.finish_chore(v, ctx);
        }
        // laid: seen in the ammunition
        if self.bullets >= 0 && v.me.rounds.0 < self.bullets {
            self.bullets = -1;
            ctx.maps.laid(v.me.team, [v.me.pos[0], v.me.pos[2]]);
            self.mines_laid += 1;
            ctx.cam.anchor_t = v.time;
            places.remove(0);
            self.chore = Chore::Mines { places, since };
            return Some(vec![(axis::MOUSE_X, 0.0)]);
        }
        self.bullets = v.me.rounds.0;
        self.pulse = !self.pulse;
        Some(vec![(axis::FIRE, self.pulse as i32 as f32), (axis::MOUSE_X, 0.0)])
    }

    /// At the wheel, mining path: if it has already arrived, it gets off. Returns whether it has to get off.
    pub fn dismount_to_mine(&mut self, v: &BotView, _ctx: &mut Surroundings, i: &crate::bot_api::Inside) -> bool {
        let Chore::Mines { places, .. } = &self.chore else { return false };
        let Some(place) = places.first().copied() else { return false };
        let vel = i.velocity[0].hypot(i.velocity[2]);
        if flat(i.pos, place) < GET_OFF_NEAR && vel < 8.0 {
            self.chore = Chore::Mines { places: places.clone(), since: v.time };
            self.on_foot_until = self.on_foot_until.max(v.time + WHILE_ON_FOOT);
            return true;
        }
        false
    }

    fn finish_chore(&mut self, v: &BotView, ctx: &mut Surroundings) -> Option<Vec<(usize, f32)>> {
        self.chore = Chore::Nothing;
        self.bullets = -1;
        ctx.cam.point = None;
        ctx.cam.path.clear();
        ctx.shooter.requested_weapon = Some((weapons::PRIMARY_SLOT, v.time + 0.2));
        None
    }

    // ---- charges

    /// If there is a stopped enemy vehicle in sight: goes for it.
    fn look_charges(&mut self, v: &BotView, ctx: &mut Surroundings) {
        if v.time < self.look {
            return;
        }
        self.look = v.time + 1.0;
        let me = &v.me;
        let seen_ones = v.vehicles();
        let flags = v.flags();
        let dt = (v.time - self.before_t) as f32;
        let before = std::mem::take(&mut self.before);
        let earlier_comes = before.clone();
        let stopped = |c: &crate::bot_api::VehicleSeen| dt > 0.5 && dt < 3.0 && before.iter().find(|a| a.0 == c.vehicle).is_some_and(|a| dist3(a.1, c.pos) < STOPPED_SPEED * dt);
        // it is the enemy's: they drive it, or it is parked beside one of their flags
        let beside_own = |c: &crate::bot_api::VehicleSeen| flags.iter().filter(|f| dist3(f.pos, c.pos) < 80.0).min_by(|a, b| dist3(a.pos, c.pos).total_cmp(&dist3(b.pos, c.pos))).is_some_and(|f| f.team != me.team && f.team != 0);
        let enemy = |c: &crate::bot_api::VehicleSeen| (c.team != 0 && c.team != me.team) || (c.team == 0 && beside_own(c));
        let with_charges = key(v, "ExpPack").is_some() && self.charges < CHARGES_MAX;
        let goal = seen_ones
            .iter()
            .filter(|_| with_charges)
            .filter(|c| !matches!(c.class, VehicleClass::Ship | VehicleClass::Submarine) && c.radius < BIG_SHIP_RADIUS && c.health > 0.0 && dist3(c.pos, me.pos) < FIGHTER_RADIUS)
            .filter(|c| enemy(c) && stopped(c) && !ctx.driver.vetoed.contains(&c.vehicle))
            .filter(|c| v.ground(c.pos[0], c.pos[2]).is_some_and(|h| c.pos[1] - h < 6.0))
            .min_by(|a, b| dist3(a.pos, me.pos).total_cmp(&dist3(b.pos, me.pos)).then(a.vehicle.cmp(&b.vehicle)))
            .filter(|c| !v.hidden(me.eye, [c.pos[0], c.pos[1] + 1.0, c.pos[2]]))
            .map(|c| c.vehicle);
        self.before = seen_ones.iter().map(|c| (c.vehicle, c.pos)).collect();
        self.before_t = v.time;
        if let Some(vehicle) = goal {
            self.chore = Chore::GoesToCharge { vehicle, since: v.time };
            ctx.cam.point = None;
            ctx.cam.path.clear();
            return;
        }
        // already at its flag, a trap on the approach from the enemy side
        if with_charges && self.fighter && self.trap.is_none() && let Some(c) = ctx.centre.filter(|c| flat(me.pos, *c) < NEAR_FLAG) {
            let far_from = |f: &crate::bot_api::Flag| flat(f.pos, c);
            if let Some(f) = flags.iter().filter(|f| f.team != me.team && f.team != 0 && far_from(f) > 1.0).min_by(|a, b| far_from(a).total_cmp(&far_from(b))) {
                let l = far_from(f);
                let place = [c[0] + (f.pos[0] - c[0]) / l * ACCESS, c[1] + (f.pos[2] - c[1]) / l * ACCESS];
                if v.ground(place[0], place[1]).is_some_and(|h| h > v.water() + 0.5) {
                    self.chore = Chore::GoesToPath { place, since: v.time };
                    ctx.cam.point = None;
                    ctx.cam.path.clear();
                    return;
                }
            }
        }
        // an enemy coming by land: mines across, ahead of where it is going
        if self.mines_laid >= MINES_CARRIED || key(v, "Landmine").is_none() {
            return;
        }
        let comes = seen_ones
            .iter()
            .filter(|c| matches!(c.class, VehicleClass::Tank | VehicleClass::Car | VehicleClass::Walker) && c.team != 0 && c.team != me.team && dist3(c.pos, me.pos) < SEES_COMING)
            .filter_map(|c| {
                let a = earlier_comes.iter().find(|a| a.0 == c.vehicle)?;
                let d = [c.pos[0] - a.1[0], c.pos[2] - a.1[2]];
                let l = d[0].hypot(d[1]);
                // it moves; where it would go if it carried on like that, at the point nearest to me, and not behind it
                if l < STOPPED_SPEED * dt || dt <= 0.5 || dt >= 3.0 {
                    return None;
                }
                let u = [d[0] / l, d[1] / l];
                let t = (me.pos[0] - c.pos[0]) * u[0] + (me.pos[2] - c.pos[2]) * u[1];
                let cut = [c.pos[0] + u[0] * t, c.pos[2] + u[1] * t];
                (t > 15.0 && flat(me.pos, cut) < CUT_OFF && !v.hidden(me.eye, [c.pos[0], c.pos[1] + 1.0, c.pos[2]])).then_some((c.vehicle, cut, u))
            })
            .min_by(|a, b| flat(me.pos, a.1).total_cmp(&flat(me.pos, b.1)).then(a.0.cmp(&b.0)));
        if let Some((_, cut, u)) = comes {
            let water = v.water();
            let mut places: Vec<[f32; 2]> = (0..MINES_CARRIED - self.mines_laid)
                .map(|k| (k as f32 - (MINES_CARRIED - self.mines_laid - 1) as f32 * 0.5) * MINE_SPACING)
                .map(|d| [cut[0] - u[1] * d, cut[1] + u[0] * d])
                .filter(|s| v.ground(s[0], s[1]).is_some_and(|h| h > water + 0.5))
                .collect();
            if places.first().zip(places.last()).is_some_and(|(a, b)| flat(me.pos, *b) < flat(me.pos, *a)) {
                places.reverse();
            }
            if !places.is_empty() {
                self.chore = Chore::Mines { places, since: v.time };
                ctx.cam.point = None;
                ctx.cam.path.clear();
            }
        }
    }

    fn go_to_charge(&mut self, v: &BotView, ctx: &mut Surroundings, vehicle: usize, since: f64) -> Option<Vec<(usize, f32)>> {
        let c = v.vehicle(vehicle).filter(|c| c.health > 0.0);
        let Some(c) = c.filter(|_| v.time - since < GO_TIME) else {
            ctx.driver.vetoed.push(vehicle);
            return self.finish_chore(v, ctx);
        };
        // still has path to go
        let near = c.radius.min(6.0) + NEAR_CHARGE;
        if let Some(axes) = self.walk_to(v, ctx, [c.pos[0], c.pos[2]], near) {
            return Some(axes);
        }
        ctx.driver.vetoed.push(vehicle);
        self.chore = Chore::Loading { vehicle: Some(vehicle), left: CHARGES_PER_VEHICLE, before: -1, since: v.time };
        Some(vec![(axis::THROTTLE, 0.0)])
    }

    fn load(&mut self, v: &BotView, _ctx: &mut Surroundings, vehicle: Option<usize>, left: u32, before: i32, since: f64) -> Option<Vec<(usize, f32)>> {
        let me = &v.me;
        let steps_aside = Chore::StepAside { spot: me.pos, since: v.time };
        let Some(k) = key(v, "ExpPack") else {
            self.chore = steps_aside;
            return Some(Vec::new());
        };
        if left == 0 || self.charges >= CHARGES_MAX || v.time - since > 6.0 {
            if self.charges > 0 {
                self.trap = Some(me.pos);
            }
            self.chore = steps_aside;
            return Some(Vec::new());
        }
        if !in_hand(v, "ExpPack") {
            return Some(vec![(axis::WEAPON1 + k - 1, 1.0), (axis::THROTTLE, 0.0)]);
        }
        let bullets = me.rounds.0 + me.rounds.1;
        let (left, before) = if before >= 0 && bullets < before {
            self.charges += 1;
            (left - 1, bullets)
        } else {
            (left, bullets)
        };
        self.chore = Chore::Loading { vehicle, left, before, since };
        self.pulse = !self.pulse;
        // (towards the vehicle, looking down: it falls at its feet)
        let g = vehicle.and_then(|j| v.vehicle(j)).map_or(0.0, |c| angle_between(me.view[2], [c.pos[0] - me.pos[0], c.pos[2] - me.pos[2]]));
        Some(vec![(axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)), (axis::FIRE, self.pulse as i32 as f32), (axis::THROTTLE, 0.0)])
    }

    /// Steps away and waits with the detonator in hand for someone to blow up, with none of its own nearby.
    fn step_aside(&mut self, v: &BotView, ctx: &mut Surroundings, spot: [f32; 3], since: f64) -> Option<Vec<(usize, f32)>> {
        let me = &v.me;
        let mut axes = Vec::with_capacity(6);
        let d = dist3(me.pos, spot);
        // the detonator: from the charges, with the secondary fire
        if !in_hand(v, "Detonator") {
            if !in_hand(v, "ExpPack")
                && let Some(k) = key(v, "ExpPack")
            {
                axes.push((axis::WEAPON1 + k - 1, 1.0));
            } else {
                self.pulse = !self.pulse;
                axes.push((axis::ALT_FIRE, self.pulse as i32 as f32));
            }
        }
        if d < ASIDE {
            // far from them, and where it will not fall
            let a = [me.pos[0] - spot[0], me.pos[2] - spot[2]];
            let l = a[0].hypot(a[1]).max(0.1);
            let g = angle_between(me.view[2], [a[0] / l, a[1] / l]);
            axes.push((axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)));
            axes.push((axis::THROTTLE, (g.abs() < 60.0) as i32 as f32));
            if ctx.cam.vehicle_jam(v, me.pos) > 1.5 {
                axes.push((axis::ACTION, 1.0));
                axes.push((axis::YAW, if ctx.player.is_multiple_of(2) { 1.0 } else { -1.0 }));
            }
            // has gone too long without managing to step away: gives up
            if v.time - since > 15.0 {
                return self.finish_chore(v, ctx);
            }
            return Some(axes);
        }
        let others = v.others();
        let friend_near = others.iter().any(|o| o.team == me.team && dist3(o.pos, spot) < CHARGE_RANGE + 2.0);
        let target = self.goal_at(v, spot, CHARGE_RANGE);
        let in_time = v.time - since > DETONATE_WAIT;
        if in_time {
            // nothing to blow up for now: leaves them laid
            return self.finish_chore(v, ctx);
        }
        if in_hand(v, "Detonator") && !friend_near && target {
            self.trap = None;
            self.chore = Chore::Detonate { until: v.time + 0.3 };
            axes.push((axis::FIRE, 1.0));
        }
        Some(axes)
    }

    /// If there is an enemy vehicle that would take a charge laid at `where`, or an enemy soldier well inside its reach.
    fn goal_at(&self, v: &BotView, spot: [f32; 3], radio: f32) -> bool {
        let me = &v.me;
        let flags = v.flags();
        v.others().iter().any(|o| o.team != me.team && !o.in_vehicle && dist3(o.pos, spot) < radio * 0.5) || v.vehicles().iter().any(|c| {
            let parked_own = c.team == 0 && flags.iter().filter(|f| dist3(f.pos, c.pos) < 80.0).min_by(|a, b| dist3(a.pos, c.pos).total_cmp(&dist3(b.pos, c.pos))).is_some_and(|f| f.team != me.team && f.team != 0);
            (parked_own || (c.team != 0 && c.team != me.team)) && c.health > 0.0 && dist3(c.pos, spot) < radio
        })
    }

    /// The charges it left laid: if something enemy passes there, goes for the detonator.
    fn look_trap(&mut self, v: &BotView, _ctx: &mut Surroundings) {
        let Some(spot) = self.trap else { return };
        if v.time < self.look_trap {
            return;
        }
        self.look_trap = v.time + 0.5;
        // (a vehicle crosses the zone in a couple of seconds: the detonator must be in hand before it arrives)
        if dist3(v.me.pos, spot) > ASIDE && self.goal_at(v, spot, SEES_APPROACH) {
            self.chore = Chore::StepAside { spot, since: v.time };
        }
    }
}
