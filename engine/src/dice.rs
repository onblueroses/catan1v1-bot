//! Public, fixed-size dice history and exact chance masses (SPEC C1 to C3).
//!
//! The balanced model reproduces Colonist's published balanced-dice code
//! for two players. It is a deck of the 36 dice pairs, of which only the
//! totals matter here (C3). The probability of a total is its share of the
//! cards left, lowered for totals among the last five rolls, and for a seven
//! also scaled by a factor that evens out the sevens between the players.
//! ENCODING.md, "Balanced dice", gives the formulas. The engine computes them
//! as exact integer masses, so no float enters a transition.
//!
//! One roll is two steps: `prepare_roll` at the ROLL action, then
//! `apply_total` at the chance outcome. `probabilities` reads the state
//! between the two and changes nothing.

/// Cards per total 2..=12 in a fresh deck: the number of dice pairs with that sum.
pub const INITIAL_COUNTS: [u8; 11] = [1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

/// The dice history. It is public information and part of the canonical
/// bytes (C3). With independent dice it stays at its initial value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiceState {
    /// Cards left in the deck for each total, index `total - 2`.
    pub counts: [u8; 11],
    /// Number of valid entries in `recent`, at most 5.
    pub recent_len: u8,
    /// The last five totals, oldest first. Unused entries are zero.
    pub recent: [u8; 5],
    /// Bit `p` is set once player `p` has rolled. The source registers a
    /// player in its seven table at that player's first roll, and the
    /// imbalance term divides by the number of registered players.
    pub initialized_player_mask: u8,
    /// The player who rolled the latest seven, or 255 before the first seven.
    pub streak_owner: u8,
    /// Sevens rolled by `streak_owner` since the other player's last seven.
    /// A roll other than seven does not reset it.
    pub streak_length: u64,
    /// Sevens rolled by each player in the whole game.
    pub sevens: [u64; 2],
}

impl Default for DiceState {
    fn default() -> Self {
        Self {
            counts: INITIAL_COUNTS,
            recent_len: 0,
            recent: [0; 5],
            initialized_player_mask: 0,
            streak_owner: 255,
            streak_length: 0,
            sevens: [0; 2],
        }
    }
}

/// Outcome total `t` has probability `weights[t-2] / total`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiceProbabilities {
    pub weights: [u128; 11],
    pub total: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiceError {
    /// The roller is not player 0 or 1.
    InvalidPlayer,
    /// The total is outside 2..=12 or has zero mass in this state.
    InvalidOutcome,
    /// `apply_total` was called for a roller that `prepare_roll` has not registered.
    UnpreparedRoll,
    /// A u64 seven counter would overflow.
    CounterOverflow,
}

impl DiceState {
    /// Perform the source's registration and pre-draw reshuffle at ROLL.
    pub fn prepare_roll(&mut self, balanced: bool, roller: u8) -> Result<(), DiceError> {
        if roller > 1 {
            return Err(DiceError::InvalidPlayer);
        }
        if balanced {
            self.initialized_player_mask |= 1 << roller;
            if self.counts.iter().map(|&c| u16::from(c)).sum::<u16>() < 13 {
                self.counts = INITIAL_COUNTS;
            }
        }
        Ok(())
    }

    /// Pure enumeration. Balanced callers must first call `prepare_roll`.
    /// State fields must satisfy the canonical dice invariants.
    pub fn probabilities(&self, balanced: bool, roller: u8) -> DiceProbabilities {
        if !balanced {
            return DiceProbabilities {
                weights: INITIAL_COUNTS.map(u128::from),
                total: 36,
            };
        }
        assert!(
            roller < 2 && self.initialized_player_mask & (1 << roller) != 0,
            "Prepare the roller before enumerating balanced dice"
        );
        let (a, d) = self.seven_factor(roller);
        let mut recent_counts = [0u8; 11];
        for &t in &self.recent[..usize::from(self.recent_len)] {
            recent_counts[usize::from(t - 2)] += 1;
        }
        // Every raw count is at most u8::MAX and every recent factor is in
        // [0, 50]. seven_factor guarantees 0 <= a <= 2*d, including its early
        // streak clamp. Thus all eleven nonnegative weights together are at
        // most 11 * 255 * 50 * 2 * d = 280500*d. This bound covers even raw
        // noncanonical count bytes, not just the canonical 36-card deck.
        // The constant guard proves both each product and the sum fit u64.
        // Since 280500 >= 2, it also proves a <= 2*d <= u64::MAX for the casts.
        const MAX_SUM_FACTOR: u64 = 11 * u8::MAX as u64 * 50 * 2;
        const MAX_U64_DENOMINATOR: u128 = (u64::MAX / MAX_SUM_FACTOR) as u128;
        if d <= MAX_U64_DENOMINATOR {
            let (a, d) = (a as u64, d as u64);
            let mut total = 0u64;
            let weights = std::array::from_fn(|i| {
                let base = u64::from(self.counts[i])
                    * u64::from(50u8.saturating_sub(17 * recent_counts[i]));
                let weight = base * if i == 5 { a } else { d };
                total += weight;
                u128::from(weight)
            });
            return DiceProbabilities {
                weights,
                total: u128::from(total),
            };
        }
        let weights = std::array::from_fn(|i| {
            let base =
                u128::from(self.counts[i]) * u128::from(50u8.saturating_sub(17 * recent_counts[i]));
            base * if i == 5 { a } else { d }
        });
        DiceProbabilities {
            total: weights.iter().sum(),
            weights,
        }
    }

    /// Reference compatibility: integer hundredths, with a fixed player count.
    pub fn reference_probabilities(&self, roller: u8) -> DiceProbabilities {
        assert!(roller < 2);
        let sevens = u128::from(self.sevens[0]) + u128::from(self.sevens[1]);
        let imbalance = if sevens >= 2 {
            200 - 200 * u128::from(self.sevens[roller as usize]) / sevens
        } else {
            100
        } as i128;
        let streak =
            40 * i128::from(self.streak_length) * if self.streak_owner == roller { -1 } else { 1 };
        let factor = (imbalance + streak).clamp(0, 200) as u128;
        let counts = if self.counts.iter().map(|&c| u16::from(c)).sum::<u16>() < 13 {
            &INITIAL_COUNTS
        } else {
            &self.counts
        };
        let weights = std::array::from_fn(|i| {
            let repeats = self.recent[..self.recent_len as usize]
                .iter()
                .filter(|&&t| t == i as u8 + 2)
                .count() as u128;
            u128::from(counts[i])
                * 100u128.saturating_sub(34 * repeats)
                * if i == 5 { factor } else { 100 }
        });
        DiceProbabilities {
            total: weights.iter().sum(),
            weights,
        }
    }

    /// The factor on the weight of a seven, as an exact fraction `(a, d)`
    /// with `0 <= a <= 2 * d`. In the terms of ENCODING.md it is
    /// `clamp(imbalance + (2/5) * streak_length * sign, 0, 2)`. The imbalance
    /// is 1 until the sevens rolled reach the number of registered players,
    /// and `2 - registered * own_sevens / total_sevens` after that. The sign
    /// is negative when the roller owns the streak. The first branch below is
    /// a u64 fast path. The i128 code after it is the original formula, kept
    /// for counters too wide for that path.
    fn seven_factor(&self, roller: u8) -> (u128, u128) {
        let own_streak = self.streak_owner == roller;
        // Imbalance is in [0, 2]. This exact clamp also avoids multiplying
        // two full-width counters before clamping their result.
        if self.streak_length >= 5 {
            return (if own_streak { 0 } else { 2 }, 1);
        }
        let registered = u64::from(self.initialized_player_mask.count_ones());
        // With at most two registered players, own <= total proves
        // 0 <= base = 10*total - 5*registered*own <= 10*total.
        // The early clamp above gives length <= 4, hence streak <= 8*total.
        // Therefore total <= MAX/18 proves every product, base+streak and
        // 2*d fit u64. In the first-roll branch d=5 and streak<=8 instead.
        const MAX_U64_TOTAL: u64 = u64::MAX / (10 + 2 * 4);
        if registered <= 2
            && let Some(total) = self.sevens[0].checked_add(self.sevens[1])
            && total <= MAX_U64_TOTAL
        {
            let (base, d, unit) = if total < registered {
                (5, 5, 1)
            } else {
                (
                    10 * total - 5 * registered * self.sevens[usize::from(roller)],
                    5 * total,
                    total,
                )
            };
            // unit is exactly d/5. Saturation is the original lower clamp,
            // while the minimum below preserves the original upper clamp.
            let streak = 2 * self.streak_length * unit;
            let a = if own_streak {
                base.saturating_sub(streak)
            } else {
                base + streak
            };
            return (u128::from(a.min(2 * d)), u128::from(d));
        }
        let registered = self.initialized_player_mask.count_ones() as i128;
        let total = i128::from(self.sevens[0]) + i128::from(self.sevens[1]);
        let (base, d) = if total < registered {
            (5, 5)
        } else {
            (
                10 * total - 5 * registered * i128::from(self.sevens[usize::from(roller)]),
                5 * total,
            )
        };
        let streak = 2 * i128::from(self.streak_length) * (d / 5);
        let a = (base + if own_streak { -streak } else { streak }).clamp(0, 2 * d);
        (a as u128, d as u128)
    }

    /// Apply a positive-mass total. Errors leave the complete history unchanged.
    pub fn apply_total(&mut self, balanced: bool, roller: u8, total: u8) -> Result<(), DiceError> {
        self.apply_total_with_rounding(balanced, roller, total, false)
    }
    /// Apply using the same support as the selected probability model.
    pub fn apply_total_with_rounding(
        &mut self,
        balanced: bool,
        roller: u8,
        total: u8,
        reference: bool,
    ) -> Result<(), DiceError> {
        if roller > 1 {
            return Err(DiceError::InvalidPlayer);
        }
        if !(2..=12).contains(&total) {
            return Err(DiceError::InvalidOutcome);
        }
        if !balanced {
            return Ok(());
        }
        if self.initialized_player_mask & (1 << roller) == 0 {
            return Err(DiceError::UnpreparedRoll);
        }
        let index = usize::from(total - 2);
        // max(0, 50 - 17*r) is positive through two recent occurrences. For
        // a prepared roller the common denominator is positive, so only seven
        // additionally needs its adjustment numerator to be positive.
        if self.counts[index] == 0
            || self.recent[..usize::from(self.recent_len)]
                .iter()
                .filter(|&&t| t == total)
                .count()
                >= 3
            || (total == 7
                && if reference {
                    self.reference_probabilities(roller).weights[5] == 0
                } else {
                    self.seven_factor(roller).0 == 0
                })
        {
            return Err(DiceError::InvalidOutcome);
        }
        let mut next = *self;
        if total == 7 {
            next.sevens[usize::from(roller)] = next.sevens[usize::from(roller)]
                .checked_add(1)
                .ok_or(DiceError::CounterOverflow)?;
            next.streak_length = if next.streak_owner == roller {
                next.streak_length
                    .checked_add(1)
                    .ok_or(DiceError::CounterOverflow)?
            } else {
                1
            };
            next.streak_owner = roller;
        }
        next.counts[index] -= 1;
        if next.recent_len == 5 {
            next.recent.copy_within(1..5, 0);
            next.recent[4] = total;
        } else {
            next.recent[usize::from(next.recent_len)] = total;
            next.recent_len += 1;
        }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod seven_factor_tests {
    use super::*;

    // Independent copy of the original i128 formula. Do not call the new
    // narrow branch or use its guard to compute the expected result.
    fn original(state: &DiceState, roller: u8) -> (u128, u128) {
        let own_streak = state.streak_owner == roller;
        if state.streak_length >= 5 {
            return (if own_streak { 0 } else { 2 }, 1);
        }
        let registered = state.initialized_player_mask.count_ones() as i128;
        let total = i128::from(state.sevens[0]) + i128::from(state.sevens[1]);
        let (base, d) = if total < registered {
            (5, 5)
        } else {
            (
                10 * total - 5 * registered * i128::from(state.sevens[usize::from(roller)]),
                5 * total,
            )
        };
        let streak = 2 * i128::from(state.streak_length) * (d / 5);
        let a = (base + if own_streak { -streak } else { streak }).clamp(0, 2 * d);
        (a as u128, d as u128)
    }

    fn counters() -> Vec<[u64; 2]> {
        // 10*total plus 2*4*total is the largest pre-clamp sum in the
        // narrow path. Exercise its exact width boundary and checked-add.
        let guard = u64::MAX / 18;
        assert!(18 * u128::from(guard) <= u128::from(u64::MAX));
        assert!(18 * (u128::from(guard) + 1) > u128::from(u64::MAX));
        let mut pairs = vec![[u64::MAX; 2], [u64::MAX, 1], [1, u64::MAX]];
        for total in [0, 1, 2, 3, guard - 1, guard, guard + 1, u64::MAX] {
            pairs.extend([[0, total], [total, 0], [total / 2, total - total / 2]]);
        }
        pairs
    }

    #[test]
    fn c2_seven_factor_matches_original_at_every_mask_and_counter_width_boundary() {
        let mut saw_narrow = false;
        let mut saw_wide = false;
        let mut saw_sum_overflow = false;
        let mut saw_registered_fallback = false;
        let mut saw_lower_clamp = false;
        let mut saw_upper_clamp = false;
        for mask in 0..=u8::MAX {
            for sevens in counters() {
                for owner in [0, 1, 255] {
                    for length in [0, 1, 2, 3, 4, 5, 6, u64::MAX - 1, u64::MAX] {
                        let state = DiceState {
                            initialized_player_mask: mask,
                            sevens,
                            streak_owner: owner,
                            streak_length: length,
                            ..DiceState::default()
                        };
                        for roller in 0..2 {
                            let actual = state.seven_factor(roller);
                            assert_eq!(
                                actual,
                                original(&state, roller),
                                "{state:?}, roller {roller}"
                            );
                            if length < 5 {
                                let total = sevens[0].checked_add(sevens[1]);
                                let narrow = mask.count_ones() <= 2
                                    && total.is_some_and(|n| n <= u64::MAX / 18);
                                saw_narrow |= narrow;
                                saw_wide |= !narrow;
                                saw_sum_overflow |= total.is_none();
                                saw_registered_fallback |= mask.count_ones() > 2;
                                saw_lower_clamp |= actual.0 == 0 && actual.1 > 0;
                                saw_upper_clamp |= actual.0 == 2 * actual.1 && actual.1 > 0;
                            }
                        }
                    }
                }
            }
        }
        assert!(saw_narrow && saw_wide && saw_sum_overflow && saw_registered_fallback);
        assert!(saw_lower_clamp && saw_upper_clamp);
    }

    #[test]
    fn c2_seven_factor_keeps_exact_public_raw_masses_and_input_at_both_widths() {
        for sevens in counters() {
            for mask in [1, 2, 3, 7, u8::MAX] {
                for owner in [0, 1, 255] {
                    for length in [0, 1, 2, 3, 4, 5, u64::MAX] {
                        for count in [0, 1, u8::MAX] {
                            for recent in [[2, 3, 4, 5, 6], [6; 5], [7; 5]] {
                                let state = DiceState {
                                    counts: [count; 11],
                                    recent_len: 5,
                                    recent,
                                    initialized_player_mask: mask,
                                    sevens,
                                    streak_owner: owner,
                                    streak_length: length,
                                };
                                let before = state;
                                let mut occurrences = [0u8; 11];
                                for total in recent {
                                    occurrences[usize::from(total - 2)] += 1;
                                }
                                for roller in 0..2 {
                                    if mask & (1 << roller) == 0 {
                                        continue;
                                    }
                                    let (a, d) = original(&state, roller);
                                    let weights: [u128; 11] = std::array::from_fn(|i| {
                                        u128::from(count)
                                            * u128::from(50u8.saturating_sub(17 * occurrences[i]))
                                            * if i == 5 { a } else { d }
                                    });
                                    let expected = DiceProbabilities {
                                        weights,
                                        total: weights.iter().sum(),
                                    };
                                    assert_eq!(state.probabilities(true, roller), expected);
                                    assert_eq!(state, before);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
