//! Exact power-of-two specialization of the existing u64 rejection sampler.
//! If b = 2^k, k in 0..64, b divides 2^64. Its rejection threshold is therefore
//! (2^64 - b) mod b = 0, and x mod b = x & (b - 1). Every first draw is accepted.
//! Bound one still draws once. Other bounds and the u128 path are unchanged.
use catan_engine::{Board, Config, RandomSource, SplitMix64, State, runner};

// Frozen pre-specialization algorithm, independent of RandomSource::below.
fn old_below(rng: &mut impl RandomSource, bound: u64) -> u64 {
    assert!(bound > 0);
    let threshold = bound.wrapping_neg() % bound;
    loop {
        let x = rng.next_u64();
        if x >= threshold {
            return x % bound;
        }
    }
}

struct OldBelow<R>(R);
impl<R: RandomSource> RandomSource for OldBelow<R> {
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
    fn below(&mut self, bound: u64) -> u64 {
        old_below(&mut self.0, bound)
    }
}

struct Scripted<'a> {
    raw: &'a [u64],
    consumed: usize,
}
impl RandomSource for Scripted<'_> {
    fn next_u64(&mut self) -> u64 {
        let x = self.raw[self.consumed];
        self.consumed += 1;
        x
    }
}

struct Recorded {
    rng: SplitMix64,
    raw: Vec<u64>,
}
impl Recorded {
    fn new(seed: u64) -> Self {
        Self {
            rng: SplitMix64::new(seed),
            raw: Vec::new(),
        }
    }
}
impl RandomSource for Recorded {
    fn next_u64(&mut self) -> u64 {
        let x = self.rng.next_u64();
        self.raw.push(x);
        x
    }
}

fn bounds() -> Vec<u64> {
    let mut bounds: Vec<_> = (1..=catan_engine::ACTION_COUNT as u64).collect();
    for bit in 0..u64::BITS {
        let power = 1u64 << bit;
        bounds.push(power);
        if power > 1 {
            bounds.push(power - 1);
        }
        bounds.push(power + 1);
    }
    bounds.extend([u64::MAX - 1, u64::MAX]);
    bounds.sort_unstable();
    bounds.dedup();
    bounds
}

#[test]
fn all_64_powers_preserve_the_first_draw_and_exact_mapping() {
    let raw: Vec<_> = [0, u64::MAX]
        .into_iter()
        .chain((0..u64::BITS).map(|bit| 1u64 << bit))
        .collect();
    for bit in 0..u64::BITS {
        let bound = 1u64 << bit;
        assert_eq!(bound.wrapping_neg() % bound, 0);
        for &x in raw.iter().chain([bound - 1, bound, bound + 1].iter()) {
            let input = [x];
            let mut candidate = Scripted {
                raw: &input,
                consumed: 0,
            };
            let mut reference = Scripted {
                raw: &input,
                consumed: 0,
            };
            assert_eq!(candidate.below(bound), old_below(&mut reference, bound));
            assert_eq!(candidate.consumed, 1, "bound={bound}, raw={x}");
            assert_eq!(reference.consumed, 1);
        }
    }
}

#[test]
fn adjacent_extreme_and_action_bounds_preserve_rejections_and_acceptance() {
    assert_eq!(catan_engine::ACTION_COUNT, 226);
    for bound in bounds().into_iter().filter(|b| !b.is_power_of_two()) {
        let threshold = bound.wrapping_neg() % bound;
        assert!(threshold > 0);
        let input = [0, threshold - 1, threshold, u64::MAX];
        let mut candidate = Scripted {
            raw: &input,
            consumed: 0,
        };
        let mut reference = Scripted {
            raw: &input,
            consumed: 0,
        };
        assert_eq!(candidate.below(bound), old_below(&mut reference, bound));
        assert_eq!(candidate.consumed, 3, "bound={bound}");
        assert_eq!(reference.consumed, 3);
        assert_eq!(candidate.below(bound), old_below(&mut reference, bound));
        assert_eq!(candidate.consumed, 4, "bound={bound}");
        assert_eq!(reference.consumed, 4);
    }
}

#[test]
fn zero_bound_panics_before_any_random_draw() {
    let mut rng = Scripted {
        raw: &[u64::MAX],
        consumed: 0,
    };
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rng.below(0)));
    assert!(panic.is_err());
    assert_eq!(rng.consumed, 0);
}

#[test]
fn mixed_bound_sequence_preserves_every_raw_draw_and_rng_position() {
    let bounds = bounds();
    for seed in [0, 41, 191, u64::MAX] {
        let mut candidate = Recorded::new(seed);
        let mut reference = OldBelow(Recorded::new(seed));
        for &bound in bounds.iter().chain(bounds.iter().rev()) {
            assert_eq!(
                candidate.below(bound),
                reference.below(bound),
                "seed={seed}, bound={bound}"
            );
            assert_eq!(candidate.rng.state, reference.0.rng.state);
        }
        assert_eq!(candidate.raw, reference.0.raw);
        assert_eq!(candidate.next_u64(), reference.next_u64());
    }
}

#[test]
fn board_shuffles_preserve_canonical_state_and_complete_rng_sequence() {
    for no_adjacent_6_8 in [false, true] {
        for no_adjacent_same_number in [false, true] {
            for dice in [0, 1] {
                let config = Config {
                    dice,
                    no_adjacent_6_8,
                    no_adjacent_same_number,
                    ..Config::default()
                };
                for seed in [0, 41, 191, u64::MAX] {
                    // Include the held-out half's first index from the existing
                    // 4096-game validation cohort, as well as the original zero.
                    for index in [0, 2048] {
                        let seed = runner::game_seed(seed, index);
                        let mut candidate = Recorded::new(seed);
                        let mut reference = OldBelow(Recorded::new(seed));
                        let board = Board::generate(&config, &mut candidate);
                        let old_board = Board::generate(&config, &mut reference);
                        assert_eq!(board, old_board);
                        assert_eq!(candidate.rng.state, reference.0.rng.state);
                        assert_eq!(candidate.raw, reference.0.raw);
                        let state = State::new(board, config).unwrap();
                        let old_state = State::new(old_board, config).unwrap();
                        assert_eq!(state, old_state);
                        assert_eq!(state.canonical_bytes(), old_state.canonical_bytes());
                        assert_eq!(state.hash(), old_state.hash());
                    }
                }
            }
        }
    }
}

#[test]
fn full_policy_games_preserve_choices_states_and_raw_rng_sequence() {
    for dice in [0, 1] {
        for index in [0, 2048] {
            let prepared = runner::prepare(
                41,
                index,
                Config {
                    dice,
                    ..Config::default()
                },
            )
            .unwrap();
            let mut state = prepared.state;
            let mut old_state = state;
            let mut candidate = Recorded::new(prepared.rng.state);
            let mut reference = OldBelow(Recorded::new(prepared.rng.state));
            while !state.is_terminal() && !state.is_dead() {
                let choice = runner::choose(&state, &mut candidate).unwrap();
                let old_choice = runner::choose(&old_state, &mut reference).unwrap();
                assert_eq!(choice, old_choice);
                state = choice.apply(&state).unwrap();
                old_state = old_choice.apply(&old_state).unwrap();
                assert_eq!(state, old_state);
                assert_eq!(candidate.rng.state, reference.0.rng.state);
            }
            assert_eq!(candidate.raw, reference.0.raw);
            assert_eq!(state.canonical_bytes(), old_state.canonical_bytes());
            assert_eq!(state.hash(), old_state.hash());
            assert_eq!(runner::play(prepared).unwrap().checksum, state.hash());
        }
    }
}
