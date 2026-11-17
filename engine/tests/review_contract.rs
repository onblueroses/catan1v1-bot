//! Two contract checks from an independent review. D5:
//! a game played from the constructor reaches zero road stock, and Road
//! Building is then refused. E10: every successor of the seed-191 dead state
//! is dead.
use catan_engine::*;

fn action(s: State, a: u16, steps: &mut usize) -> State {
    let next = s.apply_action(a).unwrap();
    next.validate().unwrap();
    *steps += 1;
    next
}
fn outcome(s: State, o: u16, steps: &mut usize) -> State {
    let next = s.apply_outcome(o).unwrap();
    next.validate().unwrap();
    *steps += 1;
    next
}
fn roll(s: State, steps: &mut usize) -> State {
    let s = action(s, 0, steps);
    let mut os = [Outcome::default(); 11];
    let n = s.chance_outcomes(&mut os);
    // Prefer production from player 0's wood/brick hexes. An unavailable total
    // uses another positive non-seven outcome, preserving the actual dice law.
    let chosen = [1, 3, 2, 0, 4, 6, 7, 8, 9, 10]
        .into_iter()
        .find(|id| os[..n].iter().any(|o| o.id == *id))
        .unwrap();
    outcome(s, chosen, steps)
}
fn next_own_turn(s: State, steps: &mut usize) -> State {
    assert_eq!(s.turn_owner, 0);
    let s = action(s, 1, steps);
    let s = roll(s, steps);
    let s = action(s, 1, steps);
    roll(s, steps)
}
fn collect(mut s: State, wood: u8, brick: u8, steps: &mut usize) -> State {
    while s.resources[0][0] < wood || s.resources[0][1] < brick {
        s = next_own_turn(s, steps);
    }
    s
}
#[test]
fn spec_d5_zero_stock_card_is_rejected_in_a_constructor_reachable_position() {
    let mut steps = 0;
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    for (n, e) in [(8, 7), (47, 66), (50, 65), (14, 21)] {
        s = action(s, 79 + n, &mut steps);
        s = action(s, 7 + e, &mut steps);
    }
    s = roll(s, &mut steps);
    for take in 2..5 {
        s = collect(s, 4, 0, &mut steps);
        s = action(s, 216, &mut steps);
        s = action(s, 221 + take, &mut steps);
    }
    s = action(s, 2, &mut steps);
    s = outcome(s, 18, &mut steps);
    s = next_own_turn(s, &mut steps);
    assert_eq!(s.playable_development[0][2], 1);
    while s.cache.pieces_left[0][0] > 0 {
        s = collect(s, 1, 1, &mut steps);
        let mut legal = [0; 226];
        let n = s.legal_actions(&mut legal);
        let road = *legal[..n].iter().find(|&&a| (7..79).contains(&a)).unwrap();
        s = action(s, road, &mut steps);
    }
    assert!(!s.development_played);
    assert_eq!(s.cache.pieces_left[0][0], 0);
    let before = s.canonical_bytes();
    let mut legal = [0; ACTION_COUNT];
    let n = s.legal_actions(&mut legal);
    assert!(!legal[..n].contains(&4));
    assert!(matches!(s.apply_action(4), Err(Error::InvalidAction)));
    assert_eq!(s.canonical_bytes(), before);
    assert_eq!(s.playable_development[0][2], 1);
    assert_eq!(s.spent_development[0][2], 0);
    assert!(!s.development_played);
    assert_eq!(s.phase, phase::MAIN);
    println!(
        "D5: {steps} accepted constructor transitions; stock=0; PLAY_ROAD_BUILDING rejected; card and allowance preserved"
    );
}

#[test]
fn e10_dead_successors_and_independent_positive_controls() {
    let bytes = trace::decode_hex(
        include_str!("fixtures/dead-seed191.hex").trim(),
        CANONICAL_SIZE,
    )
    .unwrap();
    let s = State::from_canonical_bytes(&bytes).unwrap();
    assert!(s.is_dead());
    let mut actions = [0; 226];
    let n = s.legal_actions(&mut actions);
    assert!(n > 0);
    let mut checked = 0;
    for &a in &actions[..n] {
        let next = s.apply_action(a).unwrap();
        next.validate().unwrap();
        assert!(next.is_dead());
        assert_eq!(next.cache.total_points, [14, 14]);
        assert!(!next.is_terminal());
        checked += 1;
        let mut outcomes = [Outcome::default(); 11];
        let k = next.chance_outcomes(&mut outcomes);
        for o in &outcomes[..k] {
            let next = next.apply_outcome(o.id).unwrap();
            next.validate().unwrap();
            assert!(next.is_dead());
            assert_eq!(next.cache.total_points, [14, 14]);
            checked += 1;
        }
    }
    let mut control = s;
    let road = control.edge_occupancy.iter().position(|&x| x == 1).unwrap();
    control.edge_occupancy[road] = 0;
    control.recompute_caches();
    control.normalize_awards();
    control.validate().unwrap();
    assert!(!control.is_dead());
    let mut control = s;
    let city = control.node_occupancy.iter().position(|&x| x == 3).unwrap();
    control.node_occupancy[city] = 1;
    control.recompute_caches();
    control.normalize_awards();
    control.validate().unwrap();
    assert!(!control.is_dead());
    println!(
        "E10: {checked} legal successors preserve fixed points and dead; road and city controls reject dead"
    );
}
