//! Direct source-order port shared by the dice-reference and state-level tests.
//! It retains independent pair vectors, a player map and recent counts.
use catan_engine::dice::DiceState;
use std::collections::{BTreeMap, VecDeque};
use std::ops::{Add, Div, Mul, Sub};

#[derive(Clone, Copy, Debug)]
pub(super) struct Rational(pub(super) i128, pub(super) i128);
impl Rational {
    pub(super) fn new(n: i128, d: i128) -> Self {
        assert!(d > 0);
        let (mut a, mut b) = (n.abs(), d);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Self(n / a, d / a)
    }
}

/// Run source arithmetic in binary64 and exact fractions together. Each floating
/// operation also propagates an outward-rounded interval. The interval, not a
/// chosen epsilon, bounds its roundoff, including cancellation and normalization.
#[derive(Clone, Copy, Debug)]
pub(super) struct Number {
    pub(super) q: Rational,
    pub(super) f: f64,
    pub(super) lo: f64,
    pub(super) hi: f64,
}
impl Number {
    fn integer(n: usize) -> Self {
        let f = n as f64;
        assert!(n <= (1usize << 53));
        Self {
            q: Rational(n as i128, 1),
            f,
            lo: f,
            hi: f,
        }
    }
    fn decimal(n: usize, d: usize) -> Self {
        Self::integer(n) / Self::integer(d)
    }
    fn clamp(self, low: f64, high: f64) -> Self {
        let q = if self.q.0 < low as i128 * self.q.1 {
            Rational(low as i128, 1)
        } else if high.is_finite() && self.q.0 > high as i128 * self.q.1 {
            Rational(high as i128, 1)
        } else {
            self.q
        };
        Self {
            q,
            f: self.f.clamp(low, high),
            lo: self.lo.clamp(low, high),
            hi: self.hi.clamp(low, high),
        }
    }
}
impl Add for Number {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self {
            q: Rational::new(self.q.0 * b.q.1 + b.q.0 * self.q.1, self.q.1 * b.q.1),
            f: self.f + b.f,
            lo: (self.lo + b.lo).next_down(),
            hi: (self.hi + b.hi).next_up(),
        }
    }
}
impl Sub for Number {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self {
            q: Rational::new(self.q.0 * b.q.1 - b.q.0 * self.q.1, self.q.1 * b.q.1),
            f: self.f - b.f,
            lo: (self.lo - b.hi).next_down(),
            hi: (self.hi - b.lo).next_up(),
        }
    }
}
impl Mul for Number {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        let bounds = [
            self.lo * b.lo,
            self.lo * b.hi,
            self.hi * b.lo,
            self.hi * b.hi,
        ];
        Self {
            q: Rational::new(self.q.0 * b.q.0, self.q.1 * b.q.1),
            f: self.f * b.f,
            lo: bounds.into_iter().fold(f64::INFINITY, f64::min).next_down(),
            hi: bounds
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        }
    }
}
impl Div for Number {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        assert!(b.lo > 0.0);
        let bounds = [
            self.lo / b.lo,
            self.lo / b.hi,
            self.hi / b.lo,
            self.hi / b.hi,
        ];
        Self {
            q: Rational::new(self.q.0 * b.q.1, self.q.1 * b.q.0),
            f: self.f / b.f,
            lo: bounds.into_iter().fold(f64::INFINITY, f64::min).next_down(),
            hi: bounds
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        }
    }
}

struct Deck {
    total: u8,
    pairs: Vec<(u8, u8)>,
    weight: Number,
    recent_count: usize,
}
pub(super) struct Source {
    decks: Vec<Deck>,
    cards_left: usize,
    recent: VecDeque<u8>,
    sevens: BTreeMap<u8, usize>,
    streak_player: Option<u8>,
    streak_count: usize,
}
impl Source {
    pub(super) fn new() -> Self {
        let mut s = Self {
            decks: (2..=12)
                .map(|total| Deck {
                    total,
                    pairs: Vec::new(),
                    weight: Number::integer(0),
                    recent_count: 0,
                })
                .collect(),
            cards_left: 0,
            recent: VecDeque::new(),
            sevens: BTreeMap::new(),
            streak_player: None,
            streak_count: 0,
        };
        s.reshuffle();
        s
    }
    fn reshuffle(&mut self) {
        for deck in &mut self.decks {
            deck.pairs.clear();
            for a in 1..=6 {
                for b in 1..=6 {
                    if a + b == deck.total {
                        deck.pairs.push((a, b));
                    }
                }
            }
        }
        self.cards_left = 36;
    }
    pub(super) fn prepare(&mut self, player: u8) -> [Number; 11] {
        self.sevens.entry(player).or_insert(0); // throwDice → initTotalSevens
        if self.cards_left < 13 {
            self.reshuffle();
        }
        for deck in &mut self.decks {
            deck.weight = Number::integer(deck.pairs.len()) / Number::integer(self.cards_left);
        }
        for deck in &mut self.decks {
            let reduction = Number::integer(deck.recent_count) * Number::decimal(34, 100);
            deck.weight =
                (deck.weight * (Number::integer(1) - reduction)).clamp(0.0, f64::INFINITY);
        }
        let direction = if self.streak_player == Some(player) {
            Number::integer(0) - Number::integer(1)
        } else {
            Number::integer(1)
        };
        let streak = Number::decimal(4, 10) * Number::integer(self.streak_count) * direction;
        let total_sevens: usize = self.sevens.values().sum();
        let imbalance = if total_sevens < self.sevens.len() {
            Number::integer(1)
        } else {
            let percentage = Number::integer(self.sevens[&player]) / Number::integer(total_sevens);
            let ideal = Number::integer(1) / Number::integer(self.sevens.len());
            Number::integer(1) + ((ideal - percentage) / ideal)
        };
        let adjustment = (Number::integer(1) * imbalance + streak).clamp(0.0, 2.0);
        self.decks[5].weight = self.decks[5].weight * adjustment;
        let sum = self
            .decks
            .iter()
            .fold(Number::integer(0), |a, d| a + d.weight);
        std::array::from_fn(|i| self.decks[i].weight / sum)
    }
    pub(super) fn outcome(&self, quantile: f64) -> u8 {
        let sum = self.decks.iter().map(|d| d.weight.f).sum::<f64>();
        let mut target = quantile * sum;
        for deck in &self.decks {
            if target <= deck.weight.f {
                return deck.total;
            }
            target -= deck.weight.f;
        }
        panic!("Source failed to select an outcome")
    }
    pub(super) fn apply(&mut self, player: u8, total: u8) {
        let deck = &mut self.decks[usize::from(total - 2)];
        deck.pairs.pop().unwrap(); // Pair identity has no effect on future totals.
        self.recent.push_back(total);
        deck.recent_count += 1;
        self.cards_left -= 1;
        if self.recent.len() > 5 {
            let old = self.recent.pop_front().unwrap();
            self.decks[usize::from(old - 2)].recent_count -= 1;
        }
        if total == 7 {
            *self.sevens.entry(player).or_insert(0) += 1;
            if self.streak_player == Some(player) {
                self.streak_count += 1;
            } else {
                self.streak_player = Some(player);
                self.streak_count = 1;
            }
        }
    }
    pub(super) fn assert_state(&self, state: &DiceState) {
        assert_eq!(
            state.counts,
            self.decks
                .iter()
                .map(|d| d.pairs.len() as u8)
                .collect::<Vec<_>>()
                .as_slice()
        );
        assert_eq!(
            &state.recent[..usize::from(state.recent_len)],
            self.recent.iter().copied().collect::<Vec<_>>()
        );
        assert!(
            state.recent[usize::from(state.recent_len)..]
                .iter()
                .all(|&x| x == 0)
        );
        assert_eq!(state.streak_owner, self.streak_player.unwrap_or(255));
        assert_eq!(state.streak_length, self.streak_count as u64);
        assert_eq!(
            state.initialized_player_mask,
            self.sevens.keys().fold(0, |m, p| m | (1 << p))
        );
        for p in 0..2 {
            assert_eq!(
                state.sevens[p],
                *self.sevens.get(&(p as u8)).unwrap_or(&0) as u64
            );
        }
    }
}

/// Rebuild the source representation from a public snapshot. No production
/// probability or outcome helper participates in this reconstruction.
pub(super) fn snapshot_distribution(state: &DiceState, player: u8) -> [Number; 11] {
    let mut source = Source::new();
    for (deck, &count) in source.decks.iter_mut().zip(&state.counts) {
        deck.pairs.truncate(usize::from(count));
        assert_eq!(deck.pairs.len(), usize::from(count));
    }
    source.cards_left = source.decks.iter().map(|deck| deck.pairs.len()).sum();
    source.recent = state.recent[..usize::from(state.recent_len)]
        .iter()
        .copied()
        .collect();
    for &total in &source.recent {
        source.decks[usize::from(total - 2)].recent_count += 1;
    }
    for p in 0..2 {
        if state.initialized_player_mask & (1 << p) != 0 {
            source
                .sevens
                .insert(p, usize::try_from(state.sevens[usize::from(p)]).unwrap());
        }
    }
    source.streak_player = (state.streak_owner != 255).then_some(state.streak_owner);
    source.streak_count = usize::try_from(state.streak_length).unwrap();
    source.prepare(player)
}
