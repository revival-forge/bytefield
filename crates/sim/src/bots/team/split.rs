//! The split: which flag each squad or loose player goes to, by shares or by strength.
use super::*;

/// Where a unit goes (a squad, or a loose player) after the split.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    pub flag: usize,
    pub urgent: bool,
}

/// What is split: a squad or a loose player, with its living members.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    /// where its living members are (the first, the leader)
    pub pos: Vec<[f32; 3]>,
    /// the flag it had (kept if it is still worth it: it does not change its mind every few seconds)
    pub previous: Option<usize>,
    /// the flags it found no way to ([`crate::bots::trip`]): it attacks another if there is one
    pub avoid: Vec<usize>,
}

/// The flag split among units (see the module). Deterministic: same view, same split.
pub fn split_up(t: f64, view: &TeamView, reports: &[Report], lost: &[Option<f64>], units: &[Unit], levers: &Levers) -> Vec<Option<Destination>> {
    split_up_with(t, view, reports, lost, units, levers, &BotParams::default(), None)
}

/// [`split_up`], with the side's named parameters (`p`) and the attack by strength when `intel` is given (`bots:strength`).
#[allow(clippy::too_many_arguments)]
pub fn split_up_with(t: f64, view: &TeamView, reports: &[Report], lost: &[Option<f64>], units: &[Unit], levers: &Levers, p: &BotParams, intel: Option<&crate::bots::intel::Intel>) -> Vec<Option<Destination>> {
    let mine = view.team;
    let fl = &view.flags;
    let mut out: Vec<Option<Destination>> = vec![None; units.len()];
    if fl.is_empty() {
        return out;
    }
    let weight = |u: usize| units[u].pos.len();
    let total: usize = (0..units.len()).map(weight).sum();
    let seen_ones = |i: usize, r: f32, s: f64| reports.iter().filter(|p| t - p.t < s && flat(p.pos, fl[i].pos) < r).count();
    // TODO: with no flag that changes hands (DC_No_Fly_Zone_Day2 and DC_Weapon_Bunkers: both flags are bases) the squads are sent to
    // the enemy's base and fail the trip there (Day2: 2.5 km of desert, 30–38 % standing on foot at 8 min); a player goes where the
    // enemy is (the side's reports) or takes an aircraft. How: `BYTEFIELD_BOTS_TRACE=1 tournament3 … --maps
    // DC_Final:DC_No_Fly_Zone_Day2 --seeds 1 --minutes 4 --threads 1` (`sq` and `failed` lines).
    let takeable_ones: Vec<usize> = (0..fl.len()).filter(|&i| fl[i].takeable).collect();
    let own: Vec<usize> = takeable_ones.iter().copied().filter(|&i| fl[i].owner == mine).collect();
    // (those that cannot be taken on foot are no attack target: going there wastes the people; the enemy's still mark the front)
    // (unless none is, and the side has an aircraft that lands people: a trip by air reaches them, [`crate::bots::trip`]. Not before: in
    // DC_Sea_Rigs four of five flags are on platforms and there are two helicopters; with all five attackable the squads with no
    // aircraft went round the platforms and nobody took the one on land)
    let by_air = view.vehicles.iter().any(|c| crate::bots::trip::transport_aircraft(c) || crate::bots::trip::drop_aircraft(c));
    let on_foot_ones: Vec<usize> = takeable_ones.iter().copied().filter(|&i| fl[i].owner != mine && fl[i].on_foot).collect();
    let attackable: Vec<usize> = if on_foot_ones.is_empty() && by_air { takeable_ones.iter().copied().filter(|&i| fl[i].owner != mine).collect() } else { on_foot_ones };
    let enemy_ones: Vec<usize> = (0..fl.len()).filter(|&k| fl[k].owner != mine && fl[k].owner != 0).collect();
    // how near a unit is: the leader's to go, any of its own's for an urgency
    let d = |u: usize, i: usize| units[u].pos.first().map_or(f32::MAX, |p| flat(*p, fl[i].pos));
    let d_min = |u: usize, i: usize| units[u].pos.iter().map(|p| flat(*p, fl[i].pos)).fold(f32::MAX, f32::min);
    let free_ones = |out: &[Option<Destination>]| (0..units.len()).filter(|&u| out[u].is_none() && weight(u) > 0).collect::<Vec<usize>>();
    let people = |out: &[Option<Destination>], i: usize| (0..units.len()).filter(|&u| out[u].is_some_and(|x| x.flag == i)).map(weight).sum::<usize>();
    // 1. urgency: the own flag being taken (enemy on top, or just lost and still neutral): the nearest, at once
    for &i in &takeable_ones {
        let on_top = seen_ones(i, fl[i].radio + 15.0, 6.0);
        let being_taken = (fl[i].owner == mine && on_top > 0) || (fl[i].owner == 0 && lost.get(i).copied().flatten().is_some_and(|p| t - p < 25.0));
        if !being_taken {
            continue;
        }
        let mut near: Vec<usize> = free_ones(&out).into_iter().filter(|&u| d_min(u, i) < p.urgency_m).collect();
        near.sort_by(|&a, &b| d_min(a, i).total_cmp(&d_min(b, i)).then(a.cmp(&b)));
        for u in near {
            if people(&out, i) >= (1 + on_top).min(p.urgency_most) {
                break;
            }
            out[u] = Some(Destination { flag: i, urgent: true });
        }
    }
    // 2. defence: the own flag with enemies seen nearby; first whoever was already defending it
    for &i in &own {
        let n = seen_ones(i, p.threat_m, p.report_s);
        if n == 0 {
            continue;
        }
        let mut near: Vec<usize> = free_ones(&out).into_iter().filter(|&u| d(u, i) < p.defend_m).collect();
        near.sort_by(|&a, &b| (units[a].previous != Some(i)).cmp(&(units[b].previous != Some(i))).then(d(a, i).total_cmp(&d(b, i))).then(a.cmp(&b)));
        for u in near {
            if people(&out, i) >= n.min(p.defend_most) {
                break;
            }
            out[u] = Some(Destination { flag: i, urgent: false });
        }
    }
    let mut left = free_ones(&out);
    if left.is_empty() {
        return out;
    }
    let remoteness = |i: usize| attackable.iter().map(|&k| flat(fl[i].pos, fl[k].pos)).fold(f32::MAX, f32::min);
    // where to go, with what share of the people: the front ones when there is nothing to attack, else the takeable ones nearest ours
    let (targets, pesos): (Vec<usize>, Vec<f32>) = if attackable.is_empty() {
        // (the front: our flags nearest one of the enemy's, takeable or not; that is where whoever comes for ours comes from)
        // With the enemy holding flags we cannot reach on foot (GC_Mos_Eisley, team 1) the front is under pressure from vehicles and
        // aircraft: cover every flag we own, an equal share each (a decaying share left the third flag empty and it fell at 90 s).
        // Otherwise (Berlin and the like: nothing of theirs is out of reach, they just hold nothing to take) the front-weighted spread stays.
        let to_the_enemy = |i: usize| enemy_ones.iter().map(|&k| flat(fl[i].pos, fl[k].pos)).fold(f32::MAX, f32::min);
        let mut b = own.clone();
        b.sort_by(|&x, &y| to_the_enemy(x).total_cmp(&to_the_enemy(y)).then(x.cmp(&y)));
        let pressed = takeable_ones.iter().any(|&k| fl[k].owner != mine && fl[k].owner != 0 && !fl[k].on_foot);
        b.truncate(if pressed { 4 } else { 3 });
        let w: Vec<f32> = (0..b.len()).map(|r| if pressed { 1.0 } else { 1.0 / (r as f32 + 1.0) }).collect();
        (b, w)
    } else {
        // 3. guard: the own front flag keeps a small unit (never half the side)
        if let Some(&front) = own.iter().min_by(|&&a, &&b| remoteness(a).total_cmp(&remoteness(b)).then(a.cmp(&b)))
            && levers.guard
            && total >= 6
            && people(&out, front) == 0
        {
            let fits = |u: usize| (weight(u) as f32) <= (total as f32 * p.guard_share).max(1.0);
            let g = left.iter().copied().filter(|&u| fits(u) && d(u, front) < p.guard_m).min_by(|&a, &b| (units[a].previous != Some(front)).cmp(&(units[b].previous != Some(front))).then(d(a, front).total_cmp(&d(b, front))).then(a.cmp(&b)));
            if let Some(u) = g {
                out[u] = Some(Destination { flag: front, urgent: false });
                left.retain(|&x| x != u);
            }
        }
        // 4. attack
        if let Some(intel) = intel {
            let undefended = |i: usize| seen_ones(i, UNDEFENDED_M, UNDEFENDED_S) == 0;
            attack_by_strength(view, units, &attackable, &own, &left, intel, &undefended, &mut out);
            return out;
        }
        let anchor: Vec<[f32; 3]> = if own.is_empty() { left.iter().filter_map(|&u| units[u].pos.first().copied()).collect() } else { own.iter().map(|&i| fl[i].pos).collect() };
        let near_the_side = |i: usize| anchor.iter().map(|&p| flat(fl[i].pos, p)).fold(f32::MAX, f32::min);
        let mut b = attackable.clone();
        b.sort_by(|&x, &y| near_the_side(x).total_cmp(&near_the_side(y)).then(x.cmp(&y)));
        let people_q: usize = left.iter().map(|&u| weight(u)).sum();
        b.truncate(if levers.targets > 0 { levers.targets } else if people_q >= p.attack_three_people && left.len() >= 3 { 3 } else { 2 });
        let w: Vec<f32> = match b.len() {
            1 => vec![1.0],
            2 => vec![0.6, p.attack_w2_second],
            3 => vec![0.45, p.attack_w3_second, p.attack_w3_third],
            n => (0..n).map(|r| 1.0 / (r as f32 + 1.0)).collect(),
        };
        (b, w)
    };
    // 5. nothing to take and nothing of ours to lose (two bases that do not change hands: the round is won by kills): to the enemy's
    // flags that are reached on foot, the nearest first. Waiting at the base is not playing the map.
    let (targets, pesos) = if targets.is_empty() {
        let since: Vec<[f32; 3]> = left.iter().filter_map(|&u| units[u].pos.first().copied()).collect();
        let near_us = |i: usize| since.iter().map(|&p| flat(fl[i].pos, p)).fold(f32::MAX, f32::min);
        let mut b: Vec<usize> = enemy_ones.iter().copied().filter(|&i| fl[i].on_foot).collect();
        b.sort_by(|&x, &y| near_us(x).total_cmp(&near_us(y)).then(x.cmp(&y)));
        b.truncate(2);
        let w = if b.len() == 2 { vec![0.6, 0.4] } else { vec![1.0] };
        (b, w)
    } else {
        (targets, pesos)
    };
    if targets.is_empty() {
        return out;
    }
    let sum: f32 = pesos.iter().sum();
    let people_q: usize = left.iter().map(|&u| weight(u)).sum();
    let quota: Vec<f32> = pesos.iter().map(|p| (p / sum * people_q as f32).max(1.0)).collect();
    let mut how_many = vec![0usize; targets.len()];
    // whoever was already going to one of them keeps going (if it fits); then the nearest choose first
    let near_target = |u: usize| targets.iter().map(|&i| d(u, i)).fold(f32::MAX, f32::min);
    left.sort_by(|&a, &b| units[a].previous.is_none_or(|p| !targets.contains(&p)).cmp(&units[b].previous.is_none_or(|p| !targets.contains(&p))).then(near_target(a).total_cmp(&near_target(b))).then(a.cmp(&b)));
    for u in left {
        let avoided = |r: usize| if units[u].avoid.contains(&targets[r]) { 1.0e6 } else { 0.0 };
        let cost = |r: usize| d(u, targets[r]) + p.quota_over_m * ((how_many[r] + weight(u)) as f32 / quota[r] - 1.0).max(0.0) - if units[u].previous == Some(targets[r]) { p.keep_flag_m } else { 0.0 } + avoided(r);
        let r = (0..targets.len()).min_by(|&a, &b| cost(a).total_cmp(&cost(b)).then(a.cmp(&b))).unwrap();
        how_many[r] += weight(u);
        out[u] = Some(Destination { flag: targets[r], urgent: false });
    }
    out
}

/// `bots:strength`: a flag nobody of the enemy has been seen within this of in this long is undefended, m and s.
pub const UNDEFENDED_M: f32 = 100.0;
pub const UNDEFENDED_S: f64 = 20.0;
/// `bots:strength`: the flag a unit already had counts this much nearer (it does not change its mind every split), m.
pub const PREVIOUS_M: f32 = 150.0;
/// `bots:strength`: the people a flag needs are `NEED_PER_ENEMY` times its estimated presence plus one, between these.
// TODO: neither 1.5 nor 4.5 (the estimate is about a third of the real defenders, `examples/intel_sanity`) raised B36 (attackers
// at least the defenders 20 s after a squad arrives): the units sent to one flag arrive one after another. Regrouping before going in
// (an assembly point 100-150 m short of the flag) is the next thing to try; on GC_Endor the attack step never acts (the round is the same).
pub const NEED_PER_ENEMY: f32 = 1.5;
pub const NEED_MIN: usize = 2;
pub const NEED_MAX: usize = 12;
/// `bots:strength`: a neutral flag's bonus, and the priority of an undefended one (over any other).
pub const NEUTRAL_BONUS: f32 = 0.2;
pub const UNDEFENDED_PRIORITY: f32 = 1.5;

/// Step 4 by strength (the assignment of the multiplayer bots' commanders): each attackable flag has a priority (near the side,
/// lower the more it is defended, a bonus if neutral, the top one if undefended) and a need (more people than its estimated
/// defenders). In a loop, the flag of highest priority takes whole units, the cheapest (nearest; the one it had counts
/// [`PREVIOUS_M`] nearer), until its need is covered, and its priority drops by its share of the side for each person it took.
/// When the free people cannot cover the best one's need, they all go to it (or to the second, if the second's need is covered
/// and the first's is not): no shares that are each too few.
#[allow(clippy::too_many_arguments)]
fn attack_by_strength(view: &TeamView, units: &[Unit], attackable: &[usize], own: &[usize], left: &[usize], intel: &crate::bots::intel::Intel, undefended: &dyn Fn(usize) -> bool, out: &mut [Option<Destination>]) {
    let fl = &view.flags;
    let weight = |u: usize| units[u].pos.len();
    let d = |u: usize, i: usize| units[u].pos.first().map_or(f32::MAX, |p| flat(*p, fl[i].pos));
    let anchor: Vec<[f32; 3]> = if own.is_empty() { left.iter().filter_map(|&u| units[u].pos.first().copied()).collect() } else { own.iter().map(|&i| fl[i].pos).collect() };
    let near_the_side = |i: usize| anchor.iter().map(|&p| flat(fl[i].pos, p)).fold(f32::MAX, f32::min);
    let (lo, hi) = attackable.iter().map(|&i| near_the_side(i)).fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let presence = |i: usize| intel.presence(i);
    let need: Vec<usize> = attackable.iter().map(|&i| ((NEED_PER_ENEMY * presence(i)).ceil() as usize + 1).clamp(NEED_MIN, NEED_MAX)).collect();
    let mut prio: Vec<f32> = attackable
        .iter()
        .map(|&i| {
            if undefended(i) && presence(i) < 0.1 {
                return UNDEFENDED_PRIORITY;
            }
            let near = if hi > lo { 1.0 - (near_the_side(i) - lo) / (hi - lo) } else { 1.0 };
            near / (1.0 + presence(i)) + if fl[i].owner == 0 { NEUTRAL_BONUS } else { 0.0 }
        })
        .collect();
    let total: usize = left.iter().map(|&u| weight(u)).sum::<usize>().max(1);
    let top = prio.iter().copied().fold(0.0f32, f32::max).max(1e-3);
    let can = |u: usize, r: usize| !units[u].avoid.contains(&attackable[r]);
    let cost = |u: usize, r: usize| d(u, attackable[r]) - if units[u].previous == Some(attackable[r]) { PREVIOUS_M } else { 0.0 };
    let mut free: Vec<usize> = left.to_vec();
    // concentration: when the free people cannot cover the best flag's need
    let order = |prio: &[f32]| {
        let mut o: Vec<usize> = (0..attackable.len()).collect();
        o.sort_by(|&a, &b| prio[b].total_cmp(&prio[a]).then(a.cmp(&b)));
        o
    };
    let o = order(&prio);
    if let Some(&best) = o.first()
        && total < need[best]
    {
        let pick = o.get(1).copied().filter(|&second| need[second] <= total).unwrap_or(best);
        for &u in &free {
            let r = if can(u, pick) { pick } else { (0..attackable.len()).find(|&r| can(u, r)).unwrap_or(pick) };
            out[u] = Some(Destination { flag: attackable[r], urgent: false });
        }
        return;
    }
    let mut got = vec![0usize; attackable.len()];
    while !free.is_empty() {
        let Some(&r) = order(&prio).iter().find(|&&r| prio[r] > f32::MIN && free.iter().any(|&u| can(u, r))) else {
            // (nobody can go anywhere still asked for: the first flag each one can)
            for &u in &free {
                if let Some(r) = (0..attackable.len()).find(|&r| can(u, r)) {
                    out[u] = Some(Destination { flag: attackable[r], urgent: false });
                }
            }
            return;
        };
        let mut mine: Vec<usize> = free.iter().copied().filter(|&u| can(u, r)).collect();
        mine.sort_by(|&a, &b| cost(a, r).total_cmp(&cost(b, r)).then(a.cmp(&b)));
        let mut taken = 0;
        for u in mine {
            if taken > 0 && got[r] >= need[r] {
                break;
            }
            out[u] = Some(Destination { flag: attackable[r], urgent: false });
            got[r] += weight(u);
            taken += weight(u);
            free.retain(|&x| x != u);
        }
        prio[r] -= top * taken as f32 / total as f32;
    }
}
