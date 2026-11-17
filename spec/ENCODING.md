# Catan 1v1 encoding, version 1

Status: accepted version 1. This file defines the shared contract for SPEC.md. It does not change SPEC.md. Decisions that resolve gaps appear below as encoding conventions. Both engines must implement the same conventions.

## Values and ordering

All integers are unsigned. Wire integers use little-endian order. Arrays use increasing index order. No host padding, pointers, floats, caches or random-generator state enter the wire representation.

- Players: 0, 1. Optional player, hex, node, resource and phase fields use 255 for none. Zero is a real index.
- Resources: 0 wood, 1 brick, 2 sheep, 3 wheat, 4 ore. Terrain also permits 5 desert.
- Development types: 0 knight, 1 victory point, 2 road building, 3 year of plenty, 4 monopoly.
- Ports: 0 wood, 1 brick, 2 sheep, 3 wheat, 4 ore, 5 any3to1.
- Hexes 0..18, nodes 0..53, edges 0..71 and ports 0..8 use topology.json unchanged.
- Node occupancy: 0 empty, 1 player-0 settlement, 2 player-1 settlement, 3 player-0 city, 4 player-1 city.
- Edge occupancy: 0 empty, 1 player-0 road, 2 player-1 road.
- Booleans: 0 false, 1 true. Other values are invalid.

The deck is an unordered multiset. A draw chooses a type with probability count divided by deck size. No pre-shuffled order exists in a canonical state. This is exactly the marginal process of an initially uniform random shuffle.

## Configuration

Configuration is immutable and included in every state hash. Defaults follow SPEC. Extra variants below are engine extensions, not verified Colonist rules. Reject unknown enum values. The numeric fields accept their full wire range except victory_points, which must be positive. A large victory target can make a game unwinnable. The engine does not impose a turn limit.

| Field | Wire type | Default | Meaning and other supported values |
|---|---|---|---|
| dice | u8 enum | 0 | 0 colonist_balanced, 1 independent |
| first_player | u8 | 0 | 0 or 1. Also configures S1 |
| friendly_robber | bool | 1 | R2 protection applies when true |
| discard_limit | u8 | 9 | Discard only when the total exceeds this value |
| victory_points | u16 | 15 | A4 target |
| no_adjacent_6_8 | bool | 1 | B3 generator option |
| no_adjacent_same_number | bool | 0 | B3 generator option |
| shortage_rule | u8 enum | 0 | 0 partial_single: T4. 1 all_or_none: pay nobody for a resource if total demand exceeds supply |
| longest_road_break_rule | u8 enum | 0 | 0 retain_incumbent_tie: A3. 1 clear_on_tie: after every road or building change, a tie removes the award, even from an incumbent |
| vp_cards_hidden | bool | 1 | D7: hide opponent VP identities when true. False reveals their new and playable VP counts |
| observation_rule | u8 enum | 0 | I2: 0 public_resources_hidden_devs. 1 reveal_all: reveal all card counts |
| discards_public | bool | 1 | I2: include discarded resource identities in public event reporting |
| road_building_needs_legal_edge | bool | 0 | D5 assumed variant: require at least one legal connected free road edge before PLAY_ROAD_BUILDING when true |

Generator options constrain generated boards only. They do not reject otherwise valid input boards. The board constructor requires the exact B2 resource, number and port multisets. Desert token is 0. Other tokens must match B2. The constructor accepts no randomness. A caller supplies a generated or hand-assigned board.

With discards_public=false, public discard events omit the resource identity. Resource hands remain public under I2. Consecutive observations can therefore reveal a discard by subtraction. This flag does not promise secrecy against that inference. Hidden-resource inference would require a separate history-aware observation contract.

## Global actions

An action is a u16. The policy output has exactly 226 slots, indexed 0..225. The phase determines legality, not the action's meaning. Return legal actions in ascending ID order, with no duplicates. An invalid action returns an error and leaves its input unchanged. There is no implicit pass or cancellation.

| IDs | Formula | Action |
|---|---|---|
| 0 | 0 | ROLL |
| 1 | 1 | END_TURN |
| 2 | 2 | BUY_DEVELOPMENT |
| 3 | 3 | PLAY_KNIGHT |
| 4 | 4 | PLAY_ROAD_BUILDING |
| 5 | 5 | PLAY_YEAR_OF_PLENTY |
| 6 | 6 | PLAY_MONOPOLY |
| 7..78 | 7 + edge | PLACE_ROAD(edge), paid in MAIN, free during setup or road building |
| 79..132 | 79 + node | PLACE_SETTLEMENT(node), free during setup |
| 133..186 | 133 + node | PLACE_CITY(node) |
| 187..205 | 187 + hex | MOVE_ROBBER(hex) |
| 206..210 | 206 + resource | DISCARD(resource), one card |
| 211..215 | 211 + resource | CHOOSE_RESOURCE(resource), one YOP card or the monopoly type |
| 216..220 | 216 + resource | TRADE_GIVE(resource) |
| 221..225 | 221 + resource | TRADE_TAKE(resource) |

V1 completes forced effects automatically. A fixed action buffer of 226 u16 entries is sufficient by construction.

## Chance outcomes

Chance IDs use a separate u16 namespace. Return positive-probability outcomes in ascending ID order. Even a single certain outcome remains an explicit chance node. An action at a chance node, or an outcome at a decision node, is invalid.

| IDs | Formula | Meaning |
|---|---|---|
| 0..10 | total - 2 | Dice total 2..12 |
| 11..15 | 11 + resource | Steal one card of that resource |
| 16..20 | 16 + development type | Draw that development type |

Steal probabilities are opponent resource counts divided by opponent hand size. Draw probabilities are remaining type counts divided by remaining deck size. Dice probabilities appear below. Every accepted outcome has strictly positive mass. There is no random call inside apply, initialization, serialization, legal actions or outcome enumeration.

An API may return f64 probabilities. Their definition is the exact rational distribution in this document. It may instead expose integer mass and total mass as u128. A caller supplies randomness to sample outcomes. Trace replay uses chosen IDs and needs no seed.

## Phases and pending choices

| Value | Name | Decision actor or chance kind | Permitted action families |
|---|---|---|---|
| 0 | SETUP_SETTLEMENT | setup player | PLACE_SETTLEMENT |
| 1 | SETUP_ROAD | setup player | PLACE_ROAD |
| 2 | PRE_ROLL | turn_owner | ROLL, four PLAY actions |
| 3 | DICE | chance | dice outcomes |
| 4 | DISCARD | first player with remaining discard | DISCARD |
| 5 | ROBBER | turn_owner | MOVE_ROBBER |
| 6 | STEAL | chance | steal outcomes |
| 7 | MAIN | turn_owner | END_TURN, BUY, four PLAY actions, paid building, TRADE_GIVE |
| 8 | DEVELOPMENT_DRAW | chance | development outcomes |
| 9 | ROAD_BUILDING | turn_owner | PLACE_ROAD |
| 10 | YEAR_OF_PLENTY | turn_owner | CHOOSE_RESOURCE |
| 11 | MONOPOLY | turn_owner | CHOOSE_RESOURCE |
| 12 | TRADE_TAKE | turn_owner | TRADE_TAKE |
| 13 | TERMINAL | none | none |

The setup player is [first_player, 1-first_player, 1-first_player, first_player][setup_pairs_done]. During setup turn_owner equals that setup player. During normal play turn_owner remains the roller, including the opponent's discard decisions. Chance and terminal nodes have no decision actor. The actor is derived and is not serialized.

setup_pairs_done counts completed settlement/road pairs, not individual placements. It starts at 0 and remains 4 after setup. SETUP_ROAD retains setup_anchor, the node just settled. Its road must be empty and touch that node. Other connections do not qualify. The settlement action gives starting resources immediately when setup_pairs_done is 2 or 3. The road action then increments setup_pairs_done. After pair 4, enter first_player's PRE_ROLL and set turn_number=1.

turn_number is 0 during setup. It counts normal turns started. END_TURN increments it exactly once and changes the turn owner. Dice totals, individual discards and other subactions do not increment it.

resume_phase is PRE_ROLL or MAIN while resolving a development effect. A seven sets it to MAIN for its discard/robber/steal sequence. TRADE_TAKE and DEVELOPMENT_DRAW also set it to MAIN. It is 255 in all other phases. Completing a pending effect restores resume_phase and clears all its pending fields.

new_development[p][type] contains cards bought during the current turn of player p. playable_development contains older cards, including unplayed VP cards. END_TURN moves every new card of the outgoing player into playable, then zeros new. The incoming player therefore starts with no new cards. VP cards in either group count immediately. They cannot be played.

Playing a development card removes one playable card, increments spent_development for its owner and sets development_played=1. The card is spent immediately, before any choices. Knight counts equal spent_development[p][knight]. Other played card types remain publicly counted. There is no separate knight counter. A PLAY action is legal only with a playable card and development_played=0. VP has no PLAY action. PRE_ROLL permits all four playable types.

- ROLL prepares the dice history as specified below, then enters DICE.
- A non-seven outcome pays production simultaneously by resource and enters MAIN. Compute both players' demand before removing bank cards.
- A seven computes both discard totals once, from the hands at that instant. Each total is floor(hand_size/2) if hand_size exceeds discard_limit, else 0. Store both remaining counts. The queue order is turn_owner, then the opponent. Skip a player with zero remaining count. A DISCARD action moves one card to the bank and decreases that player's remaining count. After both reach zero, enter ROBBER. No player can perform another action during the queue.
- PLAY_KNIGHT updates Largest Army immediately and then enters ROBBER, unless the win check ends the game first. It never sets discard counts.
- MOVE_ROBBER changes robber_hex. If the opponent occupies an adjacent node and has cards, enter STEAL. Otherwise complete the effect. STEAL transfers one selected card and completes the effect. No victim-selection action is needed with two players.
- PLAY_ROAD_BUILDING requires at least one road piece in stock. With road_building_needs_legal_edge=true, it also requires at least one empty connected edge where a free road is legal. With the default false, pieces in stock permit play even if no edge is legal. An accepted play sets choices_remaining=2. Each free road decreases it by one. Complete when it reaches zero, no road piece remains, or no road is legal. With one piece left, place one road and complete. Place as many roads as possible. There is no voluntary stop. If stock remains but no road is initially legal, consuming the card completes immediately.
- PLAY_YEAR_OF_PLENTY sets choices_remaining=2. Each choice transfers one available bank card and decreases it. Complete at zero or when the entire bank is empty. If the bank starts empty, consuming the card completes immediately.
- PLAY_MONOPOLY enters MONOPOLY. Any of the five resources is legal, even if the opponent has none. Transfer all opponent cards of that resource and complete the effect.
- TRADE_GIVE records trade_give without transferring cards. It is legal only if at least one different resource is available to take, and the player can pay the best current rate. TRADE_TAKE must choose a different available resource. Transfer the payment and requested card atomically, then complete the trade. The rate is derived from the current board. No other action can change that board between the choices.
- BUY_DEVELOPMENT pays its cost to the bank and enters DEVELOPMENT_DRAW. It requires a nonempty deck. The outcome removes one card from the deck and adds it to new_development, then completes the effect.
- END_TURN is legal in MAIN. First check the outgoing player's win. Otherwise age their new cards, switch owner, increment turn_number, clear development_played, set last_dice=0 and enter PRE_ROLL. Check the incoming player's win immediately.

In ROBBER, friendly protection includes both players, including the mover. It uses current visible points, after any award from playing a knight. The destination must differ from robber_hex. If no legal destination exists, complete the robber effect automatically without movement or theft. This completion convention avoids an empty nonterminal decision node on unusual input boards or configurations.

## Award and win normalization

The exact longest edge-trail lengths must be current after each road or building placement. Implementations may selectively update affected components or retain unaffected cached lengths. An edge cannot occur twice in a trail. A node may occur more than once. An opponent building prevents passage through that node, but a trail can end there. Independent branches cannot reuse edges of a cycle.

With the default award rule, retain the incumbent if its length is at least 5 and at least the opponent's length. Otherwise award the strictly longest length of at least 5. A tie without a qualifying incumbent gives no holder. With clear_on_tie, ties always give no holder. Apply the chosen rule after every placement, including setup. Largest Army retains its incumbent on a tie. Without an incumbent it goes to the first player reaching 3 knights.

After each accepted action or outcome, update awards before testing the normal turn owner's total points. This includes development draws and the first free road. The opponent's discard action cannot make that opponent win out of turn. The normal turn owner alone is eligible. Setup has no win check until the first normal turn begins.

A win immediately enters TERMINAL. It can interrupt a pending robber move, theft or development effect. Preserve all already-applied transfers and spent cards. Set winner=turn_owner, clear pending fields and retain turn_number, last_dice, development_played and card age groups. Terminal states have no outgoing transitions. A player who receives an award out of turn wins only if still qualified at the start of their own turn.

For normalization, clear setup_anchor to 255 outside SETUP_ROAD. Clear trade_give to 255 outside TRADE_TAKE. Clear choices_remaining to 0 outside ROAD_BUILDING and YEAR_OF_PLENTY. Clear discard_remaining to [0,0] outside DISCARD. Clear resume_phase to 255 outside phases 4,5,6,8,9,10,11,12. winner is 255 outside TERMINAL. No inactive field retains stale data.

Forced completion, win checks and pending-field clearing occur inside apply. They produce no extra trace step. Do not skip an explicit chance node, even if it has one outcome. At every returned state, a nonterminal decision node has at least one legal action.

## Dead positions (E10)

`dead(state)` is a separate predicate on valid, normalized engine states. Rust also exposes `State::is_dead()`.
It does not change legal actions, chance outcomes, apply, the terminal phase or the winner.
It adds no state field, canonical byte, hash input or configuration field.
It returns true exactly when all these conditions hold:

1. Setup is complete (`setup_pairs_done=4`), and the phase is not TERMINAL.
2. Both players' total points are strictly below `victory_points`.
3. For each player, either all 15 road pieces are on the board or no geometrically eligible road edge exists.
   Such an edge is empty and has at least one endpoint that either contains that player's settlement or city,
   or is empty and touches that player's road. An opponent building blocks passage and does not supply a connection.
   Ignore affordability in this check.
4. Neither player has a geometrically eligible settlement node. Such a node is empty, has no occupied neighbor,
   and touches that player's road. Ignore affordability and settlement stock in this check.
5. Each player has either all four cities on the board or no settlement to upgrade.
6. The development deck is empty for every card type.
7. Largest Army cannot change. If player h holds it, the other player's played, playable and new knights
   sum to at most h's played knights. If nobody holds it, each player's played, playable and new knights sum to less than three.

Use both players' own unplayed cards for condition 7, including cards that cannot be played this turn.
The predicate uses the omniscient state. It is not an observation feature.
These conditions are conservative. False does not assert that a winner remains reachable.

Conditions 3 through 5 freeze road and building occupancy together. Suppose there were a first transition that changed either.
It cannot place a road: that player has no piece or no geometrically eligible edge in the unchanged occupancy.
It cannot place a settlement: no geometrically eligible node exists, regardless of resources or stock.
It cannot upgrade a settlement: no city piece or no owned settlement remains. Thus no first occupancy change exists.
A new own road connection could arise only through an own road or building placement, both excluded above.
An opponent building blocks connections. No action removes a road or building or transfers its ownership.
Resource transfers cannot create a geometric site. Piece stocks, road connectivity, road cuts, ports and Longest Road stay fixed.
These statements concern valid, normalized states. With unused road stock but no eligible edge,
default Road Building can consume its card and complete immediately without placing a road.
A no-edge ROAD_BUILDING decision is not a valid returned state: forced completion runs inside apply.
Condition 6 prevents new development cards and fixes the VP cards held by each player.
For an army holder, the competitor's remaining total cannot exceed the holder's current played count.
Playing a knight preserves that competitor total and cannot decrease the holder's played count.
Without a holder, neither player can reach three knights. Largest Army therefore stays fixed.
Road Building cannot add a road. Other development effects, dice, robber moves, steals, trades and discards only change resources or pending fields.
The total points and both awards stay fixed after every accepted transition. Condition 2 therefore remains true and prevents a win.
Every listed condition remains true after every accepted action or outcome. Thus the predicate is sound and closed under successors.
Representation overflow remains an error, not a successor or a game result.

## Balanced dice

The following constants come from colonist-balanced-dice.ts: initial cards [1,2,3,4,5,6,5,4,3,2,1], reshuffle threshold 13, recent memory 5, recent penalty 0.34 and streak penalty 0.4.

Initial state: the initial card counts, recent_len=0, recent=[0,0,0,0,0], initialized_player_mask=0, sevens=[0,0], streak_owner=255 and streak_length=0.

On ROLL, before entering DICE:

1. Set the bit for turn_owner in initialized_player_mask. This reproduces initTotalSevens before the first draw.
2. If sum(dice_cards) is less than 13, replace only dice_cards with the initial counts.
3. Preserve the recent window, sevens and streak across a reshuffle.

Outcome enumeration does not mutate state. At the canonical DICE node, let p be turn_owner, c[t] the cards for total t, C their sum, r[t] its recent occurrence count, n the mask population count, S the sum of sevens and s[p] that player's sevens. Define exact rational values:

```
recent_factor[t] = max(0, 1 - (17/50) * r[t])
imbalance = 1                         if S < n
imbalance = 2 - n * s[p] / S          otherwise
sign = -1                            if streak_owner == p
sign = +1                            otherwise
seven_factor = clamp(imbalance + (2/5) * streak_length * sign, 0, 2)
w[t] = (c[t] / C) * recent_factor[t]
w[7] = w[7] * seven_factor
P(t) = w[t] / sum(w[2..12])
```

n is 1 or 2, not always 2. In particular the first roller enters the first DICE node with a one-bit mask. S<n prevents division by zero. Streak length counts successive sevens by the same player, ignoring intervening non-seven rolls. A non-seven does not reset the streak.

A shared integer representation may cancel C and 50. Let A be the clamped seven factor as numerator a over denominator d. Use d=1 when S<n and otherwise d=5*S. Convert the factor to this denominator, then clamp its numerator into [0,2*d]. When S<n, use denominator 5 instead if streak_length is nonzero. Equivalent reduced fractions are valid. Set mass[t]=c[t]*max(0,50-17*r[t])*d for t other than 7. Set mass[7]=c[7]*max(0,50-17*r[7])*a. Their sum is total mass. Intermediate arithmetic must not overflow. For streak_length >= 5, seven_factor is 0 for the streak owner and 2 for the other player. Use this shortcut before multiplying counters. For smaller streaks, signed i128 arithmetic suffices for the numerator. The resulting nonnegative masses fit u128 for the wire counter widths below.

Applying a balanced dice outcome removes one card of its total. Append that total to the FIFO. If its length exceeds five, remove the oldest entry. Store the oldest remaining entry first and zero unused entries. If the result is 7, increment sevens[p]. If streak_owner equals p, increment streak_length. Otherwise set streak_owner=p and streak_length=1. Do not reshuffle after the outcome. The state can retain 12 cards until the next ROLL action.

For independent dice, P(t) is the initial count for t divided by 36. ROLL and its outcome leave all dice-history fields at their initial values. last_dice still records the outcome for both models.

The JavaScript source computes IEEE-754 binary64 weights in total order 2..12. A direct comparison port must retain its arithmetic order: divide cards by C, multiply by recent factor, clamp negative weights, multiply seven by its clamped adjustment, then sum weights in ascending total order. Compare the rational distribution with that port using a tolerance derived from binary64 rounding, and report that tolerance with tests. Bitwise equality between rational-to-f64 and source-order binary64 is not required. Do not serialize probabilities. Source sampling's `<=` boundary at a zero-weight entry does not give that outcome positive mathematical probability. The engine excludes zero mass.

## Canonical state bytes

The canonical representation has exactly 313 bytes. The internal state may have another fixed size. Report both sizes. The four-byte magic is ASCII `C1V1`. Version is 1. Reserved bytes must be zero.

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 4 | magic: 43 31 56 31 |
| 4 | 2 | version u16: 01 00 |
| 6 | 1 | config.dice |
| 7 | 1 | config.first_player |
| 8 | 1 | config.friendly_robber |
| 9 | 1 | config.discard_limit |
| 10 | 2 | config.victory_points u16 |
| 12 | 1 | reserved zero |
| 13 | 1 | config.no_adjacent_6_8 |
| 14 | 1 | config.no_adjacent_same_number |
| 15 | 1 | reserved zero |
| 16 | 1 | config.shortage_rule |
| 17 | 1 | config.longest_road_break_rule |
| 18 | 1 | config.vp_cards_hidden |
| 19 | 1 | config.observation_rule |
| 20 | 1 | config.discards_public |
| 21 | 1 | config.road_building_needs_legal_edge bool |
| 22 | 2 | reserved zeros |
| 24 | 19 | terrain[hex] u8 |
| 43 | 19 | number[hex] u8 |
| 62 | 9 | port_type[slot] u8 |
| 71 | 54 | node_occupancy[node] u8 |
| 125 | 72 | edge_occupancy[edge] u8 |
| 197 | 1 | robber_hex u8 |
| 198 | 5 | bank[resource] u8 |
| 203 | 10 | resources[player][resource] u8, offset 203+5*p+r |
| 213 | 10 | playable_development[player][type] u8 |
| 223 | 10 | new_development[player][type] u8 |
| 233 | 10 | spent_development[player][type] u8 |
| 243 | 5 | development_deck[type] u8 |
| 248 | 1 | phase u8 |
| 249 | 1 | turn_owner u8 |
| 250 | 1 | winner u8 or 255 |
| 251 | 1 | resume_phase u8 or 255 |
| 252 | 1 | setup_pairs_done u8 |
| 253 | 1 | setup_anchor u8 or 255 |
| 254 | 1 | development_played bool |
| 255 | 1 | choices_remaining u8 |
| 256 | 1 | trade_give u8 or 255 |
| 257 | 2 | discard_remaining[player] u8 |
| 259 | 1 | largest_army_holder u8 or 255 |
| 260 | 1 | longest_road_holder u8 or 255 |
| 261 | 1 | last_dice u8, 0 before this turn's roll |
| 262 | 8 | turn_number u64 |
| 270 | 11 | dice_cards[total-2] u8 |
| 281 | 1 | recent_len u8 |
| 282 | 5 | recent FIFO u8, oldest first, unused zero |
| 287 | 1 | initialized_player_mask u8 |
| 288 | 1 | streak_owner u8 or 255 |
| 289 | 8 | streak_length u64 |
| 297 | 16 | sevens[player] u64, player 0 then 1 |

All player/type arrays use offset base+5*p+type. The canonical bytes include authoritative historical fields, such as award holders and dice history. They exclude derivable caches: points, road lengths, knights played, pieces left, bank totals, dice total, recent histograms, production tables, port access, action masks, probabilities and the decision actor. An engine may cache these internally but must validate them against recomputation.

u8 resource and card counts suffice because conservation bounds each resource count by 19 and all development cards by 25. Long-lived turn and seven counters use u64. These are representation widths, not gameplay limits. Checked overflow returns a representation error without changing the state. Never wrap, saturate, truncate a game or invent a winner. A JavaScript oracle using Number loses exact integer counter semantics beyond 2^53-1. Tests of greater counter values need a BigInt/rational oracle.

The decoder must reject wrong length, magic, version, enum values, padding, noncanonical inactive fields and broken game invariants. It must not silently repair a trace snapshot. Construction and apply return normalized states. Serialization must not mutate its input.

## Initialization

After validating the board and configuration, initialize as follows:

1. Copy board and config. Set all node and edge occupancy to zero.
2. Set robber_hex to the unique desert. Set bank=[19,19,19,19,19]. Zero all resource hands and development arrays.
3. Set development_deck=[14,5,2,2,2]. Set both award holders to 255.
4. Set phase=SETUP_SETTLEMENT, turn_owner=first_player, setup_pairs_done=0 and turn_number=0.
5. Set winner, resume_phase, setup_anchor and trade_give to 255. Set development_played, choices_remaining, discard_remaining and last_dice to zero.
6. Initialize dice history as above, even for independent dice. Set reserved bytes to zero.

Initial construction performs no setup placement, dice preparation, implicit turn start or random draw.

## Hash

Hash all 313 canonical bytes in order with FNV-1a-64:

```
h = 14695981039346656037      // 0xcbf29ce484222325
for each byte b:
    h = h XOR b
    h = (h * 1099511628211) mod 2^64
```

A trace hash is exactly 16 lowercase hexadecimal digits, most significant digit first, without `0x`. This textual display order does not change little-endian state fields. FNV-1a is a replay checksum, not a cryptographic commitment.

## Observation and determinization

The default observation exposes board, config, public state, resource hands, deck size, all spent card counts, and the observer's own card identities. It hides the opponent's unplayed types and remaining deck types. It exposes separate opponent playable and new card totals. This preserves the public purchase timing. It exposes terminal winner but never uses an omniscient hash as an observation feature.

If vp_cards_hidden=false, expose opponent VP counts separately in each age group. If observation_rule=reveal_all, expose all canonical fields. Under reveal_all, determinization is the identity operation.

For the default hidden-card observation, subtract public spent cards and the observer's own cards from [14,5,2,2,2]. Also subtract revealed opponent cards when VP visibility is disabled. These counts form the unseen multiset U. Preserve the public capacities a and b of opponent playable and new hidden slots. Preserve deck capacity c. They sum to |U|. Uniformly assign distinguishable cards from U to labeled slots in those three groups, then forget within-group order. Equivalently, sample opponent playable counts x and new counts y with probability

```
product_t binom(U[t], x[t]) / binom(|U|, a)
  * product_t binom(U[t]-x[t], y[t]) / binom(|U|-a, b)
```

The deck is U-x-y. Reinsert any revealed opponent cards into their original age group. Preserve the observer's new and playable arrays exactly. All sampled states preserve resource hands, dice history, public fields and spent cards.

Condition this distribution on the observable phase and winner: reject samples in which the current eligible player should already have won, or a terminal winner lacks the target. Also respect the special ordering of END_TURN and setup completion. Do not silently change winner or phase to fit a sample. This is a snapshot-consistent exchangeable-card prior, not an inference about the opponent's policy from their past actions. A history-aware belief model may further condition it outside the engine. The engine must reject an inconsistent observation rather than return an arbitrary state.

## JSONL traces

Each line is a UTF-8 JSON object. A file holds one game. No blank lines or comments occur. Key order is irrelevant. IDs and byte-valued arrays are JSON integers. Hashes and u64 step/turn counts are strings to preserve precision in readers that use binary64 numbers. Decimal count strings have no leading zero except `"0"`.

The required line sequence is board, config, initial, zero or more steps, result. Unknown optional metadata keys may be ignored. Required fields cannot be omitted except for the compatibility default below.

```
{"type":"board","encoding":1,"terrain":[...19 integers...],"numbers":[...19 integers...],"ports":[...9 integers...]}
{"type":"config","dice":0,"first_player":0,"friendly_robber":true,"discard_limit":9,"victory_points":15,"no_adjacent_6_8":true,"no_adjacent_same_number":false,"shortage_rule":0,"longest_road_break_rule":0,"vp_cards_hidden":true,"observation_rule":0,"discards_public":true,"road_building_needs_legal_edge":false}
{"type":"initial","hash":"0123456789abcdef"}
{"type":"step","index":"1","action":79,"hash":"0123456789abcdef"}
{"type":"step","index":"2","outcome":5,"hash":"0123456789abcdef"}
{"type":"result","status":"winner","winner":0,"steps":"2","turns":"1","hash":"0123456789abcdef"}
```

The config field road_building_needs_legal_edge defaults to false when omitted, so older v1 traces remain readable. Writers include it explicitly. Other required fields cannot be omitted.

These lines illustrate field shapes only. Ellipses and example hashes are not a valid replay fixture. Each step has exactly one of action or outcome. Its hash describes the complete normalized state after that transition. index starts at 1 and increases by one. No trace line is emitted for an automatic completion inside apply.

By default initial means the constructor state for the preceding board and config. For targeted midgame traces it may also contain `state_hex`, exactly 626 lowercase hex digits. Decode that canonical snapshot, validate it, and require exact agreement with the preceding board and config. A snapshot can represent a valid fixture without proving its reachability. Trace reports must distinguish constructor traces from snapshot traces.

A result has status `winner` or `unfinished`. A winner result requires TERMINAL and the stated winner. An unfinished result requires a nonterminal state and winner=null. steps is the number of step records. turns equals turn_number. The result hash equals the last state hash, or initial hash when no step occurred.

An unfinished result may contain `"reason":"dead"`. This requires `dead(state)=true` at the result state.
A winner result must not contain `reason`. The only defined reason value is `dead`. Readers reject other reason values.
An explicit prefix may omit `reason`, whether or not its last state is dead.
Runners that stop because of E10 emit `unfinished` with reason `dead`. They retain that game in all sampled-game counts.
This result is not a drawn game and does not change the rules-engine terminal test.
No rules-engine or runner step or turn cap exists. Arbitrary legal policies can avoid winning indefinitely.
Termination measurements concern the generator policy and must name it.

Optional producer, seed and policy metadata explain how a trace was generated. They never affect replay. Full replay traces remain omniscient even with discards_public=false. Public event export is a different format and must not expose hidden outcome IDs or state_hex.

## Small encoding examples

With default config, bytes 0..23 are:

```
43 31 56 31 01 00 00 00 01 09 0f 00 00 01 00 00 00 00 01 00 01 00 00 00
```

An initial state's bytes 248..269 are:

```
00 00 ff ff 00 ff 00 00 ff 00 00 ff ff 00 00 00 00 00 00 00 00 00
```

PLACE_SETTLEMENT(node 0) has action ID 79 and u16 bytes `4f 00`. From a default initial state it sets node byte 71 to `01`, phase byte 248 to `01`, and setup_anchor byte 253 to `00`. It changes no resource count. The legal following road on edge 0 has ID 7 and bytes `07 00`. That road sets byte 125 to `01`, phase byte 248 to `00`, turn_owner byte 249 to `01`, setup_pairs_done byte 252 to `01`, and setup_anchor byte 253 to `ff`.

Dice total 7 has chance ID 5 and bytes `05 00`. Drawing a victory point card has chance ID 17 and bytes `11 00`. These are different namespaces from action IDs.

Full engine-produced byte/hash fixtures will follow acceptance and implementation. Their generation must not precede the committed encoding contract.

## Explicit conventions and open evidence

SPEC does not fix discard order, forced completion, no-target robber completion, counter widths or trace strings. This document fixes those details for interoperability. SPEC D5 forbids Road Building with no road piece left. Its assumed no-edge rule is configurable through road_building_needs_legal_edge. The default permits a zero-effect play when stock remains. D5 requires as many roads as possible, so v1 has no voluntary stop. Win checks after chance resolution and at turn start implement A4 and the immediate VP purchase win in D7.

The alternate shortage, award and observation modes are configurable engine extensions. They are not claims about ranked play. The mask and delayed dice reshuffle come directly from the supplied Colonist source. Public resource snapshots make discards inferable even when discard events omit their identity. A stronger secrecy interpretation remains an explicit question for SPEC's owner.

## Full worked byte/hash fixtures

These three fixtures were produced by the Rust engine through its compiled C ABI.
The C smoke program recomputed FNV-1a independently. A separate Python check reconstructed all bytes from the offset table and checked each hash.
They use default configuration and this input board, in topology ID order:

```json
{"terrain":[0,0,0,0,1,1,1,2,2,2,2,3,3,3,3,4,4,4,5],"numbers":[2,3,3,4,4,5,5,6,6,8,8,9,9,10,10,11,11,12,0],"ports":[0,1,2,3,4,5,5,5,5]}
```

The generator options do not reject a hand-assigned board. Each byte block below contains all 313 bytes.
Remove whitespace to obtain the 626-character `state_hex` value. Line breaks are for display only.

### Constructor state

Hash: `7a97e8f83c5198b0`.

```text
433156310100000001090f000001000000000100010000000000000001010102
0202020303030304040405020303040405050606080809090a0a0b0b0c000001
0203040505050500000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000000000
0000000000121313131313000000000000000000000000000000000000000000
000000000000000000000000000000000000000e050202020000ffff00ff0000
ff0000ffff000000000000000000010203040506050403020100000000000000
ff000000000000000000000000000000000000000000000000
```

### After action 79: settlement at node 0

Hash: `c8d1887e91a81959`.

```text
433156310100000001090f000001000000000100010000000000000001010102
0202020303030304040405020303040405050606080809090a0a0b0b0c000001
0203040505050501000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000000000
0000000000121313131313000000000000000000000000000000000000000000
000000000000000000000000000000000000000e050202020100ffff00000000
ff0000ffff000000000000000000010203040506050403020100000000000000
ff000000000000000000000000000000000000000000000000
```

### After actions 79, 7: road at edge 0

Hash: `53323333dfad046a`.

```text
433156310100000001090f000001000000000100010000000000000001010102
0202020303030304040405020303040405050606080809090a0a0b0b0c000001
0203040505050501000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000010000
0000000000000000000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000000000
0000000000121313131313000000000000000000000000000000000000000000
000000000000000000000000000000000000000e050202020001ffff01ff0000
ff0000ffff000000000000000000010203040506050403020100000000000000
ff000000000000000000000000000000000000000000000000
```
