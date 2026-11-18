//! Which flag a bot goes to. A pure function: from the flags, the team, the place and the rng.
use super::rng::Rng;
use crate::bot_api::Flag;

pub use crate::bots3::body::Choice;

/// Share of defenders at the front flag.
pub const P_FRONT: f32 = 0.7;

fn flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// - With no own flags that can be lost, it attacks the nearest one that can be taken.
/// - With all of them, it defends: more people at the one nearest the enemy's.
/// - With some of each: whoever has more pushes towards the nearest (three in four); whoever has fewer attacks with probability
///   `p_fewer`: in Berlin it is low, because team 2 stays at B3 (B1 changes hands in 1 of 8 rounds), and on the other maps it is
///   what the specification says, «whoever is at a flag disadvantage attacks the nearest».
/// - A base that can never be taken is no destination.
pub fn choose(flags: &[Flag], team: u32, pos: [f32; 3], p_fewer: f32, p_second: f32, rng: &mut Rng) -> Option<Choice> {
    let takeable_ones: Vec<usize> = (0..flags.len()).filter(|&i| flags[i].takeable).collect();
    let (mine_ones, others_theirs): (Vec<usize>, Vec<usize>) = takeable_ones.iter().partition(|&&i| flags[i].team == team);
    let nearest = |l: &[usize]| l.iter().copied().min_by(|&a, &b| flat(flags[a].pos, pos).total_cmp(&flat(flags[b].pos, pos)).then(a.cmp(&b)));
    // the nearest; sometimes the next, so that not all go to the same one when they are hundreds of metres apart
    let attack = |rng: &mut Rng| {
        let first = nearest(&others_theirs)?;
        let rest: Vec<usize> = others_theirs.iter().copied().filter(|&i| i != first).collect();
        let flag = if rng.si(p_second) { nearest(&rest).unwrap_or(first) } else { first };
        Some(Choice { flag, attacks: true })
    };
    if mine_ones.is_empty() {
        return attack(rng);
    }
    let p_attack = if 2 * mine_ones.len() > takeable_ones.len() { 0.75 } else { p_fewer };
    if !others_theirs.is_empty() && rng.si(p_attack) {
        return attack(rng);
    }
    // the front one, the nearest to what the enemy has, gets the majority; the others, the rest
    let remoteness = |i: usize| (0..flags.len()).filter(|&j| flags[j].team != team && flags[j].team != 0).map(|j| flat(flags[i].pos, flags[j].pos)).fold(f32::MAX, f32::min);
    let front = mine_ones.iter().copied().min_by(|&a, &b| remoteness(a).total_cmp(&remoteness(b)).then(a.cmp(&b)))?;
    let rest: Vec<usize> = mine_ones.iter().copied().filter(|&i| i != front).collect();
    if rest.is_empty() || rng.si(P_FRONT) {
        return Some(Choice { flag: front, attacks: false });
    }
    // of the others, somewhat more to the one farthest from the front: the near ones are already reached by respawning
    let far = |i: usize| flat(flags[i].pos, flags[front].pos);
    let distant = rest.iter().copied().max_by(|&a, &b| far(a).total_cmp(&far(b)).then(a.cmp(&b)))?;
    let k = ((rng.one() * rest.len() as f32) as usize).min(rest.len() - 1);
    Some(Choice { flag: if rng.si(0.25) { distant } else { rest[k] }, attacks: false })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Berlin's flags, as in the specification.
    pub(crate) fn berlin() -> Vec<Flag> {
        let f = |x, z, radius, team, takeable| Flag { pos: [x, 38.0, z], radius, team, takeable, contested: false, taker: 0, bar: 0.0 };
        vec![f(1825.0, 1790.0, 10.0, 1, true), f(1799.0, 1961.0, 5.0, 2, false), f(1827.0, 1854.0, 10.0, 1, true), f(1698.0, 1781.0, 9.0, 1, true)]
    }

    fn split(flags: &[Flag], team: u32, pos: [f32; 3]) -> ([u32; 4], u32) {
        let mut a = Rng::new(11);
        let (mut n, mut attack_count) = ([0; 4], 0);
        for _ in 0..1000 {
            let e = choose(flags, team, pos, 0.1, 0.0, &mut a).unwrap();
            n[e.flag] += 1;
            attack_count += e.attacks as u32;
        }
        (n, attack_count)
    }

    #[test]
    fn whoever_has_none_attacks_the_nearest() {
        let (n, attack_count) = split(&berlin(), 2, [1799.0, 38.0, 1961.0]);
        assert_eq!(n, [0, 0, 1000, 0]);
        assert_eq!(attack_count, 1000);
    }

    #[test]
    fn never_goes_to_the_base_that_cannot_be_taken() {
        for e in [1, 2] {
            assert_eq!(split(&berlin(), e, [1799.0, 38.0, 1900.0]).0[1], 0);
        }
    }

    #[test]
    fn whoever_has_them_all_defends_the_front_one_more() {
        let (n, attack_count) = split(&berlin(), 1, [1825.0, 38.0, 1790.0]);
        assert_eq!(attack_count, 0);
        assert!(n[2] > 550 && n[0] > 100 && n[3] > 100, "{n:?}");
    }

    #[test]
    fn after_taking_one_almost_all_guard_it() {
        let mut f = berlin();
        f[2].team = 2;
        let (n, attack_count) = split(&f, 2, [1799.0, 38.0, 1961.0]);
        assert!(n[2] > 700, "{n:?}");
        assert!(attack_count < 300);
                assert_eq!(n[3], 0, "the nearest is B1, not B4: {n:?}");
    }

    #[test]
    fn on_open_maps_whoever_has_fewer_attacks() {
        let mut f = berlin();
        f[2].team = 2;
        let mut a = Rng::new(5);
        let attack_count = (0..1000).filter(|_| choose(&f, 2, [1799.0, 38.0, 1961.0], 0.7, 0.0, &mut a).unwrap().attacks).count();
        assert!(attack_count > 600, "{attack_count}");
    }

    #[test]
    fn sometimes_attacks_the_next_and_never_a_base() {
        let mut f = berlin();
        for x in &mut f {
            x.team = 0;
        }
        f[1].team = 2;
        let mut a = Rng::new(5);
        let n: Vec<usize> = (0..1000).map(|_| choose(&f, 2, [1799.0, 38.0, 1961.0], 0.7, 0.3, &mut a).unwrap().flag).collect();
        let second = n.iter().filter(|&&b| b == 0).count();
        assert!(second > 200 && second < 400, "{second}");
        assert!(!n.contains(&1));
    }

    #[test]
    fn without_flags_there_is_no_destination() {
        assert_eq!(choose(&[], 1, [0.0; 3], 0.1, 0.0, &mut Rng::new(1)), None);
    }
}
