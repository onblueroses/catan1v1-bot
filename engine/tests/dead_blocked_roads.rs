//! Recovered live-run counterexamples and independent E10 geometry controls.
use catan_engine::{dice::DiceState, topology::*, *};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
};

macro_rules! captured {
    ($hash:literal) => {
        (
            $hash,
            include_str!(concat!(
                "fixtures/blocked-roads/blocked-roads-",
                $hash,
                ".hex"
            )),
            include_str!(concat!(
                "fixtures/blocked-roads/blocked-roads-",
                $hash,
                ".jsonl"
            )),
        )
    };
}
const CAPTURED: [(&str, &str, &str); 11] = [
    captured!("0a90558550ad22c7"),
    captured!("0c82494c4d35564b"),
    captured!("15c41219edbaff81"),
    captured!("2240b8f5f20f7c5a"),
    captured!("25802412ac6f9095"),
    captured!("3c73151b339f0bf8"),
    captured!("6377fcc1cce7e44e"),
    captured!("843b77884e142c57"),
    captured!("965e934cf9aa62d5"),
    captured!("a9f1431a136bf4ec"),
    captured!("d8c057c2ffe00dc9"),
];

fn decode(hex: &str) -> State {
    State::from_canonical_bytes(&trace::decode_hex(hex.trim(), CANONICAL_SIZE).unwrap()).unwrap()
}

// Read authoritative occupancy and topology directly. Do not call geometry
// cache builders, road_legal, legal_actions, or affordability helpers.
fn raw_roads(s: &State, player: usize) -> u128 {
    let mut sites = 0;
    for (edge, ends) in EDGE_NODES.iter().enumerate() {
        if s.edge_occupancy[edge] != 0 {
            continue;
        }
        let connects = ends.iter().any(|&node| {
            let occupancy = s.node_occupancy[node as usize];
            if occupancy != 0 {
                return (occupancy - 1) as usize % 2 == player;
            }
            NODE_EDGES[node as usize]
                .iter()
                .any(|&e| e != NONE && s.edge_occupancy[e as usize] == player as u8 + 1)
        });
        if connects {
            sites |= 1u128 << edge;
        }
    }
    sites
}

fn raw_conditions(s: &State) -> [bool; 7] {
    let mut points = [0u16; 2];
    for &occupancy in &s.node_occupancy {
        if occupancy != 0 {
            points[(occupancy as usize - 1) % 2] += if occupancy >= 3 { 2 } else { 1 };
        }
    }
    for p in 0..2 {
        points[p] += u16::from(s.playable_development[p][1]) + u16::from(s.new_development[p][1]);
        points[p] += 2 * u16::from(s.largest_army_holder == p as u8)
            + 2 * u16::from(s.longest_road_holder == p as u8);
    }
    let road_frozen = (0..2).all(|p| {
        s.edge_occupancy
            .iter()
            .filter(|&&v| v == p as u8 + 1)
            .count()
            == 15
            || raw_roads(s, p) == 0
    });
    let settlement_frozen = (0..54).all(|node| {
        s.node_occupancy[node] != 0
            || NODE_NEIGHBORS[node]
                .iter()
                .any(|&n| n != NONE && s.node_occupancy[n as usize] != 0)
            || !NODE_EDGES[node]
                .iter()
                .any(|&edge| edge != NONE && s.edge_occupancy[edge as usize] != 0)
    });
    let cities_frozen = (0..2).all(|p| {
        s.node_occupancy
            .iter()
            .filter(|&&v| v == p as u8 + 3)
            .count()
            == 4
            || !s.node_occupancy.contains(&(p as u8 + 1))
    });
    let knights = |p: usize| {
        u16::from(s.spent_development[p][0])
            + u16::from(s.playable_development[p][0])
            + u16::from(s.new_development[p][0])
    };
    let holder = s.largest_army_holder as usize;
    let army_frozen = if holder < 2 {
        knights(1 - holder) <= u16::from(s.spent_development[holder][0])
    } else {
        knights(0) < 3 && knights(1) < 3
    };
    [
        s.setup_pairs_done == 4 && s.phase != phase::TERMINAL,
        points.iter().all(|&p| p < s.config.victory_points),
        road_frozen,
        settlement_frozen,
        cities_frozen,
        s.development_deck.iter().all(|&n| n == 0),
        army_frozen,
    ]
}

fn checked(s: State) -> State {
    s.validate().unwrap();
    let mut rebuilt = s;
    rebuilt.recompute_caches();
    assert_eq!(s.cache, rebuilt.cache);
    for p in 0..2 {
        assert_eq!(s.cache.road_sites[p], raw_roads(&s, p));
    }
    assert!(raw_conditions(&s).into_iter().all(|condition| condition));
    assert!(s.is_dead());
    assert!(!s.is_terminal());
    s
}

#[test]
fn e10_all_eleven_captured_states_and_snapshot_traces_have_frozen_points() {
    let mut blocked_players = BTreeSet::new();
    for (hash, hex, trace) in CAPTURED {
        let s = checked(decode(hex));
        assert_eq!(format!("{:016x}", s.hash()), hash);
        assert_eq!(s.canonical_bytes().len(), 313);
        assert!(s.cache.pieces_left.iter().any(|pieces| pieces[0] > 0));
        for p in 0..2 {
            if s.cache.pieces_left[p][0] > 0 {
                assert_eq!(raw_roads(&s, p), 0);
                blocked_players.insert(p);
            }
        }
        let report = trace::replay(Cursor::new(trace)).unwrap();
        assert!(report.snapshot);
        assert_eq!(report.state, s);
        assert_eq!(report.steps, 0);
        let mut actions = [0; ACTION_COUNT];
        let n = s.legal_actions(&mut actions);
        let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
        let m = s.chance_outcomes(&mut outcomes);
        for choice in actions[..n]
            .iter()
            .map(|&id| runner::Choice::Action(id))
            .chain(outcomes[..m].iter().map(|o| runner::Choice::Outcome(o.id)))
        {
            let next = checked(choice.apply(&s).unwrap());
            assert_eq!(next.cache.total_points, s.cache.total_points);
            let mut sparse = s;
            let undo = choice.make(&mut sparse).unwrap();
            assert_eq!(sparse, next);
            sparse.unmake(undo);
            assert_eq!(sparse, s);
        }
    }
    assert_eq!(blocked_players, BTreeSet::from([0, 1]));
}

#[test]
fn e10_opening_a_blocked_connection_refuses_dead_even_when_no_road_is_affordable() {
    for player in 0..2 {
        let control = CAPTURED
            .iter()
            .find_map(|(_, hex, _)| {
                let s = decode(hex);
                if s.cache.pieces_left[player][0] == 0 || s.cache.pieces_left[player][2] != 0 {
                    return None;
                }
                (0..54).find_map(|node| {
                    // A synthetic ownership change opens a formerly blocked
                    // endpoint without freeing a settlement site or city piece.
                    if s.node_occupancy[node] != (2 - player) as u8 {
                        return None;
                    }
                    let mut open = s;
                    open.node_occupancy[node] = player as u8 + 1;
                    open.config.victory_points = 50;
                    open.resources = [[0; 5]; 2];
                    open.bank = [19; 5];
                    open.recompute_caches();
                    open.normalize_awards();
                    if open.validate().is_err() || raw_roads(&open, player) == 0 {
                        return None;
                    }
                    let conditions = raw_conditions(&open);
                    (!conditions[2] && conditions.iter().enumerate().all(|(i, &yes)| i == 2 || yes))
                        .then_some(open)
                })
            })
            .expect("positive control must break only the road-freeze condition");
        assert!(!control.is_dead());
        assert!(control.cache.road_sites[player] != 0);
        let mut own = control;
        own.turn_owner = player as u8;
        own.phase = phase::MAIN;
        own.resume_phase = NONE;
        let mut actions = [0; ACTION_COUNT];
        let n = own.legal_actions(&mut actions);
        assert!(!actions[..n].iter().any(|id| (7..79).contains(id)));
        own.resources[player][0] = 1;
        own.resources[player][1] = 1;
        own.bank[0] -= 1;
        own.bank[1] -= 1;
        own.recompute_caches();
        own.validate().unwrap();
        assert!(!own.is_dead());
        let n = own.legal_actions(&mut actions);
        assert!(actions[..n].iter().any(|id| (7..79).contains(id)));
    }
}

#[test]
fn e10_blocked_road_certificate_is_closed_in_every_possible_pending_phase() {
    for dice in 0..2 {
        for owner in 0..2 {
            let mut s = decode(CAPTURED[10].1);
            s.phase = phase::MAIN;
            s.resume_phase = NONE;
            s.discard_remaining = [0; 2];
            s.choices_remaining = 0;
            s.trade_give = NONE;
            s.development_played = false;
            s.config.dice = dice;
            s.config.friendly_robber = false;
            s.turn_owner = owner;
            if dice == 1 {
                s.dice = DiceState::default();
            }
            s.resources = [[6; 5]; 2];
            s.bank = [7; 5];
            let holder = s.largest_army_holder as usize;
            s.spent_development[holder][0] = 8;
            s.spent_development[1 - holder][0] = 3;
            s.playable_development[holder][0] = 0;
            s.playable_development[1 - holder][0] = 3;
            s.new_development[0][0] = 0;
            s.new_development[1][0] = 0;
            for kind in 2..5 {
                for p in 0..2 {
                    s.spent_development[p][kind] = 0;
                    s.playable_development[p][kind] = 0;
                    s.new_development[p][kind] = 0;
                }
                s.playable_development[owner as usize][kind] = 2;
            }
            s.recompute_caches();
            s.normalize_awards();
            checked(s);
            let mut pending = VecDeque::from([s]);
            let mut witnesses = BTreeMap::new();
            let mut no_site_road_building = false;
            while let Some(s) = pending.pop_front() {
                let key = (
                    s.phase,
                    s.turn_owner,
                    s.actor(),
                    s.resume_phase,
                    s.choices_remaining,
                    s.discard_remaining,
                    s.development_played,
                );
                if witnesses.contains_key(&key) {
                    continue;
                }
                checked(s);
                witnesses.insert(key, s);
                let mut actions = [0; ACTION_COUNT];
                let n = s.legal_actions(&mut actions);
                let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
                let m = s.chance_outcomes(&mut outcomes);
                for choice in actions[..n]
                    .iter()
                    .map(|&id| runner::Choice::Action(id))
                    .chain(outcomes[..m].iter().map(|o| runner::Choice::Outcome(o.id)))
                {
                    let next = checked(choice.apply(&s).unwrap());
                    assert_eq!(next.cache.total_points, s.cache.total_points);
                    assert_eq!(next.node_occupancy, s.node_occupancy);
                    assert_eq!(next.edge_occupancy, s.edge_occupancy);
                    assert_eq!(next.largest_army_holder, s.largest_army_holder);
                    assert_eq!(next.longest_road_holder, s.longest_road_holder);
                    if choice == runner::Choice::Action(4) {
                        no_site_road_building = true;
                        assert_ne!(next.phase, phase::ROAD_BUILDING);
                        assert_eq!(s.cache.road_sites[s.turn_owner as usize], 0);
                    }
                    let mut sparse = s;
                    let record = choice.make(&mut sparse).unwrap();
                    assert_eq!(sparse, next);
                    sparse.unmake(record);
                    assert_eq!(sparse, s);
                    pending.push_back(next);
                }
            }
            assert_eq!(
                witnesses.values().map(|s| s.phase).collect::<BTreeSet<_>>(),
                BTreeSet::from([
                    phase::PRE_ROLL,
                    phase::DICE,
                    phase::DISCARD,
                    phase::ROBBER,
                    phase::STEAL,
                    phase::MAIN,
                    phase::YEAR_OF_PLENTY,
                    phase::MONOPOLY,
                    phase::TRADE_TAKE
                ])
            );
            if s.cache.pieces_left[owner as usize][0] > 0 {
                assert!(no_site_road_building);
            }
            // Setup/terminal fail E10. Empty-deck DEVELOPMENT_DRAW and no-edge
            // ROAD_BUILDING are not valid normalized decision states.
        }
    }
}

// The existing raw seven-condition predicate is the independent reference.
// Check each replaced subcondition even if an earlier E10 guard returns false.
fn compare_cached_dead(s: &State) {
    s.validate().unwrap();
    let raw = raw_conditions(s);
    assert_eq!(
        (s.cache.settlement_sites[0] | s.cache.settlement_sites[1]) == 0,
        raw[3]
    );
    for player in 0..2 {
        assert_eq!(
            s.cache.city_sites[player] == 0,
            !s.node_occupancy.contains(&(player as u8 + 1))
        );
    }
    assert_eq!(s.is_dead(), raw.into_iter().all(|condition| condition));
}

#[test]
fn e10_cached_geometry_matches_raw_predicate_for_captured_and_point_source_controls() {
    let mut saw_settlement_site = false;
    let mut saw_upgrade = [false; 2];
    for (_, hex, _) in CAPTURED {
        let captured = decode(hex);
        compare_cached_dead(&captured);
        for hidden in [false, true] {
            for owner in 0..2 {
                let mut base = captured;
                base.config.vp_cards_hidden = hidden;
                base.turn_owner = owner;
                base.phase = phase::MAIN;
                base.resume_phase = NONE;
                base.discard_remaining = [0; 2];
                base.choices_remaining = 0;
                base.trade_give = NONE;
                base.config.victory_points = 50;
                base.recompute_caches();
                compare_cached_dead(&base);
                // Removing a building exposes settlement geometry. Downgrading
                // a city creates a real upgrade, with no resource requirement.
                for node in 0..54 {
                    let occupancy = base.node_occupancy[node];
                    if occupancy == 0 {
                        continue;
                    }
                    let mut changed = base;
                    if occupancy >= 3 {
                        changed.node_occupancy[node] -= 2;
                    } else {
                        changed.node_occupancy[node] = 0;
                    }
                    changed.resources = [[0; 5]; 2];
                    changed.bank = [19; 5];
                    changed.recompute_caches();
                    changed.normalize_awards();
                    if changed.validate().is_err() {
                        continue;
                    }
                    compare_cached_dead(&changed);
                    saw_settlement_site |= (changed.cache.settlement_sites[0]
                        | changed.cache.settlement_sites[1])
                        != 0;
                    for player in 0..2 {
                        saw_upgrade[player] |= changed.cache.pieces_left[player][2] != 0
                            && changed.cache.city_sites[player] != 0;
                    }
                }
                // Returning each held card type to the deck must agree with
                // the raw full predicate, including hidden VP identities.
                for player in 0..2 {
                    for kind in 0..5 {
                        if base.playable_development[player][kind] == 0 {
                            continue;
                        }
                        let mut draw_possible = base;
                        draw_possible.playable_development[player][kind] -= 1;
                        draw_possible.development_deck[kind] += 1;
                        draw_possible.recompute_caches();
                        compare_cached_dead(&draw_possible);
                    }
                }
            }
        }
    }
    assert!(saw_settlement_site && saw_upgrade.into_iter().all(|seen| seen));
}

#[test]
fn e10_cached_geometry_matches_raw_predicate_and_sparse_restore_through_complete_games() {
    let mut seen_phases = BTreeSet::new();
    let mut saw_geometric_sites_without_affordability = false;
    for dice in 0..2 {
        for owner in 0..2 {
            for hidden in [false, true] {
                let config = Config {
                    dice,
                    first_player: owner,
                    vp_cards_hidden: hidden,
                    ..Config::default()
                };
                let game = runner::prepare(41, 0, config).unwrap();
                let mut state = game.state;
                let mut rng = game.rng;
                loop {
                    compare_cached_dead(&state);
                    seen_phases.insert(state.phase);
                    if state.phase == phase::MAIN
                        && (state.cache.settlement_sites[0] | state.cache.settlement_sites[1]) != 0
                    {
                        let mut actions = [0; ACTION_COUNT];
                        let n = state.legal_actions(&mut actions);
                        saw_geometric_sites_without_affordability |=
                            !actions[..n].iter().any(|id| (79..133).contains(id));
                    }
                    if state.is_dead() {
                        break;
                    }
                    let Some(choice) = runner::choose(&state, &mut rng) else {
                        break;
                    };
                    let next = choice.apply(&state).unwrap();
                    let mut current = state;
                    let undo = choice.make(&mut current).unwrap();
                    assert_eq!(current, next);
                    compare_cached_dead(&current);
                    current.unmake(undo);
                    assert_eq!(current, state);
                    compare_cached_dead(&current);
                    state = next;
                }
            }
        }
    }
    assert!(saw_geometric_sites_without_affordability);
    assert!(seen_phases.contains(&phase::SETUP_SETTLEMENT));
    assert!(seen_phases.contains(&phase::TERMINAL));
    assert!(seen_phases.contains(&phase::TRADE_TAKE));
}
