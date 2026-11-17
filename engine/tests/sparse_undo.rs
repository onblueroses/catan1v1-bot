//! E5: sparse make and unmake. A failed make leaves the state untouched,
//! nested makes unwind to each parent exactly, and the production patch
//! handles hexes that share a number and a resource.
use catan_engine::{runner::Choice, *};

fn assert_error_atomic(state: State, choice: Choice, error: Error) {
    assert_eq!(choice.apply(&state), Err(error));
    let mut current = state;
    assert_eq!(choice.make(&mut current).unwrap_err(), error);
    assert_eq!(current, state);
    assert_eq!(current.canonical_bytes(), state.canonical_bytes());
}

#[test]
fn e5_errors_restore_partial_setup_grant_and_end_turn_card_aging() {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.turn_owner = 1;
    s.setup_pairs_done = 2;
    // Node 8 touches wood hexes 0 and 1, then brick hex 4. The brick
    // subtraction fails after two wood grants and the occupancy/control edits.
    assert_eq!(topology::NODE_HEXES[8], [0, 1, 4]);
    s.bank[1] = 0;
    s.resources[0][1] = 19;
    s.recompute_caches();
    assert_error_atomic(s, Choice::Action(79 + 8), Error::InvalidState);

    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::MAIN;
    s.setup_pairs_done = 4;
    s.turn_number = u64::MAX;
    s.new_development[0][2] = 1;
    s.development_deck[2] -= 1;
    s.recompute_caches();
    s.validate().unwrap();
    assert_error_atomic(s, Choice::Action(1), Error::RepresentationOverflow);
}

#[test]
fn e5_dice_overflow_is_atomic_and_reshuffle_restores_history() {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::PRE_ROLL;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.dice.initialized_player_mask = 3;
    s.dice.sevens = [u64::MAX; 2];
    s.dice.streak_owner = 1;
    s.dice.streak_length = 1;
    let s = s.apply_action(0).unwrap();
    assert_error_atomic(s, Choice::Outcome(5), Error::RepresentationOverflow);

    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::PRE_ROLL;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    // Twenty-four explicit draws leave twelve cards. The next ROLL reshuffles.
    for _ in 0..24 {
        s = s.apply_action(0).unwrap();
        let mut outcomes = [Outcome::default(); 11];
        let n = s.chance_outcomes(&mut outcomes);
        let id = outcomes[..n].iter().find(|o| o.id != 5).unwrap().id;
        s = s.apply_outcome(id).unwrap().apply_action(1).unwrap();
    }
    let before = s;
    let record = s.make_action(0).unwrap();
    assert_eq!(s.dice.counts, dice::INITIAL_COUNTS);
    s.unmake(record);
    assert_eq!(s, before);
}

#[test]
fn e5_nested_setup_decision_and_chance_unmake_restores_each_parent() {
    eprintln!(
        "State bytes={}, UndoRecord bytes={}",
        size_of::<State>(),
        size_of::<UndoRecord>()
    );
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    let initial = s;
    let mut records = Vec::new();
    let mut parents = Vec::new();
    for _ in 0..8 {
        let mut actions = [0; ACTION_COUNT];
        assert!(s.legal_actions(&mut actions) > 0);
        parents.push(s);
        records.push(s.make_action(actions[0]).unwrap());
    }
    parents.push(s);
    records.push(s.make_action(0).unwrap());
    let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
    assert!(s.chance_outcomes(&mut outcomes) > 0);
    parents.push(s);
    records.push(s.make_outcome(outcomes[0].id).unwrap());
    for (record, parent) in records.into_iter().zip(parents).rev() {
        s.unmake(record);
        assert_eq!(s, parent);
        assert_eq!(s.canonical_bytes(), parent.canonical_bytes());
    }
    assert_eq!(s, initial);
}

#[test]
fn e5_production_patch_deduplicates_shared_roll_resource_cells() {
    let mut state = State::new(Board::default(), Config::default()).unwrap();
    state.phase = phase::ROBBER;
    state.resume_phase = phase::MAIN;
    state.setup_pairs_done = 4;
    state.turn_number = 1;
    state.config.friendly_robber = false;
    // Hexes 1 and 2 are both wood/3. Moving between them touches the
    // same two production cells, once for each player.
    state.robber_hex = 1;
    state.node_occupancy[1] = 1;
    state.node_occupancy[10] = 2;
    state.recompute_caches();
    for (mut current, choice) in [(state, Choice::Action(187 + 2)), {
        let mut city = state;
        city.phase = phase::MAIN;
        city.resume_phase = NONE;
        city.robber_hex = 18;
        city.node_occupancy = [0; 54];
        // Node 9 touches both wood/3 hexes and one brick hex.
        city.node_occupancy[9] = 1;
        city.resources[0] = [0, 0, 0, 2, 3];
        city.bank[3] -= 2;
        city.bank[4] -= 3;
        city.recompute_caches();
        (city, Choice::Action(133 + 9))
    }] {
        current.validate().unwrap();
        let before = current;
        let expected = choice.apply(&current).unwrap();
        let record = choice.make(&mut current).unwrap();
        assert_eq!(current, expected);
        current.validate().unwrap();
        current.unmake(record);
        assert_eq!(current, before);
        current.validate().unwrap();
    }
}
