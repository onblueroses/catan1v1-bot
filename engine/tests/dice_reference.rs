//! Direct source-order port of spec/sources/colonist-balanced-dice.ts.
//! The oracle retains pair vectors, a player map, and incremental recent counts.
//! It does not use production helpers or the production integer-weight formula.
use catan_engine::dice::{DiceError, DiceState};
#[allow(dead_code)]
#[path = "support/dice_oracle.rs"]
mod dice_oracle;
use dice_oracle::*;

#[test]
fn c2_c3_source_port_seeded_distributions_and_outcomes() {
    let start = std::time::Instant::now();
    let mut seed = 0xc1a55_1c_d1ce_u64;
    let mut max_error = 0.0f64;
    let mut max_bound = 0.0f64;
    // 20,000 rolls per schedule: measured as a seconds-scale targeted check.
    // Schedules cover ordinary alternation, random players, and delayed registration.
    for schedule in 0..3 {
        let mut source = Source::new();
        let mut state = DiceState::default();
        for step in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let p = match schedule {
                0 => (step % 2) as u8,
                1 => (seed & 1) as u8,
                _ => {
                    if step < 24 {
                        1
                    } else {
                        (step % 2) as u8
                    }
                }
            };
            state.prepare_roll(true, p).unwrap();
            let reference = source.prepare(p);
            // The state-level property reconstructs this same source model from
            // each public DiceState. Check the adapter against all 60,000 rolls.
            let snapshot_reference = snapshot_distribution(&state, p);
            for (snapshot, reference) in snapshot_reference.iter().zip(&reference) {
                assert_eq!((snapshot.q.0, snapshot.q.1), (reference.q.0, reference.q.1));
                assert_eq!(snapshot.f, reference.f);
                assert_eq!((snapshot.lo, snapshot.hi), (reference.lo, reference.hi));
            }
            source.assert_state(&state);
            let before = state;
            let actual = state.probabilities(true, p);
            assert_eq!(state, before);
            assert_eq!(actual.weights.iter().sum::<u128>(), actual.total);
            assert!(actual.total > 0);
            for (i, r) in reference.iter().enumerate() {
                assert_eq!(
                    actual.weights[i] * r.q.1 as u128,
                    actual.total * r.q.0 as u128,
                    "Exact mass mismatch at schedule {schedule}, roll {step}, total {}",
                    i + 2
                );
                let f = actual.weights[i] as f64 / actual.total as f64;
                // Integer masses here are below 2^53. Only the final division
                // rounds, so expanding by one representable value encloses it.
                assert!(actual.total < (1u128 << 53));
                assert!(f >= r.lo.next_down() && f <= r.hi.next_up());
                max_error = max_error.max((f - r.f).abs());
                max_bound = max_bound.max((r.f - r.lo).abs().max((r.hi - r.f).abs()));
            }
            let quantile = ((seed >> 12) as f64 + 0.5) / ((1u64 << 52) as f64);
            let expected_total = source.outcome(quantile);
            let mut cumulative = 0.0;
            let chosen = actual
                .weights
                .iter()
                .position(|&w| {
                    cumulative += w as f64 / actual.total as f64;
                    quantile < cumulative
                })
                .unwrap() as u8
                + 2;
            assert_eq!(chosen, expected_total);
            state.apply_total(true, p, chosen).unwrap();
            source.apply(p, chosen);
            source.assert_state(&state);
        }
    }
    eprintln!(
        "60,000 source comparisons in {:?}; maximum binary64 absolute error {max_error:e}; maximum propagated rounding bound {max_bound:e}",
        start.elapsed()
    );
}

#[test]
fn c1_independent_is_fair_and_has_no_history() {
    let mut state = DiceState::default();
    for p in 0..2 {
        for total in 2..=12 {
            state.prepare_roll(false, p).unwrap();
            let mass = state.probabilities(false, p);
            assert_eq!(mass.weights, [1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1]);
            assert_eq!(mass.total, 36);
            state.apply_total(false, p, total).unwrap();
            assert_eq!(state, DiceState::default());
        }
    }
}

#[test]
fn c2_registration_recent_window_and_streak_boundaries() {
    let mut state = DiceState::default();
    state.prepare_roll(true, 1).unwrap();
    assert_eq!(state.initialized_player_mask, 2);
    state.apply_total(true, 1, 7).unwrap();
    state.prepare_roll(true, 1).unwrap();
    state.apply_total(true, 1, 6).unwrap();
    assert_eq!((state.streak_owner, state.streak_length), (1, 1));
    state.prepare_roll(true, 1).unwrap();
    state.apply_total(true, 1, 7).unwrap();
    assert_eq!(state.streak_length, 2);
    state.prepare_roll(true, 0).unwrap();
    assert_eq!(state.initialized_player_mask, 3);
    state.apply_total(true, 0, 7).unwrap();
    assert_eq!((state.streak_owner, state.streak_length), (0, 1));
    assert_eq!(state.probabilities(true, 0).weights[5], 0); // Three recent sevens.
    let before = state;
    assert_eq!(
        state.apply_total(true, 0, 7),
        Err(DiceError::InvalidOutcome)
    );
    assert_eq!(state, before);
    for total in [5, 6, 8, 9, 10] {
        state.prepare_roll(true, 0).unwrap();
        state.apply_total(true, 0, total).unwrap();
    }
    assert_eq!(state.recent, [5, 6, 8, 9, 10]);
    assert_eq!(state.streak_length, 1);
}

#[test]
fn c2_reshuffle_is_before_draw_and_preserves_history() {
    let mut state = DiceState::default();
    state.counts = [1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 1]; // 13 cards.
    state.prepare_roll(true, 0).unwrap();
    assert_eq!(state.counts.iter().sum::<u8>(), 13);
    state.apply_total(true, 0, 7).unwrap();
    assert_eq!(state.counts.iter().sum::<u8>(), 12);
    let history = state;
    state.prepare_roll(true, 1).unwrap();
    assert_eq!(state.counts, [1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1]);
    assert_eq!(state.recent, history.recent);
    assert_eq!(state.sevens, history.sevens);
    assert_eq!(state.streak_length, history.streak_length);
}

#[test]
fn c2_full_width_counters_and_atomic_errors() {
    let mut state = DiceState::default();
    state.prepare_roll(true, 0).unwrap();
    state.prepare_roll(true, 1).unwrap();
    state.sevens = [u64::MAX, u64::MAX];
    state.streak_owner = 1;
    state.streak_length = u64::MAX;
    let own = state.probabilities(true, 1);
    assert_eq!(own.weights[5], 0);
    let other = state.probabilities(true, 0);
    assert_eq!(other.weights[5], 600);
    let before = state;
    assert_eq!(
        state.apply_total(true, 0, 7),
        Err(DiceError::CounterOverflow)
    );
    assert_eq!(state, before);
    state.streak_length = 1;
    let mass = state.probabilities(true, 0);
    // Equal totals give imbalance 1, opposite streak adds 2/5.
    assert_eq!(mass.weights[5] * 5, mass.weights[0] * 42);
    let before = state;
    assert_eq!(state.apply_total(true, 2, 7), Err(DiceError::InvalidPlayer));
    assert_eq!(
        state.apply_total(true, 0, 1),
        Err(DiceError::InvalidOutcome)
    );
    assert_eq!(state, before);
}
