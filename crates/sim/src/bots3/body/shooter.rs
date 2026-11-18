//! Aiming and firing: what a player does with the mouse and the trigger. Sees the nearest enemy not hidden by a wall, takes time to
//! react, aims with an aiming error, fires at its weapon's rhythm (bursts for the automatic, one shot and a pause for the
//! bolt-action), reloads, switches to the pistol when out of ammunition, throws a grenade now and then and does not fire if a mate is on
//! the line of fire. It has its own state; the random generator, which belongs to the bot, is lent to it on each call.
use super::weapons::{self, WeaponClass};
use super::rng::Rng;
use super::{TURN, CHEST, pitch, dist3, angle_between};
use crate::bot_api::{BotView, Other};
use crate::soldier::axis;

/// Overall adjustments, measured against the Berlin-8 targets.
pub const PAUSE_SCALE: f32 = 1.0;
/// The pause between bursts from a vehicle, over the one on foot (row 4: 29 % of the shots are from vehicles).
pub const VEHICLE_SCALE: f32 = 1.0;
pub const MEDIAN_REACTION: f32 = 0.6;
/// Seconds that fire lasts without pauses for whoever has just been shot.
pub const HARASSED: f64 = 3.0;
/// The pause between bursts for whoever is being shot at, over the normal one.
pub const HARASSED_SCALE: f32 = 0.3;
/// Aiming error of each burst, degrees (standard deviation).
pub const AIM_ERROR: f32 = 0.55;
/// Probability of throwing a grenade, when starting a burst with the enemy between 10 and 28 m.
pub const P_GRENADE: f32 = 0.06;
/// Probability of using the pistol when starting a fight even with ammunition.
pub const P_PISTOL: f32 = 0.05;
/// Assumed speed of a ship's shell, m/s.
pub const SHELL_SPEED: f32 = 250.0;

/// Whether, seen from above, `point` falls in the corridor a shell fired from `eye` to `target` passes through and bursts in.
pub fn in_corridor(eye: [f32; 3], target: [f32; 3], point: [f32; 3]) -> bool {
    let (dx, dz) = (target[0] - eye[0], target[2] - eye[2]);
    let l = (dx * dx + dz * dz).sqrt().max(1.0);
    let (px, pz) = (point[0] - eye[0], point[2] - eye[2]);
    let a = (px * dx + pz * dz) / l;
    let angle_between = (px * dz - pz * dx).abs() / l;
    a > 0.6 * l && a < l + 40.0 && angle_between < 20.0
}

/// Whether a mate is on the line of fire from `eye` to `target`: caution, not something observed.
pub fn friend_in_line(eye: [f32; 3], target: [f32; 3], amigos: &[[f32; 3]]) -> bool {
    let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let l2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if l2 <= 0.0 {
        return false;
    }
    amigos.iter().any(|a| {
        let p = [a[0] - eye[0], a[1] + CHEST - eye[1], a[2] - eye[2]];
        let t = (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) / l2;
        if !(0.0..=1.05).contains(&t) {
            return false;
        }
        let q = [p[0] - t * d[0], p[1] - t * d[1], p[2] - t * d[2]];
        // a metre and a half around the line
        q[0] * q[0] + q[1] * q[1] + q[2] * q[2] < 1.5 * 1.5
    })
}

/// The slot of the weapon in hand (by its name, among what it carries).
pub fn slot_in_hand(me: &crate::bot_api::Me) -> Option<usize> {
    let w = me.weapon.as_deref()?;
    me.items.iter().find(|i| i.1 == w).map(|i| i.0)
}

/// Where a soldier on foot holds the weapon (roughly): at eye height standing, crouched or not.
pub fn weapon_of(pos: [f32; 3]) -> [f32; 3] {
    [pos[0], pos[1] + 0.6, pos[2]]
}

/// The target in sight: who it is, where it is (at the chest) and how many metres away.
pub type Target = (usize, [f32; 3], f32);

#[derive(Debug)]
pub struct Shooter {
    pub look: f64,
    pub target: Option<Target>,
    /// the last time it had someone in sight
    pub target_t: f64,
    pub ready_at: f64,
    pub next_shot: f64,
    pub burst: u32,
    pub bullets: i32,
    pub error: [f32; 2],
    pub still_in_combat: bool,
    pub decide_combat: f64,
    pub requested_weapon: Option<(usize, f64)>,
    pub grenade_until: f64,
    pub no_grenade: bool,
    pub uses_pistol: bool,
    pub line_busy: bool,
    pub health: f32,
    pub harassed_until: f64,
    /// bots3: does not squeeze the trigger if a wall or the terrain hides the target from the eye or from the weapon (the target is chosen
    /// five times a second and in between it can duck behind something; the weapon is higher than the crouched eye). bots2: no.
    pub check_line: bool,
    /// bots3: sees any part of an enemy's body (head, shoulder) and aims at it, not only the chest ([`BotView::body_seen`])
    pub see_body: bool,
    /// with `see_body`: the part of the target it saw, over its [`crate::bot_api::Other::pos`], which it follows between looks
    report: [f32; 3],
    /// the last time it did not fire for that reason
    pub line_blocked_t: f64,
    /// bots3: the anti-tank weapon looks first at ground vehicles closer than this, m (0: the nearest, as bots2)
    pub prefers_vehicle_m: f32,
    /// bots3: the weapon (its slot) it wants in hand for the target it has; None: whichever it carries (bots2)
    pub wanted_slot: Option<usize>,
    /// bots3: the median reaction time to a new enemy, s, and the aiming error, degrees (standard deviation)
    pub median_reaction: f32,
    pub error_degrees: f32,
    /// the probability of throwing a grenade when starting a burst (bots3: higher against a vehicle nearby)
    pub p_grenade: f32,
    /// bots3: from an aircraft's seat that is not the pilot's, goes for what flies, leads it, never fires a healing tool at the enemy and
    /// keeps missiles for what is big or slow ([`super::dogfight::Gunner`]). bots2: None.
    pub gunner: Option<super::dogfight::Gunner>,
}

impl Default for Shooter {
    fn default() -> Shooter {
        Shooter {
            look: 0.0,
            target: None,
            target_t: -100.0,
            ready_at: 0.0,
            next_shot: 0.0,
            burst: 0,
            bullets: 0,
            error: [0.0; 2],
            still_in_combat: false,
            decide_combat: 0.0,
            requested_weapon: None,
            grenade_until: 0.0,
            no_grenade: false,
            uses_pistol: false,
            line_busy: false,
            health: 1.0,
            harassed_until: 0.0,
            check_line: false,
            see_body: false,
            report: [0.0, CHEST, 0.0],
            line_blocked_t: -100.0,
            prefers_vehicle_m: 0.0,
            wanted_slot: None,
            median_reaction: MEDIAN_REACTION,
            error_degrees: AIM_ERROR,
            p_grenade: P_GRENADE,
            gunner: None,
        }
    }
}

impl Shooter {
    /// On the first tick of the life: when it first looks.
    pub fn spawn(&mut self, v: &BotView, rng: &mut Rng) {
        self.look = v.time + rng.uniform(0.0, 0.2) as f64;
    }

    /// The enemy it sees, five times a second: the nearest within range that is not hidden.
    pub fn see(&mut self, v: &BotView, rng: &mut Rng, eye: [f32; 3], reach: f32, others: &[Other]) {
        if v.time < self.look {
            // follows it by sight wherever it is now
            if let Some(b) = self.target.as_mut() {
                match others.iter().find(|o| o.player == b.0) {
                    Some(o) => {
                        b.1 = if self.see_body { std::array::from_fn(|k| o.pos[k] + self.report[k]) } else { [o.pos[0], o.pos[1] + CHEST, o.pos[2]] };
                        b.2 = dist3(eye, b.1);
                    }
                    None => self.target = None,
                }
            }
            return;
        }
        self.look = v.time + 0.2;
        let mut near: Vec<(f32, usize, [f32; 3])> = others
            .iter()
            .filter(|o| o.team != v.me.team)
            .map(|o| {
                let at = [o.pos[0], o.pos[1] + CHEST, o.pos[2]];
                (dist3(eye, at), o.player, at)
            })
            .filter(|x| x.0 < reach)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut seen = None;
        if self.prefers_vehicle_m > 0.0 {
            // the anti-tank weapon: the nearest enemy vehicle in range, before the nearest soldier
            let in_vehicle = |p: usize| others.iter().any(|o| o.player == p && o.in_vehicle);
            seen = near.iter().filter(|x| x.0 < self.prefers_vehicle_m && in_vehicle(x.1)).take(2).find(|x| !v.hidden(eye, x.2)).copied();
        }
        let see_body = self.see_body;
        let se_ve = |x: &(f32, usize, [f32; 3])| {
            if see_body {
                v.body_seen(eye, [x.2[0], x.2[1] - CHEST, x.2[2]]).map(|p| (x.0, x.1, p))
            } else {
                (!v.hidden(eye, x.2)).then_some(*x)
            }
        };
        let seen = seen.or_else(|| near.iter().take(3).find_map(se_ve));
        match seen {
            Some((d, p, at)) => {
                // after a while without seeing anyone, it takes time to react
                if self.target.is_none() && v.time - self.target_t > 3.0 {
                    self.ready_at = v.time + rng.lognormal(self.median_reaction, 0.7) as f64;
                    self.uses_pistol = rng.si(P_PISTOL);
                    self.new_error(rng);
                    self.decide_combat = v.time;
                }
                if self.see_body {
                    self.report = others.iter().find(|o| o.player == p).map_or([0.0, CHEST, 0.0], |o| std::array::from_fn(|k| at[k] - o.pos[k]));
                }
                self.target = Some((p, at, d));
                self.target_t = v.time;
            }
            None => self.target = None,
        }
    }

    pub fn new_error(&mut self, rng: &mut Rng) {
        self.error = [rng.normal() * self.error_degrees, rng.normal() * self.error_degrees];
    }

    /// Whoever is shot does not stand thinking (any player sees their health).
    pub fn watch_health(&mut self, v: &BotView) {
        let me = &v.me;
        if me.health < self.health - 0.001 {
            self.ready_at = self.ready_at.min(v.time);
            self.next_shot = self.next_shot.min(v.time);
            self.harassed_until = v.time + HARASSED;
        }
        self.health = me.health;
    }

    /// The axes for aiming at the target, with the view in `rows`; returns whether it has aimed. A turret does not always lower to the target: hence `pitch_margin`.
    pub fn aim(&mut self, axes: &mut Vec<(usize, f32)>, rows: [[f32; 3]; 3], eye: [f32; 3], at: [f32; 3], cap: f32, pitch_margin: f32) -> bool {
        let f = rows[2];
        let a = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
        let flat_map = (a[0] * a[0] + a[2] * a[2]).sqrt();
        let (g, c) = (angle_between(f, [a[0], a[2]]) + self.error[0], a[1].atan2(flat_map).to_degrees() + self.error[1]);
        let c_now = pitch(f);
        axes.push((axis::MOUSE_X, (TURN * g / 3.0).clamp(-cap, cap)));
        axes.push((axis::MOUSE_Y, (TURN * (c_now - c) / 2.5).clamp(-cap, cap)));
        g.abs() < 3.0 && (c - c_now).abs() < pitch_margin
    }

    /// Whether, already aimed, a mate on foot is on the line of fire.
    pub fn look_line(&mut self, v: &BotView, aimed: bool, at: [f32; 3], others: &[Other]) {
        let me = &v.me;
        self.line_busy = aimed && {
            let amigos: Vec<[f32; 3]> = others.iter().filter(|o| o.team == me.team).map(|o| o.pos).collect();
            friend_in_line(me.eye, at, &amigos)
        };
        // (and where it really looks, the first metres: aimed is ± 3°, and a corner beside it eats the burst)
        let ahead = |o: [f32; 3]| {
            let f = me.view[2];
            let l = (dist3(o, at) - 1.0).clamp(0.0, 12.0);
            [o[0] + f[0] * l, o[1] + f[1] * l, o[2] + f[2] * l]
        };
        if self.check_line && aimed && !self.line_busy && (v.hidden(me.eye, at) || v.hidden(weapon_of(me.pos), at) || v.hidden(weapon_of(me.pos), ahead(weapon_of(me.pos)))) {
            self.line_busy = true;
            self.line_blocked_t = v.time;
        }
    }

    /// The trigger for whoever is on foot or in a seat, by weapon class. `alternate` is the bot's shared pulse (one tick yes, the next no).
    pub fn trigger(&mut self, v: &BotView, rng: &mut Rng, alternate: &mut bool, axes: &mut Vec<(usize, f32)>, class: WeaponClass, d: f32, reach: f32) {
        let now = v.time;
        let scale = if now < self.harassed_until { HARASSED_SCALE * PAUSE_SCALE } else { PAUSE_SCALE };
        let bullets = v.me.rounds.0;
        if class == WeaponClass::Automatic && self.burst > 0 {
            if bullets < self.bullets {
                self.burst = self.burst.saturating_sub((self.bullets - bullets) as u32);
                if self.burst == 0 {
                    self.next_shot = now + weapons::pause(d, reach, scale, rng) as f64;
                    self.new_error(rng);
                }
            }
            self.bullets = bullets;
            if self.burst > 0 {
                axes.push((axis::FIRE, 1.0));
            }
            return;
        }
        self.bullets = bullets;
        if now < self.next_shot {
            return;
        }
        match class {
            WeaponClass::Automatic => {
                self.burst = weapons::burst(rng);
                axes.push((axis::FIRE, 1.0));
            }
            WeaponClass::Bolt | WeaponClass::Rocket | WeaponClass::Pistol => {
                *alternate = !*alternate;
                if *alternate {
                    axes.push((axis::FIRE, 1.0));
                } else {
                    let pause = weapons::pause(d, reach, scale, rng);
                    self.next_shot = now + weapons::interval(class, rng).max(0.5 * pause) as f64;
                    self.new_error(rng);
                }
            }
            WeaponClass::Granada | WeaponClass::Another => {}
        }
    }

    /// With the enemy in range and aimed: reloads or switches weapon if needed, and fires at its rhythm.
    pub fn shoot_on_foot(&mut self, v: &BotView, rng: &mut Rng, alternate: &mut bool, axes: &mut Vec<(usize, f32)>, class: WeaponClass, d: f32, aimed: bool, reach: f32) {
        self.weapons(v, rng, axes, class, d);
        // (bots3: while switching to the weapon the target asks for, it does not fire the one it carries)
        let changing = self.wanted_slot.is_some_and(|r| slot_in_hand(&v.me) != Some(r) && v.me.items.iter().any(|i| i.0 == r));
        if aimed && !self.line_busy && v.time >= self.ready_at && !changing {
            if class == WeaponClass::Granada {
                if v.time >= self.grenade_until - 0.6 && v.time < self.grenade_until {
                    axes.push((axis::FIRE, 1.0));
                }
            } else if !self.requesting_weapon(v) && v.me.rounds.0 != 0 {
                self.trigger(v, rng, alternate, axes, class, d, reach);
            }
        }
    }

    /// From a seat: half-second bursts with each weapon on its key. `hidden`: a mate or an own shell in between.
    pub fn shoot_from_seat(&mut self, v: &BotView, rng: &mut Rng, axes: &mut Vec<(usize, f32)>, aimed: bool, hidden: bool, reach: f32) {
        self.shoot_from_seat_keys(v, rng, axes, aimed, hidden, reach, (true, true));
    }

    /// As [`Shooter::shoot_from_seat`], with each of the primary and secondary keys allowed or not.
    #[allow(clippy::too_many_arguments)]
    pub fn shoot_from_seat_keys(&mut self, v: &BotView, rng: &mut Rng, axes: &mut Vec<(usize, f32)>, aimed: bool, hidden: bool, reach: f32, keys: (bool, bool)) {
        if aimed && v.time >= self.ready_at && !hidden {
            // half-second bursts
            if v.time < self.next_shot + 0.5 && v.time >= self.next_shot {
                // each weapon answers to its key, and does not choose by target
                if keys.0 {
                    axes.push((axis::FIRE, 1.0));
                }
                if keys.1 {
                    axes.push((axis::ALT_FIRE, 1.0));
                }
            } else if v.time >= self.next_shot + 0.5 {
                self.next_shot = v.time + weapons::pause(self.target.map_or(0.0, |b| b.2), reach, VEHICLE_SCALE * PAUSE_SCALE, rng) as f64;
                self.new_error(rng);
            }
        }
    }

    /// With no enemy in sight: lets go of the trigger, reloads, and goes back to the primary weapon or the one it had asked for.
    pub fn calm(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>, class: WeaponClass) {
        let me = &v.me;
        self.burst = 0;
        self.line_busy = false;
        if me.rounds.0 == 0 && me.rounds.1 > 0 {
            axes.push((axis::RELOAD, 1.0));
        }
        self.back_to_primary(v, axes, class);
        if let Some((r, until)) = self.requested_weapon {
            if v.time < until {
                axes.push((axis::WEAPON1 + r - 1, 1.0));
            } else {
                self.requested_weapon = None;
            }
        }
    }

    pub fn requesting_weapon(&self, v: &BotView) -> bool {
        self.requested_weapon.is_some_and(|(_, until)| v.time < until)
    }

    /// Reloads, switches to the pistol and throws a grenade now and then.
    pub fn weapons(&mut self, v: &BotView, rng: &mut Rng, axes: &mut Vec<(usize, f32)>, class: WeaponClass, d: f32) {
        let me = &v.me;
        if let Some((r, until)) = self.requested_weapon {
            if v.time < until {
                axes.push((axis::WEAPON1 + r - 1, 1.0));
                return;
            }
            self.requested_weapon = None;
        }
        // a grenade that was not one (the engineer carries explosives in that slot)
        if class == WeaponClass::Another {
            // (what the medkit and the wrench are is already known)
            self.no_grenade |= !me.weapon.as_deref().is_some_and(|w| w.eq_ignore_ascii_case("MedPack") || w.eq_ignore_ascii_case("RepairPack"));
            self.request(v, weapons::PRIMARY_SLOT);
            return;
        }
        if class == WeaponClass::Granada {
            if v.time >= self.grenade_until || me.rounds.0 + me.rounds.1 == 0 {
                self.request(v, weapons::PRIMARY_SLOT);
            }
            return;
        }
        // ammunition
        if me.rounds.0 == 0 {
            if me.rounds.1 > 0 {
                axes.push((axis::RELOAD, 1.0));
            } else if class != WeaponClass::Pistol {
                self.request(v, weapons::PISTOL_SLOT);
            }
            return;
        }
        // bots3: the weapon the target asks for (decided by whoever drives the body, with hysteresis)
        if let Some(r) = self.wanted_slot
            && slot_in_hand(me) != Some(r)
            && me.items.iter().any(|i| i.0 == r)
        {
            self.request(v, r);
            return;
        }
        if class != WeaponClass::Pistol && self.uses_pistol {
            self.uses_pistol = false;
            self.request(v, weapons::PISTOL_SLOT);
            return;
        }
        // a grenade, rare and at mid range, when starting a burst
        if !self.no_grenade && (10.0..28.0).contains(&d) && self.burst == 0 && v.time >= self.next_shot && v.time >= self.ready_at && rng.si(self.p_grenade) {
            self.request(v, weapons::GRENADE_SLOT);
            self.grenade_until = v.time + 1.5;
        }
    }

    /// Asks to switch to slot `slot` for an instant (sapper and trade use it too).
    pub fn request(&mut self, v: &BotView, slot: usize) {
        self.requested_weapon = Some((slot, v.time + 0.1));
        self.burst = 0;
    }

    /// With no combat, goes back to the primary weapon if ammunition is left.
    pub fn back_to_primary(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>, class: WeaponClass) {
        if matches!(class, WeaponClass::Pistol | WeaponClass::Granada | WeaponClass::Another) && (v.time as u64).is_multiple_of(4) {
            axes.push((axis::WEAPON1 + weapons::PRIMARY_SLOT - 1, 1.0));
        }
    }
}
