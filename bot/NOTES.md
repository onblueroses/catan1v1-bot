# Bot implementation notes

Clean-room implementation of `spec/BOT-SPEC.md` (crate `catan_bot`). This file records the readings chosen
where the spec was unclear, and the measurements.

## Ambiguities and readings

- 2.2, dice outcome: "set `last_total` = o + 2 and `before_roll` = false" is read as applying to every dice
  outcome, not only to a 7. The decision-layer replay of all 59 games matches with this reading.

- 6.2, immediate win: "set value[first] = 1 and return at once (all other values stay -2)" is read as: keep the
  values that this search already set for other root actions, and return. Resetting them to -2 failed 12 `beam`
  records (for example `mixed-p100-s1-e292`, `reply-p0-s0-e348`); keeping them matches all.
- 7.5 step 6 and 8.8: the decision RNG draws once at the root choice; the fixtures agree (`rng_state_after`).
- 8.4: `main` is counted only when the rollout player decides at a main phase (not for a forced 181). Either
  reading gives the same play, because a forced 181 ends the turn and resets the counter.
- 10, `bot decide`: the RNG is seed(S) for each input line (fresh per position). An input line may also be the
  arena message `{"position": ..., "rng_seed": "..."}`; then its `rng_seed` is used. The arena accepts a `cmd:`
  answer as a bare joint action id or as a JSON object with `action` (so `cmd:bot decide` works).
- Fixture count: `tests/fixtures/README.md` speaks of 627 decisions of the three VINCO games; `decisions.jsonl`
  holds 592 embedded records. The three games have 859 decisions in all (forced ones included); the arena test
  replays them all, so every decision of the three games is checked through its events.

## Measurements

- Acceptance 1-2 (`cargo test --release --test layer`): decision layer 4,667 / 4,667 positions, inputs
  9,334 / 9,334, net values 37,336 / 37,336, search values (VINCO and rollout-player) 17,772 / 17,772.
- Acceptance 3 (`--test search`, `--test games`, ignored tests with `--ignored`): sort orders 275 / 275, opening
  rows 163 / 163, belief samples 2,235 / 2,235, rollout traces 1,420 / 1,420 and 2,247 / 2,247 events,
  decisions beam 2,263 / 2,263, reply 2,263 / 2,263, r16 253 / 253, vinco sampled 253 / 253, vinco embedded
  592 / 592; games: random, mixed and reply 56 / 56, VINCO self-play 3 / 3 (every event and result line).
- Negative control (`CATAN_FIXTURES` points the tests at a corrupted copy): one flipped input nibble, one flipped
  `value_bits` bit in a `beam` record and one changed sort permutation each made their test fail
  (`inputs 9,333 / 9,334`, `beam 49 / 50`, `gcc_sort_orders` panicked).
- CLI arena on our benchmark machine (16 vCPU Xeon Platinum 8488C): `bot arena reply reply 0 2`, `bot arena random random 200 1` and
  `bot arena reply "cmd:./target/release/bot decide --preset reply --workers 1" 0 1` print the result lines of
  `arena.txt` (time fields aside).

## Speed (acceptance 4)

Machine: our benchmark machine (16 vCPU Xeon Platinum 8488C), release build with `target-cpu=native`
(`.cargo/config.toml`; Rust never contracts float operations into FMA, so values do not change).

- `bot arena vinco vinco 0 3` (6 complete VINCO self-play games, 4 rollout workers, run alone): 420 s wall,
  **70.1 s per game** (about 35 s per player), 387 % CPU (about 3.9 cores busy), 42 MB peak memory. All six result
  lines equal `arena.txt` apart from the time fields. The reference times in `arena.txt` average 51.7 s per game
  (hardware unknown).
- `examples/bench.rs` (20 main-phase VINCO decisions, one worker): 2.39 s per decision before, 0.92 s after the
  two speed measures below.
- Speed measures (both exact, the full fixture suite passes after them): native CPU target; and an incremental
  first layer: each thread keeps the inputs and integer sums of its last fixed-point evaluation per net, and
  updates only the changed inputs when fewer inputs changed than are non-zero (BOT-SPEC 4.3 allows this).
  Micro-benchmark: fixed evaluation 14.2 us -> 6.4 us, inputs 7 us per viewpoint.
- Not done: the rollout-player choice memo of 8.6 and a shared value memo across rollouts.

## Speed work (2026-11-17, commits c4c94ba to b8892a5)

Per-thread input caches, an AVX-512 kernel and an int16 VNNI first layer for the fixed-point net (exact), a
per-thread cache of search values shared across searches, persistent rollout pools, and cached rollout stop values.
Measured on our benchmark machine (16 vCPU Xeon Platinum 8488C) after b8892a5 (machine shared, load 16 to 34):

| Measure | Before | After |
|---|---|---|
| VINCO decision, 1 worker (`examples/bench 20`) | 0.92 s | 0.23 s |
| VINCO game, 4 workers (`bot arena vinco vinco 0 1`, 2 games) | 70.1 s | 23 s |

The full suite passes after b8892a5. Both games equal `arena.txt` apart from the time fields.
