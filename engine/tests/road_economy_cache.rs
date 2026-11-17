//! Road placements in setup, paid and free (S1, U1, U2, D5) must leave every
//! cache equal to a full rebuild, and must resolve Longest Road and an
//! immediate win correctly (A2 to A4).
use catan_engine::{phase::*, *};

const STRIP: [usize; 15] = [0, 6, 10, 18, 23, 33, 39, 49, 54, 62, 66, 67, 68, 69, 64];

fn ready(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    s
}

fn main_state(player: u8) -> State {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = MAIN;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.turn_owner = player;
    // A port city and an opponent settlement give nontrivial board caches.
    s.node_occupancy[0] = player + 3;
    s.node_occupancy[2] = 2 - player;
    s.resources[player as usize] = [3, 3, 1, 1, 1];
    for r in 0..5 {
        s.bank[r] -= s.resources[player as usize][r];
    }
    s.playable_development[player as usize][1] = 1;
    s.development_deck[1] -= 1;
    let s = ready(s);
    assert!(
        s.cache.trade_rates[player as usize]
            .iter()
            .any(|&rate| rate < 4)
    );
    assert!(
        s.cache
            .production
            .iter()
            .flatten()
            .flatten()
            .any(|&n| n > 0)
    );
    s
}

fn with_card(mut s: State, resume: u8) -> State {
    s.phase = resume;
    s.playable_development[s.turn_owner as usize][2] += 1;
    s.development_deck[2] -= 1;
    ready(s).apply_action(4).unwrap()
}

// The expected state specifies authoritative writes explicitly, then uses the
// independent public full rebuild. Compare all fields, not just road stock.
fn road(s: State, edge: usize, mut expected: State) -> State {
    let input = s;
    let p = s.turn_owner as usize;
    expected.edge_occupancy[edge] = s.turn_owner + 1;
    expected.recompute_caches();
    expected.normalize_awards();
    let next = s.apply_action(7 + edge as u16).unwrap();
    assert_eq!(s, input, "pure apply must preserve every input field");
    assert_eq!(next.canonical_bytes(), expected.canonical_bytes());
    assert_eq!(next, expected);
    assert_eq!(next.cache.pieces_left[p][0], s.cache.pieces_left[p][0] - 1);
    assert_eq!(next.cache.pieces_left[1 - p], s.cache.pieces_left[1 - p]);
    assert_eq!(next.cache.pieces_left[p][1..], s.cache.pieces_left[p][1..]);
    assert_eq!(next.cache.production, s.cache.production);
    assert_eq!(next.cache.trade_rates, s.cache.trade_rates);
    let mut rebuilt = next;
    rebuilt.recompute_caches();
    assert_eq!(next.cache, rebuilt.cache);
    next.validate().unwrap();
    next
}

fn paid(s: State, edge: usize) -> State {
    let mut expected = s;
    for r in 0..2 {
        expected.resources[s.turn_owner as usize][r] -= 1;
        expected.bank[r] += 1;
    }
    road(s, edge, expected)
}

#[test]
fn s1_s3_s4_setup_roads_preserve_economy_and_exact_snake_transitions() {
    for first in 0..2 {
        let mut s = State::new(
            Board::default(),
            Config {
                first_player: first,
                ..Config::default()
            },
        )
        .unwrap();
        for (pair, (node, edge)) in [(0, 0), (2, 5), (16, 23), (47, 66)].into_iter().enumerate() {
            s = s.apply_action(79 + node).unwrap();
            assert_eq!(s.phase, SETUP_ROAD);
            let mut expected = s;
            expected.setup_pairs_done += 1;
            expected.setup_anchor = NONE;
            if pair == 3 {
                expected.phase = PRE_ROLL;
                expected.turn_owner = first;
                expected.turn_number = 1;
            } else {
                expected.phase = SETUP_SETTLEMENT;
                expected.turn_owner = [first, 1 - first, 1 - first, first][pair + 1];
            }
            s = road(s, edge, expected);
        }
    }
}

#[test]
fn u1_u2_paid_roads_preserve_economy_through_last_piece_and_reject_zero_stock() {
    for player in 0..2 {
        let s = paid(main_state(player), STRIP[0]);
        assert_eq!(s.cache.pieces_left[player as usize][0], 14);
        let mut s = main_state(player);
        for &edge in &STRIP[..14] {
            s.edge_occupancy[edge] = player + 1;
        }
        let s = paid(ready(s), STRIP[14]);
        assert_eq!(s.cache.pieces_left[player as usize][0], 0);
        assert!((0..72).all(|edge| !s.road_legal(edge)));
        for action in 7..79 {
            assert_eq!(s.apply_action(action), Err(Error::InvalidAction));
        }
        assert_eq!(s.apply_action(4), Err(Error::InvalidAction));
    }
}

#[test]
fn d5_free_roads_continue_then_complete_in_both_resume_phases() {
    for player in 0..2 {
        for resume in [PRE_ROLL, MAIN] {
            let s = with_card(main_state(player), resume);
            assert_eq!(s.phase, ROAD_BUILDING);
            assert_eq!(s.choices_remaining, 2);
            let mut expected = s;
            expected.choices_remaining = 1;
            let s = road(s, STRIP[0], expected);
            assert_eq!(s.phase, ROAD_BUILDING);
            assert_eq!(s.choices_remaining, 1);
            assert_eq!(s.apply_action(1), Err(Error::InvalidAction));
            let mut expected = s;
            expected.phase = resume;
            expected.resume_phase = NONE;
            expected.choices_remaining = 0;
            let s = road(s, STRIP[1], expected);
            assert_eq!(s.phase, resume);
            assert!(s.development_played);
            assert_eq!(s.spent_development[player as usize][2], 1);
        }
    }
}

#[test]
fn d5_last_free_piece_completes_and_zero_stock_keeps_card_unspent() {
    for player in 0..2 {
        for resume in [PRE_ROLL, MAIN] {
            let mut s = main_state(player);
            for &edge in &STRIP[..14] {
                s.edge_occupancy[edge] = player + 1;
            }
            let s = with_card(ready(s), resume);
            let mut expected = s;
            expected.phase = resume;
            expected.resume_phase = NONE;
            expected.choices_remaining = 0;
            let mut s = road(s, STRIP[14], expected);
            assert_eq!(s.cache.pieces_left[player as usize][0], 0);
            s.development_played = false;
            s.playable_development[player as usize][2] += 1;
            s.development_deck[2] -= 1;
            let s = ready(s);
            assert_eq!(s.apply_action(4), Err(Error::InvalidAction));
            assert_eq!(s.playable_development[player as usize][2], 1);
        }
    }
}

#[test]
fn d5_first_free_road_completes_when_no_edge_remains_with_stock_left() {
    for player in 0..2 {
        for resume in [PRE_ROLL, MAIN] {
            let mut s = main_state(player);
            s.node_occupancy[7] = 2 - player;
            s.edge_occupancy[0] = player + 1;
            s.edge_occupancy[1] = 2 - player;
            let s = with_card(ready(s), resume);
            assert!(s.road_legal(6));
            let mut expected = s;
            expected.phase = resume;
            expected.resume_phase = NONE;
            expected.choices_remaining = 0;
            let s = road(s, 6, expected);
            assert_eq!(s.cache.pieces_left[player as usize][0], 13);
            assert!((0..72).all(|edge| !s.road_legal(edge)));
        }
    }
}

#[test]
fn a2_a3_road_award_acquisition_and_ties_keep_full_cache_equality() {
    for player in 0..2 {
        for rule in 0..2 {
            for incumbent in [NONE, player, 1 - player] {
                let mut s = main_state(player);
                s.config.longest_road_break_rule = rule;
                for &edge in &STRIP[..4] {
                    s.edge_occupancy[edge] = player + 1;
                }
                for edge in [66, 67, 68, 69, 70] {
                    s.edge_occupancy[edge] = 2 - player;
                }
                // A five/five tie can begin without a holder. To test each
                // incumbent with a valid input, player already has five and
                // adds a short branch at node 7 that does not extend the trail.
                if incumbent == player || incumbent == NONE {
                    s.edge_occupancy[STRIP[4]] = player + 1;
                    s.longest_road_holder = if rule == 0 { incumbent } else { NONE };
                    s.recompute_caches();
                    s.validate().unwrap();
                    let s = paid(s, 11);
                    let holder = if rule == 0 { incumbent } else { NONE };
                    assert_eq!(s.longest_road_holder, holder);
                    assert_eq!(s.cache.road_lengths, [5, 5]);
                } else {
                    let s = paid(ready(s), STRIP[4]);
                    assert_eq!(s.cache.road_lengths, [5, 5]);
                    assert_eq!(
                        s.longest_road_holder,
                        if rule == 0 { 1 - player } else { NONE }
                    );
                    let s = paid(s, STRIP[5]);
                    assert_eq!(s.cache.road_lengths[player as usize], 6);
                    assert_eq!(s.longest_road_holder, player);
                }
            }
            let mut s = main_state(player);
            s.config.longest_road_break_rule = rule;
            for &edge in &STRIP[..4] {
                s.edge_occupancy[edge] = player + 1;
            }
            let s = paid(ready(s), STRIP[4]);
            assert_eq!(s.longest_road_holder, player);
            assert_eq!(s.cache.visible_points[player as usize], 4);
            assert_eq!(s.cache.total_points[player as usize], 5);
        }
    }
}

#[test]
fn a4_d5_first_free_road_award_wins_before_second_choice() {
    for player in 0..2 {
        for resume in [PRE_ROLL, MAIN] {
            let mut s = main_state(player);
            s.config.victory_points = 5;
            for &edge in &STRIP[..4] {
                s.edge_occupancy[edge] = player + 1;
            }
            let s = with_card(ready(s), resume);
            let mut expected = s;
            expected.phase = TERMINAL;
            expected.winner = player;
            expected.resume_phase = NONE;
            expected.choices_remaining = 0;
            let s = road(s, STRIP[4], expected);
            assert_eq!(s.winner(), Some(player));
            assert_eq!(s.cache.pieces_left[player as usize][0], 10);
            assert_eq!(s.longest_road_holder, player);
            assert_eq!(s.spent_development[player as usize][2], 1);
        }
    }
}
