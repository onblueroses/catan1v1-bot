# Reachable nonwinning position

The committed constructor trace is `nonwinning-seed191.jsonl`.
The canonical snapshot is `nonwinning-seed191.hex`.
`tests/termination.rs` replays every transition and checks the point bounds.

Reproduction uses the policy in `tests/properties.rs`:

- Game index: 47. Dice: independent (1). First player: 0.
- SplitMix64 seed: `1 + 47 * 4 + 1 * 2 + 0 = 191`.
- Generate the board with this generator. Continue with the same generator.
- Sample chance outcomes from exact integer masses.
- Prefer legal actions 7 through 186 and action 2. Otherwise sample all legal actions uniformly.
- The certificate first succeeds after transition 1522, at turn 361.
- Canonical hash: `7b7f3619b541b4a8`.
- Generator state after transition 1522: `14318020236201708877`.

Each player placed 15 roads. Neither player has an empty, distance-clear node adjoining their roads.
No new road or settlement can therefore appear. A city preserves occupation and ownership.
A city cannot change settlement eligibility, road connectivity or road cuts.
Longest Road remains with player 0. Both road lengths are 9.
The deck is empty. Player 1 played all 14 knights. Largest Army remains with player 1.

| Player | Owned nodes | Maximum building points | VP cards | Fixed award | Maximum total |
| --- | --- | --- | --- | --- | --- |
| 0 | 6 | 6 + 4 = 10 | 2 | Longest Road: 2 | 14 |
| 1 | 5 | 5 + 4 = 9 | 3 | Largest Army: 2 | 14 |

The target is 15. Neither player can win under any legal continuation.
This proves that a reachable nonwinning continuation set exists under the default target and piece limits.
The trace is an unfinished prefix. It does not introduce a draw or a turn limit.
The finite wire counters can eventually return a representation overflow. That error is not a winner.

This state is not E10-dead under the accepted frozen predicate. Each player can still upgrade one settlement to a city.
The later `dead-seed191` fixture records the frozen 14-point position at step 1763.

The regression includes two positive controls. The certificate refuses a snapshot with an unused road piece.
It also refuses a valid snapshot with an eligible settlement site. These controls do not claim historical reachability.

## Verification

The release `replay` tool validated all 1522 transitions and hashes.
It reported 1115 decisions, 407 chance transitions, turn 361 and no winner.
A separate Python check read the shared topology and canonical byte offsets without Rust caches.
It confirmed both empty site sets and both upper bounds of 14.
The diagnostic process completed. No diagnostic process remains active.
