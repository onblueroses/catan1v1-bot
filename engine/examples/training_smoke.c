/* See README.md for build commands. This is a short API demonstration,
 * not a full-game runner or a performance measurement. GCC/Clang are required
 * for unsigned 128-bit integer sampling. The caller owns all random state. */
#include "catan_engine.h"
#include <assert.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef __uint128_t U128;

static uint64_t next_u64(uint64_t *rng) {
    *rng += UINT64_C(0x9e3779b97f4a7c15);
    uint64_t z = *rng;
    z = (z ^ (z >> 30)) * UINT64_C(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)) * UINT64_C(0x94d049bb133111eb);
    return z ^ (z >> 31);
}

static U128 mass(uint64_t hi, uint64_t lo) {
    return ((U128)hi << 64) | lo;
}

static uint16_t sample_outcome(const CatanOutcome *outcomes, size_t n, uint64_t *rng) {
    assert(n > 0);
    U128 total = mass(outcomes[0].total_hi, outcomes[0].total_lo);
    assert(total > 0);
    /* Reject the incomplete residue range. Unsigned subtraction wraps mod 2^128. */
    U128 threshold = ((U128)0 - total) % total;
    U128 draw;
    do {
        /* Separate calls preserve RNG order under C's evaluation rules. */
        uint64_t hi = next_u64(rng);
        uint64_t lo = next_u64(rng);
        draw = mass(hi, lo);
    } while (draw < threshold);
    draw %= total;
    for (size_t i = 0; i < n; ++i) {
        U128 weight = mass(outcomes[i].weight_hi, outcomes[i].weight_lo);
        assert(total == mass(outcomes[i].total_hi, outcomes[i].total_lo));
        if (draw < weight) return (uint16_t)outcomes[i].id;
        draw -= weight;
    }
    abort(); /* Valid engine outcomes have positive masses summing to total. */
}

#define CHECK(call) do { \
    int32_t status = (call); \
    if (status != CATAN_OK) { \
        fprintf(stderr, "%s returned %" PRId32 "\n", #call, status); \
        goto cleanup; \
    } \
} while (0)

#define REQUIRE(condition) do { \
    if (!(condition)) { \
        fprintf(stderr, "Check failed: %s\n", #condition); \
        goto cleanup; \
    } \
} while (0)

int main(void) {
    int result = EXIT_FAILURE;
    CatanState *root = NULL, *scratch = NULL, *decoded = NULL;
    CatanBoard board;
    CatanConfig config;
    uint16_t actions[CATAN_ACTION_CAPACITY];
    CatanOutcome outcomes[CATAN_OUTCOME_CAPACITY];
    uint8_t before[CATAN_CANONICAL_SIZE], after[CATAN_CANONICAL_SIZE];
    size_t n = 0;
    uint64_t rng = 41, hash = 0, decoded_hash = 0;
    uint8_t dead = 0;
    CatanNodeInfo info;

    REQUIRE(catan_abi_version() == 1 && catan_encoding_version() == 1);
    REQUIRE(catan_canonical_size() == CATAN_CANONICAL_SIZE);
    CHECK(catan_board_default(&board));
    CHECK(catan_config_default(&config));
    CHECK(catan_state_new(&board, &config, &root));
    CHECK(catan_state_clone(root, &scratch)); /* Allocate reusable search storage once. */

    /* S1 has exactly four settlement/road pairs. Choose the first legal placement.
     * Each child starts from a copy. Its parent remains byte-for-byte unchanged. */
    for (unsigned pair = 0; pair < 4; ++pair) {
        for (unsigned placement = 0; placement < 2; ++placement) {
            CHECK(catan_state_serialize(root, before, sizeof before));
            CHECK(catan_state_legal_actions(root, actions, CATAN_ACTION_CAPACITY, &n));
            REQUIRE(n > 0);
            CHECK(catan_state_copy(scratch, root));
            CHECK(catan_state_apply_action(scratch, actions[0]));
            CHECK(catan_state_serialize(root, after, sizeof after));
            REQUIRE(memcmp(before, after, sizeof before) == 0);
            CHECK(catan_state_copy(root, scratch)); /* Commit the selected child. */
        }
    }

    CHECK(catan_state_serialize(root, before, sizeof before));
    CHECK(catan_state_copy(scratch, root));
    CHECK(catan_state_apply_action(scratch, 0)); /* ROLL enters an explicit chance node. */
    CHECK(catan_state_info(scratch, &info));
    REQUIRE(info.actor == CATAN_NONE && info.winner == CATAN_NONE);
    CHECK(catan_state_chance_outcomes(scratch, outcomes, CATAN_OUTCOME_CAPACITY, &n));
    REQUIRE(n > 0);
    uint16_t outcome = sample_outcome(outcomes, n, &rng);
    CHECK(catan_state_apply_outcome(scratch, outcome));

    /* Persist canonical bytes, never native handle storage. Decode validates them. */
    CHECK(catan_state_serialize(scratch, after, sizeof after));
    CHECK(catan_state_hash(scratch, &hash));
    CHECK(catan_state_from_bytes(after, sizeof after, &decoded));
    CHECK(catan_state_hash(decoded, &decoded_hash));
    REQUIRE(hash == decoded_hash);
    CHECK(catan_state_serialize(decoded, before, sizeof before));
    REQUIRE(memcmp(before, after, sizeof before) == 0);
    CHECK(catan_is_dead(decoded, &dead));
    CHECK(catan_state_info(decoded, &info));
    printf("sampled_outcome=%u actor=%u winner=%u dead=%u canonical_bytes=%zu hash=%016"
           PRIx64 "\n", (unsigned)outcome, (unsigned)info.actor,
           (unsigned)info.winner, (unsigned)dead, sizeof after, hash);

    /* Restore the saved parent into scratch without allocation or sparse undo. */
    CHECK(catan_state_serialize(root, before, sizeof before));
    CHECK(catan_state_copy(scratch, root));
    CHECK(catan_state_serialize(scratch, after, sizeof after));
    REQUIRE(memcmp(before, after, sizeof before) == 0);
    result = EXIT_SUCCESS;

cleanup:
    if (catan_state_free(decoded) != CATAN_OK) result = EXIT_FAILURE;
    if (catan_state_free(scratch) != CATAN_OK) result = EXIT_FAILURE;
    if (catan_state_free(root) != CATAN_OK) result = EXIT_FAILURE;
    return result;
}
