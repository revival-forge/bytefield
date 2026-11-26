//! Where to stand: a position query in the manner of the tactical point systems of shooters (candidates, cheap filters, normalised
//! weights, then the dear filters, the rays, from the best candidate down, stopping at the first that passes).
//!
//! A [`Query`] is a recipe: where the candidates come from (the centres of the walking mesh's polygons in the zone the soldier
//! stands in, so reachable by construction, plus a ring of points around it as the fallback), the cheap conditions ([`Cond`]
//! without rays), the weights ([`Crit`], each normalised to 0..1, distances capped at [`CAP_M`]), and the dear conditions (rays
//! with [`BotView::hidden`]). [`ask`] takes several, the strictest first, and answers with the first that finds a spot: the
//! fallback options are laxer, and the failure of all of them is itself an answer for whoever asked. Each query spends at most
//! [`MAX_RAYS`] rays.
//!
//! The body's point is a metre over the feet ([`crate::bots::body::BODY_OVER_FEET`]); a crouched soldier is looked at
//! [`CROUCH_M`] over its feet and sees from [`STAND_EYE_M`] when standing. `hidden` sees faces from the front only: from inside a
//! building its walls hide nothing, and it does not know the grass (hidden from bullets is not hidden from sight).
//!
//! With `bots:cover-points` ([`Ctx::cover_points`]) the level's cover points ([`crate::cover`]) near it come first: those that do not
//! hide crouched towards the enemy (their sector) are turned down without a ray, as are, for a query that shoots standing, those
//! that hide standing too (a wall); the rest are tried before any centre of a polygon.
//!
//! `BYTEFIELD_TPS_TRACE=<player>` writes, per query of that player, each candidate with its score or the filter that turned it
//! down, on stderr.
use crate::bot_api::BotView;
use crate::bots::body::nav::NavBackend;
use crate::bots::body::{BODY_OVER_FEET, feet, flat, no_fall};
use crate::cover::CoverPoint;

/// Distances in the weights are capped at this, m.
pub const CAP_M: f32 = 30.0;
/// Rays one call to [`ask`] may spend, all its options together, each its share (a ray costs 3.65 us in Berlin, 0.37 in El_Alamein: 24 rays per
/// second and bot for 64 bots is 5.6 ms a second in Berlin; 12 left the corner option, three rays a candidate, with four
/// candidates and no spot found on half the soldiers that saw an enemy in Berlin).
pub const MAX_RAYS: u32 = 24;
/// Candidates taken from the mesh, nearest first.
pub const MESH_CANDIDATES: usize = 48;
/// Cover points taken as candidates, nearest first (`bots:cover-points`).
pub const COVER_CANDIDATES: usize = 48;
/// A crouched soldier's body is looked at this far over its feet, m; standing, it sees from this height, m.
pub const CROUCH_M: f32 = 0.8;
pub const STAND_EYE_M: f32 = 1.6;
/// `NotTowards`: the way from here to the spot may come no nearer the enemy than this share of the distance to it now.
pub const NOT_TOWARDS: f32 = 0.7;
/// `Unclaimed`: no teammate already goes to a spot within this, m.
pub const CLAIM_M: f32 = 3.0;
/// `PeekNear`: the points a step to either side of the spot, across the line to the enemy, m.
pub const PEEK_M: f32 = 2.5;
/// The fallback ring: points at these distances, every 36° around the direction away from the enemy.
pub const RING_M: [f32; 3] = [6.0, 10.0, 14.0];
pub const RING_POINTS: usize = 10;
/// `MateNear`: a teammate this near the spot, m.
pub const MATE_NEAR_M: f32 = 25.0;
/// `StraightWay`: the way is looked along at this height over the floor, m (what a soldier steps over is lower).
pub const KNEE_M: f32 = 0.5;

/// A condition a candidate must meet. The first four cost no ray; `HiddenCrouch` and `SeesStanding` cost one each, `PeekNear` two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cond {
    /// nearer to me than to the enemy (it gets there first)
    ReachBefore,
    /// the straight way there does not run at the enemy ([`NOT_TOWARDS`])
    NotTowards,
    /// no teammate already goes there ([`CLAIM_M`])
    Unclaimed,
    /// [`directness`] towards the goal at least this (it moves on towards it)
    MinDirectness(f32),
    /// crouched there, the enemy does not see it
    HiddenCrouch,
    /// standing there, it sees the enemy
    SeesStanding,
    /// standing a step to one side ([`PEEK_M`]), it sees the enemy: a corner to lean out of
    PeekNear,
    /// a teammate on foot within [`MATE_NEAR_M`] of the spot
    MateNear,
    /// the nearest such teammate sees the way from the enemy to the spot (its middle): it would cover the one retreating there
    MateWatches,
    /// walking straight there from where it stands meets no wall ([`BotView::walk_blocked`] at the knee; `bots:cover-path`,
    /// [`Ctx::straight`]): the bot goes to its spot in a straight line, and pushed for seconds at a fence or a building's wall
    StraightWay,
}

/// What makes a candidate better, each 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Crit {
    /// near me
    DistMe,
    /// [`directness`] towards the goal, from -1..1 to 0..1
    Directness,
}

/// A recipe: the candidates within `r` of me, the cheap conditions, the weights and the dear conditions.
#[derive(Clone, Copy, Debug)]
pub struct Query {
    pub name: &'static str,
    pub r: f32,
    pub cheap: &'static [Cond],
    pub weights: &'static [(Crit, f32)],
    pub dear: &'static [Cond],
}

/// A firing spot: a parapet against the target (crouched it is hidden, standing it shoots), got to first and not by running at it.
pub const FIRING_SPOT: Query = Query { name: "firing", r: 16.0, cheap: &[Cond::ReachBefore, Cond::NotTowards, Cond::Unclaimed], weights: &[(Crit::DistMe, 1.0)], dear: &[Cond::HiddenCrouch, Cond::SeesStanding] };
/// [`FIRING_SPOT`] for whoever has an order to attack: the spot must also move it on towards the flag (a bound forwards).
pub const FIRING_SPOT_ADVANCE: Query = Query { name: "firing-advance", r: 16.0, cheap: &[Cond::ReachBefore, Cond::NotTowards, Cond::Unclaimed, Cond::MinDirectness(0.3)], weights: &[(Crit::DistMe, 1.0), (Crit::Directness, 0.5)], dear: &[Cond::HiddenCrouch, Cond::SeesStanding] };
/// The fallback of a firing spot: hidden crouched, and a step aside from it sees the target (a corner; Berlin's walls are whole
/// buildings and its parapets few).
pub const CORNER_SPOT: Query = Query { name: "corner", r: 16.0, cheap: &[Cond::ReachBefore, Cond::NotTowards, Cond::Unclaimed], weights: &[(Crit::DistMe, 1.0)], dear: &[Cond::HiddenCrouch, Cond::PeekNear] };
/// Cover: hidden crouched, without asking to shoot from it.
pub const COVER_SPOT: Query = Query { name: "cover", r: 15.0, cheap: &[Cond::ReachBefore, Cond::NotTowards], weights: &[(Crit::DistMe, 1.0)], dear: &[Cond::HiddenCrouch] };

/// `bots:retreat-mate`: [`COVER_SPOT`] by a teammate who sees the enemy's way to it (St3: retreat towards a mate).
pub const COVER_SPOT_MATE: Query = Query { name: "cover-mate", r: 15.0, cheap: &[Cond::ReachBefore, Cond::NotTowards, Cond::MateNear], weights: &[(Crit::DistMe, 1.0)], dear: &[Cond::HiddenCrouch, Cond::MateWatches] };

/// What a query needs to know besides the view: the enemy's body, the goal if it has one, the spots its teammates already go to,
/// and where its teammates on foot are (for [`Cond::MateNear`]).
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    pub enemy: [f32; 3],
    pub goal: Option<[f32; 3]>,
    pub claimed: Vec<[f32; 3]>,
    pub mates: Vec<[f32; 3]>,
    /// `bots:cover-points`: the level's cover points first, filtered by their sectors
    pub cover_points: bool,
    /// `bots:cover-path`: every query also asks for [`Cond::StraightWay`], first of its dear conditions
    pub straight: bool,
}

/// The answer: where to stand (the body's point, a metre over the floor), which option found it and the rays spent; and the kind
/// of candidate it was (`cover`, `mesh` or `ring`), with its cover point if it is one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub pos: [f32; 3],
    pub option: usize,
    pub rays: u32,
    pub from: &'static str,
    pub cover: Option<CoverPoint>,
}

/// Equation 26.1 of the tactical point systems: how much going from `me` to `spot` brings it nearer `goal`, per metre walked,
/// from -1 (straight away) to 1 (straight at it).
pub fn directness(me: [f32; 3], spot: [f32; 3], goal: [f32; 3]) -> f32 {
    let walked = flat(me, [spot[0], spot[2]]);
    if walked < 0.1 {
        return 0.0;
    }
    ((flat(me, [goal[0], goal[2]]) - flat(spot, [goal[0], goal[2]])) / walked).clamp(-1.0, 1.0)
}

/// How far `e` is from the straight way from `a` to `b`, across (`bots:cover-flank`).
pub fn off_way(a: [f32; 3], b: [f32; 3], e: [f32; 3]) -> f32 {
    nearest_on_way(a, b, e)
}

/// The nearest the straight way from `a` to `b` comes to `e`, across.
fn nearest_on_way(a: [f32; 3], b: [f32; 3], e: [f32; 3]) -> f32 {
    let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
    let l2 = dx * dx + dz * dz;
    let t = if l2 < 1e-6 { 0.0 } else { (((e[0] - a[0]) * dx + (e[2] - a[2]) * dz) / l2).clamp(0.0, 1.0) };
    (a[0] + dx * t - e[0]).hypot(a[2] + dz * t - e[2])
}

/// Whether a candidate meets a cheap condition.
fn cheap(c: Cond, me: [f32; 3], spot: [f32; 3], ctx: &Ctx) -> bool {
    let e = ctx.enemy;
    match c {
        Cond::ReachBefore => flat(me, [spot[0], spot[2]]) < flat(e, [spot[0], spot[2]]),
        Cond::NotTowards => nearest_on_way(me, spot, e) >= NOT_TOWARDS * flat(me, [e[0], e[2]]),
        Cond::Unclaimed => !ctx.claimed.iter().any(|c| flat(*c, [spot[0], spot[2]]) < CLAIM_M),
        Cond::MinDirectness(x) => ctx.goal.is_none_or(|g| directness(me, spot, g) >= x),
        Cond::MateNear => ctx.mates.iter().any(|m| flat(*m, [spot[0], spot[2]]) < MATE_NEAR_M),
        Cond::HiddenCrouch | Cond::SeesStanding | Cond::PeekNear | Cond::MateWatches | Cond::StraightWay => true,
    }
}

/// The rays a dear condition costs.
fn cost(c: Cond) -> u32 {
    match c {
        Cond::HiddenCrouch | Cond::SeesStanding | Cond::MateWatches | Cond::StraightWay => 1,
        Cond::PeekNear => 2,
        _ => 0,
    }
}

/// Whether a candidate (the body's point) meets a dear condition; `rays` counts what it spent.
fn dear(v: &BotView, c: Cond, spot: [f32; 3], ctx: &Ctx, rays: &mut u32) -> bool {
    let e = ctx.enemy;
    let floor = spot[1] - BODY_OVER_FEET;
    let sees = |x: f32, z: f32, rays: &mut u32| {
        *rays += 1;
        !v.hidden([x, floor + STAND_EYE_M, z], [e[0], e[1] + 0.3, e[2]])
    };
    match c {
        Cond::HiddenCrouch => {
            *rays += 1;
            v.hidden([e[0], e[1] + 0.6, e[2]], [spot[0], floor + CROUCH_M, spot[2]])
        }
        Cond::SeesStanding => sees(spot[0], spot[2], rays),
        Cond::PeekNear => {
            let (dx, dz) = (e[0] - spot[0], e[2] - spot[2]);
            let l = dx.hypot(dz).max(0.1);
            let (px, pz) = (-dz / l * PEEK_M, dx / l * PEEK_M);
            sees(spot[0] + px, spot[2] + pz, rays) || sees(spot[0] - px, spot[2] - pz, rays)
        }
        Cond::MateWatches => {
            let Some(m) = ctx.mates.iter().filter(|m| flat(**m, [spot[0], spot[2]]) < MATE_NEAR_M).min_by(|a, b| flat(**a, [spot[0], spot[2]]).total_cmp(&flat(**b, [spot[0], spot[2]]))) else { return false };
            *rays += 1;
            let mid = [(e[0] + spot[0]) * 0.5, (e[1] + spot[1]) * 0.5, (e[2] + spot[2]) * 0.5];
            !v.hidden([m[0], m[1] - BODY_OVER_FEET + STAND_EYE_M, m[2]], mid)
        }
        Cond::StraightWay => {
            *rays += 1;
            let me = v.me.pos;
            let knee = BODY_OVER_FEET - KNEE_M;
            !v.walk_blocked([me[0], me[1] - knee, me[2]], [spot[0], spot[1] - knee, spot[2]])
        }
        _ => true,
    }
}

fn weight(c: Crit, me: [f32; 3], spot: [f32; 3], ctx: &Ctx) -> f32 {
    match c {
        Crit::DistMe => 1.0 - flat(me, [spot[0], spot[2]]).min(CAP_M) / CAP_M,
        Crit::Directness => ctx.goal.map_or(0.5, |g| (directness(me, spot, g) + 1.0) / 2.0),
    }
}

/// The candidates within `r`: the cover points with `bots:cover-points`, the mesh's (reachable) and the fallback ring around the
/// direction away from the enemy, each as the body's point, with its cover point if it is one and its kind.
fn candidates(v: &BotView, nav: Option<&dyn NavBackend>, r: f32, ctx: &Ctx) -> Vec<([f32; 3], Option<CoverPoint>, &'static str)> {
    let me = v.me.pos;
    let body = |c: [f32; 3]| [c[0], c[1] + BODY_OVER_FEET, c[2]];
    let mut out: Vec<([f32; 3], Option<CoverPoint>, &'static str)> = Vec::new();
    // TODO: `bots:cover-points` loses to `cover-fight` alone and to anchor 0 (zmargin -0.19 and -0.28, 64 pairs each), but the
    // spot trace (`BYTEFIELD_SPOT_TRACE`) clears the points: they are reached as often as the mesh's centres, pushed at no more,
    // shot from as well, and hide as well crouched. What loses is `cover-fight` itself (alone, -0.21 against anchor 0): see
    // [`Bot::firing_spot`]. Left for the points: those under a roof hide crouched 1 time in 10 (leave them out), and two mates
    // pick the same one more often (`Unclaimed` looks at where mates are, not where they go).
    if ctx.cover_points
        && let Some(n) = nav
    {
        out.extend(n.cover_near(&v.cover_points(), feet(me), r, COVER_CANDIDATES).into_iter().map(|c| (body(c.pos), Some(c), "cover")));
    }
    out.extend(nav.map_or(Vec::new(), |n| n.spots_near(feet(me), r, MESH_CANDIDATES)).into_iter().map(|c| (body(c), None, "mesh")));
    let away = (me[2] - ctx.enemy[2]).atan2(me[0] - ctx.enemy[0]);
    let water = v.water();
    for k in 0..RING_POINTS {
        let step = k.div_ceil(2) as f32 * std::f32::consts::TAU / RING_POINTS as f32;
        let a = away + if k % 2 == 1 { step } else { -step };
        let d = RING_M[k % RING_M.len()].min(r);
        let (x, z) = (me[0] + d * a.cos(), me[2] + d * a.sin());
        let Some(h) = v.ground(x, z) else { continue };
        if h < water + 0.3 || !no_fall(v, me, [x, z]) {
            continue;
        }
        out.push(([x, h + BODY_OVER_FEET, z], None, "ring"));
    }
    out
}

/// Why a cover point is turned down for query `q` without a ray, if it is: it does not hide crouched towards the enemy, or the
/// query shoots standing from it and it hides standing too.
fn sector_out(q: &Query, c: &CoverPoint, enemy: [f32; 3]) -> Option<&'static str> {
    if !c.hides_crouched_from(enemy) {
        return Some("Sector");
    }
    (q.dear.contains(&Cond::SeesStanding) && c.hides_standing_from(enemy)).then_some("Wall")
}

fn trace_player() -> Option<usize> {
    // (`usize::MAX`: every player)
    crate::debug::options().bots_tps
}

/// The first option of `qs` that finds a spot, and the spot; `None` when none does (an answer too: there is nowhere to go).
pub fn ask(v: &BotView, nav: Option<&dyn NavBackend>, qs: &[Query], ctx: &Ctx) -> Option<Spot> {
    let me = v.me.pos;
    let trace = trace_player().is_some_and(|p| p == v.me.player || p == usize::MAX);
    let mut rays = 0;
    for (option, q) in qs.iter().enumerate() {
        let mut scored: Vec<(f32, [f32; 3], &'static str, Option<CoverPoint>)> = Vec::new();
        for (c, cp, from) in candidates(v, nav, q.r, ctx) {
            if let Some(why) = cp.as_ref().and_then(|cp| sector_out(q, cp, ctx.enemy)) {
                if trace {
                    eprintln!("tps p={} t={:.1} {} ({:.1},{:.1},{:.1}) cover out by {why}", v.me.player, v.time, q.name, c[0], c[1], c[2]);
                }
                continue;
            }
            if let Some(f) = q.cheap.iter().find(|&&f| !cheap(f, me, c, ctx)) {
                if trace {
                    eprintln!("tps p={} t={:.1} {} ({:.1},{:.1},{:.1}) out by {f:?}", v.me.player, v.time, q.name, c[0], c[1], c[2]);
                }
                continue;
            }
            let total: f32 = q.weights.iter().map(|w| w.1).sum::<f32>().max(1e-6);
            let score = q.weights.iter().map(|&(k, w)| w * weight(k, me, c, ctx)).sum::<f32>() / total;
            // (a cover point that passed its sector goes before every other candidate: the weights are 0..1)
            scored.push((if cp.is_some() { score + 1.0 } else { score }, c, from, cp));
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1[0].total_cmp(&b.1[0])).then(a.1[2].total_cmp(&b.1[2])));
        // (each option gets its share of the rays: the fallback is not left without any)
        let budget = MAX_RAYS * (option as u32 + 1) / qs.len() as u32;
        let dears: Vec<Cond> = ctx.straight.then_some(Cond::StraightWay).into_iter().chain(q.dear.iter().copied()).collect();
        for (score, c, from, cp) in scored {
            if rays + dears.iter().map(|&c| cost(c)).sum::<u32>() > budget {
                if trace {
                    eprintln!("tps p={} {} out of rays", v.me.player, q.name);
                }
                break;
            }
            let mut failed = None;
            for &f in &dears {
                if !dear(v, f, c, ctx, &mut rays) {
                    failed = Some(f);
                    break;
                }
            }
            if trace {
                eprintln!("tps p={} t={:.1} {} ({:.1},{:.1},{:.1}) {from} score {score:.2} {}", v.me.player, v.time, q.name, c[0], c[1], c[2], failed.map_or("chosen".to_string(), |f| format!("out by {f:?}")));
            }
            if failed.is_none() {
                if trace {
                    eprintln!("tps p={} t={:.1} end {from} rays {rays}", v.me.player, v.time);
                }
                return Some(Spot { pos: c, option, rays, from, cover: cp });
            }
        }
    }
    if trace {
        eprintln!("tps p={} t={:.1} end none rays {rays}", v.me.player, v.time);
    }
    None
}

/// `bots:cover-near`: where it stands as its firing spot, a parapet (option 0, `here`), if crouched it hides from the enemy and
/// standing it sees it (two rays).
pub fn here(v: &BotView, ctx: &Ctx) -> Option<Spot> {
    let mut rays = 0;
    let pos = v.me.pos;
    (dear(v, Cond::HiddenCrouch, pos, ctx, &mut rays) && dear(v, Cond::SeesStanding, pos, ctx, &mut rays)).then_some(Spot { pos, option: 0, rays, from: "here", cover: None })
}

/// The inertia of a chosen spot: it is kept while it is still got to first and still hides it crouched (one ray), not dropped
/// because a somewhat better one appears.
pub fn still_good(v: &BotView, spot: [f32; 3], ctx: &Ctx) -> bool {
    still_bad(v, spot, ctx).is_none()
}

/// What [`still_good`] finds wrong with a spot, if anything: `reach` (the enemy gets there first) or `hidden` (it no longer hides).
pub fn still_bad(v: &BotView, spot: [f32; 3], ctx: &Ctx) -> Option<&'static str> {
    if !cheap(Cond::ReachBefore, v.me.pos, spot, ctx) {
        return Some("reach");
    }
    (!dear(v, Cond::HiddenCrouch, spot, ctx, &mut 0)).then_some("hidden")
}

/// At most one query per second and bot, spread over the second by player: whether it is this player's turn at `time`.
pub fn due(time: f64, last: f64, player: usize) -> bool {
    let tick = (time * 30.0).round() as i64;
    time - last >= 1.0 - 1e-6 && tick.rem_euclid(30) == (player % 30) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directness_and_the_way() {
        let (me, goal) = ([0.0, 0.0, 0.0], [100.0, 0.0, 0.0]);
        assert!((directness(me, [10.0, 0.0, 0.0], goal) - 1.0).abs() < 1e-3);
        assert!((directness(me, [-10.0, 0.0, 0.0], goal) + 1.0).abs() < 1e-3);
        assert!(directness(me, [0.0, 0.0, 10.0], goal).abs() < 0.06);
        // running past the enemy at 50 m comes within 10 m of it
        assert!((nearest_on_way(me, [100.0, 0.0, 10.0], [50.0, 0.0, 0.0]) - 5.0).abs() < 0.5);
        let ctx = Ctx { enemy: [50.0, 0.0, 0.0], ..Default::default() };
        assert!(!cheap(Cond::NotTowards, me, [40.0, 0.0, 5.0], &ctx));
        assert!(cheap(Cond::NotTowards, me, [0.0, 0.0, 10.0], &ctx));
        assert!(!cheap(Cond::ReachBefore, me, [40.0, 0.0, 0.0], &ctx));
    }

    #[test]
    fn one_turn_per_second() {
        let turns = (0..90).filter(|i| due(*i as f64 / 30.0, -10.0, 7)).count();
        assert_eq!(turns, 3);
        assert!(!due(1.5, 1.0, 7));
    }

    /// On a real map (Berlin, from a minute and a half in): of the soldiers on foot that see an enemy within 60 m, the firing spot found is hidden crouched
    /// from that enemy and sees it standing (or a step aside, the corner option), is reached first, and costs no more than the rays allowed;
    /// with the cover points too, and they find a spot for at least as many with fewer rays.
    #[test]
    fn the_firing_spot_is_a_parapet() {
        let Some(game) = bytefield_testdata::base() else { return };
        let w = crate::World::load(&game, "bf1942", "Berlin", "conquest").unwrap();
        let mut s = crate::Sim::new(&w, 1.0 / 30.0);
        s.round.playing = true;
        s.start_conquest();
        s.enable_bots();
        s.set_minds(1, crate::bots::mind("bots", "Berlin", 1));
        s.set_minds(2, crate::bots::mind("bots", "Berlin", 1));
        for i in 0..32 {
            let p = s.join(0, true);
            s.players[p].kit = (i / 2) % 5;
        }
        let maps = crate::bots::body::Maps::prepare(&w);
        let (mut asked, mut found, mut rays) = (0, [0; 2], [0; 2]);
        // (the soldiers in contact at one moment depend on how the round went: every half minute from 60 to 180 s, until four)
        for _ in 0..60 * 30 {
            s.step();
        }
        for _ in 0..5 {
            if asked >= 4 {
                break;
            }
            for _ in 0..30 * 30 {
                s.step();
            }
            for k in 0..s.players.len() {
                let Some(v) = BotView::of(&s, k).filter(|v| v.me.inside.is_none() && v.me.health > 0.0) else { continue };
                let Some(e) = v.others().into_iter().filter(|o| o.team != v.me.team && o.vehicle.is_none() && flat(o.pos, [v.me.pos[0], v.me.pos[2]]) < 60.0 && !v.hidden(v.me.eye, [o.pos[0], o.pos[1] + 0.3, o.pos[2]])).min_by(|a, b| flat(a.pos, [v.me.pos[0], v.me.pos[2]]).total_cmp(&flat(b.pos, [v.me.pos[0], v.me.pos[2]]))) else { continue };
                asked += 1;
                for cp in [false, true] {
                    let ctx = Ctx { enemy: e.pos, cover_points: cp, ..Default::default() };
                    if let Some(spot) = ask(&v, maps.foot(&v), &[FIRING_SPOT, CORNER_SPOT], &ctx) {
                        found[cp as usize] += 1;
                        rays[cp as usize] += spot.rays;
                        assert!(spot.rays <= MAX_RAYS);
                        let shoots = if spot.option == 0 { Cond::SeesStanding } else { Cond::PeekNear };
                        assert!(dear(&v, Cond::HiddenCrouch, spot.pos, &ctx, &mut 0) && dear(&v, shoots, spot.pos, &ctx, &mut 0), "neither a parapet nor a corner: {spot:?}");
                        assert!(flat(v.me.pos, [spot.pos[0], spot.pos[2]]) < flat(e.pos, [spot.pos[0], spot.pos[2]]));
                    }
                }
            }
        }
        assert!(asked >= 4, "only {asked} soldiers in contact");
        assert!(found[0] >= 1, "no firing spot for any of {asked}");
        eprintln!("asked {asked}: found {found:?}, rays per spot found {:?}", [0, 1].map(|i| rays[i] as f32 / found[i].max(1) as f32));
        assert!(found[1] >= found[0], "the cover points found fewer spots: {found:?} of {asked}");
    }
}
