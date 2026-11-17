//! Independent adversarial snapshots. Fixtures do not claim constructor reachability.
use catan_engine::{topology::*, *};

fn base() -> State {
    let mut s = State::new(
        Board::default(),
        Config {
            dice: 1,
            ..Config::default()
        },
    )
    .unwrap();
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.phase = phase::MAIN;
    s
}
fn rebuild(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    State::from_canonical_bytes(&s.canonical_bytes()).unwrap()
}
fn give(s: &mut State, p: usize, r: usize, count: u8) {
    s.resources[p][r] += count;
    s.bank[r] -= count;
}
fn card(s: &mut State, p: usize, t: usize) {
    s.playable_development[p][t] += 1;
    s.development_deck[t] -= 1;
}
fn actions(s: &State) -> Vec<u16> {
    let mut out = [0; ACTION_COUNT];
    let n = s.legal_actions(&mut out);
    out[..n].to_vec()
}
fn caches(s: &State) {
    s.validate().unwrap();
    let mut recomputed = *s;
    recomputed.recompute_caches();
    assert_eq!(s.cache, recomputed.cache);
    let mut production = [[[0; 5]; 2]; 11];
    for n in 0..54 {
        let x = s.node_occupancy[n];
        if x == 0 {
            continue;
        }
        for h in 0..19 {
            if HEX_NODES[h].contains(&(n as u8))
                && h != s.robber_hex as usize
                && s.board.terrain[h] != 5
            {
                production[s.board.number[h] as usize - 2][(x as usize - 1) % 2]
                    [s.board.terrain[h] as usize] += if x >= 3 { 2 } else { 1 };
            }
        }
    }
    assert_eq!(s.cache.production, production);
    // Legal enumeration is also checked after a fresh cache rebuild.
    assert_eq!(actions(s), actions(&recomputed));
}
fn act(s: State, action: u16) -> State {
    let s = s.apply_action(action).unwrap();
    caches(&s);
    s
}
fn out(s: State, outcome: u16) -> State {
    let s = s.apply_outcome(outcome).unwrap();
    caches(&s);
    s
}
fn roll(mut s: State, total: u16) -> State {
    s.phase = phase::PRE_ROLL;
    out(act(rebuild(s), 0), total - 2)
}

#[test]
fn u3_connection_at_own_building_without_a_road_and_opponent_cut() {
    for own in [1, 3] {
        let mut s = base();
        s.node_occupancy[0] = own;
        give(&mut s, 0, 0, 2);
        give(&mut s, 0, 1, 2);
        let s = rebuild(s);
        assert!(s.edge_occupancy.iter().all(|&x| x == 0));
        assert!(actions(&s).contains(&7) && actions(&s).contains(&8));
        let s = act(s, 7);
        assert_eq!(s.edge_occupancy[0], 1);
    }
    for opponent in [2, 4] {
        let mut s = base();
        s.node_occupancy[4] = opponent;
        s.edge_occupancy[1] = 1;
        give(&mut s, 0, 0, 2);
        give(&mut s, 0, 1, 2);
        let s = rebuild(s);
        assert!(!actions(&s).contains(&(7 + 2)));
        assert!(!actions(&s).contains(&(7 + 7)));
        assert!(actions(&s).contains(&7)); // The unblocked other endpoint counts.
    }
}

#[test]
fn s2_s3_u4_exact_second_grants_desert_bank_and_distance() {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    for (pair, (node, edge)) in [(0, 0), (2, 5), (49, 70), (8, 7)].into_iter().enumerate() {
        let p = s.turn_owner as usize;
        let old_bank = s.bank;
        s = act(s, 79 + node);
        let mut expected = [0; 5];
        if pair >= 2 {
            for &h in &NODE_HEXES[node as usize] {
                if h != NONE && s.board.terrain[h as usize] < 5 {
                    expected[s.board.terrain[h as usize] as usize] += 1;
                }
            }
        }
        assert_eq!(s.resources[p], expected);
        for r in 0..5 {
            assert_eq!(old_bank[r] - s.bank[r], expected[r]);
        }
        if pair == 2 {
            assert_eq!(expected, [0, 0, 0, 0, 1]);
        } // Adjacent desert grants nothing.
        if pair == 3 {
            assert_eq!(expected, [2, 1, 0, 0, 0]);
        }
        s = act(s, 7 + edge);
    }
    let mut s = base();
    s.edge_occupancy[0] = 1;
    for r in 0..4 {
        give(&mut s, 0, r, 1);
    }
    for occupant in [1, 2, 3, 4] {
        s.node_occupancy[3] = occupant;
        let s = rebuild(s);
        assert!(!actions(&s).contains(&79));
    }
}

#[test]
fn t4_shortage_city_demands_are_simultaneous_per_resource() {
    for (a, b, supply, expected) in [
        (3, 0, 1, [1, 0]),
        (3, 2, 2, [0, 0]),
        (3, 4, 4, [2, 2]),
        (1, 4, 3, [1, 2]),
        (3, 0, 0, [0, 0]),
    ] {
        let mut s = base();
        s.node_occupancy[0] = a;
        s.node_occupancy[8] = b;
        give(&mut s, 1, 0, 19 - supply);
        let before = s.resources;
        let s = roll(rebuild(s), 2);
        assert_eq!(
            [
                s.resources[0][0] - before[0][0],
                s.resources[1][0] - before[1][0]
            ],
            expected
        );
    }
}

#[test]
fn t5_both_discard_in_roller_order_then_robber_and_steal() {
    for roller in 0..2 {
        for (hand, discards, remaining) in [(9, 0, 9), (10, 5, 5), (11, 5, 6), (13, 6, 7)] {
            let mut s = base();
            s.turn_owner = roller;
            give(&mut s, 0, 0, hand);
            give(&mut s, 1, 1, hand);
            let mut s = roll(rebuild(s), 7);
            assert_eq!(s.discard_remaining, [discards; 2]);
            for p in [roller, 1 - roller] {
                for _ in 0..discards {
                    assert_eq!(s.actor(), Some(p));
                    assert!(!actions(&s).iter().any(|a| (187..206).contains(a)));
                    s = act(s, 206 + p as u16);
                }
            }
            assert_eq!(s.phase, phase::ROBBER);
            assert_eq!(s.resources[0][0], remaining);
            assert_eq!(s.resources[1][1], remaining);
            assert_eq!(s.turn_owner, roller);
        }
    }
}

#[test]
fn r2_awards_count_hidden_vps_do_not_knight_uses_updated_army() {
    for owner in 0..2 {
        let mut s = base();
        s.phase = phase::ROBBER;
        s.resume_phase = phase::MAIN;
        s.node_occupancy[0] = owner + 1;
        s.node_occupancy[50] = owner + 1;
        s.playable_development[owner as usize][1] = 3;
        s.development_deck[1] -= 3;
        let s = rebuild(s);
        assert_eq!(s.cache.visible_points[owner as usize], 2);
        assert!(!actions(&s).contains(&187));
        let mut s = s;
        s.spent_development[owner as usize][0] = 3;
        s.development_deck[0] -= 3;
        let s = rebuild(s);
        assert_eq!(s.cache.visible_points[owner as usize], 4);
        assert!(actions(&s).contains(&187));
    }
    let mut s = base();
    s.phase = phase::PRE_ROLL;
    s.node_occupancy[0] = 1;
    s.node_occupancy[50] = 1;
    s.spent_development[0][0] = 2;
    s.development_deck[0] -= 2;
    card(&mut s, 0, 0);
    let s = act(rebuild(s), 3);
    assert_eq!(s.largest_army_holder, 0);
    assert!(actions(&s).contains(&187));
}

#[test]
fn r3_no_occupant_no_theft_and_per_card_mass() {
    for cards in [0, 1, 7] {
        let mut s = base();
        s.config.friendly_robber = false;
        s.phase = phase::ROBBER;
        s.resume_phase = phase::MAIN;
        give(&mut s, 1, 0, cards);
        let s = act(rebuild(s), 187);
        assert_eq!(s.phase, phase::MAIN);
    }
    let mut s = base();
    s.config.friendly_robber = false;
    s.phase = phase::ROBBER;
    s.resume_phase = phase::MAIN;
    s.node_occupancy[0] = 4;
    give(&mut s, 1, 0, 7);
    give(&mut s, 1, 1, 2);
    give(&mut s, 1, 4, 1);
    let s = act(rebuild(s), 187);
    let mut outcomes = [Outcome::default(); 11];
    let n = s.chance_outcomes(&mut outcomes);
    assert_eq!(
        outcomes[..n]
            .iter()
            .map(|o| (o.id, o.weight, o.total))
            .collect::<Vec<_>>(),
        vec![(11, 7, 10), (12, 2, 10), (15, 1, 10)]
    );
    for id in [11, 12, 15] {
        assert_eq!(out(s, id).phase, phase::MAIN);
    }
}

#[test]
fn d2_d3_d4_preroll_knight_then_seven_is_a_second_robber_not_a_second_card() {
    let mut s = base();
    s.phase = phase::PRE_ROLL;
    s.config.friendly_robber = false;
    card(&mut s, 0, 0);
    card(&mut s, 0, 4);
    give(&mut s, 0, 0, 11);
    let s = act(act(rebuild(s), 3), 187);
    assert_eq!(s.phase, phase::PRE_ROLL);
    assert!(s.development_played);
    assert!(!actions(&s).contains(&6));
    let mut s = out(act(s, 0), 5);
    assert_eq!(s.discard_remaining, [5, 0]);
    for _ in 0..5 {
        s = act(s, 206);
    }
    assert_eq!(s.phase, phase::ROBBER);
    assert_eq!(s.robber_hex, 0);
    assert!(!actions(&s).contains(&187));
    let s = act(s, 188);
    assert_eq!(s.phase, phase::MAIN);
    assert!(!actions(&s).contains(&6));
    assert_eq!(s.spent_development[0][0], 1);
}

#[test]
fn d5_last_piece_zero_legal_edges_and_no_early_finish() {
    let mut s = base();
    // A connected 14-edge strip and branches. The fifteenth piece must end the effect.
    for e in [0, 1, 2, 3, 4, 5, 9, 16, 15, 14, 13, 12, 11, 6] {
        s.edge_occupancy[e] = 1;
    }
    card(&mut s, 0, 2);
    let s = act(rebuild(s), 4);
    assert_eq!(s.phase, phase::ROAD_BUILDING);
    assert!(s.apply_action(1).is_err());
    let s = act(s, actions(&s)[0]);
    assert_eq!(s.phase, phase::MAIN);
    assert_eq!(s.cache.pieces_left[0][0], 0);
    let mut s = base();
    s.node_occupancy[0] = 1;
    s.edge_occupancy[0] = 2;
    s.edge_occupancy[1] = 2;
    card(&mut s, 0, 2);
    let s = act(rebuild(s), 4);
    assert_eq!(s.phase, phase::MAIN);
    assert_eq!(s.spent_development[0][2], 1);
}

#[test]
fn d6_yop_last_bank_card_and_empty_monopoly_refresh_hands() {
    let mut s = base();
    for r in 0..5 {
        give(&mut s, 1, r, 19);
    }
    s.resources[1][0] -= 1;
    s.bank[0] += 1;
    card(&mut s, 0, 3);
    let s = act(rebuild(s), 5);
    assert_eq!(actions(&s), vec![211]);
    let s = act(s, 211);
    assert_eq!(s.phase, phase::MAIN);
    assert_eq!(s.cache.resource_totals, [1, 94]);
    for r in 0..5 {
        let mut s = base();
        card(&mut s, 0, 4);
        give(&mut s, 0, r, 2);
        let s = act(act(rebuild(s), 6), 211 + r as u16);
        assert_eq!(s.resources[0][r], 2);
        assert_eq!(s.resources[1][r], 0);
        assert_eq!(s.bank[r], 17);
        let mut s = base();
        card(&mut s, 0, 4);
        give(&mut s, 0, r, 2);
        give(&mut s, 1, r, 8);
        let s = act(act(rebuild(s), 6), 211 + r as u16);
        assert_eq!(s.resources[0][r], 10);
        assert_eq!(s.resources[1][r], 0);
        assert_eq!(s.cache.resource_totals, [10, 0]);
        assert!(actions(&s).contains(&(216 + r as u16)));
        let s = act(s, 1);
        let s = out(act(s, 0), 0);
        let s = act(s, 1);
        let s = out(act(s, 0), 5);
        assert_eq!(s.discard_remaining, [5, 0]);
    }
}

#[test]
fn x1_best_rate_is_per_resource_after_city_and_settlement() {
    let mut s = base();
    s.node_occupancy[0] = 1; // generic 3:1
    s.node_occupancy[5] = 3; // wood 2:1
    s.node_occupancy[15] = 1; // brick 2:1
    for r in 0..5 {
        give(&mut s, 0, r, 4);
    }
    let s = rebuild(s);
    assert_eq!(s.cache.trade_rates[0], [2, 2, 3, 3, 3]);
    for r in 0..5 {
        let given = s.resources[0][r];
        let next = act(act(s, 216 + r as u16), 221 + ((r + 1) % 5) as u16);
        assert_eq!(given - next.resources[0][r], if r < 2 { 2 } else { 3 });
    }
}

#[test]
fn u2_city_last_stock_returns_settlement_updates_production() {
    let mut s = base();
    for n in [0, 2, 11] {
        s.node_occupancy[n] = 3;
    }
    s.node_occupancy[15] = 1;
    give(&mut s, 0, 3, 4);
    give(&mut s, 0, 4, 6);
    let s = rebuild(s);
    assert_eq!(s.cache.pieces_left[0], [15, 4, 1]);
    let before = s.cache.production;
    let s = act(s, 133 + 15);
    assert_eq!(s.cache.pieces_left[0], [15, 5, 0]);
    assert!(
        s.cache
            .production
            .iter()
            .zip(before.iter())
            .any(|(a, b)| a != b)
    );
    assert!(!actions(&s).iter().any(|a| (133..187).contains(a)));
}

#[test]
fn d7_a4_fifteenth_hidden_point_wins_at_purchase() {
    let mut s = base();
    // Four cities and four settlements yield twelve visible points.
    for n in [0, 2, 11, 15] {
        s.node_occupancy[n] = 3;
    }
    for n in [27, 32, 47, 50] {
        s.node_occupancy[n] = 1;
    }
    s.playable_development[0][1] = 2;
    s.development_deck[1] -= 2;
    for r in 2..5 {
        give(&mut s, 0, r, 1);
    }
    let s = rebuild(s);
    assert_eq!(s.cache.total_points[0], 14);
    let s = out(act(s, 2), 17);
    assert_eq!(s.winner(), Some(0));
    assert_eq!(s.cache.visible_points[0], 12);
    assert_eq!(s.cache.total_points[0], 15);
    assert_eq!(s.new_development[0][1], 1);
}
