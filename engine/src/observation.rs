//! What one player observes (I2) and sampling of full states that agree with
//! an observation (I3). ENCODING.md, "Observation and determinization", is
//! the contract.
use crate::{DEVELOPMENT_COUNTS, Error, RandomSource, State};

/// Sanitized snapshot. Hidden card counts are zero in public_state, including derived point caches.
/// Do not use State's omniscient hash as a policy feature.
///
/// Under the default observation rule `public_state` is therefore not a valid
/// `State`: its development cards no longer add up to the deck of 25. Read
/// its public fields, but do not apply transitions to it. Use `sample` to get
/// a full state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub public_state: State,
    pub observer: u8,
    /// Number of the opponent's unplayed cards bought on earlier turns.
    pub opponent_playable_total: u8,
    /// Number of the opponent's cards bought this turn. The purchase is public.
    pub opponent_new_total: u8,
    /// Cards left in the development deck.
    pub deck_total: u8,
}
impl State {
    /// I2: the observer sees everything except the types of the opponent's
    /// unplayed development cards and the composition of the deck. Resource
    /// hands are public in the two-player game. With `vp_cards_hidden` false
    /// the opponent's victory point cards stay visible, and with
    /// `observation_rule` 1 nothing is hidden.
    pub fn observation(&self, observer: u8) -> Result<Observation, Error> {
        if observer > 1 {
            return Err(Error::InvalidObservation);
        }
        let p = 1 - observer as usize;
        let mut o = Observation {
            public_state: *self,
            observer,
            opponent_playable_total: self.playable_development[p].iter().sum(),
            opponent_new_total: self.new_development[p].iter().sum(),
            deck_total: self.development_deck.iter().sum(),
        };
        if self.config.observation_rule == 0 {
            for t in 0..5 {
                if t != 1 || self.config.vp_cards_hidden {
                    o.public_state.playable_development[p][t] = 0;
                    o.public_state.new_development[p][t] = 0;
                }
            }
            o.public_state.development_deck = [0; 5];
            o.public_state.recompute_hand_caches();
        }
        Ok(o)
    }
}
/// The binomial coefficient. Each partial product is itself a binomial
/// coefficient, so every division is exact.
fn choose(n: u8, k: u8) -> u128 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut x = 1;
    for i in 0..k {
        x = x * (n - i) as u128 / (i + 1) as u128;
    }
    x
}
impl Observation {
    /// Sample the exchangeable-card prior, conditioned on the public winner/phase.
    /// The VP allocation is enumerated first, so an impossible observation returns an error.
    /// No rejection cap or policy-dependent card inference is used.
    pub fn sample(&self, rng: &mut impl RandomSource) -> Result<State, Error> {
        if self.observer > 1 {
            return Err(Error::InvalidObservation);
        }
        let own = self.observer as usize;
        let other = 1 - own;
        let mut base = self.public_state;
        base.config
            .validate()
            .map_err(|_| Error::InvalidObservation)?;
        base.board
            .validate()
            .map_err(|_| Error::InvalidObservation)?;
        if base.config.observation_rule == 1 {
            base.validate().map_err(|_| Error::InvalidObservation)?;
            if base.playable_development[other].iter().sum::<u8>() != self.opponent_playable_total
                || base.new_development[other].iter().sum::<u8>() != self.opponent_new_total
                || base.development_deck.iter().sum::<u8>() != self.deck_total
            {
                return Err(Error::InvalidObservation);
            }
            return Ok(base);
        }
        if base.config.observation_rule != 0 || base.development_deck != [0; 5] {
            return Err(Error::InvalidObservation);
        }
        let mut unseen = [0u8; 5];
        for t in 0..5 {
            if (t != 1 || base.config.vp_cards_hidden)
                && (base.playable_development[other][t] != 0 || base.new_development[other][t] != 0)
            {
                return Err(Error::InvalidObservation);
            }
            let known = base.spent_development[0][t] as u16
                + base.spent_development[1][t] as u16
                + base.playable_development[own][t] as u16
                + base.new_development[own][t] as u16
                + base.playable_development[other][t] as u16
                + base.new_development[other][t] as u16;
            if known > DEVELOPMENT_COUNTS[t] as u16 {
                return Err(Error::InvalidObservation);
            }
            unseen[t] = DEVELOPMENT_COUNTS[t] - known as u8;
        }
        // A complete temporary multiset lets the authoritative validator check
        // every public field before any indexing or arithmetic on caches.
        base.development_deck = unseen;
        base.validate_authoritative()
            .map_err(|_| Error::InvalidObservation)?;
        base.development_deck = [0; 5];
        base.recompute_caches();
        if base.cache != self.public_state.cache {
            return Err(Error::InvalidObservation);
        }
        let a = self
            .opponent_playable_total
            .checked_sub(base.playable_development[other].iter().sum())
            .ok_or(Error::InvalidObservation)?;
        let b = self
            .opponent_new_total
            .checked_sub(base.new_development[other].iter().sum())
            .ok_or(Error::InvalidObservation)?;
        let count = unseen.iter().map(|&x| x as u16).sum::<u16>();
        if a as u16 + b as u16 + self.deck_total as u16 != count {
            return Err(Error::InvalidObservation);
        }
        // Revealed VP cards cannot occupy hidden opponent slots. All unseen VP cards stay in the deck.
        //
        // `a` and `b` are the opponent's hidden playable and new slots.
        // `weight(x, y)` counts the ways to put x unseen VP cards among the
        // playable slots and y among the new slots, and to fill the other
        // slots with cards that are not VP cards. It is zero when that
        // assignment contradicts the public phase or winner. Only the VP
        // split can change the points, so it is drawn first from these exact
        // weights. The other slots are then filled card by card.
        let vp = unseen[1];
        let nonvp = count as u8 - vp;
        let weight = |x: u8, y: u8| -> u128 {
            if x > a
                || y > b
                || x + y > vp
                || a - x > nonvp
                || b - y > nonvp - (a - x)
                || (!base.config.vp_cards_hidden && (x > 0 || y > 0))
            {
                return 0;
            }
            let mut candidate = base;
            candidate.playable_development[other][1] += x;
            candidate.new_development[other][1] += y;
            candidate.recompute_hand_caches();
            if !candidate.observation_win_consistent() {
                return 0;
            }
            choose(vp, x)
                * choose(nonvp, a - x)
                * choose(vp - x, y)
                * choose(nonvp - (a - x), b - y)
        };
        let mut total = 0;
        for x in 0..=vp {
            for y in 0..=vp - x {
                total += weight(x, y);
            }
        }
        if total == 0 {
            return Err(Error::InvalidObservation);
        }
        let mut draw = rng.below_u128(total);
        let mut selected = None;
        'outer: for x in 0..=vp {
            for y in 0..=vp - x {
                let w = weight(x, y);
                if draw < w {
                    selected = Some((x, y));
                    break 'outer;
                }
                draw -= w;
            }
        }
        let (x, y) = selected.unwrap();
        let mut s = base;
        s.playable_development[other][1] += x;
        s.new_development[other][1] += y;
        unseen[1] -= x + y;
        for (group, slots) in [(0, a - x), (1, b - y)] {
            for _ in 0..slots {
                let total: u64 = (0..5).filter(|&t| t != 1).map(|t| unseen[t] as u64).sum();
                let mut pick = rng.below(total);
                let mut selected = 0;
                for t in [0, 2, 3, 4] {
                    if pick < unseen[t] as u64 {
                        selected = t;
                        break;
                    }
                    pick -= unseen[t] as u64;
                }
                unseen[selected] -= 1;
                if group == 0 {
                    s.playable_development[other][selected] += 1;
                } else {
                    s.new_development[other][selected] += 1;
                }
            }
        }
        s.development_deck = unseen;
        s.recompute_caches();
        s.validate().map_err(|_| Error::InvalidObservation)?;
        Ok(s)
    }
}
