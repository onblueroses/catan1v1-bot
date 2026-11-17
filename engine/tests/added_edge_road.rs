//! A2: the incremental Longest Road update after one added road must equal a
//! plain search over all roads, in paid, free and setup placements, and
//! after undo.
use catan_engine::{phase::*, *};
#[path = "support/geometry_oracle.rs"]
mod geometry_oracle;
use geometry_oracle::EDGES;

// Independent raw DFS: global edge IDs in a u128, copied topology, every node
// as a start. No compressed masks, insertion identity or cached old maximum.
fn raw_walk(s: &State, player: u8, node: usize, used: u128, arrived: bool) -> u8 {
    let building = s.node_occupancy[node];
    if arrived && building != 0 && (building - 1) % 2 != player {
        return 0;
    }
    let mut best = 0;
    for (edge, ends) in EDGES.iter().enumerate() {
        let bit = 1u128 << edge;
        if s.edge_occupancy[edge] == player + 1 && used & bit == 0 && ends.contains(&node) {
            best = best.max(1 + raw_walk(s, player, ends[0] ^ ends[1] ^ node, used | bit, true));
        }
    }
    best
}

fn raw(s: &State, player: u8) -> u8 {
    (0..54)
        .map(|node| raw_walk(s, player, node, 0, false))
        .max()
        .unwrap()
}

fn base(player: u8, edges: &[usize], blockers: &[usize]) -> State {
    let mut s = State::new(
        Board::default(),
        Config {
            dice: 1,
            ..Config::default()
        },
    )
    .unwrap();
    s.phase = MAIN;
    s.turn_owner = player;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.resources[player as usize] = [2, 2, 0, 0, 0];
    s.bank = [17, 17, 19, 19, 19];
    for &edge in edges {
        s.edge_occupancy[edge] = player + 1;
    }
    for &node in blockers {
        s.node_occupancy[node] = 2 - player;
    }
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    s
}

fn compare(s: State, edge: usize) -> State {
    let action = 7 + edge as u16;
    let child = s.apply_action(action).unwrap();
    for player in 0..2 {
        assert_eq!(child.cache.road_lengths[player], raw(&child, player as u8));
        assert_eq!(child.longest_road(player as u8), raw(&child, player as u8));
    }
    let mut rebuilt = child;
    rebuilt.recompute_caches();
    rebuilt.normalize_awards();
    assert_eq!(child, rebuilt);
    assert_eq!(child.canonical_bytes(), rebuilt.canonical_bytes());
    child.validate().unwrap();
    let mut current = s;
    let record = current.make_action(action).unwrap();
    assert_eq!(current, child);
    current.unmake(record);
    assert_eq!(current, s, "undo added edge {edge} in phase {}", s.phase);
    assert_eq!(current.canonical_bytes(), s.canonical_bytes());
    child
}

#[test]
fn a2_cycle_closure_shares_edges_instead_of_naive_eleven() {
    for player in 0..2 {
        let s = base(player, &[1, 7, 12, 11, 6], &[]);
        assert_eq!(raw(&s, player), 5);
        // Both independent extensions can use the same five-edge old path.
        let [a, b] = EDGES[0];
        assert_eq!(
            1 + raw_walk(&s, player, a, 0, true) + raw_walk(&s, player, b, 0, true),
            11
        );
        let child = compare(s, 0);
        geometry_oracle::assert_geometry(&child, &EDGES);
        assert_eq!(child.cache.road_lengths[player as usize], 6);
    }
}

#[test]
fn a2_added_edge_bridges_forks_disconnected_components_and_blocked_endpoints() {
    for player in 0..2 {
        for (edges, added, blockers) in [
            (vec![0, 6, 10, 23, 33, 39], 18, vec![]),
            (
                vec![0, 1, 2, 3, 4, 5, 9, 16, 15, 14, 13, 12, 11, 6],
                7,
                vec![],
            ),
            (vec![0, 1, 7, 12, 11, 6, 2, 3, 19], 25, vec![]),
            (vec![0, 1, 2, 3, 4, 65, 71, 70], 5, vec![]),
            (vec![66, 67, 68, 69, 70, 0], 6, vec![]),
            (vec![0, 6, 10, 18], 23, vec![21]),
            (vec![0, 1, 7, 11, 6, 2, 3], 12, vec![4]),
            (vec![0, 1, 7, 11, 6, 2, 3], 12, vec![4, 12]),
        ] {
            let s = base(player, &edges, &blockers);
            compare(s, added);
            let mut free = s;
            free.playable_development[player as usize][2] = 1;
            free.development_deck[2] -= 1;
            free.recompute_caches();
            let free = free.apply_action(4).unwrap();
            assert_eq!(free.phase, ROAD_BUILDING);
            compare(free, added);
        }
    }
}

#[test]
fn a2_all_subgraphs_of_cycle_and_fork_pool_match_raw_dfs_after_every_legal_addition() {
    // This complete eight-edge pool contains a hex cycle and a two-edge fork.
    // Every subset and every missing legal insertion is checked for both owners.
    let pool = [0, 1, 7, 12, 11, 6, 2, 3];
    for player in 0..2 {
        for blockers in [vec![], vec![4], vec![12], vec![4, 12]] {
            for mask in 0..1usize << pool.len() {
                let edges: Vec<_> = pool
                    .iter()
                    .enumerate()
                    .filter_map(|(bit, &edge)| (mask & (1 << bit) != 0).then_some(edge))
                    .collect();
                let s = base(player, &edges, &blockers);
                for edge in pool {
                    if s.is_legal_action(7 + edge as u16) {
                        compare(s, edge);
                    }
                }
            }
        }
    }
}

#[test]
fn u2_a2_fifteenth_piece_keeps_exact_length_and_undo_in_paid_and_free_phases() {
    let chain = [0, 6, 10, 18, 23, 33, 39, 49, 54, 62, 66, 67, 68, 69, 64];
    for player in 0..2 {
        for blockers in [vec![], vec![45], vec![27]] {
            let s = base(player, &chain[..14], &blockers);
            let child = compare(s, chain[14]);
            assert_eq!(child.cache.pieces_left[player as usize][0], 0);
            let mut free = s;
            free.playable_development[player as usize][2] = 1;
            free.development_deck[2] -= 1;
            free.recompute_caches();
            let free = free.apply_action(4).unwrap();
            let child = compare(free, chain[14]);
            assert_eq!(child.phase, MAIN);
            assert_eq!(child.cache.pieces_left[player as usize][0], 0);
        }
    }
}

#[test]
fn s1_setup_added_roads_and_undo_keep_full_rebuild_equality() {
    for first in 0..2 {
        let mut s = State::new(
            Board::default(),
            Config {
                first_player: first,
                ..Config::default()
            },
        )
        .unwrap();
        for (node, edge) in [(0, 0), (2, 5), (16, 23), (47, 66)] {
            s = s.apply_action(79 + node).unwrap();
            s = compare(s, edge);
        }
    }
}

#[test]
fn a3_added_edge_uses_the_exact_maximum_after_an_opponent_settlement_cut() {
    for player in 0..2 {
        let mut s = base(player, &[0, 1, 2, 3, 4, 5, 9], &[]);
        s.edge_occupancy[7] = 2 - player;
        s.turn_owner = 1 - player;
        s.resources[1 - player as usize] = [1, 1, 1, 1, 0];
        for r in 0..4 {
            s.bank[r] -= 1;
        }
        s.recompute_caches();
        s.normalize_awards();
        s.validate().unwrap();
        let mut cut = s.apply_action(79 + 4).unwrap();
        assert!(cut.cache.road_lengths[player as usize] < s.cache.road_lengths[player as usize]);
        assert_eq!(cut.cache.road_lengths[player as usize], raw(&cut, player));
        cut.turn_owner = player;
        cut.validate().unwrap();
        compare(cut, 8);
    }
}
