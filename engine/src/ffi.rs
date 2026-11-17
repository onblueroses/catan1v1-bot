//! C ABI v1. Pointer and ownership requirements are in include/catan_engine.h.
//! Only handle creation, cloning and destruction allocate. Each fallible entry
//! catches Rust unwinding and commits outputs only after validation succeeds.
use crate::{
    ACTION_COUNT, Board, CANONICAL_SIZE, Config, Error, MAX_OUTCOMES, NONE, Outcome, State,
};
use std::{
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

// Status codes. They mirror `CatanStatus` in the header and must not be
// renumbered, because C callers compile against these values.
pub const OK: i32 = 0;
pub const NULL_POINTER: i32 = 1;
/// The caller's array is shorter than the result. Nothing was written.
pub const BUFFER_TOO_SMALL: i32 = 2;
/// A board, configuration or canonical byte string failed validation.
pub const INVALID_INPUT: i32 = 3;
/// The action or outcome is not legal in the handle's state.
pub const ILLEGAL_TRANSITION: i32 = 4;
/// A u64 turn or seven counter would overflow. The state is unchanged.
pub const COUNTER_OVERFLOW: i32 = 5;
/// A Rust panic was caught at the boundary.
pub const PANIC: i32 = 6;
pub const MISALIGNED_POINTER: i32 = 7;

/// Opaque to C. Allocate and release only through this module.
pub struct CatanState {
    inner: State,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatanBoard {
    pub terrain: [u8; 19],
    pub numbers: [u8; 19],
    pub ports: [u8; 9],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatanConfig {
    pub dice: u8,
    pub first_player: u8,
    pub friendly_robber: u8,
    pub discard_limit: u8,
    pub victory_points: u16,
    pub no_adjacent_6_8: u8,
    pub no_adjacent_same_number: u8,
    pub shortage_rule: u8,
    pub longest_road_break_rule: u8,
    pub vp_cards_hidden: u8,
    pub observation_rule: u8,
    pub discards_public: u8,
    pub road_building_needs_legal_edge: u8,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CatanOutcome {
    pub id: u32,
    pub reserved: u32,
    pub weight_lo: u64,
    pub weight_hi: u64,
    pub total_lo: u64,
    pub total_hi: u64,
    pub probability: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CatanNodeInfo {
    pub phase: u8,
    pub actor: u8,
    pub turn_owner: u8,
    pub winner: u8,
}

impl From<Config> for CatanConfig {
    fn from(c: Config) -> Self {
        Self {
            dice: c.dice,
            first_player: c.first_player,
            friendly_robber: c.friendly_robber as u8,
            discard_limit: c.discard_limit,
            victory_points: c.victory_points,
            no_adjacent_6_8: c.no_adjacent_6_8 as u8,
            no_adjacent_same_number: c.no_adjacent_same_number as u8,
            shortage_rule: c.shortage_rule,
            longest_road_break_rule: c.longest_road_break_rule,
            vp_cards_hidden: c.vp_cards_hidden as u8,
            observation_rule: c.observation_rule,
            discards_public: c.discards_public as u8,
            road_building_needs_legal_edge: c.road_building_needs_legal_edge as u8,
        }
    }
}
impl TryFrom<CatanConfig> for Config {
    type Error = i32;
    fn try_from(c: CatanConfig) -> Result<Self, i32> {
        if [
            c.friendly_robber,
            c.no_adjacent_6_8,
            c.no_adjacent_same_number,
            c.vp_cards_hidden,
            c.discards_public,
            c.road_building_needs_legal_edge,
        ]
        .iter()
        .any(|&x| x > 1)
        {
            return Err(INVALID_INPUT);
        }
        let result = Self {
            dice: c.dice,
            first_player: c.first_player,
            friendly_robber: c.friendly_robber != 0,
            discard_limit: c.discard_limit,
            victory_points: c.victory_points,
            no_adjacent_6_8: c.no_adjacent_6_8 != 0,
            no_adjacent_same_number: c.no_adjacent_same_number != 0,
            shortage_rule: c.shortage_rule,
            longest_road_break_rule: c.longest_road_break_rule,
            vp_cards_hidden: c.vp_cards_hidden != 0,
            observation_rule: c.observation_rule,
            discards_public: c.discards_public != 0,
            road_building_needs_legal_edge: c.road_building_needs_legal_edge != 0,
            reference_dice_rounding: false,
        };
        result.validate().map_err(engine_error)?;
        Ok(result)
    }
}
/// Map an engine error to a status code.
fn engine_error(e: Error) -> i32 {
    match e {
        Error::InvalidAction | Error::InvalidOutcome => ILLEGAL_TRANSITION,
        Error::RepresentationOverflow => COUNTER_OVERFLOW,
        _ => INVALID_INPUT,
    }
}
/// Run one entry point. Unwinding into C is undefined behaviour, so a panic
/// is caught here and reported as `PANIC`.
fn boundary(f: impl FnOnce() -> Result<(), i32>) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => OK,
        Ok(Err(e)) => e,
        Err(_) => PANIC,
    }
}
fn check<T>(p: *const T) -> Result<(), i32> {
    if p.is_null() {
        Err(NULL_POINTER)
    } else if (p as usize) % mem::align_of::<T>() != 0 {
        Err(MISALIGNED_POINTER)
    } else {
        Ok(())
    }
}
/// Capacity is checked before the pointer, and a null pointer is accepted
/// when nothing has to be written. The header documents both.
fn check_buffer<T>(p: *const T, needed: usize, capacity: usize) -> Result<(), i32> {
    if capacity < needed {
        return Err(BUFFER_TOO_SMALL);
    }
    if needed != 0 {
        check(p)?;
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn catan_abi_version() -> u32 {
    1
}
#[unsafe(no_mangle)]
pub extern "C" fn catan_encoding_version() -> u32 {
    1
}
#[unsafe(no_mangle)]
pub extern "C" fn catan_canonical_size() -> usize {
    CANONICAL_SIZE
}
#[unsafe(no_mangle)]
pub extern "C" fn catan_action_capacity() -> usize {
    ACTION_COUNT
}
#[unsafe(no_mangle)]
pub extern "C" fn catan_outcome_capacity() -> usize {
    MAX_OUTCOMES
}
#[unsafe(no_mangle)]
pub extern "C" fn catan_state_size() -> usize {
    mem::size_of::<CatanState>()
}
#[unsafe(no_mangle)]
pub extern "C" fn catan_state_alignment() -> usize {
    mem::align_of::<CatanState>()
}

/// # Safety
/// `out` must point to writable, aligned storage for one CatanBoard.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_board_default(out: *mut CatanBoard) -> i32 {
    boundary(|| {
        check(out)?;
        let b = Board::default();
        unsafe {
            out.write(CatanBoard {
                terrain: b.terrain,
                numbers: b.number,
                ports: b.port_type,
            });
        }
        Ok(())
    })
}
/// # Safety
/// `out` must point to writable, aligned storage for one CatanConfig.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_config_default(out: *mut CatanConfig) -> i32 {
    boundary(|| {
        check(out)?;
        unsafe {
            out.write(Config::default().into());
        }
        Ok(())
    })
}
/// # Safety
/// Inputs must be readable. `out` must be writable and disjoint from inputs.
/// On success the caller owns the returned handle and must free it once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_new(
    board: *const CatanBoard,
    config: *const CatanConfig,
    out: *mut *mut CatanState,
) -> i32 {
    boundary(|| {
        check(board)?;
        check(config)?;
        check(out)?;
        let (b, c) = unsafe { (board.read(), config.read()) };
        let state = State::new(
            Board {
                terrain: b.terrain,
                number: b.numbers,
                port_type: b.ports,
                port_edges: Board::default_port_edges(),
            },
            Config::try_from(c)?,
        )
        .map_err(engine_error)?;
        let handle = Box::into_raw(Box::new(CatanState { inner: state }));
        unsafe {
            out.write(handle);
        }
        Ok(())
    })
}
/// # Safety
/// `bytes` must reference `length` readable bytes. `out` must be writable,
/// aligned and disjoint from bytes. It receives a newly owned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_from_bytes(
    bytes: *const u8,
    length: usize,
    out: *mut *mut CatanState,
) -> i32 {
    boundary(|| {
        check(out)?;
        if length != CANONICAL_SIZE {
            return Err(INVALID_INPUT);
        }
        check(bytes)?;
        let state =
            State::from_canonical_bytes(unsafe { std::slice::from_raw_parts(bytes, length) })
                .map_err(engine_error)?;
        let handle = Box::into_raw(Box::new(CatanState { inner: state }));
        unsafe {
            out.write(handle);
        }
        Ok(())
    })
}
/// # Safety
/// `source` must be a live handle. `out` must be writable and disjoint from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_clone(
    source: *const CatanState,
    out: *mut *mut CatanState,
) -> i32 {
    boundary(|| {
        check(source)?;
        check(out)?;
        let state = unsafe { (*source).inner };
        let handle = Box::into_raw(Box::new(CatanState { inner: state }));
        unsafe {
            out.write(handle);
        }
        Ok(())
    })
}
/// # Safety
/// Both pointers must be live handles. They may be equal. No other aliases
/// may access destination concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_copy(
    destination: *mut CatanState,
    source: *const CatanState,
) -> i32 {
    boundary(|| {
        check(source)?;
        check(destination)?;
        let state = unsafe { (*source).inner };
        unsafe {
            (*destination).inner = state;
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be null or a live handle obtained from this library. A nonnull
/// handle is consumed and must never be accessed or freed again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_free(state: *mut CatanState) -> i32 {
    boundary(|| {
        if state.is_null() {
            return Ok(());
        }
        check(state)?;
        unsafe {
            drop(Box::from_raw(state));
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be live. `out` must be writable, aligned and disjoint from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_info(
    state: *const CatanState,
    out: *mut CatanNodeInfo,
) -> i32 {
    boundary(|| {
        check(state)?;
        check(out)?;
        let s = unsafe { &(*state).inner };
        let info = CatanNodeInfo {
            phase: s.phase,
            actor: s.actor().unwrap_or(NONE),
            turn_owner: s.turn_owner,
            winner: s.winner,
        };
        unsafe {
            out.write(info);
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be live. `out` must be writable and disjoint from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_is_dead(state: *const CatanState, out: *mut u8) -> i32 {
    boundary(|| {
        check(state)?;
        check(out)?;
        let value = unsafe { (*state).inner.is_dead() } as u8;
        unsafe {
            out.write(value);
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be live. Output storage must be writable, aligned and mutually
/// disjoint, and must not overlap state. Capacity counts u16 entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_legal_actions(
    state: *const CatanState,
    actions: *mut u16,
    capacity: usize,
    out_count: *mut usize,
) -> i32 {
    boundary(|| {
        check(state)?;
        check(out_count)?;
        let mut buffer = [0; ACTION_COUNT];
        let count = unsafe { (*state).inner.legal_actions(&mut buffer) };
        check_buffer(actions, count, capacity)?;
        unsafe {
            if count != 0 {
                ptr::copy_nonoverlapping(buffer.as_ptr(), actions, count);
            }
            out_count.write(count);
        }
        Ok(())
    })
}
/// # Safety
/// As for legal_actions, with capacity in CatanOutcome entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_chance_outcomes(
    state: *const CatanState,
    outcomes: *mut CatanOutcome,
    capacity: usize,
    out_count: *mut usize,
) -> i32 {
    boundary(|| {
        check(state)?;
        check(out_count)?;
        let mut buffer = [Outcome {
            id: 0,
            weight: 0,
            total: 0,
        }; MAX_OUTCOMES];
        let count = unsafe { (*state).inner.chance_outcomes(&mut buffer) };
        check_buffer(outcomes, count, capacity)?;
        for (i, o) in buffer[..count].iter().enumerate() {
            let value = CatanOutcome {
                id: o.id as u32,
                reserved: 0,
                weight_lo: o.weight as u64,
                weight_hi: (o.weight >> 64) as u64,
                total_lo: o.total as u64,
                total_hi: (o.total >> 64) as u64,
                probability: o.weight as f64 / o.total as f64,
            };
            unsafe {
                outcomes.add(i).write(value);
            }
        }
        unsafe {
            out_count.write(count);
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be a live, exclusively accessed handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_apply_action(state: *mut CatanState, action: u16) -> i32 {
    boundary(|| {
        check(state)?;
        let next = unsafe { (*state).inner.apply_action(action) }.map_err(engine_error)?;
        unsafe {
            (*state).inner = next;
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be a live, exclusively accessed handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_apply_outcome(state: *mut CatanState, outcome: u16) -> i32 {
    boundary(|| {
        check(state)?;
        let next = unsafe { (*state).inner.apply_outcome(outcome) }.map_err(engine_error)?;
        unsafe {
            (*state).inner = next;
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be live. `bytes` must point to writable, disjoint storage for
/// at least capacity bytes. Only the canonical prefix is written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_serialize(
    state: *const CatanState,
    bytes: *mut u8,
    capacity: usize,
) -> i32 {
    boundary(|| {
        check(state)?;
        check_buffer(bytes, CANONICAL_SIZE, capacity)?;
        let serialized = unsafe { (*state).inner.canonical_bytes() };
        unsafe {
            ptr::copy_nonoverlapping(serialized.as_ptr(), bytes, CANONICAL_SIZE);
        }
        Ok(())
    })
}
/// # Safety
/// `state` must be live. `out` must be writable, aligned and disjoint from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn catan_state_hash(state: *const CatanState, out: *mut u64) -> i32 {
    boundary(|| {
        check(state)?;
        check(out)?;
        let hash = unsafe { (*state).inner.hash() };
        unsafe {
            out.write(hash);
        }
        Ok(())
    })
}
