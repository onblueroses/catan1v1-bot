//! A2: the compressed Longest Road search at the 15-road limit (U2), and its
//! fallback for a hand-edited fixture with 16 roads.
use catan_engine::{Board, Config, State};

#[test]
fn a2_exact_trail_at_piece_limit_and_over_stock_fixture_with_blocked_endpoints() {
    // This simple chain follows nodes 0,3,7,11,16,21,27,33,38,43,47,51,
    // 48,52,49,45,41. SPEC U2 allows 15 roads. The 16-road fixture also
    // checks that the public geometry query still handles direct invalid edits.
    let chain = [0, 6, 10, 18, 23, 33, 39, 49, 54, 62, 66, 67, 68, 69, 64, 59];
    for player in 0..2 {
        for count in [15, 16] {
            let mut s = State::new(Board::default(), Config::default()).unwrap();
            for &edge in &chain[..count] {
                s.edge_occupancy[edge] = player + 1;
            }
            assert_eq!(s.longest_road(player), count as u8);
            assert_eq!(s.longest_road(1 - player), 0);

            s.node_occupancy[0] = 2 - player;
            s.node_occupancy[if count == 15 { 45 } else { 41 }] = 4 - player;
            assert_eq!(s.longest_road(player), count as u8);

            // Node 27 is six edges from the first endpoint.
            s.node_occupancy[27] = 2 - player;
            assert_eq!(s.longest_road(player), (count - 6) as u8);
        }
    }
}
