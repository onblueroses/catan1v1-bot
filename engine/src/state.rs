//! The game state, its configuration, the canonical bytes and the derived caches.
//!
//! Field encodings follow `../spec/ENCODING.md`, "Values and ordering":
//!
//! - Players are 0 and 1. `NONE` (255) means no player, node, resource or phase.
//! - Resources are 0 wood, 1 brick, 2 sheep, 3 wheat, 4 ore. Terrain adds 5 desert.
//! - Development types are 0 knight, 1 victory point, 2 road building,
//!   3 year of plenty, 4 monopoly.
//! - Node occupancy is 0 empty, 1 or 2 a settlement of player 0 or 1, 3 or 4 a
//!   city of player 0 or 1. The owner of a nonempty node `x` is `(x - 1) % 2`,
//!   and `x >= 3` means a city. This expression occurs throughout the crate.
//! - Edge occupancy is 0 empty, 1 or 2 a road of player 0 or 1.
//!
//! The file has four parts, in this order: the plain data types, the canonical
//! serialisation and decoder (E7), the cache maintenance with the Longest Road
//! search (E6, A2), and the validators that the decoder and `validate` use.
use crate::{Board, CANONICAL_SIZE, DEVELOPMENT_COUNTS, NONE, dice::DiceState, topology::*};
use serde::{Deserialize, Serialize};

/// Phase numbers of ENCODING.md, "Phases and pending choices". DICE, STEAL and
/// DEVELOPMENT_DRAW are chance nodes. TERMINAL has no transitions. All others
/// are decision nodes.
pub mod phase {
    pub const SETUP_SETTLEMENT: u8 = 0;
    pub const SETUP_ROAD: u8 = 1;
    pub const PRE_ROLL: u8 = 2;
    pub const DICE: u8 = 3;
    pub const DISCARD: u8 = 4;
    pub const ROBBER: u8 = 5;
    pub const STEAL: u8 = 6;
    pub const MAIN: u8 = 7;
    pub const DEVELOPMENT_DRAW: u8 = 8;
    pub const ROAD_BUILDING: u8 = 9;
    pub const YEAR_OF_PLENTY: u8 = 10;
    pub const MONOPOLY: u8 = 11;
    pub const TRADE_TAKE: u8 = 12;
    pub const TERMINAL: u8 = 13;
}
/// Every failure the engine reports. An error never changes the state it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The board does not have the B2 multisets of terrain, tokens and ports.
    InvalidBoard,
    /// A configuration enum is out of range, or `victory_points` is zero.
    InvalidConfig,
    /// Canonical bytes or a hand-built state break a format rule or a game invariant.
    InvalidState,
    /// The action is not legal in this state, or the state is a chance node.
    InvalidAction,
    /// The outcome has no positive mass in this state, or the state is a decision node.
    InvalidOutcome,
    /// A u64 turn or seven counter would overflow. This is not a game result.
    RepresentationOverflow,
    /// An observation contradicts itself, so no full state agrees with it (I3).
    InvalidObservation,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
/// The immutable rule configuration. It is part of every state hash. The
/// defaults are the SPEC values. Every assumed rule (marked A in SPEC) has its
/// field here. The table in ENCODING.md, "Configuration", is the reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// C1: 0 is the Colonist balanced model, 1 is two fair independent dice.
    pub dice: u8,
    /// S1: the player who places first and takes the first normal turn.
    pub first_player: u8,
    /// R2: when true, a hex next to a player with 2 or fewer visible points is protected.
    pub friendly_robber: bool,
    /// T5: on a seven, a player discards only when holding more cards than this.
    pub discard_limit: u8,
    /// A4: the total a player needs on their own turn to win.
    pub victory_points: u16,
    /// B3: generator option. It does not restrict boards supplied as input.
    pub no_adjacent_6_8: bool,
    /// B3: generator option. It does not restrict boards supplied as input.
    pub no_adjacent_same_number: bool,
    /// T4: 0 pays the remaining cards to a sole claimant. 1 pays nobody on any shortage.
    pub shortage_rule: u8,
    /// A3: 0 lets the holder keep Longest Road on a tie. 1 removes the award on any tie.
    pub longest_road_break_rule: u8,
    /// D7: when true, an observation hides the opponent's victory point cards.
    pub vp_cards_hidden: bool,
    /// I2: 0 hides the opponent's unplayed cards and the deck order. 1 reveals everything.
    pub observation_rule: u8,
    /// I2: when false, public discard events omit the resource identity.
    pub discards_public: bool,
    /// D5: when true, playing Road Building also needs one legal edge. Older
    /// trace files omit this field, so it defaults to false when absent.
    #[serde(default)]
    pub road_building_needs_legal_edge: bool,
    /// Reference-bot compatibility: truncate the seven imbalance to hundredths.
    #[serde(default)]
    pub reference_dice_rounding: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            dice: 0,
            first_player: 0,
            friendly_robber: true,
            discard_limit: 9,
            victory_points: 15,
            no_adjacent_6_8: true,
            no_adjacent_same_number: false,
            shortage_rule: 0,
            longest_road_break_rule: 0,
            vp_cards_hidden: true,
            observation_rule: 0,
            discards_public: true,
            road_building_needs_legal_edge: false,
            reference_dice_rounding: false,
        }
    }
}
impl Config {
    /// Reject unknown enum values and a zero target. Numeric fields accept
    /// their whole wire range otherwise (ENCODING.md, "Configuration").
    pub fn validate(&self) -> Result<(), Error> {
        if self.dice > 1
            || self.first_player > 1
            || self.victory_points == 0
            || self.shortage_rule > 1
            || self.longest_road_break_rule > 1
            || self.observation_rule > 1
        {
            Err(Error::InvalidConfig)
        } else {
            Ok(())
        }
    }
}
/// One outcome of a chance node (E2). Its exact probability is `weight / total`.
/// Ids are 0..=10 for dice totals 2..=12, 11..=15 for a stolen resource and
/// 16..=20 for a drawn development type (ENCODING.md, "Chance outcomes").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub id: u16,
    pub weight: u128,
    pub total: u128,
}
impl Outcome {
    /// Convenience approximation. The integer masses are the definition.
    pub fn probability(&self) -> f64 {
        self.weight as f64 / self.total as f64
    }
}
/// Derived data that a search or a network reads often (E6). Nothing here
/// enters the canonical bytes or the hash. `State::validate` requires every
/// field to equal its recomputation from the authoritative fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cache {
    /// R2: settlements, cities and both awards. Hidden victory point cards do not count.
    pub visible_points: [u8; 2],
    /// A4: visible points plus held victory point cards of both age groups.
    pub total_points: [u8; 2],
    /// A2: length of each player's longest edge trail.
    pub road_lengths: [u8; 2],
    /// Indices are road, settlement, city.
    pub pieces_left: [[u8; 3]; 2],
    /// X1: cards a player gives for one bank card, by resource given: 4, 3 or 2.
    pub trade_rates: [[u8; 5]; 2],
    /// Indices are dice total minus two, player, resource. Includes the robber block.
    pub production: [[[u8; 5]; 2]; 11],
    /// Hand size of each player, used by T5 discards and R3 steals.
    pub resource_totals: [u8; 2],
    /// Public board geometry without phase, stock or affordability gates.
    /// Bit `n` is set when node `n` is empty and has no occupied neighbour (S2).
    pub distance_sites: u64,
    /// Bit `e` is set when edge `e` is empty and connects to that player's network (U3).
    pub road_sites: [u128; 2],
    /// Bit `n` is set when node `n` is a distance site that touches that player's road (U4).
    pub settlement_sites: [u64; 2],
    /// Bit `n` is set when node `n` holds that player's settlement (U5).
    pub city_sites: [u64; 2],
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            visible_points: [0; 2],
            total_points: [0; 2],
            road_lengths: [0; 2],
            pieces_left: [[15, 5, 4]; 2],
            trade_rates: [[4; 5]; 2],
            production: [[[0; 5]; 2]; 11],
            resource_totals: [0; 2],
            distance_sites: 0,
            road_sites: [0; 2],
            settlement_sites: [0; 2],
            city_sites: [0; 2],
        }
    }
}
/// The complete, omniscient game state (I1). It is one fixed-size `Copy` value
/// with no heap memory (E1). Every field except `cache` is authoritative and
/// has its place in the canonical bytes. The fields are public so that tests
/// can build fixtures. After a direct edit, call `recompute_caches`, and call
/// `validate` before the state is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub board: Board,
    pub config: Config,
    /// 0 empty, 1 or 2 a settlement, 3 or 4 a city, of player 0 or 1.
    pub node_occupancy: [u8; 54],
    /// 0 empty, 1 or 2 a road of player 0 or 1.
    pub edge_occupancy: [u8; 72],
    pub robber_hex: u8,
    /// T4: the bank starts with 19 cards of each resource. Bank plus both
    /// hands is always 19 for each resource.
    pub bank: [u8; 5],
    /// Indices are player, resource.
    pub resources: [[u8; 5]; 2],
    /// D2: cards bought on an earlier turn. Indices are player, development type.
    pub playable_development: [[u8; 5]; 2],
    /// D2: cards bought on the current turn. They cannot be played yet.
    pub new_development: [[u8; 5]; 2],
    /// Cards already played. The knight entry is the Largest Army count (A1).
    pub spent_development: [[u8; 5]; 2],
    /// The deck is an unordered multiset. A draw picks a type with probability
    /// proportional to its count.
    pub development_deck: [u8; 5],
    /// One of the constants in [`phase`].
    pub phase: u8,
    /// During setup this is the placing player. In normal play it stays the
    /// roller, also while the opponent discards.
    pub turn_owner: u8,
    /// A player only in TERMINAL, otherwise `NONE`.
    pub winner: u8,
    /// The phase to return to when a pending effect completes: PRE_ROLL or
    /// MAIN. It is `NONE` when no effect is pending.
    pub resume_phase: u8,
    /// S1: completed settlement and road pairs, 0 to 4. It stays 4 after setup.
    pub setup_pairs_done: u8,
    /// S1: the node just settled, only in SETUP_ROAD. The setup road must touch it.
    pub setup_anchor: u8,
    /// D3: true when a development card was played this turn.
    pub development_played: bool,
    /// Roads or cards still to take in ROAD_BUILDING or YEAR_OF_PLENTY.
    pub choices_remaining: u8,
    /// The resource offered, only in TRADE_TAKE.
    pub trade_give: u8,
    /// T5: cards each player must still discard, only in DISCARD.
    pub discard_remaining: [u8; 2],
    pub largest_army_holder: u8,
    pub longest_road_holder: u8,
    /// The total of this turn's roll, or 0 before the roll.
    pub last_dice: u8,
    /// 0 during setup. It counts the normal turns started.
    pub turn_number: u64,
    pub dice: DiceState,
    pub cache: Cache,
}
impl State {
    /// The constructor state of ENCODING.md, "Initialization": an empty board,
    /// the robber on the desert (B4), a full bank and deck, and the first
    /// setup settlement to place. It performs no placement and no random draw.
    pub fn new(board: Board, config: Config) -> Result<Self, Error> {
        board.validate()?;
        config.validate()?;
        let mut state = Self {
            board,
            config,
            node_occupancy: [0; 54],
            edge_occupancy: [0; 72],
            robber_hex: board.terrain.iter().position(|&x| x == 5).unwrap() as u8,
            bank: [19; 5],
            resources: [[0; 5]; 2],
            playable_development: [[0; 5]; 2],
            new_development: [[0; 5]; 2],
            spent_development: [[0; 5]; 2],
            development_deck: DEVELOPMENT_COUNTS,
            phase: phase::SETUP_SETTLEMENT,
            turn_owner: config.first_player,
            winner: NONE,
            resume_phase: NONE,
            setup_pairs_done: 0,
            setup_anchor: NONE,
            development_played: false,
            choices_remaining: 0,
            trade_give: NONE,
            discard_remaining: [0; 2],
            largest_army_holder: NONE,
            longest_road_holder: NONE,
            last_dice: 0,
            turn_number: 0,
            dice: DiceState::default(),
            cache: Cache::default(),
        };
        state.recompute_geometry();
        Ok(state)
    }
    /// The player who decides now, or `None` at a chance or terminal node.
    /// During discards the roller goes first, then the opponent (T5).
    pub fn actor(&self) -> Option<u8> {
        match self.phase {
            phase::DICE | phase::STEAL | phase::DEVELOPMENT_DRAW | phase::TERMINAL => None,
            phase::DISCARD => Some(if self.discard_remaining[self.turn_owner as usize] > 0 {
                self.turn_owner
            } else {
                1 - self.turn_owner
            }),
            _ => Some(self.turn_owner),
        }
    }
    pub fn is_chance(&self) -> bool {
        matches!(
            self.phase,
            phase::DICE | phase::STEAL | phase::DEVELOPMENT_DRAW
        )
    }
    pub fn is_terminal(&self) -> bool {
        self.phase == phase::TERMINAL
    }
    pub fn winner(&self) -> Option<u8> {
        if self.winner < 2 {
            Some(self.winner)
        } else {
            None
        }
    }

    // ---- Canonical serialisation and hash (E7) ----

    /// The 313 canonical bytes. The offsets are the table in ENCODING.md,
    /// "Canonical state bytes". Bytes 12, 15, 22 and 23 are reserved zeros.
    /// Caches are left out because they can be recomputed.
    pub fn canonical_bytes(&self) -> [u8; CANONICAL_SIZE] {
        assert!(
            !self.uses_compatibility(),
            "Use compatibility_bytes for non-v1 boards or rules"
        );
        self.canonical_prefix()
    }
    fn canonical_prefix(&self) -> [u8; CANONICAL_SIZE] {
        let mut b = [0; CANONICAL_SIZE];
        b[..4].copy_from_slice(b"C1V1");
        b[4] = 1;
        let c = self.config;
        b[6] = c.dice;
        b[7] = c.first_player;
        b[8] = c.friendly_robber as u8;
        b[9] = c.discard_limit;
        b[10..12].copy_from_slice(&c.victory_points.to_le_bytes());
        b[13] = c.no_adjacent_6_8 as u8;
        b[14] = c.no_adjacent_same_number as u8;
        b[16] = c.shortage_rule;
        b[17] = c.longest_road_break_rule;
        b[18] = c.vp_cards_hidden as u8;
        b[19] = c.observation_rule;
        b[20] = c.discards_public as u8;
        b[21] = c.road_building_needs_legal_edge as u8;
        b[24..43].copy_from_slice(&self.board.terrain);
        b[43..62].copy_from_slice(&self.board.number);
        b[62..71].copy_from_slice(&self.board.port_type);
        b[71..125].copy_from_slice(&self.node_occupancy);
        b[125..197].copy_from_slice(&self.edge_occupancy);
        b[197] = self.robber_hex;
        b[198..203].copy_from_slice(&self.bank);
        for p in 0..2 {
            b[203 + 5 * p..208 + 5 * p].copy_from_slice(&self.resources[p]);
            b[213 + 5 * p..218 + 5 * p].copy_from_slice(&self.playable_development[p]);
            b[223 + 5 * p..228 + 5 * p].copy_from_slice(&self.new_development[p]);
            b[233 + 5 * p..238 + 5 * p].copy_from_slice(&self.spent_development[p]);
        }
        b[243..248].copy_from_slice(&self.development_deck);
        b[248..262].copy_from_slice(&[
            self.phase,
            self.turn_owner,
            self.winner,
            self.resume_phase,
            self.setup_pairs_done,
            self.setup_anchor,
            self.development_played as u8,
            self.choices_remaining,
            self.trade_give,
            self.discard_remaining[0],
            self.discard_remaining[1],
            self.largest_army_holder,
            self.longest_road_holder,
            self.last_dice,
        ]);
        b[262..270].copy_from_slice(&self.turn_number.to_le_bytes());
        b[270..281].copy_from_slice(&self.dice.counts);
        b[281] = self.dice.recent_len;
        b[282..287].copy_from_slice(&self.dice.recent);
        b[287] = self.dice.initialized_player_mask;
        b[288] = self.dice.streak_owner;
        b[289..297].copy_from_slice(&self.dice.streak_length.to_le_bytes());
        b[297..305].copy_from_slice(&self.dice.sevens[0].to_le_bytes());
        b[305..313].copy_from_slice(&self.dice.sevens[1].to_le_bytes());
        b
    }
    /// FNV-1a-64 over the canonical bytes. Both implementations must produce
    /// the same value, and traces record it after every step.
    pub fn hash(&self) -> u64 {
        let fold = |h: u64, b: &u8| (h ^ u64::from(*b)).wrapping_mul(1099511628211);
        if self.uses_compatibility() {
            self.compatibility_bytes()
                .iter()
                .fold(0xcbf29ce484222325, fold)
        } else {
            self.canonical_bytes().iter().fold(0xcbf29ce484222325, fold)
        }
    }
    /// True when the shared v1 format cannot represent the board or rules.
    pub fn uses_compatibility(&self) -> bool {
        self.board.port_edges != Board::default_port_edges() || self.config.reference_dice_rounding
    }
    /// Comparison format: C1PC magic, version 1, the v1 field offsets, then
    /// nine port edges and the rounding boolean. It is separate from C1V1.
    pub fn compatibility_bytes(&self) -> [u8; CANONICAL_SIZE + 10] {
        let mut bytes = [0; CANONICAL_SIZE + 10];
        bytes[..CANONICAL_SIZE].copy_from_slice(&self.canonical_prefix());
        bytes[..4].copy_from_slice(b"C1PC");
        bytes[313..322].copy_from_slice(&self.board.port_edges);
        bytes[322] = self.config.reference_dice_rounding as u8;
        bytes
    }
    /// Decode the comparison format without changing the v1 decoder.
    pub fn from_compatibility_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != CANONICAL_SIZE + 10
            || &bytes[..4] != b"C1PC"
            || bytes[4..6] != [1, 0]
            || bytes[322] > 1
        {
            return Err(Error::InvalidState);
        }
        let mut prefix = [0; CANONICAL_SIZE];
        prefix.copy_from_slice(&bytes[..CANONICAL_SIZE]);
        prefix[..4].copy_from_slice(b"C1V1");
        Self::from_prefix(
            &prefix,
            bytes[313..322].try_into().unwrap(),
            bytes[322] != 0,
        )
    }
    /// Exact dice distribution under this state's selected compatibility mode.
    pub fn dice_probabilities(&self) -> crate::dice::DiceProbabilities {
        if self.config.dice == 0 && self.config.reference_dice_rounding {
            self.dice.reference_probabilities(self.turn_owner)
        } else {
            self.dice
                .probabilities(self.config.dice == 0, self.turn_owner)
        }
    }
    /// Strict decoder. It rejects a wrong length, magic or version, nonzero
    /// reserved bytes, booleans other than 0 and 1, stale inactive fields and
    /// broken game invariants. It never repairs its input. It does not prove
    /// that the state can be reached from the constructor (see QUESTIONS.md,
    /// "Snapshot validation boundary").
    pub fn from_canonical_bytes(b: &[u8]) -> Result<Self, Error> {
        Self::from_prefix(b, Board::default_port_edges(), false)
    }
    fn from_prefix(b: &[u8], port_edges: [u8; 9], rounding: bool) -> Result<Self, Error> {
        if b.len() != CANONICAL_SIZE
            || &b[..4] != b"C1V1"
            || b[4..6] != [1, 0]
            || [12, 15, 22, 23].iter().any(|&i| b[i] != 0)
            || [8, 13, 14, 18, 20, 21, 254].iter().any(|&i| b[i] > 1)
        {
            return Err(Error::InvalidState);
        }
        let c = Config {
            dice: b[6],
            first_player: b[7],
            friendly_robber: b[8] != 0,
            discard_limit: b[9],
            victory_points: u16::from_le_bytes(b[10..12].try_into().unwrap()),
            no_adjacent_6_8: b[13] != 0,
            no_adjacent_same_number: b[14] != 0,
            shortage_rule: b[16],
            longest_road_break_rule: b[17],
            vp_cards_hidden: b[18] != 0,
            observation_rule: b[19],
            discards_public: b[20] != 0,
            road_building_needs_legal_edge: b[21] != 0,
            reference_dice_rounding: rounding,
        };
        let board = Board {
            terrain: b[24..43].try_into().unwrap(),
            number: b[43..62].try_into().unwrap(),
            port_type: b[62..71].try_into().unwrap(),
            port_edges,
        };
        let mut s = Self::new(board, c)?;
        s.node_occupancy.copy_from_slice(&b[71..125]);
        s.edge_occupancy.copy_from_slice(&b[125..197]);
        s.robber_hex = b[197];
        s.bank.copy_from_slice(&b[198..203]);
        for p in 0..2 {
            s.resources[p].copy_from_slice(&b[203 + 5 * p..208 + 5 * p]);
            s.playable_development[p].copy_from_slice(&b[213 + 5 * p..218 + 5 * p]);
            s.new_development[p].copy_from_slice(&b[223 + 5 * p..228 + 5 * p]);
            s.spent_development[p].copy_from_slice(&b[233 + 5 * p..238 + 5 * p]);
        }
        s.development_deck.copy_from_slice(&b[243..248]);
        s.phase = b[248];
        s.turn_owner = b[249];
        s.winner = b[250];
        s.resume_phase = b[251];
        s.setup_pairs_done = b[252];
        s.setup_anchor = b[253];
        s.development_played = b[254] != 0;
        s.choices_remaining = b[255];
        s.trade_give = b[256];
        s.discard_remaining = [b[257], b[258]];
        s.largest_army_holder = b[259];
        s.longest_road_holder = b[260];
        s.last_dice = b[261];
        s.turn_number = u64::from_le_bytes(b[262..270].try_into().unwrap());
        s.dice.counts.copy_from_slice(&b[270..281]);
        s.dice.recent_len = b[281];
        s.dice.recent.copy_from_slice(&b[282..287]);
        s.dice.initialized_player_mask = b[287];
        s.dice.streak_owner = b[288];
        s.dice.streak_length = u64::from_le_bytes(b[289..297].try_into().unwrap());
        s.dice.sevens = [
            u64::from_le_bytes(b[297..305].try_into().unwrap()),
            u64::from_le_bytes(b[305..313].try_into().unwrap()),
        ];
        s.validate_authoritative()?;
        s.recompute_caches();
        s.validate_normalized()?;
        Ok(s)
    }

    // ---- Cache maintenance (E6) ----
    //
    // The full rebuilds below are the reference. The transition bodies in
    // rules.rs call the narrow updates, and `validate` compares the two.

    /// Rebuild derived data after direct fixture edits. Does not change award holders.
    pub fn recompute_caches(&mut self) {
        self.cache = Cache::default();
        self.recompute_non_road_board_caches();
        self.cache.road_lengths = [self.longest_road(0), self.longest_road(1)];
        self.recompute_hand_caches();
        self.recompute_geometry();
    }
    /// Hand sizes and total points. Total points add the held victory point
    /// cards of both age groups, because they count as soon as held (D7).
    pub(crate) fn recompute_hand_caches(&mut self) {
        for p in 0..2 {
            self.cache.resource_totals[p] = self.resources[p].iter().sum();
            self.cache.total_points[p] = self.cache.visible_points[p]
                + self.playable_development[p][1]
                + self.new_development[p][1];
        }
    }
    /// Pieces left (U2), port trade rates (X1), production and points, all
    /// from raw occupancy. A settlement or city placement calls this. A road
    /// placement does not, because it changes none of these except road stock.
    pub(crate) fn recompute_non_road_board_caches(&mut self) {
        self.cache.visible_points = [0; 2];
        self.cache.pieces_left = [[15, 5, 4]; 2];
        self.cache.trade_rates = [[4; 5]; 2];
        for &x in &self.edge_occupancy {
            if x != 0 {
                self.cache.pieces_left[(x - 1) as usize][0] -= 1;
            }
        }
        for &x in &self.node_occupancy {
            if x != 0 {
                let p = ((x - 1) % 2) as usize;
                let city = x >= 3;
                self.cache.pieces_left[p][if city { 2 } else { 1 }] -= 1;
                self.cache.visible_points[p] += if city { 2 } else { 1 };
            }
        }
        for (slot, nodes) in self.board.port_nodes().iter().enumerate() {
            for &n in nodes {
                let x = self.node_occupancy[n as usize];
                if x != 0 {
                    let p = ((x - 1) % 2) as usize;
                    let r = self.board.port_type[slot];
                    if r == 5 {
                        for rate in &mut self.cache.trade_rates[p] {
                            *rate = (*rate).min(3);
                        }
                    } else {
                        self.cache.trade_rates[p][r as usize] = 2;
                    }
                }
            }
        }
        self.recompute_production();
        self.refresh_points();
    }
    /// A2: a settlement of the player to move on `node` can shorten the
    /// opponent's road. The mover's own length cannot change.
    pub(crate) fn recompute_road_cut_by_settlement(&mut self, node: usize) {
        let opponent = 1 - self.turn_owner;
        // A new building can only cut passage between two opponent roads.
        let incident = NODE_EDGES[node]
            .iter()
            .filter(|&&e| e != NONE && self.edge_occupancy[e as usize] == opponent + 1)
            .count();
        if incident >= 2 {
            self.cache.road_lengths[opponent as usize] = self.longest_road(opponent);
        }
    }
    /// A4: settlements count 1, cities 2 and each award 2.
    pub(crate) fn refresh_points(&mut self) {
        let mut points = [0u8; 2];
        for &x in &self.node_occupancy {
            if x != 0 {
                points[((x - 1) % 2) as usize] += if x >= 3 { 2 } else { 1 };
            }
        }
        for holder in [self.largest_army_holder, self.longest_road_holder] {
            if holder < 2 {
                points[holder as usize] += 2;
            }
        }
        self.cache.visible_points = points;
        self.recompute_hand_caches();
    }
    /// T3 and R4: what each dice total pays to each player, with the robber's
    /// hex and the desert left out. A settlement adds 1 and a city adds 2.
    pub(crate) fn recompute_production(&mut self) {
        self.cache.production = [[[0; 5]; 2]; 11];
        for (h, nodes) in HEX_NODES.iter().enumerate() {
            if h == self.robber_hex as usize || self.board.terrain[h] == 5 {
                continue;
            }
            let t = self.board.number[h] as usize - 2;
            let r = self.board.terrain[h] as usize;
            for &n in nodes {
                let x = self.node_occupancy[n as usize];
                if x != 0 {
                    self.cache.production[t][((x - 1) % 2) as usize][r] +=
                        if x >= 3 { 2 } else { 1 };
                }
            }
        }
    }
    // Moving the robber changes only its two hex contributions. Restore the
    // old hex before blocking the destination so shared roll/resource cells
    // and shared buildings retain all other hex contributions. The temporary
    // sum is ordinary unblocked board demand: U2 stocks and B1 incidence bound
    // each player's total contribution by 3*(5 settlements + 2*4 cities)=39.
    pub(crate) fn update_production_robber(&mut self, destination: u8) {
        for (hex, restore) in [(self.robber_hex, true), (destination, false)] {
            let h = hex as usize;
            let r = self.board.terrain[h] as usize;
            if r == 5 {
                continue;
            }
            let t = self.board.number[h] as usize - 2;
            for &node in &HEX_NODES[h] {
                let occupancy = self.node_occupancy[node as usize];
                if occupancy != 0 {
                    let player = ((occupancy - 1) % 2) as usize;
                    let amount = if occupancy >= 3 { 2 } else { 1 };
                    if restore {
                        self.cache.production[t][player][r] += amount;
                    } else {
                        self.cache.production[t][player][r] -= amount;
                    }
                }
            }
        }
    }
    // ---- Longest Road (A2, A3) ----
    //
    // The search works on a compressed copy of one player's roads. Each road
    // gets a bit in a u16 (U2 allows at most 15). `incidence[node]` holds the
    // roads that a trail may continue on from that node. It is left empty for
    // a node with an opponent building, so a trail can end there but cannot
    // pass (A2). `endpoint_xor[road] ^ node` gives the other end of a road.

    /// Exact edge-trail search. Returning to a visited node is legal, reusing an edge is not.
    pub fn longest_road(&self, player: u8) -> u8 {
        fn walk(incidence: &[u16; 54], endpoint_xor: &[u8; 15], node: u8, free: u16) -> u8 {
            let mut candidates = incidence[node as usize] & free;
            let mut best = 0;
            while candidates != 0 {
                let edge = candidates.trailing_zeros() as usize;
                let bit = 1u16 << edge;
                candidates &= candidates - 1;
                let next = endpoint_xor[edge] ^ node;
                best = best.max(1 + walk(incidence, endpoint_xor, next, free ^ bit));
            }
            best
        }

        // SPEC U2 permits at most 15 roads per player, so valid positions fit
        // in u16. Keep the old search for directly edited, over-stock fixtures.
        let mut incidence = [0u16; 54];
        let mut endpoints = [[0u8; 2]; 15];
        let mut endpoint_xor = [0u8; 15];
        let mut count = 0;
        for (edge, &owner) in self.edge_occupancy.iter().enumerate() {
            if owner != player + 1 {
                continue;
            }
            if count == endpoints.len() {
                return self.longest_road_uncompressed(player);
            }
            let bit = 1u16 << count;
            let nodes = EDGE_NODES[edge];
            endpoints[count] = nodes;
            endpoint_xor[count] = nodes[0] ^ nodes[1];
            for node in nodes {
                let building = self.node_occupancy[node as usize];
                if building == 0 || (building - 1) % 2 == player {
                    incidence[node as usize] |= bit;
                }
            }
            count += 1;
        }

        let free = (1u16 << count) - 1;
        let mut best = 0;
        for (edge, nodes) in endpoints[..count].iter().enumerate() {
            let remaining = free ^ (1u16 << edge);
            for &node in nodes {
                // Start at the other endpoint, including when it is blocked.
                // Continuation at this destination uses filtered incidence.
                best = best.max(1 + walk(&incidence, &endpoint_xor, node, remaining));
            }
        }
        best
    }
    /// Exact update after an insertion. Old trails remain valid; an improvement
    /// must contain the added edge. The two extensions share their free-edge mask.
    pub(crate) fn longest_road_after_addition(&self, player: u8, added: usize) -> u8 {
        fn walk(incidence: &[u16; 54], endpoint_xor: &[u8; 15], node: u8, free: u16) -> u8 {
            let mut candidates = incidence[node as usize] & free;
            let mut best = 0;
            while candidates != 0 {
                let edge = candidates.trailing_zeros() as usize;
                let bit = 1u16 << edge;
                candidates &= candidates - 1;
                best = best.max(
                    1 + walk(
                        incidence,
                        endpoint_xor,
                        endpoint_xor[edge] ^ node,
                        free ^ bit,
                    ),
                );
            }
            best
        }
        fn extend(
            incidence: &[u16; 54],
            endpoint_xor: &[u8; 15],
            node: u8,
            other: u8,
            free: u16,
            length: u8,
            bound: u8,
            best: &mut u8,
        ) {
            // Stopping the left extension at every prefix allows the right
            // extension to use edges that a longer left extension would consume.
            *best = (*best).max(length + walk(incidence, endpoint_xor, other, free));
            if *best == bound {
                return;
            }
            let mut candidates = incidence[node as usize] & free;
            while candidates != 0 {
                let edge = candidates.trailing_zeros() as usize;
                let bit = 1u16 << edge;
                candidates &= candidates - 1;
                extend(
                    incidence,
                    endpoint_xor,
                    endpoint_xor[edge] ^ node,
                    other,
                    free ^ bit,
                    length + 1,
                    bound,
                    best,
                );
                if *best == bound {
                    return;
                }
            }
        }

        let mut incidence = [0u16; 54];
        let mut endpoint_xor = [0u8; 15];
        let mut count = 0;
        for (edge, &owner) in self.edge_occupancy.iter().enumerate() {
            if owner != player + 1 || edge == added {
                continue;
            }
            // Valid U2 positions have at most fourteen old edges here. Retain
            // the public full query for directly edited over-stock fixtures.
            if count == endpoint_xor.len() {
                return self.longest_road(player);
            }
            let bit = 1u16 << count;
            let nodes = EDGE_NODES[edge];
            endpoint_xor[count] = nodes[0] ^ nodes[1];
            for node in nodes {
                let building = self.node_occupancy[node as usize];
                if building == 0 || (building - 1) % 2 == player {
                    incidence[node as usize] |= bit;
                }
            }
            count += 1;
        }

        let free = (1u16 << count) - 1;
        let [mut left, mut right] = EDGE_NODES[added];
        // Start the enumerated side at a leaf/blocked endpoint when possible.
        // It then has no prefix branches, so only one opposite walk is needed.
        if incidence[left as usize].count_ones() > incidence[right as usize].count_ones() {
            std::mem::swap(&mut left, &mut right);
        }
        let mut best = self.cache.road_lengths[player as usize];
        extend(
            &incidence,
            &endpoint_xor,
            left,
            right,
            free,
            1,
            count as u8 + 1,
            &mut best,
        );
        best
    }
    /// Plain search over global edge ids. No valid game state reaches it. It
    /// keeps `longest_road` total for hand-edited fixtures with more than 15
    /// roads of one player.
    #[cold]
    fn longest_road_uncompressed(&self, player: u8) -> u8 {
        fn walk(s: &State, p: u8, n: usize, used: u128, arrived: bool) -> u8 {
            let x = s.node_occupancy[n];
            if arrived && x != 0 && (x - 1) % 2 != p {
                return 0;
            }
            let mut best = 0;
            for &e in &NODE_EDGES[n] {
                if e == NONE {
                    continue;
                }
                let i = e as usize;
                let bit = 1u128 << i;
                if s.edge_occupancy[i] == p + 1 && used & bit == 0 {
                    let ends = EDGE_NODES[i];
                    let next = (ends[0] as usize) ^ (ends[1] as usize) ^ n;
                    best = best.max(1 + walk(s, p, next, used | bit, true));
                }
            }
            best
        }
        (0..54)
            .map(|n| walk(self, player, n, 0, false))
            .max()
            .unwrap_or(0)
    }
    /// Full recomputation of both road lengths, both award holders and the
    /// geometry. Fixtures call it after direct edits. Transitions use the
    /// cheaper `normalize_cached_awards`.
    pub fn normalize_awards(&mut self) {
        self.cache.road_lengths = [self.longest_road(0), self.longest_road(1)];
        self.normalize_cached_awards();
        self.recompute_geometry();
    }
    /// Resolve awards after the placement path has updated affected road lengths.
    ///
    /// Longest Road (A2, A3): with the default break rule the holder keeps the
    /// award while its length is at least 5 and at least the opponent's.
    /// Otherwise the award goes to the strictly longer road of 5 or more, or
    /// to nobody. Largest Army (A1): it passes only to strictly more played
    /// knights, and the first player with 3 takes it when nobody holds it.
    pub(crate) fn normalize_cached_awards(&mut self) {
        let l = self.cache.road_lengths;
        let incumbent = self.longest_road_holder;
        self.longest_road_holder = if incumbent < 2
            && self.config.longest_road_break_rule == 0
            && l[incumbent as usize] >= 5
            && l[incumbent as usize] >= l[1 - incumbent as usize]
        {
            incumbent
        } else if l[0] >= 5 && l[0] > l[1] {
            0
        } else if l[1] >= 5 && l[1] > l[0] {
            1
        } else {
            NONE
        };
        let k = [self.spent_development[0][0], self.spent_development[1][0]];
        if self.largest_army_holder < 2 {
            let h = self.largest_army_holder as usize;
            if k[1 - h] > k[h] && k[1 - h] >= 3 {
                self.largest_army_holder = (1 - h) as u8;
            }
        } else if k[self.turn_owner as usize] >= 3 {
            self.largest_army_holder = self.turn_owner;
        } else if k[1 - self.turn_owner as usize] >= 3 {
            self.largest_army_holder = 1 - self.turn_owner;
        }
        self.refresh_points();
    }

    // ---- Validation ----
    //
    // `validate_authoritative` checks ranges, conservation, piece limits, the
    // distance rule and the dice history. It reads no cache, so the decoder
    // can run it before the caches exist. `validate_normalized` then checks
    // the phase-dependent fields and the awards, and it needs current caches.

    /// Field ranges, card conservation (19 per resource, 25 development
    /// cards), piece limits (U2), the distance rule (S2) and a consistent
    /// dice history (C2).
    pub(crate) fn validate_authoritative(&self) -> Result<(), Error> {
        if self.phase > 13
            || self.turn_owner > 1
            || self.robber_hex > 18
            || self.setup_pairs_done > 4
            || self.node_occupancy.iter().any(|&x| x > 4)
            || self.edge_occupancy.iter().any(|&x| x > 2)
            || [
                self.winner,
                self.largest_army_holder,
                self.longest_road_holder,
            ]
            .iter()
            .any(|&x| x != NONE && x > 1)
            || (self.last_dice != 0 && !(2..=12).contains(&self.last_dice))
        {
            return Err(Error::InvalidState);
        }
        for r in 0..5 {
            if self.bank[r] as u16 + self.resources[0][r] as u16 + self.resources[1][r] as u16 != 19
            {
                return Err(Error::InvalidState);
            }
        }
        for t in 0..5 {
            let total = self.development_deck[t] as u16
                + (0..2)
                    .map(|p| {
                        self.playable_development[p][t] as u16
                            + self.new_development[p][t] as u16
                            + self.spent_development[p][t] as u16
                    })
                    .sum::<u16>();
            if total != DEVELOPMENT_COUNTS[t] as u16 {
                return Err(Error::InvalidState);
            }
        }
        if self.spent_development[0][1] != 0 || self.spent_development[1][1] != 0 {
            return Err(Error::InvalidState);
        }
        for p in 0..2 {
            if self.edge_occupancy.iter().filter(|&&x| x == p + 1).count() > 15
                || self.node_occupancy.iter().filter(|&&x| x == p + 1).count() > 5
                || self.node_occupancy.iter().filter(|&&x| x == p + 3).count() > 4
            {
                return Err(Error::InvalidState);
            }
        }
        for n in 0..54 {
            if self.node_occupancy[n] != 0
                && NODE_NEIGHBORS[n]
                    .iter()
                    .any(|&v| v != NONE && self.node_occupancy[v as usize] != 0)
            {
                return Err(Error::InvalidState);
            }
        }
        let d = &self.dice;
        if d.recent_len > 5
            || d.initialized_player_mask > 3
            || (d.streak_owner != NONE && d.streak_owner > 1)
            || d.counts
                .iter()
                .zip([1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1])
                .any(|(&c, m)| c > m)
            || !(12..=36).contains(&d.counts.iter().map(|&x| x as u16).sum::<u16>())
        {
            return Err(Error::InvalidState);
        }
        for i in 0..5 {
            if (i < d.recent_len as usize && !(2..=12).contains(&d.recent[i]))
                || (i >= d.recent_len as usize && d.recent[i] != 0)
            {
                return Err(Error::InvalidState);
            }
        }
        if (d.streak_length == 0) != (d.streak_owner == NONE)
            || (d.streak_owner < 2 && d.streak_length > d.sevens[d.streak_owner as usize])
            || (0..2).any(|p| d.sevens[p] > 0 && d.initialized_player_mask & (1 << p) == 0)
        {
            return Err(Error::InvalidState);
        }
        let total_sevens = d.sevens[0] as u128 + d.sevens[1] as u128;
        let recent_sevens = d.recent[..d.recent_len as usize]
            .iter()
            .filter(|&&t| t == 7)
            .count() as u128;
        if (total_sevens == 0) != (d.streak_owner == NONE)
            || recent_sevens > total_sevens
            || (d.recent_len != 0 && d.initialized_player_mask == 0)
        {
            return Err(Error::InvalidState);
        }
        // The first reshuffle follows 24 draws (36 down to 12). Thus a
        // shorter-than-five FIFO contains the entire history, before any
        // reshuffle. Once full, it never becomes shorter again.
        if d.recent_len < 5 {
            if total_sevens != recent_sevens {
                return Err(Error::InvalidState);
            }
            let mut expected = crate::dice::INITIAL_COUNTS;
            for &t in &d.recent[..d.recent_len as usize] {
                expected[(t - 2) as usize] = expected[(t - 2) as usize]
                    .checked_sub(1)
                    .ok_or(Error::InvalidState)?;
            }
            if d.counts != expected {
                return Err(Error::InvalidState);
            }
        }
        if self.config.dice == 1 && *d != DiceState::default() {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
    /// Everything the decoder checks, and also that every cache equals its
    /// recomputation. Call it on a hand-built fixture before use.
    pub fn validate(&self) -> Result<(), Error> {
        self.board.validate()?;
        self.config.validate()?;
        self.validate_authoritative()?;
        self.validate_normalized()?;
        let mut check = *self;
        check.recompute_caches();
        if check.cache != self.cache {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
    /// The normal form of ENCODING.md, "Award and win normalization": no
    /// inactive field holds stale data, each pending phase can make progress,
    /// a nonterminal decision node has a legal action, and both award holders
    /// agree with the road lengths and knight counts.
    fn validate_normalized(&self) -> Result<(), Error> {
        use phase::*;
        let pending = matches!(
            self.phase,
            DISCARD
                | ROBBER
                | STEAL
                | DEVELOPMENT_DRAW
                | ROAD_BUILDING
                | YEAR_OF_PLENTY
                | MONOPOLY
                | TRADE_TAKE
        );
        if (pending && ![PRE_ROLL, MAIN].contains(&self.resume_phase))
            || (matches!(self.phase, DISCARD | DEVELOPMENT_DRAW | TRADE_TAKE)
                && self.resume_phase != MAIN)
            || (!pending && self.resume_phase != NONE)
            || (self.phase != SETUP_ROAD && self.setup_anchor != NONE)
            || (self.phase == SETUP_ROAD
                && (self.setup_anchor >= 54
                    || self.node_occupancy[self.setup_anchor as usize] != self.turn_owner + 1))
            || (self.phase != TRADE_TAKE && self.trade_give != NONE)
            || (self.phase == TRADE_TAKE && self.trade_give >= 5)
            || (!matches!(self.phase, ROAD_BUILDING | YEAR_OF_PLENTY)
                && self.choices_remaining != 0)
            || (matches!(self.phase, ROAD_BUILDING | YEAR_OF_PLENTY)
                && !(1..=2).contains(&self.choices_remaining))
            || (self.phase != DISCARD && self.discard_remaining != [0; 2])
            || (self.phase == DISCARD && self.discard_remaining == [0; 2])
            || (self.phase != TERMINAL && self.winner != NONE)
            || (self.phase == TERMINAL && self.winner != self.turn_owner)
        {
            return Err(Error::InvalidState);
        }
        if self.phase <= SETUP_ROAD {
            if self.setup_pairs_done >= 4
                || self.turn_number != 0
                || self.turn_owner
                    != [
                        self.config.first_player,
                        1 - self.config.first_player,
                        1 - self.config.first_player,
                        self.config.first_player,
                    ][self.setup_pairs_done as usize]
            {
                return Err(Error::InvalidState);
            }
            self.validate_setup()?;
        } else if self.setup_pairs_done != 4 || self.turn_number == 0 {
            return Err(Error::InvalidState);
        }
        if !self.observation_win_consistent() {
            return Err(Error::InvalidState);
        }
        if self.phase == DICE
            && self.config.dice == 0
            && (self.dice.initialized_player_mask & (1 << self.turn_owner) == 0
                || self.dice.counts.iter().sum::<u8>() < 13)
        {
            return Err(Error::InvalidState);
        }
        if self.phase == DISCARD
            && (0..2).any(|p| self.discard_remaining[p] > self.cache.resource_totals[p])
        {
            return Err(Error::InvalidState);
        }
        if self.phase == STEAL
            && (self.cache.resource_totals[1 - self.turn_owner as usize] == 0
                || !HEX_NODES[self.robber_hex as usize].iter().any(|&n| {
                    let x = self.node_occupancy[n as usize];
                    x != 0 && (x - 1) % 2 != self.turn_owner
                }))
        {
            return Err(Error::InvalidState);
        }
        if self.phase == DEVELOPMENT_DRAW && self.development_deck.iter().sum::<u8>() == 0 {
            return Err(Error::InvalidState);
        }
        if self.phase == TRADE_TAKE
            && self.resources[self.turn_owner as usize][self.trade_give as usize]
                < self.cache.trade_rates[self.turn_owner as usize][self.trade_give as usize]
        {
            return Err(Error::InvalidState);
        }
        if self.phase != TERMINAL && !self.is_chance() {
            let mut a = [0; 226];
            if self.legal_actions(&mut a) == 0 {
                return Err(Error::InvalidState);
            }
        }
        let l = self.cache.road_lengths;
        let h = self.longest_road_holder;
        if h < 2 {
            if l[h as usize] < 5
                || l[h as usize] < l[1 - h as usize]
                || (self.config.longest_road_break_rule == 1 && l[0] == l[1])
            {
                return Err(Error::InvalidState);
            }
        } else if l[0].max(l[1]) >= 5 && l[0] != l[1] {
            return Err(Error::InvalidState);
        }
        let k = [self.spent_development[0][0], self.spent_development[1][0]];
        if self.largest_army_holder < 2 {
            let h = self.largest_army_holder as usize;
            if k[h] < 3 || k[h] < k[1 - h] {
                return Err(Error::InvalidState);
            }
        } else if k[0] >= 3 || k[1] >= 3 {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
    /// Setup states must match the snake order exactly (S1): the right number
    /// of settlements and roads per player, every road next to its owner's
    /// settlement, and hands equal to the second-settlement grant (S3).
    fn validate_setup(&self) -> Result<(), Error> {
        let order = [
            self.config.first_player,
            1 - self.config.first_player,
            1 - self.config.first_player,
            self.config.first_player,
        ];
        let done = self.setup_pairs_done as usize;
        let mut roads = [0usize; 2];
        for &p in &order[..done] {
            roads[p as usize] += 1;
        }
        let mut settlements = roads;
        if self.phase == phase::SETUP_ROAD {
            settlements[self.turn_owner as usize] += 1;
        }
        if self.node_occupancy.iter().any(|&x| x >= 3)
            || self.development_deck != DEVELOPMENT_COUNTS
            || self.dice != DiceState::default()
            || self.development_played
            || self.last_dice != 0
            || self.largest_army_holder != NONE
            || self.longest_road_holder != NONE
            || self.board.terrain[self.robber_hex as usize] != 5
        {
            return Err(Error::InvalidState);
        }
        for p in 0..2 {
            if self
                .edge_occupancy
                .iter()
                .filter(|&&x| x == p as u8 + 1)
                .count()
                != roads[p]
                || self
                    .node_occupancy
                    .iter()
                    .filter(|&&x| x == p as u8 + 1)
                    .count()
                    != settlements[p]
            {
                return Err(Error::InvalidState);
            }
            if settlements[p] < 2 {
                if self.resources[p] != [0; 5] {
                    return Err(Error::InvalidState);
                }
            } else {
                // The second settlement grants exactly its adjacent terrain.
                // Its anchor is known while its road is pending. Otherwise
                // either of that player's two settlements can be the second.
                let matches = (0..54).any(|n| {
                    if self.node_occupancy[n] != p as u8 + 1
                        || (self.phase == phase::SETUP_ROAD
                            && p == self.turn_owner as usize
                            && n != self.setup_anchor as usize)
                    {
                        return false;
                    }
                    let mut grant = [0; 5];
                    for &h in &NODE_HEXES[n] {
                        if h != NONE {
                            let r = self.board.terrain[h as usize];
                            if r < 5 {
                                grant[r as usize] += 1;
                            }
                        }
                    }
                    grant == self.resources[p]
                });
                if !matches {
                    return Err(Error::InvalidState);
                }
            }
        }
        for (e, &x) in self.edge_occupancy.iter().enumerate() {
            if x != 0
                && !EDGE_NODES[e]
                    .iter()
                    .any(|&n| self.node_occupancy[n as usize] == x)
            {
                return Err(Error::InvalidState);
            }
        }
        Ok(())
    }
    /// A4: whether phase, winner and point totals can occur together. After
    /// setup, the turn owner is at or above the target in TERMINAL and below
    /// it in every other phase. The decoder uses this check, and observation
    /// sampling uses it to reject card assignments under which the game would
    /// already be over (I3).
    pub(crate) fn observation_win_consistent(&self) -> bool {
        // END_TURN checks the outgoing player before switching owners. No
        // pre-roll action can increase that player's score. The first turn
        // instead follows setup, which does not check the other player's win.
        if self.turn_number > 1
            && (matches!(self.phase, phase::PRE_ROLL | phase::DICE)
                || self.resume_phase == phase::PRE_ROLL)
            && self.cache.total_points[1 - self.turn_owner as usize] as u16
                >= self.config.victory_points
        {
            return false;
        }
        self.phase <= phase::SETUP_ROAD
            || if self.phase == phase::TERMINAL {
                self.cache.total_points[self.turn_owner as usize] as u16
                    >= self.config.victory_points
            } else {
                (self.cache.total_points[self.turn_owner as usize] as u16)
                    < self.config.victory_points
            }
    }
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    fn pre_roll() -> State {
        let mut s = State::new(Board::default(), Config::default()).unwrap();
        s.phase = phase::PRE_ROLL;
        s.setup_pairs_done = 4;
        s.turn_number = 1;
        s
    }

    fn decoded(s: State) -> Result<State, Error> {
        State::from_canonical_bytes(&s.canonical_bytes())
    }

    #[test]
    fn e7_dice_history_invariants_reject_contradictions() {
        let first = pre_roll().apply_action(0).unwrap();
        assert!(decoded(first).is_ok()); // Prepared but not drawn.
        let rolled = first.apply_outcome(0).unwrap(); // Total two.
        assert!(decoded(rolled).is_ok());
        let mut bad = rolled;
        bad.dice.sevens[0] = 1; // Seven counters require an existing streak.
        assert!(decoded(bad).is_err());
        bad.dice.streak_owner = 0;
        bad.dice.streak_length = 1; // A short FIFO records all sevens.
        assert!(decoded(bad).is_err());
        let mut bad = rolled;
        bad.dice.recent[0] = 7; // Recent seven is absent from the counters.
        assert!(decoded(bad).is_err());
        let mut bad = rolled;
        bad.dice.initialized_player_mask = 0;
        assert!(decoded(bad).is_err());
        let mut bad = rolled;
        bad.dice.counts[1] -= 1; // Extra consumed card is absent from FIFO.
        assert!(decoded(bad).is_err());
        let mut bad = rolled;
        bad.dice.counts[0] = 1;
        bad.dice.counts[1] -= 1; // Correct total, wrong consumed type.
        assert!(decoded(bad).is_err());
    }

    #[test]
    fn e7_pending_trade_requires_the_payment_stock() {
        let mut s = pre_roll();
        s.phase = phase::TRADE_TAKE;
        s.resume_phase = phase::MAIN;
        s.trade_give = 0;
        s.last_dice = 2;
        assert!(decoded(s).is_err());
        assert!(!s.is_legal_action(222));
        assert_eq!(s.apply_action(222), Err(Error::InvalidAction));
        s.resources[0][0] = 4;
        s.bank[0] = 15;
        s.recompute_caches();
        assert!(decoded(s).is_ok());
        let paid = s.apply_action(222).unwrap();
        assert_eq!(paid.resources[0], [0, 1, 0, 0, 0]);
        assert_eq!(paid.bank, [19, 18, 19, 19, 19]);
    }

    #[test]
    fn e7_setup_rejects_depleted_bank_and_preserves_valid_second_grants() {
        let mut bad = State::new(Board::default(), Config::default()).unwrap();
        bad.turn_owner = 1;
        bad.setup_pairs_done = 2;
        bad.bank[0] = 0;
        bad.resources[0][0] = 19;
        assert!(decoded(bad).is_err());
        assert_eq!(bad.apply_action(79), Err(Error::InvalidState));

        let mut s = State::new(Board::default(), Config::default()).unwrap();
        for (node, edge) in [(0, 0), (2, 5), (16, 23), (47, 66)] {
            s = s.apply_action(79 + node).unwrap();
            assert!(decoded(s).is_ok());
            s = s.apply_action(7 + edge).unwrap();
            assert!(decoded(s).is_ok());
        }
    }

    #[test]
    fn e7_dice_history_invariants_preserve_reshuffle_and_independent_states() {
        let mut s = pre_roll();
        // 24 draws consume the first deck down to 12. Draw 25 reshuffles.
        for draw in 0..25 {
            s = s.apply_action(0).unwrap();
            if draw == 24 {
                assert_eq!(s.dice.counts, crate::dice::INITIAL_COUNTS);
                assert_eq!(s.dice.recent_len, 5);
            }
            assert!(decoded(s).is_ok());
            let mut outcomes = [Outcome::default(); 11];
            let n = s.chance_outcomes(&mut outcomes);
            let id = outcomes[..n].iter().find(|o| o.id != 5).unwrap().id;
            s = s.apply_outcome(id).unwrap();
            assert!(decoded(s).is_ok());
            s = s.apply_action(1).unwrap();
        }
        let mut s = pre_roll();
        s.config.dice = 1;
        let s = s.apply_action(0).unwrap().apply_outcome(5).unwrap();
        assert_eq!(s.dice, DiceState::default());
        assert!(decoded(s).is_ok());
    }
}
