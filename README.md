# catan1v1-bot

A bot for Colonist 1v1 Catan. It uses a small neural net (122,945 weights) together with a search.

The rules are the Colonist 1v1 rules: 15 victory points, balanced dice, friendly robber, discard above 9 cards,
and trades with the bank only.

By far the smallest net so far to reach this performance. 

## Strength

| Opponent | Result | Games |
|---|---|---|
| Oversized Pig, the #1 bot on the Colonist 1v1 leaderboard on 2026-11-09 (its own 3 nets, same search settings) | 50.0% ± 1.1 | 2,000 |
| VINCO #1 and #2 on the Colonist 1v1 leaderboard (the bot of the VINCO demo site) | 146 wins of 201 (73%) | 201 |

## How it works

1. **Rules engine.** Our own Colonist 1v1 engine in Rust (`engine/`).
2. **Net.** 351 inputs, two hidden layers (320 and 32), 1 output: the win chance of the player to move.
   `nets/a320c1l.bin` is the main net. `nets/s320b.bin` is used in the setup phase.
3. **Opening.** A short alpha-beta search over the settlement and road places, with a bonus for resource goals.
4. **Whole-turn search.** The bot plans its full turn (up to 6 actions) with a beam of 32 plans.
5. **Reply check.** For the 4 best plans, the bot looks at the best reply of the opponent.
6. **Rollouts.** 128 paired rollouts to the end of the game. The bot does not see the opponent's cards. It samples
   them from what is public.

All details are in `spec/BOT-SPEC.md`.

## Build and run

Rust 1.85 or later.

```sh
cargo build --release
echo '<position>' | target/release/bot decide            # one action for one position
target/release/bot arena vinco random 0 10               # 10 board pairs, both seats
cargo test --release -p catan_bot -- --include-ignored   # all tests against the fixtures
```

The position format is in `tests/fixtures/README.md`. The commands are in `spec/BOT-SPEC.md`, section 10.
Speed: approximately 23 s per game with 4 threads, 0.23 s per decision with 1 thread (Xeon Platinum 8488C).

## Folders

| Folder | Content |
|---|---|
| `engine/` | The rules engine (crate `catan_engine`) |
| `bot/` | The bot (crate `catan_bot`, binary `bot`) |
| `nets/` | The two nets |
| `spec/` | `BOT-SPEC.md` (the bot), `SPEC.md` and `ENCODING.md` (the rules), `PROVENANCE.md` (who wrote which part) |
| `tests/fixtures/` | Test data: positions, inputs, net values, decisions and complete games |

## License

GNU Affero General Public License v3.0 only (`LICENSE`). This applies to the code and to the nets.

## Acknowledgements

The design of the search follows Oversized Pig by Wei Hu
([colonizing-pig](https://github.com/TheWeiHu/colonizing-pig)). His net was a teacher in the training of our nets.
Thank you.
