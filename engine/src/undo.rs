//! Sparse make/unmake for E5. No state snapshot or allocation.
//!
//! `make_action` and `make_outcome` apply a transition in place and return an
//! `UndoRecord`. `unmake` restores the earlier state from it, caches
//! included. A record holds the small control fields that almost every
//! transition touches (`Common`) and one `Change` variant with the old values
//! of the larger groups that this kind of transition can write. E5 asked for
//! a measurement of undo against copy-and-apply. Sparse
//! undo was faster on the measured branching ensemble, and both stay
//! available.
use crate::{Error, NONE, State, dice::DiceState, phase::*, topology::NODE_HEXES};

/// Restoration information for one successful make. Consume it on the same
/// state, in reverse make order. It contains neither a State nor canonical bytes.
#[derive(Debug)]
pub struct UndoRecord {
    common: Common,
    change: Change,
}

#[derive(Debug)]
struct Common {
    phase: u8,
    turn_owner: u8,
    winner: u8,
    resume_phase: u8,
    setup_pairs_done: u8,
    setup_anchor: u8,
    development_played: bool,
    choices_remaining: u8,
    trade_give: u8,
    discard_remaining: [u8; 2],
    largest_army_holder: u8,
    longest_road_holder: u8,
    last_dice: u8,
    visible_points: [u8; 2],
    total_points: [u8; 2],
    resource_totals: [u8; 2],
}
impl Common {
    fn capture(s: &State) -> Self {
        Self {
            phase: s.phase,
            turn_owner: s.turn_owner,
            winner: s.winner,
            resume_phase: s.resume_phase,
            setup_pairs_done: s.setup_pairs_done,
            setup_anchor: s.setup_anchor,
            development_played: s.development_played,
            choices_remaining: s.choices_remaining,
            trade_give: s.trade_give,
            discard_remaining: s.discard_remaining,
            largest_army_holder: s.largest_army_holder,
            longest_road_holder: s.longest_road_holder,
            last_dice: s.last_dice,
            visible_points: s.cache.visible_points,
            total_points: s.cache.total_points,
            resource_totals: s.cache.resource_totals,
        }
    }
    fn restore(self, s: &mut State) {
        s.phase = self.phase;
        s.turn_owner = self.turn_owner;
        s.winner = self.winner;
        s.resume_phase = self.resume_phase;
        s.setup_pairs_done = self.setup_pairs_done;
        s.setup_anchor = self.setup_anchor;
        s.development_played = self.development_played;
        s.choices_remaining = self.choices_remaining;
        s.trade_give = self.trade_give;
        s.discard_remaining = self.discard_remaining;
        s.largest_army_holder = self.largest_army_holder;
        s.longest_road_holder = self.longest_road_holder;
        s.last_dice = self.last_dice;
        s.cache.visible_points = self.visible_points;
        s.cache.total_points = self.total_points;
        s.cache.resource_totals = self.resource_totals;
    }
}

#[derive(Debug)]
struct Economy {
    bank: [u8; 5],
    resources: [[u8; 5]; 2],
}
impl Economy {
    fn capture(s: &State) -> Self {
        Self {
            bank: s.bank,
            resources: s.resources,
        }
    }
    fn restore(self, s: &mut State) {
        s.bank = self.bank;
        s.resources = self.resources;
    }
}

#[derive(Debug, Default)]
struct ProductionPatch {
    // B1 topology: a building touches at most three hexes. A robber move
    // changes two single-resource hexes times two players, at most four cells.
    // Save distinct old cell values before any transition mutation.
    cells: [(u8, u8); 4],
    len: u8,
}
impl ProductionPatch {
    fn save(&mut self, s: &State, h: u8, p: usize) {
        let r = s.board.terrain[h as usize] as usize;
        if r == 5 {
            return;
        }
        let t = s.board.number[h as usize] as usize - 2;
        let index = (t * 10 + p * 5 + r) as u8;
        if self.cells[..self.len as usize]
            .iter()
            .any(|&(i, _)| i == index)
        {
            return;
        }
        self.cells[self.len as usize] = (index, s.cache.production[t][p][r]);
        self.len += 1;
    }
    fn building(s: &State, n: usize) -> Self {
        let mut patch = Self::default();
        for &h in &NODE_HEXES[n] {
            if h != NONE && h != s.robber_hex {
                patch.save(s, h, s.turn_owner as usize);
            }
        }
        patch
    }
    fn robber(s: &State, h: u8) -> Self {
        let mut patch = Self::default();
        for hex in [s.robber_hex, h] {
            for p in 0..2 {
                patch.save(s, hex, p);
            }
        }
        patch
    }
    fn restore(self, s: &mut State) {
        for &(index, value) in &self.cells[..self.len as usize] {
            let index = index as usize;
            s.cache.production[index / 10][index / 5 % 2][index % 5] = value;
        }
    }
}

#[derive(Debug)]
struct Road {
    turn_number: u64,
    edge: u8,
    old: u8,
    piece: u8,
    lengths: [u8; 2],
}
impl Road {
    fn capture(s: &State, a: u16) -> Self {
        let edge = (a - 7) as usize;
        Self {
            turn_number: s.turn_number,
            edge: edge as u8,
            old: s.edge_occupancy[edge],
            piece: s.cache.pieces_left[s.turn_owner as usize][0],
            lengths: s.cache.road_lengths,
        }
    }
    fn restore(self, s: &mut State, p: usize) {
        s.turn_number = self.turn_number;
        s.edge_occupancy[self.edge as usize] = self.old;
        s.update_geometry_road(self.edge as usize);
        s.cache.pieces_left[p][0] = self.piece;
        s.cache.road_lengths = self.lengths;
    }
}

#[derive(Debug)]
struct Building {
    economy: Economy,
    production: ProductionPatch,
    node: u8,
    old: u8,
    pieces: [u8; 3],
    rates: [u8; 5],
    lengths: [u8; 2],
}
impl Building {
    fn capture(s: &State, a: u16) -> Self {
        let n = (if a < 133 { a - 79 } else { a - 133 }) as usize;
        let p = s.turn_owner as usize;
        Self {
            economy: Economy::capture(s),
            production: ProductionPatch::building(s, n),
            node: n as u8,
            old: s.node_occupancy[n],
            pieces: s.cache.pieces_left[p],
            rates: s.cache.trade_rates[p],
            lengths: s.cache.road_lengths,
        }
    }
    fn restore(self, s: &mut State, p: usize) {
        self.economy.restore(s);
        self.production.restore(s);
        s.node_occupancy[self.node as usize] = self.old;
        if self.old == 0 {
            s.restore_geometry_settlement(self.node as usize);
        } else {
            s.update_geometry_city(self.node as usize);
        }
        s.cache.pieces_left[p] = self.pieces;
        s.cache.trade_rates[p] = self.rates;
        s.cache.road_lengths = self.lengths;
    }
}

/// The old values that one kind of transition overwrites, beyond `Common`.
#[derive(Debug)]
enum Change {
    /// Only control fields change: TRADE_GIVE, and ROLL with independent dice.
    Control,
    /// Bank and hands change: discard, year of plenty, monopoly, trade,
    /// buying a card, a steal, and a dice outcome with independent dice.
    Economy(Economy),
    /// ROLL with balanced dice registers the roller and can reshuffle the deck.
    Roll(DiceState),
    /// A balanced dice outcome changes the dice history and pays production.
    Dice {
        dice: DiceState,
        economy: Economy,
    },
    EndTurn {
        turn_number: u64,
        playable: [u8; 5],
        new: [u8; 5],
    },
    Play {
        kind: u8,
        playable: u8,
        spent: u8,
    },
    Draw {
        kind: u8,
        new: u8,
        deck: u8,
    },
    Road(Road),
    PaidRoad {
        road: Road,
        economy: Economy,
    },
    Building(Building),
    Robber {
        hex: u8,
        production: ProductionPatch,
    },
}

impl crate::runner::Choice {
    /// Dispatch the explicit decision/chance namespace to sparse make.
    pub fn make(self, state: &mut State) -> Result<UndoRecord, Error> {
        match self {
            Self::Action(id) => state.make_action(id),
            Self::Outcome(id) => state.make_outcome(id),
        }
    }
}

impl State {
    /// Apply a checked decision in place and return sparse restoration data.
    /// As with pure apply, direct fixture edits must first rebuild caches.
    /// Returned errors leave the complete state unchanged.
    pub fn make_action(&mut self, a: u16) -> Result<UndoRecord, Error> {
        if !self.is_legal_action(a) {
            return Err(Error::InvalidAction);
        }
        let p = self.turn_owner as usize;
        let change = match self.phase {
            SETUP_SETTLEMENT => Change::Building(Building::capture(self, a)),
            SETUP_ROAD | ROAD_BUILDING => Change::Road(Road::capture(self, a)),
            PRE_ROLL | MAIN => match a {
                0 if self.config.dice == 0 => Change::Roll(self.dice),
                0 => Change::Control,
                1 => Change::EndTurn {
                    turn_number: self.turn_number,
                    playable: self.playable_development[p],
                    new: self.new_development[p],
                },
                2 => Change::Economy(Economy::capture(self)),
                3..=6 => {
                    let t = [0, 2, 3, 4][(a - 3) as usize];
                    Change::Play {
                        kind: t as u8,
                        playable: self.playable_development[p][t],
                        spent: self.spent_development[p][t],
                    }
                }
                7..=78 => Change::PaidRoad {
                    road: Road::capture(self, a),
                    economy: Economy::capture(self),
                },
                79..=186 => Change::Building(Building::capture(self, a)),
                _ => Change::Control,
            },
            ROBBER => Change::Robber {
                hex: self.robber_hex,
                production: ProductionPatch::robber(self, (a - 187) as u8),
            },
            DISCARD | YEAR_OF_PLENTY | MONOPOLY | TRADE_TAKE => {
                Change::Economy(Economy::capture(self))
            }
            _ => unreachable!("action legality determines the mutation family"),
        };
        let record = UndoRecord {
            common: Common::capture(self),
            change,
        };
        if let Err(error) = self.apply_action_in_place(a) {
            self.unmake(record);
            return Err(error);
        }
        Ok(record)
    }

    /// Apply a checked explicit chance outcome. Errors are atomic.
    pub fn make_outcome(&mut self, id: u16) -> Result<UndoRecord, Error> {
        self.check_outcome(id)?;
        let p = self.turn_owner as usize;
        let change = match self.phase {
            DICE if self.config.dice == 0 => Change::Dice {
                dice: self.dice,
                economy: Economy::capture(self),
            },
            DICE | STEAL => Change::Economy(Economy::capture(self)),
            DEVELOPMENT_DRAW => {
                let t = (id - 16) as usize;
                Change::Draw {
                    kind: t as u8,
                    new: self.new_development[p][t],
                    deck: self.development_deck[t],
                }
            }
            _ => unreachable!("outcome legality determines the mutation family"),
        };
        let record = UndoRecord {
            common: Common::capture(self),
            change,
        };
        if let Err(error) = self.apply_outcome_in_place(id) {
            self.unmake(record);
            return Err(error);
        }
        Ok(record)
    }

    /// Restore one successful make, including all caches and pending controls.
    /// Records must be consumed on their originating state in reverse make order.
    pub fn unmake(&mut self, record: UndoRecord) {
        let p = record.common.turn_owner as usize;
        match record.change {
            Change::Control => {}
            Change::Economy(economy) => economy.restore(self),
            Change::Roll(dice) => self.dice = dice,
            Change::Dice { dice, economy } => {
                self.dice = dice;
                economy.restore(self);
            }
            Change::EndTurn {
                turn_number,
                playable,
                new,
            } => {
                self.turn_number = turn_number;
                self.playable_development[p] = playable;
                self.new_development[p] = new;
            }
            Change::Play {
                kind,
                playable,
                spent,
            } => {
                self.playable_development[p][kind as usize] = playable;
                self.spent_development[p][kind as usize] = spent;
            }
            Change::Draw { kind, new, deck } => {
                self.new_development[p][kind as usize] = new;
                self.development_deck[kind as usize] = deck;
            }
            Change::Road(road) => road.restore(self, p),
            Change::PaidRoad { road, economy } => {
                road.restore(self, p);
                economy.restore(self);
            }
            Change::Building(building) => building.restore(self, p),
            Change::Robber { hex, production } => {
                self.robber_hex = hex;
                production.restore(self);
            }
        }
        record.common.restore(self);
    }
}
