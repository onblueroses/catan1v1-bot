//! R4, T3, T4: the incremental production update when the robber moves,
//! against two independent full computations, for every pair of old and new
//! hex, with sparse undo and with both shortage rules on the next roll.
use catan_engine::{phase::*, topology::*, *};

// Inverse-node traversal is independent of the full hex traversal and the
// incremental old/new-hex update. Only authoritative board fields are read.
fn raw_production(s: &State) -> [[[u8; 5]; 2]; 11] {
    let mut production = [[[0; 5]; 2]; 11];
    for (node, &occupancy) in s.node_occupancy.iter().enumerate() {
        if occupancy == 0 {
            continue;
        }
        for &hex in &NODE_HEXES[node] {
            if hex == NONE || hex == s.robber_hex {
                continue;
            }
            let hex = hex as usize;
            let resource = s.board.terrain[hex] as usize;
            if resource != 5 {
                production[s.board.number[hex] as usize - 2][((occupancy - 1) % 2) as usize]
                    [resource] += if occupancy >= 3 { 2 } else { 1 };
            }
        }
    }
    production
}

fn fixture(start: Option<usize>) -> State {
    let mut s = State::new(
        Board::default(),
        Config {
            dice: 1,
            friendly_robber: false,
            ..Config::default()
        },
    )
    .unwrap();
    s.phase = ROBBER;
    s.resume_phase = MAIN;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    if let Some(start) = start {
        let mut buildings = 0;
        for offset in 0..54 {
            let node = (start + offset) % 54;
            if NODE_NEIGHBORS[node]
                .iter()
                .all(|&n| n == NONE || s.node_occupancy[n as usize] == 0)
            {
                s.node_occupancy[node] = [1, 2, 3, 4][buildings % 4];
                buildings += 1;
                // U2 stocks: five settlements and four cities per player.
                if buildings == 2 * (5 + 4) {
                    break;
                }
            }
        }
        assert_eq!(buildings, 2 * (5 + 4));
    }
    s.recompute_caches();
    s.validate().unwrap();
    s
}

fn expected_move(mut s: State, destination: u8) -> State {
    s.robber_hex = destination;
    let victim = 1 - s.turn_owner;
    let occupied_by_victim = HEX_NODES[destination as usize].iter().any(|&node| {
        let occupancy = s.node_occupancy[node as usize];
        occupancy != 0 && (occupancy - 1) % 2 == victim
    });
    if occupied_by_victim && s.resources[victim as usize].iter().any(|&count| count > 0) {
        s.phase = STEAL;
    } else {
        s.phase = s.resume_phase;
        s.resume_phase = NONE;
    }
    s.recompute_caches();
    assert_eq!(s.cache.production, raw_production(&s));
    s
}

#[test]
fn r4_every_old_new_hex_pair_matches_full_and_raw_oracles_and_sparse_undo() {
    let mut saw_desert_source = false;
    let mut saw_desert_destination = false;
    let mut saw_overlapping_cells = false;
    let mut saw_both_players = false;
    let mut saw_city = false;
    let mut saw_steal = false;
    let mut covered_hexes = [false; 19];
    for layout in [None, Some(0), Some(17)] {
        let template = fixture(layout);
        for (hex, nodes) in HEX_NODES.iter().enumerate() {
            covered_hexes[hex] |= nodes
                .iter()
                .any(|&node| template.node_occupancy[node as usize] != 0);
        }
        for player in 0..2 {
            for resume in [PRE_ROLL, MAIN] {
                for hands in [false, true] {
                    for old in 0..19 {
                        let mut before = template;
                        before.turn_owner = player;
                        before.resume_phase = resume;
                        before.robber_hex = old;
                        if hands {
                            before.resources = [[1; 5]; 2];
                            before.bank = [17; 5];
                        }
                        before.recompute_caches();
                        before.validate().unwrap();
                        assert_eq!(before.cache.production, raw_production(&before));
                        assert_eq!(
                            before.apply_action(187 + u16::from(old)),
                            Err(Error::InvalidAction)
                        );
                        for destination in 0..19 {
                            if destination == old {
                                continue;
                            }
                            let expected = expected_move(before, destination);
                            let next = before.apply_action(187 + u16::from(destination)).unwrap();
                            assert_eq!(
                                next, expected,
                                "old {old}, destination {destination}, player {player}"
                            );
                            assert_eq!(next.canonical_bytes(), expected.canonical_bytes());
                            next.validate().unwrap();
                            let mut current = before;
                            let record = current.make_action(187 + u16::from(destination)).unwrap();
                            assert_eq!(current, next);
                            if current.phase == STEAL {
                                saw_steal = true;
                                let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
                                assert!(current.chance_outcomes(&mut outcomes) > 0);
                                let theft = current.make_outcome(outcomes[0].id).unwrap();
                                current.unmake(theft);
                                assert_eq!(current, next);
                            }
                            current.unmake(record);
                            assert_eq!(current, before);
                            assert_eq!(current.canonical_bytes(), before.canonical_bytes());
                            saw_desert_source |= before.board.terrain[old as usize] == 5;
                            saw_desert_destination |=
                                before.board.terrain[destination as usize] == 5;
                            saw_overlapping_cells |= before.board.terrain[old as usize] < 5
                                && before.board.terrain[old as usize]
                                    == before.board.terrain[destination as usize]
                                && before.board.number[old as usize]
                                    == before.board.number[destination as usize]
                                && next.cache.production.iter().any(|players| {
                                    players.iter().flatten().any(|&count| count > 0)
                                });
                            saw_both_players |= next.cache.production.iter().any(|players| {
                                players[0].iter().any(|&n| n > 0)
                                    && players[1].iter().any(|&n| n > 0)
                            });
                            saw_city |= before
                                .node_occupancy
                                .iter()
                                .any(|&occupancy| occupancy >= 3);
                        }
                    }
                }
            }
        }
    }
    assert!(covered_hexes.into_iter().all(|covered| covered));
    assert!(
        saw_desert_source
            && saw_desert_destination
            && saw_overlapping_cells
            && saw_both_players
            && saw_city
            && saw_steal
    );
}

#[test]
fn r4_shared_city_and_equal_roll_resource_hexes_keep_other_contributions() {
    let mut s = fixture(None);
    // Both hexes 1 and 2 are wood/3. Node 9's city touches both, node 1
    // belongs only to hex 1, and node 6's opponent city only to hex 2.
    s.robber_hex = 1;
    s.node_occupancy[9] = 3;
    s.node_occupancy[1] = 1;
    s.node_occupancy[6] = 4;
    s.recompute_caches();
    s.validate().unwrap();
    assert_eq!(s.cache.production[1][0][0], 2);
    assert_eq!(s.cache.production[1][1][0], 2);
    assert_eq!(s.cache.production[3][0][1], 2);
    let before = s;
    let record = s.make_action(187 + 2).unwrap();
    assert_eq!(s.cache.production[1][0][0], 3);
    assert_eq!(s.cache.production[1][1][0], 0);
    assert_eq!(s.cache.production[3][0][1], 2);
    assert_eq!(s, expected_move(before, 2));
    s.validate().unwrap();
    s.unmake(record);
    assert_eq!(s, before);
}

fn moved_with_shortage(both_players: bool) -> (State, u8, usize) {
    let template = fixture(Some(0));
    for old in 0..19 {
        for destination in 0..19 {
            if old == destination {
                continue;
            }
            let mut s = template;
            s.robber_hex = old;
            s.recompute_caches();
            let moved = s.apply_action(187 + u16::from(destination)).unwrap();
            assert_eq!(moved.phase, MAIN);
            let production = raw_production(&moved);
            for (roll, demand) in production.iter().enumerate() {
                for resource in 0..5 {
                    if u16::from(demand[0][resource]) + u16::from(demand[1][resource]) > 1
                        && (demand[0][resource] > 0 && demand[1][resource] > 0) == both_players
                    {
                        return (moved, roll as u8 + 2, resource);
                    }
                }
            }
        }
    }
    panic!("positive control requires a moved position with the requested shortage demand")
}

#[test]
fn t3_t4_roll_after_robber_move_preserves_both_shortage_rules_and_sparse_restoration() {
    for both_players in [false, true] {
        for shortage_rule in 0..2 {
            for supply in [0, 1] {
                let (mut s, total, shortage_resource) = moved_with_shortage(both_players);
                s.config.shortage_rule = shortage_rule;
                s.resources[0][shortage_resource] = 19 - supply;
                s.bank[shortage_resource] = supply;
                s.recompute_caches();
                s.validate().unwrap();
                let dice = s.apply_action(1).unwrap().apply_action(0).unwrap();
                let before = dice;
                let mut expected = dice;
                let demand = raw_production(&dice)[(total - 2) as usize];
                for resource in 0..5 {
                    let owed = u16::from(demand[0][resource]) + u16::from(demand[1][resource]);
                    if owed <= u16::from(expected.bank[resource]) {
                        for player in 0..2 {
                            expected.resources[player][resource] += demand[player][resource];
                            expected.bank[resource] -= demand[player][resource];
                        }
                    } else if shortage_rule == 0
                        && (demand[0][resource] > 0) != (demand[1][resource] > 0)
                    {
                        let player = usize::from(demand[0][resource] == 0);
                        expected.resources[player][resource] += expected.bank[resource];
                        expected.bank[resource] = 0;
                    }
                }
                expected.last_dice = total;
                expected.phase = MAIN;
                expected.recompute_caches();
                let actual = dice.apply_outcome(u16::from(total - 2)).unwrap();
                assert_eq!(actual, expected);
                actual.validate().unwrap();
                if !both_players && shortage_rule == 0 {
                    assert_eq!(actual.bank[shortage_resource], 0);
                } else {
                    assert_eq!(actual.bank[shortage_resource], supply);
                }
                let mut sparse = dice;
                let record = sparse.make_outcome(u16::from(total - 2)).unwrap();
                assert_eq!(sparse, actual);
                sparse.unmake(record);
                assert_eq!(sparse, before);
            }
        }
    }
}
