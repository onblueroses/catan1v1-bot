//! xoshiro256** seeded by SplitMix64 (BOT-SPEC 8.2).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    pub s: [u64; 4],
}

impl Rng {
    pub fn seed(mut x: u64) -> Self {
        let mut s = [0u64; 4];
        for w in &mut s {
            x = x.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            *w = z ^ (z >> 31);
        }
        Rng { s }
    }

    #[inline]
    pub fn next(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Lemire's bounded integer on 32 bits; `1 <= n < 2^32`.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n >= 1 && n < (1u64 << 32));
        let mut m = (self.next() >> 32) * n;
        if (m & 0xffff_ffff) < n {
            let t = ((1u64 << 32) - n) % n;
            while (m & 0xffff_ffff) < t {
                m = (self.next() >> 32) * n;
            }
        }
        m >> 32
    }

    pub fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Chance sampling: index of the outcome picked by integer weight.
    pub fn pick_weighted(&mut self, weights: &[u128]) -> usize {
        let total: u128 = weights.iter().sum();
        let mut pick = self.below(total as u64) as u128;
        for (i, &w) in weights.iter().enumerate() {
            if pick < w {
                return i;
            }
            pick -= w;
        }
        unreachable!("pick below total")
    }
}
