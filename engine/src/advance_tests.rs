//! Forward rollout regression tests. This module can call crate-private advance.
use crate::{
    runner::{self, Choice},
    *,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counted;
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
fn allocation() {
    if ACTIVE.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
    }
}
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation();
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
fn measured(f: impl FnOnce()) -> usize {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            ACTIVE.with(|a| a.set(false));
        }
    }
    ALLOCATIONS.with(|n| n.set(0));
    ACTIVE.with(|a| a.set(true));
    let guard = Guard;
    f();
    drop(guard);
    ALLOCATIONS.with(Cell::get)
}
fn supply(s: &mut State, r: usize, minimum: u8) {
    let p = s.turn_owner as usize;
    if s.resources[p][r] < minimum {
        let n = minimum - s.resources[p][r];
        s.bank[r] -= n;
        s.resources[p][r] += n;
    }
    s.recompute_caches();
}
fn card(mut s: State, t: usize) -> State {
    s.playable_development[s.turn_owner as usize][t] += 1;
    s.development_deck[t] -= 1;
    s.recompute_caches();
    s
}
fn phases() -> [State; 14] {
    let mut config = Config::default();
    config.dice = 1;
    let initial = State::new(Board::default(), config).unwrap();
    let mut states = [initial; 14];
    let mut s = initial;
    // Complete exactly the four setup pairs required by S1, picking the first legal action.
    for placement in 0..8 {
        let mut actions = [0; 226];
        let n = s.legal_actions(&mut actions);
        assert!(n > 0);
        s = s.apply_action(actions[0]).unwrap();
        if placement == 0 {
            states[1] = s;
        }
    }
    states[2] = s;
    states[3] = s.apply_action(0).unwrap();
    states[7] = states[3].apply_outcome(0).unwrap();
    states[5] = states[3].apply_outcome(5).unwrap();
    let mut discard = states[2];
    supply(&mut discard, 0, 10);
    states[4] = discard.apply_action(0).unwrap().apply_outcome(5).unwrap();
    let mut robber = states[5];
    robber.config.friendly_robber = false;
    let opponent = 1 - robber.turn_owner as usize;
    robber.resources[opponent][0] += 1;
    robber.bank[0] -= 1;
    robber.recompute_caches();
    let mut actions = [0; 226];
    let n = robber.legal_actions(&mut actions);
    states[6] = actions[..n]
        .iter()
        .find_map(|&a| {
            let after = robber.apply_action(a).unwrap();
            (after.phase == phase::STEAL).then_some(after)
        })
        .expect("opponent setup settlement supplies a theft destination");
    let mut buy = states[7];
    for r in 2..5 {
        supply(&mut buy, r, 1);
    }
    states[8] = buy.apply_action(2).unwrap();
    states[9] = card(states[7], 2).apply_action(4).unwrap();
    states[10] = card(states[7], 3).apply_action(5).unwrap();
    states[11] = card(states[7], 4).apply_action(6).unwrap();
    let mut trade = states[7];
    supply(&mut trade, 0, 4);
    states[12] = trade.apply_action(216).unwrap();
    let mut winning = buy;
    winning.config.victory_points =
        u16::from(winning.cache.total_points[winning.turn_owner as usize]) + 1;
    states[13] = winning.apply_action(2).unwrap().apply_outcome(17).unwrap();
    for (phase, s) in states.iter().enumerate() {
        assert_eq!(s.phase as usize, phase);
        s.validate().unwrap();
    }
    states
}

fn compare(state: State, choice: Choice) {
    let expected = choice.apply(&state);
    let mut current = state;
    let result = choice.advance(&mut current);
    match expected {
        Ok(next) => {
            assert_eq!(result, Ok(()));
            assert_eq!(current, next, "phase {} {choice:?}", state.phase);
            let mut rebuilt = current;
            rebuilt.recompute_caches();
            assert_eq!(current.cache, rebuilt.cache);
            assert_eq!(current.canonical_bytes(), next.canonical_bytes());
        }
        Err(error) => {
            assert_eq!(result, Err(error));
            assert_eq!(
                current, state,
                "error must preserve every field for {choice:?}"
            );
            assert_eq!(current.canonical_bytes(), state.canonical_bytes());
        }
    }
}

#[test]
fn forward_all_phase_ids_match_pure_apply_with_exact_caches_and_errors() {
    for state in phases() {
        for id in (0..ACTION_COUNT as u16).chain([ACTION_COUNT as u16, u16::MAX]) {
            compare(state, Choice::Action(id));
        }
        for id in (0..21).chain([21, u16::MAX]) {
            compare(state, Choice::Outcome(id));
        }
    }
}

#[test]
fn forward_errors_are_atomic_for_setup_and_full_width_counters() {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.turn_owner = 1;
    s.setup_pairs_done = 2;
    // Legality accepts this deliberately depleted-bank fixture. Wood grants
    // would precede the failed brick grant at node 8 in the old mutation body.
    s.bank[1] = 0;
    s.resources[0][1] = 19;
    s.recompute_caches();
    assert_eq!(s.apply_action(87), Err(Error::InvalidState));
    compare(s, Choice::Action(87));

    let mut s = phases()[7];
    s.turn_number = u64::MAX;
    s.new_development[0][2] += 1;
    s.development_deck[2] -= 1;
    s.recompute_caches();
    s.validate().unwrap();
    assert_eq!(s.apply_action(1), Err(Error::RepresentationOverflow));
    compare(s, Choice::Action(1));

    // Even an unnormalized winning MAIN fixture at the maximum counter must
    // preserve the outgoing-player win before checking the next turn number.
    s.config.victory_points = u16::from(s.cache.total_points[0]);
    let winner = s.apply_action(1).unwrap();
    assert_eq!(winner.winner(), Some(0));
    assert_eq!(winner.turn_number, u64::MAX);
    assert_eq!(winner.new_development, s.new_development);
    compare(s, Choice::Action(1));

    let mut s = phases()[2];
    s.config.dice = 0;
    s.dice.initialized_player_mask = 3;
    s.dice.sevens = [u64::MAX; 2];
    s.dice.streak_owner = 1;
    s.dice.streak_length = 1;
    s.dice.recent_len = 5;
    s.dice.recent = [2, 3, 4, 5, 6];
    s = s.apply_action(0).unwrap();
    s.validate().unwrap();
    assert_eq!(s.apply_outcome(5), Err(Error::RepresentationOverflow));
    compare(s, Choice::Outcome(5));
}

#[test]
fn forward_all_phases_allocate_nothing_with_positive_control() {
    assert!(
        measured(|| {
            let allocation = Box::new(std::hint::black_box([1u64; 32]));
            std::hint::black_box(&allocation);
        }) > 0,
        "allocator control must fire"
    );
    for state in phases() {
        let mut actions = [0; ACTION_COUNT];
        let actions_len = state.legal_actions(&mut actions);
        let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
        let outcomes_len = state.chance_outcomes(&mut outcomes);
        let count = measured(|| {
            for &id in &actions[..actions_len] {
                let mut current = state;
                current.advance_action(id).unwrap();
                std::hint::black_box(&mut current);
            }
            for outcome in &outcomes[..outcomes_len] {
                let mut current = state;
                current.advance_outcome(outcome.id).unwrap();
                std::hint::black_box(&mut current);
            }
        });
        assert_eq!(count, 0, "phase {} allocated", state.phase);
    }
}

#[test]
fn forward_runner_preserves_copy_trajectory_rng_and_totals() {
    for dice in 0..2 {
        for first_player in 0..2 {
            let mut config = Config::default();
            config.dice = dice;
            config.first_player = first_player;
            let game = runner::prepare(41, 0, config).unwrap();
            let mut copy = game.state;
            let mut forward = game.state;
            let mut copy_rng = game.rng;
            let mut forward_rng = game.rng;
            let mut expected = runner::Totals::default();
            while !copy.is_dead() {
                let chosen = runner::choose(&copy, &mut copy_rng);
                assert_eq!(chosen, runner::choose(&forward, &mut forward_rng));
                assert_eq!(copy_rng.state, forward_rng.state);
                let Some(choice) = chosen else {
                    break;
                };
                match choice {
                    Choice::Action(_) => expected.decisions += 1,
                    Choice::Outcome(_) => expected.chance_steps += 1,
                }
                copy = choice.apply(&copy).unwrap();
                choice.advance(&mut forward).unwrap();
                assert_eq!(forward, copy);
                assert_eq!(forward.canonical_bytes(), copy.canonical_bytes());
            }
            expected.attempts = 1;
            expected.turns = copy.turn_number;
            expected.winners = u64::from(copy.winner < 2);
            expected.dead_games = u64::from(copy.is_dead());
            expected.games = expected.winners + expected.dead_games;
            if copy.winner < 2 {
                expected.winner_counts[copy.winner as usize] = 1;
            }
            expected.checksum = copy.hash();
            assert_eq!(runner::play(game).unwrap(), expected);
        }
    }
}

#[test]
fn forward_sparse_prerequisite_dice_errors_and_reshuffle_preserve_inputs() {
    // Preserve the sparse experiment's overflow fixture with an empty recent
    // window as well as the full-window fixture in the forward regression above.
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::PRE_ROLL;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.dice.initialized_player_mask = 3;
    s.dice.sevens = [u64::MAX; 2];
    s.dice.streak_owner = 1;
    s.dice.streak_length = 1;
    let s = s.apply_action(0).unwrap();
    assert_eq!(s.apply_outcome(5), Err(Error::RepresentationOverflow));
    compare(s, Choice::Outcome(5));

    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::PRE_ROLL;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    // Twenty-four explicit draws leave twelve cards. The next ROLL reshuffles
    // only the deck counts, preserving the rest of the dice history.
    for _ in 0..24 {
        s = s.apply_action(0).unwrap();
        let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
        let n = s.chance_outcomes(&mut outcomes);
        let id = outcomes[..n].iter().find(|o| o.id != 5).unwrap().id;
        s = s.apply_outcome(id).unwrap().apply_action(1).unwrap();
    }
    let before = s;
    compare(s, Choice::Action(0));
    let next = s.apply_action(0).unwrap();
    assert_eq!(s, before);
    let mut expected_dice = before.dice;
    expected_dice.counts = dice::INITIAL_COUNTS;
    assert_eq!(next.dice, expected_dice);
}

/// Select an exact legal-list index or integer chance mass without changing
/// production sampling. Real SplitMix64 draw counts are checked separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FixedDraw {
    draw: u128,
    calls: usize,
}
impl RandomSource for FixedDraw {
    fn next_u64(&mut self) -> u64 {
        panic!("fixed selector must use the bounded RNG interface")
    }
    fn below(&mut self, bound: u64) -> u64 {
        assert!(self.draw < u128::from(bound));
        self.calls += 1;
        self.draw as u64
    }
    fn below_u128(&mut self, bound: u128) -> u128 {
        assert!(self.draw < bound);
        self.calls += 1;
        self.draw
    }
}

fn compare_chosen(state: State, draw: u128) -> Option<Choice> {
    let mut checked_rng = FixedDraw { draw, calls: 0 };
    let mut chosen_rng = checked_rng;
    let choice = runner::choose(&state, &mut checked_rng);
    let mut checked = state;
    let expected = match choice {
        Some(choice) => choice.advance(&mut checked).map(|()| Some(choice)),
        None => Ok(None),
    };
    let mut chosen = state;
    assert_eq!(chosen.choose_and_advance(&mut chosen_rng), expected);
    assert_eq!(chosen_rng, checked_rng);
    assert_eq!(chosen, checked);
    assert_eq!(chosen.canonical_bytes(), checked.canonical_bytes());
    if expected.is_err() {
        assert_eq!(chosen, state, "failed chosen transition must be atomic");
    } else {
        let mut rebuilt = chosen;
        rebuilt.recompute_caches();
        assert_eq!(chosen.cache, rebuilt.cache);
    }
    choice
}

fn draw_for(state: &State, choice: Choice) -> u128 {
    match choice {
        Choice::Action(id) => {
            let mut actions = [0; ACTION_COUNT];
            let n = state.legal_actions(&mut actions);
            actions[..n].iter().position(|&a| a == id).unwrap() as u128
        }
        Choice::Outcome(id) => {
            let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
            let n = state.chance_outcomes(&mut outcomes);
            assert!(outcomes[..n].iter().any(|o| o.id == id));
            outcomes[..n]
                .iter()
                .take_while(|o| o.id != id)
                .map(|o| o.weight)
                .sum()
        }
    }
}

#[test]
fn chosen_every_enumerated_choice_matches_checked_in_all_phases_and_dice_modes() {
    for dice in 0..2 {
        for mut state in phases() {
            state.config.dice = dice;
            if state.phase == phase::DICE {
                state
                    .dice
                    .prepare_roll(dice == 0, state.turn_owner)
                    .unwrap();
            }
            state.validate().unwrap();
            let mut actions = [0; ACTION_COUNT];
            let n = state.legal_actions(&mut actions);
            assert!(actions[..n].windows(2).all(|a| a[0] < a[1]));
            for (index, &id) in actions[..n].iter().enumerate() {
                assert_eq!(
                    compare_chosen(state, index as u128),
                    Some(Choice::Action(id))
                );
            }
            let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
            let n = state.chance_outcomes(&mut outcomes);
            assert!(outcomes[..n].windows(2).all(|o| o[0].id < o[1].id));
            let mut cumulative = 0;
            for o in &outcomes[..n] {
                assert!(o.weight > 0);
                for draw in [cumulative, cumulative + o.weight - 1] {
                    assert_eq!(compare_chosen(state, draw), Some(Choice::Outcome(o.id)));
                }
                cumulative += o.weight;
            }
            if state.phase == phase::TERMINAL {
                assert_eq!(compare_chosen(state, 0), None);
            }
        }
    }
}

#[test]
fn chosen_errors_preserve_setup_grants_full_width_counters_and_outgoing_win() {
    let mut setup = phases()[0];
    setup.turn_owner = 1;
    setup.setup_pairs_done = 2;
    setup.bank[1] = 0;
    setup.resources[0][1] = 19;
    setup.recompute_caches();
    assert_eq!(setup.apply_action(87), Err(Error::InvalidState));
    compare_chosen(setup, draw_for(&setup, Choice::Action(87)));

    let mut main = phases()[7];
    main.turn_number = u64::MAX;
    main.new_development[0][2] += 1;
    main.development_deck[2] -= 1;
    main.recompute_caches();
    main.validate().unwrap();
    assert_eq!(main.apply_action(1), Err(Error::RepresentationOverflow));
    compare_chosen(main, draw_for(&main, Choice::Action(1)));
    main.config.victory_points = u16::from(main.cache.total_points[0]);
    let won = main.apply_action(1).unwrap();
    assert_eq!(won.winner(), Some(0));
    assert_eq!(won.new_development, main.new_development);
    compare_chosen(main, draw_for(&main, Choice::Action(1)));

    for full_recent in [false, true] {
        let mut state = phases()[2];
        state.config.dice = 0;
        state.dice.initialized_player_mask = 3;
        state.dice.sevens = [u64::MAX; 2];
        state.dice.streak_owner = 1;
        state.dice.streak_length = 1;
        if full_recent {
            state.dice.recent_len = 5;
            state.dice.recent = [2, 3, 4, 5, 6];
        }
        let state = state.apply_action(0).unwrap();
        if full_recent {
            state.validate().unwrap();
        } else {
            // Preserve the historical sparse overflow fixture too. A short
            // recent window cannot canonically hold these full-width counts.
            assert_eq!(state.validate(), Err(Error::InvalidState));
        }
        assert_eq!(state.apply_outcome(5), Err(Error::RepresentationOverflow));
        compare_chosen(state, draw_for(&state, Choice::Outcome(5)));
    }
}

#[test]
fn chosen_all_phases_allocate_nothing_with_positive_control() {
    assert!(
        measured(|| {
            let control = Box::new(std::hint::black_box([1u64; 32]));
            std::hint::black_box(&control);
        }) > 0
    );
    for state in phases() {
        let mut rng = SplitMix64::new(u64::from(state.phase));
        assert_eq!(
            measured(|| {
                let mut chosen = state;
                std::hint::black_box(chosen.choose_and_advance(&mut rng).unwrap());
                std::hint::black_box(chosen);
            }),
            0,
            "phase {} allocated",
            state.phase
        );
    }
}

#[test]
fn chosen_trajectories_rng_caches_and_totals_match_checked_both_dice_modes() {
    for dice in 0..2 {
        for first_player in 0..2 {
            let config = Config {
                dice,
                first_player,
                ..Config::default()
            };
            let game = runner::prepare(41, 0, config).unwrap();
            let mut checked = game.state;
            let mut chosen = game.state;
            let mut checked_rng = game.rng;
            let mut chosen_rng = game.rng;
            let mut totals = runner::Totals::default();
            while !checked.is_dead() {
                let expected = runner::choose(&checked, &mut checked_rng);
                assert_eq!(chosen.choose_and_advance(&mut chosen_rng), Ok(expected));
                assert_eq!(checked_rng.state, chosen_rng.state);
                let Some(choice) = expected else { break };
                choice.advance(&mut checked).unwrap();
                assert_eq!(chosen, checked);
                assert_eq!(chosen.canonical_bytes(), checked.canonical_bytes());
                let mut rebuilt = chosen;
                rebuilt.recompute_caches();
                assert_eq!(chosen.cache, rebuilt.cache);
                match choice {
                    Choice::Action(_) => totals.decisions += 1,
                    Choice::Outcome(_) => totals.chance_steps += 1,
                }
            }
            totals.attempts = 1;
            totals.turns = checked.turn_number;
            totals.winners = u64::from(checked.winner < 2);
            totals.dead_games = u64::from(checked.is_dead());
            totals.games = totals.winners + totals.dead_games;
            if checked.winner < 2 {
                totals.winner_counts[checked.winner as usize] = 1;
            }
            totals.checksum = checked.hash();
            assert_eq!(runner::play(game).unwrap(), totals);
        }
    }
}

// Keep the pre-compact policy as a reference using the unchanged public APIs.
fn enumerated_choose(state: &State, rng: &mut impl RandomSource) -> Option<Choice> {
    if state.phase == phase::TERMINAL {
        return None;
    }
    if state.actor().is_some() {
        let mut actions = [0; ACTION_COUNT];
        let n = state.legal_actions(&mut actions);
        Some(Choice::Action(actions[rng.below(n as u64) as usize]))
    } else {
        let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
        let n = state.chance_outcomes(&mut outcomes);
        Some(Choice::Outcome(runner::sample_outcome(&outcomes[..n], rng)))
    }
}

fn compact_chance_states() -> Vec<State> {
    let fixtures = phases();
    let mut states = Vec::new();
    for dice in 0..2 {
        for phase in [phase::DICE, phase::STEAL, phase::DEVELOPMENT_DRAW] {
            let mut state = fixtures[phase as usize];
            state.config.dice = dice;
            if phase == phase::DICE {
                state
                    .dice
                    .prepare_roll(dice == 0, state.turn_owner)
                    .unwrap();
            }
            states.push(state);
            if phase == phase::STEAL {
                for counts in [[0, 2, 0, 1, 0], [0, 0, 0, 0, 1]] {
                    let mut sparse = state;
                    let victim = 1 - sparse.turn_owner as usize;
                    for (r, &count) in counts.iter().enumerate() {
                        sparse.bank[r] += sparse.resources[victim][r];
                        sparse.bank[r] -= count;
                        sparse.resources[victim][r] = count;
                    }
                    sparse.recompute_caches();
                    states.push(sparse);
                }
            } else if phase == phase::DEVELOPMENT_DRAW {
                for counts in [[0, 1, 0, 2, 0], [0, 0, 0, 0, 1]] {
                    let mut sparse = state;
                    let opponent = 1 - sparse.turn_owner as usize;
                    for (t, &count) in counts.iter().enumerate() {
                        sparse.playable_development[opponent][t] +=
                            sparse.development_deck[t] - count;
                        sparse.development_deck[t] = count;
                    }
                    sparse.recompute_caches();
                    states.push(sparse);
                }
            }
        }
    }
    for (sevens, owner, streak) in [
        ([u64::MAX, u64::MAX], 1, 1),
        ([u64::MAX, u64::MAX], 1, 4),
        ([u64::MAX, u64::MAX], 1, 5),
        ([u64::MAX, u64::MAX], 1, u64::MAX),
        ([u64::MAX, 0], 0, 1),
        ([u64::MAX, 0], 0, u64::MAX),
        ([u64::MAX - 1, 1], 0, 4),
    ] {
        for roller in 0..2 {
            let mut state = fixtures[phase::DICE as usize];
            state.config.dice = 0;
            state.turn_owner = roller;
            state.dice = dice::DiceState {
                recent_len: 5,
                recent: [2, 3, 4, 5, 6],
                initialized_player_mask: 3,
                sevens,
                streak_owner: owner,
                streak_length: streak,
                ..dice::DiceState::default()
            };
            state.recompute_caches();
            states.push(state);
        }
    }
    // Zero support from depleted counts and from three recent occurrences,
    // independently of the seven-factor clamps above.
    for total in 2..=12 {
        for depleted in [false, true] {
            let mut state = fixtures[phase::DICE as usize];
            state.config.dice = 0;
            state.dice.initialized_player_mask = 3;
            state.dice.recent_len = 5;
            state.dice.recent = [total, total, total, 6, 8];
            if depleted {
                state.dice.counts[(total - 2) as usize] = 0;
            }
            if total == 7 {
                state.dice.sevens = [3, 0];
                state.dice.streak_owner = 0;
                state.dice.streak_length = 3;
            }
            states.push(state);
        }
    }
    for state in &states {
        state.validate().unwrap();
    }
    states
}

#[test]
fn compact_chance_all_interval_endpoints_match_enumerator_both_dice_and_sparse_support() {
    let mut saw_full_width_mass = false;
    let mut saw_zero_dice_weight = false;
    for state in compact_chance_states() {
        let before = state;
        let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
        let n = state.chance_outcomes(&mut outcomes);
        assert!(n > 0 && outcomes[..n].windows(2).all(|o| o[0].id < o[1].id));
        if state.phase == phase::DICE {
            saw_zero_dice_weight |= n < MAX_OUTCOMES;
        }
        let total = outcomes[0].total;
        saw_full_width_mass |= total > u128::from(u64::MAX);
        let mut cumulative = 0;
        for outcome in &outcomes[..n] {
            for draw in [cumulative, cumulative + outcome.weight - 1] {
                let mut reference_rng = FixedDraw { draw, calls: 0 };
                let mut compact_rng = reference_rng;
                let expected = enumerated_choose(&state, &mut reference_rng);
                assert_eq!(expected, Some(Choice::Outcome(outcome.id)));
                assert_eq!(runner::choose(&state, &mut compact_rng), expected);
                assert_eq!(compact_rng, reference_rng);
            }
            cumulative += outcome.weight;
        }
        assert_eq!(cumulative, total);
        assert_eq!(state, before);
    }
    assert!(saw_full_width_mass && saw_zero_dice_weight);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChanceWords {
    words: [u64; 6],
    cursor: usize,
}
impl RandomSource for ChanceWords {
    fn next_u64(&mut self) -> u64 {
        let word = self.words[self.cursor];
        self.cursor += 1;
        word
    }
}

#[test]
fn compact_chance_preserves_two_word_rejection_sampling_and_rng_position() {
    let mut rejected_control = false;
    let mut no_rejection_control = false;
    for state in compact_chance_states() {
        let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
        let n = state.chance_outcomes(&mut outcomes);
        let total = outcomes[0].total;
        let threshold = total.wrapping_neg() % total;
        let mut cumulative = 0;
        for outcome in &outcomes[..n] {
            for draw in [cumulative, cumulative + outcome.weight - 1] {
                let accepted = if draw < threshold {
                    draw.checked_add(total).unwrap()
                } else {
                    draw
                };
                assert!(accepted >= threshold && accepted % total == draw);
                for reject in [false, true] {
                    if reject && threshold == 0 {
                        continue;
                    }
                    let mut words = ChanceWords {
                        words: [0; 6],
                        cursor: 0,
                    };
                    let offset = if reject { 2 } else { 0 };
                    words.words[offset] = (accepted >> 64) as u64;
                    words.words[offset + 1] = accepted as u64;
                    words.words[offset + 2] = 0x91a2_b3c4_d5e6_f708;
                    let mut reference_rng = words.clone();
                    assert_eq!(
                        runner::choose(&state, &mut words),
                        enumerated_choose(&state, &mut reference_rng)
                    );
                    assert_eq!(words, reference_rng);
                    assert_eq!(words.cursor, offset + 2);
                    assert_eq!(words.next_u64(), 0x91a2_b3c4_d5e6_f708);
                    rejected_control |= reject;
                    no_rejection_control |= !reject;
                }
            }
            cumulative += outcome.weight;
        }
    }
    assert!(rejected_control && no_rejection_control);
}

#[test]
fn compact_chance_complete_trajectories_rng_caches_and_totals_match_public_reference() {
    for dice in 0..2 {
        for first_player in 0..2 {
            let game = runner::prepare(
                41,
                0,
                Config {
                    dice,
                    first_player,
                    ..Config::default()
                },
            )
            .unwrap();
            let mut reference = game.state;
            let mut compact = game.state;
            let mut reference_rng = game.rng;
            let mut compact_rng = game.rng;
            let mut totals = runner::Totals::default();
            while !reference.is_dead() {
                let expected = enumerated_choose(&reference, &mut reference_rng);
                assert_eq!(compact.choose_and_advance(&mut compact_rng), Ok(expected));
                assert_eq!(compact_rng.state, reference_rng.state);
                let Some(choice) = expected else { break };
                reference = choice.apply(&reference).unwrap();
                assert_eq!(compact, reference);
                assert_eq!(compact.canonical_bytes(), reference.canonical_bytes());
                let mut rebuilt = compact;
                rebuilt.recompute_caches();
                assert_eq!(compact.cache, rebuilt.cache);
                match choice {
                    Choice::Action(_) => totals.decisions += 1,
                    Choice::Outcome(_) => totals.chance_steps += 1,
                }
            }
            totals.attempts = 1;
            totals.turns = reference.turn_number;
            totals.winners = u64::from(reference.winner < 2);
            totals.dead_games = u64::from(reference.is_dead());
            totals.games = totals.winners + totals.dead_games;
            if reference.winner < 2 {
                totals.winner_counts[reference.winner as usize] = 1;
            }
            totals.checksum = reference.hash();
            assert_eq!(runner::play(game).unwrap(), totals);
        }
    }
}

#[test]
fn compact_chance_allocates_nothing_with_positive_control() {
    assert!(
        measured(|| {
            let control = Box::new(std::hint::black_box([1u64; 32]));
            std::hint::black_box(&control);
        }) > 0
    );
    for state in compact_chance_states() {
        let mut rng = SplitMix64::new(u64::from(state.phase));
        assert_eq!(
            measured(|| {
                std::hint::black_box(runner::choose(&state, &mut rng));
            }),
            0
        );
    }
}

fn compare_scratch(state: State, draw: u128) {
    let mut reference_rng = FixedDraw { draw, calls: 0 };
    let mut scratch_rng = reference_rng;
    let choice = runner::choose(&state, &mut reference_rng);
    let mut reference = state;
    let expected = match choice {
        Some(choice) => choice.advance(&mut reference).map(|()| Some(choice)),
        None => Ok(None),
    };
    let mut scratch = [u16::MAX; ACTION_COUNT];
    let mut current = state;
    assert_eq!(
        current.choose_and_advance_with_scratch(&mut scratch_rng, &mut scratch),
        expected
    );
    assert_eq!(scratch_rng, reference_rng);
    assert_eq!(current, reference);
    assert_eq!(current.canonical_bytes(), reference.canonical_bytes());
    if expected.is_err() {
        assert_eq!(current, state, "failed scratch transition must be atomic");
    }
}

#[test]
fn scratch_poisoned_prefixes_all_phases_and_rng_overrides_match_public_choose() {
    for dice in 0..2 {
        for mut state in phases() {
            state.config.dice = dice;
            if state.phase == phase::DICE {
                state
                    .dice
                    .prepare_roll(dice == 0, state.turn_owner)
                    .unwrap();
            }
            state.validate().unwrap();
            let mut expected = [0; ACTION_COUNT];
            let n = state.legal_actions(&mut expected);
            if state.actor().is_some() {
                assert!(n > 0);
                for draw in 0..n {
                    let mut scratch = [u16::MAX; ACTION_COUNT];
                    let mut rng = FixedDraw {
                        draw: draw as u128,
                        calls: 0,
                    };
                    assert_eq!(
                        runner::choose_with_scratch(&state, &mut rng, &mut scratch),
                        Some(Choice::Action(expected[draw]))
                    );
                    assert_eq!(&scratch[..n], &expected[..n]);
                    assert!(scratch[..n].iter().all(|&id| id < ACTION_COUNT as u16));
                    assert_eq!(rng.calls, 1);
                    compare_scratch(state, draw as u128);
                }
            } else {
                let mut scratch = [u16::MAX; ACTION_COUNT];
                let mut rng = FixedDraw { draw: 0, calls: 0 };
                let _ = runner::choose_with_scratch(&state, &mut rng, &mut scratch);
                assert_eq!(scratch, [u16::MAX; ACTION_COUNT]);
                if state.is_terminal() {
                    assert_eq!(rng.calls, 0);
                    compare_scratch(state, 0);
                } else {
                    let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
                    let n = state.chance_outcomes(&mut outcomes);
                    assert!(n > 0);
                    for outcome in &outcomes[..n] {
                        compare_scratch(state, draw_for(&state, Choice::Outcome(outcome.id)));
                    }
                }
            }
        }
    }
}

#[test]
fn scratch_full_trajectories_rng_caches_and_totals_match_fresh_buffers() {
    for dice in 0..2 {
        for first_player in 0..2 {
            for index in [0, 2048] {
                let config = Config {
                    dice,
                    first_player,
                    ..Config::default()
                };
                let mut game = runner::prepare(41, index, config).unwrap();
                // Preserve an explicitly advanced supplied stream as well.
                game.rng.next_u64();
                let mut reference = game.state;
                let mut current = game.state;
                let mut reference_rng = game.rng;
                let mut scratch_rng = game.rng;
                // Reuse stale contents across decisions and phase changes.
                let mut scratch = [u16::MAX; ACTION_COUNT];
                let mut totals = runner::Totals::default();
                while !reference.is_dead() {
                    let expected = runner::choose(&reference, &mut reference_rng);
                    assert_eq!(
                        current.choose_and_advance_with_scratch(&mut scratch_rng, &mut scratch),
                        Ok(expected)
                    );
                    assert_eq!(scratch_rng.state, reference_rng.state);
                    let Some(choice) = expected else { break };
                    choice.advance(&mut reference).unwrap();
                    assert_eq!(current, reference);
                    assert_eq!(current.canonical_bytes(), reference.canonical_bytes());
                    let mut rebuilt = current;
                    rebuilt.recompute_caches();
                    assert_eq!(current.cache, rebuilt.cache);
                    match choice {
                        Choice::Action(_) => totals.decisions += 1,
                        Choice::Outcome(_) => totals.chance_steps += 1,
                    }
                }
                totals.attempts = 1;
                totals.turns = reference.turn_number;
                totals.winners = u64::from(reference.winner < 2);
                totals.dead_games = u64::from(reference.is_dead());
                totals.games = totals.winners + totals.dead_games;
                if reference.winner < 2 {
                    totals.winner_counts[reference.winner as usize] = 1;
                }
                totals.checksum = reference.hash();
                assert_eq!(current.hash(), reference.hash());
                assert_eq!(runner::play(game).unwrap(), totals);
            }
        }
    }
}

#[test]
fn scratch_errors_are_atomic_and_steady_state_allocates_nothing() {
    let mut setup = State::new(Board::default(), Config::default()).unwrap();
    setup.turn_owner = 1;
    setup.setup_pairs_done = 2;
    setup.bank[1] = 0;
    setup.resources[0][1] = 19;
    setup.recompute_caches();
    compare_scratch(setup, draw_for(&setup, Choice::Action(87)));
    let mut main = phases()[7];
    main.turn_number = u64::MAX;
    compare_scratch(main, draw_for(&main, Choice::Action(1)));
    assert!(
        measured(|| {
            std::hint::black_box(Box::new(std::hint::black_box([1u64; 32])));
        }) > 0
    );
    for state in phases() {
        let mut scratch = [u16::MAX; ACTION_COUNT];
        let mut rng = SplitMix64::new(u64::from(state.phase));
        assert_eq!(
            measured(|| {
                let mut current = state;
                std::hint::black_box(
                    current
                        .choose_and_advance_with_scratch(&mut rng, &mut scratch)
                        .unwrap(),
                );
                std::hint::black_box(current);
            }),
            0,
            "phase {} allocated",
            state.phase
        );
    }
}
