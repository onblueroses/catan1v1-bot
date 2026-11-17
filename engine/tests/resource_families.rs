//! Fixed resource-family enumeration. Empty pending effects and malformed X1
//! controls test scalar compatibility, not normalized-state reachability.
use catan_engine::{
    runner::{self, Choice, Totals},
    topology::PORT_NODES,
    *,
};

fn base(player: u8) -> State {
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
    s.turn_owner = player;
    s
}

fn rebuild(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s
}

struct Rank {
    index: u64,
    bound: u64,
    calls: usize,
}
impl RandomSource for Rank {
    fn next_u64(&mut self) -> u64 {
        panic!("selection must honor below override")
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
    let expected = scalar(s);
    let mut actions = [u16::MAX; ACTION_COUNT];
    let n = s.legal_actions(&mut actions);
    assert_eq!(
        &actions[..n],
        expected,
        "phase={} give={}",
        s.phase,
        s.trade_give
    );
    assert!(actions[..n].windows(2).all(|ids| ids[0] < ids[1]));
    assert!(actions[n..].iter().all(|&id| id == u16::MAX));
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

fn transition(s: State, id: u16) -> State {
    s.validate().unwrap();
    let next = s.apply_action(id).unwrap();
    next.validate().unwrap();
    let mut current = s;
    let undo = current.make_action(id).unwrap();
    assert_eq!(current, next);
    current.unmake(undo);
    assert_eq!(current, s);
    assert_eq!(current.canonical_bytes(), s.canonical_bytes());
    assert_eq!(current.hash(), s.hash());
    check(&current);
    next
}

#[test]
fn resource_discard_all_hand_supports_and_both_actor_queue_orders() {
    for roller in 0..2 {
        for first_hand in 0..(1 << 5) {
            for second_hand in 0..(1 << 5) {
                let mut s = base(roller);
                for r in 0..5 {
                    s.resources[0][r] = u8::from(first_hand & (1 << r) != 0);
                    s.resources[1][r] = u8::from(second_hand & (1 << r) != 0);
                    s.bank[r] = 19 - s.resources[0][r] - s.resources[1][r];
                }
                let mut s = rebuild(s);
                s.phase = phase::DISCARD;
                s.resume_phase = phase::MAIN;
                // Include empty/malformed queue controls to preserve the exact
                // existing actor fallback instead of introducing a new guard.
                for queue in [[1, 1], [1, 0], [0, 1], [0, 0]] {
                    s.discard_remaining = queue;
                    let actor = if queue[roller as usize] > 0 {
                        roller
                    } else {
                        1 - roller
                    };
                    assert_eq!(s.actor(), Some(actor));
                    let expected: Vec<_> = (0..5)
                        .filter(|&r| s.resources[actor as usize][r] > 0)
                        .map(|r| 206 + r as u16)
                        .collect();
                    assert_eq!(check(&s), expected);
                }
            }
        }
        let mut s = base(roller);
        s.phase = phase::PRE_ROLL;
        s.resources[0][0] = 10;
        s.resources[1][1] = 10;
        s.bank[0] = 9;
        s.bank[1] = 9;
        let s = rebuild(s);
        s.validate().unwrap();
        let mut s = transition(s, 0).apply_outcome(5).unwrap();
        s.validate().unwrap();
        assert_eq!(s.discard_remaining, [5, 5]);
        for actor in [roller, 1 - roller] {
            for remaining in (1..=5).rev() {
                assert_eq!(s.actor(), Some(actor));
                assert_eq!(s.discard_remaining[actor as usize], remaining);
                check(&s);
                s = transition(s, 206 + u16::from(actor));
            }
        }
        assert_eq!(s.phase, phase::ROBBER);
    }
}

#[test]
fn resource_bank_supports_yop_pending_counts_and_monopoly_all_resources() {
    for player in 0..2 {
        for support in 0..(1 << 5) {
            for resume in [phase::PRE_ROLL, phase::MAIN] {
                let mut s = base(player);
                for r in 0..5 {
                    s.bank[r] = u8::from(support & (1 << r) != 0);
                    s.resources[1 - player as usize][r] = 19 - s.bank[r];
                }
                let mut s = rebuild(s);
                s.resume_phase = resume;
                s.phase = phase::YEAR_OF_PLENTY;
                for remaining in [1, 2] {
                    s.choices_remaining = remaining;
                    let actions = check(&s);
                    assert_eq!(
                        actions,
                        (0..5)
                            .filter(|&r| support & (1 << r) != 0)
                            .map(|r| 211 + r as u16)
                            .collect::<Vec<_>>()
                    );
                    if support != 0 {
                        for action in actions {
                            transition(s, action);
                        }
                    }
                }
                s.phase = phase::MONOPOLY;
                s.choices_remaining = 0;
                assert_eq!(check(&s), vec![211, 212, 213, 214, 215]);
                for action in check(&s) {
                    transition(s, action);
                }
            }
        }
        for support in 0..(1 << 5) {
            let mut s = base(player);
            for r in 0..5 {
                s.resources[1 - player as usize][r] = u8::from(support & (1 << r) != 0);
                s.bank[r] = 19 - s.resources[1 - player as usize][r];
            }
            let mut s = rebuild(s);
            s.phase = phase::MONOPOLY;
            s.resume_phase = phase::MAIN;
            for action in check(&s) {
                transition(s, action);
            }
        }
    }
}

#[test]
fn resource_trade_all_bank_supports_rates_funding_and_malformed_give_guards() {
    for player in 0..2 {
        // Nine actual ports plus no building cover all real trade rates.
        for port in 0..=9 {
            let mut geometry = base(player);
            if port < 9 {
                geometry.node_occupancy[PORT_NODES[port][0] as usize] = player + 1;
            }
            let geometry = rebuild(geometry);
            geometry.validate().unwrap();
            for give in 0..5 {
                let rate = geometry.cache.trade_rates[player as usize][give];
                for support in 0..(1 << 5) {
                    for amount in [0, rate - 1, rate, rate + 1] {
                        let mut s = geometry;
                        for r in 0..5 {
                            s.bank[r] = u8::from(support & (1 << r) != 0);
                            s.resources[player as usize][r] = if r == give { amount } else { 0 };
                            s.resources[1 - player as usize][r] =
                                19 - s.bank[r] - s.resources[player as usize][r];
                        }
                        let mut s = rebuild(s);
                        s.phase = phase::TRADE_TAKE;
                        s.resume_phase = phase::MAIN;
                        s.trade_give = give as u8;
                        let expected: Vec<_> = (0..5)
                            .filter(|&r| r != give && support & (1 << r) != 0 && amount >= rate)
                            .map(|r| 221 + r as u16)
                            .collect();
                        assert_eq!(check(&s), expected);
                        if amount < rate {
                            // X1 negative controls must not expose take choices
                            // merely because another resource exists in bank.
                            for id in 221..226 {
                                assert_eq!(s.apply_action(id), Err(Error::InvalidAction));
                                let mut current = s;
                                assert!(current.make_action(id).is_err());
                                assert_eq!(current, s);
                            }
                        } else if !expected.is_empty() {
                            for action in expected {
                                transition(s, action);
                            }
                        }
                    }
                }
            }
            for give in [5, NONE] {
                let mut s = geometry;
                s.phase = phase::TRADE_TAKE;
                s.resume_phase = phase::MAIN;
                s.trade_give = give;
                assert!(check(&s).is_empty());
                for id in 221..226 {
                    assert_eq!(s.apply_action(id), Err(Error::InvalidAction));
                    let mut current = s;
                    assert!(current.make_action(id).is_err());
                    assert_eq!(current, s);
                }
            }
        }
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
fn resource_full_streams_states_totals_and_undo_match_scalar_both_dice() {
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
                        phase::DISCARD
                            | phase::YEAR_OF_PLENTY
                            | phase::MONOPOLY
                            | phase::TRADE_TAKE
                    ) {
                        check(&current);
                        let Choice::Action(id) = choice else {
                            unreachable!()
                        };
                        current = transition(current, id);
                    } else {
                        current = choice.apply(&current).unwrap();
                    }
                    reference = choice.apply(&reference).unwrap();
                    assert_eq!(current, reference);
                    assert_eq!(current.canonical_bytes(), reference.canonical_bytes());
                    assert_eq!(current.hash(), reference.hash());
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
