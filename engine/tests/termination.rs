//! Reachable counterexample to the specification's universal termination claim.
//! This certificate is deliberately narrow. It is not a runner adjudication rule.
use catan_engine::{
    NONE, State, phase,
    topology::{NODE_EDGES, NODE_NEIGHBORS},
    trace,
};
use std::io::Cursor;

fn fixture() -> State {
    let bytes =
        trace::decode_hex(include_str!("fixtures/nonwinning-seed191.hex").trim(), 313).unwrap();
    State::from_canonical_bytes(&bytes).unwrap()
}

/// Prove a nonwinning continuation set only when every point source is fixed
/// except city upgrades. Return None when any precondition is absent.
fn frozen_position_point_bounds(s: &State) -> Option<[u16; 2]> {
    s.validate().ok()?;
    if s.phase == phase::TERMINAL
        || s.setup_pairs_done != 4
        || s.development_deck.iter().any(|&v| v != 0)
        || (0..2).any(|p| s.cache.pieces_left[p][0] != 0)
        || (0..2).any(|p| s.playable_development[p][0] != 0 || s.new_development[p][0] != 0)
    {
        return None;
    }
    let mut bounds = [0; 2];
    for (p, bound) in bounds.iter_mut().enumerate() {
        let future_site = (0..54).any(|n| {
            s.node_occupancy[n] == 0
                && NODE_NEIGHBORS[n]
                    .iter()
                    .all(|&v| v == NONE || s.node_occupancy[v as usize] == 0)
                && NODE_EDGES[n]
                    .iter()
                    .any(|&e| e != NONE && s.edge_occupancy[e as usize] == p as u8 + 1)
        });
        if future_site {
            return None;
        }
        let own_nodes = s
            .node_occupancy
            .iter()
            .filter(|&&v| v != 0 && (v as usize - 1) % 2 == p)
            .count() as u16;
        // Each owned node scores one point. At most four nodes can be cities.
        // No roads remain. No settlement site exists. Cities preserve ownership,
        // distance-rule occupation, road connectivity and road cut points.
        // No knight remains to play or buy. Both awards therefore remain fixed.
        *bound = own_nodes
            + own_nodes.min(4)
            + u16::from(s.playable_development[p][1])
            + u16::from(s.new_development[p][1])
            + 2 * u16::from(s.longest_road_holder == p as u8)
            + 2 * u16::from(s.largest_army_holder == p as u8);
    }
    Some(bounds)
}

#[test]
fn u2_u4_a1_a4_reachable_position_cannot_reach_fifteen_points() {
    let report = trace::replay(Cursor::new(include_bytes!(
        "fixtures/nonwinning-seed191.jsonl"
    )))
    .unwrap();
    assert!(
        !report.snapshot,
        "The trace must establish constructor reachability"
    );
    assert_eq!(report.steps, 1522);
    assert_eq!((report.decisions, report.chance_steps), (1115, 407));
    let s = fixture();
    assert_eq!(report.state.canonical_bytes(), s.canonical_bytes());
    assert_eq!(s.hash(), 0x7b7f3619b541b4a8);
    assert_eq!(s.turn_number, 361);
    assert_eq!(s.cache.total_points, [13, 13]);
    let bounds = frozen_position_point_bounds(&s).unwrap();
    assert_eq!(bounds, [14, 14]);
    assert!(bounds.iter().all(|&b| b < s.config.victory_points));
    assert_eq!(s.winner(), None);
}

#[test]
fn certificate_refuses_a_position_with_an_unused_road_piece() {
    let mut s = fixture();
    let edge = s.edge_occupancy.iter().position(|&v| v == 1).unwrap();
    s.edge_occupancy[edge] = 0;
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    assert_eq!(s.cache.pieces_left[0][0], 1);
    assert_eq!(frozen_position_point_bounds(&s), None);
}

#[test]
fn certificate_refuses_a_position_with_a_future_settlement_site() {
    let original = fixture();
    // This control is a valid snapshot, not a claimed reachable continuation.
    let control = (0..54)
        .find_map(|n| {
            if original.node_occupancy[n] == 0 {
                return None;
            }
            let mut s = original;
            let p = (s.node_occupancy[n] - 1) % 2;
            s.node_occupancy[n] = 0;
            s.recompute_caches();
            s.normalize_awards();
            let eligible = NODE_NEIGHBORS[n]
                .iter()
                .all(|&v| v == NONE || s.node_occupancy[v as usize] == 0)
                && NODE_EDGES[n]
                    .iter()
                    .any(|&e| e != NONE && s.edge_occupancy[e as usize] == p + 1);
            (eligible && s.validate().is_ok()).then_some(s)
        })
        .expect("A valid positive control must exist");
    assert_eq!(control.cache.pieces_left[0][0], 0);
    assert_eq!(control.cache.pieces_left[1][0], 0);
    assert_eq!(frozen_position_point_bounds(&control), None);
}
