//! E3, E6: the cached site sets for roads, settlements and cities against an
//! independent oracle, after placements, after undo, and under observation
//! sampling. The cache must not change the canonical bytes.
use catan_engine::*;
#[path = "support/geometry_oracle.rs"]
mod geometry_oracle;
use geometry_oracle::{EDGES, assert_geometry};

fn main_state(player: u8) -> State {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::MAIN;
    s.turn_owner = player;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.resources = [[5; 5]; 2];
    s.bank = [9; 5];
    s.recompute_caches();
    s
}

fn check(s: &State) {
    assert_geometry(s, &EDGES);
    let mut raw = *s;
    raw.recompute_caches();
    assert_eq!(
        s.cache, raw.cache,
        "entire incremental cache versus raw rebuild"
    );
    let mut actions = [u16::MAX; ACTION_COUNT];
    let n = s.legal_actions(&mut actions);
    let scalar: Vec<_> = (0..ACTION_COUNT as u16)
        .filter(|&a| raw.is_legal_action(a))
        .collect();
    assert_eq!(&actions[..n], scalar);
    assert!(actions[..n].windows(2).all(|w| w[0] < w[1]));
    assert!(actions[n..].iter().all(|&a| a == u16::MAX));
    for a in 0..ACTION_COUNT as u16 {
        assert_eq!(s.is_legal_action(a), actions[..n].contains(&a));
    }
}

fn check_building_successors(s: State) {
    check(&s);
    let mut actions = [0; ACTION_COUNT];
    let n = s.legal_actions(&mut actions);
    for &a in &actions[..n] {
        if !(7..187).contains(&a) {
            continue;
        }
        let child = s.apply_action(a).unwrap();
        check(&child);
        let mut current = s;
        let undo = current.make_action(a).unwrap();
        assert_eq!(current, child);
        current.unmake(undo);
        assert_eq!(current, s, "complete restoration for board action {a}");
        check(&current);
    }
}

#[test]
fn e3_e6_copied_boards_cover_every_edge_node_owner_and_building_type() {
    for p in 0..2 {
        // B1 supplies all 72 edges. Include high edge bits above 63 and every
        // blocking/connecting building type at each edge's first endpoint.
        for (edge, ends) in EDGES.iter().enumerate() {
            for building in 0..=4 {
                let mut copied = main_state(p);
                copied.edge_occupancy[edge] = p + 1;
                copied.node_occupancy[ends[0]] = building;
                copied.recompute_caches();
                check_building_successors(copied);
            }
        }
        for node in 0..54 {
            for occupied in [false, true] {
                let mut copied = main_state(p);
                if occupied {
                    copied.node_occupancy[node] = p + 1;
                }
                // Opposite owners meet at the empty node or at an own
                // settlement. Settlement insertion must cut only the opponent.
                for (i, (edge, _)) in EDGES
                    .iter()
                    .enumerate()
                    .filter(|(_, ends)| ends.contains(&node))
                    .enumerate()
                {
                    copied.edge_occupancy[edge] = if i == 0 { p + 1 } else { 2 - p };
                }
                copied.recompute_caches();
                check_building_successors(copied);
            }
        }
    }
}

#[test]
fn e5_u3_undo_road_restores_candidate_and_removes_new_connections() {
    for p in 0..2 {
        let mut s = main_state(p);
        s.edge_occupancy[0] = p + 1;
        s.recompute_caches();
        let before = s;
        assert_ne!(s.cache.road_sites[p as usize] & (1 << 6), 0);
        assert_eq!(s.cache.road_sites[p as usize] & (1 << 11), 0);
        assert_eq!(s.cache.settlement_sites[p as usize] & (1 << 7), 0);
        let record = s.make_action(7 + 6).unwrap();
        assert_eq!(s.cache.road_sites[p as usize] & (1 << 6), 0);
        assert_ne!(s.cache.road_sites[p as usize] & (1 << 11), 0);
        assert_ne!(s.cache.settlement_sites[p as usize] & (1 << 7), 0);
        check(&s);
        s.unmake(record);
        assert_eq!(s, before);
        assert_ne!(s.cache.road_sites[p as usize] & (1 << 6), 0);
        assert_eq!(s.cache.road_sites[p as usize] & (1 << 11), 0);
        assert_eq!(s.cache.settlement_sites[p as usize] & (1 << 7), 0);
        check(&s);
    }
}

#[test]
fn e5_s2_u3_undo_settlement_restores_distance_sites_and_opponent_passage() {
    for p in 0..2 {
        let mut s = main_state(p);
        s.edge_occupancy[2] = p + 1;
        s.edge_occupancy[1] = 2 - p;
        s.recompute_caches();
        let before = s;
        assert_ne!(s.cache.settlement_sites[p as usize] & (1 << 4), 0);
        assert_ne!(s.cache.road_sites[1 - p as usize] & (1 << 7), 0);
        let record = s.make_action(79 + 4).unwrap();
        assert_eq!(s.cache.road_sites[1 - p as usize] & (1 << 7), 0);
        for n in [0, 1, 4, 8] {
            assert_eq!(s.cache.distance_sites & (1 << n), 0);
            assert_eq!(s.cache.settlement_sites[p as usize] & (1 << n), 0);
        }
        check(&s);
        s.unmake(record);
        assert_eq!(s, before);
        for n in [0, 1, 4, 8] {
            assert_ne!(s.cache.distance_sites & (1 << n), 0);
        }
        assert_ne!(s.cache.settlement_sites[p as usize] & (1 << 4), 0);
        assert_ne!(s.cache.road_sites[1 - p as usize] & (1 << 7), 0);
        check(&s);
    }
}

#[test]
fn e5_u5_undo_city_restores_upgrade_option_without_changing_connections() {
    for p in 0..2 {
        let mut s = main_state(p);
        s.node_occupancy[4] = p + 1;
        s.edge_occupancy[2] = p + 1;
        s.edge_occupancy[1] = 2 - p;
        s.recompute_caches();
        let before = s;
        assert!(s.is_legal_action(133 + 4));
        let record = s.make_action(133 + 4).unwrap();
        assert_eq!(s.cache.city_sites[p as usize] & (1 << 4), 0);
        assert_eq!(s.cache.road_sites, before.cache.road_sites);
        assert_eq!(s.cache.settlement_sites, before.cache.settlement_sites);
        assert_eq!(s.cache.distance_sites, before.cache.distance_sites);
        check(&s);
        s.unmake(record);
        assert_eq!(s, before);
        assert_ne!(s.cache.city_sites[p as usize] & (1 << 4), 0);
        assert!(s.is_legal_action(133 + 4));
        check(&s);
    }
}

#[test]
fn i2_i3_geometry_is_public_across_hidden_types_ages_and_sampled_states() {
    for p in 0..2 {
        for visible_vp in [false, true] {
            let mut baseline = main_state(p);
            baseline.config.vp_cards_hidden = !visible_vp;
            baseline.node_occupancy[4] = 1;
            baseline.edge_occupancy[2] = 1;
            baseline.edge_occupancy[1] = 2;
            baseline.playable_development[1][0] = 1;
            baseline.new_development[1][0] = 1;
            baseline.development_deck[0] -= 2;
            baseline.recompute_caches();
            let observation = baseline.observation(0).unwrap();
            check(&observation.public_state);
            for playable in 0..5 {
                for new in 0..5 {
                    if visible_vp && (playable == 1 || new == 1) {
                        continue;
                    }
                    let mut varied = baseline;
                    varied.playable_development[1] = [0; 5];
                    varied.new_development[1] = [0; 5];
                    varied.development_deck = DEVELOPMENT_COUNTS;
                    varied.playable_development[1][playable] = 1;
                    varied.new_development[1][new] = 1;
                    varied.development_deck[playable] -= 1;
                    varied.development_deck[new] -= 1;
                    varied.recompute_caches();
                    assert_eq!(varied.observation(0).unwrap(), observation);
                    let sample = observation.sample(&mut SplitMix64::new(7)).unwrap();
                    assert_eq!(sample.observation(0).unwrap(), observation);
                    check(&sample);
                    varied.config.observation_rule = 1;
                    let full = varied.observation(0).unwrap();
                    assert_eq!(full.public_state, varied);
                    assert_eq!(full.sample(&mut SplitMix64::new(7)).unwrap(), varied);
                }
            }
        }
    }
}

#[test]
fn e1_e6_e7_geometry_layout_and_cache_validation_keep_canonical_bytes() {
    let s = main_state(0);
    eprintln!(
        "State {}/{} Cache {}/{} UndoRecord {}/{} canonical {}",
        size_of::<State>(),
        align_of::<State>(),
        size_of::<Cache>(),
        align_of::<Cache>(),
        size_of::<UndoRecord>(),
        align_of::<UndoRecord>(),
        CANONICAL_SIZE
    );
    for field in 0..4 {
        let mut corrupt = s;
        match field {
            0 => corrupt.cache.distance_sites ^= 1,
            1 => corrupt.cache.road_sites[0] ^= 1,
            2 => corrupt.cache.settlement_sites[1] ^= 1,
            _ => corrupt.cache.city_sites[0] ^= 1,
        }
        assert_eq!(corrupt.canonical_bytes(), s.canonical_bytes());
        assert!(corrupt.validate().is_err());
    }
    let decoded = State::from_canonical_bytes(&s.canonical_bytes()).unwrap();
    assert_eq!(decoded, s);
    check(&decoded);
}
