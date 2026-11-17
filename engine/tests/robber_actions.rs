//! R1, R2: the direct robber enumeration against the scalar legality test,
//! for every hex, both owners, settlements and cities, and visible point
//! totals on both sides of the friendly robber limit.
use catan_engine::{
    runner::{self, Choice},
    *,
};

// Building heights produce visible totals 0, 1, 2, 2, 3 and 4. The two
// two-point cases distinguish settlements from a city at the R2 boundary.
const BUILDINGS: [[u8; 2]; 6] = [[0, 0], [1, 0], [1, 1], [2, 0], [2, 1], [2, 2]];
const NODES: [[usize; 2]; 2] = [[0, 16], [2, 20]];

fn fixture(buildings: [[u8; 2]; 2], friendly: bool, actor: u8) -> State {
    let mut s = State::new(Board::default(), Config::default()).unwrap();
    s.phase = phase::ROBBER;
    s.resume_phase = phase::MAIN;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s.turn_owner = actor;
    s.config.friendly_robber = friendly;
    for p in 0..2 {
        for (i, &height) in buildings[p].iter().enumerate() {
            if height != 0 {
                s.node_occupancy[NODES[p][i]] = p as u8 + 1 + 2 * (height - 1);
            }
        }
    }
    s.recompute_caches();
    assert_eq!(
        s.cache.visible_points,
        buildings.map(|heights| heights.iter().sum::<u8>())
    );
    s.validate().unwrap();
    s
}

fn scalar_actions(s: &State) -> Vec<u16> {
    (0..ACTION_COUNT as u16)
        .filter(|&id| s.is_legal_action(id))
        .collect()
}

fn check_actions(s: &State) -> Vec<u16> {
    let before = *s;
    let expected = scalar_actions(s);
    let mut actions = [u16::MAX; ACTION_COUNT];
    let n = s.legal_actions(&mut actions);
    assert_eq!(&actions[..n], expected);
    assert!(actions[..n].windows(2).all(|ids| ids[0] < ids[1]));
    assert!(actions[n..].iter().all(|&id| id == u16::MAX));
    assert!(!actions[..n].contains(&(187 + u16::from(s.robber_hex))));
    for id in 0..ACTION_COUNT as u16 {
        assert_eq!(
            actions[..n].contains(&id),
            s.is_legal_action(id),
            "action {id}"
        );
    }
    assert!(!s.is_legal_action(ACTION_COUNT as u16));
    assert!(!s.is_legal_action(u16::MAX));
    assert_eq!(*s, before);
    expected
}

struct RankSource {
    rank: u64,
    bound: u64,
    calls: usize,
}
impl RandomSource for RankSource {
    fn next_u64(&mut self) -> u64 {
        panic!("Decision selection must use exactly one bounded draw")
    }
    fn below(&mut self, bound: u64) -> u64 {
        assert_eq!(bound, self.bound);
        self.calls += 1;
        self.rank
    }
}

#[test]
fn r1_r2_all_hexes_point_boundaries_owners_and_buildings_match_scalar_and_choose_stream() {
    let mut actual_rng = SplitMix64::new(41);
    let mut reference_rng = actual_rng;
    let mut unrestricted = 0;
    let mut restricted = 0;
    let mut deserts = 0;
    for friendly in [false, true] {
        for actor in 0..2 {
            for p0 in BUILDINGS {
                for p1 in BUILDINGS {
                    let base = fixture([p0, p1], friendly, actor);
                    // B1 has 19 robber positions, including its unique desert.
                    for hex in 0..19 {
                        let mut s = base;
                        s.robber_hex = hex;
                        s.recompute_caches();
                        s.validate().unwrap();
                        deserts += usize::from(s.board.terrain[hex as usize] == 5);
                        let expected = check_actions(&s);
                        assert!(!expected.is_empty());
                        if !friendly || s.cache.visible_points.iter().all(|&points| points > 2) {
                            unrestricted += 1;
                            assert_eq!(expected.len(), 18);
                        } else if expected.len() < 18 {
                            restricted += 1;
                        }
                        for (rank, &action) in expected.iter().enumerate() {
                            let mut rng = RankSource {
                                rank: rank as u64,
                                bound: expected.len() as u64,
                                calls: 0,
                            };
                            assert_eq!(runner::choose(&s, &mut rng), Some(Choice::Action(action)));
                            assert_eq!(rng.calls, 1);
                        }
                        // Keep one continuous stream across every changing legal
                        // count rather than resetting either RNG for each state.
                        let action = expected[reference_rng.below(expected.len() as u64) as usize];
                        assert_eq!(
                            runner::choose(&s, &mut actual_rng),
                            Some(Choice::Action(action))
                        );
                        assert_eq!(actual_rng.state, reference_rng.state);
                    }
                }
            }
        }
    }
    assert!(unrestricted > 0);
    assert!(restricted > 0);
    assert!(deserts > 0);
}

#[test]
fn r2_city_at_two_visible_points_stays_protected_despite_hidden_vp_cards() {
    let mut protected = fixture([[2, 0], [2, 1]], true, 1);
    protected.robber_hex = 18;
    protected.playable_development[0][1] = 2;
    protected.development_deck[1] -= 2;
    protected.recompute_caches();
    protected.validate().unwrap();
    assert_eq!(protected.cache.visible_points, [2, 3]);
    assert_eq!(protected.cache.total_points, [4, 3]);
    assert!(!check_actions(&protected).contains(&187));

    let mut unprotected = fixture([[2, 1], [2, 1]], true, 1);
    unprotected.robber_hex = 18;
    unprotected.recompute_caches();
    assert_eq!(unprotected.cache.visible_points, [3, 3]);
    assert!(check_actions(&unprotected).contains(&187));

    protected.config.friendly_robber = false;
    assert!(check_actions(&protected).contains(&187));
}

#[test]
fn r2_desert_destination_respects_protected_cities_of_either_owner() {
    for protected in 0..2 {
        let mut buildings = [[2, 1]; 2];
        buildings[protected] = [0, 0];
        let mut s = fixture(buildings, true, protected as u8);
        assert_eq!(s.board.terrain[18], 5);
        assert!(topology::HEX_NODES[18].contains(&53));
        s.node_occupancy[53] = protected as u8 + 3;
        s.robber_hex = 0;
        s.recompute_caches();
        s.validate().unwrap();
        assert_eq!(s.cache.visible_points[protected], 2);
        assert!(!check_actions(&s).contains(&205));

        s.config.friendly_robber = false;
        assert!(check_actions(&s).contains(&205));

        s.config.friendly_robber = true;
        s.node_occupancy[NODES[protected][1]] = protected as u8 + 1;
        s.recompute_caches();
        s.validate().unwrap();
        assert_eq!(s.cache.visible_points, [3, 3]);
        assert!(check_actions(&s).contains(&205));
    }
}
