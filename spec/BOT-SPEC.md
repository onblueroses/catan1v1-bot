# Catan 1v1 bot: behaviour specification

Version 1, 2026-11-17. This document is the only description of the bot that the implementer gets. It describes
what the bot computes, in which order, with which constants, and with which floating-point steps. It is written
against our rules engine `catan_engine` (`engine/`, commit `fe9a482`) and our two networks in `nets/`. The test data
in `tests/fixtures/` (see its `README.md`) was produced by the current bot. A correct implementation reproduces every
fixture bit for bit.

Conventions used below:

- **f32** and **f64** are IEEE-754 single and double precision with round-to-nearest-even. Every arithmetic step
  named f32 rounds to f32 after each single operation. No fused multiply-add anywhere (compile with FMA contraction
  off). The order of operations is as written, left to right, unless brackets say otherwise.
- `ln`, `exp`, `ln1p` and `pow` on f64, and `lnf`, `expf` and `powf` on f32, are the functions of the GNU C library
  (glibc). Rust's `f64::ln`, `f64::exp`, `f64::ln_1p`, `f32::ln`, `f32::exp` and `f32::powf` call exactly these on
  Linux. Other implementations differ by one unit in the last place in some cases (we measured NumPy's `exp` on
  f32: 31 of 256 network values were off by one ulp). Use glibc.
- `round(x)` rounds half away from zero. `trunc(x)` rounds toward zero.
- "Ascending" lists of nodes, edges and tiles always use the **bot numbering** of section 2.3, never the engine ids.
- A "stable sort" keeps equal elements in their input order.
- Players are 0 and 1. The **viewer** is the player for whom a value is computed. The **rival** is the other player.
- Resources in engine order: 0 lumber, 1 brick, 2 wool, 3 grain, 4 ore. Development cards: 0 Knight, 1 Victory
  Point, 2 Road Building, 3 Year of Plenty, 4 Monopoly. Costs: road (1,1,0,0,0), settlement (1,1,1,1,0), city
  (0,0,0,2,3), development card (0,0,1,1,1). "Cost index" 0 road, 1 settlement, 2 city, 3 card.
- `ways(n)` for a number token n in 2..12 is the number of dice pairs with that total: 1,2,3,4,5,6,5,4,3,2,1
  (`catan_engine::dice::INITIAL_COUNTS[n-2]`).

## 1. Overview

The bot plays two-player Catan with Colonist 1v1 rules (15 points, discard above 9 cards, friendly robber, balanced
dice). For each decision it receives a position and returns one action. It keeps no memory between decisions.

The decision procedure depends on the phase:

| Situation | Procedure | Section |
|---|---|---|
| Exactly one legal action | Return it at once | 7.5 |
| Setup settlement or setup road | Two-player alpha-beta search to the end of setup, with a goal bonus at its leaves | 5 |
| Own main phase | Whole-turn plan search (beam 32, up to 6 own actions), then a reply check on the 4 best distinct plans, then 128 paired rollouts that pick among those plans | 6, 7, 8 |
| Before the roll, with a playable card | One-step look-ahead through the roll | 7.4 |
| Robber after a 7 or a Knight in the main phase | One-step look-ahead, then a full plan search for the 4 best tiles | 7.5 |
| Discard, free road, robber before the roll | One- or two-step look-ahead | 7.4 |

Every value is "the probability that the viewer wins", a number in [0, 1]. Values come from a neural network on 351
input bytes (section 3), plus two hand-written terms in log-odds space (section 4.4).

The bot uses two networks, both 351-320-32-1 multilayer perceptrons with 122,945 weights:

| File | Role | SHA-256 |
|---|---|---|
| `nets/a320c1l.bin` | Main network: every position after setup, and half of the setup average | `36a17039ddb9acaad5c1d07b1f55b685c4238308c9b85bde267650b3be2942c6` |
| `nets/s320b.bin` | Opening network: the other half of the average for setup positions | `eb20391cafd24fbfa37a3fd98e0fda0806fbb925ce19548f46d7d611255a4984` |

**The VINCO settings** are the configuration to reproduce. All constants:

| Name | Value | Meaning |
|---|---|---|
| beam width | 32 | Partial plans kept per ply in the main-phase plan search |
| plan depth | 6 | Most own actions in one plan |
| opening width | 6 | Settlement spots searched at each setup settlement node |
| goal reach | 2 | Roads a settlement goal may need |
| patience | 0.75 (f32) | Discount per turn of waiting in the goal bonus |
| economy weight | 0.25 (f32) | Weight of the production term (section 4.4) |
| card weight | 0.1 (f32, that is `0.1f32`) | Weight of the development-card term |
| robber rechecks | 4 | Robber tiles that get a full plan search |
| reply finalists | 4 | Plans that get the reply check |
| rollout samples | 128 | Paired rollout samples per decision |
| rollout workers | 4 | Threads. Results do not depend on this number (section 8.7) |
| one-step depth | 2 | Look-ahead depth of the simple decisions (section 7.3) |
| pre-roll rule | on | Section 7.3 |

The rollout stage runs **one** round of 128 samples. (The VINCO report's "4 rounds" refers to the 4 worker
threads; there is no multi-round elimination in this bot.)

Two reduced settings are used by the fixtures and are useful during development: **reply** = VINCO with 0 rollout
samples, and **beam** = VINCO with 0 reply finalists and 0 rollout samples. The fixtures also use **r16** = VINCO with
16 samples.

## 2. Engine settings, positions and actions

### 2.1 Engine configuration

| `catan_engine::Config` field | Parity tests (fixtures, reference games) | Colonist play |
|---|---|---|
| `dice` | 0 (balanced) | 0 |
| `victory_points` | 15 | 15 |
| `discard_limit` | 9 | 9 |
| `friendly_robber` | true | true |
| `first_player` | 0 | the real first player |
| `shortage_rule` | **1** (a bank shortage pays nobody that resource) | open: Colonist's rule is unverified. Use 1 until the rule is confirmed. |
| `reference_dice_rounding` | **true** (seven factor in truncated hundredths) | false (matches the Colonist source) |
| board port positions | **reference port rings** (section 2.5), set with `Board::set_port_nodes` | the real board's ports, set the same way |
| `road_building_needs_legal_edge` | false | false |
| other fields | engine defaults | engine defaults |

These settings change the game, not the bot. The bot's search uses the engine's chance weights, so with
`reference_dice_rounding = false` its dice odds are the Colonist odds. Two parts of the bot are fixed and do not
follow the configuration: the next-roll inputs (section 3.2, inputs 15-25) always use
`DiceState::reference_probabilities`, and the step limit (2.4) is part of the position.

A position whose port positions differ from the engine default, or which uses reference rounding, cannot be written
as standard `C1V1` canonical bytes. Store and load engine states with `State::compatibility_bytes` (323 bytes,
magic `C1PC`) and `State::from_compatibility_bytes`.

### 2.2 The decision layer over the engine

The bot does not act on raw engine actions. It acts on **joint actions** (2.3) and keeps a few public facts that
the normalized engine state does not hold. A **bot position** is:

- the engine `State`;
- `phase`: the bot phase, 0..10 (below);
- `mover`: the player who acts next, or the turn owner at chance and at the end;
- `before_roll`: true from the start of a turn until its dice outcome;
- `setup_placed`: setup settlements placed so far, 0..4;
- `free_roads_left`: free roads still owed by a Road Building card, 0..2;
- `last_total`: the last dice total rolled in the game (0 before the first roll);
- `step`: number of joint actions applied since the game began (chance outcomes do not count);
- `discard_active` and `discard_slots`: a queue of two slots `(player, cards still owed)`; `discard_active` is the
  number of live slots. Slots that are no longer live keep their old bytes (they enter the sample seed, 8.1);
- `dice_counts`: the bot's copy of the 11 dice-deck counts (below);
- `port_ring`: 0 or 1, which reference port ring the board uses (0 for boards from elsewhere);
- `step_limit`: the step cap (2.4).

**Bot phase.** `phase` equals the engine phase (0 setup settlement, 1 setup road, 2 before roll, 3 dice, 4 discard,
5 robber, 6 steal, 7 main, 8 development draw, 9 road building), with two exceptions: it is 10 when the engine state
is terminal, and it is 9 while Road Building roads are still owed in the bot's bookkeeping even if the engine has
already left its road-building phase ("pending roads", below). Engine phases 10, 11 and 12 (resource choices and the
second half of a trade) never appear, because joint actions complete them.

**`mover`** = `State::actor()`, or `turn_owner` when `actor()` is `None`.

**Chance.** A position is a chance position when `State::is_chance()`. Its outcomes are `State::chance_outcomes` in
engine order, with bot outcome ids: dice outcome id 0..10 for totals 2..12; steal outcome id = engine id - 11 (the
resource); draw outcome id = engine id - 16 (the card kind). Weights are the engine's exact integer weights.

**The game is over** ("done") when the engine state is terminal, or when `step >= step_limit` at a non-chance
position. A done position has no legal actions and no outcomes.

**Applying a joint action** `a` to position P (not done, not chance) gives P':

1. If P.phase = 9 and `a` = 181: this is the protocol finish of a Road Building card. It is legal only while the
   engine is not in its road-building phase. The engine state is unchanged. Set `free_roads_left` = 0.
   Otherwise apply the engine action sequence of `a` (2.3). If `a` = 184 (play Road Building) set
   `free_roads_left` = 2. If P.phase = 9 and `a` is a road (54..125): decrease `free_roads_left` by 1, and set it to
   0 if the turn owner has no road piece left (`cache.pieces_left[owner][0] == 0`).
2. If P.phase = 0 and `a` < 54: `setup_placed` += 1.
3. If P.phase = 4: decrease the owed count of slot 0 by 1. When it reaches 0, copy slot 1 into slot 0 and decrease
   `discard_active` by 1 (slot 1 keeps its bytes).
4. `step` += 1.
5. `before_roll`: false if the new engine phase is 0 or 1; else true if the old engine phase was 0 or 1, or if P.phase
   = 7 and `a` = 181; else unchanged.
6. **Pending roads**: true when the new engine state is not terminal, `free_roads_left` > 0, and (P.phase = 9 or
   `a` = 184).
7. **Forced roll**: if pending roads is false, the new engine phase is "before roll" (2) and the engine's
   `development_played` is true, apply the engine action ROLL (0) as well. (A card played before the roll leaves
   rolling as the only choice. The bot does not offer that choice; it rolls at once. The forced roll does not count
   as a step.)
8. `phase`: 10 if terminal, 9 if pending roads, else the new engine phase. `mover` as above.

**Applying a chance outcome** `o` to P (chance, not done): apply engine outcome `o` (+11 at a steal, +16 at a draw).
If P.phase = 3 (dice): set `dice_counts` to the new engine `dice.counts`; if the total is 7, rebuild the discard
queue: `discard_active` = 0, then for each player in the order (turn owner, other), if that player held more than
`discard_limit` cards **before** the roll, write `(player, cards / 2)` into the next slot and increase
`discard_active` (slots that are not written keep their old bytes); set `last_total` = `o` + 2 and
`before_roll` = false. Then apply the forced roll of step 7 above if the engine is in phase 2 with
`development_played` true (this happens after the steal that follows a Knight played before the roll). Set `phase`
and `mover` as in step 8.

**Why `dice_counts` is separate.** The engine reshuffles a deck with fewer than 13 cards when the ROLL action is
applied. The bot's copy changes only at the dice outcome. In phase 3 the two can differ. The inputs and the sample
seed use the bot's copy.

**Initial position.** Engine `State::new(board, config)`, `phase` 0, `mover` = actor, `before_roll` false, all
counters 0, `discard_slots` all zero, `dice_counts` = (1,2,3,4,5,6,5,4,3,2,1), `last_total` 0.

### 2.3 Joint actions and the bot numbering

The bot numbers nodes, edges and tiles differently from the engine. All tie-breaks and all floating-point sums
follow the bot numbering, so it is part of the behaviour. The tables give the engine id for each bot index.

Tiles, bot index 0..18 → engine hex:

    0, 1, 2, 6, 11, 15, 18, 17, 16, 12, 7, 3, 4, 5, 10, 14, 13, 8, 9

Nodes, bot index 0..53 → engine node:

    0, 4, 8, 12, 7, 3, 1, 5, 9, 13, 2, 6, 10, 14, 15, 20, 25, 19, 26, 32, 37, 31, 42, 46, 41, 36,
    50, 53, 49, 45, 40, 52, 48, 44, 39, 51, 47, 43, 28, 34, 38, 33, 16, 22, 27, 21, 17, 11, 18, 23,
    24, 30, 35, 29

Edges, bot index 0..71 → engine edge:

    1, 7, 12, 11, 6, 0, 3, 8, 14, 13, 2, 5, 9, 16, 15, 4, 17, 22, 31, 30, 21, 32, 38, 48, 47, 37,
    53, 61, 60, 52, 46, 65, 71, 70, 64, 59, 58, 69, 68, 63, 57, 56, 67, 66, 62, 55, 41, 50, 54, 49,
    40, 24, 34, 39, 33, 23, 19, 25, 18, 10, 20, 27, 26, 29, 28, 45, 36, 51, 44, 43, 42, 35

Derived geometry, all in bot numbering: the 6 nodes of each tile (from `topology::HEX_NODES`), the two nodes of each
edge (from `topology::EDGE_NODES`), for each node its incident edges in ascending edge order, its tiles in ascending
tile order, and its neighbour nodes. Node sets are 54-bit masks (bit n = bot node n), road sets 72-bit masks. "For
each node of a mask" means ascending node index.

**Joint actions**, ids 0..248. The legal joint actions of a position are listed in ascending id order. A joint action
is legal when its whole engine sequence is legal from the current engine state (plus the extra checks named
below). A done or chance position has none. At phase 9 while the engine is no longer in its road-building phase,
the only legal joint action is 181.

| Joint id | Meaning | Engine action sequence |
|---|---|---|
| 0..53 | settlement on bot node n = id | 79 + NODE[n] |
| 54..125 | road on bot edge e = id - 54 | 7 + EDGE[e] |
| 126..179 | city on bot node n = id - 126 | 133 + NODE[n] |
| 180 | roll | 0 |
| 181 | end turn (or finish Road Building, 2.2) | 1 (none in the finish case) |
| 182 | buy development card | 2 |
| 183 | play Knight | 3 |
| 184 | play Road Building | 4 |
| 185..199 | play Year of Plenty for the pair (x, y) in row id - 185 of: (0,0) (0,1) (0,2) (0,3) (0,4) (1,1) (1,2) (1,3) (1,4) (2,2) (2,3) (2,4) (3,3) (3,4) (4,4). Extra check: the bank holds at least 2 of x if x = y, else at least 1 of x, and at least 1 of y | 5, 211 + x, 211 + y |
| 200..204 | play Monopoly on resource id - 200 | 6, 211 + r |
| 205..223 | move the robber to bot tile t = id - 205 | 187 + TILE[t] |
| 224..228 | discard one card of resource id - 224 | 206 + r |
| 229..248 | bank trade: give g = (id - 229) div 4; j = (id - 229) mod 4; take j if j < g, else j + 1 | 216 + g, 221 + take |

The trade id of (give g, take t) is 229 + 4g + t - (1 if t > g else 0).

### 2.4 Step limit

Parity games use `step_limit` 10,000 (no fixture game reaches it). Colonist play uses 4,294,967,295 (no limit).

### 2.5 Reference board generation (parity games only)

`bot arena` (section 10) generates boards the way the reference games did, from an RNG `L` (section 8.2):

1. Resources in bot tile order start as (0,0,0,0, 1,1,1, 2,2,2,2, 3,3,3,3, 4,4,4, desert). For i from 18 down to
   1: swap entries i and `L.below(i+1)`.
2. Port kinds start as (0,1,2,3,4, generic, generic, generic, generic). For i from 8 down to 1: swap entries i and
   `L.below(i+1)`.
3. Number tokens (5,2,6,3,8,10,9,12,11,4,8,10,9,4,5,6,3,11) go to the non-desert tiles in bot tile order. The desert
   gets number 0.
4. `port_ring` = 1 if `L.below(2)` is not 0, else 0. Port slot i gets port kind i and the node pair below, in bot
   node ids (convert with the node table, then call `Board::set_port_nodes`):

| Slot | Ring 0 | Ring 1 |
|---|---|---|
| 0 | 18, 19 | 1, 6 |
| 1 | 22, 23 | 10, 11 |
| 2 | 28, 31 | 14, 15 |
| 3 | 35, 36 | 20, 22 |
| 4 | 40, 41 | 26, 27 |
| 5 | 42, 47 | 31, 32 |
| 6 | 5, 0 | 37, 40 |
| 7 | 6, 7 | 44, 45 |
| 8 | 12, 14 | 47, 4 |

Engine board: `terrain[TILE[i]]` = resource (desert = 5), `number[TILE[i]]` = number, `port_type[i]` = port kind
(generic = 5).

## 3. The 351 inputs

### 3.1 The viewpoint

Inputs are always computed for a **viewpoint** (P, v): a bot position seen by viewer v. The viewpoint is made from
a position P by hiding the rival's development cards:

- H = the rival's development cards (playable + new), N = the rival's new cards (bought this turn), D = the
  number of cards in the engine development deck (the true deck).
- In a copy of the engine state: add the rival's cards of each kind to `development_deck`, set the rival's
  `playable_development` and `new_development` to zero, and call `recompute_caches()`. The copy's
  `development_deck` is now the **unseen pool**: the deck plus the rival's hand, by kind.

All protocol fields are copied unchanged. The bot never reads the rival's card kinds, only H and N. The viewer's own
cards are exact.

### 3.2 Encoding

Each input is a number x ≥ 0 written as one byte. Two rounding rules are used:

- rule A (inputs 0-222 and 329-350): byte = clamp(round(100·x), 0, 255), computed in f64.
- rule B (inputs 223-328): byte = min(trunc(100·x + 0.5), 255), computed in f64.

Unless stated, every quantity below is computed in f64, in the order written. "/" is f64 division of the two
f64 values. Integer counts are converted to f64 before they are used.

Shared definitions for a viewpoint:

- `me` = v, and the **order** of per-player blocks is (me, rival).
- Player data in bot numbering: settlement mask, city mask, road mask, hand, development cards `dev[k]` (playable +
  new; zero for the rival), `new[k]`, `played[k]` (`spent_development`), trade ratios (`cache.trade_rates`),
  `played_card` (true when this player is the turn owner and `development_played` is true). `built` = settlements
  ∪ cities.
- `robber` = the bot index of the robber tile. `desert` tiles have resource 5.
- `weight(p, t)` = settlements of p on the corners of tile t + 2 × cities of p on them.
- `pips(n)` = sum of `ways(number)` over the tiles of node n with a number.
- `blocked` = all nodes with a building of either player, plus all their neighbours. `open` = all nodes not in
  `blocked`.
- `ends(p)` = nodes touched by a road of p.
- `expand(F, R)` = nodes reached from a node of F along one edge that is not in the road set R.
- `unseen` = sum of the unseen pool. `pool[k]` = the unseen pool.
- `dice_cards` = sum of `dice_counts`.
- **Next-roll odds**: the next roller is the turn owner if `before_roll` or phase = 3, else the other player. Take a
  copy of the engine `dice` with `counts` replaced by `dice_counts`. If `config.dice` = 0, w = its
  `reference_probabilities(next roller).weights`, else w = `INITIAL_COUNTS`. `odds[i]` = w[i] / (Σ w), for totals
  i + 2.
- **Funding time** `fund(hand, income[5], ratio[5], cost[5])`: for t = 0, 1, ..., 10: missing = 0, spare = 0; for each
  resource r: have = hand[r] + (2t)·income[r]; if have < cost[r] then missing += cost[r] - have; else spare += (if
  t = 0: the integer (hand[r] - cost[r]) div ratio[r], as f64; else (have - cost[r]) / ratio[r]). If spare ≥ missing
  return t. If no t qualifies return 99.

### 3.3 Global inputs (0-42), rule A

| Index | Value |
|---|---|
| 0-10 | 1 if `phase` = i, else 0, for i = 0..10 |
| 11 | 1 if `mover` = me |
| 12 | 1 if the engine `turn_owner` = me |
| 13 | 1 if `before_roll` |
| 14 | `setup_placed` / 4 |
| 15-25 | `odds[i]` × 6, totals 2..12 |
| 26 | `dice_cards` / 36 |
| 27-31 | engine `bank[r]` / 19 |
| 32 | D / 25 |
| 33 | min(T, 200) / 200, T = engine `turn_number` - 1 (0 if `turn_number` is 0); the min is taken on the integer |
| 34 | `free_roads_left` / 2 |
| 35 | engine `discard_remaining[me]` / 10 |
| 36 | engine `discard_remaining[rival]` / 10 |
| 37-41 | `pool[k]` / `unseen` (0 if `unseen` = 0) |
| 42 | |open| / 10 |

### 3.4 Player inputs (43-174), rule A

66 inputs per player: offsets 0..65 at 43 + offset for me and 109 + offset for the rival. "This player" is p, "the
other" is q. "mine" means p = me.

| Offset | Value |
|---|---|
| 0 | V / 15, V = engine `cache.visible_points[p]` |
| 1 | (V + (`dev[1]` if mine else 0)) / 15 |
| 2 | (`victory_points` - that point count) / 15 |
| 3 | settlements / 5 |
| 4 | cities / 4 |
| 5 | roads / 15 |
| 6 | engine `cache.road_lengths[p]` / 15 |
| 7 | 1 if p holds Longest Road |
| 8 | 1 if p holds Largest Army |
| 9 | min(`played[0]`, 6) / 6 |
| 10-14 | min(hand[r], 12) / 6 |
| 15 | (hand total) / 20 |
| 16 | 1 if hand total > `discard_limit` |
| 17-21 | (4 - ratio[r]) / 2 |
| 22-26 | 3 × inc[r] (below) |
| 27 | 3 × rob (below) |
| 28-32 | 3 × nxt[r] (below) |
| 33 | (number of r with inc[r] > 0) / 5 |
| 34 | 1.5 × all, all = inc[0] + inc[1] + inc[2] + inc[3] + inc[4] in that order, from 0 |
| 35-38 | mine: min(`dev[k]` - `new[k]`, 3) / 3 for k = 0, 2, 3, 4; rival: 0 |
| 39 | mine: `dev[1]` / 3; rival: 0 |
| 40 | (mine: Σ `new[k]`; rival: N) / 3 |
| 41 | mine: 0; rival: min(H, 8) / 4 |
| 42-44 | `played[k]` / 2 for k = 2, 3, 4 |
| 45 | 1 if `played_card` |
| 46-49 | 1 if hand[r] ≥ cost[r] for every r, for cost index 0, 1, 2, 3 |
| 50 | min(|S|, 4) / 4, S = ends(p) ∖ blocked |
| 51 | max pips over S / 15 (0 if S is empty) |
| 52 | min(|E|, 10) / 10, E = all edges incident to a node of (built(p) ∪ (ends(p) ∖ built(q))), minus every road of both players |
| 53-56 | two expansion steps, below: for step 1 and step 2: min(|X ∖ blocked|, 6) / 6, then max pips over X ∖ blocked / 15 |
| 57-60 | Σ_r max(0, cost[r] - hand[r]) / 3, for cost index 0, 1, 2, 3 |
| 61 | min(Σ_r (hand[r] div ratio[r]), 4) / 2 (integer division) |
| 62 | min(K, 6) / 3, K = `played[0]` + (mine: `dev[0]`; rival: H × `pool[0]` / `unseen`, evaluated as (H × pool[0]) / unseen, 0 if `unseen` = 0) |
| 63 | clamp(len(p) - len(q) + 5, 0, 10) / 5 (integers; len = `cache.road_lengths`) |
| 64 | 1.5 × rem (below) |
| 65 | max pips over p's settlements / 15 (0 if none) |

Income sums for offsets 22-32 and 64, over tiles t in ascending order with weight(p, t) > 0 and a resource
(not desert); c = ways(number) / 36 computed first:

- if t = robber: rob += weight × c;
- else inc[res] += weight × c and nxt[res] += weight × odds[number - 2];
- rem (offset 64): for t ≠ robber, rem += weight × (if `dice_cards` ≥ 13: `dice_counts[number-2]` / `dice_cards`;
  else ways(number) / 36).

Expansion steps (offsets 53-56): walls = built(q); F = (built(p) ∪ ends(p)) ∖ walls; seen = F. Each step: X =
expand(F, roads of both) ∖ seen; seen = seen ∪ X; F = X ∖ walls.

### 3.5 Build inputs (175-222), rule A

24 per player: 175 + offset for me, 199 + offset for the rival.

**Production table** (depends on the board and the robber only): prod[n][r] = for each tile t in ascending order
that has a resource and is not the robber tile, for each of its 6 nodes n: prod[n][res(t)] += ways(number) / 36.

**Player income** ginc[r] = for each node n of built(p) ascending: ginc[r] += count × prod[n][r], count 1 for a
settlement and 2 for a city. (This is summed in a different order from inc above. Keep both.)

**Profile** of a node set C: base[r] = ln1p(4 × ginc[r]). For each n in C ascending: s = 0; for r = 0..4 with
prod[n][r] ≠ 0: s += wr × (ln1p(4 × (ginc[r] + prod[n][r])) - base[r]), with wr = (1, 1, 1, 1.5, 2). The best node is
the first n with s greater than every earlier s, starting from -1. Output 5 inputs: 3 × prod[best][r], or 0 if C is
empty.

| Offset | Value |
|---|---|
| 0-4 | profile of (p's settlements if p has fewer than 4 cities, else empty) |
| 5-19 | three profiles, steps 0, 1, 2 (below) |
| 20-23 | min(fund(hand, ginc, ratio, cost), 12) / 12 for cost index 0, 1, 2, 3 |

Steps: walls = built(q); F = (built(p) ∪ ends(p)) ∖ walls; seen = F. For step 0, 1, 2: the profile of (F ∖ blocked)
if p has fewer than 5 settlements and roads(p) + step ≤ 15, else of the empty set; then X = expand(F, roads of both)
∖ seen; seen ∪= X; F = X ∖ walls.

### 3.6 Board inputs (223-328), rule B

Global part:

| Index | Value |
|---|---|
| 223-227 | 2 × supply[r], supply[r] = Σ over tiles with a resource (robber tile included), ascending, of ways / 36 |
| 228-231 | the four largest pips among open nodes, descending, / 15 (0 where fewer exist). Insertion rule: keep four slots starting at 0; for each open node ascending, carry its pips through the slots 0..3 and swap whenever the carried value is greater than the slot |
| 232 | min(R, 6) / 6, R = open nodes with pips ≥ 10 |

Per player, 48 inputs: 233 + offset for me, 281 + offset for the rival. Let k be the block (0 me, 1 rival), p its
player, q the other.

Common data: binc[k][r] = Σ over tiles t ascending with a resource and t ≠ robber of weight(p, t) × ways / 36
(evaluated as (weight × ways) / 36). **Road distance** dist(p)[n]: start every node at 9. walls = built(q); F =
(built(p) ∪ ends(p)) ∖ walls; seen = F. For step 0..5: stop if F is empty; set dist = step for every node of F; X =
expand(F, roads of both) ∖ seen; seen ∪= X; F = X ∖ walls. mine = dist(p), theirs = dist(q).

Scan of open nodes n ascending, with d = mine[n], skipping nodes with d > 5:

- if d is 3 or 4: count it in far[d-3] and keep the largest pips in top[d-3];
- if d < theirs[n]: secure count +1, secure pips sum += pips, keep the largest secure pips; else if d = theirs[n]:
  contested count +1, keep the largest contested pips;
- if d ≤ theirs[n] and d ≤ 4: append (n, pips - 2d) to the candidate list;
- if n is a port node: near[kind] = min(near[kind], d), kind 0 for a generic port, 1 + resource for a 2:1 port
  (near starts at 9).

**Greedy plan.** room = 5 - settlements(p), spare = 15 - roads(p), chosen = empty, used = 0, built = 0, gain[5] = 0.
While built < room: among candidates not yet taken, with no neighbour in chosen and used + d ≤ spare, pick the first
with the largest value. If none, stop. Record the first two picks. Mark it taken, add it to chosen, used += d,
built += 1, and for each tile t of the node in ascending order that has a resource and is not the robber:
gain[res] += (fade[d] × ways) / 36, fade = (1, 0.8, 0.64, 0.51, 0.41).

| Offset | Value |
|---|---|
| 0 | min(far[0], 6) / 6 |
| 1 | top[0] / 15 |
| 2 | min(far[1], 6) / 6 |
| 3 | top[1] / 15 |
| 4 | min(secure count, 8) / 8 |
| 5 | min(secure pips sum, 80) / 40 |
| 6 | largest secure pips / 15 |
| 7 | min(contested count, 4) / 4 |
| 8 | largest contested pips / 15 |
| 9 | built / 4 |
| 10 | used / 8 |
| 11-15 | 3 × gain[r] |
| 16-20 | for the first pick n1 with d1 = mine[n1], d2 = theirs[n1], raced = (d2 ≤ 5 and q has fewer than 5 settlements): pips(n1) / 15; d1 / 4; min(fund(hand(p), binc[k], ratio(p), (1+d1, 1+d1, 1, 1, 0)), 12) / 12; raced ? (6 - d2) / 6 : 0; raced ? min(fund(hand(q), binc[1-k], ratio(q), (1+d2, 1+d2, 1, 1, 0)), 12) / 12 : 1. With no first pick: 0, 0, 1, 0, 1 |
| 21 | second pick: pips / 15, else 0 |
| 22 | second pick: mine / 4, else 0 |
| 23 | 3 × top yield (below) |
| 24 | 3 × second yield |
| 25 | shared / all if all > 0, else 0 |
| 26 | 3 × exposed |
| 27-36 | min(bynum[n], 8) / 4 for n = 2, 3, 4, 5, 6, 8, 9, 10, 11, 12 |
| 37-41 | 1.5 × rem2[r] |
| 42 | generic port: 1 if every ratio of p is ≤ 3; else (5 - near[0]) / 6 if near[0] ≤ 4; else 0 |
| 43-47 | 2:1 port of resource r: 1 if ratio[r] = 2; else (5 - near[r+1]) / 6 if near[r+1] ≤ 4; else 0 |

Yield scan for offsets 23-41: for tiles t ascending with a building of p on it and a resource: y = (weight × ways) /
36; all += y; if q has a building on t then shared += y, else exposed = max(exposed, y); if y > top then (second =
top, top = y) else second = max(second, y); then, only if t ≠ robber: bynum[number] += weight and rem2[res] +=
weight × (dice_cards ≥ 13 ? dice_counts[number-2] / dice_cards : ways / 36).

### 3.7 Card inputs (329-350), rule A

11 per player: 329 + offset for me, 340 + offset for the rival. held[k] = mine: `dev[k]`; rival: (H × pool[k]) /
unseen, or 0 if unseen = 0. any(k) = min(held[k], 1).

| Offset | Value |
|---|---|
| 0 | (any(4) × max(hand of q)) / 6 |
| 1 | (any(4) × q's ore) / 6 |
| 2 | (any(4) × q's grain) / 6 |
| 3-5 | for cost index 2, 1, 3 (city, settlement, card) in that order: any(3) if Σ_r max(0, cost[r] - hand[r]) is 1 or 2, else 0 |
| 6 | if p has fewer than 5 settlements: (any(2) × M) / 15, else 0. M = the largest pips over X ∖ blocked of both expansion steps of 3.4 (0 if none) |
| 7 | any(2) if p does not hold Longest Road and len(p) + 2 ≥ 5 and len(p) + 2 > len(q), else 0 |
| 8 | (clamp(`played[0]` + held[0] - q's `played[0]`, -3, 3) + 3) / 6 |
| 9 | any(0) if p does not hold Largest Army and `played[0]` + 1 ≥ 3 and `played[0]` + 1 > q's `played[0]`, else 0 |
| 10 | held[1] / 3 |

## 4. Network file format and evaluation

### 4.1 File format

Little-endian. A file is:

1. the 4 bytes `CATN`;
2. u32 L, the number of layers (L ≥ 1);
3. L + 1 u32 widths w0..wL. w0 must be 175, 223, 329 or 351, and wL must be 1;
4. for each layer l = 1..L: the weights as f32, **output-major** (for each output o, all w(l-1) input weights of o
   in input order), then the w(l) biases as f32.

The file length must equal the header plus 4 bytes per parameter exactly, and every parameter must be finite. Both
our nets have widths 351, 320, 32, 1. A net with w0 < 351 reads the first w0 inputs.

### 4.2 Float evaluation

Given the input bytes x:

1. a[o] = bias1[o] for each first-layer output o. For each input i ascending with x[i] ≠ 0: s = f32(x[i]) × 0.01
   (an f32 product with the f32 constant 0.01); then for every o: a[o] = a[o] + s × W1[i][o] (f32 product, then
   f32 sum).
2. For each further layer l: z[o] = bias_l[o]; for each previous output i ascending with a[i] > 0 (this is the
   ReLU: non-positive activations are skipped): for every o: z[o] = z[o] + a[i] × Wl[i][o] (f32, two roundings).
   Then a = z.
3. The last layer has one output z (no ReLU). The value is 1 / (1 + expf(-z)), all in f32.

### 4.3 Fixed-point evaluation

The first layer can also be evaluated in integers. **The search uses this path for almost every value** (4.4), so it
must be exact.

Preparation, once per net, from the first layer:

1. For each output o: bound_o = f64(f32(|bias[o]| × 100)), then for each input i ascending: bound_o += f64(f32(255
   × |W1[i][o]|)) (each product in f32, the sum in f64). B = max(1, max_o bound_o).
2. scale = 2^floor(log2(2^30 / B)) (f64 `log2`, `floor`, `exp2`). unit = f32(1 / (100 × scale)) (the division in
   f64).
3. Integer weights Wi[i][o] = round(f64(W1[i][o]) × scale). Integer biases bi[o] = round(f64(f32(bias[o] × 100)) ×
   scale). Both are i32. (Check values: a320c1l has B = 32976.066788867116, scale 16384, unit bits `3523d70a`;
   s320b has B = 32111.709552168846, scale 32768, unit bits `34a3d70a`.)

Evaluation: sum[o] = bi[o] + Σ_i x[i] × Wi[i][o] as exact integers (they never overflow i32). Then a[o] =
f32(sum[o]) × unit (conversion rounds to nearest, then an f32 product). Continue with step 2 of 4.2.

Integer sums are exact, so any incremental update scheme (adding (x_new[i] - x_old[i]) × Wi[i][o] for changed
inputs only) gives the same value. Float and fixed values of one input differ in the last few bits; the fixtures
give both.

### 4.4 The search value

The **search value** of a viewpoint (P, v) inside one search is computed as follows. The search has a mode: it is
an **opening search** when its root position has phase 0 or 1, else a **main search**. The root viewpoint gives v, H,
N; inside the search H and N stay fixed at their root values, while D follows the search (4.5).

1. If the engine reports a winner: 1 if the winner is v, else 0. No further steps.
2. x = the 351 inputs of the viewpoint.
3. Raw value p:
   - opening search and phase ≥ 2: the **float** evaluation of a320c1l;
   - opening search and phase 0 or 1: f32 sum: (fixed s320b) + (fixed a320c1l), then divided by 2 (f32);
   - main search: the fixed evaluation of a320c1l.
4. **Economy term** (only if the economy weight e ≠ 0): for each player w, income[r] = Σ over tiles t ascending,
   skipping deserts and the robber tile, of (c × ways) / 36 with c = settlements of w on t + 2 × cities of w on t
   (also when c = 0). P(w) = Σ_r wr × ln1p(4 × income[r]), r = 0..4 in order from 0, wr = (1, 1, 1, 1.5, 2). Then:
   q = clamp(p, 0.00001, 0.99999) (f32 constants); L = f64(lnf(q / (1 - q))) (f32 division and f32 log); p =
   f32(1 / (1 + exp(-L - f64(e) × (P(v) - P(rival))))) in f64.
5. **Card term** (only if the card weight c ≠ 0): q = clamp(p, 0.00001, 0.99999); K = the viewer's development cards in
   P (playable + new); L = lnf(q / (1 - q)) + c × f32(K - H), all in f32; p = f32(1 / (1 + exp(-f64(L)))).
6. The result is p.

Both terms change the value even when their difference is zero, because of the clamp and the round trip through
log-odds. Keep them exactly as written.

The **leaf value** is:

- main search: the search value if the state is terminal, else clamp(search value, 0.000001, 0.999999) (f32
  constants);
- opening search: section 5.3.

### 4.5 Search-wide rules

- **Legal actions in the search** are the legal joint actions, minus 182 (buy a card) when D = 0. D starts at the
  viewpoint's deck count and decreases by 1 in the children of every development-draw chance node (phase 8).
- **Chance averages.** Where a value is the average over the outcomes of a chance node, it is computed as: S = Σ
  f64(f32(w) × v) over the outcomes in engine order (the product in f32: the weight converted to f32, times the f32
  value), T = Σ f64(w), result = f32(S / T). This rule is called **chance mean** below. The rollout control variate
  uses a different product (8.5).
- **Ties.** "Sort by value descending" without further note means: sort the pairs (−value, index) ascending, with the
  index of the action in the legal list as the second key. Explicit stable sorts are named.
- **Memo tables** (caching values by state, by input bytes, or rollout choices by viewpoint) are allowed and change
  nothing, as long as their keys hold the whole state that the value depends on. The reference bot used three, and
  results did not change with the number of worker threads, each of which had its own tables.

## 5. Opening (setup phases)

### 5.1 Root

The root viewpoint is (P, mover). With one legal action, return it (7.5 step 1). Otherwise:

- **Settlement root** (phase 0): compute the search value of every child; reorder the legal actions by that value
  descending (ties by action id). Every action is searched.
- **Road root** (phase 1): if there are more than 6 legal roads, compute the search value of every child; cut = the
  6th largest of these values. Only actions whose child value is ≥ cut are searched; the others get value -1. With 6
  or fewer roads, all are searched.

Then, in that order, with best = -1: for each searched action a, low = best, v = node value of the child (5.2) with
bounds (low, 2); if v ≤ low, the action's value is -1, else v; best = max(best, value). Unsearched actions have -1.
Choose among the actions whose value is ≥ best (7.5). The first action that reaches the best value keeps it,
because later equal values are recorded as -1.

### 5.2 Node value (alpha-beta)

For a state s with bounds (low, high):

1. If phase(s) > 1 (setup is over): return the opening leaf value (5.3).
2. side = +1 if mover(s) is the root viewer, else -1.
3. Rank the legal actions by side × (search value of the child), descending, ties by action id.
4. width = 1 if phase(s) = 1 (a road), else 6.
5. best = -2. For each of the first `width` ranked actions: if side = +1, v = node value of the child with bounds
   (max(low, best), high); else v = node value of the child with bounds (low, min(high, -best)). best = max(best,
   side × v). Stop the loop if side = +1 and best ≥ high, or if side = -1 and -best ≤ low.
6. Return side × best.

### 5.3 Opening leaf value

At a state s after setup (in an opening search): if s is terminal, its search value. Else g = search value of s (4.4,
with the float path of a320c1l); g = g + goal(s, viewer, g); g = g - goal(s, rival, g). The rival's goal receives the
value that already includes the viewer's goal. All f32. No clamp.

### 5.4 Goal bonus

goal(s, w, base) for a player w, with σ = +1 if w is the root viewer, else -1:

1. income[r] = Σ over tiles t ascending with c > 0, a resource and t ≠ robber of (c × ways) / 36 (c as in 4.4 for w).
2. wait = 0 if phase(s) = 7 and mover(s) = w, else 1.
3. best = 0 (f32). A **candidate** is a state b and a cost: turns = max(fund(hand of w in s, income, ratios of w in
   s, cost), wait). If turns > 10, skip it. Otherwise set w's hand in b to hand[r] - cost[r] floored at 0 for each r,
   call `recompute_caches()` on b, then put back the road lengths of s (`cache.road_lengths`). gain = (powf(0.75,
   f32(turns)) × σ) × (search value of b - base), all f32. best = max(best, gain).
4. **City goals** (if w has fewer than 4 cities): w's settlement nodes ascending, then stable-sorted by pips
   descending; for the first two: b = s with that node turned into a city of w; cost (0,0,0,2,3).
5. **Settlement goals** (if w has fewer than 5 settlements): walls = built(other); blocked as in 3.2; F = (built(w) ∪
   ends(w)) ∖ walls; seen = F; taken = all roads; path[n] = empty road set for all n. Spots = the nodes of F ∖
   blocked ascending, each with 0 roads. For step = 1..2: stop if roads(w) + step > 15. X = empty; for each node n of
   F ascending, for each edge e of n ascending, with m the other end of e: skip if e ∈ taken, m ∈ seen or m ∈ X;
   else add m to X and set path[m] = path[n] ∪ {e}. Then seen ∪= X; F = X ∖ walls; append the nodes of X ∖
   blocked ascending, each with step roads. Sort the spots with the GCC sort of 5.5, where spot a goes before spot b
   when pips(a) - 2·roads(a) > pips(b) - 2·roads(b). For the first three spots: b = s with a road of w on every edge of
   path[n] and a settlement of w on n; cost (1 + roads, 1 + roads, 1, 1, 0).
6. Return best.

### 5.5 The GCC sort

The order of equal keys in 5.4 changes which goals are scored. It must equal `std::sort` of libstdc++ (GCC 13) with
the comparison "a before b". For n elements:

1. If n < 2, stop. Let depth = 2 × floor(log2 n).
2. **Partition loop** on the range [first, last): while it has more than 16 elements: if depth = 0, heap-sort the
   range (step 4) and leave the loop; else depth -= 1; take the elements at positions first+1, first + len/2 and
   last-1 and move their median to position first by one swap (the median rule: with a, b, c those three: if a<b
   then (if b<c swap first with b, else if a<c swap with c, else swap with a); else (if a<c swap with a, else if b<c
   swap with c, else swap with b); "x<y" means "x goes before y"). Then partition: i = first+1, j = last; loop:
   advance i while element i goes before the pivot at first; decrease j by one, then decrease j while the pivot goes
   before element j; if i ≥ j stop; else swap elements i and j and advance i. Recurse on [i, last) with the current
   depth, then continue the loop on [first, i).
3. Finally run an insertion sort over the whole array: for each element from the second on, move it left past every
   element that it goes before (strictly). (libstdc++ does this as a guarded pass over the first 16 elements and an
   unguarded pass over the rest; the result is the same.)
4. Heap sort of a range of length m (libstdc++ `make_heap` then `sort_heap`, with the "hole" sift-down): to sift a
   value v into hole h of a heap of length m: top = h; child = h; while child < (m-1)/2: child = 2(child+1); if
   element child goes before element child-1, child -= 1; move element child into the hole; hole = child. If m is
   even and child = (m-2)/2: child = 2(child+1) - 1; move element child into the hole; hole = child. Then push up:
   while hole > top and element parent=(hole-1)/2 goes before v: move the parent into the hole; hole = parent. Put v
   into the hole. make_heap sifts each parent from (m-2)/2 down to 0 with its own value. sort_heap: for end from m-1
   down to 1: v = element end; element end = element 0; sift v into hole 0 of the heap of length end.

`tests/fixtures/sort-orders.txt` has 275 key lists with the permutation produced by GCC 13.3.

## 6. Whole-turn search (own main phase)

### 6.1 Plan values

**Turn-end value** E(s) of a state s (viewer v, the root viewer):

1. If s is done, or mover(s) ≠ v: the leaf value of s.
2. If s is a chance position: chance mean of E over the outcomes (D - 1 in the children of a draw).
3. If phase(s) = 7: the leaf value of s after joint action 181 (end turn).
4. If phase(s) is 5 or 9: rank the legal actions by the search value of the child, descending; take all of them,
   except only the first 3 when phase(s) = 9 and `free_roads_left` = 2; return max(0, max of E(child)).
5. Otherwise the leaf value of s.

### 6.2 Beam

Inputs: the root viewpoint and the root's legal actions R (in the search's legal order, index i). Output: one value
per root action, initial -2, and for each root action an **endpoint** state (initially none).

Lines are (state, first root index, rank). The frontier starts as the root alone. For ply = 0..5 (6 plies), while
the frontier is not empty:

1. Candidates = empty. For each frontier line in order, for each action a in order (at ply 0: R, with first = the
   index of a; later: the legal actions of the line's state, keeping the line's first):
   - next = state after a.
   - If next has a winner and it is v: set value[first] = 1 and **return at once** (all other values stay -2; no
     reply, no rollouts).
   - If next is done, or a chance position, or mover(next) ≠ v: x = E(next); if x > value[first], set value[first] = x
     and endpoint[first] = next.
   - Else append the candidate (next, first, rank) with rank = search value of (next after 181 if phase(next) = 7,
     else next). The rank is not clamped.
2. Stable-sort the candidates by rank descending.
3. New frontier: go through the candidates in order and keep a candidate unless it is **the same partial plan** as a
   line already kept; stop at 32 kept lines.

After the last ply, for each line of the frontier: x = E(state); if x > value[first], update value and endpoint as
above.

**Same partial plan** means equal in all of these: engine `node_occupancy`, `edge_occupancy`, `resources`, `bank`,
`playable_development`, `new_development`, `spent_development`, `development_played`, `discard_remaining`,
`largest_army_holder`, `longest_road_holder`, `cache.road_lengths`, `cache.trade_rates`, `robber_hex`,
`turn_owner`; and the bot fields `phase`, `mover`, `before_roll`, `step`, `free_roads_left`, `discard_active`,
`discard_slots`.

With reply finalists = 0 and rollout samples = 0 (setting **beam**), these values are the root values.

## 7. Reply check

### 7.1 Finalists for the reply

If reply finalists > 0: list the root actions that have an endpoint, by value descending (ties by index). If rollout
samples > 0, remove every entry whose **end key** equals the end key of an earlier entry; the end key is the 181-byte
key (8.1) of the endpoint, after joint action 181 if the endpoint has phase 7 and is not done, with the root D. Set
every value to -2. For the first 4 entries: value = W(endpoint) (7.2).

(With rollouts off, equal endpoints are not merged. This is deliberate: it is the reference behaviour.)

### 7.2 Value with the rival's reply

**W(s)**, finishing the viewer's turn:

1. If s is done or mover(s) ≠ v: return Q(s).
2. Chance: chance mean of W.
3. Phase 7: Q(s after 181).
4. Phase 5 or 9: rank the legal actions by E(child) descending (ties by index); return W(first child). (If there is
   no legal action, fall to step 5.)
5. Else the leaf value of s.

**Q(s)**, the rival's public reply:

1. If s is done, or (phase 2 and mover = v): the leaf value of s.
2. Phase 2 (the rival before its roll): Q(s after 180). The rival's cards are never played.
3. Chance: chance mean of Q (D - 1 in the children of a draw).
4. Phase 4 or 5: side = +1 if mover(s) = v, else -1. For each legal action in order: y = the search value of the
   child, or the chance mean of the search values of its outcomes if the child is a chance position; keep the action
   with the largest side × y (the first one on ties; start from -2). Return Q(s after that action), or the leaf value
   of s if there was no action.
5. If phase ≠ 7 or mover = v: the leaf value of s.
6. If the rival has a **public winning purchase** (7.3): return 0.
7. Rival beam: frontier = [s]; best = 1 (f32). Repeat 4 times while the frontier is not empty: candidates = empty;
   for each frontier state, for each legal action a with a < 180, a = 181 or a ≥ 229 (builds, end turn, bank
   trades; no card purchase or play): next = state after a.
   - If next is done: best = min(best, leaf value of next); if best = 0, return 0.
   - Else if mover(next) = v: best = min(best, leaf value of next).
   - Else: rank = search value of (next after 181); best = min(best, rank); add (next, rank).
   Stable-sort the candidates by rank ascending; the new frontier is the first 2 candidates that are not the same
   partial plan (6.2) as one already kept.
   Return clamp(best, 0, 1).

### 7.3 Public winning purchase

At the rival's main phase s (mover u, points V = `cache.visible_points[u]`, target T): if V + 3 < T, false. For cost
index k = 0 (road), 1 (settlement), 2 (city): b = funded(s, cost k); if it exists, for each legal action a of b with a
< 180 whose kind matches k (a < 54 is a settlement, a < 126 a road, else a city): if b after a has winner u, true.
Then, if V + 2 ≥ T and u has at most 2 cities and at least 2 settlements: b = funded(s, (0,0,0,4,6)); if it exists,
repeat twice: take the first legal action in 126..179 (stop if none), apply it, and return true if u has won. Else
false.

**funded(s, cost)**: with u = mover(s): if for some r the bank holds fewer than max(0, cost[r] - hand[r]), none. For
each resource t = 0..4 in order, while hand[t] < cost[t]: g = the first resource r with hand[r] - cost[r] ≥
ratio[r] (integers); if none, return none; if s is done, return none; apply the trade (give g, take t). Return none if
s is done, else s.

### 7.4 Plain one-step decisions

Two helpers, with the root viewer v:

- **after(s, depth)**: if s is terminal or not a chance position: node(s, depth). Else the chance mean of node(child,
  depth) (D - 1 in the children of a draw).
- **node(s, depth)**: the leaf value of s if s is terminal, depth = 0, s is a chance position, or mover(s) ≠ v. Else
  max(0, max over legal actions a of after(s after a, depth - 1)). (A position at the step limit has no legal
  action and gets 0 here.)

**Before-the-roll rule** (pre-roll rule on, used at the root only):

- **pre-step(s)**: best = -2; for each legal action a in order: y = after(s after a, 1); keep a if y > best. Returns
  the action and best.
- **finish-before-roll F(s)**: if s is done, mover(s) ≠ v or not `before_roll`: the leaf value of s. Phase 3: after(s,
  1). Phase 6: chance mean of F. Phase 5 or 9: F(s after the action of pre-step(s)). Else the leaf value of s.

### 7.5 Root procedure for all phases after setup

1. Legal actions (4.5). If there is exactly one, return it with value -1. The RNG is not used.
2. If the pre-roll rule is on, `before_roll` is true and phase is 5 or 9: return the action of pre-step(root) with its
   value. The RNG is used once: `below(1)`.
3. Phase 7: values = the result of the plan search (6.2), the reply check (7.1) and the rollouts (8); best = the
   largest value (from -2).
4. Other phases: best = -1; for each action in order: value = F(child) if the pre-roll rule is on and phase is 2,
   else after(child, one-step depth - 1) = after(child, 1); best = max(best, value).
5. **Robber recheck**: if robber rechecks > 0, phase = 5, not `before_roll` and the turn owner is v: rank the actions
   by value descending (ties by index). New values are all -1, best = -1. For each of the first 4: c = root after
   the action; if c is a chance position, value = chance mean over its outcomes of plan(outcome state); else value =
   f32(plan(c)) (the same mean over one term). best = max(best, value). **plan(s)**: the leaf value of s if s is done,
   phase ≠ 7 or mover ≠ v; else a fresh main search of the viewpoint (s; same v, H, N and the root D) with the same
   settings but no robber recheck, no reply and no rollouts, run on all legal actions of s (6.2); the maximum of its
   values (from -2).
6. **Choice**: the candidates are the actions whose value is ≥ best. Pick candidate number `rng.below(count)`. Return
   it with the value best.

Section 5 replaces steps 3-5 at setup.

## 8. Paired rollouts

Run after the reply check when rollout samples > 0, in a main search whose root is the viewer's own main phase.

### 8.1 State key and sample seed

The **181-byte key** of a viewpoint state s with deck count D:

| Bytes | Content |
|---|---|
| 0-57 | player 0: hand (5), dev = playable + new (5), new (5), spent (5), trade ratios (5), `played_card` (1), settlement mask u64 LE (8), city mask u64 LE (8), road mask u128 LE (16); masks in bot numbering |
| 58-115 | player 1, the same |
| 116-120 | bank |
| 121-125 | engine `development_deck` (in a viewpoint: the unseen pool) |
| 126-128 | `mover`, `turn_owner`, `phase` |
| 129-132 | engine `turn_number` - 1 (0 if 0), u32 LE |
| 133-136 | `step`, u32 LE |
| 137-140 | `setup_placed`, robber bot tile, `free_roads_left`, `last_total` |
| 141-142 | `cache.road_lengths` |
| 143 | bot node of `setup_anchor` if phase = 1, else 255 |
| 144-146 | `longest_road_holder`, `largest_army_holder`, `winner` (engine values, 255 for none) |
| 147-148 | `before_roll` (0/1), `discard_active` |
| 149-152 | `discard_slots` (player, owed, player, owed), live or not |
| 153-163 | `dice_counts` |
| 164-168 | engine `dice.recent` |
| 169 | engine `dice.recent_len` |
| 170-173 | engine `dice.sevens[0]`, `dice.sevens[1]`, each truncated to u16 LE |
| 174 | engine `dice.streak_owner` |
| 175-176 | engine `dice.streak_length` truncated to u16 LE |
| 177-180 | D as u32 LE |

**Root seed** of the root viewpoint: h = 0xcbf29ce484222325; read the key as 22 little-endian u64 words plus a 23rd
word made of its last 5 bytes and 3 zero bytes; for each word: h = (h XOR word) × 0x100000001b3 (wrapping). Then
the MurmurHash3 finalizer: h ^= h >> 33; h ×= 0xff51afd7ed558ccd; h ^= h >> 33; h ×= 0xc4ceb9fe1a85ec53; h ^= h
>> 33. Then for each extra byte b: h = (h XOR b) × 0x100000001b3. The extra bytes are: v, H, N, D; for each bot tile
0..18: its resource (255 for desert) and its number; for each port slot 0..8: its kind (255 for generic); and
`port_ring`.

**Sample seeds**: seed_i = root XOR (0x9e3779b97f4a7c15 × (i + 1)) (wrapping), i = 0..127. (The sample count is
clamped to 1..192 and the worker count to 1..8; VINCO uses 128 and 4.)

### 8.2 Random numbers

All randomness uses **xoshiro256\*\*** seeded by **SplitMix64**:

- seeding from a u64 s: four times: s += 0x9e3779b97f4a7c15; z = s; z = (z XOR (z >> 30)) × 0xbf58476d1ce4e5b9; z =
  (z XOR (z >> 27)) × 0x94d049bb133111eb; word = z XOR (z >> 31). The words are the state s0..s3 in that order.
- next(): result = rotl(s1 × 5, 7) × 9; t = s1 << 17; s2 ^= s0; s3 ^= s1; s1 ^= s2; s0 ^= s3; s2 ^= t; s3 =
  rotl(s3, 45).
- below(n), 1 ≤ n < 2^32 (Lemire's method on 32 bits): m = (next() >> 32) × n as u64; if (m mod 2^32) < n: t = (2^32 -
  n) mod n; while (m mod 2^32) < t: m = (next() >> 32) × n. Return m >> 32. below(1) consumes one draw.
- uniform(): (next() >> 11) × 2^-53 as f64.
- **Chance sampling** with an RNG: total = Σ weights (as an integer); pick = below(total); return the first outcome
  in engine order with pick < weight, subtracting each skipped weight from pick.

### 8.3 Who may run rollouts, and the belief

The rollout stage does nothing (the reply values stay) if the list of finalists is empty, or the root is not phase 7
with turn owner v, or the root is done, or N ≠ 0, or the sum of the unseen pool ≠ H + D.

**Belief about the rival's held cards**: pool[k] = the unseen pool, held = H, cap = `victory_points` - 1 - the
rival's visible points. The law on counts c[0..4] with Σ c = held, c[k] ≤ pool[k] and c[1] ≤ cap is proportional to
Π_k C(pool[k], c[k]) × exp(θ[k] × c[k]), with

    θ = (0, 1.9643513439850364, 0.007863944175760883, -0.320691297631005, 0.12540603456233995)

The belief is undefined (the stage does nothing) if cap < 0, Σ pool > 25, held > Σ pool, or pool[k] > (14, 5, 2, 2,
2)[k] for some k, or if the law has no mass. Tables, all f64:

- shift = max θ. limit[k] = min(pool[k], held), and also ≤ cap for k = 1.
- f[k][j] for j = 0..limit[k]: lc = 0; for j ≥ 1: lc += ln(pool[k] - j + 1) - ln(j); f[k][j] = lc + (θ[k] - shift) ×
  j. Entries beyond limit are -∞.
- suffix[5][0] = 0, every other entry starts at -∞. For k = 4 down to 0, for left = 0..held: m = min(limit[k], left);
  big = max over j = 0..m of f[k][j] + suffix[k+1][left-j]; if big is not finite leave -∞; else suffix[k][left] = big +
  ln(Σ_{j=0..m} exp(f[k][j] + suffix[k+1][left-j] - big)).
- The law has mass when suffix[0][held] is finite.

**Sampling** with seed s: RNG = seed(s XOR 0xa49b8132f093ca61); left = held; for k = 0..4: u = uniform(); acc = 0;
for j = 0..min(limit[k], left): skip j if suffix[k+1][left-j] is not finite; remember j as the last valid; acc +=
exp(f[k][j] + suffix[k+1][left-j] - suffix[k][left]); if u < acc, take j and stop. If no j was taken, take the last
valid j (if none, the sample fails). c[k] = j; left -= j. The sample fails unless left = 0.

For each sample i: draw c with seed_i (a failure stops the whole stage). The **world** is the root viewpoint with the
rival's `playable_development` = c, `new_development` = 0, `development_deck` = pool - c, then
`recompute_caches()`. The stage stops if the world's deck total ≠ D or the rival's visible points + c[1] ≥
`victory_points`.

### 8.4 One rollout

Inputs: a finalist's endpoint e (a state of the root search), the world of sample i, seed_i.

1. Start state r = e with the world's `development_deck` and the rival's playable cards from the world, the rival's
   new cards 0, then `recompute_caches()`.
2. Streams: dice = seed(seed_i XOR 0x2dd79a409a716c17); draw = seed(seed_i XOR 0x1cb875d064a0ba11); theft =
   seed(seed_i XOR 0xe133ab27fc80150d); player 0 = seed(seed_i XOR 0x871893dcc7a5c363); player 1 = seed(seed_i XOR
   0x9c18e08134fe6aa5).
3. stop = root `turn_number` + 5. counted = r's `turn_number`; main = 0; correction = 0 (f64).
4. Repeat at most 384 times:
   - If r is done or r's `turn_number` ≥ stop, leave the loop.
   - If r's `turn_number` ≠ counted: counted = it; main = 0.
   - **Chance**: the stream is dice (phase 3), draw (phase 8) or theft (otherwise). o = chance sampling with that
     stream. Control variate: M = T = 0; for each outcome o' in engine order with weight w: y' = stop value (8.5) of r after o';
     M += f64(w) × f64(y'); T += f64(w); remember y' of o. correction += f64(y of o) - M / T. r = r after o.
   - **Decision**: if phase(r) = 7 and (r's `turn_number` = the root's, or main ≥ 12), the action is 181. Otherwise
     the mover u decides as a **rollout player** (8.6) on the viewpoint (r, u), with player u's stream. If phase(r)
     = 7, main += 1. Apply the action.
5. If r is not done and its `turn_number` < stop, the rollout is **cut**: the whole rollout stage stops and the reply
   values stay.
6. Return f32(f64(stop value of r) - correction).

Rollouts start from the endpoint, so the rest of the viewer's root turn (a pending draw, theft, robber move or free
road) is played first; at any main phase of the root turn the turn ends at once. The rollout ends at the start of the
fifth turn after the root turn's number (the rival's turn after the viewer's second following turn), or at the end of
the game.

### 8.5 Stop value

The stop value of a state r: 1 if the root viewer has won, 0 if the rival has won; else the **fixed** evaluation of
a320c1l on the inputs of the viewpoint (r, root viewer) (3.1: with r's own true deck count and the rival's cards
hidden again). No economy or card term. No clamp.

### 8.6 Rollout player

A main search (whatever the phase) of its viewpoint with these settings: beam width 2, plan depth 3, economy weight
0, card weight 0.1, robber rechecks 4, reply finalists 0, rollout samples 0, one-step depth 1, pre-roll rule off.
It uses the full root procedure of 7.5 with these settings (so its other decisions are after(child, 0), that is the
leaf value of the child or its chance mean) and its own stream as the RNG for step 6. Its search values use the
viewpoint of the deciding player: the rollout player of the rival knows the rival's sampled cards; the viewer's
rollout player sees them hidden.

A rollout player may keep, for the whole root decision, a memo from (its viewpoint's 181-byte key, viewer, H, N) to
the list of candidate actions of step 6 (only for decisions that reached step 6). On a hit it picks
`list[rng.below(len)]`. This uses the stream exactly as a fresh search would.

### 8.7 Order and parallelism

Returns g[k][i] for finalist k (in shortlist order) and sample i. Each (k, i) is independent of all others: the world,
the streams and the rollout players' choices depend only on seed_i, the finalist and the root. Run them in any order
on any number of threads; collect them by index. If any rollout is cut, or any belief sample fails, the whole stage
stops and the reply values stay.

### 8.8 Final pick

The shortlist is the root actions with value > -1 after the reply check (at most 4), by value descending (ties by
index), at most 8. (If reply finalists were 0, the shortlist would be built from the plan values instead, after
removing equal end keys as in 7.1. VINCO does not use this.) n = 128 (as f64). selected = 0, gain = 0. For k = 1..K-1: Σ = Σ_i d_i and Q = Σ_i d_i² over i in
order, with d_i = f64(g[k][i]) - f64(g[0][i]); err = sqrt(max(0, Q - Σ × Σ / n) / (n × (n - 1))); adj = Σ / n - err;
if adj > gain: gain = adj, selected = k. Then mean = f32(Σ_i f64(g[selected][i]) / n). All values become -2, and the
value of the selected root action becomes clamp(mean, 0.000001, 0.999999). Step 6 of 7.5 then returns it.

Finalist 0 is the best plan by the reply check (the **anchor**). Another finalist replaces it only if its mean paired
gain minus one standard error is positive. With a single finalist the 128 rollouts still run and set its value.

## 9. Hidden information

- **What the bot uses.** The full engine state that its owner can see: the board, all buildings and roads, both
  resource hands (public in 1v1), the bank, the dice history, all public card history (cards played), its own
  development cards by kind, the rival's card count H and the count N bought this turn, the true deck count D, and the
  game counters of 2.2. The rival's card kinds and its Victory Point cards are never read: the viewpoint (3.1) moves
  them into the unseen pool before anything is computed.
- **For a caller that does not know the rival's card kinds** (Colonist play): give the rival any kinds, as long as
  for every kind the rival's cards plus the deck equal the unseen pool (14, 5, 2, 2, 2 minus the bot's own cards and
  minus the cards played by both players).
- **In the plan search and the reply check**, the rival holds no development cards: its cards are part of the unseen
  pool, so a card the bot buys is drawn from the unseen pool with exact weights, and the rival never buys or plays a
  card. Its visible points are its only points.
- **In the rollouts**, each sample gives the rival concrete cards drawn from the belief of 8.3, conditioned on "the
  rival has not already won". The rest of the pool is the deck. Both rollout players play the sampled world; each sees
  only its own viewpoint.

## 10. Command-line interface

One binary, `bot`. All JSON is one object per line. Positions use the fixture position format
(`tests/fixtures/README.md`): `state` (hex of the 323 `C1PC` bytes) and `protocol` (the fields of 2.2).

| Command | Input | Output |
|---|---|---|
| `bot decide [--preset vinco\|reply\|beam] [--rollouts N] [--workers N] [--seed S] [--nets DIR]` | one position per line on stdin | one line per position: `{"action": joint id, "engine_actions": [...], "value": f32, "values": [[joint id, f32], ...], "rng_state_after": [4 hex words]}` |
| `bot inputs` | positions | per position `{"inputs": [hex viewer 0, hex viewer 1]}` |
| `bot value --net FILE [--float\|--fixed]` | one hex input string per line | one f32 per line, as 8 hex digits of its bits |
| `bot arena A B FIRST_PAIR PAIRS` | A and B: a preset (`vinco`, `reply`, `beam`), `random`, or `cmd:PROGRAM` | one result line per game |

Defaults: preset vinco, `--nets nets`, `--seed 0`, `--workers` = 4. The decision RNG is seed(S) (8.2). `values` lists
the root actions in the order in which they were valued (the settlement root reorders them, 5.1). `engine_actions` is
the engine sequence of 2.3 (empty for the protocol finish of Road Building). The decider reads its viewer from the
position's `mover`.

**Arena.** For pair p = FIRST_PAIR .. FIRST_PAIR + PAIRS - 1 and seat s = 0, 1 (A sits at seat s, B at the other):
seed = p × 1000003 (wrapping u64); board from seed(seed + 1) (2.5); chance stream seed(seed + 2); parity
configuration of 2.1 with step limit 10,000; initial position of 2.2. Until done: at a chance position, chance
sampling with the chance stream; at a decision, the player whose seat is `mover` decides with RNG seed(seed + step ×
1009 + 3) (step = the position's `step`). A `random` player returns the only action if there is one, else
`legal[below(count)]`. A `cmd:` player is a program started once per game: the arena writes `{"position": ...,
"rng_seed": "..."}` on its stdin and reads one line with a joint action id; an illegal answer loses the game.
Output per game:

    R pair seat winner turns steps seconds_A seconds_B decisions_A decisions_B

winner is A, B or - (step limit), turns = engine `turn_number` - 1, steps = final `step`, seconds with two decimals,
decisions = calls to the player including forced ones. All fields except the two times are deterministic.

## 11. Test plan

Build in this order and gate each layer on its fixtures (formats in `tests/fixtures/README.md`):

1. **Random numbers.** xoshiro256\*\*, `below` and seeding: `rng_state_after` of every decision record. A decision
   with one legal action draws nothing; every other decision draws through exactly one `below` call (7.5).
2. **Decision layer.** Replay every game of `games.jsonl` from its board through 2.2: after every event the engine
   state bytes, the protocol fields and the legal joint actions must match `positions.jsonl` wherever a position is
   listed, and the final `result_line` must match. `arena.txt` has the same results from the reference arena.
3. **Inputs.** 4,667 positions × 2 viewers: all 351 bytes.
4. **Networks.** The same positions: float and fixed values of both nets, bit for bit.
5. **Search value.** `search_value` (VINCO main or opening mode as of 4.4, for the position itself) and
   `rollout_player_value` (economy 0).
6. **Goal bonus and GCC sort.** `sort-orders.txt`, then `opening.jsonl`: for every final setup road, each child's
   search value, value + own goal, and the opening leaf.
7. **Opening decisions.** Decision records at phases 0 and 1 (all configs give the same).
8. **Beam.** `config = beam` records: action and every listed value.
9. **Reply.** `config = reply` records.
10. **Seeds and belief.** `key181` and `sample_root_seed` of every main-phase decision record; `retention.jsonl`.
11. **Rollouts.** `rollout-traces/`: two r16 decisions with every rollout event (start state key, every action and
    chance step, each control-variate term, the stop). Then all `r16` and `vinco` records, and the three complete
    VINCO games (every decision of them is a `vinco` record with its position embedded).
12. **Arena.** Reproduce `arena.txt` with `bot arena reply reply 0 24` and so on (the file names its runs).

A test fails on any differing bit. A test that passes must also be shown to fail on a corrupted fixture value once.
