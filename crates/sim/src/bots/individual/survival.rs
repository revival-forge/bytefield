//! Survival: the action by utility ([`Individual::decide`]), what it looks at, how it carries the action out, cover, retreat and the reaction to a shot.
use super::*;

impl Individual {
    /// What it sees and what the team told it, in a [`Situation`]; chooses the action ([`choose`]).
    pub(super) fn decide(&mut self, v: &BotView, command: &Command, meta: Option<[f32; 3]>) -> Action {
        let me = &v.me;
        let t = v.time;
        let target = self.body.target();
        if let Some(b) = target {
            self.contact_t = t;
            self.enemy = Some(b.1);
        }
        // a shot is also a contact (and it reacts)
        if me.health < self.health - 0.001 {
            self.contact_t = t;
            // (a hit overrides turning to a shot heard)
            if target.is_none() && self.reaction.is_none_or(|r| t >= r.0 || r.3 == Reaction::Heard) {
                let seen = enemy_visible(v, REACTION_SIGHT_M);
                let heard = if seen.is_none() && self.settings.heard { enemy_heard(v, REACTION_SIGHT_M, 0.0) } else { None };
                let look_at = seen.or(heard);
                if let Some(e) = look_at {
                    self.enemy = Some(e);
                }
                let side_ = if (me.player + (t * 2.0) as usize) % 2 == 0 { 1.0 } else { -1.0 };
                self.reaction = Some((t + REACTION_S, look_at, side_, if heard.is_some() { Reaction::ShotHeard } else { Reaction::Hit }));
            }
            // the hidden spot was not there: another place
            if self.under_cover.is_some() {
                self.cover = None;
                self.under_cover = None;
            }
        }
        self.health = me.health;
        // B14: with no target and nothing to react to, it turns to an enemy shot heard near (a person looks where the shooting is)
        if self.settings.heard && target.is_none() && self.reaction.is_none_or(|r| t >= r.0 + HEARD_CALM_S) && !command.urgent && t >= self.heard_t
            && let Some(e) = { self.heard_t = t + 0.25; enemy_heard(v, HEARD_TURN_M, HEARD_RECENT_S) }
        {
            self.reaction = Some((t + HEARD_TURN_S, Some(e), 1.0, Reaction::Heard));
        }
        let others = v.others();
        self.look(v, &others, command);
        let s = Situation {
            health: me.health,
            ammo: me.rounds,
            enemies_seen: target.is_some() as usize,
            mates_near: others.iter().filter(|x| x.team == me.team && flat(me.pos, [x.pos[0], x.pos[2]]) < 30.0).count(),
            goal_distance: meta.map_or(0.0, |p| flat(me.pos, [p[0], p[2]])),
            order: command.order,
            urgent: command.urgent,
            on_foot: me.inside.is_none(),
            enemy_m: target.map(|b| b.2),
            since_enemy: (t - self.contact_t) as f32,
            no_cover: t < self.no_cover_until,
            wounded: self.wounded.map(|h| (h.2, h.3)),
            called: self.heal_called,
            medic_m: self.medic.map(|m| m.2),
            damaged_m: self.damaged.map(|d| d.2),
            low_health: self.profile.low_health(),
        };
        let a = choose(&s, self.action, self.action_since, t, &self.settings.params);
        self.last = Some(s);
        if a != self.action {
            if crate::bots::trace_actions() {
                eprintln!("action t={t:.1} p={} {:?} -> {:?} health {:.2} enemy {:?} wounded {:?} medic {:?} damaged {:?}", me.player, self.action, a, me.health, s.enemy_m.map(|x| x as i32), s.wounded.map(|h| (h.0 as i32, h.1)), s.medic_m.map(|x| x as i32), s.damaged_m.map(|x| x as i32));
            }
            self.action = a;
            self.action_since = t;
            self.cover = None;
            self.under_cover = None;
            self.retreat = None;
            self.beside_medic = None;
        }
        a
    }

    /// Who is wounded (if a medic), which medic it has nearby (if not) and which vehicle with its own inside is damaged (if
    /// an engineer). The list is rebuilt twice a second; where each one is, on every tick.
    pub(super) fn look(&mut self, v: &BotView, others: &[Other], command: &Command) {
        let me = &v.me;
        let t = v.time;
        let d = |p: [f32; 3]| flat(me.pos, [p[0], p[2]]);
        let mate = |j: usize| others.iter().find(|x| x.player == j && x.team == me.team && !x.in_vehicle);
        // where they are now
        self.wounded = self.wounded.and_then(|h| mate(h.0).and_then(|x| Some((h.0, x.pos, d(x.pos), x.health?)))).filter(|h| h.3 < HEALTH_WOUNDED_HIGH);
        self.medic = self.medic.and_then(|m| mate(m.0).map(|x| (m.0, x.pos, d(x.pos))));
        if t < self.look_t {
            return;
        }
        self.look_t = t + 0.5;
        let medic = self.kind == Some(Kind::Medic);
        // B15: who called "medic!" goes first, if still hurt (as a mate with little health, a call weighs no more)
        let call = command.call_medic.filter(|_| medic).and_then(mate).filter(|x| x.health.is_some_and(|h| h < HEALTH_WOUNDED_HIGH) && d(x.pos) < crate::bots::radio::DISPATCH_M);
        // (only a person's call: between bots, going with an enemy in sight cost reactions to fire (B14, 86 -> 76 %) and the
        // callers that live 20 s were already attended 93 % of the time)
        self.heal_called = call.is_some() && command.call_person;
        self.wounded = if let Some(x) = call {
            Some((x.player, x.pos, d(x.pos), x.health.unwrap_or(1.0)))
        } else if medic {
            others
                .iter()
                .filter(|x| x.team == me.team && !x.in_vehicle && x.health.is_some_and(|h| h < HEALTH_HEAL) && d(x.pos) < HEAL_RADIUS)
                .min_by(|a, b| d(a.pos).total_cmp(&d(b.pos)).then(a.player.cmp(&b.player)))
                .map(|x| (x.player, x.pos, d(x.pos), x.health.unwrap_or(1.0)))
        } else {
            None
        };
        let veto = self.medic_veto.filter(|m| t < m.1).map(|m| m.0);
        self.medic = if !medic && me.health < self.settings.params.health_medic {
            others
                .iter()
                .filter(|x| x.team == me.team && !x.in_vehicle && command.medics.has(x.player) && Some(x.player) != veto && d(x.pos) < MEDIC_RADIUS)
                .min_by(|a, b| d(a.pos).total_cmp(&d(b.pos)).then(a.player.cmp(&b.player)))
                .map(|x| (x.player, x.pos, d(x.pos)))
        } else {
            None
        };
        let own_one = self.repair_own.filter(|r| t - r.1 < REPAIR_OWN_S).map(|r| r.0);
        // B15: the vehicle whose crew called "need repairs" goes first, if still damaged
        let call = command.call_repairs.filter(|_| self.kind == Some(Kind::Engineer)).and_then(|k| v.vehicle(k)).filter(|x| x.team == me.team && x.health > 0.0 && x.health < HEALTH_REPAIRED && x.radius < 25.0 && d(x.pos) < crate::bots::radio::ANSWER_M);
        self.damaged = if let Some(x) = call {
            Some((x.vehicle, x.pos, d(x.pos), x.radius))
        } else if self.kind == Some(Kind::Engineer) {
            v.vehicles()
                .into_iter()
                .filter(|x| (x.team == me.team || (x.team == 0 && Some(x.vehicle) == own_one)) && x.health > 0.0 && x.radius < 25.0 && d(x.pos) < REPAIR_RADIUS)
                .filter(|x| if Some(x.vehicle) == own_one { x.health < 0.9 } else { !x.driver_free && x.health < HEALTH_REPAIR })
                .min_by(|a, b| d(a.pos).total_cmp(&d(b.pos)).then(a.vehicle.cmp(&b.vehicle)))
                .map(|x| (x.vehicle, x.pos, d(x.pos), x.radius))
        } else {
            None
        };
    }

    /// What the action does with the body; returns the point it goes to (centre and radius) if not the order's.
    pub(super) fn execute(&mut self, v: &BotView, a: Action) -> Option<([f32; 3], f32)> {
        let me = &v.me;
        let t = v.time;
        match a {
            Action::TakeCover => {
                let e = self.enemy?;
                if self.cover.is_none() && t >= self.cover_search {
                    self.cover_search = t + 1.0;
                    self.cover = self.body.cover_spot(v, e);
                    if self.cover.is_none() {
                        // nowhere: moves away (Retreat) for a while
                        self.no_cover_until = t + 5.0;
                    }
                }
                self.under_cover(v)
            }
            Action::Retreat => {
                // first under cover from the last enemy it saw, if it has it nearby; if not, far, towards its own
                let e = self.enemy.filter(|e| flat(me.pos, [e[0], e[2]]) < 80.0);
                if self.cover.is_none()
                    && self.retreat.is_none()
                    && let Some(e) = e
                {
                    self.cover = self.body.cover_spot(v, e);
                }
                if self.cover.is_some() {
                    return self.under_cover(v);
                }
                let still_valid = self.retreat.is_some_and(|r| e.is_none_or(|e| flat(r.1, [e[0], e[2]]) < 20.0));
                if !still_valid {
                    self.retreat = retreat_point(v, e, self.settings.params.retreat_m).map(|p| (p, e.unwrap_or(me.pos)));
                }
                let r = self.retreat?;
                self.body.break_contact = true;
                Some((r.0, 6.0))
            }
            Action::FindMedic => {
                let m = self.medic?;
                // beside it: waits for it to heal, but not forever
                if m.2 < 8.0 {
                    let since = *self.beside_medic.get_or_insert(t);
                    if t - since > 20.0 {
                        self.medic_veto = Some((m.0, t + 40.0));
                        self.medic = None;
                        self.beside_medic = None;
                    }
                } else {
                    self.beside_medic = None;
                }
                Some((m.1, 3.0))
            }
            Action::Heal => {
                let h = self.wounded?;
                self.body.trade.heal = Some(h.0);
                Some((h.1, 3.0))
            }
            Action::Repair => {
                let r = self.damaged?;
                self.body.trade.repair = Some(r.0);
                Some((r.1, r.3 + 2.0))
            }
            _ => None,
        }
    }

    /// Goes to its hidden spot and stays; if there they keep seeing it (the enemy in sight for more than a second) or it has been long, another spot
    /// or retreat.
    pub(super) fn under_cover(&mut self, v: &BotView) -> Option<([f32; 3], f32)> {
        let me = &v.me;
        let t = v.time;
        let c = self.cover?;
        self.body.break_contact = true;
        if flat(me.pos, [c[0], c[2]]) < 2.5 {
            let since = *self.under_cover.get_or_insert(t);
            let seen = self.body.target().is_some() && t - since > 1.0;
            if seen {
                // does not hide: another (one per second at most)
                self.cover = None;
                self.under_cover = None;
                self.cover_search = t + 1.0;
                if let Some(e) = self.body.target().map(|b| b.1) {
                    self.cover = self.body.cover_spot(v, e).filter(|p| flat(*p, [c[0], c[2]]) > 3.0);
                }
                if self.cover.is_none() {
                    self.no_cover_until = t + 5.0;
                }
                return self.cover.map(|p| (p, 1.5));
            }
            if t - since > self.profile.under_cover_s() as f64 {
                self.no_cover_until = t + 5.0;
            }
        }
        Some((c, 1.5))
    }

    /// `bots:tank-cover`: on foot with no target, about its business, an enemy in armour that nothing it carries hurts near and in
    /// sight ([`crate::bots::body::shooter::Shooter::armour_near`]): the spot hidden from him it goes to, and stays at, while he is near
    /// (another once he has moved; none found, it goes on). `None` when it does not hide.
    // TODO: parked (short run +0.2, level): it acts 9 times in 8 rounds of 32 players, as nearly every kit carries grenades and a
    // tank 8-28 m off is a grenade target instead. Next: count also the tank it can only hurt with grenades while it is not
    // throwing them, and count it first with BYTEFIELD_BOTS_ACTIONS (`tank_cover` lines).
    pub(super) fn tank_cover(&mut self, v: &BotView, about_its_business: bool) -> Option<([f32; 3], f32)> {
        let t = v.time;
        let sh = self.body.shooter();
        if !sh.fps.tank_cover || v.me.inside.is_some() || sh.target.is_some() || !about_its_business || t < self.tank_rest_until {
            self.tank_cover = None;
            return None;
        }
        let near = sh.armour_near;
        if let (Some(c), Some(n)) = (self.tank_cover.as_mut(), near)
            && c.tank == n.0
        {
            c.seen = t;
        }
        if let Some(c) = self.tank_cover {
            if t - c.seen > TANK_GONE_S {
                self.tank_cover = None;
                return None;
            }
            if t - c.since > TANK_HIDE_S {
                self.tank_cover = None;
                self.tank_rest_until = t + TANK_REST_S;
                return None;
            }
        }
        let n = near.or_else(|| self.tank_cover.map(|c| (c.tank, c.tank_at)))?;
        let moved = self.tank_cover.is_some_and(|c| c.tank != n.0 || flat(c.tank_at, [n.1[0], n.1[2]]) > TANK_MOVED_M);
        if (self.tank_cover.is_none() || moved) && t >= self.tank_search {
            self.tank_search = t + 1.0;
            let since = self.tank_cover.map_or(t, |c| c.since);
            self.tank_cover = find_cover(v, n.1).map(|spot| TankCover { spot, tank: n.0, tank_at: n.1, seen: t, since });
            if crate::bots::trace_actions() {
                eprintln!("tank_cover t={t:.1} p={} tank {} at {:.0} m: {:?}", v.me.player, n.0, flat(v.me.pos, [n.1[0], n.1[2]]), self.tank_cover.map(|c| c.spot.map(|x| x as i32)));
            }
        }
        let c = self.tank_cover?;
        self.body.break_contact = true;
        Some((c.spot, 1.5))
    }

    pub(super) fn react(&mut self, v: &BotView, axes: &mut Vec<(usize, f32)>) {
        let Some((until, look_at, side_, cause)) = self.reaction else { return };
        // (whoever is already breaking contact is already moving: it does not turn to the enemy)
        // TODO: most bots that do not react in B14 (Berlin) do react, but turn towards another shot than the nearest enemy one the
        // meter hears (95-175° off: `angle=` in the `BYTEFIELD_QUALITY_B14` trace), and move only 2-3 m in the 2 s. Tried without gain
        // (12 seeds, full B14 88.2 %): the newest shot heard first on a hit (86.9 %), running on while turning (87.0 %). Next: print
        // the point the bot picks next to the meter's. See `tournament3 --first-seed`.
        if v.time >= until || self.body.target().is_some() || v.me.inside.is_some() || self.body.break_contact {
            self.reaction = None;
            return;
        }
        let me = &v.me;
        match look_at {
            Some(e) => {
                let g = angle_between(me.view[2], [e[0] - me.pos[0], e[2] - me.pos[2]]);
                axes.retain(|x| x.0 != axis::MOUSE_X);
                axes.push((axis::MOUSE_X, (TURN * g / 3.0).clamp(-10.0, 10.0)));
                // (`bots:heard-walk`: a shot heard with nobody in sight: it walks on along its way while it looks)
                if cause == Reaction::Heard
                    && self.body.shooter().fps.heard_walk
                    && let Some(d) = self.body.walk_dir
                {
                    axes.retain(|x| !matches!(x.0, axis::THROTTLE | axis::YAW));
                    crate::bots::life::strafe_to(me.view[2], d, axes);
                }
                // hit from where it only heard a shot: it turns to it and steps aside, out of the line it was hit along
                if cause == Reaction::ShotHeard {
                    axes.retain(|x| !matches!(x.0, axis::YAW | axis::CROUCH));
                    axes.push((axis::YAW, side_));
                }
            }
            None => {
                axes.retain(|x| !matches!(x.0, axis::THROTTLE | axis::YAW | axis::CROUCH));
                axes.push((axis::THROTTLE, 1.0));
                axes.push((axis::YAW, side_));
            }
        }
    }
}

/// A wounded whom the medic goes to stops being one with this health (the medkit heals them all the way up anyway).
const HEALTH_WOUNDED_HIGH: f32 = 0.8;
/// B15: a vehicle whose crew called for repairs is repaired up to this.
const HEALTH_REPAIRED: f32 = 0.9;

/// The nearest enemy it sees (of the four nearest within `r` m, the first with no wall between its eyes and its chest).
pub fn enemy_visible(v: &BotView, r: f32) -> Option<[f32; 3]> {
    let me = &v.me;
    let mut near: Vec<(f32, [f32; 3])> = v.others().into_iter().filter(|o| o.team != me.team).map(|o| (flat(me.pos, [o.pos[0], o.pos[2]]), o.pos)).filter(|x| x.0 < r).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.into_iter().take(4).find(|x| !v.hidden(me.eye, [x.1[0], x.1[1] + CHEST, x.1[2]])).map(|x| x.1)
}

/// B14: where the nearest enemy shot heard within `r` m came from (fired at most `age` s ago; 0: any it still hears). Who fired is
/// not heard: a shot from where a mate of its own team stands (within 3 m) is taken for his.
pub fn enemy_heard(v: &BotView, r: f32, age: f32) -> Option<[f32; 3]> {
    let me = &v.me;
    let heard_ones = v.heard(r);
    if heard_ones.is_empty() {
        return None;
    }
    let others = v.others();
    let own_one = |p: [f32; 3]| others.iter().any(|o| o.team == me.team && (0..3).map(|k| (o.pos[k] - p[k]).powi(2)).sum::<f32>() < 9.0);
    heard_ones.into_iter().filter(|h| (age <= 0.0 || h.age <= age) && flat(me.pos, [h.pos[0], h.pos[2]]) > 3.0 && !own_one(h.pos)).min_by(|a, b| flat(me.pos, [a.pos[0], a.pos[2]]).total_cmp(&flat(me.pos, [b.pos[0], b.pos[2]]))).map(|h| h.pos)
}

/// A spot a few metres away that a wall or the terrain hides from the enemy at `e` (its body): about ten points at three
/// distances, starting with those that leave its back to the enemy; not in the water nor behind a cliff. The first that is good.
pub fn find_cover(v: &BotView, e: [f32; 3]) -> Option<[f32; 3]> {
    let me = &v.me;
    let eye = [e[0], e[1] + 0.6, e[2]];
    let flees = (me.pos[2] - e[2]).atan2(me.pos[0] - e[0]);
    let water = v.water();
    for k in 0..COVER_POINTS {
        // 0, +36°, -36°, +72°... around the direction of flight
        let step = k.div_ceil(2) as f32 * std::f32::consts::TAU / COVER_POINTS as f32;
        let a = flees + if k % 2 == 1 { step } else { -step };
        let r = COVER_M[k % COVER_M.len()];
        let (x, z) = (me.pos[0] + r * a.cos(), me.pos[2] + r * a.sin());
        let Some(h) = v.ground(x, z) else { continue };
        if h < water + 0.3 || !no_fall(v, me.pos, [x, z]) {
            continue;
        }
        let p = [x, h + 1.0, z];
        if v.hidden(eye, [x, p[1] + CHEST, z]) {
            return Some(p);
        }
    }
    None
}

/// Where it retreats to: the nearest own flag that is farther from the enemy than it is (within 400 m) or, if none,
/// to `retreat_m` in the direction opposite the enemy (or somewhat sideways, if there is water that way).
pub fn retreat_point(v: &BotView, e: Option<[f32; 3]>, retreat_m: f32) -> Option<[f32; 3]> {
    let me = &v.me;
    let de_e = |p: [f32; 3]| e.map_or(0.0, |e| flat(p, [e[0], e[2]]));
    let yo = de_e(me.pos);
    let far = |p: [f32; 3]| flat(me.pos, [p[0], p[2]]);
    let flag = v.flags().into_iter().filter(|f| f.team == me.team && far(f.pos) < 400.0 && far(f.pos) > 15.0 && (e.is_none() || de_e(f.pos) > yo + 20.0)).min_by(|a, b| far(a.pos).total_cmp(&far(b.pos)));
    if let Some(f) = flag {
        return Some(f.pos);
    }
    let e = e?;
    let flees = (me.pos[2] - e[2]).atan2(me.pos[0] - e[0]);
    let water = v.water();
    for k in [0.0f32, 0.5, -0.5, 1.0, -1.0] {
        let a = flees + k * std::f32::consts::FRAC_PI_4 * 1.5;
        let (x, z) = (me.pos[0] + retreat_m * a.cos(), me.pos[2] + retreat_m * a.sin());
        if let Some(h) = v.ground(x, z).filter(|h| *h > water + 0.3) {
            return Some([x, h, z]);
        }
    }
    None
}
