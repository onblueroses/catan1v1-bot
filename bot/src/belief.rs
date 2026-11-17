//! The belief about the rival's held development cards (BOT-SPEC 8.3).

use crate::rng::Rng;

pub const THETA: [f64; 5] = [0.0, 1.9643513439850364, 0.007863944175760883, -0.320691297631005, 0.12540603456233995];
const POOL_MAX: [u8; 5] = [14, 5, 2, 2, 2];

pub struct Belief {
    held: usize,
    limit: [usize; 5],
    /// f[k][j], -inf beyond the limit.
    f: [[f64; 26]; 5],
    /// suffix[k][left], k = 0..=5.
    suffix: [[f64; 26]; 6],
}

impl Belief {
    /// Build the law, or None when it is undefined.
    pub fn new(pool: [u8; 5], held: u8, cap: i32) -> Option<Belief> {
        let total: u32 = pool.iter().map(|&x| x as u32).sum();
        if cap < 0 || total > 25 || held as u32 > total || (0..5).any(|k| pool[k] > POOL_MAX[k]) {
            return None;
        }
        let held = held as usize;
        let shift = THETA.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mut limit = [0usize; 5];
        for k in 0..5 {
            limit[k] = (pool[k] as usize).min(held);
            if k == 1 {
                limit[k] = limit[k].min(cap as usize);
            }
        }
        let mut f = [[f64::NEG_INFINITY; 26]; 5];
        for k in 0..5 {
            let mut lc = 0.0f64;
            for j in 0..=limit[k] {
                if j >= 1 {
                    lc += (pool[k] as f64 - j as f64 + 1.0).ln() - (j as f64).ln();
                }
                f[k][j] = lc + (THETA[k] - shift) * j as f64;
            }
        }
        let mut suffix = [[f64::NEG_INFINITY; 26]; 6];
        suffix[5][0] = 0.0;
        for k in (0..5).rev() {
            for left in 0..=held {
                let m = limit[k].min(left);
                let mut big = f64::NEG_INFINITY;
                for j in 0..=m {
                    big = big.max(f[k][j] + suffix[k + 1][left - j]);
                }
                if !big.is_finite() {
                    continue;
                }
                let mut s = 0.0f64;
                for j in 0..=m {
                    s += (f[k][j] + suffix[k + 1][left - j] - big).exp();
                }
                suffix[k][left] = big + s.ln();
            }
        }
        if !suffix[0][held].is_finite() {
            return None;
        }
        Some(Belief { held, limit, f, suffix })
    }

    /// One sample with seed s, or None if sampling fails.
    pub fn sample(&self, s: u64) -> Option<[u8; 5]> {
        let mut rng = Rng::seed(s ^ 0xa49b8132f093ca61);
        let mut left = self.held;
        let mut c = [0u8; 5];
        for k in 0..5 {
            let u = rng.uniform();
            let mut acc = 0.0f64;
            let mut last_valid: Option<usize> = None;
            let mut taken: Option<usize> = None;
            for j in 0..=self.limit[k].min(left) {
                if !self.suffix[k + 1][left - j].is_finite() {
                    continue;
                }
                last_valid = Some(j);
                acc += (self.f[k][j] + self.suffix[k + 1][left - j] - self.suffix[k][left]).exp();
                if u < acc {
                    taken = Some(j);
                    break;
                }
            }
            let j = taken.or(last_valid)?;
            c[k] = j as u8;
            left -= j;
        }
        if left == 0 { Some(c) } else { None }
    }
}
