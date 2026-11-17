/* Compile against the actual shared library:
 * cc -std=c11 -Wall -Wextra -Werror -Iinclude tests/ffi_smoke.c \
 *   -Ltarget/debug -lcatan_engine -Wl,-rpath,"$PWD/target/debug" \
 *   -o .local/ffi-smoke
 * .local/ffi-smoke
 * No source file inclusion or Rust test harness substitutes for this check.
 */
#include "catan_engine.h"
#include <assert.h>
#include <inttypes.h>
#include <stdio.h>
#include <string.h>

_Static_assert(sizeof(CatanBoard) == 47, "board ABI");
_Static_assert(sizeof(CatanConfig) == 14, "config ABI");
_Static_assert(offsetof(CatanConfig, victory_points) == 4, "config offset");
_Static_assert(sizeof(CatanOutcome) == 48, "outcome ABI");
_Static_assert(offsetof(CatanOutcome, probability) == 40, "probability offset");
_Static_assert(sizeof(CatanNodeInfo) == 4, "node ABI");
static uint64_t fnv(const uint8_t *bytes, size_t n) {
    uint64_t h = UINT64_C(14695981039346656037);
    for (size_t i = 0; i < n; ++i) h = (h ^ bytes[i]) * UINT64_C(1099511628211);
    return h;
}
static void checkpoint(CatanState *state, const char *name, int dump) {
    uint8_t bytes[CATAN_CANONICAL_SIZE];
    uint64_t hash = 0;
    assert(catan_state_serialize(state, bytes, sizeof bytes) == CATAN_OK);
    assert(catan_state_hash(state, &hash) == CATAN_OK);
    assert(hash == fnv(bytes, sizeof bytes));
    if (dump) {
        printf("%s hash=%016" PRIx64 " bytes=", name, hash);
        for (size_t i = 0; i < sizeof bytes; ++i) printf("%02x", bytes[i]);
        putchar('\n');
    }
}
int main(int argc, char **argv) {
    const int dump = argc == 2 && strcmp(argv[1], "--examples") == 0;
    assert(catan_abi_version() == 1 && catan_encoding_version() == 1);
    assert(catan_canonical_size() == CATAN_CANONICAL_SIZE);
    assert(catan_action_capacity() == CATAN_ACTION_CAPACITY);
    assert(catan_outcome_capacity() == CATAN_OUTCOME_CAPACITY);
    assert(catan_state_size() > 0 && catan_state_alignment() > 0);
    CatanBoard board;
    CatanConfig config;
    assert(catan_board_default(&board) == CATAN_OK);
    assert(catan_config_default(&config) == CATAN_OK);
    CatanState *state = NULL, *clone = NULL, *decoded = NULL;
    assert(catan_state_new(&board, &config, &state) == CATAN_OK);
    uint8_t dead = 99;
    assert(catan_is_dead(state, &dead) == CATAN_OK && dead == 0);
    dead = 99;
    assert(catan_is_dead(NULL, &dead) == CATAN_NULL_POINTER && dead == 99);
    assert(catan_is_dead(state, NULL) == CATAN_NULL_POINTER);
    checkpoint(state, "initial", dump);
    assert(catan_state_apply_action(state, 79) == CATAN_OK);
    checkpoint(state, "settlement0", dump);
    assert(catan_state_apply_action(state, 7) == CATAN_OK);
    checkpoint(state, "road0", dump);
    uint16_t actions[CATAN_ACTION_CAPACITY];
    size_t n = 0;
    for (int i = 0; i < 6; ++i) {
        assert(catan_state_legal_actions(state, actions, CATAN_ACTION_CAPACITY, &n) == CATAN_OK);
        assert(n > 0);
        assert(catan_state_apply_action(state, actions[0]) == CATAN_OK);
    }
    CatanNodeInfo info;
    assert(catan_state_info(state, &info) == CATAN_OK);
    assert(info.phase == 2 && info.actor == 0 && info.winner == CATAN_NONE);
    assert(catan_state_apply_action(state, 0) == CATAN_OK);
    assert(catan_state_info(state, &info) == CATAN_OK);
    assert(info.phase == 3 && info.actor == CATAN_NONE);
    CatanOutcome outcomes[CATAN_OUTCOME_CAPACITY];
    assert(catan_state_chance_outcomes(state, outcomes, CATAN_OUTCOME_CAPACITY, &n) == CATAN_OK);
    assert(n == 11);
    uint64_t sum = 0;
    double probability_sum = 0;
    for (size_t i = 0; i < n; ++i) {
        assert(outcomes[i].id == i && outcomes[i].reserved == 0);
        assert(outcomes[i].weight_hi == 0 && outcomes[i].total_hi == 0);
        assert(outcomes[i].total_lo == outcomes[0].total_lo);
        sum += outcomes[i].weight_lo;
        probability_sum += outcomes[i].probability;
    }
    assert(sum == outcomes[0].total_lo);
    /* Summing eleven binary64 terms: error bounded by gamma_11. */
    const double u = 0x1p-53;
    const double gamma_11 = (11 * u) / (1 - 11 * u);
    assert(probability_sum >= 1 - 2 * gamma_11 && probability_sum <= 1 + 2 * gamma_11);
    assert(catan_state_apply_outcome(state, 0) == CATAN_OK);
    uint8_t bytes[CATAN_CANONICAL_SIZE], other[CATAN_CANONICAL_SIZE];
    assert(catan_state_serialize(state, bytes, sizeof bytes) == CATAN_OK);
    assert(catan_state_clone(state, &clone) == CATAN_OK);
    assert(catan_state_from_bytes(bytes, sizeof bytes, &decoded) == CATAN_OK);
    assert(catan_state_serialize(decoded, other, sizeof other) == CATAN_OK);
    assert(memcmp(bytes, other, sizeof bytes) == 0);
    assert(catan_state_apply_action(clone, 1) == CATAN_OK);
    assert(catan_state_copy(clone, state) == CATAN_OK);
    assert(catan_state_serialize(clone, other, sizeof other) == CATAN_OK);
    assert(memcmp(bytes, other, sizeof bytes) == 0);
    memset(other, 0xa5, sizeof other);
    assert(catan_state_serialize(state, other, sizeof other - 1) == CATAN_BUFFER_TOO_SMALL);
    for (size_t i = 0; i < sizeof other; ++i) assert(other[i] == 0xa5);
    assert(catan_state_apply_action(state, UINT16_MAX) == CATAN_ILLEGAL_TRANSITION);
    assert(catan_state_serialize(state, other, sizeof other) == CATAN_OK);
    assert(memcmp(bytes, other, sizeof bytes) == 0);
    CatanState *unchanged = decoded;
    bytes[12] = 1;
    assert(catan_state_from_bytes(bytes, sizeof bytes, &unchanged) == CATAN_INVALID_INPUT);
    assert(unchanged == decoded);
    assert(catan_state_info(NULL, &info) == CATAN_NULL_POINTER);
    /* Load the same frozen E10 fixture used by the Rust regression. */
    FILE *fixture = fopen("tests/fixtures/dead-seed191.hex", "r");
    assert(fixture != NULL);
    for (size_t i = 0; i < sizeof bytes; ++i) {
        unsigned int byte;
        assert(fscanf(fixture, "%2x", &byte) == 1);
        bytes[i] = (uint8_t)byte;
    }
    assert(fclose(fixture) == 0);
    CatanState *dead_state = NULL;
    assert(catan_state_from_bytes(bytes, sizeof bytes, &dead_state) == CATAN_OK);
    assert(catan_is_dead(dead_state, &dead) == CATAN_OK && dead == 1);
    assert(catan_state_info(dead_state, &info) == CATAN_OK);
    assert(info.phase != 13 && info.winner == CATAN_NONE);
    assert(catan_state_serialize(dead_state, other, sizeof other) == CATAN_OK);
    assert(memcmp(bytes, other, sizeof bytes) == 0);
    assert(catan_state_free(dead_state) == CATAN_OK);
    assert(catan_state_free(state) == CATAN_OK);
    assert(catan_state_free(clone) == CATAN_OK);
    assert(catan_state_free(decoded) == CATAN_OK);
    assert(catan_state_free(NULL) == CATAN_OK);
    puts("C ABI smoke passed");
    return 0;
}
