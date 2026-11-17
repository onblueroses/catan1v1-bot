//! E10: a separate, sound certificate that is closed under legal transitions.
use catan_engine::{
    dead, phase,
    topology::{NODE_EDGES, NODE_NEIGHBORS},
    trace, *,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
};

fn frozen() -> State {
    let bytes = trace::decode_hex(
        include_str!("fixtures/dead-seed191.hex").trim(),
        CANONICAL_SIZE,
    )
    .unwrap();
    State::from_canonical_bytes(&bytes).unwrap()
}

fn normalize(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    s
}

#[test]
fn e10_constructor_trace_reaches_dead_without_a_terminal_win() {
    let report = trace::replay(Cursor::new(include_bytes!("fixtures/dead-seed191.jsonl"))).unwrap();
    assert!(!report.snapshot);
    assert_eq!(report.steps, 1763);
    let s = frozen();
    assert_eq!(report.state.canonical_bytes(), s.canonical_bytes());
    assert_eq!((s.turn_number, s.hash()), (402, 0x4b3fd2661552e6c0));
    assert_eq!(s.cache.total_points, [14, 14]);
    assert!(dead(&s) && s.is_dead());
    assert!(!s.is_terminal());
    assert_eq!(s.winner(), None);
    let bytes = s.canonical_bytes();
    assert!(dead(&s));
    assert_eq!(s.canonical_bytes(), bytes);
    // Measured x86-64 size/alignment after the geometry-only cache change:
    // State 528/16, Cache 208/16, UndoRecord 88/8. Canonical bytes stay 313.
    assert_eq!(std::mem::size_of::<State>(), 528);
    assert_eq!(bytes.len(), 313);
}

#[test]
fn e10_refuses_setup_and_terminal_states() {
    let initial = State::new(Board::default(), Config::default()).unwrap();
    assert!(!dead(&initial));
    let mut s = frozen();
    s.config.victory_points = 14;
    let terminal = s.apply_action(1).unwrap();
    terminal.validate().unwrap();
    assert!(terminal.is_terminal());
    assert!(!dead(&terminal));
}

#[test]
fn e10_refuses_an_opponent_already_at_the_target() {
    let mut s = frozen();
    s.turn_owner = 0;
    s.playable_development[0][1] -= 1;
    s.playable_development[1][1] += 1;
    let s = normalize(s);
    assert_eq!(s.cache.total_points, [13, 15]);
    assert!(!dead(&s));
}

#[test]
fn e10_refuses_unused_road_stock() {
    let mut s = frozen();
    let edge = s.edge_occupancy.iter().position(|&v| v == 1).unwrap();
    s.edge_occupancy[edge] = 0;
    let s = normalize(s);
    assert_eq!(s.cache.pieces_left[0][0], 1);
    assert!(!dead(&s));
}

#[test]
fn e10_checks_settlement_geometry_without_affordability() {
    let original = frozen();
    let s = (0..54)
        .find_map(|n| {
            // Removing a settlement preserves the exhausted city stocks.
            if !(1..=2).contains(&original.node_occupancy[n]) {
                return None;
            }
            let mut s = original;
            let p = s.node_occupancy[n] - 1;
            s.node_occupancy[n] = 0;
            s.resources = [[0; 5]; 2];
            s.bank = [19; 5];
            s.config.victory_points = 50;
            let s = normalize(s);
            let eligible = NODE_NEIGHBORS[n]
                .iter()
                .all(|&v| v == NONE || s.node_occupancy[v as usize] == 0)
                && NODE_EDGES[n]
                    .iter()
                    .any(|&e| e != NONE && s.edge_occupancy[e as usize] == p + 1);
            eligible.then_some(s)
        })
        .expect("The positive control must expose a settlement site");
    assert_eq!(s.cache.pieces_left[0][2], 0);
    assert_eq!(s.cache.pieces_left[1][2], 0);
    assert!(!dead(&s));
}

#[test]
fn e10_refuses_a_possible_city_upgrade_and_any_nonempty_deck() {
    let mut s = frozen();
    let n = s.node_occupancy.iter().position(|&v| v == 3).unwrap();
    s.node_occupancy[n] = 1;
    let s = normalize(s);
    assert_eq!(s.cache.pieces_left[0][2], 1);
    assert!(!dead(&s));
    let mut s = frozen();
    s.playable_development[0][1] -= 1;
    s.development_deck[1] += 1;
    assert!(!dead(&normalize(s)));
}

#[test]
fn e10_accepts_unused_city_stock_if_no_settlement_can_be_upgraded() {
    let mut s = frozen();
    s.config.victory_points = 50;
    // Preserve every occupied node. Transfer player 0's settlements and one
    // city to player 1 as settlements. Player 0 keeps only three cities.
    let city = s.node_occupancy.iter().position(|&v| v == 3).unwrap();
    for v in &mut s.node_occupancy {
        if *v == 1 {
            *v = 2;
        }
    }
    s.node_occupancy[city] = 2;
    let s = normalize(s);
    assert_eq!(s.cache.pieces_left[0][2], 1);
    assert!(!s.node_occupancy.contains(&1));
    assert!(dead(&s));
}

fn army_fixture(holder_spent: u8, rival_spent: u8, rival_old: u8, rival_new: u8) -> State {
    assert_eq!(holder_spent + rival_spent + rival_old + rival_new, 14);
    let mut s = frozen();
    s.turn_owner = 0;
    s.spent_development[1][0] = holder_spent;
    s.spent_development[0][0] = rival_spent;
    s.playable_development[0][0] = rival_old;
    s.new_development[0][0] = rival_new;
    normalize(s)
}

#[test]
fn e10_army_bound_counts_played_old_and_new_knights() {
    let tied_maximum = army_fixture(7, 3, 2, 2);
    assert!(dead(&tied_maximum), "A future tie cannot pass Largest Army");
    let old_can_overtake = army_fixture(6, 3, 5, 0);
    assert!(!dead(&old_can_overtake));
    let new_can_overtake = army_fixture(6, 3, 0, 5);
    assert!(!dead(&new_can_overtake));
    let mut no_holder = frozen();
    no_holder.largest_army_holder = NONE;
    no_holder.spent_development[1][0] = 0;
    no_holder.playable_development[0][0] = 7;
    no_holder.playable_development[1][0] = 7;
    let no_holder = normalize(no_holder);
    assert_eq!(no_holder.largest_army_holder, NONE);
    assert!(!dead(&no_holder));
}

#[test]
fn e10_remaining_knights_can_reach_only_a_tie_and_preserve_dead() {
    let mut s = army_fixture(7, 3, 2, 2);
    let points = s.cache.total_points;
    // The fixture has four opposing knights left, including two bought this turn.
    for _ in 0..4 {
        s = s.apply_action(3).unwrap();
        assert!(dead(&s));
        while s.phase != phase::MAIN {
            let choice = runner::choose(&s, &mut SplitMix64::new(0)).unwrap();
            s = choice.apply(&s).unwrap();
            s.validate().unwrap();
            assert!(dead(&s));
        }
        assert_eq!(s.cache.total_points, points);
        assert_eq!(s.largest_army_holder, 1);
        // Return to the rival's next MAIN phase through accepted transitions.
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
        assert!(dead(&s));
    }
    assert_eq!(
        [s.spent_development[0][0], s.spent_development[1][0]],
        [7, 7]
    );
    assert_eq!(s.largest_army_holder, 1);
}

fn rich_dead_fixture(dice: u8, owner: u8) -> State {
    let mut s = army_fixture(8, 3, 3, 0);
    s.config.dice = dice;
    s.turn_owner = owner;
    s.resources = [[6; 5]; 2];
    s.bank = [7; 5];
    for t in 2..5 {
        for p in 0..2 {
            s.spent_development[p][t] = 0;
            s.playable_development[p][t] = 0;
            s.new_development[p][t] = 0;
        }
        s.playable_development[owner as usize][t] = 2;
    }
    normalize(s)
}

#[test]
fn e10_every_accepted_transition_preserves_points_and_dead_in_every_possible_phase() {
    for dice in 0..2 {
        for owner in 0..2 {
            let mut pending = VecDeque::from([rich_dead_fixture(dice, owner)]);
            let mut witnesses = BTreeMap::new();
            while let Some(s) = pending.pop_front() {
                // Cover actors, pending-effect completion and card-play flags.
                // This finite control-state traversal is not a gameplay limit.
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
                assert!(dead(&s));
                witnesses.insert(key, s);
                let mut check = |next: State| {
                    next.validate().unwrap();
                    assert_eq!(next.cache.total_points, s.cache.total_points);
                    assert_eq!(next.winner(), None);
                    assert!(dead(&next), "dice={dice} owner={owner} phase={}", s.phase);
                    pending.push_back(next);
                };
                let mut actions = [0; ACTION_COUNT];
                let n = s.legal_actions(&mut actions);
                for &action in &actions[..n] {
                    check(s.apply_action(action).unwrap());
                }
                let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
                let n = s.chance_outcomes(&mut outcomes);
                for outcome in &outcomes[..n] {
                    check(s.apply_outcome(outcome.id).unwrap());
                }
            }
            assert_eq!(
                witnesses
                    .values()
                    .map(|s| s.phase)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>(),
                vec![
                    phase::PRE_ROLL,
                    phase::DICE,
                    phase::DISCARD,
                    phase::ROBBER,
                    phase::STEAL,
                    phase::MAIN,
                    phase::YEAR_OF_PLENTY,
                    phase::MONOPOLY,
                    phase::TRADE_TAKE
                ]
            );
            // Setup and terminal fail E10. DEVELOPMENT_DRAW requires a nonempty
            // deck. ROAD_BUILDING completes automatically when road stock is zero.
        }
    }
}
