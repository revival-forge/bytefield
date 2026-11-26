//! The streetwise trait (`bots:streetwise`, experimental) and randomness without streaks.
//!
//! **The trait**: one bot in four of a side has it high (0.7 to 1), the rest low (0 to 0.2): the rogue of the group, not a group of
//! rogues. It is fixed when the bot first asks ([`Traits::of`]), from its own generator seeded by the side and the player, so the
//! side's generator is untouched. Without the lever it is 0 for everyone. `BYTEFIELD_BOTS_STREETWISE=<player>:<value>` forces it.
//!
//! **Filtered randomness** (the filtered-randomness rules of the game AI books): a [`Filtered`] keeps the history of one decision
//! (each use its own, or the streaks come back) and turns a draw over when it would make what a person reads as a pattern:
//! a run of four of the same coin side is turned over three times in four; the patterns `1100 1100` and `111000` are turned over;
//! an integer does not repeat the last one, does not make `x, y, x`, and no value comes three times in the last ten.
use super::body::rng::Rng;
use std::collections::{BTreeMap, VecDeque};

/// One bot in this many of a side has the trait high.
pub const ONE_IN: usize = 4;
/// The history a [`Filtered`] keeps.
pub const HISTORY: usize = 12;

/// Randomness without streaks, for one decision.
#[derive(Clone, Debug, Default)]
pub struct Filtered {
    bits: VecDeque<bool>,
    ints: VecDeque<u32>,
}

impl Filtered {
    /// A coin with probability one half, filtered.
    pub fn coin(&mut self, rng: &mut Rng) -> bool {
        let mut b = rng.si(0.5);
        let n = self.bits.len();
        let last = |k: usize| self.bits.get(n.wrapping_sub(k)).copied();
        // a run of four: turned over three times in four
        if n >= 4 && (1..=4).all(|k| last(k) == Some(b)) && rng.si(0.75) {
            b = !b;
        }
        // 1100 1100 (what comes would repeat it) and 111000
        let ends = |pat: &[bool]| n >= pat.len() - 1 && pat[..pat.len() - 1].iter().enumerate().all(|(i, &x)| self.bits.get(n - (pat.len() - 1) + i) == Some(&x)) && pat[pat.len() - 1] == b;
        if ends(&[true, true, false, false, true, true, false, false]) || ends(&[false, false, true, true, false, false, true, true]) || ends(&[true, true, true, false, false, false]) || ends(&[false, false, false, true, true, true]) {
            b = !b;
        }
        self.bits.push_back(b);
        if self.bits.len() > HISTORY {
            self.bits.pop_front();
        }
        b
    }

    /// An integer in `0..n`, filtered (with `n` < 3 the rules that cannot hold are left out).
    pub fn int(&mut self, rng: &mut Rng, n: u32) -> u32 {
        if n <= 1 {
            return 0;
        }
        let ok = |x: u32, h: &VecDeque<u32>| {
            let l = h.len();
            let last = h.back().copied();
            let not_repeat = last != Some(x);
            let not_xyx = n < 3 || l < 2 || h[l - 2] != x;
            let not_three = h.iter().rev().take(10).filter(|&&y| y == x).count() < 2 || n < 4;
            not_repeat && not_xyx && not_three
        };
        let mut x = (rng.one() * n as f32) as u32 % n;
        for _ in 0..8 {
            if ok(x, &self.ints) {
                break;
            }
            x = (rng.one() * n as f32) as u32 % n;
        }
        self.ints.push_back(x);
        if self.ints.len() > HISTORY {
            self.ints.pop_front();
        }
        x
    }
}

/// The trait of each bot of a side.
#[derive(Clone, Debug, Default)]
pub struct Traits {
    of: BTreeMap<usize, f32>,
}

fn forced(player: usize) -> Option<f32> {
    crate::debug::options().bots_streetwise.filter(|f| f.0 == player).map(|f| f.1)
}

impl Traits {
    /// The trait of `player` of side `team`: fixed the first time it is asked, one in [`ONE_IN`] high, in the order they ask.
    pub fn of(&mut self, team: u32, player: usize) -> f32 {
        if let Some(x) = forced(player) {
            return x;
        }
        let n = self.of.len();
        *self.of.entry(player).or_insert_with(|| {
            let mut rng = Rng::new(0x5743_0000 ^ ((team as u64) << 20) ^ player as u64);
            if n % ONE_IN == ONE_IN - 1 { rng.uniform(0.7, 1.0) } else { rng.uniform(0.0, 0.2) }
        })
    }

    /// How many have it high.
    pub fn high(&self) -> usize {
        self.of.values().filter(|&&x| x >= 0.5).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(v: &[bool], len: usize) -> usize {
        let mut out = 0;
        let mut k = 1;
        for i in 1..=v.len() {
            if i < v.len() && v[i] == v[i - 1] {
                k += 1;
            } else {
                if k >= len {
                    out += 1;
                }
                k = 1;
            }
        }
        out
    }

    #[test]
    fn the_filtered_coin_has_far_fewer_streaks_and_stays_fair() {
        let mut rng = Rng::new(7);
        let plain: Vec<bool> = (0..1000).map(|_| rng.si(0.5)).collect();
        let mut rng = Rng::new(7);
        let mut f = Filtered::default();
        let filtered: Vec<bool> = (0..1000).map(|_| f.coin(&mut rng)).collect();
        let (a, b) = (runs(&plain, 5), runs(&filtered, 5));
        assert!(b * 3 <= a.max(1), "runs of 5: {a} plain, {b} filtered");
        let share = filtered.iter().filter(|&&x| x).count() as f32 / 1000.0;
        assert!((share - 0.5).abs() <= 0.03, "{share}");
    }

    #[test]
    fn the_filtered_integer_does_not_repeat_nor_bounce() {
        let mut rng = Rng::new(3);
        let mut f = Filtered::default();
        let v: Vec<u32> = (0..500).map(|_| f.int(&mut rng, 8)).collect();
        assert!(v.windows(2).all(|w| w[0] != w[1]));
        assert!(v.windows(3).filter(|w| w[0] == w[2]).count() <= 5);
        assert!(v.iter().all(|&x| x < 8));
    }

    #[test]
    fn one_in_four_has_the_trait_high() {
        let mut t = Traits::default();
        let v: Vec<f32> = (0..16).map(|p| t.of(1, p)).collect();
        assert_eq!(t.high(), 4);
        assert!(v.iter().all(|&x| (0.0..=1.0).contains(&x)));
        // asked again, the same
        assert_eq!(t.of(1, 3), v[3]);
    }
}
