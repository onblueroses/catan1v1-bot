//! Board data (B2, B3), the board generator and the caller-supplied random source.
use crate::{Config, Error, topology::HEX_NEIGHBORS};

/// Randomness is supplied by the caller, outside the rules core.
pub trait RandomSource {
    fn next_u64(&mut self) -> u64;
    /// A uniform integer in `0..bound`, exact by rejection: draws below
    /// `2^64 mod bound` are discarded, so no residue is favoured.
    fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0);
        if bound.is_power_of_two() {
            // For bound = 2^k, 0 <= k < 64, bound divides 2^64. Thus
            // (2^64 - bound) % bound is zero: the old sampler accepts its
            // first draw. Modulo equals masking, including bound one, which
            // must still consume that one draw to preserve the RNG stream.
            return self.next_u64() & (bound - 1);
        }
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let x = self.next_u64();
            if x >= threshold {
                return x % bound;
            }
        }
    }
    /// The same rejection method on 128 bits, from two draws. Chance sampling
    /// uses it because dice masses can exceed 64 bits.
    fn below_u128(&mut self, bound: u128) -> u128 {
        assert!(bound > 0);
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let x = ((self.next_u64() as u128) << 64) | self.next_u64() as u128;
            if x >= threshold {
                return x % bound;
            }
        }
    }
}
/// The SplitMix64 generator: small, fast and reproducible from a seed. It is
/// a convenience for runners and tests, and it is not cryptographic.
#[derive(Clone, Copy, Debug)]
pub struct SplitMix64 {
    pub state: u64,
}
impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}
impl RandomSource for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
}
/// The variable part of the board on the fixed topology (B1, B3). The engine
/// treats it as input data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Board {
    /// Resource of each hex: 0 wood, 1 brick, 2 sheep, 3 wheat, 4 ore, 5 desert.
    pub terrain: [u8; 19],
    /// Number token of each hex, 2..=12 without 7. The desert has 0.
    pub number: [u8; 19],
    /// Type of each port slot: a resource for a 2:1 port, or 5 for a 3:1 port.
    pub port_type: [u8; 9],
    /// Coastal edge ids, in the same order as port_type.
    pub port_edges: [u8; 9],
}
/// A fixed, valid board in topology order, used by examples, tests and the
/// worked fixtures of ENCODING.md. It ignores the generator options.
impl Default for Board {
    fn default() -> Self {
        Self {
            terrain: [0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5],
            number: [2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12, 0],
            port_type: [0, 1, 2, 3, 4, 5, 5, 5, 5],
            port_edges: Self::default_port_edges(),
        }
    }
}
impl Board {
    pub const fn default_port_edges() -> [u8; 9] {
        let mut edges = [0; 9];
        let mut i = 0;
        while i < 9 {
            let [a, b] = crate::topology::PORT_NODES[i];
            let mut e = 0;
            while e < 72 {
                let [x, y] = crate::topology::EDGE_NODES[e];
                if (a == x && b == y) || (a == y && b == x) {
                    edges[i] = e as u8;
                    break;
                }
                e += 1;
            }
            i += 1;
        }
        edges
    }
    pub fn port_nodes(&self) -> [[u8; 2]; 9] {
        self.port_edges
            .map(|edge| crate::topology::EDGE_NODES[edge as usize])
    }
    /// Set external port positions from coastal endpoint pairs.
    pub fn set_port_nodes(&mut self, nodes: [[u8; 2]; 9]) -> Result<(), Error> {
        let mut candidate = *self;
        for (i, [a, b]) in nodes.into_iter().enumerate() {
            candidate.port_edges[i] = crate::topology::EDGE_NODES
                .iter()
                .position(|&pair| pair == [a, b] || pair == [b, a])
                .ok_or(Error::InvalidBoard)? as u8;
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// B2: exactly 4 wood, 3 brick, 4 sheep, 4 wheat, 3 ore and 1 desert, the
    /// 18 standard tokens on the hexes other than the desert, and one 2:1
    /// port per resource plus four 3:1 ports. The generator options are not
    /// checked here (B3).
    pub fn validate(&self) -> Result<(), Error> {
        let mut resources = [0u8; 6];
        let mut numbers = [0u8; 13];
        let mut ports = [0u8; 6];
        for h in 0..19 {
            if self.terrain[h] > 5
                || self.number[h] > 12
                || ((self.terrain[h] == 5) != (self.number[h] == 0))
            {
                return Err(Error::InvalidBoard);
            }
            resources[self.terrain[h] as usize] += 1;
            numbers[self.number[h] as usize] += 1;
        }
        for &p in &self.port_type {
            if p > 5 {
                return Err(Error::InvalidBoard);
            }
            ports[p as usize] += 1;
        }
        if resources != [4, 3, 4, 4, 3, 1]
            || numbers != [1, 0, 1, 2, 2, 2, 2, 0, 2, 2, 2, 2, 1]
            || ports != [1, 1, 1, 1, 1, 4]
        {
            return Err(Error::InvalidBoard);
        }
        let mut used = 0u64;
        for &edge in &self.port_edges {
            if edge >= 72 {
                return Err(Error::InvalidBoard);
            }
            let [a, b] = crate::topology::EDGE_NODES[edge as usize];
            if a >= 54 || b >= 54 || a == b || used & ((1u64 << a) | (1u64 << b)) != 0 {
                return Err(Error::InvalidBoard);
            }
            let land = crate::topology::HEX_NODES
                .iter()
                .filter(|nodes| nodes.contains(&a) && nodes.contains(&b))
                .count();
            if land != 1 {
                return Err(Error::InvalidBoard);
            }
            used |= (1u64 << a) | (1u64 << b);
        }
        Ok(())
    }
    /// B3: a random board. Terrain and ports are shuffled once. The tokens
    /// are reshuffled until the placement meets the two options of `config`.
    /// The ranked map constraints are not published, so this is an assumption.
    pub fn generate(config: &Config, rng: &mut impl RandomSource) -> Self {
        fn shuffle(a: &mut [u8], r: &mut impl RandomSource) {
            for i in (1..a.len()).rev() {
                let j = r.below((i + 1) as u64) as usize;
                a.swap(i, j);
            }
        }
        let mut b = Self::default();
        shuffle(&mut b.terrain, rng);
        shuffle(&mut b.port_type, rng);
        let mut tokens = [2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12];
        loop {
            shuffle(&mut tokens, rng);
            let mut i = 0;
            for h in 0..19 {
                b.number[h] = if b.terrain[h] == 5 {
                    0
                } else {
                    let x = tokens[i];
                    i += 1;
                    x
                };
            }
            if (0..19).all(|h| {
                HEX_NEIGHBORS[h].iter().filter(|&&n| n != 255).all(|&n| {
                    let a = b.number[h];
                    let c = b.number[n as usize];
                    !(config.no_adjacent_6_8 && (a == 6 || a == 8) && (c == 6 || c == 8))
                        && !(config.no_adjacent_same_number && a == c)
                })
            }) {
                return b;
            }
        }
    }
}
