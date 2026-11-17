//! I2, I3, written by an independent review. The
//! sampler's exact distribution is compared with a full enumeration of
//! distinguishable unseen cards. Observations must not depend on hidden cards.
use catan_engine::*;
use std::collections::BTreeMap;
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
    s.turn_owner = 1;
    s
}
fn ready(mut s: State) -> State {
    s.recompute_caches();
    s.normalize_awards();
    s.validate().unwrap();
    s
}
fn permutations(a: &mut [usize], start: usize, out: &mut Vec<Vec<usize>>) {
    if start == a.len() {
        out.push(a.to_vec());
        return;
    }
    for i in start..a.len() {
        a.swap(start, i);
        permutations(a, start + 1, out);
        a.swap(start, i);
    }
}
struct Draws {
    rank: u128,
    total: u128,
    cards: Vec<u8>,
}
impl RandomSource for Draws {
    fn next_u64(&mut self) -> u64 {
        panic!("Every sampler draw must use its bounded API");
    }
    fn below_u128(&mut self, bound: u128) -> u128 {
        assert_eq!(bound, self.total);
        self.rank
    }
    fn below(&mut self, bound: u64) -> u64 {
        // Simulate a uniformly shuffled sequence of distinguishable non-VP cards.
        let t = self.cards.remove(0);
        let rank = self.cards.iter().filter(|&&c| c < t).count() as u64;
        assert_eq!(bound, self.cards.len() as u64 + 1);
        rank
    }
}
#[test]
fn i3_exact_nonvp_and_vp_joint_prior_subtracts_seen_and_spent_cards() {
    let mut s = base();
    s.spent_development[0][0] = 10;
    s.playable_development[0] = [2, 4, 1, 2, 2];
    s.playable_development[1][0] = 1;
    s.new_development[1][2] = 1;
    s.development_deck = [1, 1, 0, 0, 0];
    let s = ready(s);
    assert_eq!(s.largest_army_holder, 0);
    let obs = s.observation(0).unwrap();
    // Unseen cards are distinguishable K0,K1,V,R. The first card goes to
    // playable, the second to new, and the rest to the deck.
    let types = [0u8, 0, 1, 2];
    let mut full_orders = Vec::new();
    permutations(&mut [0, 1, 2, 3], 0, &mut full_orders);
    let mut expected = BTreeMap::new();
    for order in full_orders {
        *expected
            .entry((types[order[0]], types[order[1]]))
            .or_insert(0usize) += 1;
    }
    // The sampler's VP step has twelve integer ranks. Each rank is crossed
    // with all six permutations of the three non-VP cards.
    let nonvp = [0u8, 0, 2];
    let mut orders = Vec::new();
    permutations(&mut [0, 1, 2], 0, &mut orders);
    let mut actual = BTreeMap::new();
    for rank in 0..12 {
        for order in &orders {
            let mut rng = Draws {
                rank,
                total: 12,
                cards: order.iter().map(|&i| nonvp[i]).collect(),
            };
            let sampled = obs.sample(&mut rng).unwrap();
            assert_eq!(sampled.observation(0).unwrap(), obs);
            assert_eq!(sampled.spent_development, s.spent_development);
            assert_eq!(sampled.playable_development[0], s.playable_development[0]);
            let x = sampled.playable_development[1]
                .iter()
                .position(|&c| c == 1)
                .unwrap() as u8;
            let y = sampled.new_development[1]
                .iter()
                .position(|&c| c == 1)
                .unwrap() as u8;
            *actual.entry((x, y)).or_insert(0usize) += 1;
        }
    }
    for n in expected.values_mut() {
        *n *= 3;
    } // 72 sampler paths versus 24 full permutations.
    assert_eq!(actual, expected);
    println!("I3: all 72 weighted sampler paths equal the 24-card-permutation oracle");
}
#[test]
fn i2_observations_are_equal_after_hidden_cards_exchange() {
    let mut s = base();
    s.playable_development[1] = [1, 1, 0, 0, 0];
    s.new_development[1] = [0, 0, 1, 0, 0];
    s.development_deck = [13, 4, 1, 2, 2];
    let s = ready(s);
    let mut changed = s;
    changed.playable_development[1] = [0, 0, 0, 1, 1];
    changed.new_development[1] = [1, 0, 0, 0, 0];
    changed.development_deck = [13, 5, 2, 1, 1];
    let changed = ready(changed);
    assert_ne!(s.canonical_bytes(), changed.canonical_bytes());
    assert_eq!(s.observation(0).unwrap(), changed.observation(0).unwrap());
    assert_ne!(s.observation(1).unwrap(), changed.observation(1).unwrap());
}
#[test]
fn i3_impossible_winner_and_public_capacities_are_rejected() {
    let mut s = base();
    s.node_occupancy[0] = 4;
    s.config.victory_points = 3;
    s.playable_development[1][1] = 1;
    s.development_deck[1] -= 1;
    s.phase = phase::TERMINAL;
    s.winner = 1;
    let s = ready(s);
    let obs = s.observation(0).unwrap();
    for seed in 0..12 {
        let sampled = obs.sample(&mut SplitMix64::new(seed)).unwrap();
        assert_eq!(sampled.cache.total_points[1], 3);
        assert_eq!(sampled.winner(), Some(1));
    }
    let mut bad = obs;
    bad.opponent_playable_total = 0;
    bad.deck_total += 1;
    assert_eq!(
        bad.sample(&mut SplitMix64::new(1)),
        Err(Error::InvalidObservation)
    );
}
#[test]
fn i2_hidden_discards_can_be_recovered_exactly_from_public_snapshots() {
    let mut s = base();
    s.config.discards_public = false;
    s.phase = phase::PRE_ROLL;
    s.resources[1] = [5, 5, 0, 0, 0];
    s.bank = [14, 14, 19, 19, 19];
    let s = ready(s).apply_action(0).unwrap().apply_outcome(5).unwrap();
    let before = s.observation(0).unwrap();
    let event = s.public_decision_event(206).unwrap();
    assert_eq!(event.kind, PublicDecisionKind::Discard { resource: None });
    let after = s.apply_action(206).unwrap().observation(0).unwrap();
    let inferred: [u8; 5] = std::array::from_fn(|r| {
        before.public_state.resources[1][r] - after.public_state.resources[1][r]
    });
    assert_eq!(inferred, [1, 0, 0, 0, 0]);
    println!(
        "I2: discards_public=false redacts event but exposes discarded resource 0 by subtraction"
    );
}
