# Rule questions and implementation conventions

## Adopted E10 blocked-road certificate, 2026-11-12

This change replaces only condition 3: each player either has
no road piece left or has no geometrically eligible road edge. Ignore
affordability. All other E10 conditions remain unchanged. Eleven captured
seed-41 games satisfy this certificate and fail the earlier road-stock check.
Their exact origin indices, canonical snapshots and snapshot traces are in
`tests/fixtures/blocked-roads/`. Independent raw-edge and successor tests cover
both players, pending effects and unaffordable-road positive controls.

Shared encoding commit `de1302e` records the adopted condition and
closed-successor proof. `diagnostics/ENCODING-blocked-roads.patch` preserves the
reviewed change. Both builds must use the exact condition and replay the new
fixtures. The original 418-file trace manifest remains unchanged.

The shared [encoding v1](../spec/ENCODING.md), committed before engine code, fixes the following details for both builds. These choices are conventions where SPEC leaves a gap, not new claims about ranked play.

- Discards resolve one card at a time, with the roller first. The normal turn owner remains unchanged during the opponent's discard actions.
- Road Building and Year of Plenty take as much as legally possible. Neither effect offers a voluntary stop. Year of Plenty permits a zero-effect play.
- D5 correction (2026-11-12, SPEC commit `2a171a6`, ENCODING commit `d872711`): Road Building requires at least one road piece. One remaining piece permits play and completion after one road. With stock but no legal edge, the default permits a zero-effect play. The assumed variant `road_building_needs_legal_edge=true` requires a connected free legal edge before play. Canonical byte 21 holds this boolean. Bytes 22..23 remain zero. The format stays 313 bytes at version 1, and default worked hashes stay unchanged. JSON readers default an omitted flag to false for older traces.
- A robber effect with no legal destination completes without movement or theft. Friendly protection applies to both players, including the mover.
- Win checks run after each action and outcome. They also run at the start of the incoming player's turn. A win can interrupt a pending effect.
- Unplayed development cards have separate playable and new groups. Observation sampling preserves both public group totals.
- Balanced dice registers the roller before the first draw. Reshuffling occurs before a roll when fewer than 13 cards remain. It preserves the recent window and seven history.
- Snapshot traces can contain valid fixtures without proving their historical reachability. Decoding checks game invariants, not an exhaustive backward reachability proof.
- Counters use the wire widths. Overflow returns an error. It does not end a game or select a winner.

## Evidence still missing

Ranked board-generation constraints are not published. The generator defaults to no adjacent 6/8 and permits adjacent equal numbers. Both constraints are configurable. Input boards need the standard multisets, but need not obey generator constraints.

The alternate shortage rule, award tie rule and observation mode are configurable extensions. The defaults implement SPEC. They are not verified alternate Colonist settings.

`discards_public=false` controls resource identities in public discard-event reporting. Resource snapshots remain public under I2. Two consecutive observations can reveal a discard by subtraction. Full replay traces remain omniscient. A history-aware hidden-resource observation requires a separate contract.

Observation sampling implements the specified exchangeable-card prior. It conditions on current victory eligibility, phase and winner. It does not infer card identities from the opponent's policy or from past decisions.

No legal-policy termination theorem is claimed. An agent can keep ending turns without building. Runner termination measurements describe the named policy and seed set. An unfinished trace is a prefix, not a draw.

## Reachable nonwinning game

The property seed 191 reaches a position where neither player can reach the 15-point target.
Each player placed all 15 roads. Neither player has a legal future settlement site.
The development deck is empty. The fixed awards and remaining city upgrades permit at most 14 points per player.
The constructor trace proves reachability through 1522 transitions at turn 361.
See `tests/fixtures/nonwinning-seed191.md` and `tests/termination.rs`.

SPEC resolves the conflict with E10. The accepted ENCODING conditions define a separate dead predicate.
The first implementation uses the frozen case. Both players lack roads, settlement sites and possible city upgrades.
The deck is empty, and Largest Army cannot change even after all remaining knights are played.
`dead(&state)` and `State::is_dead()` implement these conditions. The C binding exposes `catan_is_dead`.
These queries do not change apply, the win-only terminal test, state bytes or hashes.
False does not assert that a winner remains reachable.

The later seed-191 position at step 1763 has fixed totals of 14 and 14. It satisfies the frozen predicate.
Its constructor trace is `tests/fixtures/dead-seed191.jsonl`.
The trace result is `unfinished` with reason `dead`. The earlier 1522-step trace remains an explicit prefix without a reason.
`tests/dead.rs` checks the conditions, positive controls and every accepted transition from all possible dead phases.
Both dice models and both turn owners are covered. The tests also cover unplayed knights that cannot pass Largest Army.
Runners retain certified nonwins in every sampled-game count. No step or turn cap is introduced.

## Implementation status

The Rust core is original code. Longest Road does not reuse the suspect cycle algorithm from the reference audit. E5 is resolved: the public move-only `UndoRecord` measures 88 bytes, and sparse make/unmake is faster on the measured branching ensemble. Pure copy-and-apply remains available. Geometry layout changes require their own E5 check before acceptance.

## Snapshot validation boundary

The decoder checks canonical fields, active-operation safety, conservation, piece limits and awards. It does not prove that a fixture has a legal predecessor. The following review observations remain advisory within that boundary:

- A pending Road Building, Year of Plenty or Monopoly fixture can have `development_played=false` or no matching spent card. The decoder does not reconstruct the preceding PLAY action.
- A pending development draw can lack the bank-stock increase expected from its preceding purchase. The decoder does not reconstruct that payment.
- An isolated MAIN fixture can have `last_dice=0` or an empty board. Such fixtures support rule tests without a complete setup history.

These cases preserve operation safety and conservation. Tests that depend on a preceding action must construct that action or supply its complete context. Unpaid pending trades and depleted-bank setup grants are different: they could underflow on apply, so the decoder rejects them and the transfers also check subtraction.
