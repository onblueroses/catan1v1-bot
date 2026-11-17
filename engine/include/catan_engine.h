#ifndef CATAN_ENGINE_H
#define CATAN_ENGINE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* ABI 1. Link libcatan_engine.so. Integer fields here use native byte order.
 * Canonical state bytes instead use ENCODING.md's little-endian format.
 *
 * A handle belongs to this library. Only new/from_bytes/clone allocate it.
 * Free each handle exactly once. Do not malloc, memcpy, resize or inspect it.
 * copy reuses an existing handle without allocation. All other hot operations
 * avoid allocation. state_size/alignment report the current internal layout for
 * diagnostics only, not caller-owned storage or a persistent ABI guarantee.
 *
 * Nonnull pointers must be valid, naturally aligned and readable/writable for
 * their declared type and count. Buffers and output parameters must not overlap
 * each other or the handle. copy alone permits identical source/destination.
 * A pointer can be null only for free, or for a zero-result array. Insufficient
 * array capacity returns BUFFER_TOO_SMALL before checking the array pointer.
 *
 * On error, state and output parameters are unchanged. out_count is written on
 * success only. Allocate ACTION_CAPACITY and OUTCOME_CAPACITY entries to avoid
 * retrying enumeration. serialize writes exactly CANONICAL_SIZE bytes.
 * Constructors leave *out unchanged on error. Passing an existing handle as
 * *out on success does not free it: the caller must retain/free that old handle.
 *
 * Rust unwinding is caught and reported as PANIC. Invalid dangling pointers,
 * data races and aliases that violate this contract are undefined behavior.
 * Process aborts, such as allocation failure, cannot be caught. Build this
 * library with Rust panic=unwind when panic recovery is required.
 *
 * Separate handles can be used concurrently. One handle permits concurrent
 * reads, but mutation/free requires exclusive access. No internal RNG exists.
 */
#define CATAN_ACTION_CAPACITY 226
#define CATAN_OUTCOME_CAPACITY 11
#define CATAN_CANONICAL_SIZE 313
#define CATAN_NONE 255

/* Every function below that returns int32_t returns one of these values.
 * INVALID_INPUT: a board, a configuration or canonical bytes failed validation.
 * ILLEGAL_TRANSITION: the action or outcome is not legal in the handle's state.
 * COUNTER_OVERFLOW: a 64-bit turn or seven counter would overflow. This is a
 * representation limit, not a game result. */
typedef enum CatanStatus {
    CATAN_OK = 0, CATAN_NULL_POINTER = 1, CATAN_BUFFER_TOO_SMALL = 2,
    CATAN_INVALID_INPUT = 3, CATAN_ILLEGAL_TRANSITION = 4,
    CATAN_COUNTER_OVERFLOW = 5, CATAN_PANIC = 6, CATAN_MISALIGNED_POINTER = 7
} CatanStatus;

typedef struct CatanState CatanState;
/* Resource order: wood, brick, sheep, wheat, ore. Terrain 5 = desert.
 * Port types: resource index or 5 = generic 3:1. Use topology.json IDs. */
typedef struct CatanBoard {
    uint8_t terrain[19], numbers[19], ports[9];
} CatanBoard;
/* Booleans are uint8_t 0/1. See ENCODING.md for enums. */
typedef struct CatanConfig {
    uint8_t dice, first_player, friendly_robber, discard_limit;
    uint16_t victory_points;
    uint8_t no_adjacent_6_8, no_adjacent_same_number;
    uint8_t shortage_rule, longest_road_break_rule, vp_cards_hidden;
    uint8_t observation_rule, discards_public, road_building_needs_legal_edge;
} CatanConfig;
/* Exact mass = (hi << 64) | lo. Probability is weight / total.
 * All listed outcomes have positive weight. reserved is always zero. */
typedef struct CatanOutcome {
    uint32_t id, reserved;
    uint64_t weight_lo, weight_hi, total_lo, total_hi;
    double probability;
} CatanOutcome;
/* Phase codes 0..13 are in ENCODING.md. actor/winner use CATAN_NONE.
 * turn_owner stays the roller while an opponent discards. */
typedef struct CatanNodeInfo {
    uint8_t phase, actor, turn_owner, winner;
} CatanNodeInfo;

/* Versions and sizes of the loaded library. Compare them with the macros above. */
uint32_t catan_abi_version(void);
uint32_t catan_encoding_version(void);
size_t catan_canonical_size(void);
size_t catan_action_capacity(void);
size_t catan_outcome_capacity(void);
size_t catan_state_size(void);
size_t catan_state_alignment(void);
/* The fixed default board and the default rule configuration of SPEC.md. */
int32_t catan_board_default(CatanBoard *out);
int32_t catan_config_default(CatanConfig *out);
/* Handle life cycle. new, from_bytes and clone allocate a handle into *out.
 * from_bytes decodes exactly CATAN_CANONICAL_SIZE validated bytes. copy
 * overwrites an existing handle and allocates nothing. free accepts null. */
int32_t catan_state_new(const CatanBoard *board, const CatanConfig *config, CatanState **out);
int32_t catan_state_from_bytes(const uint8_t *bytes, size_t length, CatanState **out);
int32_t catan_state_clone(const CatanState *source, CatanState **out);
int32_t catan_state_copy(CatanState *destination, const CatanState *source);
int32_t catan_state_free(CatanState *state);
/* Queries. A node is terminal when info.winner is a player, a chance node when
 * info.actor is CATAN_NONE and there is no winner, and a decision node otherwise. */
int32_t catan_state_info(const CatanState *state, CatanNodeInfo *out);
/* Separate, allocation-free E10 certificate. Writes 0 or 1 on success.
 * A dead position is nonterminal and cannot lead to a win. A false result does
 * not assert that a win is reachable. This query does not mutate the handle. */
int32_t catan_is_dead(const CatanState *state, uint8_t *out);
/* Enumeration in ascending id order. Action ids and outcome ids are separate
 * number spaces, both defined in ENCODING.md. A decision node has no outcomes
 * and a chance node has no actions: the count is then zero. */
int32_t catan_state_legal_actions(const CatanState *state, uint16_t *actions,
    size_t capacity, size_t *out_count);
int32_t catan_state_chance_outcomes(const CatanState *state, CatanOutcome *outcomes,
    size_t capacity, size_t *out_count);
/* Transitions mutate the handle. To keep the parent, copy it into a scratch
 * handle first and apply there. */
int32_t catan_state_apply_action(CatanState *state, uint16_t action);
int32_t catan_state_apply_outcome(CatanState *state, uint16_t outcome);
/* The 313 canonical bytes and their FNV-1a-64 hash (ENCODING.md). */
int32_t catan_state_serialize(const CatanState *state, uint8_t *bytes, size_t capacity);
int32_t catan_state_hash(const CatanState *state, uint64_t *out);

#ifdef __cplusplus
}
#endif
#endif
