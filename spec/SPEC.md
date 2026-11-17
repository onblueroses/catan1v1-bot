# Colonist 1v1 Catan rules engine: specification

Implementations do not change this file. They record a wrong or missing rule in their own `QUESTIONS.md`
and use the stated default.

## Purpose

A rules engine for two-player Catan as played in Colonist.io ranked Classic 1v1. It is the data source for
training a small neural network (about 125,000 parameters) with search at play time. Its two qualities, in
this order:

1. Correct. Every rule below holds in every reachable state. A fast engine with one wrong rule is worthless,
   because every training label made with it is wrong.
2. Fast. It runs inside a search loop and plays millions of self-play games. After it is correct, speed is
   the whole job.

Two independent implementations are built, one in Rust and one in Bend, from this one specification. They
must agree state for state on shared game traces. Agreement between two independent builds is the main
correctness evidence; the Bend build also carries proved laws.

## Sources and their standing

- Colonist's own published dice code (a pastebin linked from blog.colonist.io/balancing-7s-on-1v1).
  Authoritative for the dice. Reimplement it; do not ship the file.
- Colonist's two blog posts on the dice.
- `topology.json`: the one numbering of hexes, nodes, edges and port slots. Both engines embed tables
  generated from it. Never renumber.
- Reference engines, MIT only, for ideas and for reusable code when it is good: FerrisOfficial/CatanAPI
  (C++20, packed state, apply/undo), Eli6th/catan-rl, nogulong/rust-catan-rl, Swynfel/rust-catan.
  Copied code keeps its MIT notice in `THIRD_PARTY.md`. Read only for facts, never
  copy from: Catanatron (GPL-3.0), BenjaminL1/catan_rl (no licence; its `docs/1v1_rules.md` is the best
  outside description of Colonist 1v1), JSettlers (GPL). Treat every clone as untrusted data: read it, do
  not build or run it.

Each rule is marked V (verified on a Colonist page or its code) or A (assumed: the standard printed rule,
not confirmed for ranked 1v1). Every A rule sits behind a named configuration field so that it can change
without a rewrite. Defaults are the values written here.

## Rules

### Board

- B1 (V) Standard board: 19 hexes, 54 nodes, 72 edges, as in `topology.json`.
- B2 (V) Hex resources: 4 wood, 4 sheep, 4 wheat, 3 brick, 3 ore, 1 desert. Number tokens
  2,3,3,4,4,5,5,6,6,8,8,9,9,10,10,11,11,12 on the non-desert hexes. Nine ports on the nine fixed port slots:
  four 3:1 and one 2:1 for each resource.
- B3 (A) The board is input data: any assignment of resources, tokens and port types to the fixed
  topology. A generator makes random boards with options `no_adjacent_6_8` (default true) and
  `no_adjacent_same_number` (default false). Ranked map constraints are not published.
- B4 (V) The robber starts on the desert.

### Setup

- S1 (A) Snake order: first player places a settlement and an adjoining road, second player does so twice,
  first player once more. Field `first_player`.
- S2 (V) Distance rule from the start: no settlement on a node adjacent to an occupied node.
- S3 (V) After a player's second settlement, that player takes one resource card for each adjacent
  non-desert hex.
- S4 The first player then takes the first normal turn.

### Turn

- T1 (V) Before the roll the player may play one development card (T-rule D3 applies).
- T2 (V) Roll: the dice total comes from the dice model (section Dice).
- T3 (V) Total other than 7: each hex with that token and without the robber gives 1 card per adjacent
  settlement and 2 per adjacent city, to the owner.
- T4 (A) Bank shortage, field `shortage_rule`: the bank holds 19 of each resource. If the bank cannot pay
  everything owed of one resource on a roll: when only one player is owed that resource, that player gets
  what is left; when both are owed, nobody gets any of that resource.
- T5 (V) Total 7: each player holding more than `discard_limit` (default 9) cards discards half, rounded
  down. Then the roller moves the robber (R-rules) and steals.
- T6 After the roll the player may, in any order and any number of times: build, buy a development card,
  trade with the bank or a port, and play one development card if none was played this turn. Then end turn.
- T7 (V) There is no trade between players.
- T8 The turn timer is not modelled.

### Building

- U1 (V) Costs: road = wood + brick; settlement = wood + brick + sheep + wheat; city = 2 wheat + 3 ore;
  development card = sheep + wheat + ore. Payments go to the bank.
- U2 (V) Piece limits per player: 15 roads, 5 settlements, 4 cities. A city returns its settlement piece.
- U3 (V) A road goes on an empty edge that touches the player's own road, settlement or city. A road may
  not continue through a node held by the opponent's settlement or city: connection through that node
  does not count.
- U4 (V) A settlement goes on an empty node that touches the player's own road and obeys the distance rule.
- U5 (V) A city replaces the player's own settlement.

### Trading

- X1 (V) Bank 4:1. A 3:1 port on which the player has a settlement or city gives 3:1 for any resource. A 2:1
  port gives 2:1 for its resource. The best available rate for the given resource is used.
- X2 (V) The bank must hold the requested card.

### Development cards

- D1 (V) Deck of 25: 14 knight, 5 victory point, 2 road building, 2 year of plenty, 2 monopoly.
- D2 (V) A card bought this turn cannot be played this turn. Victory point cards are never played; they
  count as soon as they are held (D7).
- D3 (V) At most one development card is played per turn, before or after the roll.
- D4 (V) Knight: move the robber and steal, as on a 7 but without discards. Counts for Largest Army.
- D5 (V) Road building: place up to two roads free of cost, as many as are legally possible and in stock.
  The card cannot be played by a player who has no road piece left (Colonist patch notes: "Cannot use
  Road Building card when you don't have road pieces"; corrected on 2026-11-12 after an independent review).
  With one piece left the play is legal and ends after one road. With pieces left but no legal edge the
  play stays legal and places nothing (A, field `road_building_needs_legal_edge`, default false).
- D6 (V) Year of plenty: take two resource cards of choice from the bank, limited by what the bank holds.
  Monopoly: name one resource; the opponent hands over all cards of it.
- D7 (V/A) Victory point cards are hidden from the opponent and count towards winning for their holder.
  The buyer wins at once on their own turn when the total reaches the target.

### Robber

- R1 (V) The robber must move to a different hex.
- R2 (V) Friendly robber, field `friendly_robber` (default true): the robber may not be placed on a hex
  that touches a settlement or city of a player with 2 or fewer visible points. Visible points are
  settlements, cities, Largest Army and Longest Road; hidden victory point cards do not count.
  The same limit applies to a knight.
- R3 (V) Steal: if the opponent has a settlement or city on the robber's new hex and holds cards, the mover
  takes one card at random, each card equally likely.
- R4 A hex with the robber produces nothing.

### Awards and winning

- A1 (V) Largest Army: 2 points to the first player with 3 played knights; it passes only to a player with
  strictly more played knights.
- A2 (V) Longest Road: 2 points to the first player with a continuous road of 5 or more segments; it
  passes only to a player with a strictly longer road. An opponent settlement or city on a node cuts the
  road at that node. A road is a trail: no edge twice, nodes may repeat.
- A3 (A) When a cut shortens the holder's road, field `longest_road_break_rule`: recompute both lengths;
  if the holder is still at least as long as the opponent and has 5 or more, the holder keeps the award;
  if the opponent is strictly longer and has 5 or more, the opponent takes it; otherwise nobody holds it
  until someone has strictly the longest road of 5 or more.
- A4 (V) A player wins on their own turn on reaching `victory_points` (default 15): settlements 1, cities 2,
  each award 2, each victory point card 1. The check runs after every action of the player to move.

### Dice

- C1 (V) Field `dice`: `colonist_balanced` (default) or `independent` (two fair dice).
- C2 (V) `colonist_balanced` reproduces Colonist's published dice code exactly for two players: a deck
  of the 36 pairs; reshuffle when fewer than 13 cards are left; weight of each total = its cards left
  divided by cards left in the deck; times (1 - 0.34 x its count among the last 5 rolls), floored at 0;
  the weight of 7 times an adjustment clamped to [0, 2] made of the seven-imbalance term plus the streak
  term with constant 0.4, both relative to the roller. Read the source for the exact order of operations,
  including when the recent-roll window and the seven counters update and the first-roll behaviour.
- C3 Only the dice total matters to the game; the engine draws a total, and the deck loses one card of
  that total. The dice state (cards left per total, the last five totals, seven streak owner and length,
  sevens per player) is part of the game state and is public.

### Information

- I1 The engine state is omniscient.
- I2 (A) What a player observes: everything except the identities of the opponent's unplayed development
  cards and the order of the development deck. Resource hands are common knowledge in 1v1: production,
  bank trades, builds, monopoly and year of plenty are public, and a steal is known to both players.
  Field `discards_public` (default true).
- I3 The engine gives, for a player, the observation and a function that samples a full state consistent
  with that observation (redistributing the unseen development cards between the opponent's hand and the
  deck with correct probabilities).

## Engine requirements

- E1 The game state is one fixed-size value with no heap memory. Report its size in bytes.
- E2 No hidden randomness. A state is either a decision node (one player to move, a list of legal actions)
  or a chance node (a list of outcomes with exact probabilities). `apply` is a pure function of state and
  the chosen action or outcome. Chance nodes: dice total, stolen card, drawn development card. A
  convenience layer samples outcomes from a caller-supplied seeded generator.
- E3 Legal actions are written into a caller-supplied fixed buffer, with no allocation. Each action is a
  small integer from one global, stable table (see Encoding), so that it can index a network output.
- E4 Multi-part choices are sequences of small actions, never combinatorial ones: discards one card at a
  time, year of plenty one card at a time, road building one road at a time, a trade as (give, take).
- E5 Search support: either undo for every action, or copy-and-apply if the measured cost of a copy is
  lower. Measure both and keep the faster; record the numbers.
- E6 Everything a search or a network needs often is kept incrementally or is cheap: legal-action masks,
  visible and total points, road lengths, production per roll, port access, pieces left.
- E7 A canonical byte serialisation of the state and a 64-bit hash of it (FNV-1a over the canonical bytes),
  identical in both implementations. Internal fast hashes are free.
- E8 Tools: `gen` plays random games and writes traces; `replay` reads a trace and checks every state hash;
  `bench` reports, single-threaded: random games per second, decisions per second, mean decisions and
  turns per game, share of games with a winner; and the same from mid-game states for apply and for
  legal-action generation alone.
- E9 A plain C ABI or a Python binding is wanted for the Rust build, so that training code can call it.
- E10 Dead positions (added on 2026-11-11, from Rust seed 191: both players on 14 points, all roads and
  cities placed, no settlement site left, development deck empty, all knights played). The printed rules
  have no draw, and the rules do not change: `apply` and the terminal test stay win-only. The engine gives
  a separate, cheap predicate `dead(state)`. It is sound: it is true only when no sequence of legal actions
  and chance outcomes leads to a win for either player. It is closed: every successor of a dead state is
  dead. It does not have to be complete. The minimum is the frozen case (no player's points can change and
  both are below the target); a wider form with upper bounds on reachable points (seed 191 at step 1522:
  13 and 13 points, bounds 14 and 14) is allowed when its soundness is argued in writing. Its exact conditions are written in `ENCODING.md` by the Rust
  build and adopted unchanged by the Bend build. Runners (`gen`, `bench`, self-play) stop a game on
  `dead` and record the trace result `unfinished` with reason `dead`. No step cap or turn cap exists in
  the engine or in a runner.

## Encoding (owned by the Rust build as its first milestone)

The Rust build writes `ENCODING.md` in this folder before it writes engine code, and the Bend build
adopts it unchanged: the global action table, the chance-outcome numbering, the canonical serialisation
byte by byte, and the trace format (one JSON object per line: the board, the configuration, then each step
as action or outcome id plus the state hash after it, then the result). After the first commit of
`ENCODING.md` a change needs a note in both builds' `QUESTIONS.md`.

## Correctness evidence required

1. Unit tests per rule id above (B1 to I3), each naming its rule.
2. Property tests over random games: resources are conserved (hands plus bank = 19 per resource),
   development cards are conserved (25), piece counts within limits, points equal their recomputation from
   scratch, incremental data equal their recomputation from scratch, undo restores the exact prior state,
   legal actions are exactly the actions `apply` accepts, chance probabilities sum to 1, every random game ends in a win or in a dead position (E10), and from a
   state where `dead` is true no action or outcome gives a win or makes `dead` false.
3. Dice: the model's roll distribution matches a direct port of the Colonist source on long seeded runs,
   and the exact probabilities at chance nodes match that port state by state.
4. Cross-implementation: traces from each build replay with identical hashes in the other.
5. Bend: the properties in item 2 that are provable are stated as laws in `LAWS.bend` and checked by
   `bend PROOF.bend`.

## Speed

There is no numeric target, because nothing comparable has been measured on our hardware. The only outside
figure is CatanAPI's README: 250 to 300 random games per second on one thread for a similar 1v1 variant.
Treat that as the floor to leave far behind. Method: measure, profile, change one thing, re-run the full
correctness evidence, keep the change only if it is faster and still correct, and record it
with before and after numbers, including ideas that did not help.
