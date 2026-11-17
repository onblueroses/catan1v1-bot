# Provenance of BOT-SPEC.md and the test fixtures

## Who and when

Written on 2026-11-17 by Claude Opus acting as the **spec writer** of a clean-room rewrite. A second, separate
instance, the implementer, builds the new bot from `BOT-SPEC.md` and `tests/fixtures/` only. The implementer does not see the
reference code or its Rust port.

## What the spec writer read

- The reference bot, Oversized Pig by Wei Hu (`colonizing-pig`, commit `3eb2be8`). It has no licence. Only its
  behaviour and numeric facts were taken from it, mostly through the port below.
- The line-by-line Rust port of it in a private repository (commit `23bac21`, mirrored as `d01d3af` on a separate
  machine with identical sources), including its `PORT.md` parity record.
- Our study notes `SEARCH.md`, `FEATURES.md`, `ENGINE-COMPARISON.md` and `RESULTS.md` (the VINCO settings), and the
  VINCO run scripts (for the exact settings string).
- Our engine `catan_engine` (`engine/`, a copy of our engine repository at `fe9a482`) and its encoding notes.

## Rules followed

- The spec describes behaviour: quantities, order, constants, formulas, tie-breaks and floating-point steps, in prose,
  tables and formulas. It contains no code, no code paraphrased line by line, no identifiers, function names,
  variable names or comments of the reference or the port, and no file paths or line numbers of either.
- Terms in the spec and the fixture field names were chosen new (for example "viewpoint", "joint action", "turn-end
  value", `mover`, `before_roll`, `rival_held`). Names that appear are those of our own engine API.
- Numeric facts are stated in full: the bot's node, edge and tile numbering, the joint action table, the input
  layout, the network file format, the port rings, the fitted belief weights, seeds, stream constants and limits.
  Standard public algorithms are named and specified: xoshiro256\*\*, SplitMix64, Lemire's bounded integers,
  FNV-1a, the MurmurHash3 finalizer, and the GCC 13 libstdc++ `std::sort`.
- Fixtures were produced by running the existing bot as a black box. A scratch generator on a separate
  machine, outside this repository, linked the port as a library, played arena-style games and
  recorded positions, input bytes, network values and decisions. In a scratch copy of the port it added three
  read-only accessor functions (the search value of the root, the 181-byte key, one belief sample) so that these
  intermediate values could be written out; no behaviour was changed, and the generator's games equal the
  unchanged arena's results line for line. The rollout event traces come from the port's own trace switch. The
  fixtures hold only data, in formats defined for this repository: states as our engine's `C1PC` bytes, not the
  reference's formats.
- `tests/fixtures/sort-orders.txt` is data from the port's test fixture: key lists and the permutation that GCC
  13.3 `std::sort` produced for them.
- Nothing outside this repository was changed (the scratch generator is new and separate). Nothing was
  deleted or committed.

## How the spec was checked

An independent Python script, written from the spec text of sections 3 and 4 and our engine's topology and byte
layout only, reproduced all 9,334 input vectors, all 37,336 network values (both nets, float and fixed paths) and
all 9,334 search values of the fixtures bit for bit. This needed glibc `expf`, `logf`, `exp` and `log1p`; NumPy's
`exp` differs by one ulp in some cases. Sections 2.2 (decision layer) and 5-8 (search) were transcribed and
reviewed against the port, but not executed independently: the first implementation will be their first
independent test.

## Deliberately left out

- **Speed measures** of the reference and the port: incremental first-layer updates, the int16 compact weight form
  and its corrections, SIMD kernels, input templates and build caches, the memo tables (only their transparency is
  stated), and thread scheduling.
- **Features that VINCO does not use**: ensembles of three nets and a separate board-opening net, the board-graph
  evaluator, the awards term, discard sets, opening symmetry, multi-round rollout elimination, rollout
  normalization, plan terms inside rollouts, the uniform rollout belief, narrower input sets (175, 223, 329 inputs),
  the reference's 38-field settings string (replaced by named presets) and its training records.
- **Reference-specific tools and formats**: the oracle stream format, the live-play tool for Colonist, the
  reference's bookkeeping of card-holding history (unused by any decision), and its 10,000-step game cap outside
  parity games.
- **Training**: how the nets were trained is not part of this spec; the nets are ours.

## Open points

- The Colonist rule for a bank shortage is unverified; the spec keeps `shortage_rule = 1` for Colonist play until the
  rule is confirmed (BOT-SPEC 2.1).
- Year of Plenty with a short bank: the bot offers it only when the bank holds both cards. Colonist may allow it
  with fewer. This changes only what the bot considers, not the engine.

## Implementer

Written on 2026-11-17 by Claude Opus acting as the **implementer** of the clean-room rewrite. It built the
crate `bot/` from these sources only:

- `spec/BOT-SPEC.md`, `spec/SPEC.md`, `spec/ENCODING.md` and this file;
- our engine `engine/` (its source and documentation) and the two nets in `nets/`;
- the test data in `tests/fixtures/` with its `README.md`, used as a black box;
- crates.io crates and general documentation (the Rust standard library, the libstdc++ `std::sort` algorithm as
  described in BOT-SPEC 5.5).

The implementer did not open, read, list or search any of the forbidden paths: the earlier port, the reference code,
the fixture generator, the earlier code audit and the working records of the other instances, on all machines.
It did not search the web for the reference bot or its author.
Where the spec was unclear, the fixtures decided; each such reading is in `bot/NOTES.md`.

## Speed work

Done on 2026-11-17 by a third Claude Opus instance (commits c4c94ba to b8892a5). Its instructions carried the same
clean-room rule as the implementer's. It read only this repository. A separate check of all 198 of its tool calls
found that none opened a forbidden path. The full test suite passes after its last commit.
