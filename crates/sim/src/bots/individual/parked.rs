//! The variants tried and parked (`bots:X`, all off by default and identical when off): each keeps its why in a TODO beside it,
//! so a later try starts from what was measured.
use super::*;

/// `bots:streetwise`: whether the streetwise bot prefers the busy enemy ([`crate::bots::body::shooter::Opportunist`]).
// TODO: off: it did not move B27 (the share of busy victims of the side) with one bot in four, and lost tickets on DC_Al_Nas once;
// the facing of a seen enemy (its back) was left for a second try.
pub const OPPORTUNIST_ON: bool = false;
/// `bots:retreat-mate`: from this trait up.
pub const RETREAT_MATE_TRAIT: f32 = 0.5;
// TODO: the lines about the streetwise acts (with `bots:dialogue`) did not raise B33 past 23 % (50 % asked) and the medic calls
// fell 25-35 % (20 % allowed), even with "medic!" not waiting after them: they take the life's places a hurt bot would call again
// with. B33 counts every busy kill of the side, by anyone, and with three messages a life it cannot reach half.
/// `bots:dialogue`: it says "fire in the hole" this near the place of its next mine, m.
pub const MINE_SAY_M: f32 = 4.0;
/// `BYTEFIELD_COVER_TRACE=1`: what `bots:cover-flank` does, once a second per bot (stderr).
pub fn cover_trace() -> bool {
    crate::debug::options().bots_cover
}
// TODO: `bots:cover-flank` off: with P1 and P2 on both sides it lost tickets (-0.3 ± 6.4 waiting 15 s for a mate at a firing spot,
// -9.7 ± 6.8 waiting 6 s for one fighting) and moved neither B09 nor B05. The covering fire itself is rare (25-62 bot-seconds in
// two rounds of Berlin): firing spots are few and the enemy seen near the flag goes stale. A flanker that waits in cover, not where
// it stands, and covering fire from wherever a mate sees the flag, are what to try. See `BYTEFIELD_COVER_TRACE=1`.
/// `bots:cover-flank`: the flanker waits at most this for a mate at a firing spot, s.
// (15 s left the flankers standing in the open: 789 bot-seconds of waiting in two rounds of Berlin, covered 17 % of the time)
pub const COVER_WAIT_S: f64 = 6.0;
// TODO: `bots:opportunist-all` off: weighed by everyone (0.3 + trait x 0.5) B27 rose +1 point, and +2.9 with "looking away" as
// vulnerable too (+5 asked), and the side stood still more on four maps (Kursk +2.3..+3.7 points). The busy share of kills is
// mostly chance (two on the same enemy): the choice of target barely moves it. B27 per bot, not per side, would tell more.
/// `bots:opportunist-all`: the weight every bot gives to a vulnerable enemy, and how much its trait adds to it.
pub const OPPORTUNIST_ALL_W: f32 = 0.3;
pub const OPPORTUNIST_ALL_TRAIT: f32 = 0.5;
/// `bots:streetwise`: from this trait up it takes the enemy's free vehicles ([`crate::bots::body::driver::STEAL_M`]).
pub const STEAL_TRAIT: f32 = 0.5;
// TODO: off: an empty vehicle has no side, so "the enemy's" is one by an enemy flag, and the streetwise few seldom found one free with
// nobody near; when they did (DC_Al_Nas) the side lost tickets (-94 ± 43). It wants a look with the team trace before more.
pub const STEAL_ON: bool = false;

/// `bots:escort2`: the escort's point lies this far behind the tank, m; it walks there when farther than [`ESCORT_SLACK_M`] from it,
/// m; within [`ESCORT_FLAG_M`] of the flag, everybody goes to the flag, m.
// (16 m and 6 m left B02 as it was, -0.5 and +5 points on El_Alamein and Kursk)
pub const ESCORT_BEHIND_M: f32 = 10.0;
pub const ESCORT_SLACK_M: f32 = 4.0;
pub const ESCORT_FLAG_M: f32 = 60.0;

/// `bots:flank2`: the flank spot lies at these distances from the flag, m, at least this far round from the combat vector,
/// degrees; one by a sector the side has not arrived by lately counts this much nearer, m.
pub const FLANK2_M: [f32; 2] = [40.0, 55.0];
pub const FLANK2_DEGREES: f32 = 60.0;
pub const FLANK2_FRESH_M: f32 = 40.0;

/// `bots:streetwise`: an ambush on a habit: the cell where the enemy passes, the point it walks to first ([`AMBUSH_STAGE_M`] short of
/// the cell, on its side), the spot it waits at (a firing spot on the cell, hidden crouched from it) and until when.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ambush {
    cell: [f32; 3],
    stage: [f32; 3],
    spot: Option<[f32; 3]>,
    until: f64,
    wait: f64,
}

/// `bots:streetwise`: a bot with the trait from [`AMBUSH_TRAIT`] up, free (no urgent order, on foot, health over [`AMBUSH_HEALTH`],
/// nobody in sight), lies in wait for a habit within [`AMBUSH_FAR_M`], m: it walks to [`AMBUSH_STAGE_M`] short of the cell, m, asks a
/// firing spot on it, and waits [`AMBUSH_WAIT_S`] (drawn without streaks), s; one ambush every [`AMBUSH_AGAIN_S`], s, and it gives
/// up getting there after [`AMBUSH_GO_S`], s.
// TODO: off: against a person walking the same stretch life after life (`examples/habit_bench`), its deaths in the second half against
// the first stayed as without it (1.04 against 0.94; wanted 1.5); the habit cell often lies behind the enemy's lines, and the one
// lying in wait died there every 25 s. It wants the ambush only on cells nearer its own side than the enemy's.
pub const AMBUSH_ON: bool = false;
pub const AMBUSH_TRAIT: f32 = 0.7;
pub const AMBUSH_HEALTH: f32 = 0.6;
pub const AMBUSH_FAR_M: f32 = 250.0;
pub const AMBUSH_STAGE_M: f32 = 25.0;
pub const AMBUSH_WAIT_S: [f64; 2] = [30.0, 90.0];
pub const AMBUSH_AGAIN_S: f64 = 180.0;
pub const AMBUSH_GO_S: f64 = 90.0;

impl Individual {
    pub fn cover_flank_on(&self) -> bool {
        self.settings.cover_flank
    }
    /// `bots:cover-flank`: whether it covers (at its firing spot, or fighting an enemy it sees), and the flank it goes round (flag,
    /// point).
    pub fn cover_state(&self, v: &BotView) -> (bool, Option<(usize, [f32; 3])>) {
        (self.body.at_firing_spot(v) || self.body.target().is_some(), self.flank.filter(|x| !x.2).map(|x| (x.0, x.1)))
    }
    pub fn ambush_own_on(&self) -> bool {
        self.settings.ambush_own
    }
    pub fn steal_flank_on(&self) -> bool {
        self.settings.steal_flank
    }
    pub fn opportunist_all_on(&self) -> bool {
        self.settings.opportunist_all
    }
    pub fn streetwise_on(&self) -> bool {
        self.settings.streetwise
    }
    pub fn dialogue_on(&self) -> bool {
        self.settings.streetwise && self.settings.dialogue
    }
    pub fn streetwise(&self) -> Option<f32> {
        self.sw.as_ref().map(|s| s.0)
    }
    /// Whether it has set off to flank since last asked.
    pub fn flank_started(&mut self) -> bool {
        std::mem::take(&mut self.flank_new)
    }
    pub fn set_streetwise(&mut self, x: f32, player: usize) {
        self.sw = Some((x, crate::bots::body::rng::Rng::new(0x5357 ^ player as u64), crate::bots::streetwise::Filtered::default()));
    }

    /// The parked variants' part of a tick (all off by default): the opportunist, mines on traffic, covering the flanker, retreating
    /// to a mate, stealing; and the ambush point it lies in wait at, if any.
    pub(super) fn parked_tick(&mut self, v: &BotView, command: &Command) -> Option<([f32; 3], f32)> {
        if self.settings.opportunist_all {
            let w = OPPORTUNIST_ALL_W + OPPORTUNIST_ALL_TRAIT * self.streetwise().unwrap_or(0.0);
            self.body.opportunist(w / crate::bots::body::shooter::OPPORTUNIST_W, &command.busy, true);
        } else if let Some(x) = self.streetwise().filter(|_| OPPORTUNIST_ON || self.settings.opportunist) {
            self.body.opportunist(x, &command.busy, false);
        }
        if self.sw.is_some() {
            self.body.mines_at(command.mines_at, self.settings.traffic_spread);
        }
        if self.settings.cover_flank {
            self.body.set_cover(command.cover);
        }
        self.body.retreat_mate = self.settings.retreat_mate && self.streetwise().is_some_and(|x| x >= RETREAT_MATE_TRAIT);
        let ambush_point = self.ambush(v, command);
        let flanking = self.flank.is_some_and(|x| !x.2);
        // (`bots:steal-flank`: any flanker, not only the high trait)
        // TODO: off: B29 moved ±0.2 a round (+0.5 asked): near the flag it flanks there is seldom an empty vehicle within 60 m that the
        // side saw an enemy drive. Stealing on the way, not only round the flank, is the next thing to try.
        let trait_ok = self.settings.steal_flank || self.streetwise().is_some_and(|x| x >= STEAL_TRAIT);
        self.body.driver_mut().steals = (STEAL_ON || self.settings.steal) && trait_ok && !command.urgent && (!self.settings.steal_flank || flanking);
        self.body.driver_mut().only_steal = false;
        self.body.driver_mut().enemy_seen = self.settings.steal_flank.then(|| command.enemy_vehicles.iter().copied().filter(|&k| k != usize::MAX).collect());
        ambush_point
    }

    /// `bots:streetwise`: whether it asks its side for the trait, and the trait it has (once given).
    /// `bots:dialogue`: the line about a streetwise act it does now, if any: about to lay a mine, on its way to steal, at the wheel of
    /// what it stole with a seat and a mate near, or firing at an enemy a mate fires at (once per enemy).
    pub fn act_line(&mut self, v: &BotView, busy: &[usize; 8]) -> Option<crate::bots::radio::St> {
        use crate::bots::radio::St;
        let me = &v.me;
        let d = self.body.driver();
        if d.stealing && me.inside.is_none() {
            self.stolen = d.vehicle;
        }
        // (not hurt: the place it has left on the radio is for "medic!")
        if me.health < crate::bots::radio::HURT {
            return None;
        }
        if me.inside.is_none() && self.body.sapper().laying_mine(me.pos, MINE_SAY_M) && me.weapon.as_deref().is_some_and(|w| w.to_ascii_lowercase().contains("mine")) {
            return Some(St::Mine);
        }
        if me.inside.is_none() && d.stealing && d.vehicle.is_some() {
            return Some(St::Steal);
        }
        if let Some(i) = me.inside.as_ref().filter(|i| i.seat == 0 && Some(i.vehicle) == self.stolen)
            && v.vehicle(i.vehicle).is_some_and(|c| !c.free_seats.is_empty())
            && v.others().iter().any(|o| o.team == me.team && !o.in_vehicle && flat(me.pos, [o.pos[0], o.pos[2]]) <= crate::bot_api::VOICE_M)
        {
            return Some(St::Stolen);
        }
        // (once it fires, not when it first sees it: the kill it announces comes seconds later)
        let target = self.body.target().map(|t| t.0).filter(|_| v.time >= self.body.shooter().ready_at);
        if let Some(j) = target.filter(|j| busy.contains(j) && self.finish_said != Some(*j)) {
            self.finish_said = Some(j);
            return Some(St::Finish);
        }
        None
    }

    /// `bots:streetwise`: the point of the ambush it lies in, with its radius, or none ([`Ambush`]).
    pub(super) fn ambush(&mut self, v: &BotView, command: &Command) -> Option<([f32; 3], f32)> {
        self.streetwise().filter(|&x| (AMBUSH_ON || self.settings.ambush) && x >= AMBUSH_TRAIT)?;
        let me = &v.me;
        let t = v.time;
        let free = !command.urgent && me.inside.is_none() && me.health > AMBUSH_HEALTH;
        if let Some(mut a) = self.ambush {
            if !free || t > a.until {
                self.ambush = None;
                self.ambush_next = t + AMBUSH_AGAIN_S;
                return None;
            }
            if a.spot.is_none() && flat(me.pos, [a.stage[0], a.stage[2]]) < 8.0 {
                let ctx = crate::bots::tactics::Ctx { enemy: [a.cell[0], a.cell[1] + 1.0, a.cell[2]], ..Default::default() };
                let spot = crate::bots::tactics::ask(v, self.body.maps().foot(v), &[crate::bots::tactics::FIRING_SPOT, crate::bots::tactics::CORNER_SPOT], &ctx);
                a.spot = Some(spot.map_or(a.stage, |s| s.pos));
                a.until = t + a.wait;
            }
            self.ambush = Some(a);
            if a.spot.is_some_and(|p| flat(me.pos, [p[0], p[2]]) < 4.0) {
                self.waits = Some(crate::bot_api::Wait::Ambush);
            }
            return Some(match a.spot {
                Some(p) => (p, 1.0),
                None => (a.stage, 4.0),
            });
        }
        if !free || t < self.ambush_next || self.body.target().is_some() {
            return None;
        }
        let cell = command.habit?;
        let d = flat(me.pos, [cell[0], cell[2]]);
        if d > AMBUSH_FAR_M || d < 1.0 {
            return None;
        }
        let k = (d - AMBUSH_STAGE_M).max(0.0) / d;
        let (x, z) = (cell[0] + (me.pos[0] - cell[0]) * (1.0 - k), cell[2] + (me.pos[2] - cell[2]) * (1.0 - k));
        let stage = [x, v.ground(x, z).unwrap_or(me.pos[1]), z];
        let cell = [cell[0], v.ground(cell[0], cell[2]).unwrap_or(me.pos[1]), cell[2]];
        let (_, rng, f) = self.sw.as_mut()?;
        let wait = AMBUSH_WAIT_S[0] + (AMBUSH_WAIT_S[1] - AMBUSH_WAIT_S[0]) * f.int(rng, 7) as f64 / 6.0;
        self.ambush = Some(Ambush { cell, stage, spot: None, until: t + AMBUSH_GO_S, wait });
        if crate::debug::options().bots_ambush {
            eprintln!("ambush: t={t:.0} player {} for the habit at {:.0} {:.0}, {d:.0} m away, waits {wait:.0} s", me.player, cell[0], cell[2]);
        }
        Some((stage, 4.0))
    }
}

/// `bots:flank2`: the flanker's spot, round `flag` at [`FLANK2_M`], at least [`FLANK2_DEGREES`] from the combat vector (from the
/// enemy to its squad, here its leader), on ground it can walk to (the zone of its feet), hidden from where the enemy is, by a sector
/// the side has not arrived by lately if there is one, the nearest to it; at most [`crate::bots::tactics::MAX_RAYS`] / 2 rays.
/// `None`: nowhere (it does not flank).
pub fn flank2_point(v: &BotView, nav: Option<&dyn crate::bots::body::nav::NavBackend>, flag: [f32; 3], leader: [f32; 3], fk: &crate::bots::team::Flank) -> Option<[f32; 3]> {
    let me = &v.me;
    let combat = (leader[2] - fk.enemy[2]).atan2(leader[0] - fk.enemy[0]);
    let zone = nav.and_then(|n| n.zone_at_feet(crate::bots::body::feet(me.pos)));
    let water = v.water();
    let mut cands: Vec<(f32, [f32; 3])> = Vec::new();
    for k in 0..24 {
        let a = k as f32 * std::f32::consts::TAU / 24.0;
        let off = (a - combat + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        if off.abs().to_degrees() < FLANK2_DEGREES {
            continue;
        }
        for r in FLANK2_M {
            let (x, z) = (flag[0] + r * a.cos(), flag[2] + r * a.sin());
            let Some(h) = v.ground(x, z).filter(|h| *h > water + 0.3) else { continue };
            if let (Some(n), Some(zn)) = (nav, zone)
                && n.zone_at(x, z).is_some_and(|q| q != zn)
            {
                continue;
            }
            let p = [x, h + 1.0, z];
            let fresh = !fk.used[crate::bots::team::sector(flag, p)];
            cands.push((flat(me.pos, [x, z]) - if fresh { FLANK2_FRESH_M } else { 0.0 }, p));
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1[0].total_cmp(&b.1[0])).then(a.1[2].total_cmp(&b.1[2])));
    let e = fk.enemy;
    cands.into_iter().take((crate::bots::tactics::MAX_RAYS / 2) as usize).map(|c| c.1).find(|p| v.hidden([e[0], e[1] + 0.6, e[2]], [p[0], p[1] + 0.3, p[2]]))
}
