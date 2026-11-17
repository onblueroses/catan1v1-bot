# Test fixtures for the bot

Black-box outputs of the current bot (VINCO settings, `nets/a320c1l.bin` and `nets/s320b.bin`), produced on
2026-11-17. The behaviour is specified in [`../../spec/BOT-SPEC.md`](../../spec/BOT-SPEC.md); section numbers below
refer to it. Every float is given as the 8 hex digits of its f32 bits (big-endian text, as `f32::to_bits` prints
it); some records also carry a decimal copy for reading. A correct implementation matches every bit.

Size: about 20 MB. 59 games, 4,667 positions (9,334 viewer input vectors), 5,624 decision records.

## Common formats

**Position.** Every position is an engine state plus the bot's own fields (BOT-SPEC 2.2):

- `state`: hex of the 323 bytes of `State::compatibility_bytes` (magic `C1PC`). Load with
  `State::from_compatibility_bytes`. The config inside is the parity configuration of BOT-SPEC 2.1.
- `protocol`: `phase`, `mover`, `before_roll`, `setup_placed`, `free_roads_left`, `last_total`, `step`,
  `discard_active`, `discard_slots` (two `[player, owed]` pairs, live or not), `dice_counts` (11), `port_ring`
  (0/1), `step_limit`.

**Action ids** are joint action ids 0..248 (BOT-SPEC 2.3). **Chance outcome ids** are bot outcome ids: dice total - 2,
stolen resource, or drawn card kind.

**Games.** Boards and seeds follow `bot arena` (BOT-SPEC 10): game `KIND-pP-sS` is pair P with player A in seat S.
Kinds: `reply` (both players use the reply setting: VINCO without rollouts), `mixed` (A reply, B random), `random`
(both random), `vinco` (both full VINCO with 4 worker threads). Self-play kinds list seat 0 only, because seat 1
gives the same game.

## Files

### `games.jsonl` (59 lines)

One complete game per line: `game`, `kind`, `pair`, `seat_a`; `board` (`resources` and `numbers` in bot tile order,
255 = desert; `ports` per slot, 255 = generic; `port_ring`; `port_nodes` as bot node pairs); `initial_state` (hex
`C1PC`); `events`, a space-separated list in which `aN` is joint action N and `cN` is chance outcome N; `winner`
(`A`, `B` or `-`), `turns`, `steps`, `decisions` (`[A, B]`, forced ones included) and `result_line` (the arena line
without the two time fields).

Test: start from the board, replay the events through your decision layer, and check every listed position on the
way (see `positions.jsonl`) and the final fields.

### `arena.txt`

Result lines of the reference arena (94 games) for the same pairs and both seats, grouped by kind, with the two
time fields (ignore those). The 59 lines of games in `games.jsonl` equal their `result_line`. The other 35 are the
seat-1 games of the self-play kinds; each is its seat-0 game with A and B swapped.

### `positions.jsonl` (4,667 lines)

Positions taken from the games: every setup position, and a deterministic sample of the rest (about one in 3 events
of `reply` games, one in 10 of `mixed`, one in 40 of `random`), chance positions included. Phases: 0: 224, 1: 224,
2: 975, 3: 945, 4: 43, 5: 240, 6: 157, 7: 1,644, 8: 194, 9: 21.

Fields: `id` (`GAME-eK`), `game`, `event` (K = the number of events of the game applied before this position),
`state`, `protocol`, `legal` (decision positions: the joint actions in ascending order, before the search's
removal of 182 at an empty deck) or `chances` (chance positions: `[outcome, weight]` in engine order), and `views`,
one per viewer 0 and 1:

| Field | Meaning |
|---|---|
| `rival_held`, `rival_new`, `deck_left` | H, N, D of the viewpoint (BOT-SPEC 3.1) |
| `inputs` | the 351 input bytes, hex (BOT-SPEC 3) |
| `a320c1l.float`, `a320c1l.fixed`, `s320b.float`, `s320b.fixed` | network values on these inputs, float path (4.2) and fixed path (4.3) |
| `search_value` | the search value (4.4) of the position itself, for a search rooted at this viewpoint with the VINCO weights: opening mode if the phase is 0 or 1, else main mode |
| `rollout_player_value` | phase > 1 only: the same with economy weight 0 (the rollout player's value, 8.6) |

### `decisions.jsonl` (5,624 lines)

One decision per line: the decision is made by the position's `mover` with RNG seed `rng_seed` (decimal u64; it is
the arena's seed for that step).

| Field | Meaning |
|---|---|
| `position` | id in `positions.jsonl`, or, when `embedded` is present, the id of the embedded position |
| `embedded` | only for the 627 decisions of the three VINCO games: `state`, `protocol`, `legal`, `game`, `event` |
| `config` | `beam`, `reply`, `r16` (16 rollout samples) or `vinco` (128). All use one worker thread except the embedded VINCO games (4); results do not depend on it |
| `viewer` | the deciding player |
| `action`, `value`, `value_bits` | the chosen joint action and the returned value (-1 for a forced action) |
| `values` | `[action, f32 bits]` for each root action, in the order in which the root valued them (a settlement root reorders by child value, BOT-SPEC 5.1). Empty when the pre-roll rule returned at once (7.5 step 2). Values -1 and -2 mark unsearched or replaced actions |
| `rng_state_after` | the four xoshiro words after the decision |
| `key181`, `sample_root_seed` | main-phase decisions only: the 181-byte key of the root viewpoint (8.1) and the root seed |
| `ms` | the time the reference took, for scale only |

Decision positions that are not main phase give the same result under every config (checked for all 1,089 of them);
they appear under `beam` and `reply`. Main-phase positions appear under `beam` and `reply`, and a sample of 253 also
under `r16` and `vinco`. The main-phase `vinco` and `r16` records whose `values` hold a single value > -1 ran their
rollouts to the end; the others fell back to the reply values (immediate win, no belief, or a cut rollout).

### `opening.jsonl` (56 lines)

For every last setup road with more than one legal road (setup settlement count 4): `viewer` and `rows`, one per
legal road: `[action, search value of the child, value + own goal, value + own goal - rival goal]` (BOT-SPEC 5.3,
5.4). The last column is the opening leaf value.

### `retention.jsonl` (2,235 lines)

The belief of BOT-SPEC 8.3 at sampled main-phase positions where the rival holds cards: `pool`, `held` (H), `cap`,
`seed` (hex; the sample seeds i = 0, 1, 2 of that position), `defined` (whether the law has mass and the checks pass)
and `sample` (the counts, or null if sampling failed).

### `rollout-traces/`

Two main-phase decisions run with 16 samples, one worker and RNG seed 12345, with every rollout event:

- `ID.decision.json`: `action`, `value_bits`, `values` of the decision.
- `ID.events.jsonl`: rollouts in order (sample i outer, finalist k inner in shortlist order). Each event has
  `stage`, `seed` (seed_i, decimal) and `key177`: the first 177 bytes of the 181-byte key (BOT-SPEC 8.1) of the
  rollout state itself, not of a viewpoint, so the rival's sampled cards are visible. It is the state after the
  step for `after_chance` and `after_action`, and the current state for the other stages:
  - `start`: the start state after the belief is applied;
  - `chance`: before sampling; `detail` = the sampled outcome, `mean` = M / T, `realized` = the stop value of the
    sampled outcome, `correction` = the running correction after this event (all f64 printed with 17 digits);
  - `after_chance`, `after_action`: the state after the step; `detail` = the outcome or joint action;
  - `cut`: a cut rollout (none occur here);
  - `stop`: `detail` = number of steps, `raw` = stop value, `correction`, `value` = the return.

### `sort-orders.txt`

275 cases of the GCC 13.3 `std::sort` order of BOT-SPEC 5.5: each line is `keys | permutation`, where the keys are
sorted descending ("a before b" when key a > key b) and the permutation lists the original positions in the sorted
order.

## Check values

| Net | SHA-256 | Fixed-point bound B | scale | unit (bits) |
|---|---|---|---|---|
| a320c1l | `36a17039ddb9acaad5c1d07b1f55b685c4238308c9b85bde267650b3be2942c6` | 32976.066788867116 | 16384 | `3523d70a` |
| s320b | `eb20391cafd24fbfa37a3fd98e0fda0806fbb925ce19548f46d7d611255a4984` | 32111.709552168846 | 32768 | `34a3d70a` |

## How these were checked

- The fixture generator replayed each game and compared its result line with the reference arena: all match.
- Each position was written, read back, and its inputs recomputed: equal.
- An independent script written from BOT-SPEC sections 3, 4.2-4.4 alone (no access to the bot's code) reproduced all
  9,334 input vectors, all 37,336 network values (two nets, two paths), and all 9,334 `search_value` entries bit for
  bit.
  The search sections (5-8) were not checked this way.
