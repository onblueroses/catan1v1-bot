//! Rule fixtures are original and use the shared topology, not another engine.
//!
//! The tests cover the rule ids of `../spec/SPEC.md`, B1 to I3. Each test
//! name starts with the ids it covers. The fixtures use the raw numbers of
//! `../spec/ENCODING.md`:
//!
//! - Phases: 0 setup settlement, 1 setup road, 2 before the roll, 3 dice,
//!   4 discard, 5 robber, 6 steal, 7 main, 8 development draw, 9 road
//!   building, 10 year of plenty, 11 monopoly, 12 trade take, 13 terminal.
//! - Actions: 0 roll, 1 end turn, 2 buy a card, 3 to 6 play knight, road
//!   building, year of plenty, monopoly, `7 + edge` road, `79 + node`
//!   settlement, `133 + node` city, `187 + hex` robber, `206 + r` discard,
//!   `211 + r` choose, `216 + r` trade give, `221 + r` trade take.
//! - Outcomes: `total - 2` dice, `11 + r` steal, `16 + type` development draw.
//! - Resources `r`: 0 wood, 1 brick, 2 sheep, 3 wheat, 4 ore.
//! - Node occupancy: 1 or 2 settlement, 3 or 4 city, of player 0 or 1.
//!
//! `act` applies an action through both the pure and the sparse path and
//! requires them to agree.
use catan_engine::*;

fn board() -> Board {
    Board {
        terrain: [0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5],
        number: [2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12, 0],
        port_type: [5, 5, 5, 5, 0, 1, 2, 3, 4],
        port_edges: Board::default_port_edges(),
    }
}
fn initial() -> State {
    State::new(board(), Config::default()).unwrap()
}
fn main_state() -> State {
    let mut s = initial();
    s.phase = 7;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.recompute_caches();
    s
}
fn legal(s: &State) -> Vec<u16> {
    let mut a = [0; 226];
    let n = s.legal_actions(&mut a);
    a[..n].to_vec()
}
fn act(s: State, a: u16) -> State {
    let expected = s
        .apply_action(a)
        .unwrap_or_else(|e| panic!("action {a}, phase {}: {e:?}", s.phase));
    let mut current = s;
    let record = current.make_action(a).unwrap();
    assert_eq!(current, expected, "sparse action {a}, phase {}", s.phase);
    current.unmake(record);
    assert_eq!(current, s, "sparse restoration {a}, phase {}", s.phase);
    expected
}
fn give(s: &mut State, p: usize, r: usize, n: u8) {
    s.resources[p][r] += n;
    s.bank[r] -= n;
    s.recompute_caches();
}
fn card(s: &mut State, p: usize, t: usize, n: u8) {
    s.playable_development[p][t] += n;
    s.development_deck[t] -= n;
    s.recompute_caches();
}
fn roll(mut s: State, total: u16) -> State {
    s.config.dice = 1;
    s.phase = 2;
    act(s, 0).apply_outcome(total - 2).unwrap()
}
fn road(s: &mut State, p: usize, edges: &[usize]) {
    for &e in edges {
        s.edge_occupancy[e] = p as u8 + 1;
    }
    s.recompute_caches();
}

#[test]
fn b1_b2_b4_standard_board_and_initial_stock() {
    let s = initial();
    assert_eq!(s.node_occupancy.len(), 54);
    assert_eq!(s.edge_occupancy.len(), 72);
    assert_eq!(s.board.terrain.len(), 19);
    assert_eq!(s.robber_hex, 18);
    assert_eq!(s.bank, [19; 5]);
    assert_eq!(s.development_deck, [14, 5, 2, 2, 2]);
    assert_eq!(s.phase, 0);
    let mut b = board();
    b.terrain[0] = 5;
    assert!(State::new(b, Config::default()).is_err());
    let mut b = board();
    b.number[0] = 7;
    assert!(State::new(b, Config::default()).is_err());
    let mut b = board();
    b.port_type[0] = 0;
    assert!(State::new(b, Config::default()).is_err());
}
#[test]
fn b3_arbitrary_valid_assignments_are_input_data() {
    let mut b = board();
    b.terrain.swap(0, 18);
    b.number.swap(0, 18);
    b.port_type.reverse();
    let s = State::new(b, Config::default()).unwrap();
    assert_eq!(s.robber_hex, 0);
    assert_eq!(s.board.number, b.number);
}
#[test]
fn s1_s2_s3_s4_snake_distance_second_resources_and_first_turn() {
    for first in 0..2 {
        let mut c = Config::default();
        c.first_player = first;
        let mut s = State::new(board(), c).unwrap();
        for (pair, (node, edge)) in [(0, 0), (2, 5), (16, 23), (47, 66)].into_iter().enumerate() {
            assert_eq!(s.turn_owner, [first, 1 - first, 1 - first, first][pair]);
            let p = s.turn_owner as usize;
            let before = s.resources[p];
            s = act(s, 79 + node);
            assert_eq!(s.phase, 1);
            assert!(legal(&s).iter().all(|a| (7..79).contains(a)));
            if pair < 2 {
                assert_eq!(s.resources[p], before);
            } else {
                assert!(s.resources[p].iter().sum::<u8>() > before.iter().sum::<u8>());
            }
            s = act(s, 7 + edge);
            if pair == 0 {
                assert!(!legal(&s).contains(&82));
                assert!(!legal(&s).contains(&83));
            }
        }
        assert_eq!(s.phase, 2);
        assert_eq!(s.turn_owner, first);
        assert_eq!(s.turn_number, 1);
    }
}
#[test]
fn t1_t2_t6_t7_t8_phase_boundaries_and_no_timer() {
    let mut s = main_state();
    s.phase = 2;
    give(&mut s, 0, 0, 4);
    assert_eq!(legal(&s), vec![0]);
    assert!(s.apply_action(1).is_err());
    assert!(s.apply_action(216).is_err());
    let d = act(s, 0);
    assert_eq!(d.phase, 3);
    assert!(d.apply_action(0).is_err());
    let s = d.apply_outcome(0).unwrap();
    assert!(legal(&s).contains(&1));
    assert!(!legal(&s).contains(&0));
    let n = act(s, 1);
    assert_eq!(n.turn_owner, 1);
    assert_eq!(n.turn_number, 2);
    assert_eq!(n.phase, 2);
    assert!(n.apply_action(226).is_err());
    assert!(n.apply_action(u16::MAX).is_err());
}
#[test]
fn t3_r4_production_settlement_city_and_robber() {
    let mut s = main_state();
    s.node_occupancy[0] = 1;
    s.node_occupancy[8] = 4;
    s.recompute_caches();
    let out = roll(s, 2);
    assert_eq!(out.resources[0][0], 1);
    assert_eq!(out.resources[1][0], 2);
    s.robber_hex = 0;
    s.recompute_caches();
    let out = roll(s, 2);
    assert_eq!(out.resources, [[0; 5]; 2]);
}
#[test]
fn t4_shortage_single_player_partial_both_players_none_and_exact_supply() {
    let mut s = main_state();
    s.node_occupancy[0] = 3;
    give(&mut s, 1, 0, 18);
    s.recompute_caches();
    assert_eq!(roll(s, 2).resources[0][0], 1);
    s.node_occupancy[8] = 2;
    s.recompute_caches();
    let out = roll(s, 2);
    assert_eq!(out.resources, s.resources);
    assert_eq!(out.bank[0], 1);
    let mut s = main_state();
    s.node_occupancy[0] = 1;
    s.node_occupancy[8] = 4;
    give(&mut s, 0, 0, 16);
    let out = roll(s, 2);
    assert_eq!(out.resources[0][0], 17);
    assert_eq!(out.resources[1][0], 2);
    assert_eq!(out.bank[0], 0);
}
#[test]
fn t5_discard_nine_ten_and_floor_half_in_roller_order() {
    let mut s = main_state();
    give(&mut s, 0, 0, 9);
    give(&mut s, 1, 1, 10);
    let mut s = roll(s, 7);
    assert_eq!(s.phase, 4);
    assert_eq!(s.actor(), Some(1));
    assert_eq!(s.discard_remaining, [0, 5]);
    for _ in 0..5 {
        s = act(s, 207);
    }
    assert_eq!(s.phase, 5);
    assert_eq!(s.resources[0][0], 9);
    assert_eq!(s.resources[1][1], 5);
    let mut s = main_state();
    give(&mut s, 0, 0, 11);
    give(&mut s, 1, 1, 11);
    let mut s = roll(s, 7);
    assert_eq!(s.actor(), Some(0));
    for _ in 0..5 {
        s = act(s, 206);
    }
    assert_eq!(s.actor(), Some(1));
    for _ in 0..5 {
        s = act(s, 207);
    }
    assert_eq!(s.resources[0][0], 6);
    assert_eq!(s.resources[1][1], 6);
}
#[test]
fn u1_paid_costs_return_to_bank() {
    let mut s = main_state();
    s.node_occupancy[0] = 1;
    for r in 0..5 {
        give(&mut s, 0, r, 3);
    }
    let s = act(s, 7);
    assert_eq!(s.resources[0], [2, 2, 3, 3, 3]);
    assert_eq!(s.bank, [17, 17, 16, 16, 16]);
    let s = act(s, 133);
    assert_eq!(s.resources[0], [2, 2, 3, 1, 0]);
    assert_eq!(s.node_occupancy[0], 3);
    let mut s = main_state();
    road(&mut s, 0, &[0]);
    for r in 0..4 {
        give(&mut s, 0, r, 1);
    }
    let s = act(s, 79);
    assert_eq!(s.resources[0], [0; 5]);
    assert_eq!(s.bank, [19; 5]);
    let mut s = main_state();
    for r in 2..5 {
        give(&mut s, 0, r, 1);
    }
    let s = act(s, 2);
    assert_eq!(s.phase, 8);
    assert_eq!(s.bank, [19; 5]);
}
#[test]
fn u2_piece_limits_and_city_returns_settlement() {
    let mut s = main_state();
    for n in [0, 2, 16, 50, 52] {
        s.node_occupancy[n] = 1;
    }
    road(
        &mut s,
        0,
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],
    );
    for r in 0..5 {
        give(&mut s, 0, r, 4);
    }
    s.recompute_caches();
    assert_eq!(s.cache.pieces_left[0], [0, 0, 4]);
    assert!(!legal(&s).iter().any(|a| (7..133).contains(a)));
    let s = act(s, 133);
    assert_eq!(s.cache.pieces_left[0], [0, 1, 3]);
    let mut s = main_state();
    for n in [0, 2, 16, 50] {
        s.node_occupancy[n] = 3;
    }
    s.node_occupancy[52] = 1;
    give(&mut s, 0, 3, 2);
    give(&mut s, 0, 4, 3);
    assert!(s.apply_action(185).is_err());
}
#[test]
fn u3_road_connection_blocked_opponent_and_own_buildings() {
    let mut s = main_state();
    road(&mut s, 0, &[0]);
    give(&mut s, 0, 0, 1);
    give(&mut s, 0, 1, 1);
    assert!(legal(&s).contains(&8));
    assert!(!legal(&s).contains(&78));
    s.node_occupancy[0] = 2;
    s.recompute_caches();
    assert!(!legal(&s).contains(&8));
    s.node_occupancy[0] = 4;
    s.recompute_caches();
    assert!(!legal(&s).contains(&8));
    s.node_occupancy[0] = 1;
    s.recompute_caches();
    assert!(legal(&s).contains(&8));
    assert!(s.apply_action(7).is_err());
}
#[test]
fn u4_u5_settlement_requires_road_distance_and_city_ownership() {
    let mut s = main_state();
    for r in 0..5 {
        give(&mut s, 0, r, 3);
    }
    assert!(s.apply_action(79).is_err());
    road(&mut s, 0, &[0]);
    assert!(legal(&s).contains(&79));
    s.node_occupancy[3] = 2;
    s.recompute_caches();
    assert!(s.apply_action(79).is_err());
    assert!(s.apply_action(136).is_err());
    s.node_occupancy[3] = 1;
    s.recompute_caches();
    assert!(s.apply_action(136).is_ok());
    s.node_occupancy[3] = 3;
    s.recompute_caches();
    assert!(s.apply_action(136).is_err());
}
#[test]
fn x1_x2_best_ports_atomic_trade_and_empty_requested_bank() {
    let mut s = main_state();
    give(&mut s, 0, 0, 4);
    let t = act(s, 216);
    assert_eq!(t.resources, s.resources);
    assert_eq!(t.bank, s.bank);
    assert!(t.apply_action(221).is_err());
    assert!(t.apply_action(1).is_err());
    let t = act(t, 222);
    assert_eq!(t.resources[0], [0, 1, 0, 0, 0]);
    s.node_occupancy[1] = 1;
    s.recompute_caches();
    assert_eq!(s.cache.trade_rates[0], [3; 5]);
    s.node_occupancy[49] = 3;
    s.recompute_caches();
    assert_eq!(s.cache.trade_rates[0], [2, 3, 3, 3, 3]);
    give(&mut s, 1, 1, 19);
    let t = act(s, 216);
    assert!(t.apply_action(222).is_err());
}
#[test]
fn d1_d2_d3_new_cards_age_and_one_card_per_turn() {
    let mut s = main_state();
    for r in 2..5 {
        give(&mut s, 0, r, 1);
    }
    s = act(s, 2).apply_outcome(16).unwrap();
    assert_eq!(s.new_development[0][0], 1);
    assert!(s.apply_action(3).is_err());
    s = act(s, 1);
    s = roll(s, 2);
    s = act(s, 1);
    assert_eq!(s.playable_development[0][0], 1);
    card(&mut s, 0, 4, 1);
    s = act(s, 6);
    s = act(s, 211);
    assert_eq!(s.phase, 2);
    assert!(s.apply_action(3).is_err());
    s = roll(s, 2);
    assert!(s.apply_action(3).is_err());
}
#[test]
fn d1_empty_development_deck_cannot_buy() {
    let mut s = main_state();
    s.playable_development[1] = s.development_deck;
    s.development_deck = [0; 5];
    for r in 2..5 {
        give(&mut s, 0, r, 1);
    }
    assert!(!legal(&s).contains(&2));
    assert!(s.apply_action(2).is_err());
}
#[test]
fn d4_knight_before_roll_does_not_discard_and_returns_before_roll() {
    let mut s = main_state();
    s.phase = 2;
    card(&mut s, 0, 0, 1);
    give(&mut s, 0, 0, 10);
    let s = act(s, 3);
    assert_eq!(s.phase, 5);
    assert_eq!(s.discard_remaining, [0, 0]);
    assert_eq!(s.spent_development[0][0], 1);
    let s = act(s, 187);
    assert_eq!(s.phase, 2);
    assert_eq!(s.resources[0][0], 10);
    assert_eq!(s.resume_phase, 255);
}
#[test]
fn d5_free_roads_mandatory_two_and_zero_legal_edges_default() {
    let mut s = main_state();
    s.phase = 2;
    s.node_occupancy[0] = 1;
    card(&mut s, 0, 2, 1);
    let s = act(s, 4);
    assert_eq!(s.phase, 9);
    assert!(s.apply_action(1).is_err());
    let s = act(s, 7);
    assert_eq!(s.phase, 9);
    let s = act(s, 8);
    assert_eq!(s.phase, 2);
    assert_eq!(s.resources[0], [0; 5]);
    assert_eq!(s.choices_remaining, 0);
    let mut s = main_state();
    card(&mut s, 0, 2, 1);
    let s = act(s, 4);
    assert_eq!(s.phase, 7);
    assert_eq!(s.spent_development[0][2], 1);
}
#[test]
fn d5_no_road_piece_forbids_card_in_both_play_phases() {
    for phase in [phase::PRE_ROLL, phase::MAIN] {
        for owner in 0..2 {
            for needs_edge in [false, true] {
                let mut s = main_state();
                s.phase = phase;
                s.turn_owner = owner;
                s.config.road_building_needs_legal_edge = needs_edge;
                road(&mut s, owner as usize, &(0..15).collect::<Vec<_>>());
                card(&mut s, owner as usize, 2, 1);
                assert_eq!(s.cache.pieces_left[owner as usize][0], 0);
                let before = s;
                assert!(!s.is_legal_action(4), "phase {phase}, owner {owner}");
                assert!(!legal(&s).contains(&4));
                assert_eq!(s.apply_action(4), Err(Error::InvalidAction));
                assert_eq!(s, before);
            }
        }
    }
}
#[test]
fn d5_no_legal_edge_obeys_config_and_connected_edge_needs_no_resources() {
    for phase in [phase::PRE_ROLL, phase::MAIN] {
        for owner in 0..2 {
            for needs_edge in [false, true] {
                let mut s = main_state();
                s.phase = phase;
                s.turn_owner = owner;
                s.config.road_building_needs_legal_edge = needs_edge;
                card(&mut s, owner as usize, 2, 1);
                assert_eq!(s.cache.pieces_left[owner as usize][0], 15);
                assert!(!(0..72).any(|e| s.road_legal(e)));
                assert_eq!(s.is_legal_action(4), !needs_edge);
                assert_eq!(legal(&s).contains(&4), !needs_edge);
                if needs_edge {
                    assert_eq!(s.apply_action(4), Err(Error::InvalidAction));
                    assert_eq!(s.playable_development[owner as usize][2], 1);
                    assert!(!s.development_played);
                } else {
                    let after = act(s, 4);
                    assert_eq!(after.phase, phase);
                    assert_eq!(after.resume_phase, NONE);
                    assert_eq!(after.choices_remaining, 0);
                    assert_eq!(after.edge_occupancy, s.edge_occupancy);
                    assert_eq!(after.playable_development[owner as usize][2], 0);
                    assert_eq!(after.spent_development[owner as usize][2], 1);
                    assert!(after.development_played);
                }
                // A free road needs connectivity and stock, but no paid-road resources.
                s.node_occupancy[0] = owner + 1;
                s.recompute_caches();
                assert_eq!(s.resources[owner as usize], [0; 5]);
                assert!(s.is_legal_action(4));
                assert!(legal(&s).contains(&4));
                let after = act(act(s, 4), 7);
                assert_eq!(after.edge_occupancy[0], owner + 1);
                assert_eq!(after.phase, phase::ROAD_BUILDING);
            }
        }
    }
}
#[test]
fn d6_year_of_plenty_bank_limit_and_monopoly_all_cards() {
    let mut s = main_state();
    s.phase = 2;
    card(&mut s, 0, 3, 1);
    for r in 0..5 {
        give(&mut s, 1, r, 19);
    }
    s.resources[1][4] -= 1;
    s.bank[4] = 1;
    s.recompute_caches();
    let s = act(s, 5);
    assert_eq!(legal(&s), vec![215]);
    let s = act(s, 215);
    assert_eq!(s.phase, 2);
    assert_eq!(s.resources[0][4], 1);
    let mut s = main_state();
    card(&mut s, 0, 3, 1);
    for r in 0..5 {
        give(&mut s, 1, r, 19);
    }
    let s = act(s, 5);
    assert_eq!(s.phase, 7);
    let mut s = main_state();
    card(&mut s, 0, 4, 1);
    give(&mut s, 1, 2, 7);
    let s = act(act(s, 6), 213);
    assert_eq!(s.resources[0][2], 7);
    assert_eq!(s.resources[1][2], 0);
    assert_eq!(s.bank[2], 12);
}
#[test]
fn d7_a4_hidden_vp_purchase_wins_immediately() {
    let mut s = main_state();
    s.config.victory_points = 3;
    s.node_occupancy[0] = 3;
    for r in 2..5 {
        give(&mut s, 0, r, 1);
    }
    let s = act(s, 2).apply_outcome(17).unwrap();
    assert_eq!(s.phase, 13);
    assert_eq!(s.winner, 0);
    assert_eq!(s.cache.visible_points[0], 2);
    assert_eq!(s.cache.total_points[0], 3);
    assert!(legal(&s).is_empty());
    assert!(s.apply_action(1).is_err());
    assert!(s.apply_outcome(17).is_err());
}
#[test]
fn r1_r2_friendly_robber_protects_both_players_visible_two_not_hidden() {
    for p in 0..2 {
        for city in [false, true] {
            let mut s = main_state();
            s.phase = 5;
            s.resume_phase = 7;
            s.node_occupancy[0] = 1 + p as u8 + if city { 2 } else { 0 };
            if !city {
                s.node_occupancy[50] = 1 + p as u8;
            }
            card(&mut s, p, 1, 2);
            s.recompute_caches();
            assert_eq!(s.cache.visible_points[p], 2);
            assert!(!legal(&s).contains(&187));
            s.node_occupancy[52] = 1 + p as u8;
            s.recompute_caches();
            assert!(legal(&s).contains(&187));
            assert!(!legal(&s).contains(&(187 + 18)));
        }
    }
}
#[test]
fn r3_steal_weights_are_per_card_and_empty_hand_skips() {
    let mut s = main_state();
    s.config.friendly_robber = false;
    s.phase = 5;
    s.resume_phase = 7;
    s.node_occupancy[0] = 2;
    give(&mut s, 1, 0, 2);
    give(&mut s, 1, 4, 1);
    let s = act(s, 187);
    assert_eq!(s.phase, 6);
    let mut o = [Outcome::default(); 11];
    let n = s.chance_outcomes(&mut o);
    assert_eq!(n, 2);
    assert_eq!(o[0].id, 11);
    assert_eq!(o[0].weight * 3, o[0].total * 2);
    let t = s.apply_outcome(15).unwrap();
    assert_eq!(t.resources[0][4], 1);
    assert_eq!(t.resources[1][4], 0);
    assert_eq!(t.phase, 7);
    let mut s = main_state();
    s.config.friendly_robber = false;
    s.phase = 5;
    s.resume_phase = 7;
    s.node_occupancy[0] = 2;
    assert_eq!(act(s, 187).phase, 7);
}
#[test]
fn a1_largest_army_threshold_tie_and_strict_transfer() {
    let mut s = main_state();
    s.spent_development[0][0] = 2;
    s.development_deck[0] -= 2;
    card(&mut s, 0, 0, 1);
    let mut s = act(s, 3);
    assert_eq!(s.largest_army_holder, 0);
    s.spent_development[1][0] = 3;
    s.development_deck[0] -= 3;
    s.normalize_awards();
    assert_eq!(s.largest_army_holder, 0);
    s.spent_development[1][0] += 1;
    s.development_deck[0] -= 1;
    s.normalize_awards();
    assert_eq!(s.largest_army_holder, 1);
}
#[test]
fn a2_a3_longest_trails_cycles_cuts_disconnected_and_ties() {
    let mut s = main_state();
    road(&mut s, 0, &[1, 7, 12, 11, 6, 0]);
    s.normalize_awards();
    assert_eq!(s.cache.road_lengths[0], 6);
    assert_eq!(s.longest_road_holder, 0);
    road(&mut s, 0, &[2, 3]);
    s.normalize_awards();
    assert_eq!(s.cache.road_lengths[0], 8);
    s.node_occupancy[4] = 2;
    s.normalize_awards();
    assert_eq!(s.cache.road_lengths[0], 6);
    s.node_occupancy[12] = 2;
    s.normalize_awards();
    assert_eq!(s.cache.road_lengths[0], 4);
    assert_eq!(s.longest_road_holder, 255);
    let mut s = main_state();
    road(&mut s, 0, &[0, 6, 10, 65, 71, 70]);
    s.normalize_awards();
    assert_eq!(s.cache.road_lengths[0], 3);
    let mut s = main_state();
    road(&mut s, 0, &[0, 1, 2, 3, 4]);
    road(&mut s, 1, &[66, 67, 68, 69, 70]);
    s.normalize_awards();
    assert_eq!(s.longest_road_holder, 255);
    s.longest_road_holder = 0;
    s.normalize_awards();
    assert_eq!(s.longest_road_holder, 0);
    road(&mut s, 1, &[71]);
    s.normalize_awards();
    assert_eq!(s.longest_road_holder, 1);
}
#[test]
fn a4_winning_knight_interrupts_pending_robber() {
    let mut s = main_state();
    s.phase = 2;
    s.config.victory_points = 3;
    s.node_occupancy[0] = 1;
    s.spent_development[0][0] = 2;
    s.development_deck[0] -= 2;
    card(&mut s, 0, 0, 1);
    let s = act(s, 3);
    assert_eq!(s.phase, 13);
    assert_eq!(s.winner, 0);
    assert_eq!(s.resume_phase, 255);
    assert_eq!(s.robber_hex, 18);
}
#[test]
fn c1_c2_c3_independent_distribution_and_public_balanced_history() {
    let mut s = main_state();
    s.phase = 2;
    s.config.dice = 1;
    let s = act(s, 0);
    let mut o = [Outcome::default(); 11];
    let n = s.chance_outcomes(&mut o);
    assert_eq!(n, 11);
    for (i, count) in [1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1].into_iter().enumerate() {
        assert_eq!(o[i].weight * 36, o[i].total * count);
    }
    let mut s = main_state();
    s.phase = 2;
    let s = act(s, 0).apply_outcome(0).unwrap();
    assert_eq!(s.last_dice, 2);
    assert_eq!(s.canonical_bytes()[270], 0);
    assert_eq!(s.canonical_bytes()[282], 2);
}
#[test]
fn i1_i2_i3_observation_hides_types_preserves_public_and_samples() {
    let mut s = main_state();
    s.turn_owner = 1;
    card(&mut s, 0, 1, 1);
    card(&mut s, 1, 0, 2);
    s.new_development[1][3] = 1;
    s.development_deck[3] -= 1;
    give(&mut s, 1, 2, 4);
    let obs = s.observation(0).unwrap();
    assert_eq!(obs.opponent_playable_total, 2);
    assert_eq!(obs.opponent_new_total, 1);
    assert_eq!(obs.public_state.resources, s.resources);
    assert_eq!(
        obs.public_state.playable_development[0],
        s.playable_development[0]
    );
    assert_eq!(obs.public_state.playable_development[1], [0; 5]);
    let mut rng = SplitMix64::new(1);
    let sample = obs.sample(&mut rng).unwrap();
    assert_eq!(sample.resources, s.resources);
    assert_eq!(sample.playable_development[1].iter().sum::<u8>(), 2);
    assert_eq!(sample.new_development[1].iter().sum::<u8>(), 1);
    assert_eq!(sample.playable_development[0], s.playable_development[0]);
    for t in 0..5 {
        assert_eq!(
            sample.development_deck[t]
                + sample.playable_development[0][t]
                + sample.playable_development[1][t]
                + sample.new_development[0][t]
                + sample.new_development[1][t]
                + sample.spent_development[0][t]
                + sample.spent_development[1][t],
            [14, 5, 2, 2, 2][t]
        );
    }
}
#[test]
fn d5_one_piece_left_finishes_after_one_free_road() {
    for phase in [phase::PRE_ROLL, phase::MAIN] {
        for owner in 0..2 {
            for needs_edge in [false, true] {
                let mut s = main_state();
                s.phase = phase;
                s.turn_owner = owner;
                s.config.road_building_needs_legal_edge = needs_edge;
                road(
                    &mut s,
                    owner as usize,
                    &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
                );
                card(&mut s, owner as usize, 2, 1);
                assert!(s.is_legal_action(4));
                assert!(legal(&s).contains(&4));
                let s = act(s, 4);
                let a = legal(&s)[0];
                let s = act(s, a);
                assert_eq!(s.phase, phase);
                assert_eq!(s.cache.pieces_left[owner as usize][0], 0);
                assert_eq!(s.choices_remaining, 0);
                assert_eq!(s.spent_development[owner as usize][2], 1);
            }
        }
    }
}
#[test]
fn a4_first_free_road_can_win_without_second_road() {
    let mut s = main_state();
    s.config.victory_points = 3;
    s.node_occupancy[0] = 1;
    road(&mut s, 0, &[0, 1, 2, 3]);
    card(&mut s, 0, 2, 1);
    let s = act(act(s, 4), 11);
    assert_eq!(s.phase, 13);
    assert_eq!(s.winner, 0);
    assert_eq!(s.cache.road_lengths[0], 5);
    assert_eq!(s.cache.pieces_left[0][0], 10);
    assert_eq!(s.choices_remaining, 0);
}
#[test]
fn i3_samples_condition_on_nonterminal_and_terminal_victory() {
    let mut s = main_state();
    s.turn_owner = 1;
    s.config.victory_points = 3;
    s.node_occupancy[0] = 4;
    card(&mut s, 1, 0, 1);
    let obs = s.observation(0).unwrap();
    let mut rng = SplitMix64::new(49);
    // One sample per unseen card is enough to exercise different assignments here.
    // These assertions are logical invariants, not a frequency-fit claim.
    for _ in 0..obs.deck_total + obs.opponent_playable_total + obs.opponent_new_total {
        let t = obs.sample(&mut rng).unwrap();
        assert_eq!(t.phase, 7);
        assert_eq!(t.playable_development[1][1], 0);
        assert_eq!(t.new_development[1][1], 0);
    }
    let mut s = main_state();
    s.turn_owner = 1;
    s.config.victory_points = 3;
    s.node_occupancy[0] = 4;
    card(&mut s, 1, 1, 1);
    s.phase = 13;
    s.winner = 1;
    let obs = s.observation(0).unwrap();
    for _ in 0..obs.deck_total + obs.opponent_playable_total + obs.opponent_new_total {
        let t = obs.sample(&mut rng).unwrap();
        assert_eq!(t.phase, 13);
        assert_eq!(t.winner, 1);
        assert_eq!(t.playable_development[1][1], 1);
    }
}
#[test]
fn i2_i3_reveal_all_is_identity_and_hidden_vp_toggle() {
    let mut s = main_state();
    card(&mut s, 1, 1, 1);
    card(&mut s, 1, 0, 1);
    s.config.observation_rule = 1;
    let obs = s.observation(0).unwrap();
    let mut rng = SplitMix64::new(5);
    assert_eq!(
        obs.sample(&mut rng).unwrap().canonical_bytes(),
        s.canonical_bytes()
    );
    s.config.observation_rule = 0;
    s.config.vp_cards_hidden = false;
    let obs = s.observation(0).unwrap();
    assert_eq!(obs.public_state.playable_development[1][1], 1);
    assert_eq!(obs.public_state.playable_development[1][0], 0);
    let t = obs.sample(&mut rng).unwrap();
    assert_eq!(t.playable_development[1][1], 1);
}
#[test]
fn a4_opponent_award_waits_until_own_turn() {
    let mut s = main_state();
    s.config.victory_points = 3;
    s.node_occupancy[50] = 2;
    road(&mut s, 1, &[0, 1, 2, 3, 4]);
    s.normalize_awards();
    assert_eq!(s.cache.total_points[1], 3);
    assert_eq!(s.phase, 7);
    assert_eq!(s.winner, 255);
    let s = act(s, 1);
    assert_eq!(s.turn_owner, 1);
    assert_eq!(s.phase, 13);
    assert_eq!(s.winner, 1);
}
#[test]
fn r3_certain_steal_remains_explicit_chance() {
    let mut s = main_state();
    s.config.friendly_robber = false;
    s.phase = 5;
    s.resume_phase = 7;
    s.node_occupancy[0] = 2;
    give(&mut s, 1, 0, 1);
    let s = act(s, 187);
    assert_eq!(s.phase, 6);
    let mut outcomes = [Outcome::default(); 11];
    assert_eq!(s.chance_outcomes(&mut outcomes), 1);
    assert_eq!(outcomes[0].id, 11);
    assert_eq!(outcomes[0].weight, outcomes[0].total);
    assert_eq!(s.resources[0][0], 0);
    assert_eq!(s.apply_outcome(11).unwrap().resources[0][0], 1);
}
#[test]
fn e7_pending_phases_reject_impossible_pre_roll_resume() {
    let mut draw = main_state();
    for r in 2..5 {
        give(&mut draw, 0, r, 1);
    }
    draw = act(draw, 2);
    let mut discard = main_state();
    give(&mut discard, 0, 0, 10);
    discard = roll(discard, 7);
    let mut trade = main_state();
    give(&mut trade, 0, 0, 4);
    trade = act(trade, 216);
    for state in [draw, discard, trade] {
        let mut bytes = state.canonical_bytes();
        assert!(State::from_canonical_bytes(&bytes).is_ok());
        bytes[251] = 2;
        assert!(
            State::from_canonical_bytes(&bytes).is_err(),
            "phase {} cannot resume PRE_ROLL",
            state.phase
        );
    }
}
#[test]
fn i3_pre_roll_samples_respect_outgoing_players_end_turn_win_check() {
    let mut s = main_state();
    s.phase = phase::PRE_ROLL;
    s.turn_number = 2;
    s.config.victory_points = 3;
    s.node_occupancy[0] = 4;
    card(&mut s, 1, 0, 1);
    let observation = s.observation(0).unwrap();
    let mut rng = SplitMix64::new(87);
    for _ in 0..observation.deck_total + observation.opponent_playable_total {
        let sampled = observation.sample(&mut rng).unwrap();
        assert_eq!(sampled.playable_development[1][1], 0);
        assert_eq!(sampled.phase, phase::PRE_ROLL);
    }
}
#[test]
fn i3_exact_small_unseen_multiset_probabilities_preserve_age_groups() {
    // Three distinguishable unseen cards (two knights and one VP), assigned to
    // one old slot, one new slot and one deck slot: six equally likely orders.
    // Enumerating every integer mass verifies the sampler without a fit threshold.
    struct Rank(u128);
    impl RandomSource for Rank {
        fn next_u64(&mut self) -> u64 {
            0
        }
        fn below_u128(&mut self, bound: u128) -> u128 {
            assert_eq!(bound, 6);
            self.0
        }
    }
    let mut s = main_state();
    s.turn_owner = 1;
    s.playable_development[0] = [12, 4, 2, 2, 2];
    s.playable_development[1][0] = 1;
    s.new_development[1][0] = 1;
    s.development_deck = [0, 1, 0, 0, 0];
    s.recompute_caches();
    let observation = s.observation(0).unwrap();
    let mut counts = [0; 3];
    for rank in 0..6 {
        let sampled = observation.sample(&mut Rank(rank)).unwrap();
        assert_eq!(sampled.playable_development[1].iter().sum::<u8>(), 1);
        assert_eq!(sampled.new_development[1].iter().sum::<u8>(), 1);
        if sampled.playable_development[1][1] == 1 {
            counts[0] += 1;
        } else if sampled.new_development[1][1] == 1 {
            counts[1] += 1;
        } else {
            assert_eq!(sampled.development_deck[1], 1);
            counts[2] += 1;
        }
    }
    assert_eq!(counts, [2, 2, 2]);
}
