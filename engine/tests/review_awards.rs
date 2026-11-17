//! A1 to A4, written by an independent review. Road
//! lengths are compared with an oracle that shares no code with the engine:
//! the largest connected edge subset with zero or two nodes of odd degree.
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
    s.phase = phase::MAIN;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s
}
fn ready(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    s
}
fn cached(s: State) -> State {
    let mut full = s;
    full.recompute_caches();
    assert_eq!(s.cache, full.cache);
    s.validate().unwrap();
    s
}
// Independent characterization: an edge subset forms a trail iff its graph is
// connected and has zero or two odd-degree vertices. Split every opponent node
// into one leaf per incident edge, so no traversal can pass through that node.
fn euler_subset(s: &State, player: u8) -> u8 {
    let es: Vec<_> = EDGE_NODES
        .iter()
        .enumerate()
        .filter(|(e, _)| s.edge_occupancy[*e] == player + 1)
        .collect();
    let mut best = 0;
    for mask in 1usize..1 << es.len() {
        let count = mask.count_ones() as u8;
        if count <= best {
            continue;
        }
        let mut degree = [0u8; 198];
        let mut parent: [usize; 198] = std::array::from_fn(|i| i);
        fn root(p: &[usize; 198], mut v: usize) -> usize {
            while p[v] != v {
                v = p[v];
            }
            v
        }
        for (i, (e, ends)) in es.iter().enumerate() {
            if mask & (1 << i) == 0 {
                continue;
            }
            let nodes: [usize; 2] = std::array::from_fn(|side| {
                let n = ends[side] as usize;
                let x = s.node_occupancy[n];
                if x != 0 && (x - 1) % 2 != player {
                    54 + 2 * e + side
                } else {
                    n
                }
            });
            for n in nodes {
                degree[n] += 1;
            }
            let a = root(&parent, nodes[0]);
            let b = root(&parent, nodes[1]);
            parent[a] = b;
        }
        let odd = degree.iter().filter(|&&d| d % 2 == 1).count();
        if odd != 0 && odd != 2 {
            continue;
        }
        let first = degree.iter().position(|&d| d > 0).unwrap();
        if degree
            .iter()
            .enumerate()
            .all(|(n, &d)| d == 0 || root(&parent, n) == root(&parent, first))
        {
            best = count;
        }
    }
    best
}
#[test]
fn a2_trails_match_independent_euler_subsets_cycles_branches_and_cuts() {
    for (edges, blockers, expected) in [
        (vec![0, 1, 7, 12, 11, 6], vec![], 6),
        (vec![0, 1, 7, 12, 11, 6, 2, 3], vec![], 8),
        (vec![0, 1, 7, 12, 11, 6, 2, 3, 19, 25], vec![], 8),
        (vec![0, 1, 7, 12, 11, 6, 2, 3], vec![4], 6),
        (vec![0, 1, 7, 12, 11, 6, 2, 3], vec![4, 12], 4),
        (vec![0, 1, 2, 3, 4, 65, 71, 70], vec![], 5),
        (vec![0, 1, 2, 3], vec![], 4),
    ] {
        let mut s = base();
        for &e in &edges {
            s.edge_occupancy[e] = 1;
        }
        for &n in &blockers {
            s.node_occupancy[n] = 2;
        }
        let s = ready(s);
        assert_eq!(
            euler_subset(&s, 0),
            expected,
            "edges={edges:?} blockers={blockers:?}"
        );
        assert_eq!(s.cache.road_lengths[0], expected);
        assert_eq!(s.longest_road_holder, if expected >= 5 { 0 } else { NONE });
    }
}
#[test]
fn a3_settlement_cut_updates_cache_keeps_qualifying_tie_and_transfers_to_opponent() {
    for (nroads, expected, holder) in [(7, [5, 5], 0), (6, [4, 5], 1), (5, [3, 4], NONE)] {
        let mut s = base();
        s.turn_owner = 1;
        // Player 0 has a chain through node 4. Player 1 can settle node 4 via edge 7.
        for &e in &[0, 1, 2, 3, 4, 5, 9][..nroads] {
            s.edge_occupancy[e] = 1;
        }
        s.edge_occupancy[7] = 2;
        let rival = if nroads == 5 { 4 } else { 5 };
        for &e in &[66, 67, 68, 69, 70][..rival] {
            s.edge_occupancy[e] = 2;
        }
        for r in 0..4 {
            s.resources[1][r] = 1;
            s.bank[r] -= 1;
        }
        let s = ready(s);
        assert_eq!(s.longest_road_holder, 0);
        let s = cached(s.apply_action(79 + 4).unwrap());
        assert_eq!(s.cache.road_lengths, expected);
        for p in 0..2 {
            assert_eq!(s.cache.road_lengths[p], euler_subset(&s, p as u8));
        }
        assert_eq!(s.longest_road_holder, holder);
    }
}
#[test]
fn a1_knight_actions_keep_tie_then_overtake() {
    let mut s = base();
    s.turn_owner = 1;
    s.spent_development[0][0] = 3;
    s.spent_development[1][0] = 2;
    s.playable_development[1][0] = 2;
    s.development_deck[0] -= 7;
    s.largest_army_holder = 0;
    let s = cached(ready(s).apply_action(3).unwrap());
    assert_eq!(s.largest_army_holder, 0);
    let mut s = s.apply_action(187).unwrap();
    s = s
        .apply_action(1)
        .unwrap()
        .apply_action(0)
        .unwrap()
        .apply_outcome(0)
        .unwrap();
    s = s
        .apply_action(1)
        .unwrap()
        .apply_action(0)
        .unwrap()
        .apply_outcome(0)
        .unwrap();
    let s = cached(s.apply_action(3).unwrap());
    assert_eq!(s.largest_army_holder, 1);
    assert_eq!(s.cache.visible_points, [0, 2]);
}
#[test]
fn a4_snapshot_opponent_at_fifteen_waits_until_turn_start() {
    let mut s = base();
    s.turn_owner = 0;
    // This snapshot tests turn-owner eligibility without claiming reachability.
    // With two players, a placement can never *award* Longest Road to the idle player:
    // own roads only get longer, while own settlements only shorten the idle road.
    for n in [0, 2, 11, 15] {
        s.node_occupancy[n] = 4;
    }
    for n in [27, 32, 47] {
        s.node_occupancy[n] = 2;
    }
    s.playable_development[1][1] = 4;
    s.development_deck[1] -= 4;
    let s = ready(s);
    assert_eq!(s.cache.total_points[1], 15);
    assert_eq!(s.winner(), None);
    let s = cached(s.apply_action(1).unwrap());
    assert_eq!(s.turn_owner, 1);
    assert_eq!(s.winner(), Some(1));
}
