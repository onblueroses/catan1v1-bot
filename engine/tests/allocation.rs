//! A per-thread allocator gate excludes harness allocations from measurements.
use catan_engine::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counted;
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
fn allocation() {
    if ACTIVE.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
    }
}
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation();
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
fn measured(f: impl FnOnce()) -> usize {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            ACTIVE.with(|a| a.set(false));
        }
    }
    ALLOCATIONS.with(|n| n.set(0));
    ACTIVE.with(|a| a.set(true));
    let guard = Guard;
    f();
    drop(guard);
    ALLOCATIONS.with(Cell::get)
}
fn supply(s: &mut State, r: usize, minimum: u8) {
    let p = s.turn_owner as usize;
    if s.resources[p][r] < minimum {
        let n = minimum - s.resources[p][r];
        s.bank[r] -= n;
        s.resources[p][r] += n;
    }
    s.recompute_caches();
}
fn card(mut s: State, t: usize) -> State {
    s.playable_development[s.turn_owner as usize][t] += 1;
    s.development_deck[t] -= 1;
    s.recompute_caches();
    s
}
fn phases() -> [State; 14] {
    let mut config = Config::default();
    config.dice = 1;
    let initial = State::new(Board::default(), config).unwrap();
    let mut states = [initial; 14];
    let mut s = initial;
    // Complete exactly the four setup pairs required by S1, picking the first legal action.
    for placement in 0..8 {
        let mut actions = [0; 226];
        let n = s.legal_actions(&mut actions);
        assert!(n > 0);
        s = s.apply_action(actions[0]).unwrap();
        if placement == 0 {
            states[1] = s;
        }
    }
    states[2] = s;
    states[3] = s.apply_action(0).unwrap();
    states[7] = states[3].apply_outcome(0).unwrap();
    states[5] = states[3].apply_outcome(5).unwrap();
    let mut discard = states[2];
    supply(&mut discard, 0, 10);
    states[4] = discard.apply_action(0).unwrap().apply_outcome(5).unwrap();
    let mut robber = states[5];
    robber.config.friendly_robber = false;
    let opponent = 1 - robber.turn_owner as usize;
    robber.resources[opponent][0] += 1;
    robber.bank[0] -= 1;
    robber.recompute_caches();
    let mut actions = [0; 226];
    let n = robber.legal_actions(&mut actions);
    states[6] = actions[..n]
        .iter()
        .find_map(|&a| {
            let after = robber.apply_action(a).unwrap();
            (after.phase == phase::STEAL).then_some(after)
        })
        .expect("opponent setup settlement supplies a theft destination");
    let mut buy = states[7];
    for r in 2..5 {
        supply(&mut buy, r, 1);
    }
    states[8] = buy.apply_action(2).unwrap();
    states[9] = card(states[7], 2).apply_action(4).unwrap();
    states[10] = card(states[7], 3).apply_action(5).unwrap();
    states[11] = card(states[7], 4).apply_action(6).unwrap();
    let mut trade = states[7];
    supply(&mut trade, 0, 4);
    states[12] = trade.apply_action(216).unwrap();
    let mut winning = buy;
    winning.config.victory_points =
        u16::from(winning.cache.total_points[winning.turn_owner as usize]) + 1;
    states[13] = winning.apply_action(2).unwrap().apply_outcome(17).unwrap();
    for (phase, s) in states.iter().enumerate() {
        assert_eq!(s.phase as usize, phase);
        s.validate().unwrap();
    }
    states
}

#[test]
fn e1_e2_e3_all_fourteen_phases_have_zero_heap_allocations_with_positive_control() {
    let positive = measured(|| {
        let allocation = Box::new(std::hint::black_box([1u64; 32]));
        std::hint::black_box(&allocation);
        drop(allocation);
    });
    assert!(
        positive > 0,
        "allocator instrument did not detect the allocating control"
    );
    let states = phases();
    for state in states {
        let mut rng = SplitMix64::new(u64::from(state.phase));
        let allocations = measured(|| {
            let mut actions = [0u16; 226];
            let n = state.legal_actions(&mut actions);
            for &action in &actions[..n] {
                std::hint::black_box(state.apply_action(action).unwrap());
                let mut current = state;
                let record = current.make_action(action).unwrap();
                std::hint::black_box(&mut current);
                current.unmake(record);
                std::hint::black_box(current);
            }
            let mut outcomes = [Outcome::default(); 11];
            let n = state.chance_outcomes(&mut outcomes);
            for outcome in &outcomes[..n] {
                std::hint::black_box(state.apply_outcome(outcome.id).unwrap());
                let mut current = state;
                let record = current.make_outcome(outcome.id).unwrap();
                std::hint::black_box(&mut current);
                current.unmake(record);
                std::hint::black_box(current);
            }
            let bytes = state.canonical_bytes();
            std::hint::black_box(State::from_canonical_bytes(&bytes).unwrap());
            std::hint::black_box(state.hash());
            std::hint::black_box(state.is_dead());
            for observer in 0..2 {
                let observation = state.observation(observer).unwrap();
                std::hint::black_box(observation.sample(&mut rng).unwrap());
            }
        });
        assert_eq!(allocations, 0, "phase {} allocated", state.phase);
    }
    let encoded = trace::decode_hex(
        include_str!("fixtures/dead-seed191.hex").trim(),
        CANONICAL_SIZE,
    )
    .unwrap();
    let state = State::from_canonical_bytes(&encoded).unwrap();
    assert!(state.is_dead());
    let mut handle = std::ptr::null_mut();
    unsafe {
        assert_eq!(
            ffi::catan_state_from_bytes(encoded.as_ptr(), encoded.len(), &mut handle),
            ffi::OK
        );
    }
    let mut result = 0;
    let allocations = measured(|| {
        std::hint::black_box(dead(&state));
        unsafe {
            assert_eq!(ffi::catan_is_dead(handle, &mut result), ffi::OK);
        }
    });
    assert_eq!(result, 1);
    assert_eq!(allocations, 0, "Rust and C dead queries allocated");
    unsafe {
        assert_eq!(ffi::catan_state_free(handle), ffi::OK);
    }
}

#[test]
fn e5_all_phase_transitions_restore_exact_state_and_caches() {
    for state in phases() {
        for choice in (0..ACTION_COUNT as u16)
            .chain([ACTION_COUNT as u16, u16::MAX])
            .map(runner::Choice::Action)
            .chain((0..21).map(runner::Choice::Outcome))
        {
            let expected = choice.apply(&state);
            let mut current = state;
            match choice.make(&mut current) {
                Ok(record) => {
                    assert_eq!(Ok(current), expected, "phase {} {choice:?}", state.phase);
                    let mut rebuilt = current;
                    rebuilt.recompute_caches();
                    assert_eq!(current.cache, rebuilt.cache);
                    current.unmake(record);
                }
                Err(error) => assert_eq!(Err(error), expected),
            }
            assert_eq!(current, state, "phase {} {choice:?}", state.phase);
            assert_eq!(current.canonical_bytes(), state.canonical_bytes());
            let mut rebuilt = current;
            rebuilt.recompute_caches();
            assert_eq!(current.cache, rebuilt.cache);
        }
    }
}
