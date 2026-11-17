# Catan rules engine

`catan_engine` is a Rust rules engine for two-player Colonist 1v1 Catan. The bot in `../bot/` uses it. The engine gives explicit decisions and explicit chance outcomes. It has no internal random generator and no game-length limit. It has a Rust API and a C ABI.

## Rules

[SPEC](../spec/SPEC.md) gives the rules. Each rule has an id, for example `T5` or `D5`. Comments and test names use these ids. [ENCODING](../spec/ENCODING.md) gives the action ids, the chance ids, the phase numbers, the 313-byte canonical state and the trace format.

[QUESTIONS.md](QUESTIONS.md) records the conventions where SPEC leaves a gap, and the rule evidence that is still missing.

Start to read the code at `src/lib.rs`, which lists the modules. `src/state.rs` holds the state. `src/rules.rs` holds the legal actions and the transitions. The action id table is at the top of that file.

## Build

The crate uses Rust edition 2024. It is a member of the workspace in the repository root, so the binaries go to `../target/release/`. Run the commands in this section from `engine/`.

```sh
cargo build --release
../target/release/gen --seed 41
../target/release/bench --games 200 --seed 41 --threads 1
```

- `gen --seed 41` plays one random game. It writes the trace to standard output, one JSON object per line.
- `bench` writes one JSON line. `games_per_second` and `decisions_per_second` are the throughput figures.

`../.cargo/config.toml` sets `-Ctarget-cpu=native` for the workspace. The binaries are thus specific to the CPU of the build machine. Rebuild them on a different machine.

## Tests

```sh
cargo test --release -p catan_engine --test rules
cargo test --release -p catan_engine
CATAN_PROPERTY_GAMES_PER_CONFIG=100 cargo test --release -p catan_engine --test properties -- --nocapture
```

- `tests/rules.rs` tests each rule id. It runs in a few seconds.
- `tests/properties.rs` plays complete random games. It checks every invariant in every state: conservation of cards, cache recomputation, legal actions against `apply`, and exact restore. The default is one game for each dice model and first player. The third command plays 400 games and takes minutes.
- `tests/review_topology.rs` compares the embedded topology tables with `../spec/topology.json`. It reads that file at compile time.
- `tests/fixtures/` holds the reachable dead and nonwinning positions of seed 191 and eleven blocked-road snapshots. Each fixture has a note that tells its origin.

## Tools

[TOOLS.md](TOOLS.md) describes the command-line tools:

- `gen` plays random games and writes traces.
- `replay` reads traces and checks the state hash after each step.
- `bench` measures random-game throughput and per-operation costs.
- `perft` counts the paths of a decision and chance tree to a given depth.
- `undo-bench` compares copy-and-apply with sparse make/unmake in a search tree.

## Library API

```rust
use catan_engine::{Board, Config, State, ACTION_COUNT};
let state = State::new(Board::default(), Config::default()).unwrap();
let mut actions = [0; ACTION_COUNT];
let count = state.legal_actions(&mut actions);
assert!(count > 0);
let child = state.apply_action(actions[0]).unwrap();
let restored = State::from_canonical_bytes(&child.canonical_bytes()).unwrap();
assert_eq!(restored.hash(), child.hash());
```

`State` is a fixed-size `Copy` value with no heap allocation. The native state is 528 bytes on x86-64. The canonical form is 313 bytes.

- `legal_actions` fills a caller-owned `[u16; 226]` in increasing id order.
- `chance_outcomes` fills a caller-owned `[Outcome; 11]`. Each outcome has an exact `u128` weight.
- `apply_action` and `apply_outcome` return a new state or an error. They do not change the input.
- `make_action` and `make_outcome` change the state in place and return an 88-byte `UndoRecord`. `unmake(record)` restores the state and its caches. Use the records in reverse order (LIFO). An error keeps the full state.
- `actor()` returns the player to decide, or `None` at chance and terminal nodes.
- `winner()` returns a player only after a win. `is_dead()` is the separate E10 check that no win is reachable.
- `canonical_bytes`, `from_canonical_bytes` and `hash` implement the encoding. The decoder checks the invariants.
- `observation(player)` removes the hidden cards. Its `sample` method gives states that agree with the observation.

The transitions do not allocate memory. `Board::generate` and observation sampling use a caller-supplied `RandomSource`. `SplitMix64` is a reproducible generator. It is not cryptographic.

A rollout selects a legal action or samples a chance outcome. The random generator stays outside the state:

```rust
use catan_engine::{Board, Config, Outcome, RandomSource, SplitMix64, State,
                   ACTION_COUNT, MAX_OUTCOMES, runner};
let mut rng = SplitMix64::new(41);
let config = Config::default();
let mut state = State::new(Board::generate(&config, &mut rng), config).unwrap();
while state.winner().is_none() && !state.is_dead() {
    state = if state.actor().is_some() {
        let mut actions = [0; ACTION_COUNT];
        let n = state.legal_actions(&mut actions);
        state.apply_action(actions[rng.below(n as u64) as usize]).unwrap()
    } else {
        let mut outcomes = [Outcome::default(); MAX_OUTCOMES];
        let n = state.chance_outcomes(&mut outcomes);
        let id = runner::sample_outcome(&outcomes[..n], &mut rng);
        state.apply_outcome(id).unwrap()
    };
}
```

Do not select chance outcomes uniformly. `sample_outcome` uses the exact integer weights. The dead check is sound but not complete: a false result does not prove that a win is still reachable. Dead positions are not terminal in the rules. Runners record them as `unfinished` with reason `dead`.

Canonical bytes contain the full state, including hidden cards. For the inputs of one player, use `state.observation(player)`.

If you change state fields directly, call `recompute_caches()`. It keeps the award holders. `normalize_awards()` updates the award holders and the point caches. Call `validate()` before you use the state.

## C ABI

[include/catan_engine.h](include/catan_engine.h) declares the C ABI. The handles are opaque and owned by the library. `new`, `clone` and `from_bytes` allocate a handle. `copy(destination, source)` reuses a handle. Free each handle once. Do not copy a handle with `memcpy`. Use the 313-byte canonical form for interchange. Check the status of each call. An error keeps the state and the output parameters. `catan_is_dead` is the E10 check.

Build the library with `cargo build --release`. Then compile and run the two C programs from `engine/`:

```sh
mkdir -p .local
cc -std=c11 -Wall -Wextra -Werror -Iinclude examples/training_smoke.c \
  -L../target/release -lcatan_engine -Wl,-rpath,"$PWD/../target/release" \
  -o .local/training-smoke
.local/training-smoke
cc -std=c11 -Wall -Wextra -Werror -Iinclude tests/ffi_smoke.c \
  -L../target/release -lcatan_engine -Wl,-rpath,"$PWD/../target/release" \
  -o .local/ffi-smoke
.local/ffi-smoke
```

`examples/training_smoke.c` shows setup, reusable search storage, a dice sample, snapshot restore and a canonical roundtrip. It uses the unsigned 128-bit integer extension of GCC and Clang.

## Performance

On a 16 vCPU Xeon Platinum 8488C with a native build and 16 worker threads, the standard workload (seed 41, 4,096 random games, balanced dice) runs at 85,031 games/s and 130.9 million decisions/s. A game counts when it reaches a win or a dead position. Per operation, legal-action generation takes 24.6 ns, a checked apply 57.5 ns, a state copy 16.2 ns and a sparse make/unmake pair 61.0 ns. A profile-guided build for uniform rollouts reached 99,792 games/s on held-out games, but it made search 2 to 5% slower, so the native build stays the default.

## Licence

[THIRD_PARTY.md](THIRD_PARTY.md) lists the reference engines that were read. No third-party code was copied.
