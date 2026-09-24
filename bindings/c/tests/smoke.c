/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* Copyright (C) 2026 Robson Trasel */

/* Compiled and run in CI against the generated header and the built library:
 * builds an index, searches it, checks the results and the error paths. */

#include <stdio.h>
#include <string.h>

#include "keyhammer.h"

static int failures = 0;

#define CHECK(cond)                                                          \
    do {                                                                     \
        if (!(cond)) {                                                       \
            fprintf(stderr, "%s:%d: check failed: %s\n", __FILE__, __LINE__, \
                    #cond);                                                  \
            failures++;                                                      \
        }                                                                    \
    } while (0)

#define ENTRY(s, w) {(const uint8_t *)(s), sizeof(s) - 1, (w)}

int main(void) {
    CHECK(kh_abi_version() == KH_ABI_VERSION);

    kh_entry entries[] = {
        ENTRY("hello", 5),
        ENTRY("help", 9),
        ENTRY("world", 1),
        ENTRY("Hello", 2), /* duplicate of "hello" after normalising */
    };
    kh_index *idx = NULL;
    CHECK(kh_index_build(entries, 4, &idx) == KH_OK);
    CHECK(idx != NULL);
    if (idx == NULL) {
        return 1;
    }

    size_t n = 0;
    CHECK(kh_index_len(idx, &n) == KH_OK);
    CHECK(n == 3);

    kh_config cfg;
    CHECK(kh_config_default(&cfg) == KH_OK);
    CHECK(cfg.struct_size == sizeof(kh_config));
    cfg.k = 5;
    cfg.ranking = KH_RANKING_EXACT;

    kh_results res;
    CHECK(kh_search(idx, (const uint8_t *)"helo", 4, &cfg, &res) == KH_OK);
    CHECK(res.len >= 2);
    int found_hello = 0;
    for (size_t i = 0; i < res.len; i++) {
        if (res.hits[i].term_len == 5 &&
            memcmp(res.hits[i].term, "hello", 5) == 0) {
            found_hello = 1;
            CHECK(res.hits[i].weight == 5);
        }
        if (i > 0) {
            CHECK(res.hits[i - 1].cost <= res.hits[i].cost);
        }
    }
    CHECK(found_hello);
    kh_results_free(&res);
    CHECK(res.hits == NULL && res.len == 0);
    kh_results_free(&res); /* second free is a no-op */

    /* Default config (NULL), exact match. */
    CHECK(kh_search(idx, (const uint8_t *)"world", 5, NULL, &res) == KH_OK);
    CHECK(res.len >= 1 && res.hits[0].cost == 0);
    kh_results_free(&res);

    /* Unicode: case and diacritics are folded, hits return the original
       text and the position of the entry (ABI version 2). */
    kh_entry uni[] = {
        ENTRY("S\xc3\xa3o Paulo", 3), /* "Sao Paulo" with a tilde on the a */
        ENTRY("a\xc3\xa7\xc3\xa3o", 1), /* the same word in lower case */
        ENTRY("A\xc3\x87\xc3\x83" "O", 9), /* upper case: same term after folding */
    };
    kh_index *uidx = NULL;
    CHECK(kh_index_build(uni, 3, &uidx) == KH_OK);
    if (uidx != NULL) {
        CHECK(kh_index_len(uidx, &n) == KH_OK);
        CHECK(n == 2);
        CHECK(kh_search(uidx, (const uint8_t *)"SAO PAULO", 9, NULL, &res) ==
              KH_OK);
        CHECK(res.len == 1 && res.hits[0].cost == 0 &&
              res.hits[0].input_index == 0);
        CHECK(res.len == 1 && res.hits[0].term_len == 10 &&
              memcmp(res.hits[0].term, "S\xc3\xa3o Paulo", 10) == 0);
        kh_results_free(&res);
        CHECK(kh_search(uidx, (const uint8_t *)"acao", 4, NULL, &res) == KH_OK);
        CHECK(res.len == 1 && res.hits[0].input_index == 2 &&
              res.hits[0].weight == 9);
        kh_results_free(&res);
        /* Invalid UTF-8 is still an error. */
        CHECK(kh_search(uidx, (const uint8_t *)"\xc3(", 2, NULL, &res) ==
              KH_ERR_INVALID_UTF8);
        kh_index_free(&uidx);
    }

    /* Error paths. */
    CHECK(kh_search(idx, (const uint8_t *)"\xff\xfe", 2, NULL, &res) ==
          KH_ERR_INVALID_UTF8);
    CHECK(strlen(kh_last_error()) > 0);
    CHECK(kh_search(NULL, (const uint8_t *)"a", 1, NULL, &res) ==
          KH_ERR_NULL_POINTER);
    CHECK(kh_search(idx, (const uint8_t *)"a", 1, NULL, NULL) ==
          KH_ERR_NULL_POINTER);
    kh_config bad = cfg;
    bad.ranking = 99;
    CHECK(kh_search(idx, (const uint8_t *)"a", 1, &bad, &res) ==
          KH_ERR_INVALID_ARGUMENT);
    CHECK(kh_search(idx, (const uint8_t *)"a", (size_t)-1, NULL, &res) ==
          KH_ERR_QUERY_TOO_LONG);
    bad.ranking = KH_RANKING_COARSE;
    bad.reserved = 1;
    CHECK(kh_search(idx, (const uint8_t *)"a", 1, &bad, &res) ==
          KH_ERR_INVALID_ARGUMENT);
    CHECK(strcmp(kh_status_string(KH_ERR_INVALID_UTF8), "invalid UTF-8") == 0);

    kh_index *none = (kh_index *)1;
    CHECK(kh_index_build(entries, 0, &none) == KH_ERR_EMPTY_INDEX);
    CHECK(none == NULL);

    kh_index_free(&idx);
    CHECK(idx == NULL);
    kh_index_free(&idx); /* second free is a no-op */

    if (failures != 0) {
        fprintf(stderr, "%d check(s) failed\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
