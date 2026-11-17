//! State-level expectation derived without the production outcome enumerator.
//! Dice use the supplied source's arithmetic and independent exact fractions.
use catan_engine::{Outcome, State, phase};
#[allow(dead_code)]
#[path = "dice_oracle.rs"]
mod dice_oracle;

fn reduced(mut numerator: u128, mut denominator: u128) -> (u128, u128) {
    assert!(denominator > 0);
    let (mut a, mut b) = (numerator, denominator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    numerator /= a;
    denominator /= a;
    (numerator, denominator)
}

/// Check support and exact probability at every supplied chance node. For card
/// draws and theft, also require the API's literal per-card counts and total.
/// Non-chance phases must have no outcomes, even if enumeration omits all mass.
pub fn assert_outcomes(state: &State, outcomes: &[Outcome]) {
    match state.phase {
        phase::DICE => {
            let expected: [(u128, u128); 11] = if state.config.dice == 0 {
                dice_oracle::snapshot_distribution(&state.dice, state.turn_owner).map(|mass| {
                    assert!(mass.q.0 >= 0 && mass.q.1 > 0);
                    (mass.q.0 as u128, mass.q.1 as u128)
                })
            } else {
                // Count the 36 ordered faces directly, independent of engine
                // tables and the balanced-dice integer-weight formula.
                let mut counts = [0; 11];
                for first in 1..=6 {
                    for second in 1..=6 {
                        counts[first + second - 2] += 1;
                    }
                }
                counts.map(|count| reduced(count, 36))
            };
            let expected: Vec<_> = expected
                .into_iter()
                .enumerate()
                .filter(|(_, (numerator, _))| *numerator > 0)
                .collect();
            assert_eq!(outcomes.len(), expected.len(), "independent dice support");
            for (actual, (id, mass)) in outcomes.iter().zip(expected) {
                assert_eq!(actual.id, id as u16, "independent dice outcome ID");
                assert!(actual.weight > 0 && actual.total > 0);
                // Reduce before comparing instead of cross-multiplying. This
                // preserves exactness without multiplying two u128 masses.
                assert_eq!(
                    reduced(actual.weight, actual.total),
                    mass,
                    "independent dice probability for outcome {id}"
                );
            }
        }
        phase::STEAL | phase::DEVELOPMENT_DRAW => {
            let (counts, first_id) = if state.phase == phase::STEAL {
                (&state.resources[1 - usize::from(state.turn_owner)], 11)
            } else {
                (&state.development_deck, 16)
            };
            let total: u128 = counts.iter().map(|&count| u128::from(count)).sum();
            let expected: Vec<_> = counts
                .iter()
                .enumerate()
                .filter(|(_, count)| **count > 0)
                .map(|(kind, &count)| Outcome {
                    id: first_id + kind as u16,
                    weight: u128::from(count),
                    total,
                })
                .collect();
            assert!(!expected.is_empty(), "valid chance node has positive mass");
            assert_eq!(outcomes, expected, "independent per-card chance masses");
        }
        _ => assert!(outcomes.is_empty(), "non-chance phase has no outcomes"),
    }
}
