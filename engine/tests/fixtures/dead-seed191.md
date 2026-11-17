# Reachable frozen dead position

`dead-seed191.jsonl` is the committed constructor trace. `dead-seed191.hex` contains its final canonical state.
`tests/dead.rs` replays every transition and checks the E10 predicate.

This trace continues the same seed and policy as `nonwinning-seed191.md`.
The first frozen position occurs at step 1763, turn 402. Its hash is `4b3fd2661552e6c0`.
The generator state after this transition is `5494071951108994201`.

Each player placed 15 roads and four cities. Neither player has a geometrically eligible settlement site.
The development deck is empty. Player 1 played all 14 knights.
Player 0 holds Longest Road with tied road lengths of 9. Player 1 holds Largest Army.
Both players have 14 points. No accepted action or chance outcome can change either total.

The result is `unfinished` with reason `dead`. It is not a draw or a rules-engine terminal state.
The snapshot remains 313 canonical bytes. The dead predicate added no state field: the Rust State was 448 bytes before and after it.
The later geometry cache raised the Rust State to 528 bytes without changing the canonical bytes.

The tests include positive controls for every applicable condition.
They enumerate all outgoing actions and outcomes from witnesses of each possible dead phase.
The phase coverage uses both dice models and both turn owners.
They also test an army holder with remaining opposing knights that can tie but cannot pass the holder.
