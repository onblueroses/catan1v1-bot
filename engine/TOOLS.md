# Trace and performance tools

The tools implement [encoding v1](../spec/ENCODING.md). Each JSONL file holds one game. Hashes cover the full canonical state. Constructor traces and validated snapshot traces are reported separately. Snapshots prove snapshot validity, not reachability.

Run full suites and measurements on a dedicated benchmark machine. These examples run from the repository root after `cargo build --release`. `traces/` is an output directory of your choice:

```sh
target/release/gen --seed 41 --games 200 --dice balanced --out traces
target/release/gen --seed 41 --games 200 --dice independent --out traces
target/release/replay traces/*.jsonl
target/release/bench --games 200 --seed 41 --scale
target/release/bench --games 200 --seed 41 --threads 1 --micro-repeats 100 --trace engine/tests/fixtures/dead-seed191.jsonl
target/release/perft --depth 2 --seed 41
```

The example workload sizes are caller choices, not engine limits. Record machine identity, build revision/options, `uptime`, and `nproc` beside headline measurements. `--scale` runs every integer thread count from 1 through 16. `--threads` also accepts other positive counts.

`bench --start-index N` selects the first global game index. It defaults to zero, which preserves the existing workload. Each prepared game uses `start_index + offset` with the same root seed and policy. The inclusive last index must fit `u64`; overflow is rejected before board generation. All JSON records expose `start_index` as a decimal string, like `seed`. Trace microbenchmarks still use their supplied trace files; this field records the preceding game workload's start index.

For PGO, the existing validated 4,096-game seed-41 cohort splits into two disjoint equal halves. These counts come from that cohort, not a new workload limit:

```sh
target/release/bench --games 2048 --seed 41 --start-index 0 --threads 1
target/release/bench --games 2048 --seed 41 --start-index 2048 --threads 1
```

The first command uses training indices 0–2047. The second uses held-out indices 2048–4095. Keep compiler options and other benchmark flags fixed for a comparison.

`gen` plays to a winner or an E10 dead position and writes one game to stdout by default. A dead result has status `unfinished`, winner `null`, and reason `dead`. With `--out`, it creates files exclusively and fails rather than overwrite evidence. `--prefix-steps N` explicitly requests a prefix. A prefix that reaches terminal is a winner trace. A prefix that reaches dead records reason `dead`. Other prefixes are `unfinished` without a reason. No engine or runner has a step or turn cap. `--snapshot FILE` loads exactly 313 canonical binary bytes and retains that snapshot's board and config. `--config FILE` loads all configuration fields from JSON for constructor games. `--dice` and `--config` apply in argument order. A raw snapshot takes precedence over constructor options.

`replay` reads files, or stdin when no file is given. It rejects invalid record order, missing fields, invalid counter strings, bad hashes, wrong action/outcome namespace, invalid snapshots, header disagreements, and wrong results. A result reason must be `dead` and requires an actual dead, nonterminal state. Winner results cannot have a reason. Explicit unfinished prefixes may omit the reason, including at a dead state. Errors include the line, phase, chosen transition, turn and relevant rule family. Unknown metadata keys are allowed. Replay requires a result record even for prefixes.

The policy selects uniformly among legal action IDs. Chance sampling uses rejection sampling over exact `u128` integer weights. Game index and root seed determine each random stream before scheduling. Boards and constructor states are prepared before game timing. Worker threads each process a contiguous batch with local state and counters. They return counters once. No hot state or counters are shared. Combined counts and final-state checksum must match across thread counts.

Game timing includes policy RNG, legal/chance generation, transitions, dead checks, final hash and thread launch/join. It excludes board generation, constructors and JSON. `--games N` specifies attempts and keeps every seed. `games` counts reached results, both winners and dead positions. `attempts`, `winners`, and `dead_games` report the separate counts. `games_per_second` counts these reached results. `attempts_per_second`, `won_games_per_second`, `winner_fraction`, and `dead_fraction` expose the other measures. A completed run must satisfy `attempts = games = winners + dead_games`. The games/s figure counts wins plus dead results, as E10 defines. Decisions count player actions only. Chance steps are separate. Turns use the canonical count of normal turns started. The dead predicate is sound and incomplete. An arbitrary legal policy can continue indefinitely outside the recognized dead class.

Microbenchmarks require explicit `--micro-repeats` and one or more `--trace` files. They replay and validate the files before timing. The corpus contains every normal-play pre-transition state in those traces. Output includes phase counts and decision/chance sample counts so corpus coverage can be assessed. Apply cost uses the actual recorded action or outcome. Legal generation uses decision states only. Chance generation is separate. Policy-selection timings use choices enumerated before timing. All timed inputs and outputs cross `black_box` compiler barriers. Snapshot restore and apply-plus-snapshot-restore are measured; neither is a claim about sparse undo. Representative default-profile traces from both dice models should support final measurements.

`perft --depth N` counts every explicit decision and positive-mass chance edge. Depth counts both kinds of transition. Terminal nodes are leaves even when depth remains. Dead nodes retain every legal branch, because E10 leaves the rules unchanged. The caller chooses a search depth. Counts are path counts, not probabilities. Count overflow returns an error. `--snapshot FILE` starts from a validated binary snapshot.

Library entry points are in `runner` (`prepare`, `choose`, `play`, `parallel`, `perft`) and `trace` (`generate`, `write_header`, `write_step`, `write_result`, `replay`, `replay_with`). `choose` continues to expose legal transitions from dead positions. `play` and `generate` check `State::is_dead()` separately before choosing. `write_result` adds reason `dead` when the supplied state is dead. The explicit trace writers support deterministic targeted rule fixtures without invoking the random policy.

## Sparse search undo (E5)

`State::make_action` and `State::make_outcome` mutate in place and return a move-only `UndoRecord`. `State::unmake(record)` restores that transition. Consume records on their originating state, in reverse make order. `runner::Choice::make` dispatches the explicit action or outcome namespace. Errors leave the full state unchanged. Direct fixture edits must rebuild caches, as with pure apply. Pure `apply_action` and `apply_outcome` remain available and call the same mutation bodies after copying their input.

The record contains selected control fields and a tagged payload for the affected resource, development, dice, occupancy, and cache groups. It saves old production cells directly from topology and deduplicates their indices before mutation. The fixed four-cell capacity follows from two robber hexes times two players. A building touches at most three adjacent hexes. No full-state snapshot, state diff, allocation, or canonical serialization creates the record. The accepted layout has 528-byte states and 88-byte records on the tested x86-64 target (448-byte states before the geometry cache was added). `tests/dead.rs` asserts the state size, and `tests/geometry_actions.rs` prints all sizes. Recheck sizes after layout changes.

`undo-bench` compares real branching search with copy children against sparse make/recurse/unmake. It expands every legal action and positive-mass chance outcome. Depth counts both kinds of edge. Terminal nodes are leaves. Dead positions retain their legal branches. Before timing, both traversals must agree on leaf counts, decision/chance edge counts, and an ordered checksum of canonical leaf hashes. The sparse verification pass also checks each successor against pure apply, validates caches and invariants, and checks exact full-state and canonical restoration after each branch. Timed instantiations exclude those checks and canonical hashing. Compiler barriers consume transition inputs, children, restored states, and traversal results. Root preparation and result-buffer allocation precede timing. Method order alternates across repetitions.

The caller must supply depth and repetitions. With traces, supply explicit corpus root indices or binary snapshots. Trace files are validated before timing. Every normal-play pre-transition state supplies the micro corpus. Passing the fixed middle-quartile trace slices preserves the existing middle corpus definition. Corpus indices start at zero and follow trace input order. Micro output reports phase counts, copy-and-apply cost, sparse make/unmake cost, and separate make and unmake costs. Sparse micro loops reuse mutable roots and record slots without copying a State inside a timed loop. Validation and input-restoration checks remain outside timing.

Run the following shape on the benchmark machine. Replace the shell variables with explicit workload choices and fixed corpus paths:

```sh
cargo build --release --jobs 1 --bin undo-bench
target/release/undo-bench --depth "$depth" --repeats "$repeats" --seed 41
target/release/undo-bench --depth "$depth" --repeats "$repeats" \
  --trace "$middle_balanced_trace" --trace "$middle_independent_trace" \
  --root-index "$decision_index" --root-index "$chance_index"
```

Record the revision, build flags, executable and trace hashes, `uptime`, and `nproc` with the raw JSONL. Select root indices across ordinary decisions, pending choices, and chance phases. The constructor example alone does not cover chance edges at shallow depths. Sparse undo is accepted for the measured E5 branching ensemble. Repeat the comparison for other search workloads or when caches change State size or undo maintenance.
