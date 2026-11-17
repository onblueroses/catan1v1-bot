//! Check selected-outcome acceptance against the unchanged full distribution.
use catan_engine::dice::{DiceError, DiceState, INITIAL_COUNTS};
use catan_engine::{Board, Config, State, phase};

fn check_balanced(dice: DiceState, roller: u8) {
    // These are validated snapshots. Reachability is checked separately below.
    let mut state = State::new(Board::default(), Config::default()).unwrap();
    state.phase = phase::DICE;
    state.setup_pairs_done = 4;
    state.turn_number = 1;
    state.turn_owner = roller;
    state.dice = dice;
    state.validate().unwrap();

    for (index, weight) in dice
        .probabilities(true, roller)
        .weights
        .into_iter()
        .enumerate()
    {
        let total = index as u8 + 2;
        let expected = if weight == 0 {
            Err(DiceError::InvalidOutcome)
        } else if total == 7
            && (dice.sevens[roller as usize] == u64::MAX
                || (dice.streak_owner == roller && dice.streak_length == u64::MAX))
        {
            Err(DiceError::CounterOverflow)
        } else {
            Ok(())
        };
        let mut next = dice;
        assert_eq!(
            next.apply_total(true, roller, total),
            expected,
            "roller {roller}, total {total}, dice {dice:?}"
        );
        if expected.is_err() {
            assert_eq!(next, dice, "rejected outcomes must preserve the history");
        }
    }
}

#[test]
fn c2_selected_acceptance_matches_full_weights_across_count_and_recent_boundaries() {
    for total in 2..=12 {
        for depleted in [false, true] {
            for occurrences in 0..=5 {
                let filler = if total == 6 { 8 } else { 6 };
                let mut dice = DiceState {
                    recent_len: 5,
                    recent: [filler; 5],
                    initialized_player_mask: 3,
                    sevens: [5, 5],
                    streak_owner: 1,
                    streak_length: 1,
                    ..DiceState::default()
                };
                dice.recent[..occurrences].fill(total);
                if depleted {
                    dice.counts[(total - 2) as usize] = 0;
                }
                for roller in 0..2 {
                    check_balanced(dice, roller);
                }
            }
        }
    }
}

#[test]
fn c2_selected_acceptance_matches_full_weights_through_reachable_reshuffle() {
    let mut dice = DiceState::default();
    // A deck has 36 cards and reshuffles before drawing with fewer than 13.
    // One initial deck's worth of draws therefore crosses that boundary.
    for step in 0..INITIAL_COUNTS.iter().map(|&n| usize::from(n)).sum() {
        let roller = [1, 1, 0][step % 3];
        dice.prepare_roll(true, roller).unwrap();
        check_balanced(dice, roller);
        let weights = dice.probabilities(true, roller).weights;
        let selected = (0..weights.len())
            .map(|offset| (step + offset) % weights.len())
            .find(|&index| weights[index] > 0)
            .unwrap();
        dice.apply_total(true, roller, selected as u8 + 2).unwrap();
    }
    let mut dice = DiceState::default();
    for total in [6, 6, 6] {
        dice.prepare_roll(true, 0).unwrap();
        check_balanced(dice, 0);
        dice.apply_total(true, 0, total).unwrap();
    }
    dice.prepare_roll(true, 0).unwrap();
    assert_eq!(dice.probabilities(true, 0).weights[4], 0);
    check_balanced(dice, 0);
}

#[test]
fn c2_selected_acceptance_preserves_full_width_clamps_and_overflow_precedence() {
    for (sevens, owner, streak) in [
        ([u64::MAX, u64::MAX], 1, 1),
        ([u64::MAX, u64::MAX], 1, 4),
        ([u64::MAX, u64::MAX], 1, 5),
        ([u64::MAX, u64::MAX], 1, u64::MAX),
        ([u64::MAX, 0], 0, 1),
        ([u64::MAX, 0], 0, u64::MAX),
        ([u64::MAX - 1, 1], 0, 4),
    ] {
        let dice = DiceState {
            recent_len: 5,
            recent: [2, 3, 4, 5, 6],
            initialized_player_mask: 3,
            sevens,
            streak_owner: owner,
            streak_length: streak,
            ..DiceState::default()
        };
        for roller in 0..2 {
            check_balanced(dice, roller);
        }
    }
}

#[test]
fn c1_c2_selected_acceptance_preserves_independent_and_argument_errors() {
    for roller in 0..2 {
        for total in 2..=12 {
            let mut independent = DiceState::default();
            assert_eq!(independent.apply_total(false, roller, total), Ok(()));
            assert_eq!(independent, DiceState::default());
            let mut unprepared = DiceState::default();
            assert_eq!(
                unprepared.apply_total(true, roller, total),
                Err(DiceError::UnpreparedRoll)
            );
            assert_eq!(unprepared, DiceState::default());
        }
    }
    for balanced in [false, true] {
        for (roller, total, error) in [
            (2, 1, DiceError::InvalidPlayer),
            (2, 7, DiceError::InvalidPlayer),
            (0, 0, DiceError::InvalidOutcome),
            (0, 1, DiceError::InvalidOutcome),
            (0, 13, DiceError::InvalidOutcome),
            (0, u8::MAX, DiceError::InvalidOutcome),
        ] {
            let mut dice = DiceState::default();
            assert_eq!(dice.apply_total(balanced, roller, total), Err(error));
            assert_eq!(dice, DiceState::default());
        }
    }
}
