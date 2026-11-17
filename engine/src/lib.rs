//! Deterministic, allocation-free Catan transitions. See the shared encoding contract.
//!
//! This crate is the Rust implementation of the two-player Catan rules in
//! `../spec/SPEC.md`. Rule ids such as `T5` or `D5` in comments refer to that
//! file. Byte layouts, action ids, chance ids and phase numbers come from
//! `../spec/ENCODING.md`, which both implementations share.
//!
//! Where to look:
//!
//! - [`state`]: the game state, its configuration, the canonical 313-byte
//!   serialisation, the decoder with its invariant checks, the derived caches
//!   and the Longest Road search.
//! - [`rules`]: legal actions, chance outcomes and the transition bodies.
//! - `geometry` (private): the cached sets of places where a road, a settlement
//!   or a city can go, kept current after each placement.
//! - [`dice`]: the Colonist balanced dice model and the fair dice model (C1 to C3).
//! - [`termination`]: the dead-position predicate (E10).
//! - [`observation`]: what one player sees, and sampling of full states that
//!   agree with it (I2, I3).
//! - [`undo`]: in-place make and unmake for search (E5).
//! - [`board`]: board data, board generation (B2, B3) and the random source trait.
//! - [`topology`]: the fixed adjacency tables generated from `topology.json` (B1).
//! - [`runner`]: the uniform random policy, the parallel game runner and perft.
//!   It contains no rules.
//! - [`trace`]: the JSONL trace writer and the strict replay reader (E8).
//! - [`ffi`]: the C ABI declared in `include/catan_engine.h` (E9).
//!
//! The engine has no internal randomness and no game-length limit (E2, E10).
pub mod board;
pub mod dice;
pub mod ffi;
mod geometry;
pub mod observation;
pub mod rules;
pub mod runner;
pub mod state;
pub mod termination;
pub mod topology;
pub mod trace;
pub mod undo;
pub use board::{Board, RandomSource, SplitMix64};
pub use observation::Observation;
pub use rules::{PublicDecisionEvent, PublicDecisionKind};
pub use state::{Cache, Config, Error, Outcome, State, phase};
pub use termination::dead;
pub use undo::UndoRecord;
/// Number of action ids, and the size of the legal-action buffer (E3).
pub const ACTION_COUNT: usize = 226;
/// Largest number of outcomes at one chance node: the eleven dice totals.
pub const MAX_OUTCOMES: usize = 11;
/// Length of the canonical state serialisation in bytes (E7).
pub const CANONICAL_SIZE: usize = 313;
/// Wire value for "no player", "no node", "no resource" and "no phase".
pub const NONE: u8 = 255;
/// Development deck by type: knight, victory point, road building, year of
/// plenty, monopoly (D1).
pub const DEVELOPMENT_COUNTS: [u8; 5] = [14, 5, 2, 2, 2];

#[cfg(test)]
mod advance_tests;
