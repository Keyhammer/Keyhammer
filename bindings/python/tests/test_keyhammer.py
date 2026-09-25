# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Robson Trasel
import pathlib
import threading

import pytest

import keyhammer as kh

WORDS = [
    ("javascript", 10),
    ("typescript", 10),
    ("python", 10),
    ("rust", 10),
    ("java", 10),
]


def test_readme_example_matches_core_doc_example():
    # Mirrors the Rust quick start: "javasript" -> "javascript" at cost 16.
    idx = kh.Index(WORDS)
    res = idx.search("javasript")
    assert res.hits[0].term == "javascript"
    assert res.hits[0].cost == 16
    assert res.hits[0].weight == 10
    assert not res.truncated
    assert len(res) == len(res.hits)


def test_matches_rust_results_on_small_dictionary():
    # Expected values were produced by running the Rust core directly on the
    # same five terms: Trie::build(WORDS), CostModel::qwerty(), Searcher::search
    # with the SearchConfig noted in each comment.
    idx = kh.Index(WORDS)
    # k=3, defaults (budget 32, Coarse)
    got = [(h.term, h.cost, h.weight) for h in idx.search("pyhton", k=3).hits]
    assert got == [("python", 12, 10)]
    # k=5, budget 64, Exact
    res = idx.search("jav", k=5, budget=64, ranking=kh.Ranking.EXACT)
    assert [(h.term, h.cost, h.weight) for h in res.hits] == [
        ("java", 16, 10),
        ("rust", 60, 10),
    ]
    # k=5, budget 64, Exact: nothing within budget
    assert idx.search("scrip", k=5, budget=64, ranking=kh.Ranking.EXACT).hits == []


def test_exact_ranking_and_config_object():
    idx = kh.Index(WORDS)
    a = idx.search("javasript", ranking=kh.Ranking.EXACT)
    b = idx.search("javasript", config=kh.SearchConfig(ranking=kh.Ranking.EXACT))
    assert [h.term for h in a.hits] == [h.term for h in b.hits]
    assert kh.Ranking.EXACT != kh.Ranking.COARSE
    cfg = kh.SearchConfig.high_recall()
    assert (cfg.budget, cfg.tsb, cfg.k) == (48, True, 10)
    # keyword arguments override the config
    assert len(idx.search("java", k=1, config=cfg).hits) == 1


def test_case_is_folded_and_duplicates_keep_highest_weight():
    idx = kh.Index([("Rust", 1), ("rust", 7), ("go", 3)])
    assert len(idx) == 2
    hit = idx.search("RUST").hits[0]
    assert (hit.term, hit.weight, hit.cost) == ("rust", 7, 0)


def test_build_from_generator():
    idx = kh.Index((w, 1) for w in ["alpha", "beta"])
    assert len(idx) == 2


def test_k_zero_and_no_match():
    idx = kh.Index(WORDS)
    assert idx.search("java", k=0).hits == []
    assert idx.search("qqqqqqqq", budget=16).hits == []


@pytest.mark.parametrize(
    "items, exc",
    [
        ([], kh.BuildError),
        ([("", 1)], kh.BuildError),
        ([("a" * 70000, 1)], kh.BuildError),
        ([("ok", -1)], kh.BuildError),
        ([("ok", 65536)], kh.BuildError),
        ([("ok", 2**70)], kh.BuildError),
        ([("ab\ud800", 1)], kh.BuildError),
        ([("ok", "x")], TypeError),
        ([("ok",)], TypeError),
        (5, TypeError),
    ],
)
def test_build_errors(items, exc):
    with pytest.raises(exc):
        kh.Index(items)


def test_search_errors():
    idx = kh.Index(WORDS)
    with pytest.raises(kh.QueryTooLongError):
        idx.search("a" * 129)
    with pytest.raises(kh.BudgetTooLargeError):
        idx.search("java", budget=65)
    with pytest.raises(kh.BudgetTooLargeError):
        idx.search("java", budget=70000)
    with pytest.raises(kh.SearchError):
        idx.search("ab\ud800")
    with pytest.raises(kh.SearchError):
        idx.search("java", k=-1)
    with pytest.raises(kh.SearchError):
        idx.search("java", budget=-1)
    with pytest.raises(kh.KeyhammerError):
        kh.SearchConfig(budget=70000)
    with pytest.raises(TypeError):
        idx.search(5)
    assert issubclass(kh.QueryTooLongError, kh.SearchError)
    assert issubclass(kh.SearchError, kh.KeyhammerError)
    assert issubclass(kh.KeyhammerError, ValueError)


def test_hit_index_maps_back_to_input_position():
    items = [("Rust", 1), ("go", 3), ("rust", 7), ("zig", 2)]
    idx = kh.Index(items)
    hit = idx.search("rust").hits[0]
    assert hit.index == 2  # highest weight wins among duplicates
    assert items[hit.index][1] == hit.weight
    tie = kh.Index([("rust", 5), ("Rust", 5)]).search("rust").hits[0]
    assert tie.index == 0  # first among ties
    assert hash(hit) == hash(idx.search("rust").hits[0])


def test_config_defaults_and_high_recall():
    assert kh.SearchConfig() == kh.SearchConfig(10, 32, kh.Ranking.COARSE, False, 100000)
    cfg = kh.SearchConfig.high_recall()
    assert cfg.ranking == kh.Ranking.COARSE


def test_public_surface():
    assert set(kh.__all__) == {
        "BudgetTooLargeError",
        "BuildError",
        "Hit",
        "Index",
        "KeyhammerError",
        "QueryTooLongError",
        "Ranking",
        "SearchConfig",
        "SearchError",
        "SearchResult",
    }
    for name in kh.__all__:
        assert hasattr(kh, name)
    assert "Index" in repr(kh.Index(WORDS))


def test_concurrent_searches_agree():
    # Index is immutable and frozen, so concurrent search() calls are safe;
    # every thread must see the same answer as a single-threaded run.
    idx = kh.Index(WORDS)
    expected = [(h.term, h.cost) for h in idx.search("javasript").hits]
    results = []

    def work():
        for _ in range(200):
            results.append([(h.term, h.cost) for h in idx.search("javasript").hits])

    threads = [threading.Thread(target=work) for _ in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(results) == 1600
    assert all(r == expected for r in results)


# ---- Unicode normalisation (issue #67) ----

TESTDATA = pathlib.Path(__file__).resolve().parents[2] / "testdata"


def _dict():
    rows = [l.split("\t") for l in (TESTDATA / "unicode_dict.tsv").read_text(encoding="utf-8").splitlines()]
    return [(t, int(w)) for t, w in rows]


def _cases():
    rows = [l.split("\t") for l in (TESTDATA / "unicode_cases.tsv").read_text(encoding="utf-8").splitlines()]
    return [(q, int(b), r) for q, b, r in rows]


def _golden():
    out = []
    for line in (TESTDATA / "unicode_expected.tsv").read_text(encoding="utf-8").splitlines():
        _, hits = line.split("\t")
        out.append([tuple(int(x) for x in h.split(":")) for h in hits.split(",")] if hits else [])
    return out


def test_matches_the_rust_core_on_the_shared_unicode_dictionary():
    # bindings/testdata/unicode_expected.tsv is produced by the Rust core alone
    # (Trie::build_normalized + Searcher::search_text; the test that keeps it
    # fresh is in bindings/c). Same dictionary, same queries, same hits.
    items = _dict()
    idx = kh.Index(items)
    cases, golden = _cases(), _golden()
    assert len(cases) == len(golden) > 50
    for i, ((q, budget, ranking), want) in enumerate(zip(cases, golden)):
        ranking = kh.Ranking.EXACT if ranking == "exact" else kh.Ranking.COARSE
        got = idx.search(q, budget=budget, ranking=ranking).hits
        assert [(h.index, h.cost, h.weight) for h in got] == want, (i, q)
        # hits return the caller's original text
        assert [h.term for h in got] == [items[h.index][0] for h in got], (i, q)


PORTUGUESE = [
    ("São Paulo", 9),
    ("coração", 5),
    ("Ação", 1),
    ("ação", 8),
    ("não", 3),
    ("pé", 2),
    ("ônibus", 4),
    ("Ç", 6),
    ("Straße", 7),
    ("Müller", 3),
    ("Crème Brûlée", 5),
]


@pytest.mark.parametrize(
    "query, term",
    [
        ("sao paulo", "São Paulo"),
        ("SAO PAULO", "São Paulo"),
        ("São Paulo", "São Paulo"),
        ("SÃO PAULO", "São Paulo"),
        ("ACAO", "ação"),
        ("AÇÃO", "ação"),
        ("coracao", "coração"),
        ("NAO", "não"),
        ("PE", "pé"),
        ("onibus", "ônibus"),
        ("c", "Ç"),
        ("strasse", "Straße"),
        ("STRASSE", "Straße"),
        ("muller", "Müller"),
        ("creme brulee", "Crème Brûlée"),
    ],
)
def test_query_case_and_diacritics_are_folded(query, term):
    idx = kh.Index(PORTUGUESE)
    hits = idx.search(query, ranking=kh.Ranking.EXACT).hits
    assert (hits[0].term, hits[0].cost) == (term, 0)


def test_hits_return_the_original_text_and_merge_equal_terms():
    idx = kh.Index(PORTUGUESE)
    assert len(idx) == len(PORTUGUESE) - 1  # "Ação" and "ação" merge
    hit = idx.search("acao").hits[0]
    assert (hit.term, hit.weight, hit.index) == ("ação", 8, 3)  # higher weight kept
    # the normalised form is never what comes back
    assert idx.search("straße").hits[0].term == "Straße"


def test_folding_can_be_turned_off():
    items = [("Café", 1), ("cafe", 1), ("CAFE", 1)]
    assert len(kh.Index(items)) == 1
    assert len(kh.Index(items, fold_case=False)) == 3
    assert len(kh.Index(items, fold_diacritics=False)) == 2
    keep = kh.Index(items, fold_diacritics=False)
    hit = keep.search("CAFÉ", ranking=kh.Ranking.EXACT).hits[0]
    assert (hit.term, hit.cost) == ("Café", 0)
    exact = kh.Index(items, fold_case=False)
    assert exact.search("CAFE", ranking=kh.Ranking.EXACT).hits[0].term == "CAFE"
    with pytest.raises(TypeError):
        kh.Index(items, False)  # the options are keyword-only


def test_a_typo_costs_one_edit_over_the_folded_text():
    idx = kh.Index(PORTUGUESE)
    hit = idx.search("SAO PAOLO", ranking=kh.Ranking.EXACT).hits[0]
    assert (hit.term, hit.cost) == ("São Paulo", 16)


def test_query_limit_counts_code_points_after_normalisation():
    idx = kh.Index([("é", 1)])
    idx.search("é" * 128)  # 256 bytes, 128 code points: accepted
    with pytest.raises(kh.QueryTooLongError):
        idx.search("é" * 129)
    with pytest.raises(kh.QueryTooLongError):
        idx.search("ß" * 65)  # folds to 130 letters
    # over the limit as given (200 code points), within it once folded (100)
    idx.search("e\u0301" * 100)


def test_lone_surrogates_are_rejected_with_a_clear_message():
    with pytest.raises(kh.BuildError, match="lone surrogate"):
        kh.Index([("ab\ud800", 1)])
    idx = kh.Index([("ab", 1)])
    with pytest.raises(kh.SearchError, match="lone surrogate"):
        idx.search("ab\udfff")


def test_a_term_that_folds_to_nothing_is_a_build_error():
    with pytest.raises(kh.BuildError, match="entry 1: term normalises to nothing"):
        kh.Index([("ok", 1), ("\u0301\u0302", 1)])
    # with the diacritic folding off nothing folds to nothing
    assert len(kh.Index([("\u0301", 1)], fold_diacritics=False)) == 1
    # decomposed input equals precomposed input
    assert kh.Index([("Cafe\u0301", 1)]).search("cafe").hits[0].cost == 0


def test_non_latin_text_is_compared_per_code_point_without_folding_scripts():
    idx = kh.Index([("Привет", 1), ("東京", 1)])
    assert idx.search("Привет").hits[0].cost == 0
    assert idx.search("東京").hits[0].term == "東京"
