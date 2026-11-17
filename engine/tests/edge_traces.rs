//! Validated snapshots for cross-engine replay. These are not constructor traces.
use catan_engine::{runner::Choice, trace, *};
use std::io::Cursor;

fn fixture(roller: u8) -> State {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::MAIN;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.turn_owner = roller;
    s.dice.prepare_roll(true, roller).unwrap();
    s.dice.apply_total(true, roller, 2).unwrap();
    s.last_dice = 2;
    s
}
fn independent_pre_roll_fixture() -> State {
    let mut s = fixture(0);
    s.config.dice = 1;
    s.phase = phase::PRE_ROLL;
    s.last_dice = 0;
    s.dice = dice::DiceState::default();
    s
}
fn give(s: &mut State, p: usize, r: usize, n: u8) {
    s.resources[p][r] += n;
    s.bank[r] -= n;
}
fn finish_fixture(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    s
}
fn emit(name: &str, state: State, choices: &[Choice]) -> State {
    let mut state = finish_fixture(state);
    let mut bytes = Vec::new();
    trace::write_header(&mut bytes, &state, true).unwrap();
    for (index, &choice) in choices.iter().enumerate() {
        let expected = choice.apply(&state).unwrap();
        let before = state;
        let record = choice.make(&mut state).unwrap();
        assert_eq!(state, expected);
        let mut rebuilt = state;
        rebuilt.recompute_caches();
        assert_eq!(state.cache, rebuilt.cache);
        state.unmake(record);
        assert_eq!(state, before);
        state = expected;
        state.validate().unwrap();
        trace::write_step(&mut bytes, index as u64 + 1, choice, &state).unwrap();
    }
    trace::write_result(&mut bytes, choices.len() as u64, &state).unwrap();
    let replay = trace::replay(Cursor::new(&bytes)).unwrap();
    assert!(replay.snapshot);
    assert_eq!(replay.state.canonical_bytes(), state.canonical_bytes());
    if let Some(path) = std::env::var_os("CATAN_EDGE_TRACE_DIR") {
        let dir = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("edge-{name}.jsonl")), bytes).unwrap();
    }
    state
}

#[test]
fn deterministic_rule_edge_snapshots_replay() {
    use Choice::{Action as A, Outcome as O};
    for both in [false, true] {
        let mut s = independent_pre_roll_fixture();
        s.node_occupancy[0] = 3;
        if both {
            s.node_occupancy[8] = 2;
        }
        give(&mut s, 1, 0, 18);
        let t = emit(
            if both {
                "bank-shortage-both"
            } else {
                "bank-shortage-single"
            },
            s,
            &[A(0), O(0)],
        );
        assert_eq!(t.resources[0][0], if both { 0 } else { 1 });
    }
    let mut s = fixture(1);
    for e in [0, 1, 2, 3, 4, 5] {
        s.edge_occupancy[e] = 1;
    }
    s.edge_occupancy[7] = 2;
    for r in 0..4 {
        give(&mut s, 1, r, 1);
    }
    let t = emit("road-cut", s, &[A(83)]);
    assert_eq!(t.cache.road_lengths[0], 4);
    assert_eq!(t.longest_road_holder, 255);

    for p in 0..2 {
        for visible in [2, 3] {
            let mut s = fixture(0);
            s.phase = phase::ROBBER;
            s.resume_phase = phase::MAIN;
            s.node_occupancy[0] = 3 + p as u8;
            if visible == 3 {
                s.node_occupancy[50] = 1 + p as u8;
            }
            s.playable_development[p][1] = 2;
            s.development_deck[1] -= 2;
            s = finish_fixture(s);
            assert_eq!(s.apply_action(187).is_ok(), visible == 3);
            let choices = if visible == 3 {
                vec![A(187)]
            } else {
                vec![A(188)]
            };
            emit(&format!("friendly-player{p}-visible{visible}"), s, &choices);
        }
    }

    for cards in [9, 10] {
        let mut s = independent_pre_roll_fixture();
        give(&mut s, 0, 0, cards);
        let mut choices = vec![A(0), O(5)];
        if cards == 10 {
            choices.extend([A(206); 5]);
        }
        choices.push(A(187));
        let t = emit(&format!("discard-{cards}"), s, &choices);
        assert_eq!(t.resources[0][0], if cards == 10 { 5 } else { 9 });
    }

    let mut s = fixture(0);
    s.playable_development[1] = s.development_deck;
    s.development_deck = [0; 5];
    for r in 2..5 {
        give(&mut s, 0, r, 1);
    }
    s = finish_fixture(s);
    assert!(s.apply_action(2).is_err());
    emit("empty-development-deck", s, &[A(1)]);

    let mut s = fixture(0);
    for n in [0, 2, 16, 50, 52] {
        s.node_occupancy[n] = 1;
    }
    for e in 0..15 {
        s.edge_occupancy[e] = 1;
    }
    for r in 0..5 {
        give(&mut s, 0, r, 4);
    }
    s = finish_fixture(s);
    assert_eq!(s.cache.pieces_left[0], [0, 0, 4]);
    let t = emit("piece-limits-city-returns-settlement", s, &[A(133)]);
    assert_eq!(t.cache.pieces_left[0], [0, 1, 3]);

    let mut s = fixture(0);
    s.config.victory_points = 3;
    s.node_occupancy[0] = 3;
    for r in 2..5 {
        give(&mut s, 0, r, 1);
    }
    let t = emit("hidden-vp-immediate-win", s, &[A(2), O(17)]);
    assert_eq!(t.phase, phase::TERMINAL);
    assert_eq!(t.cache.visible_points[0], 2);
    assert_eq!(t.winner, 0);
}

#[test]
fn d5_road_building_edge_snapshots_replay() {
    use Choice::Action as A;

    let mut s = fixture(0);
    s.config.road_building_needs_legal_edge = true;
    s.node_occupancy[0] = 1;
    s.playable_development[0][2] = 1;
    s.development_deck[2] -= 1;
    let t = emit("road-building-needs-edge-two-roads", s, &[A(4), A(7), A(8)]);
    assert_eq!(&t.edge_occupancy[..2], &[1, 1]);
    assert_eq!(t.phase, phase::MAIN);
    assert_eq!(t.choices_remaining, 0);
    assert_eq!(t.spent_development[0][2], 1);

    let mut s = fixture(0);
    for e in 0..14 {
        s.edge_occupancy[e] = 1;
    }
    s.playable_development[0][2] = 1;
    s.development_deck[2] -= 1;
    s = finish_fixture(s);
    assert!(!s.config.road_building_needs_legal_edge);
    assert_eq!(s.cache.pieces_left[0][0], 1);
    let t = emit("road-building-one-piece", s, &[A(4), A(21)]);
    assert_eq!(t.edge_occupancy[14], 1);
    assert_eq!(t.cache.pieces_left[0][0], 0);
    assert_eq!(t.phase, phase::MAIN);
    assert_eq!(t.choices_remaining, 0);
    assert_eq!(t.spent_development[0][2], 1);

    let mut s = fixture(0);
    s.playable_development[0][2] = 1;
    s.development_deck[2] -= 1;
    s = finish_fixture(s);
    assert!(!s.config.road_building_needs_legal_edge);
    assert_eq!(s.cache.pieces_left[0][0], 15);
    assert!(!(0..72).any(|e| s.road_legal(e)));
    let t = emit("road-building-no-edge-default", s, &[A(4)]);
    assert_eq!(t.edge_occupancy, s.edge_occupancy);
    assert_eq!(t.phase, phase::MAIN);
    assert_eq!(t.choices_remaining, 0);
    assert_eq!(t.spent_development[0][2], 1);
    assert_eq!(t.playable_development[0][2], 0);

    s.config.road_building_needs_legal_edge = true;
    assert_eq!(s.apply_action(4), Err(Error::InvalidAction));
    let t = emit("road-building-no-edge-required", s, &[A(1)]);
    assert_eq!(t.phase, phase::PRE_ROLL);
    assert_eq!(t.playable_development[0][2], 1);
    assert_eq!(t.spent_development[0][2], 0);
}
