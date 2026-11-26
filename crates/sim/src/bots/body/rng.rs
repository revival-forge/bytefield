//! Each bot's own seeded random generator, so the same state gives the same axes.

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    /// splitmix64
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn one(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn uniform(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.one()
    }

    pub fn si(&mut self, p: f32) -> bool {
        self.one() < p
    }

    /// Standard normal (Box–Muller).
    pub fn normal(&mut self) -> f32 {
        let (u, v) = (self.one().max(1e-6), self.one());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }

    pub fn exponential(&mut self, mean: f32) -> f32 {
        -mean * (1.0 - self.one()).max(1e-6).ln()
    }

    /// Lognormal with that median and sigma.
    pub fn lognormal(&mut self, median: f32, sigma: f32) -> f32 {
        median * (sigma * self.normal()).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same() {
        let (mut a, mut b) = (Rng::new(9), Rng::new(9));
        for _ in 0..100 {
            assert_eq!(a.one().to_bits(), b.one().to_bits());
        }
        assert!((0..1000).all(|_| (0.0..1.0).contains(&a.one())));
    }
}
