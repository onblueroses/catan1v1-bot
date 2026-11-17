//! E9: the C ABI called from Rust. Layouts match the header, results match
//! the Rust API, and every error leaves the handle and the outputs unchanged.
//! `tests/ffi_smoke.c` exercises the same ABI from a real C program.
use catan_engine::ffi::*;
use catan_engine::{ACTION_COUNT, Board, CANONICAL_SIZE, Config, MAX_OUTCOMES, Outcome, State};
use std::{mem, ptr};

unsafe fn create() -> *mut CatanState {
    let b = Board::default();
    let board = CatanBoard {
        terrain: b.terrain,
        numbers: b.number,
        ports: b.port_type,
    };
    let mut config = mem::MaybeUninit::uninit();
    assert_eq!(unsafe { catan_config_default(config.as_mut_ptr()) }, OK);
    let mut handle = ptr::null_mut();
    assert_eq!(
        unsafe { catan_state_new(&board, config.as_ptr(), &mut handle) },
        OK
    );
    handle
}
unsafe fn bytes(handle: *const CatanState) -> [u8; CANONICAL_SIZE] {
    let mut b = [0; CANONICAL_SIZE];
    assert_eq!(
        unsafe { catan_state_serialize(handle, b.as_mut_ptr(), b.len()) },
        OK
    );
    b
}
#[test]
fn ffi_layout_and_metadata() {
    assert_eq!(catan_abi_version(), 1);
    assert_eq!(catan_encoding_version(), 1);
    assert_eq!(catan_canonical_size(), 313);
    assert_eq!(catan_action_capacity(), 226);
    assert_eq!(catan_outcome_capacity(), 11);
    assert_eq!(catan_state_size(), mem::size_of::<State>());
    assert_eq!(catan_state_alignment(), mem::align_of::<State>());
    assert_eq!(mem::size_of::<CatanBoard>(), 47);
    assert_eq!(mem::size_of::<CatanConfig>(), 14);
    assert_eq!(mem::offset_of!(CatanConfig, victory_points), 4);
    assert_eq!(
        mem::offset_of!(CatanConfig, road_building_needs_legal_edge),
        13
    );
    assert_eq!(mem::size_of::<CatanOutcome>(), 48);
    assert_eq!(mem::offset_of!(CatanOutcome, weight_lo), 8);
    assert_eq!(mem::offset_of!(CatanOutcome, probability), 40);
    assert_eq!(mem::size_of::<CatanNodeInfo>(), 4);
}
#[test]
fn ffi_d5_config_flag_roundtrip_and_nonboolean_rejection() {
    unsafe {
        let b = Board::default();
        let board = CatanBoard {
            terrain: b.terrain,
            numbers: b.number,
            ports: b.port_type,
        };
        for flag in [false, true] {
            let rust = Config {
                road_building_needs_legal_edge: flag,
                ..Config::default()
            };
            let config = CatanConfig::from(rust);
            assert_eq!(config.road_building_needs_legal_edge, flag as u8);
            assert_eq!(Config::try_from(config), Ok(rust));
            let mut state = ptr::null_mut();
            assert_eq!(catan_state_new(&board, &config, &mut state), OK);
            assert_eq!(bytes(state)[21], flag as u8);
            let saved = state;
            for value in 2..=u8::MAX {
                let mut bad = config;
                bad.road_building_needs_legal_edge = value;
                assert_eq!(Config::try_from(bad), Err(INVALID_INPUT));
                assert_eq!(catan_state_new(&board, &bad, &mut state), INVALID_INPUT);
                assert_eq!(state, saved);
            }
            assert_eq!(catan_state_free(state), OK);
        }
    }
}
#[test]
fn ffi_setup_roll_and_roundtrip_match_rust() {
    unsafe {
        let state = create();
        let mut rust = State::new(Board::default(), Config::default()).unwrap();
        assert_eq!(bytes(state), rust.canonical_bytes());
        for _ in 0..8 {
            let mut actions = [0; ACTION_COUNT];
            let mut n = usize::MAX;
            assert_eq!(
                catan_state_legal_actions(state, actions.as_mut_ptr(), actions.len(), &mut n),
                OK
            );
            let mut expected = [0; ACTION_COUNT];
            assert_eq!(n, rust.legal_actions(&mut expected));
            assert_eq!(actions[..n], expected[..n]);
            rust = rust.apply_action(actions[0]).unwrap();
            assert_eq!(catan_state_apply_action(state, actions[0]), OK);
            assert_eq!(bytes(state), rust.canonical_bytes());
        }
        assert_eq!(catan_state_apply_action(state, 0), OK);
        rust = rust.apply_action(0).unwrap();
        let mut info = CatanNodeInfo::default();
        assert_eq!(catan_state_info(state, &mut info), OK);
        assert_eq!(
            info,
            CatanNodeInfo {
                phase: 3,
                actor: 255,
                turn_owner: 0,
                winner: 255
            }
        );
        let mut outcomes = [CatanOutcome::default(); MAX_OUTCOMES];
        let mut n = 0;
        assert_eq!(
            catan_state_chance_outcomes(state, outcomes.as_mut_ptr(), outcomes.len(), &mut n),
            OK
        );
        let mut expected = [Outcome::default(); MAX_OUTCOMES];
        assert_eq!(n, rust.chance_outcomes(&mut expected));
        for (got, want) in outcomes[..n].iter().zip(expected.iter()) {
            assert_eq!(got.id, want.id as u32);
            assert_eq!(
                (u128::from(got.weight_hi) << 64) | u128::from(got.weight_lo),
                want.weight
            );
            assert_eq!(
                (u128::from(got.total_hi) << 64) | u128::from(got.total_lo),
                want.total
            );
            assert_eq!(got.probability, want.weight as f64 / want.total as f64);
        }
        assert_eq!(catan_state_apply_outcome(state, outcomes[0].id as u16), OK);
        rust = rust.apply_outcome(expected[0].id).unwrap();
        assert_eq!(bytes(state), rust.canonical_bytes());
        let serialized = bytes(state);
        let mut decoded = ptr::null_mut();
        assert_eq!(
            catan_state_from_bytes(serialized.as_ptr(), serialized.len(), &mut decoded),
            OK
        );
        let mut cloned = ptr::null_mut();
        assert_eq!(catan_state_clone(state, &mut cloned), OK);
        assert_eq!(bytes(cloned), serialized);
        assert_eq!(bytes(decoded), serialized);
        let mut hash = 0;
        assert_eq!(catan_state_hash(decoded, &mut hash), OK);
        assert_eq!(hash, rust.hash());
        assert_eq!(catan_state_copy(cloned, cloned), OK);
        assert_eq!(catan_state_apply_action(cloned, 1), OK);
        assert_ne!(bytes(cloned), serialized);
        assert_eq!(catan_state_copy(cloned, state), OK);
        assert_eq!(bytes(cloned), serialized);
        for h in [state, cloned, decoded] {
            assert_eq!(catan_state_free(h), OK);
        }
    }
}
#[test]
fn ffi_errors_preserve_state_and_outputs() {
    unsafe {
        let state = create();
        let initial = bytes(state);
        let mut actions = [0xbeef; ACTION_COUNT];
        let mut n = 987;
        assert_eq!(
            catan_state_legal_actions(state, actions.as_mut_ptr(), 1, &mut n),
            BUFFER_TOO_SMALL
        );
        assert_eq!(actions, [0xbeef; ACTION_COUNT]);
        assert_eq!(n, 987);
        assert_eq!(
            catan_state_legal_actions(state, ptr::null_mut(), ACTION_COUNT, &mut n),
            NULL_POINTER
        );
        assert_eq!(n, 987);
        assert_eq!(
            catan_state_apply_action(state, u16::MAX),
            ILLEGAL_TRANSITION
        );
        assert_eq!(catan_state_apply_outcome(state, 0), ILLEGAL_TRANSITION);
        assert_eq!(catan_state_apply_action(ptr::null_mut(), 0), NULL_POINTER);
        let mut output = [0xab; CANONICAL_SIZE];
        assert_eq!(
            catan_state_serialize(state, output.as_mut_ptr(), CANONICAL_SIZE - 1),
            BUFFER_TOO_SMALL
        );
        assert_eq!(output, [0xab; CANONICAL_SIZE]);
        assert_eq!(
            catan_state_serialize(state, ptr::null_mut(), CANONICAL_SIZE),
            NULL_POINTER
        );
        let mut unchanged = state;
        let mut corrupt = initial;
        corrupt[12] = 1;
        assert_eq!(
            catan_state_from_bytes(corrupt.as_ptr(), corrupt.len(), &mut unchanged),
            INVALID_INPUT
        );
        assert_eq!(unchanged, state);
        assert_eq!(
            catan_state_from_bytes(initial.as_ptr(), initial.len() - 1, &mut unchanged),
            INVALID_INPUT
        );
        assert_eq!(unchanged, state);
        assert_eq!(catan_state_clone(state, ptr::null_mut()), NULL_POINTER);
        let b = Board::default();
        let board = CatanBoard {
            terrain: b.terrain,
            numbers: b.number,
            ports: b.port_type,
        };
        let mut config = CatanConfig::from(Config::default());
        config.friendly_robber = 2;
        assert_eq!(
            catan_state_new(&board, &config, &mut unchanged),
            INVALID_INPUT
        );
        assert_eq!(unchanged, state);
        let mut storage = [0u64; 2];
        let misaligned = storage.as_mut_ptr().cast::<u8>().add(1).cast::<u64>();
        assert_eq!(catan_state_hash(state, misaligned), MISALIGNED_POINTER);
        assert_eq!(storage, [0; 2]);
        assert_eq!(bytes(state), initial);
        assert_eq!(catan_state_free(state), OK);
        assert_eq!(catan_state_free(ptr::null_mut()), OK);
    }
}

#[test]
fn ffi_dead_query_is_separate_and_preserves_state_and_error_outputs() {
    unsafe {
        let initial = create();
        let mut out = 99u8;
        assert_eq!(catan_is_dead(initial, &mut out), OK);
        assert_eq!(out, 0);
        assert_eq!(catan_state_free(initial), OK);

        let encoded = catan_engine::trace::decode_hex(
            include_str!("fixtures/dead-seed191.hex").trim(),
            CANONICAL_SIZE,
        )
        .unwrap();
        let mut state = ptr::null_mut();
        assert_eq!(
            catan_state_from_bytes(encoded.as_ptr(), encoded.len(), &mut state),
            OK
        );
        let before = bytes(state);
        assert_eq!(catan_is_dead(state, &mut out), OK);
        assert_eq!(out, 1);
        let mut info = CatanNodeInfo::default();
        assert_eq!(catan_state_info(state, &mut info), OK);
        assert_ne!(info.phase, catan_engine::phase::TERMINAL);
        assert_eq!(info.winner, catan_engine::NONE);

        out = 99;
        assert_eq!(catan_is_dead(ptr::null(), &mut out), NULL_POINTER);
        assert_eq!(out, 99);
        assert_eq!(catan_is_dead(state, ptr::null_mut()), NULL_POINTER);
        // The handle pointer must retain its existing alignment contract.
        let misaligned = state.cast::<u8>().add(1).cast::<CatanState>();
        assert_eq!(catan_is_dead(misaligned, &mut out), MISALIGNED_POINTER);
        assert_eq!(out, 99);
        assert_eq!(bytes(state), before);
        assert_eq!(catan_state_free(state), OK);
    }
}
