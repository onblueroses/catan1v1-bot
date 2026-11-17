//! The decision layer over the engine (BOT-SPEC 2.2, 2.3): bot positions,
//! joint actions, chance outcomes and the protocol bookkeeping.

use crate::geom::{EDGE, NODE, TILE, geometry};
use catan_engine::{Outcome, State, phase as ep};

/// Bot phase numbers (engine phases 0..9, plus 10 for the end of the game).
pub mod bp {
    pub const SETUP_SETTLEMENT: u8 = 0;
    pub const SETUP_ROAD: u8 = 1;
    pub const PRE_ROLL: u8 = 2;
    pub const DICE: u8 = 3;
    pub const DISCARD: u8 = 4;
    pub const ROBBER: u8 = 5;
    pub const STEAL: u8 = 6;
    pub const MAIN: u8 = 7;
    pub const DRAW: u8 = 8;
    pub const ROAD_BUILDING: u8 = 9;
    pub const OVER: u8 = 10;
}

pub const ROLL: u8 = 180;
pub const END_TURN: u8 = 181;
pub const BUY_CARD: u8 = 182;
pub const PLAY_KNIGHT: u8 = 183;
pub const PLAY_ROAD_BUILDING: u8 = 184;

/// Year of Plenty pairs, row `id - 185`.
pub const YOP_PAIRS: [(u8, u8); 15] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (0, 3),
    (0, 4),
    (1, 1),
    (1, 2),
    (1, 3),
    (1, 4),
    (2, 2),
    (2, 3),
    (2, 4),
    (3, 3),
    (3, 4),
    (4, 4),
];

/// Trade id of (give, take).
#[inline]
pub fn trade_id(give: u8, take: u8) -> u8 {
    229 + 4 * give + take - if take > give { 1 } else { 0 }
}

/// (give, take) of a trade id.
#[inline]
pub fn trade_pair(id: u8) -> (u8, u8) {
    let g = (id - 229) / 4;
    let j = (id - 229) % 4;
    (g, if j < g { j } else { j + 1 })
}

/// A list of joint actions in ascending order, without heap memory.
#[derive(Clone, Copy)]
pub struct Acts {
    len: u16,
    buf: [u8; 249],
}

impl Acts {
    pub fn new() -> Self {
        Acts { len: 0, buf: [0; 249] }
    }
    #[inline]
    pub fn push(&mut self, a: u8) {
        self.buf[self.len as usize] = a;
        self.len += 1;
    }
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len as usize]
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Remove one id if present.
    pub fn remove(&mut self, a: u8) {
        if let Some(i) = self.as_slice().iter().position(|&x| x == a) {
            self.buf.copy_within(i + 1..self.len as usize, i);
            self.len -= 1;
        }
    }
}

impl Default for Acts {
    fn default() -> Self {
        Self::new()
    }
}

/// The engine action sequence of a joint action (empty for the protocol
/// finish of Road Building, which the caller handles).
pub fn engine_sequence(a: u8) -> ([u16; 3], usize) {
    let a16 = a as u16;
    match a {
        0..=53 => ([79 + NODE[a as usize] as u16, 0, 0], 1),
        54..=125 => ([7 + EDGE[(a - 54) as usize] as u16, 0, 0], 1),
        126..=179 => ([133 + NODE[(a - 126) as usize] as u16, 0, 0], 1),
        180..=184 => ([a16 - 180, 0, 0], 1),
        185..=199 => {
            let (x, y) = YOP_PAIRS[(a - 185) as usize];
            ([5, 211 + x as u16, 211 + y as u16], 3)
        }
        200..=204 => ([6, 211 + (a16 - 200), 0], 2),
        205..=223 => ([187 + TILE[(a - 205) as usize] as u16, 0, 0], 1),
        224..=228 => ([206 + (a16 - 224), 0, 0], 1),
        229..=248 => {
            let (g, t) = trade_pair(a);
            ([216 + g as u16, 221 + t as u16, 0], 2)
        }
        _ => panic!("bad joint action {a}"),
    }
}

/// A bot position: the engine state plus the bot's own public facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub st: State,
    pub phase: u8,
    pub mover: u8,
    pub before_roll: bool,
    pub setup_placed: u8,
    pub free_roads_left: u8,
    pub last_total: u8,
    pub step: u32,
    pub discard_active: u8,
    pub discard_slots: [[u8; 2]; 2],
    pub dice_counts: [u8; 11],
    pub port_ring: u8,
    pub step_limit: u32,
}

impl Pos {
    /// The initial position of a game (BOT-SPEC 2.2).
    pub fn initial(st: State, port_ring: u8, step_limit: u32) -> Pos {
        let mut p = Pos {
            st,
            phase: 0,
            mover: 0,
            before_roll: false,
            setup_placed: 0,
            free_roads_left: 0,
            last_total: 0,
            step: 0,
            discard_active: 0,
            discard_slots: [[0; 2]; 2],
            dice_counts: catan_engine::dice::INITIAL_COUNTS,
            port_ring,
            step_limit,
        };
        p.mover = p.st.actor().unwrap_or(p.st.turn_owner);
        p
    }

    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.st.phase == ep::TERMINAL
    }
    #[inline]
    pub fn is_chance(&self) -> bool {
        self.st.is_chance()
    }
    /// The game is over: terminal, or at the step limit at a decision.
    #[inline]
    pub fn done(&self) -> bool {
        self.is_terminal() || (!self.is_chance() && self.step >= self.step_limit)
    }
    #[inline]
    pub fn winner(&self) -> Option<u8> {
        self.st.winner()
    }

    fn set_phase_and_mover(&mut self, pending_roads: bool) {
        self.phase = if self.is_terminal() {
            bp::OVER
        } else if pending_roads {
            bp::ROAD_BUILDING
        } else {
            self.st.phase
        };
        self.mover = self.st.actor().unwrap_or(self.st.turn_owner);
    }

    /// The legal joint actions in ascending order.
    pub fn legal(&self) -> Acts {
        let mut out = Acts::new();
        if self.done() || self.is_chance() {
            return out;
        }
        if self.phase == bp::ROAD_BUILDING && self.st.phase != ep::ROAD_BUILDING {
            out.push(END_TURN);
            return out;
        }
        let g = geometry();
        let mut buf = [0u16; 226];
        let n = self.st.legal_actions(&mut buf);
        let mut ids = [0u8; 249];
        let mut k = 0usize;
        let mut push = |a: u8| {
            ids[k] = a;
            k += 1;
        };
        let p = self.st.turn_owner as usize;
        for &ea in &buf[..n] {
            match ea {
                0..=4 => push(180 + ea as u8),
                5 => {
                    let bank = self.st.bank;
                    for (row, &(x, y)) in YOP_PAIRS.iter().enumerate() {
                        let ok = if x == y {
                            bank[x as usize] >= 2
                        } else {
                            bank[x as usize] >= 1 && bank[y as usize] >= 1
                        };
                        if ok {
                            push(185 + row as u8);
                        }
                    }
                }
                6 => {
                    for r in 0..5 {
                        push(200 + r);
                    }
                }
                7..=78 => push(54 + g.edge_of_engine[(ea - 7) as usize]),
                79..=132 => push(g.node_of_engine[(ea - 79) as usize]),
                133..=186 => push(126 + g.node_of_engine[(ea - 133) as usize]),
                187..=205 => push(205 + g.tile_of_engine[(ea - 187) as usize]),
                206..=210 => push(224 + (ea - 206) as u8),
                216..=220 => {
                    let give = (ea - 216) as u8;
                    debug_assert!(self.st.resources[p][give as usize] >= self.st.cache.trade_rates[p][give as usize]);
                    for take in 0..5u8 {
                        if take != give && self.st.bank[take as usize] > 0 {
                            push(trade_id(give, take));
                        }
                    }
                }
                _ => panic!("engine action {ea} in phase {}", self.st.phase),
            }
        }
        let ids = &mut ids[..k];
        ids.sort_unstable();
        for &a in ids.iter() {
            out.push(a);
        }
        out
    }

    /// Apply a legal joint action (BOT-SPEC 2.2).
    pub fn apply(&self, a: u8) -> Pos {
        let mut p = *self;
        let old_engine_phase = self.st.phase;
        if self.phase == bp::ROAD_BUILDING && a == END_TURN {
            debug_assert!(self.st.phase != ep::ROAD_BUILDING);
            p.free_roads_left = 0;
        } else {
            let (seq, n) = engine_sequence(a);
            for &ea in &seq[..n] {
                p.st = p
                    .st
                    .apply_action(ea)
                    .unwrap_or_else(|e| panic!("joint {a} engine {ea}: {e:?}"));
            }
            if a == PLAY_ROAD_BUILDING {
                p.free_roads_left = 2;
            }
            if self.phase == bp::ROAD_BUILDING && (54..126).contains(&a) {
                p.free_roads_left = p.free_roads_left.saturating_sub(1);
                if p.st.cache.pieces_left[p.st.turn_owner as usize][0] == 0 {
                    p.free_roads_left = 0;
                }
            }
        }
        if self.phase == bp::SETUP_SETTLEMENT && a < 54 {
            p.setup_placed += 1;
        }
        if self.phase == bp::DISCARD {
            p.discard_slots[0][1] -= 1;
            if p.discard_slots[0][1] == 0 {
                p.discard_slots[0] = p.discard_slots[1];
                p.discard_active -= 1;
            }
        }
        p.step += 1;
        let np = p.st.phase;
        if np == ep::SETUP_SETTLEMENT || np == ep::SETUP_ROAD {
            p.before_roll = false;
        } else if old_engine_phase == ep::SETUP_SETTLEMENT
            || old_engine_phase == ep::SETUP_ROAD
            || (self.phase == bp::MAIN && a == END_TURN)
        {
            p.before_roll = true;
        }
        let pending = !p.is_terminal()
            && p.free_roads_left > 0
            && (self.phase == bp::ROAD_BUILDING || a == PLAY_ROAD_BUILDING);
        if !pending && p.st.phase == ep::PRE_ROLL && p.st.development_played {
            p.st = p.st.apply_action(0).expect("forced roll");
        }
        p.set_phase_and_mover(pending);
        p
    }

    /// Chance outcomes as (bot outcome id, engine weight), in engine order.
    pub fn outcomes(&self) -> ([(u8, u128); 11], usize) {
        let mut out = [(0u8, 0u128); 11];
        if !self.is_chance() || self.is_terminal() {
            return (out, 0);
        }
        let mut buf = [Outcome::default(); 11];
        let n = self.st.chance_outcomes(&mut buf);
        let base = match self.st.phase {
            ep::DICE => 0,
            ep::STEAL => 11,
            _ => 16,
        };
        for i in 0..n {
            out[i] = ((buf[i].id - base) as u8, buf[i].weight);
        }
        (out, n)
    }

    /// Apply a chance outcome by bot outcome id.
    pub fn apply_outcome(&self, o: u8) -> Pos {
        let mut p = *self;
        let engine_id = match self.st.phase {
            ep::DICE => o as u16,
            ep::STEAL => o as u16 + 11,
            _ => o as u16 + 16,
        };
        p.st = self
            .st
            .apply_outcome(engine_id)
            .unwrap_or_else(|e| panic!("outcome {o}: {e:?}"));
        if self.phase == bp::DICE {
            p.dice_counts = p.st.dice.counts;
            if o + 2 == 7 {
                p.discard_active = 0;
                let owner = self.st.turn_owner;
                for q in [owner, 1 - owner] {
                    let cards = self.st.cache.resource_totals[q as usize];
                    if cards > self.st.config.discard_limit {
                        p.discard_slots[p.discard_active as usize] = [q, cards / 2];
                        p.discard_active += 1;
                    }
                }
            }
            p.last_total = o + 2;
            p.before_roll = false;
        }
        if p.st.phase == ep::PRE_ROLL && p.st.development_played {
            p.st = p.st.apply_action(0).expect("forced roll");
        }
        p.set_phase_and_mover(false);
        p
    }
}

// ---- JSON forms ----

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Protocol {
    pub phase: u8,
    pub mover: u8,
    pub before_roll: bool,
    pub setup_placed: u8,
    pub free_roads_left: u8,
    pub last_total: u8,
    pub step: u32,
    pub discard_active: u8,
    pub discard_slots: [[u8; 2]; 2],
    pub dice_counts: [u8; 11],
    pub port_ring: u8,
    pub step_limit: u32,
}

pub fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

pub fn hex_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

impl Pos {
    pub fn from_parts(state_hex: &str, pr: &Protocol) -> Result<Pos, String> {
        let st = State::from_compatibility_bytes(&hex_decode(state_hex))
            .map_err(|e| format!("state: {e:?}"))?;
        Ok(Pos {
            st,
            phase: pr.phase,
            mover: pr.mover,
            before_roll: pr.before_roll,
            setup_placed: pr.setup_placed,
            free_roads_left: pr.free_roads_left,
            last_total: pr.last_total,
            step: pr.step,
            discard_active: pr.discard_active,
            discard_slots: pr.discard_slots,
            dice_counts: pr.dice_counts,
            port_ring: pr.port_ring,
            step_limit: pr.step_limit,
        })
    }
    pub fn protocol(&self) -> Protocol {
        Protocol {
            phase: self.phase,
            mover: self.mover,
            before_roll: self.before_roll,
            setup_placed: self.setup_placed,
            free_roads_left: self.free_roads_left,
            last_total: self.last_total,
            step: self.step,
            discard_active: self.discard_active,
            discard_slots: self.discard_slots,
            dice_counts: self.dice_counts,
            port_ring: self.port_ring,
            step_limit: self.step_limit,
        }
    }
    pub fn state_hex(&self) -> String {
        hex_encode(&self.st.compatibility_bytes())
    }
}
