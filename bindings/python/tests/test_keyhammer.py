# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Robson Trasel
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
        ([("ok", 70000)], kh.BuildError),
        ([("café", 1)], kh.BuildError),
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
    with pytest.raises(kh.SearchError):
        idx.search("java", budget=1000)
    with pytest.raises(ValueError):  # every keyhammer error is a ValueError
        idx.search("café")
    with pytest.raises(ValueError):
        idx.search("java", k=-1)
    with pytest.raises(ValueError):
        kh.SearchConfig(budget=70000)
    assert issubclass(kh.QueryTooLongError, kh.KeyhammerError)


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

