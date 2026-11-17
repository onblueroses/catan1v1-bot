//! Placement enumeration controls use raw board snapshots. A control with no
//! legal pending placement is not claimed to be normalized or reachable. Tests
//! of constructors, transitions and undo separately validate their states.
use catan_engine::{
    runner::{self, Choice, Totals},
    topology::*,
    *,
};

fn base(player: u8) -> State {
    let mut s = State::new(
        Board::default(),
        Config {
            dice: 1,
            victory_points: u16::MAX,
            ..Config::default()
        },
    )
    .unwrap();
    s.phase = phase::MAIN;
    s.turn_owner = player;
    s.setup_pairs_done = 4;
    s.turn_number = 1;
    s
}

fn rebuild(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s
}

fn road_building(mut s: State) -> State {
    s.phase = phase::ROAD_BUILDING;
    s.resume_phase = phase::MAIN;
    s.choices_remaining = 2;
    s
}

struct Rank {
    index: u64,
    bound: u64,
    calls: usize,
}
impl RandomSource for Rank {
    fn next_u64(&mut self) -> u64 {
        panic!("selection must use bounded RNG override")
    }
    fn below(&mut self, bound: u64) -> u64 {
        assert_eq!(bound, self.bound);
        assert!(self.index < bound);
        self.calls += 1;
        self.index
    }
}

fn scalar(s: &State) -> Vec<u16> {
    (0..ACTION_COUNT as u16)
        .filter(|&id| s.is_legal_action(id))
        .collect()
}

fn check(s: &State) -> Vec<u16> {
    let before = *s;
    let mut raw = *s;
    raw.recompute_caches();
    assert_eq!(s.cache, raw.cache);
    let expected = scalar(&raw);
    let mut out = [u16::MAX; ACTION_COUNT];
    let n = s.legal_actions(&mut out);
    assert_eq!(&out[..n], expected, "phase {}", s.phase);
    assert!(out[..n].windows(2).all(|ids| ids[0] < ids[1]));
    assert!(out[n..].iter().all(|&id| id == u16::MAX));
    for (index, &id) in expected.iter().enumerate() {
        let mut rng = Rank {
            index: index as u64,
            bound: n as u64,
            calls: 0,
        };
        assert_eq!(runner::choose(s, &mut rng), Some(Choice::Action(id)));
        assert_eq!(rng.calls, 1);
    }
    assert_eq!(*s, before);
    expected
}

fn transition(s: State, action: u16) -> State {
    let expected = s.apply_action(action).unwrap();
    expected.validate().unwrap();
    let mut current = s;
    let undo = current.make_action(action).unwrap();
    assert_eq!(current, expected);
    current.unmake(undo);
    assert_eq!(current, s);
    assert_eq!(current.canonical_bytes(), s.canonical_bytes());
    assert_eq!(current.hash(), s.hash());
    check(&current);
    expected
}

#[test]
fn placement_distance_cache_excludes_every_occupied_node_and_neighbor() {
    for player in 0..2 {
        let initial = State::new(
            Board::default(),
            Config {
                first_player: player,
                ..Config::default()
            },
        )
        .unwrap();
        assert_eq!(initial.cache.distance_sites, (1u64 << 54) - 1);
        assert_eq!(check(&initial), (79..133).collect::<Vec<_>>());
        for node in 0..54 {
            for building in 1..=4 {
                let mut s = base(player);
                s.node_occupancy[node] = building;
                let mut s = rebuild(s);
                s.validate().unwrap();
                s.phase = phase::SETUP_SETTLEMENT;
                let actions = check(&s);
                assert!(!actions.contains(&(79 + node as u16)));
                for &neighbor in &NODE_NEIGHBORS[node] {
                    if neighbor != NONE {
                        assert!(!actions.contains(&(79 + u16::from(neighbor))));
                    }
                }
            }
        }
        let mut stock = base(player);
        let mut placed = 0;
        for node in 0..54 {
            if placed == 5 {
                break;
            }
            if stock.node_occupancy[node] == 0
                && NODE_NEIGHBORS[node]
                    .iter()
                    .all(|&n| n == NONE || stock.node_occupancy[n as usize] == 0)
            {
                stock.node_occupancy[node] = player + 1;
                placed += 1;
            }
        }
        assert_eq!(placed, 5); // U2 settlement stock.
        let mut stock = rebuild(stock);
        stock.validate().unwrap();
        stock.phase = phase::SETUP_SETTLEMENT;
        assert_eq!(stock.cache.pieces_left[player as usize][1], 0);
        assert!(check(&stock).is_empty());
    }
}

#[test]
fn placement_setup_roads_cover_all_anchors_occupancy_patterns_edges_and_stock() {
    for player in 0..2 {
        let initial = State::new(
            Board::default(),
            Config {
                first_player: player,
                ..Config::default()
            },
        )
        .unwrap();
        for anchor in 0..54 {
            let anchored = transition(initial, 79 + anchor as u16);
            assert_eq!(anchored.setup_anchor as usize, anchor);
            let incident: Vec<_> = NODE_EDGES[anchor]
                .iter()
                .copied()
                .filter(|&e| e != NONE)
                .collect();
            for pattern in 0..3usize.pow(incident.len() as u32) {
                let mut s = anchored;
                let mut value = pattern;
                for &edge in &incident {
                    s.edge_occupancy[edge as usize] = (value % 3) as u8;
                    value /= 3;
                }
                check(&rebuild(s));
            }
            // Every board edge can be occupied by either owner. Only empty
            // anchor-incident edges may appear, regardless of other connections.
            for edge in 0..72 {
                for owner in 1..=2 {
                    let mut s = anchored;
                    s.edge_occupancy[edge] = owner;
                    check(&rebuild(s));
                }
            }
            for action in check(&anchored) {
                transition(anchored, action);
            }
        }
        for roads in [14, 15] {
            // U2: one road left or exhausted.
            let mut s = base(player);
            s.edge_occupancy[..roads].fill(player + 1);
            s.node_occupancy[53] = player + 1;
            let mut s = rebuild(s);
            s.validate().unwrap();
            s.phase = phase::SETUP_ROAD;
            s.setup_anchor = 53;
            assert_eq!(check(&s).len(), if roads == 14 { 2 } else { 0 });
        }
        let mut s = initial;
        s.phase = phase::SETUP_ROAD;
        for anchor in [54, NONE] {
            s.setup_anchor = anchor;
            assert!(check(&s).is_empty());
        }
    }
}

#[test]
fn placement_free_roads_cover_every_edge_owner_blocker_and_stock() {
    for player in 0..2 {
        for edge in 0..72 {
            for endpoint in 0..2 {
                for building in 0..=4 {
                    let mut s = base(player);
                    s.edge_occupancy[edge] = player + 1;
                    s.node_occupancy[EDGE_NODES[edge][endpoint] as usize] = building;
                    let s = road_building(rebuild(s));
                    s.validate().unwrap();
                    for action in check(&s) {
                        transition(s, action);
                    }
                }
            }
        }
        for roads in [14, 15] {
            let mut s = base(player);
            s.edge_occupancy[..roads].fill(player + 1);
            let s = road_building(rebuild(s));
            assert_eq!(s.cache.pieces_left[player as usize][0], (15 - roads) as u8);
            let actions = check(&s);
            if roads == 15 {
                assert!(actions.is_empty());
            } else {
                assert!(!actions.is_empty());
            }
        }
    }
}

#[test]
fn placement_setup_snake_turn_order_and_opponent_cut_undo_match_scalar() {
    for first_player in 0..2 {
        let mut s = State::new(
            Board::default(),
            Config {
                first_player,
                ..Config::default()
            },
        )
        .unwrap();
        for (pair, (node, edge)) in [(0, 0), (2, 5), (16, 23), (47, 66)].into_iter().enumerate() {
            assert_eq!(
                s.turn_owner,
                [
                    first_player,
                    1 - first_player,
                    1 - first_player,
                    first_player
                ][pair]
            );
            assert_eq!(s.actor(), Some(s.turn_owner));
            s = transition(s, 79 + node);
            check(&s);
            s = transition(s, 7 + edge);
            if s.phase == phase::SETUP_SETTLEMENT {
                check(&s);
            }
        }
        assert_eq!(
            (s.phase, s.turn_owner, s.turn_number),
            (phase::PRE_ROLL, first_player, 1)
        );
    }
    for player in 0..2 {
        let mut s = base(player);
        s.edge_occupancy[2] = player + 1;
        s.edge_occupancy[1] = 2 - player;
        s.resources[player as usize] = [1, 1, 1, 1, 0];
        s.bank = [18, 18, 18, 18, 19];
        let s = rebuild(s);
        s.validate().unwrap();
        let mut opponent = road_building(s);
        opponent.turn_owner = 1 - player;
        assert!(check(&opponent).contains(&(7 + 7)));
        let settled = transition(s, 79 + 4);
        let mut opponent = road_building(settled);
        opponent.turn_owner = 1 - player;
        assert!(!check(&opponent).contains(&(7 + 7)));
    }
}

#[test]
fn placement_d5_dead_ends_complete_or_reject_without_empty_decision_nodes() {
    for player in 0..2 {
        for needs_edge in [false, true] {
            let mut s = base(player);
            s.config.road_building_needs_legal_edge = needs_edge;
            s.edge_occupancy[..14].fill(player + 1);
            let mut s = rebuild(s);
            let mut sites = s.cache.road_sites[player as usize];
            while sites != 0 {
                let edge = sites.trailing_zeros() as usize;
                s.edge_occupancy[edge] = 2 - player;
                sites &= sites - 1;
            }
            s.playable_development[player as usize][2] = 1;
            s.development_deck[2] -= 1;
            let s = rebuild(s);
            s.validate().unwrap();
            assert_eq!(s.cache.pieces_left[player as usize][0], 1);
            assert_eq!(s.cache.road_sites[player as usize], 0);
            assert!(check(&road_building(s)).is_empty());
            if needs_edge {
                assert_eq!(s.apply_action(4), Err(Error::InvalidAction));
            } else {
                assert_eq!(transition(s, 4).phase, phase::MAIN);
            }
        }
        // U2 leaves two road pieces. Block every exit but one, whose far node
        // has an opponent building, so its placement cannot open a new exit.
        let mut s = base(player);
        s.edge_occupancy[..13].fill(player + 1);
        let mut s = rebuild(s);
        let (chosen, blocked_node) = (0..72)
            .find_map(|edge| {
                if s.cache.road_sites[player as usize] & (1u128 << edge) == 0 {
                    return None;
                }
                EDGE_NODES[edge]
                    .iter()
                    .copied()
                    .find(|&node| {
                        NODE_EDGES[node as usize]
                            .iter()
                            .all(|&e| e == NONE || s.edge_occupancy[e as usize] != player + 1)
                    })
                    .map(|node| (edge, node))
            })
            .expect("raw board has an outward road endpoint");
        s.node_occupancy[blocked_node as usize] = 2 - player;
        let mut sites = s.cache.road_sites[player as usize] & !(1u128 << chosen);
        while sites != 0 {
            s.edge_occupancy[sites.trailing_zeros() as usize] = 2 - player;
            sites &= sites - 1;
        }
        s.playable_development[player as usize][2] = 1;
        s.development_deck[2] -= 1;
        let s = rebuild(s);
        s.validate().unwrap();
        assert_eq!(s.cache.road_sites[player as usize], 1u128 << chosen);
        let building = transition(s, 4);
        assert_eq!(building.phase, phase::ROAD_BUILDING);
        assert_eq!(check(&building), vec![7 + chosen as u16]);
        let finished = transition(building, 7 + chosen as u16);
        assert_eq!(finished.phase, phase::MAIN);
        assert_eq!(finished.cache.pieces_left[player as usize][0], 1);
        assert_eq!(finished.cache.road_sites[player as usize], 0);
    }
}

fn scalar_choose(s: &State, rng: &mut impl RandomSource) -> Option<Choice> {
    if s.is_terminal() {
        None
    } else if s.actor().is_some() {
        let actions = scalar(s);
        Some(Choice::Action(
            actions[rng.below(actions.len() as u64) as usize],
        ))
    } else {
        runner::choose(s, rng)
    }
}

#[test]
fn placement_full_trajectories_rng_caches_and_undo_match_scalar_both_dice() {
    for dice in 0..2 {
        for first_player in 0..2 {
            for index in [0, 2048] {
                let config = Config {
                    dice,
                    first_player,
                    ..Config::default()
                };
                let mut game = runner::prepare(41, index, config).unwrap();
                game.rng.next_u64();
                let mut current = game.state;
                let mut reference = current;
                let mut rng = game.rng;
                let mut reference_rng = rng;
                let mut totals = Totals::default();
                while !reference.is_dead() {
                    let expected = scalar_choose(&reference, &mut reference_rng);
                    assert_eq!(runner::choose(&current, &mut rng), expected);
                    assert_eq!(rng.state, reference_rng.state);
                    let Some(choice) = expected else { break };
                    if matches!(
                        reference.phase,
                        phase::SETUP_SETTLEMENT | phase::SETUP_ROAD | phase::ROAD_BUILDING
                    ) {
                        check(&current);
                        let Choice::Action(action) = choice else {
                            unreachable!()
                        };
                        current = transition(current, action);
                    } else {
                        current = choice.apply(&current).unwrap();
                    }
                    reference = choice.apply(&reference).unwrap();
                    assert_eq!(current, reference);
                    assert_eq!(current.canonical_bytes(), reference.canonical_bytes());
                    let mut rebuilt = current;
                    rebuilt.recompute_caches();
                    assert_eq!(current.cache, rebuilt.cache);
                    match choice {
                        Choice::Action(_) => totals.decisions += 1,
                        Choice::Outcome(_) => totals.chance_steps += 1,
                    }
                }
                totals.attempts = 1;
                totals.turns = reference.turn_number;
                totals.winners = u64::from(reference.winner < 2);
                totals.dead_games = u64::from(reference.is_dead());
                totals.games = totals.winners + totals.dead_games;
                if reference.winner < 2 {
                    totals.winner_counts[reference.winner as usize] = 1;
                }
                totals.checksum = reference.hash();
                assert_eq!(runner::play(game).unwrap(), totals);
            }
        }
    }
    assert_eq!(std::mem::size_of::<State>(), 528);
    assert_eq!(CANONICAL_SIZE, 313);
}
