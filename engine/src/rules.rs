//! Legal actions, chance outcomes and the transition bodies.
//!
//! Action ids are the global table of ENCODING.md, "Global actions". The
//! literals in this file are those ids:
//!
//! | Ids | Action |
//! |---|---|
//! | 0 | ROLL |
//! | 1 | END_TURN |
//! | 2 | BUY_DEVELOPMENT |
//! | 3, 4, 5, 6 | PLAY knight, road building, year of plenty, monopoly |
//! | 7..=78 | PLACE_ROAD on edge `id - 7` |
//! | 79..=132 | PLACE_SETTLEMENT on node `id - 79` |
//! | 133..=186 | PLACE_CITY on node `id - 133` |
//! | 187..=205 | MOVE_ROBBER to hex `id - 187` |
//! | 206..=210 | DISCARD one card of resource `id - 206` |
//! | 211..=215 | CHOOSE_RESOURCE `id - 211` for year of plenty or monopoly |
//! | 216..=220 | TRADE_GIVE resource `id - 216` |
//! | 221..=225 | TRADE_TAKE resource `id - 221` |
//!
//! Chance ids are a separate namespace: 0..=10 for dice totals 2..=12,
//! 11..=15 for a stolen resource, 16..=20 for a drawn development type.
//!
//! The table has no action for a trade between players (T7) and none for a
//! timer (T8). A choice with several parts is a sequence of small actions
//! (E4): one card per DISCARD, one card per CHOOSE_RESOURCE, one road per
//! PLACE_ROAD, and a bank trade as TRADE_GIVE followed by TRADE_TAKE.
//!
//! Two routes answer "is this action legal". `is_legal_action` checks one id
//! against the occupancy arrays and does not use the cached site sets. It is
//! the reference. `legal_actions` enumerates all legal ids in ascending order
//! and reads the cached site sets where it can. The property tests require the
//! two to agree in every state they visit.
//!
//! Every transition ends in `normalize_transition`, which refreshes the hand
//! caches, runs the win check (A4), completes effects that have no choice
//! left, and clears the fields that are inactive in the new phase.
use crate::{Error, NONE, Outcome, State, phase::*, topology::*};

// U1: building costs by resource (wood, brick, sheep, wheat, ore).
const ROAD_COST: [u8; 5] = [1, 1, 0, 0, 0];
const SETTLEMENT_COST: [u8; 5] = [1, 1, 1, 1, 0];
const CITY_COST: [u8; 5] = [0, 0, 0, 2, 3];
const DEVELOPMENT_COST: [u8; 5] = [0, 0, 1, 1, 1];

/// A public decision event contains no chance outcome or development draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicDecisionEvent {
    pub actor: u8,
    pub kind: PublicDecisionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicDecisionKind {
    Action(u16),
    Discard { resource: Option<u8> },
}

impl State {
    /// Describe a legal decision without applying it. Resource snapshots stay
    /// public, so a redacted discard can still be inferred by subtraction.
    pub fn public_decision_event(&self, action: u16) -> Result<PublicDecisionEvent, Error> {
        if !self.is_legal_action(action) {
            return Err(Error::InvalidAction);
        }
        Ok(PublicDecisionEvent {
            actor: self.actor().ok_or(Error::InvalidAction)?,
            kind: if self.phase == DISCARD {
                PublicDecisionKind::Discard {
                    resource: self.config.discards_public.then_some((action - 206) as u8),
                }
            } else {
                PublicDecisionKind::Action(action)
            },
        })
    }
    fn affordable(&self, cost: &[u8; 5]) -> bool {
        self.resources[self.turn_owner as usize]
            .iter()
            .zip(cost)
            .all(|(&have, &need)| have >= need)
    }
    /// U1: every payment goes to the bank.
    fn pay(&mut self, cost: &[u8; 5]) {
        let p = self.turn_owner as usize;
        for r in 0..5 {
            self.resources[p][r] -= cost[r];
            self.bank[r] += cost[r];
        }
    }
    /// S2: the node is empty and so is every neighbouring node.
    fn distance_clear(&self, n: usize) -> bool {
        self.node_occupancy[n] == 0
            && NODE_NEIGHBORS[n]
                .iter()
                .all(|&v| v == NONE || self.node_occupancy[v as usize] == 0)
    }
    /// Whether the player to move may put a road on edge `e`, apart from its
    /// cost. The edge must be empty and a road piece must be left (U2). A
    /// setup road must touch the settlement just placed (S1). Every other
    /// road must connect to the player's network (U3).
    pub fn road_legal(&self, e: usize) -> bool {
        if e >= 72
            || self.edge_occupancy[e] != 0
            || self.cache.pieces_left[self.turn_owner as usize][0] == 0
        {
            return false;
        }
        if self.phase == SETUP_ROAD {
            return EDGE_NODES[e].contains(&self.setup_anchor);
        }
        self.road_connected(e)
    }
    /// U3: an endpoint connects when it holds the player's own building, or
    /// when it is empty and touches the player's own road. An endpoint with an
    /// opponent building never connects, because a road may not continue
    /// through it.
    fn road_connected(&self, e: usize) -> bool {
        EDGE_NODES[e].iter().any(|&n| {
            let x = self.node_occupancy[n as usize];
            if x != 0 {
                (x - 1) % 2 == self.turn_owner
            } else {
                NODE_EDGES[n as usize]
                    .iter()
                    .any(|&j| j != NONE && self.edge_occupancy[j as usize] == self.turn_owner + 1)
            }
        })
    }
    /// D5: the card needs a road piece in stock. It needs a legal edge as
    /// well only when `road_building_needs_legal_edge` is set.
    fn road_building_playable(&self) -> bool {
        self.cache.pieces_left[self.turn_owner as usize][0] > 0
            && (!self.config.road_building_needs_legal_edge
                || (0..72).any(|e| self.edge_occupancy[e] == 0 && self.road_connected(e)))
    }
    /// R1: the robber must leave its hex. R2: with the friendly robber, a hex
    /// is closed when any building on it belongs to a player with 2 or fewer
    /// visible points. This protects the mover's own buildings too.
    fn robber_legal(&self, h: usize) -> bool {
        h < 19
            && h != self.robber_hex as usize
            && (!self.config.friendly_robber
                || HEX_NODES[h].iter().all(|&n| {
                    let x = self.node_occupancy[n as usize];
                    x == 0 || self.cache.visible_points[((x - 1) % 2) as usize] > 2
                }))
    }
    /// The reference legality test for one action id. See the module
    /// documentation for the id ranges.
    pub fn is_legal_action(&self, a: u16) -> bool {
        let p = self.turn_owner as usize;
        match self.phase {
            SETUP_SETTLEMENT => {
                (79..133).contains(&a)
                    && self.cache.pieces_left[p][1] > 0
                    && self.distance_clear((a - 79) as usize)
            }
            SETUP_ROAD | ROAD_BUILDING => (7..79).contains(&a) && self.road_legal((a - 7) as usize),
            PRE_ROLL | MAIN => {
                // T1, D2, D3: a card can be played before or after the roll,
                // only from the playable group and only once per turn. The
                // table maps play ids 3..=6 to development types, skipping
                // type 1, the victory point card, which is never played.
                if (3..=6).contains(&a) {
                    let t = [0, 2, 3, 4][(a - 3) as usize];
                    return !self.development_played
                        && self.playable_development[p][t] > 0
                        && (a != 4 || self.road_building_playable());
                }
                if self.phase == PRE_ROLL {
                    return a == 0;
                }
                match a {
                    1 => true,
                    2 => {
                        self.development_deck.iter().any(|&n| n > 0)
                            && self.affordable(&DEVELOPMENT_COST)
                    }
                    7..=78 => self.affordable(&ROAD_COST) && self.road_legal((a - 7) as usize),
                    79..=132 => {
                        let n = (a - 79) as usize;
                        self.cache.pieces_left[p][1] > 0
                            && self.affordable(&SETTLEMENT_COST)
                            && self.distance_clear(n)
                            && NODE_EDGES[n].iter().any(|&e| {
                                e != NONE && self.edge_occupancy[e as usize] == self.turn_owner + 1
                            })
                    }
                    133..=186 => {
                        self.cache.pieces_left[p][2] > 0
                            && self.affordable(&CITY_COST)
                            && self.node_occupancy[(a - 133) as usize] == self.turn_owner + 1
                    }
                    // X1, X2: the player can pay the best rate, and the bank
                    // holds at least one card of another resource to take.
                    216..=220 => {
                        let r = (a - 216) as usize;
                        self.resources[p][r] >= self.cache.trade_rates[p][r]
                            && (0..5).any(|s| s != r && self.bank[s] > 0)
                    }
                    _ => false,
                }
            }
            DISCARD => {
                (206..211).contains(&a)
                    && self.resources[self.actor().unwrap() as usize][(a - 206) as usize] > 0
            }
            ROBBER => (187..206).contains(&a) && self.robber_legal((a - 187) as usize),
            YEAR_OF_PLENTY => (211..216).contains(&a) && self.bank[(a - 211) as usize] > 0,
            MONOPOLY => (211..216).contains(&a),
            TRADE_TAKE => {
                (221..226).contains(&a)
                    && self.trade_give < 5
                    && a - 221 != self.trade_give as u16
                    && self.bank[(a - 221) as usize] > 0
                    && self.resources[p][self.trade_give as usize]
                        >= self.cache.trade_rates[p][self.trade_give as usize]
            }
            _ => false,
        }
    }
    /// Write every legal action id into `out` in ascending order and return
    /// the count (E3). It allocates nothing. A chance or terminal node gives 0.
    pub fn legal_actions(&self, out: &mut [u16; 226]) -> usize {
        // MAIN has its own path. It emits the fixed ids first and then each
        // building family from its cached site set.
        if self.phase == MAIN {
            let p = self.turn_owner as usize;
            let mut n = 1;
            out[0] = 1;
            if self.development_deck.iter().any(|&count| count > 0)
                && self.affordable(&DEVELOPMENT_COST)
            {
                out[n] = 2;
                n += 1;
            }
            if !self.development_played {
                for (i, t) in [0, 2, 3, 4].into_iter().enumerate() {
                    if self.playable_development[p][t] > 0
                        && (t != 2 || self.road_building_playable())
                    {
                        out[n] = 3 + i as u16;
                        n += 1;
                    }
                }
            }
            // Geometry is incremental. Enumerate each family in stable ID order
            // after its shared stock and affordability gates.
            if self.cache.pieces_left[p][0] > 0 && self.affordable(&ROAD_COST) {
                let mut sites = self.cache.road_sites[p];
                while sites != 0 {
                    out[n] = 7 + sites.trailing_zeros() as u16;
                    n += 1;
                    sites &= sites - 1;
                }
            }
            if self.cache.pieces_left[p][1] > 0 && self.affordable(&SETTLEMENT_COST) {
                let mut sites = self.cache.settlement_sites[p];
                while sites != 0 {
                    out[n] = 79 + sites.trailing_zeros() as u16;
                    n += 1;
                    sites &= sites - 1;
                }
            }
            if self.cache.pieces_left[p][2] > 0 && self.affordable(&CITY_COST) {
                let mut sites = self.cache.city_sites[p];
                while sites != 0 {
                    out[n] = 133 + sites.trailing_zeros() as u16;
                    n += 1;
                    sites &= sites - 1;
                }
            }
            for a in 216..221 {
                if self.is_legal_action(a) {
                    out[n] = a;
                    n += 1;
                }
            }
            return n;
        }
        // Each remaining phase either returns its ids directly or yields an id
        // range `(lo, hi)` that the scalar loop at the end filters. Only the
        // protected ROBBER case uses that loop.
        let (lo, hi) = match self.phase {
            SETUP_SETTLEMENT => {
                if self.cache.pieces_left[self.turn_owner as usize][1] == 0 {
                    return 0;
                }
                // Includes only empty nodes whose neighbors are all empty.
                let mut sites = self.cache.distance_sites;
                let mut n = 0;
                while sites != 0 {
                    out[n] = 79 + sites.trailing_zeros() as u16;
                    n += 1;
                    sites &= sites - 1;
                }
                return n;
            }
            SETUP_ROAD | ROAD_BUILDING => {
                let p = self.turn_owner as usize;
                if self.cache.pieces_left[p][0] == 0 {
                    return 0;
                }
                let mut sites = if self.phase == SETUP_ROAD {
                    if self.setup_anchor >= 54 {
                        return 0;
                    }
                    let mut incident = 0u128;
                    for &edge in &NODE_EDGES[self.setup_anchor as usize] {
                        if edge != NONE && self.edge_occupancy[edge as usize] == 0 {
                            incident |= 1u128 << edge;
                        }
                    }
                    incident
                } else {
                    self.cache.road_sites[p]
                };
                // Set-bit order preserves ascending IDs independently of the
                // order of the setup anchor's three incident-edge entries.
                let mut n = 0;
                while sites != 0 {
                    out[n] = 7 + sites.trailing_zeros() as u16;
                    n += 1;
                    sites &= sites - 1;
                }
                return n;
            }
            PRE_ROLL => {
                let p = self.turn_owner as usize;
                out[0] = 0;
                let mut n = 1;
                if !self.development_played {
                    // The action table puts the four playable card IDs at 3..=6.
                    for (i, t) in [0, 2, 3, 4].into_iter().enumerate() {
                        if self.playable_development[p][t] > 0
                            && (t != 2 || self.road_building_playable())
                        {
                            out[n] = 3 + i as u16;
                            n += 1;
                        }
                    }
                }
                return n;
            }
            // Not reached: MAIN returned above. This arm is a leftover.
            MAIN => (1, 221),
            // T5: the actor may discard any resource it holds.
            DISCARD => {
                let actor = self.actor().unwrap() as usize;
                let mut n = 0;
                for (resource, &count) in self.resources[actor].iter().enumerate() {
                    if count > 0 {
                        out[n] = 206 + resource as u16;
                        n += 1;
                    }
                }
                return n;
            }
            ROBBER
                if !self.config.friendly_robber
                    || (self.cache.visible_points[0] > 2 && self.cache.visible_points[1] > 2) =>
            {
                // R2 protects no player here. Only the current hex is excluded.
                let mut n = 0;
                for hex in 0..19u8 {
                    if hex != self.robber_hex {
                        out[n] = 187 + u16::from(hex);
                        n += 1;
                    }
                }
                return n;
            }
            ROBBER => (187, 206),
            YEAR_OF_PLENTY => {
                let mut n = 0;
                for (resource, &count) in self.bank.iter().enumerate() {
                    if count > 0 {
                        out[n] = 211 + resource as u16;
                        n += 1;
                    }
                }
                return n;
            }
            MONOPOLY => {
                out[..5].copy_from_slice(&[211, 212, 213, 214, 215]);
                return 5;
            }
            TRADE_TAKE => {
                // X1: reject an invalid give ID before indexing, and retain
                // its funding guard even on malformed pending-trade controls.
                if self.trade_give >= 5 {
                    return 0;
                }
                let give = self.trade_give as usize;
                let p = self.turn_owner as usize;
                if self.resources[p][give] < self.cache.trade_rates[p][give] {
                    return 0;
                }
                let mut n = 0;
                for (resource, &count) in self.bank.iter().enumerate() {
                    if resource != give && count > 0 {
                        out[n] = 221 + resource as u16;
                        n += 1;
                    }
                }
                return n;
            }
            _ => (0, 0),
        };
        let mut n = 0;
        for a in lo..hi {
            if self.is_legal_action(a) {
                out[n] = a;
                n += 1;
            }
        }
        n
    }
    /// Write every positive-mass outcome of a chance node into `out` in
    /// ascending id order and return the count (E2). A decision or terminal
    /// node gives 0. A steal draws one card uniformly from the opponent's hand
    /// (R3), and a development draw one card uniformly from the deck (D1).
    pub fn chance_outcomes(&self, out: &mut [Outcome; 11]) -> usize {
        let mut n = 0;
        match self.phase {
            DICE => {
                let dist = self.dice_probabilities();
                for (i, &weight) in dist.weights.iter().enumerate() {
                    if weight > 0 {
                        out[n] = Outcome {
                            id: i as u16,
                            weight,
                            total: dist.total,
                        };
                        n += 1;
                    }
                }
            }
            STEAL | DEVELOPMENT_DRAW => {
                let (counts, base) = if self.phase == STEAL {
                    (&self.resources[1 - self.turn_owner as usize], 11)
                } else {
                    (&self.development_deck, 16)
                };
                let total = counts.iter().map(|&x| x as u128).sum();
                for (i, &c) in counts.iter().enumerate() {
                    if c > 0 {
                        out[n] = Outcome {
                            id: base + i as u16,
                            weight: c as u128,
                            total,
                        };
                        n += 1;
                    }
                }
            }
            _ => {}
        }
        n
    }
    /// Return from a pending effect to PRE_ROLL or MAIN.
    fn finish_effect(&mut self) {
        self.phase = self.resume_phase;
    }
    /// Reset every field that has no meaning in the current phase, so that
    /// equal game situations have equal canonical bytes (ENCODING.md, "Award
    /// and win normalization").
    fn clear_inactive(&mut self) {
        if self.phase != SETUP_ROAD {
            self.setup_anchor = NONE;
        }
        if self.phase != TRADE_TAKE {
            self.trade_give = NONE;
        }
        if !matches!(self.phase, ROAD_BUILDING | YEAR_OF_PLENTY) {
            self.choices_remaining = 0;
        }
        if self.phase != DISCARD {
            self.discard_remaining = [0; 2];
        }
        if !matches!(
            self.phase,
            DISCARD
                | ROBBER
                | STEAL
                | DEVELOPMENT_DRAW
                | ROAD_BUILDING
                | YEAR_OF_PLENTY
                | MONOPOLY
                | TRADE_TAKE
        ) {
            self.resume_phase = NONE;
        }
        if self.phase != TERMINAL {
            self.winner = NONE;
        }
    }
    /// A4: only the turn owner can win, and only after setup. The opponent's
    /// total is not tested here, so an opponent at the target wins at the
    /// start of their own turn.
    fn check_win(&mut self) {
        if self.phase > SETUP_ROAD
            && self.cache.total_points[self.turn_owner as usize] as u16
                >= self.config.victory_points
        {
            self.phase = TERMINAL;
            self.winner = self.turn_owner;
        }
    }
    /// The common tail of every transition. The win check comes first, so a
    /// win interrupts a pending effect. Then forced steps complete without a
    /// trace step of their own: discards that are done lead to the robber, and
    /// a robber, road building or year of plenty effect with no choice left
    /// returns to its resume phase.
    fn normalize_transition(&mut self) {
        self.recompute_hand_caches();
        self.check_win();
        if self.phase == DISCARD && self.discard_remaining == [0; 2] {
            self.phase = ROBBER;
        }
        if (self.phase == ROBBER && !(0..19).any(|h| self.robber_legal(h)))
            || (self.phase == ROAD_BUILDING
                && (self.choices_remaining == 0 || !(0..72).any(|e| self.road_legal(e))))
            || (self.phase == YEAR_OF_PLENTY
                && (self.choices_remaining == 0 || self.bank.iter().all(|&n| n == 0)))
        {
            self.finish_effect();
        }
        self.clear_inactive();
    }
    /// Apply one decision without mutating the input. Public fixture edits must rebuild caches first.
    pub fn apply_action(&self, a: u16) -> Result<Self, Error> {
        let mut s = *self;
        s.advance_action(a)?;
        Ok(s)
    }
    // Internal rollout path. The caller owns an invariant-valid constructor or
    // decoded state, with current caches. Every returned error is atomic.
    pub(crate) fn advance_action(&mut self, a: u16) -> Result<(), Error> {
        if !self.is_legal_action(a) {
            return Err(Error::InvalidAction);
        }
        self.apply_action_in_place(a)
    }
    // Checked advance, sparse make and choose_and_advance share this body. The
    // action must be validated against this unchanged state's legal actions.
    // Representation and setup-grant failures still need atomic preflights.
    pub(crate) fn apply_action_in_place(&mut self, a: u16) -> Result<(), Error> {
        let s = self;
        let p = s.turn_owner as usize;
        match s.phase {
            // S1 to S3: place the settlement. The second settlement of each
            // player (pairs 2 and 3) takes one card for each adjacent hex that
            // is not the desert.
            SETUP_SETTLEMENT => {
                let n = (a - 79) as usize;
                let mut grants = [0u8; 5];
                if s.setup_pairs_done >= 2 {
                    for &h in &NODE_HEXES[n] {
                        if h != NONE {
                            let r = s.board.terrain[h as usize];
                            if r < 5 {
                                grants[r as usize] += 1;
                            }
                        }
                    }
                }
                // Check the complete grant before occupancy, control, resource
                // or cache writes, including a depleted-bank fixture.
                if (0..5).any(|r| s.bank[r] < grants[r]) {
                    return Err(Error::InvalidState);
                }
                s.node_occupancy[n] = s.turn_owner + 1;
                s.update_geometry_settlement(n);
                s.setup_anchor = n as u8;
                s.phase = SETUP_ROAD;
                for (r, &grant) in grants.iter().enumerate() {
                    s.bank[r] -= grant;
                    s.resources[p][r] += grant;
                }
                s.recompute_non_road_board_caches();
                s.recompute_road_cut_by_settlement(n);
                s.normalize_cached_awards();
            }
            // S1, S4: the road closes the pair. The snake order is first,
            // second, second, first. After the fourth pair the first player
            // starts turn 1 before the roll.
            SETUP_ROAD => {
                s.edge_occupancy[(a - 7) as usize] = s.turn_owner + 1;
                s.update_geometry_road((a - 7) as usize);
                s.setup_pairs_done += 1;
                s.cache.pieces_left[p][0] -= 1;
                s.cache.road_lengths[p] =
                    s.longest_road_after_addition(s.turn_owner, (a - 7) as usize);
                s.normalize_cached_awards();
                if s.setup_pairs_done == 4 {
                    s.turn_owner = s.config.first_player;
                    s.phase = PRE_ROLL;
                    s.turn_number = 1;
                } else {
                    s.turn_owner = [
                        s.config.first_player,
                        1 - s.config.first_player,
                        1 - s.config.first_player,
                        s.config.first_player,
                    ][s.setup_pairs_done as usize];
                    s.phase = SETUP_SETTLEMENT;
                }
            }
            PRE_ROLL | MAIN => match a {
                // ROLL (T2): prepare the dice history, then the DICE chance
                // node picks the total.
                0 => {
                    s.dice
                        .prepare_roll(s.config.dice == 0, s.turn_owner)
                        .map_err(|_| Error::InvalidState)?;
                    s.phase = DICE;
                }
                // END_TURN (T6): cards bought this turn become playable (D2),
                // and the opponent starts a turn before the roll.
                1 => {
                    s.check_win();
                    if s.phase != TERMINAL {
                        // A winning outgoing player ends the game before this
                        // check. On overflow, check_win made no state change.
                        let next_turn = s
                            .turn_number
                            .checked_add(1)
                            .ok_or(Error::RepresentationOverflow)?;
                        for t in 0..5 {
                            s.playable_development[p][t] += s.new_development[p][t];
                            s.new_development[p][t] = 0;
                        }
                        s.turn_number = next_turn;
                        s.turn_owner = 1 - s.turn_owner;
                        s.development_played = false;
                        s.last_dice = 0;
                        s.phase = PRE_ROLL;
                    }
                }
                // BUY_DEVELOPMENT (U1, D1): pay, then a chance node draws the card.
                2 => {
                    s.pay(&DEVELOPMENT_COST);
                    s.resume_phase = MAIN;
                    s.phase = DEVELOPMENT_DRAW;
                }
                // PLAY (D3 to D6): the card is spent at once, before its
                // choices. A knight (D4) leads to the robber without any
                // discard. It updates Largest Army here (A1), so the robber
                // move that follows already sees the new visible points (R2).
                3..=6 => {
                    let t = [0, 2, 3, 4][(a - 3) as usize];
                    s.playable_development[p][t] -= 1;
                    s.spent_development[p][t] += 1;
                    s.development_played = true;
                    s.resume_phase = s.phase;
                    s.phase = match a {
                        3 => ROBBER,
                        4 => ROAD_BUILDING,
                        5 => YEAR_OF_PLENTY,
                        _ => MONOPOLY,
                    };
                    if a == 4 || a == 5 {
                        s.choices_remaining = 2;
                    }
                    if a == 3 {
                        let h = s.largest_army_holder;
                        if s.spent_development[p][0] >= 3
                            && (h == NONE
                                || s.spent_development[p][0] > s.spent_development[h as usize][0])
                        {
                            s.largest_army_holder = p as u8;
                        }
                        s.refresh_points();
                    }
                }
                // Paid road (U1, U3). Each placement arm below writes the
                // occupancy, updates the cached site sets and road lengths,
                // and then resolves the awards (A2, A3).
                7..=78 => {
                    s.pay(&ROAD_COST);
                    s.edge_occupancy[(a - 7) as usize] = s.turn_owner + 1;
                    s.update_geometry_road((a - 7) as usize);
                    s.cache.pieces_left[p][0] -= 1;
                    s.cache.road_lengths[p] =
                        s.longest_road_after_addition(s.turn_owner, (a - 7) as usize);
                    s.normalize_cached_awards();
                }
                // Paid settlement (U1, U4). It can cut the opponent's road.
                79..=132 => {
                    s.pay(&SETTLEMENT_COST);
                    let n = (a - 79) as usize;
                    s.node_occupancy[n] = s.turn_owner + 1;
                    s.update_geometry_settlement(n);
                    s.recompute_non_road_board_caches();
                    s.recompute_road_cut_by_settlement(n);
                    s.normalize_cached_awards();
                }
                // City (U1, U5). Occupancy `turn_owner + 3` is that player's
                // city. The settlement piece returns to stock (U2).
                133..=186 => {
                    s.pay(&CITY_COST);
                    s.node_occupancy[(a - 133) as usize] = s.turn_owner + 3;
                    s.update_geometry_city((a - 133) as usize);
                    s.recompute_non_road_board_caches();
                    s.normalize_cached_awards();
                }
                // TRADE_GIVE (X1): record the offer only. No card moves until
                // TRADE_TAKE, so the trade is atomic.
                216..=220 => {
                    s.trade_give = (a - 216) as u8;
                    s.resume_phase = MAIN;
                    s.phase = TRADE_TAKE;
                }
                _ => unreachable!(),
            },
            // T5: one card per action. `normalize_transition` moves on to the
            // robber when both counts reach zero.
            DISCARD => {
                let actor = s.actor().unwrap() as usize;
                let r = (a - 206) as usize;
                s.resources[actor][r] -= 1;
                s.bank[r] += 1;
                s.discard_remaining[actor] -= 1;
            }
            // R1, R3: move the robber. A STEAL chance node follows only when
            // the opponent has a building on the new hex and holds a card.
            ROBBER => {
                s.update_production_robber((a - 187) as u8);
                s.robber_hex = (a - 187) as u8;
                if s.cache.resource_totals[1 - p] > 0
                    && HEX_NODES[s.robber_hex as usize].iter().any(|&n| {
                        let x = s.node_occupancy[n as usize];
                        x != 0 && (x - 1) % 2 != s.turn_owner
                    })
                {
                    s.phase = STEAL;
                } else {
                    s.finish_effect();
                }
            }
            // D5: one free road. `normalize_transition` ends the effect when
            // no choice, no piece or no legal edge is left.
            ROAD_BUILDING => {
                s.edge_occupancy[(a - 7) as usize] = s.turn_owner + 1;
                s.update_geometry_road((a - 7) as usize);
                s.choices_remaining -= 1;
                s.cache.pieces_left[p][0] -= 1;
                s.cache.road_lengths[p] =
                    s.longest_road_after_addition(s.turn_owner, (a - 7) as usize);
                s.normalize_cached_awards();
            }
            // D6: one card from the bank per action, up to two.
            YEAR_OF_PLENTY => {
                let r = (a - 211) as usize;
                s.bank[r] -= 1;
                s.resources[p][r] += 1;
                s.choices_remaining -= 1;
            }
            // D6: the opponent hands over every card of the named resource.
            MONOPOLY => {
                let r = (a - 211) as usize;
                s.resources[p][r] += s.resources[1 - p][r];
                s.resources[1 - p][r] = 0;
                s.finish_effect();
            }
            // X1, X2: pay the rate for the offered resource and take one card.
            TRADE_TAKE => {
                let r = s.trade_give as usize;
                let t = (a - 221) as usize;
                let rate = s.cache.trade_rates[p][r];
                s.resources[p][r] = s.resources[p][r]
                    .checked_sub(rate)
                    .ok_or(Error::InvalidState)?;
                s.bank[r] += rate;
                s.bank[t] -= 1;
                s.resources[p][t] += 1;
                s.finish_effect();
            }
            _ => unreachable!(),
        }
        s.normalize_transition();
        Ok(())
    }
    /// Apply one chance outcome without mutating the input. The id must have
    /// positive mass in this state.
    pub fn apply_outcome(&self, id: u16) -> Result<Self, Error> {
        let mut s = *self;
        s.advance_outcome(id)?;
        Ok(s)
    }
    /// An outcome is valid exactly when `chance_outcomes` lists it.
    pub(crate) fn check_outcome(&self, id: u16) -> Result<(), Error> {
        let mut outcomes = [Outcome::default(); 11];
        let n = self.chance_outcomes(&mut outcomes);
        if !outcomes[..n].iter().any(|o| o.id == id) {
            return Err(Error::InvalidOutcome);
        }
        Ok(())
    }
    // Same validity contract as advance_action. Dice failures occur before
    // other state writes, and DiceState commits its history only on success.
    pub(crate) fn advance_outcome(&mut self, id: u16) -> Result<(), Error> {
        self.check_outcome(id)?;
        self.apply_outcome_in_place(id)
    }
    // The outcome must have positive mass in this unchanged state. Retain
    // DiceState::apply_total's selected-support and counter-overflow checks.
    pub(crate) fn apply_outcome_in_place(&mut self, id: u16) -> Result<(), Error> {
        let s = self;
        let p = s.turn_owner as usize;
        match s.phase {
            DICE => {
                let total = id as u8 + 2;
                s.dice
                    .apply_total_with_rounding(
                        s.config.dice == 0,
                        s.turn_owner,
                        total,
                        s.config.reference_dice_rounding,
                    )
                    .map_err(|e| match e {
                        crate::dice::DiceError::CounterOverflow => Error::RepresentationOverflow,
                        _ => Error::InvalidOutcome,
                    })?;
                s.last_dice = total;
                // T5: both discard counts are fixed now, from the hands at
                // this instant: half the hand, rounded down, for a hand above
                // the limit.
                if total == 7 {
                    s.resume_phase = MAIN;
                    s.phase = DISCARD;
                    for q in 0..2 {
                        let count = s.cache.resource_totals[q];
                        s.discard_remaining[q] = if count > s.config.discard_limit {
                            count / 2
                        } else {
                            0
                        };
                    }
                } else {
                    // T3, T4: production is settled for each resource on its
                    // own. When the bank covers both players, both are paid.
                    // On a shortage, the default rule gives what is left to a
                    // sole claimant and nothing when both players claim.
                    let demand = s.cache.production[(total - 2) as usize];
                    for r in 0..5 {
                        let sum = demand[0][r] as u16 + demand[1][r] as u16;
                        if sum <= s.bank[r] as u16 {
                            for q in 0..2 {
                                s.bank[r] -= demand[q][r];
                                s.resources[q][r] += demand[q][r];
                            }
                        } else if s.config.shortage_rule == 0
                            && ((demand[0][r] == 0) != (demand[1][r] == 0))
                        {
                            let q = if demand[0][r] > 0 { 0 } else { 1 };
                            s.resources[q][r] += s.bank[r];
                            s.bank[r] = 0;
                        }
                    }
                    s.phase = MAIN;
                }
            }
            // R3: the turn owner takes the drawn card from the opponent.
            STEAL => {
                let r = (id - 11) as usize;
                s.resources[1 - p][r] -= 1;
                s.resources[p][r] += 1;
                s.finish_effect();
            }
            // D2, D7: the card joins the new group. A victory point card
            // counts at once, so `normalize_transition` can end the game here.
            DEVELOPMENT_DRAW => {
                let t = (id - 16) as usize;
                s.development_deck[t] -= 1;
                s.new_development[p][t] += 1;
                s.finish_effect();
            }
            _ => unreachable!(),
        }
        s.normalize_transition();
        Ok(())
    }
    /// Uniform rollout selection and mutation share one exclusive state borrow.
    /// No unchecked choice escapes: choose enumerates this exact state, and no
    /// state write occurs between enumeration and the private mutation body.
    #[cfg(test)]
    pub(crate) fn choose_and_advance(
        &mut self,
        rng: &mut impl crate::RandomSource,
    ) -> Result<Option<crate::runner::Choice>, Error> {
        use crate::runner::{Choice, choose};
        let Some(choice) = choose(self, rng) else {
            return Ok(None);
        };
        match choice {
            Choice::Action(id) => self.apply_action_in_place(id)?,
            Choice::Outcome(id) => self.apply_outcome_in_place(id)?,
        }
        Ok(Some(choice))
    }

    /// Same exclusive enumeration/mutation boundary, with a game-owned buffer.
    pub(crate) fn choose_and_advance_with_scratch(
        &mut self,
        rng: &mut impl crate::RandomSource,
        actions: &mut [u16; crate::ACTION_COUNT],
    ) -> Result<Option<crate::runner::Choice>, Error> {
        use crate::runner::{Choice, choose_with_scratch};
        let Some(choice) = choose_with_scratch(self, rng, actions) else {
            return Ok(None);
        };
        match choice {
            Choice::Action(id) => self.apply_action_in_place(id)?,
            Choice::Outcome(id) => self.apply_outcome_in_place(id)?,
        }
        Ok(Some(choice))
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;
    use crate::{Board, Config};

    #[test]
    fn i2_public_discard_event_redacts_only_the_resource_identity() {
        let mut s = State::new(Board::default(), Config::default()).unwrap();
        assert_eq!(
            s.public_decision_event(79).unwrap(),
            PublicDecisionEvent {
                actor: 0,
                kind: PublicDecisionKind::Action(79)
            }
        );
        assert_eq!(s.public_decision_event(0), Err(Error::InvalidAction));
        s.phase = DISCARD;
        s.resume_phase = MAIN;
        s.setup_pairs_done = 4;
        s.turn_number = 1;
        s.resources[1][0] = 10;
        s.bank[0] = 9;
        s.discard_remaining = [0, 5];
        s.recompute_caches();
        assert_eq!(
            s.public_decision_event(206).unwrap(),
            PublicDecisionEvent {
                actor: 1,
                kind: PublicDecisionKind::Discard { resource: Some(0) }
            }
        );
        s.config.discards_public = false;
        assert_eq!(
            s.public_decision_event(206).unwrap(),
            PublicDecisionEvent {
                actor: 1,
                kind: PublicDecisionKind::Discard { resource: None }
            }
        );
        assert_eq!(s.observation(0).unwrap().public_state.resources[1][0], 10);
        assert_eq!(s.public_decision_event(207), Err(Error::InvalidAction));
        s.phase = DEVELOPMENT_DRAW;
        s.discard_remaining = [0; 2];
        assert_eq!(s.public_decision_event(16), Err(Error::InvalidAction));
    }
}
