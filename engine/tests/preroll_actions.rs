//! T1, D2, D3, D5: the direct PRE_ROLL enumeration (the roll plus playable
//! cards) against the scalar `is_legal_action`, over every subset of card
//! types, both age groups, both players and the Road Building guards.
use catan_engine::{runner::Choice, topology::*, *};

fn fixtures(mut check: impl FnMut(State, u8, bool, bool, bool)) {
    for player in 0..2 {
        for stock in [0, 1] {
            for edge_available in [false, true] {
                let mut geometry = State::new(Board::default(), Config::default()).unwrap();
                geometry.phase = phase::PRE_ROLL;
                geometry.turn_owner = player;
                geometry.setup_pairs_done = 4;
                geometry.turn_number = 1;
                // U2 gives fifteen road pieces. These compact upper-board
                // roads leave six/seven boundary edges to close for no-edge cases.
                let roads = 15 - stock;
                geometry.edge_occupancy[..roads].fill(player + 1);
                if !edge_available {
                    for edge in roads..72 {
                        if EDGE_NODES[edge].iter().any(|&node| {
                            NODE_EDGES[node as usize]
                                .iter()
                                .any(|&adjacent| adjacent != NONE && (adjacent as usize) < roads)
                        }) {
                            geometry.edge_occupancy[edge] = 2 - player;
                        }
                    }
                }
                geometry.recompute_caches();
                geometry.normalize_awards();
                geometry.validate().unwrap();
                assert_eq!(geometry.cache.pieces_left[player as usize][0], stock as u8);
                assert_eq!(
                    (0..72).any(|edge| geometry.road_legal(edge)),
                    stock == 1 && edge_available
                );
                for needs_edge in [false, true] {
                    for played in [false, true] {
                        // D1 has four playable types, hence sixteen subsets.
                        for subset in 0..16 {
                            // Only old cards, only new cards, or both groups.
                            for group in 0..3 {
                                let mut s = geometry;
                                s.config.road_building_needs_legal_edge = needs_edge;
                                s.development_played = played;
                                for (bit, kind) in [0, 2, 3, 4].into_iter().enumerate() {
                                    let present = u8::from(subset & (1 << bit) != 0);
                                    let old = present * u8::from(group != 1);
                                    let new = present * u8::from(group != 0);
                                    s.playable_development[player as usize][kind] = old;
                                    s.new_development[player as usize][kind] = new;
                                    // Opponent-only cards detect use of the wrong hand.
                                    let opponent = u8::from(old + new == 0);
                                    s.playable_development[1 - player as usize][kind] = opponent;
                                    s.development_deck[kind] -= old + new + opponent;
                                }
                                // Neither age group of VP cards has a PLAY action.
                                s.playable_development[player as usize][1] = 1;
                                s.new_development[player as usize][1] = 1;
                                s.development_deck[1] -= 2;
                                s.recompute_caches();
                                s.validate().unwrap();
                                check(s, subset, group != 1, edge_available, stock == 1);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn scalar(s: &State) -> Vec<u16> {
    (0..ACTION_COUNT as u16)
        .filter(|&action| s.is_legal_action(action))
        .collect()
}

#[test]
fn preroll_all_card_subsets_ages_players_stock_and_edge_guards_match_scalar() {
    fixtures(|s, subset, old_cards, edge_available, stock| {
        let before = s;
        let mut actions = [u16::MAX; ACTION_COUNT];
        let count = s.legal_actions(&mut actions);
        let reference = scalar(&s);
        assert_eq!(&actions[..count], reference);
        assert_eq!(actions[0], 0);
        assert!(actions[..count].windows(2).all(|pair| pair[0] < pair[1]));
        assert!(actions[count..].iter().all(|&id| id == u16::MAX));
        for bit in 0..4 {
            let id = 3 + bit;
            let allowed = !s.development_played
                && old_cards
                && subset & (1 << bit) != 0
                && (id != 4
                    || (stock && (!s.config.road_building_needs_legal_edge || edge_available)));
            assert_eq!(reference.contains(&id), allowed, "subset={subset} id={id}");
        }
        assert_eq!(s, before);
    });
}

struct Rank {
    index: u64,
    bound: u64,
    calls: usize,
}
impl RandomSource for Rank {
    fn next_u64(&mut self) -> u64 {
        panic!("decision selection must use the bounded RNG interface")
    }
    fn below(&mut self, bound: u64) -> u64 {
        assert_eq!(bound, self.bound);
        assert!(self.index < bound);
        self.calls += 1;
        self.index
    }
}

#[test]
fn preroll_every_rank_and_seeded_choice_preserve_scalar_order_and_rng() {
    fixtures(|s, subset, _, _, _| {
        let reference = scalar(&s);
        for (index, &id) in reference.iter().enumerate() {
            let mut rng = Rank {
                index: index as u64,
                bound: reference.len() as u64,
                calls: 0,
            };
            assert_eq!(runner::choose(&s, &mut rng), Some(Choice::Action(id)));
            assert_eq!(rng.calls, 1);
        }
        // The subset and player vary real RNG positions across the matrix.
        let mut expected_rng = SplitMix64::new(u64::from(subset) + (u64::from(s.turn_owner) << 32));
        let mut actual_rng = expected_rng;
        let id = reference[expected_rng.below(reference.len() as u64) as usize];
        assert_eq!(
            runner::choose(&s, &mut actual_rng),
            Some(Choice::Action(id))
        );
        assert_eq!(actual_rng.state, expected_rng.state);
    });
}
