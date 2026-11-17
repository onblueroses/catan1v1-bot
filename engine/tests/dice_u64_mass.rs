//! Exact pre-change u128 reference. No production arithmetic helper is used.
use catan_engine::dice::{DiceProbabilities, DiceState, INITIAL_COUNTS};
use catan_engine::{Board, Config, Outcome, State, phase};

fn reference_factor(state: &DiceState, roller: u8) -> (u128, u128) {
    let own_streak = state.streak_owner == roller;
    if state.streak_length >= 5 {
        return (if own_streak { 0 } else { 2 }, 1);
    }
    let registered = state.initialized_player_mask.count_ones() as i128;
    let total = i128::from(state.sevens[0]) + i128::from(state.sevens[1]);
    let (base, denominator) = if total < registered {
        (5, 5)
    } else {
        (
            10 * total - 5 * registered * i128::from(state.sevens[usize::from(roller)]),
            5 * total,
        )
    };
    let streak = 2 * i128::from(state.streak_length) * (denominator / 5);
    let numerator = (base + if own_streak { -streak } else { streak }).clamp(0, 2 * denominator);
    (numerator as u128, denominator as u128)
}

fn reference(state: &DiceState, balanced: bool, roller: u8) -> DiceProbabilities {
    if !balanced {
        return DiceProbabilities {
            weights: INITIAL_COUNTS.map(u128::from),
            total: 36,
        };
    }
    let (a, d) = reference_factor(state, roller);
    let mut recent_counts = [0u8; 11];
    for &total in &state.recent[..usize::from(state.recent_len)] {
        recent_counts[usize::from(total - 2)] += 1;
    }
    let weights = std::array::from_fn(|i| {
        let base =
            u128::from(state.counts[i]) * u128::from(50u8.saturating_sub(17 * recent_counts[i]));
        base * if i == 5 { a } else { d }
    });
    DiceProbabilities {
        total: weights.iter().sum(),
        weights,
    }
}

fn compare(state: DiceState, roller: u8) {
    let before = state;
    for balanced in [false, true] {
        let expected = reference(&state, balanced, roller);
        let actual = state.probabilities(balanced, roller);
        assert_eq!(
            actual, expected,
            "balanced {balanced}, roller {roller}, {state:?}"
        );
        assert_eq!(actual.weights.iter().sum::<u128>(), actual.total);
    }
    assert_eq!(state, before);
}

#[test]
fn c2_u64_guard_boundary_matches_u128_for_every_raw_count_byte() {
    // Eleven raw u8 counts, recent factor <=50, and a<=2*d prove the guard.
    // Actual nontrivial denominators are multiples of five. Exercise the last
    // representable denominator below the guard and the first one above it.
    let factor = 11 * u128::from(u8::MAX) * 50 * 2;
    let guard = u128::from(u64::MAX) / factor;
    assert!(factor * guard <= u128::from(u64::MAX));
    assert!(factor * (guard + 1) > u128::from(u64::MAX));
    let boundary_sevens = (guard / 5) as u64;
    assert!(5 * u128::from(boundary_sevens) <= guard);
    assert!(5 * u128::from(boundary_sevens + 1) > guard);
    let mut saw_narrow = false;
    let mut saw_fallback = false;
    for count in 0..=u8::MAX {
        for total in [
            boundary_sevens - 1,
            boundary_sevens,
            boundary_sevens + 1,
            boundary_sevens + 2,
        ] {
            for sevens in [[total, 0], [0, total], [total / 2, total - total / 2]] {
                for recent in [[2, 3, 4, 5, 6], [6; 5], [7; 5]] {
                    let state = DiceState {
                        counts: [count; 11],
                        recent_len: 5,
                        recent,
                        initialized_player_mask: 3,
                        streak_owner: 0,
                        streak_length: 1,
                        sevens,
                    };
                    for roller in 0..2 {
                        let (a, d) = reference_factor(&state, roller);
                        assert!(a <= 2 * d);
                        saw_narrow |= d <= guard;
                        saw_fallback |= d > guard;
                        compare(state, roller);
                    }
                }
            }
        }
    }
    assert!(saw_narrow && saw_fallback);
}

#[test]
fn c2_u64_and_fallback_preserve_full_width_clamps_recent_zeroes_and_products() {
    let mut saw_large_denominator = false;
    let mut saw_lower_clamp = false;
    let mut saw_upper_clamp = false;
    for sevens in [
        [0, 0],
        [1, 0],
        [0, 1],
        [1, 1],
        [u64::MAX; 2],
        [u64::MAX, 0],
        [0, u64::MAX],
        [u64::MAX - 1, 1],
    ] {
        for owner in 0..2 {
            for streak in [0, 1, 4, 5, 6, u64::MAX] {
                for count in [0, 1, u8::MAX] {
                    for total in 2..=12 {
                        for occurrences in 0..=5 {
                            let filler = if total == 6 { 8 } else { 6 };
                            let mut state = DiceState {
                                counts: [count; 11],
                                recent_len: 5,
                                recent: [filler; 5],
                                initialized_player_mask: 3,
                                streak_owner: if streak == 0 { 255 } else { owner },
                                streak_length: streak,
                                sevens,
                            };
                            state.recent[..occurrences].fill(total);
                            // Exercise mixed count bytes as well as uniform extremes.
                            state.counts[(total - 2) as usize] = u8::MAX - count;
                            for roller in 0..2 {
                                let (a, d) = reference_factor(&state, roller);
                                saw_large_denominator |= d > u128::from(u64::MAX);
                                saw_lower_clamp |= a == 0;
                                saw_upper_clamp |= a == 2 * d;
                                compare(state, roller);
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(saw_large_denominator && saw_lower_clamp && saw_upper_clamp);
}

#[test]
fn c1_c2_worker_chance_enumeration_keeps_exact_support_and_mass_sums_both_dice() {
    for dice in 0..2 {
        for roller in 0..2 {
            let mut state = State::new(
                Board::default(),
                Config {
                    dice,
                    ..Config::default()
                },
            )
            .unwrap();
            state.phase = phase::DICE;
            state.setup_pairs_done = 4;
            state.turn_number = 1;
            state.turn_owner = roller;
            state.dice.prepare_roll(dice == 0, roller).unwrap();
            let mut snapshots = vec![state];
            if dice == 0 {
                for sevens in [[u64::MAX; 2], [u64::MAX, 0], [0, u64::MAX]] {
                    for streak in [1, 4, 5, u64::MAX] {
                        let owner = if sevens[0] > 0 { 0 } else { 1 };
                        let mut rare = state;
                        rare.dice = DiceState {
                            recent_len: 5,
                            recent: [6, 6, 6, 8, 8],
                            initialized_player_mask: 3,
                            sevens,
                            streak_owner: owner,
                            streak_length: streak,
                            ..DiceState::default()
                        };
                        snapshots.push(rare);
                    }
                }
            }
            for state in snapshots {
                state.validate().unwrap();
                let expected = reference(&state.dice, dice == 0, roller);
                let mut outcomes = [Outcome::default(); 11];
                let n = state.chance_outcomes(&mut outcomes);
                assert!(n > 0);
                assert_eq!(
                    n,
                    expected
                        .weights
                        .iter()
                        .filter(|&&weight| weight > 0)
                        .count()
                );
                let mut actual_weights = [0; 11];
                for outcome in &outcomes[..n] {
                    assert!(outcome.weight > 0);
                    assert_eq!(outcome.total, expected.total);
                    actual_weights[outcome.id as usize] = outcome.weight;
                }
                assert_eq!(actual_weights, expected.weights);
                assert_eq!(
                    outcomes[..n].iter().map(|o| o.weight).sum::<u128>(),
                    expected.total
                );
                assert!(outcomes[..n].windows(2).all(|o| o[0].id < o[1].id));
            }
        }
    }
}
