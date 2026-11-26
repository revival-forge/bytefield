//! Supply depots: the ammo boxes, the medic lockers, the repair
//! points and the airfields' and carriers' rearming. Every half second (`handleUpdate`: more
//! than the template's interval, 0.5 s, since the last) a depot works out what it gives in that
//! time (`update`) and hands it to the alive soldiers (`workOnSoldiers`) and to the vehicles
//! (`workOnVehicles`) of its team (0: both) within its `radius`:
//!
//! - `addAmmoType <type> <amount> <rate> <regen>`: `rate` rounds a second (the time's share,
//!   rounded, and the remainder kept) to each weapon of that `ammoType`;
//!   `amount` the stock (−1: endless) that grows back `regen` a second;
//! - `setHealth <amount> <rate> <regen>` (−1, 0.5, 0.25 by default: a depot heals unless told
//!   `setHealth 0 0 0`): `rate` hit points a second to a soldier (time × rate);
//! - `addVehicleType <template> <amount> <repair> <regen>`: `repair` hit points to a vehicle of
//!   that template each update, whatever the time; not to one abandoned past
//!   its time run out).
//!
//! A soldier in a vehicle only gets what a depot of that vehicle gives (the M3A1's); a depot
//! on a destroyed object gives nothing. Ammo types in vanilla: 0 the land vehicles' guns (the
//! default), 1 the soldiers' fire arms, 2 grenades, explosives and mines, 3 the bazookas, 7–10
//! the planes' bombs and guns, 14–15 the boats'.

use std::sync::Arc;

use bytefield_con::templates::Template;
use serde::Serialize;

use crate::vecmath::V3;

/// The supply depot's update handling: at most every this long (the template's).
pub const UPDATE_INTERVAL: f32 = 0.5;

/// `addAmmoType`: (`ammoType`, stock, rounds a second, stock back a second).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AmmoSupply {
    pub ammo_type: i32,
    pub amount: f32,
    pub rate: f32,
    pub regen: f32,
}

/// `addVehicleType`: (template, stock, hit points an update, stock back a second).
#[derive(Debug, Clone, Serialize)]
pub struct Repair {
    pub template: String,
    pub amount: f32,
    pub repair: f32,
    pub regen: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepotTemplate {
    pub name: String,
    /// `radius` (2)
    pub radius: f32,
    /// `team` (0: both)
    pub team: u32,
    /// `workOnSoldiers` (1), `workOnVehicles` (0)
    pub soldiers: bool,
    pub vehicles: bool,
    pub ammo: Vec<AmmoSupply>,
    /// `setHealth`: stock (−1), hit points a second (0.5), stock back a second
    /// (0.25), the stock's cap (only `setHealth` sets it)
    pub health: [f32; 3],
    pub health_cap: f32,
    pub repairs: Vec<Repair>,
}

impl DepotTemplate {
    /// A `SupplyDepot` template's data; None for another kind.
    pub fn from_template(t: &Template) -> Option<DepotTemplate> {
        if !t.kind.eq_ignore_ascii_case("supplydepot") {
            return None;
        }
        let first = |n: &str| t.get(n).and_then(|a| a.first()).map(|v| v.trim().to_string());
        let num = |n: &str, d: f32| first(n).and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        let args = |a: &[String]| -> [f32; 4] {
            let mut out = [0.0; 4];
            for (o, v) in out.iter_mut().zip(a) {
                *o = v.trim().parse().unwrap_or(0.0);
            }
            out
        };
        let mut health = [-1.0, 0.5, 0.25];
        let mut health_cap = 0.0;
        if let Some(a) = t.get("sethealth") {
            let [amount, rate, regen, _] = args(a);
            // (an int: `(float)param_1`)
            health = [amount.trunc(), rate, regen];
            health_cap = amount.trunc();
        }
        Some(DepotTemplate {
            name: t.name.clone(),
            radius: num("radius", 2.0),
            team: first("team").and_then(|v| v.parse().ok()).unwrap_or(0),
            soldiers: first("workonsoldiers").is_none_or(|v| v != "0"),
            vehicles: first("workonvehicles").is_some_and(|v| v != "0"),
            ammo: t
                .all("addammotype")
                .into_iter()
                .map(|a| {
                    let [k, amount, rate, regen] = args(a);
                    AmmoSupply { ammo_type: k as i32, amount: amount.trunc(), rate, regen }
                })
                .collect(),
            health,
            health_cap,
            repairs: t
                .all("addvehicletype")
                .into_iter()
                .filter_map(|a| {
                    let name = a.first()?.trim().to_string();
                    let [amount, repair, regen, _] = args(&a[1..]);
                    Some(Repair { template: name, amount: amount.trunc(), repair, regen })
                })
                .collect(),
        })
    }

    /// `isHealing`: it heals soldiers.
    pub fn heals(&self) -> bool {
        self.health[1] > 0.0
    }
}

/// The HUD's three supply icons ([`crate::Sim::supply_icons`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct SupplyIcons {
    pub heal: bool,
    pub repair: bool,
    pub ammo: bool,
}

/// What a depot gave.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Given {
    /// rounds into a weapon (vehicle, its weapon's index in the soldier's or vehicle's list)
    Ammo { weapon: usize, rounds: u32 },
    /// hit points to a soldier
    Heal(f32),
    /// hit points to a vehicle
    Repair(f32),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SupplyEvent {
    pub step: u64,
    pub depot: usize,
    pub vehicle: usize,
    pub given: Given,
}

/// A depot in the world (`SupplyDepot`).
#[derive(Debug, Clone, Serialize)]
pub struct Depot {
    #[serde(skip)]
    pub template: Arc<DepotTemplate>,
    /// on a vehicle: (its index, the depot's place in its frame); None: the level's
    pub on: Option<(usize, V3)>,
    /// `on` is a projectile (`projectiles`), not a vehicle: GC's thrown repair droids and ammo;
    /// it stops when the projectile ends
    pub on_projectile: bool,
    pub pos: V3,
    /// the template's
    pub team: u32,
    /// per `addAmmoType`: the stock, the clock, the rounds of this update
    pub stock: Vec<f32>,
    pub clock: Vec<f32>,
    pub rounds: Vec<i32>,
    /// the health stock (−1: endless) and what it heals in this update
    pub health: i32,
    pub heal: f32,
    /// per `addVehicleType`, the stock
    pub repair_stock: Vec<f32>,
    /// some rounds to give in this update
    pub reloading: bool,
    /// the world time of the last update
    pub last: f32,
}

impl Depot {
    /// The supply depot's constructor: the stocks full, the clocks at 0; the last update at a
    /// random frame of the first second (`rand % fps / fps`: `phase`).
    pub fn new(template: Arc<DepotTemplate>, on: Option<(usize, V3)>, pos: V3, phase: f32) -> Depot {
        let n = template.ammo.len();
        Depot {
            team: template.team,
            stock: template.ammo.iter().map(|a| a.amount).collect(),
            clock: vec![0.0; n],
            rounds: vec![0; n],
            health: template.health[0].round_ties_even() as i32,
            heal: 0.0,
            repair_stock: template.repairs.iter().map(|r| r.amount).collect(),
            reloading: false,
            last: phase,
            template,
            on,
            on_projectile: false,
            pos,
        }
    }

    /// `handleUpdate`: the time since the last update, if more than [`UPDATE_INTERVAL`] (the
    /// update is then due, and the clock restarts).
    pub fn due(&mut self, world_time: f32) -> Option<f32> {
        let dt = world_time - self.last;
        (dt > UPDATE_INTERVAL).then(|| {
            self.last = world_time;
            dt
        })
    }

    /// `update`'s stocks and clocks for `dt` seconds (before it works on anyone): the ammo
    /// stocks grow back and each type's clock gives its rounds; the health stock grows back and
    /// the heal is `dt` × rate; the vehicles' stocks grow back.
    pub fn update(&mut self, dt: f32) {
        let t = self.template.clone();
        self.reloading = false;
        for (k, a) in t.ammo.iter().enumerate() {
            if self.stock[k] != -1.0 {
                self.stock[k] = (self.stock[k] + dt * a.regen).min(a.amount);
            }
            if self.stock[k] != 0.0 {
                self.clock[k] -= dt;
                if self.clock[k] < 0.0 {
                    let n = (-self.clock[k] * a.rate).round_ties_even() as i32;
                    self.rounds[k] = n;
                    if n > 0 {
                        self.clock[k] += n as f32 * (1.0 / a.rate);
                        self.reloading = true;
                    }
                }
            }
        }
        if t.soldiers && t.heals() {
            if self.health != -1 {
                self.health += (dt * t.health[2]).round_ties_even() as i32;
                if t.health_cap < self.health as f32 {
                    self.health = t.health_cap.round_ties_even() as i32;
                }
            }
            self.heal = dt * t.health[1];
        }
        if t.vehicles && !self.repair_stock.is_empty() {
            for (k, r) in t.repairs.iter().enumerate() {
                if self.repair_stock[k] != -1.0 {
                    self.repair_stock[k] = (self.repair_stock[k] + dt * r.regen).min(r.amount);
                }
            }
        }
    }

    /// `reloadAmmo` for one weapon of `ammo_type`: the rounds to offer it from each matching
    /// type, then what it took back from the stock (`took`: the weapon's reload rounds
    /// left over). Calls `reload(n) -> left over` once per matching type.
    pub fn reload(&mut self, ammo_type: i32, mut reload: impl FnMut(u32) -> u32) -> u32 {
        let mut given = 0;
        for k in 0..self.template.ammo.len() {
            let n = self.rounds[k];
            if n == 0 || self.template.ammo[k].ammo_type != ammo_type {
                continue;
            }
            let stock = self.stock[k];
            if stock == -1.0 {
                given += n as u32 - reload(n as u32);
            } else if stock <= n as f32 {
                let offer = stock.round_ties_even() as u32;
                let left = reload(offer);
                // (as written: the stock becomes what was left over)
                self.stock[k] = left as f32;
                given += offer - left;
            } else {
                let left = reload(n as u32);
                self.stock[k] -= (n as u32 - left) as f32;
                given += n as u32 - left;
            }
        }
        given
    }

    /// `healSoldier`: what to heal a soldier with `hp` of `max_hp` (always the update's heal
    /// from an endless stock; from a stock, at most the stock, which loses what the soldier
    /// lacked of it).
    pub fn heal_soldier(&mut self, hp: f32, max_hp: f32) -> f32 {
        if self.health == -1 {
            return self.heal;
        }
        let lacking = max_hp - hp;
        if self.health as f32 <= self.heal {
            let h = self.health as f32;
            let over = h - lacking;
            let used = if over >= 0.0 { h - over } else { h };
            self.health -= used.round_ties_even() as i32;
            h
        } else {
            let over = self.heal - lacking;
            let used = if over >= 0.0 { self.heal - over } else { self.heal };
            self.health -= used.round_ties_even() as i32;
            self.heal
        }
    }

    /// `repairVehicle`: what to repair a vehicle of `template` with `hp` of `max_hp` (None: not
    /// one of its types), the stock taking what the vehicle lacked of it.
    pub fn repair_vehicle(&mut self, template: &str, hp: f32, max_hp: f32) -> Option<f32> {
        let k = self.template.repairs.iter().position(|r| r.template.eq_ignore_ascii_case(template))?;
        let repair = self.template.repairs[k].repair;
        let stock = self.repair_stock[k];
        if stock == -1.0 {
            return Some(repair);
        }
        let lacking = max_hp - hp;
        let amount = if stock <= repair { stock } else { repair };
        let over = amount - lacking;
        let used = if over >= 0.0 { amount - over } else { amount };
        self.repair_stock[k] -= used;
        Some(amount)
    }

    /// A depot on a vehicle follows it: its place from the vehicle's body.
    pub fn follow(&mut self, body_pos: V3, rows: &[V3; 3]) {
        if let Some((_, l)) = self.on {
            let p = [0, 1, 2].map(|k| l[0] * rows[0][k] + l[1] * rows[1][k] + l[2] * rows[2][k]);
            self.pos = [body_pos[0] + p[0], body_pos[1] + p[1], body_pos[2] + p[2]];
        }
    }

    pub fn within(&self, p: V3) -> bool {
        let d = [p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() <= self.template.radius
    }

    pub fn serves(&self, team: u32) -> bool {
        self.team == 0 || self.team == team
    }
}

impl crate::World {
    /// A `SupplyDepot` template's data.
    pub fn depot_template(&self, name: &str) -> Option<DepotTemplate> {
        DepotTemplate::from_template(self.db.object_template(name)?)
    }

    /// The level's depots (the placed objects' and their children's): (template, place).
    pub fn level_depots(&self) -> Vec<(String, V3)> {
        use bytefield_world::scene::Mat4;
        let vec3 = |args: &[String]| args.first().and_then(|a| bytefield_world::parse_vec(a)).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        let mut out = Vec::new();
        for inst in &self.db.instances {
            let get = |n: &str| inst.props.iter().rev().find(|p| p.name == n).and_then(|p| vec3(&p.args));
            let Some(pos) = get("absoluteposition") else {
                continue;
            };
            let rot = get("rotation").or_else(|| get("setrotation")).unwrap_or([0.0; 3]);
            let m = crate::ref2::placed(pos, rot);
            let m = Mat4(std::array::from_fn(|r| std::array::from_fn(|c| m[4 * r + c] as f64)));
            let own = self.db.object_template(&inst.template).is_some_and(|t| t.kind.eq_ignore_ascii_case("supplydepot"));
            let mut found = bytefield_world::child_templates_of_kind(&self.db, &inst.template, "supplydepot");
            if own {
                found.push((inst.template.clone(), Mat4::IDENTITY));
            }
            for (name, local) in found {
                let p = m.mul(&local).transform_point([0.0; 3]);
                out.push((name, p.map(|x| x as f32)));
            }
        }
        out
    }
}

/// `rand % fps / fps` for the `k`th depot (ours: a fixed spread, not the game's random
/// numbers).
pub(crate) fn phase(k: usize) -> f32 {
    let fps = crate::contact::FPS.round() as u64;
    ((k as u64).wrapping_mul(2_654_435_761) % fps) as f32 / fps as f32
}

impl crate::Sim<'_> {
    /// Which of the three supply icons the HUD shows `player` (`SupplyDepot::show{Heal,Repair,Ammo}IconInMenu`, asked of
    /// every depot): only what the depot could give them from where they are; nothing says the stock lasts or that they lack it.
    /// TODO: the repair and the vehicle's ammo measure from the seat's object in the original; here from the vehicle's root.
    pub fn supply_icons(&self, player: usize) -> SupplyIcons {
        let mut out = SupplyIcons::default();
        let p = &self.players[player];
        let Some(s) = p.soldier.filter(|_| p.alive).map(|i| &self.vehicles[i]) else { return out };
        let kit = |d: &Depot| s.soldier().is_some_and(|m| m.weapon.iter().chain(m.stowed.iter()).any(|w| d.template.ammo.iter().any(|a| a.ammo_type == w.template.ammo_type)));
        let ride = s.entry.inside.map(|k| (k, &self.vehicles[k], s.entry.position));
        for d in self.depots.iter().filter(|d| d.team == 0 || d.team == p.team) {
            let t = &d.template;
            let same = ride.is_some_and(|(k, ..)| !d.on_projectile && d.on.is_some_and(|o| o.0 == k));
            let near = d.within(s.craft.body.pos);
            out.heal |= t.soldiers && (ride.is_none() || same) && near && t.heals();
            match ride {
                None => out.ammo |= t.soldiers && near && kit(d),
                Some((_, v, seat)) => {
                    let near_root = d.within(v.craft.body.pos);
                    let pco = v.entry.seats.get(seat).map_or(v.template.as_str(), |x| x.pco.as_str());
                    out.repair |= t.vehicles && near_root && t.repairs.iter().any(|r| r.template.eq_ignore_ascii_case(pco));
                    // the driver's seat answers for all the vehicle's weapons (no parent: the whole tree)
                    let guns = v.weapons.iter().filter(|(part, _)| seat == 0 || v.craft.parts[*part].seat == seat).any(|(_, w)| t.ammo.iter().any(|a| a.ammo_type == w.template.ammo_type));
                    out.ammo |= (t.vehicles && near_root && guns) || (t.soldiers && same && near && kit(d));
                }
            }
        }
        out
    }

    /// The depots in vehicle `i`'s tree (the M3A1's, the Hanomag's, a carrier's), following it.
    pub(crate) fn add_vehicle_depots(&mut self, i: usize) {
        let t = self.vehicles[i].template.clone();
        for (name, m) in bytefield_world::child_templates_of_kind(&self.world.db, &t, "supplydepot") {
            let Some(dt) = self.world.depot_template(&name) else {
                continue;
            };
            let c = m.0;
            let local = [c[3][0] as f32, c[3][1] as f32, c[3][2] as f32];
            let mut d = Depot::new(Arc::new(dt), Some((i, local)), [0.0; 3], phase(self.depots.len()));
            let b = &self.vehicles[i].craft.body;
            d.follow(b.pos, &b.rows);
            self.depots.push(d);
        }
    }

    /// The depots in projectile `k`'s tree (GC's thrown repair droids and ammo), following it
    /// while it lasts.
    pub(crate) fn add_projectile_depots(&mut self, k: usize, template: &str) {
        for (name, m) in bytefield_world::child_templates_of_kind(&self.world.db, template, "supplydepot") {
            let Some(dt) = self.world.depot_template(&name) else {
                continue;
            };
            let c = m.0;
            let local = [c[3][0] as f32, c[3][1] as f32, c[3][2] as f32];
            let mut d = Depot::new(Arc::new(dt), Some((k, local)), [0.0; 3], phase(self.depots.len()));
            d.on_projectile = true;
            let q = &self.projectiles[k];
            d.follow(q.pos, &q.rows);
            self.depots.push(d);
        }
    }

    /// The level's depots, once.
    fn place_depots(&mut self) {
        self.depots_placed = true;
        for (name, pos) in self.world.level_depots() {
            if let Some(t) = self.world.depot_template(&name) {
                let d = Depot::new(Arc::new(t), None, pos, phase(self.depots.len()));
                self.depots.push(d);
            }
        }
    }

    /// Every depot's `handleUpdate` in this frame.
    pub(crate) fn supply(&mut self) {
        if !self.depots_placed {
            self.place_depots();
        }
        // the frame's world time (it stays 0 before the game plays)
        let wt = if self.round.playing { (self.clock.time - self.clock.dt) as f32 } else { 0.0 };
        for d in 0..self.depots.len() {
            if let Some((p, _)) = self.depots[d].on.filter(|_| self.depots[d].on_projectile) {
                let Some(q) = self.projectiles.get(p).filter(|q| q.alive()) else { continue };
                let (pos, rows) = (q.pos, q.rows);
                self.depots[d].follow(pos, &rows);
            } else if let Some((v, _)) = self.depots[d].on {
                let veh = &self.vehicles[v];
                if veh.removed {
                    continue;
                }
                let (p, r) = (veh.craft.body.pos, veh.craft.body.rows);
                self.depots[d].follow(p, &r);
            }
            let Some(dt) = self.depots[d].due(wt) else {
                continue;
            };
            // (on a destroyed object it does nothing)
            if !self.depots[d].on_projectile && self.depots[d].on.is_some_and(|(v, _)| self.vehicles[v].armor.as_ref().is_some_and(|a| a.destroyed)) {
                continue;
            }
            self.depots[d].update(dt);
            let t = self.depots[d].template.clone();
            if t.soldiers {
                self.supply_soldiers(d);
            }
            if t.vehicles {
                self.supply_vehicles(d);
            }
        }
    }

    /// `workOnSoldiers`: each alive player's soldier of its team within its radius (one in a
    /// vehicle, only from that vehicle's depots) gets the rounds (`reloadAmmo`) and the heal
    /// (`healSoldier`).
    fn supply_soldiers(&mut self, d: usize) {
        let step = self.clock.steps;
        let heals = self.depots[d].template.heals();
        for p in 0..self.players.len() {
            let (alive, team, Some(s)) = (self.players[p].alive, self.players[p].team, self.players[p].soldier) else {
                continue;
            };
            let depot = &mut self.depots[d];
            let v = &mut self.vehicles[s];
            if !alive || v.removed || v.soldier().is_none() {
                continue;
            }
            if v.entry.inside.is_some() && v.entry.inside != depot.on.filter(|_| !depot.on_projectile).map(|o| o.0) {
                continue;
            }
            if !depot.serves(team) || !depot.within(v.craft.body.pos) {
                continue;
            }
            if depot.reloading {
                let m = v.soldier_mut().unwrap();
                for (k, w) in m.weapon.iter_mut().chain(m.stowed.iter_mut()).enumerate() {
                    let given = depot.reload(w.template.ammo_type, |n| w.reload_ammo(n));
                    if given > 0 {
                        self.journal.supply_log.push(SupplyEvent { step, depot: d, vehicle: s, given: Given::Ammo { weapon: k, rounds: given } });
                    }
                }
            }
            if heals && let Some(a) = v.armor.as_mut() {
                let (hp, max) = (a.hp, a.max_hp);
                let amount = depot.heal_soldier(hp, max);
                a.give(-amount);
                if a.hp > hp {
                    self.journal.supply_log.push(SupplyEvent { step, depot: d, vehicle: s, given: Given::Heal(a.hp - hp) });
                }
            }
        }
    }

    /// `workOnVehicles`: each vehicle (not a soldier) of its team within its radius, manned or
    /// not, gets the rounds and, if of one of its types and not abandoned past its time, the
    /// repair.
    fn supply_vehicles(&mut self, d: usize) {
        let step = self.clock.steps;
        let repairs = !self.depots[d].template.repairs.is_empty();
        for i in 0..self.vehicles.len() {
            let depot = &mut self.depots[d];
            let v = &mut self.vehicles[i];
            if !v.in_world() || v.soldier().is_some() || !depot.serves(v.team) || !depot.within(v.craft.body.pos) {
                continue;
            }
            if depot.reloading {
                for (k, (_, w)) in v.weapons.iter_mut().enumerate() {
                    let given = depot.reload(w.template.ammo_type, |n| w.reload_ammo(n));
                    if given > 0 {
                        self.journal.supply_log.push(SupplyEvent { step, depot: d, vehicle: i, given: Given::Ammo { weapon: k, rounds: given } });
                    }
                }
            }
            if repairs
                && !v.entry.abandon.as_ref().is_some_and(|a| a.timer == 0.0)
                && let Some(a) = v.armor.as_mut()
            {
                let (hp, max) = (a.hp, a.max_hp);
                if let Some(amount) = depot.repair_vehicle(&v.body.template, hp, max) {
                    a.give(-amount);
                    if a.hp > hp {
                        self.journal.supply_log.push(SupplyEvent { step, depot: d, vehicle: i, given: Given::Repair(a.hp - hp) });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ObjectTemplate.create SupplyDepot D` and then these `ObjectTemplate.<command> <args>`.
    fn template(con: &str) -> DepotTemplate {
        let props = con
            .lines()
            .filter_map(|l| {
                let mut w = l.strip_prefix("ObjectTemplate.")?.split_whitespace();
                let name = w.next()?.to_ascii_lowercase();
                (name != "create").then(|| bytefield_con::templates::Property { name, args: w.map(str::to_string).collect() })
            })
            .collect();
        let t = Template { namespace: "objecttemplate".into(), kind: "SupplyDepot".into(), name: "D".into(), props, origin: String::new(), seq: 0, create_args: Vec::new() };
        DepotTemplate::from_template(&t).unwrap()
    }

    #[test]
    fn an_ammo_box_gives_its_rate_in_rounds_and_keeps_the_remainder() {
        let t = template("ObjectTemplate.create SupplyDepot D\nObjectTemplate.radius 3\nObjectTemplate.setHealth 0 0 0\nObjectTemplate.addAmmoType 1 -1 15 0\nObjectTemplate.addAmmoType 2 -1 1.2 0\n");
        assert_eq!(t.radius, 3.0);
        assert!(!t.heals() && t.soldiers && !t.vehicles);
        let mut d = Depot::new(Arc::new(t), None, [0.0; 3], 0.0);
        // a frame past the half second (30 a second)
        let dt = 16.0 / 30.0;
        d.update(dt);
        assert_eq!(d.rounds, vec![8, 1]);
        assert!(d.reloading);
        // the rifle takes 8; a grenade's type gives one
        let mut got = Vec::new();
        assert_eq!(
            d.reload(1, |n| {
                got.push(n);
                0
            }),
            8
        );
        assert_eq!(d.reload(2, |n| n - 1), 1);
        assert_eq!(d.reload(3, |_| unreachable!()), 0);
        assert_eq!(got, vec![8]);
        // over two seconds, 30 rounds and 2 or 3 grenades
        let mut total = [8, 1];
        for _ in 0..3 {
            d.update(dt);
            total[0] += d.rounds[0];
            total[1] += d.rounds[1];
        }
        assert_eq!(total[0], 32);
        assert!((2..=3).contains(&total[1]), "{total:?}");
    }

    #[test]
    fn a_depot_heals_by_default_and_a_locker_heals_its_rate() {
        let t = template("ObjectTemplate.create SupplyDepot D\nObjectTemplate.addVehicleType tiger -1 4 0\n");
        assert!(t.heals() && t.soldiers && !t.vehicles);
        assert_eq!(t.health, [-1.0, 0.5, 0.25]);
        assert_eq!(t.repairs.len(), 1);
        assert_eq!((t.repairs[0].amount, t.repairs[0].repair), (-1.0, 4.0));
        let t = template("ObjectTemplate.create SupplyDepot D\nObjectTemplate.radius 2\nObjectTemplate.workOnVehicles 0\nObjectTemplate.workOnSoldiers 1\nObjectTemplate.setHealth -1 4.0 0\n");
        let mut d = Depot::new(Arc::new(t), None, [0.0; 3], 0.0);
        d.update(0.6);
        assert!((d.heal_soldier(10.0, 30.0) - 2.4).abs() < 1e-6);
    }

    #[test]
    fn a_repair_point_repairs_its_amount_each_update_and_only_its_types() {
        let t = template("ObjectTemplate.create SupplyDepot D\nObjectTemplate.workOnVehicles 1\nObjectTemplate.addVehicleType Tiger -1 4 0\nObjectTemplate.addVehicleType willy 10 2 1\n");
        let mut d = Depot::new(Arc::new(t), None, [0.0; 3], 0.0);
        d.update(3.0);
        assert_eq!(d.repair_vehicle("tiger", 10.0, 100.0), Some(4.0));
        assert_eq!(d.repair_vehicle("sherman", 10.0, 100.0), None);
        // a stock of 10: it gives 2 an update, and loses what the jeep lacked of it
        assert_eq!(d.repair_vehicle("willy", 99.0, 100.0), Some(2.0));
        assert_eq!(d.repair_stock[1], 9.0);
    }

    #[test]
    fn the_update_is_due_past_half_a_second() {
        let t = template("ObjectTemplate.create SupplyDepot D\n");
        let mut d = Depot::new(Arc::new(t), None, [0.0; 3], 0.2);
        assert_eq!(d.due(0.7), None);
        assert!(d.due(0.7334).is_some());
        assert_eq!(d.last, 0.7334);
    }

    fn game() -> Option<std::path::PathBuf> {
        let game = bytefield_testdata::game();
        game.is_dir().then_some(game)
    }

    /// El Alamein's depots: ammo boxes for soldiers (and their vehicles' twin), the repair
    /// points; a soldier with his magazines empty beside a box gets them back at the box's rate
    /// (15 rounds a second to each fire arm, 1.2 grenades), and not 4 m away (radius 3).
    #[test]
    fn el_alamein_ammo_box_refills_a_soldier() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let depots = w.level_depots();
        let count = |n: &str| depots.iter().filter(|(t, _)| t.eq_ignore_ascii_case(n)).count();
        eprintln!("{} depots: {:?}", depots.len(), depots.iter().map(|d| d.0.as_str()).collect::<std::collections::BTreeSet<_>>());
        assert!(count("AmmoboxSupplyDepot") > 0 && count("repairpoint") > 0);
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let a = s.add_player(1, "GermanSoldier", false);
        for _ in 0..30 * 30 {
            s.step();
            if s.players[a].alive {
                break;
            }
        }
        let sol = s.players[a].soldier.unwrap();
        let box_at = s.depots.iter().find(|d| d.template.name.eq_ignore_ascii_case("AmmoboxSupplyDepot")).unwrap().pos;
        let empty = |s: &mut crate::Sim| {
            let m = s.vehicles[sol].soldier_mut().unwrap();
            for w in m.weapon.iter_mut().chain(m.stowed.iter_mut()) {
                for r in w.mags.iter_mut() {
                    *r = 0;
                }
            }
        };
        let total = |s: &crate::Sim, k: i32| -> i32 {
            let m = s.vehicles[sol].soldier().unwrap();
            m.weapon.iter().chain(m.stowed.iter()).filter(|w| w.template.ammo_type == k).map(|w| w.mags.iter().sum::<i32>()).sum()
        };
        empty(&mut s);
        // the box is inside a barrack, on its floor: the soldier stands on it (his position 1 m
        // up, as in the original), not in it, where the plane of material 99 under it kills him
        let box_at = [box_at[0], box_at[1] + 1.0, box_at[2]];
        s.vehicles[sol].craft.body.pos = [box_at[0] + 4.0, box_at[1], box_at[2]];
        s.vehicles[sol].craft.body.vel = [0.0; 3];
        for _ in 0..60 {
            s.step();
            s.vehicles[sol].craft.body.pos = [box_at[0] + 4.0, box_at[1], box_at[2]];
        }
        assert_eq!(total(&s, 1), 0, "4 m away: nothing");
        let seen = s.journal.supply_log.len();
        for _ in 0..60 {
            s.step();
            s.vehicles[sol].craft.body.pos = [box_at[0] + 1.0, box_at[1], box_at[2]];
        }
        let (rifle, grenades) = (total(&s, 1), total(&s, 2));
        eprintln!("2 s at the box: {rifle} rounds, {grenades} grenades; {:?}", &s.journal.supply_log[seen..]);
        // each update (16 frames) gives each fire arm 8 rounds (fewer once full), each grenade
        // weapon one or none
        let ammo: Vec<_> = s.journal.supply_log[seen..]
            .iter()
            .filter_map(|e| match e.given {
                Given::Ammo { weapon, rounds } => Some((e.step, weapon, rounds)),
                _ => None,
            })
            .collect();
        let steps: std::collections::BTreeSet<_> = ammo.iter().map(|a| a.0).collect();
        assert_eq!(steps.iter().zip(steps.iter().skip(1)).map(|(a, b)| b - a).collect::<Vec<_>>(), vec![16; steps.len() - 1]);
        assert!(ammo.iter().all(|a| a.2 <= 8));
        assert!(rifle >= 3 * 8 - 1, "{rifle}");
        assert!((2..=3).contains(&grenades), "{grenades}");
    }

    /// A tank hurt at El Alamein's repair point gets 4 hit points each update (twice a second
    /// or so), manned or not.
    #[test]
    fn el_alamein_repair_point_repairs_a_tank() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.step();
        let rp = s.depots.iter().find(|d| d.template.name.eq_ignore_ascii_case("repairpoint")).unwrap().pos;
        let Some(t) = s.spawn_vehicle("Tiger", [rp[0], rp[1] + 0.5, rp[2]], [0.0; 3]) else { panic!("no Tiger") };
        // hurt, not burning (above its critical damage)
        let a = s.vehicles[t].armor.as_ref().unwrap();
        let hurt = (a.max_hp - a.template.critical_damage) * 0.5;
        s.give_damage(t, hurt, crate::damage::Source::Hit);
        let seen = s.journal.supply_log.len();
        for _ in 0..100 {
            s.step();
        }
        let repairs: Vec<_> = s.journal.supply_log[seen..].iter().filter(|e| e.vehicle == t).map(|e| (e.step, e.given)).collect();
        eprintln!("Tiger: {repairs:?} hp {:?} at {:?}", s.vehicles[t].armor.as_ref().map(|a| (a.hp, a.max_hp)), s.vehicles[t].craft.body.pos);
        assert!(repairs.len() >= 4, "{repairs:?}");
        assert!(repairs.iter().all(|r| r.1 == Given::Repair(4.0)));
        assert!(repairs.windows(2).all(|w| w[1].0 - w[0].0 == 16), "{repairs:?}");
    }

    /// A soldier hurt by 20 at El Alamein's medic locker (`setHealth -1 4.0 0`) gets 4 hit points
    /// a second, 2.133 each update of 16 frames, as the original (reference run `supply_medlocker`: 10 →
    /// 12.133 → 14.133 …, 2.0 when the update came after 15 frames).
    #[test]
    fn el_alamein_medic_locker_heals_a_soldier() {
        let Some(game) = game() else { return };
        let w = crate::World::load(&game, "bf1942", "El_Alamein", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        let a = s.add_player(1, "GermanSoldier", false);
        for _ in 0..30 * 30 {
            s.step();
            if s.players[a].alive {
                break;
            }
        }
        let sol = s.players[a].soldier.unwrap();
        let at = s.depots.iter().find(|d| d.template.name.eq_ignore_ascii_case("mediclockerRepairpoint")).unwrap().pos;
        s.give_damage(sol, 20.0, crate::damage::Source::Hit);
        let seen = s.journal.supply_log.len();
        for _ in 0..70 {
            s.step();
            s.vehicles[sol].craft.body.pos = [at[0], at[1] + 0.5, at[2] + 0.5];
        }
        let heals: Vec<_> = s.journal.supply_log[seen..].iter().filter(|e| e.vehicle == sol).map(|e| e.given).collect();
        eprintln!("{heals:?}");
        assert!(heals.len() >= 3);
        // (16 frames, or 15 when the clock's float rounding lets the update in a frame early)
        assert!(heals.iter().all(|g| matches!(g, Given::Heal(h) if (h - 16.0 / 30.0 * 4.0).abs() < 1e-4 || (h - 2.0).abs() < 1e-4)), "{heals:?}");
    }
}
