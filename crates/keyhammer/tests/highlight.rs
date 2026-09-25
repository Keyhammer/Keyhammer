// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Match highlighting (`docs/design/highlighting.md`): golden examples,
//! UTF-16 offsets of astral characters, errors, and properties of the ranges
//! on random plain and normalised dictionaries.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use keyhammer::cost::{CostModel, Layout};
use keyhammer::highlight::{Highlight, HighlightError, HighlightMode, MatchRange};
use keyhammer::search::{Hit, MAX_QUERY_LEN, SearchConfig, Searcher};
use keyhammer::text::{self, Normalizer};
use keyhammer::trie::Trie;
use support::{Rng, oracle_cost, oracle_prefix_cost};

/// The code-point ranges of `h`.
fn chars(h: &Highlight) -> Vec<(usize, usize)> {
    h.ranges
        .iter()
        .map(|r| (r.chars.start, r.chars.end))
        .collect()
}

/// The highlighted substrings of `source`.
fn parts<'a>(h: &Highlight, source: &'a str) -> Vec<&'a str> {
    h.ranges.iter().map(|r| &source[r.utf8.clone()]).collect()
}

/// Searches `q` in a plain trie of `terms` and highlights the hit for `want`.
fn plain(terms: &[&str], q: &str, want: &str, mode: HighlightMode) -> Highlight {
    let items: Vec<(&str, u16)> = terms.iter().map(|&t| (t, 1)).collect();
    let trie = Trie::build(&items).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let cfg = SearchConfig::default();
    let out = match mode {
        HighlightMode::Prefix => s.search_prefix(&trie, &cm, q.as_bytes(), &cfg),
        _ => s.search(&trie, &cm, q.as_bytes(), &cfg),
    }
    .unwrap();
    let hit = out
        .hits
        .iter()
        .find(|h| trie.term(h.id) == want)
        .expect("the term is a hit");
    let h = s
        .highlight(&trie, &cm, q.as_bytes(), hit, want, mode)
        .unwrap();
    assert_eq!(h.cost, hit.cost);
    h
}

/// Searches `q` in a trie of `items` normalised by `n` and highlights the hit
/// whose source is `want`.
fn normalised(items: &[&str], n: Normalizer, q: &str, want: &str) -> Highlight {
    let entries: Vec<(&str, u16)> = items.iter().map(|&t| (t, 1)).collect();
    let trie = Trie::build_normalized(&entries, &n).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let out = s
        .search_text(&trie, &cm, q, &SearchConfig::default())
        .unwrap();
    let hit = out
        .hits
        .iter()
        .find(|h| items[trie.input_index(h.id) as usize] == want)
        .expect("the term is a hit");
    let h = s
        .highlight_text(&trie, &cm, q, hit, want, HighlightMode::Whole)
        .unwrap();
    assert_eq!(h.cost, hit.cost);
    h
}

#[test]
fn portuguese_accents_fold_and_ranges_cover_the_original_bytes() {
    let default = Normalizer::new();
    // Exact after folding: the whole original string, 7 code points, 9 bytes.
    let h = normalised(&["coração", "corrida"], default, "coracao", "coração");
    assert_eq!(h.cost, 0);
    assert_eq!(h.ranges.len(), 1);
    assert_eq!(
        h.ranges[0],
        MatchRange {
            chars: 0..7,
            utf8: 0..9,
            utf16: 0..7
        }
    );
    // Uppercase source, same folding.
    let h = normalised(&["CORAÇÃO"], default, "coracao", "CORAÇÃO");
    assert_eq!(parts(&h, "CORAÇÃO"), ["CORAÇÃO"]);
    // A typo on the last letter ("p" next to "o").
    let h = normalised(&["coração"], default, "coracap", "coração");
    assert_eq!(h.cost, 8);
    assert_eq!(parts(&h, "coração"), ["coraçã"]);
    assert_eq!(h.ranges[0].utf8, 0..8);
    // Case folding only: "ç" and "ã" are symbols of their own and the query's
    // "c" and "a" are substitutions, so they are not highlighted.
    let case_only = Normalizer::new().with_diacritic_folding(false);
    let h = normalised(&["coração"], case_only, "coracao", "coração");
    assert_eq!(h.cost, 32);
    assert_eq!(chars(&h), [(0, 4), (6, 7)]);
    assert_eq!(parts(&h, "coração"), ["cora", "o"]);
    assert_eq!(h.ranges[1].utf8, 8..9);
    assert_eq!(h.aligned.utf8, 0..9);
}

#[test]
fn a_skipped_letter_splits_the_range() {
    let h = plain(
        &["javascript", "typescript", "java"],
        "javasript",
        "javascript",
        HighlightMode::Whole,
    );
    assert_eq!(h.cost, 16);
    assert_eq!(chars(&h), [(0, 5), (6, 10)]);
    assert_eq!(parts(&h, "javascript"), ["javas", "ript"]);
}

#[test]
fn transposed_letters_are_highlighted() {
    let h = plain(
        &["receive", "recipe"],
        "recieve",
        "receive",
        HighlightMode::Whole,
    );
    assert_eq!(h.cost, 12);
    assert_eq!(chars(&h), [(0, 7)]);
    // A substitution is not.
    let h = plain(&["receive"], "receuve", "receive", HighlightMode::Whole);
    assert_eq!(h.cost, 8);
    assert_eq!(parts(&h, "receive"), ["rece", "ve"]);
}

#[test]
fn doubled_letters_skip_the_repeated_one() {
    // Skipping the second "l" costs 8 (it repeats the first), the first 16.
    let h = plain(&["hello"], "helo", "hello", HighlightMode::Whole);
    assert_eq!(h.cost, 8);
    assert_eq!(parts(&h, "hello"), ["hel", "o"]);
    // An extra "l" in the query is dropped: the whole term was typed.
    let h = plain(&["hello"], "helllo", "hello", HighlightMode::Whole);
    assert_eq!(h.cost, 8);
    assert_eq!(parts(&h, "hello"), ["hello"]);
}

#[test]
fn prefix_mode_highlights_the_typed_prefix_only() {
    let h = plain(
        &["javascript", "java", "python"],
        "jav",
        "javascript",
        HighlightMode::Prefix,
    );
    assert_eq!(h.cost, 0);
    assert_eq!(chars(&h), [(0, 3)]);
    assert_eq!(h.aligned.chars, 0..3);
    // A typo inside the prefix ("s" for "a", neighbouring keys).
    let h = plain(&["javascript"], "javs", "javascript", HighlightMode::Prefix);
    assert_eq!(h.cost, 8);
    assert_eq!(parts(&h, "javascript"), ["jav"]);
    assert_eq!(h.aligned.chars, 0..4);
    // The empty query aligns with the empty prefix.
    let h = plain(&["javascript"], "", "javascript", HighlightMode::Prefix);
    assert!(h.ranges.is_empty());
    assert_eq!(h.aligned.chars, 0..0);
}

#[test]
fn a_partially_typed_expansion_counts_as_typed() {
    let n = Normalizer::new();
    // "ß" -> "ss"; one "s" typed.
    let h = normalised(&["Straße"], n, "strase", "Straße");
    assert_eq!(h.cost, 8);
    assert_eq!(parts(&h, "Straße"), ["Straße"]);
    // Neither "s" typed ("x" is not a neighbour of "s"): "ß" is not highlighted.
    let h = normalised(&["Straße"], n, "straxxe", "Straße");
    assert_eq!(parts(&h, "Straße"), ["Stra", "e"]);
    assert_eq!(h.ranges[1].utf8, 6..7);
}

#[test]
fn a_dropped_combining_mark_follows_its_letter() {
    let n = Normalizer::new();
    // "e" + U+0301: the mark produces no symbol and belongs to the "e".
    let src = "cafe\u{301}s";
    let h = normalised(&[src], n, "cafes", src);
    assert_eq!(chars(&h), [(0, 6)]);
    // The "e" mistyped: its mark is not highlighted either.
    let h = normalised(&[src], n, "cafrs", src);
    assert_eq!(chars(&h), [(0, 3), (5, 6)]);
    assert_eq!(h.ranges[1].utf8, 6..7);
    // A mark before any letter produces nothing and is never highlighted.
    let src = "\u{301}abc";
    let h = normalised(&[src], n, "abc", src);
    assert_eq!(chars(&h), [(1, 4)]);
    assert_eq!(h.ranges[0].utf8, 2..5);
    assert_eq!(h.aligned.chars, 1..4);
}

#[test]
fn utf16_offsets_count_astral_characters_twice() {
    // Emoji are one code point, four bytes, two UTF-16 units.
    let term = "😀ab😀c";
    let h = plain(&[term], "😀ab😀c", term, HighlightMode::Whole);
    assert_eq!(
        h.ranges,
        [MatchRange {
            chars: 0..5,
            utf8: 0..11,
            utf16: 0..7
        }]
    );
    // "x" for "b": the range splits around it.
    let h = plain(&[term], "😀ax😀c", term, HighlightMode::Whole);
    assert_eq!(
        h.ranges,
        [
            MatchRange {
                chars: 0..2,
                utf8: 0..5,
                utf16: 0..3
            },
            MatchRange {
                chars: 3..5,
                utf8: 6..11,
                utf16: 4..7
            }
        ]
    );
    // The emoji itself mistyped: the gap is two UTF-16 units wide.
    let h = plain(&[term], "😀abxc", term, HighlightMode::Whole);
    assert_eq!(h.ranges[0].utf16, 0..4);
    assert_eq!(h.ranges[1].utf16, 6..7);
    let js: Vec<u16> = term.encode_utf16().collect();
    assert_eq!(
        String::from_utf16(&js[h.ranges[1].utf16.clone()]).unwrap(),
        "c"
    );
}

#[test]
fn errors_are_typed() {
    let trie = Trie::build(&[("hello", 1), ("help", 1)]).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let cfg = SearchConfig::default();
    let out = s.search(&trie, &cm, b"helo", &cfg).unwrap();
    let hit = out.hits[0];
    let term = trie.term(hit.id);
    let whole = HighlightMode::Whole;
    let bad = Hit { id: 99, ..hit };
    assert_eq!(
        s.highlight(&trie, &cm, b"helo", &bad, term, whole),
        Err(HighlightError::UnknownTerm { id: 99 })
    );
    assert_eq!(
        s.highlight(&trie, &cm, b"helo", &hit, "Hello", whole),
        Err(HighlightError::SourceMismatch)
    );
    let wrong = Hit {
        cost: hit.cost + 1,
        ..hit
    };
    assert!(matches!(
        s.highlight(&trie, &cm, b"helo", &wrong, term, whole),
        Err(HighlightError::CostMismatch { found: Some(c), .. }) if c == hit.cost
    ));
    // Another query.
    assert!(matches!(
        s.highlight(&trie, &cm, b"help", &hit, term, whole),
        Err(HighlightError::CostMismatch { .. })
    ));
    let long = vec![b'a'; MAX_QUERY_LEN + 1];
    assert_eq!(
        s.highlight(&trie, &cm, &long, &hit, term, whole),
        Err(HighlightError::QueryTooLong {
            len: MAX_QUERY_LEN + 1,
            max: MAX_QUERY_LEN
        })
    );
    // A whole-term hit in prefix mode: "hel" is a cheaper prefix of "hello".
    let out = s.search(&trie, &cm, b"hel", &cfg).unwrap();
    let h = out
        .hits
        .iter()
        .find(|h| trie.term(h.id) == "hello")
        .unwrap();
    assert!(matches!(
        s.highlight(&trie, &cm, b"hel", h, "hello", HighlightMode::Prefix),
        Err(HighlightError::CostMismatch { .. })
    ));
    // A normalised trie wants the original, and the plain method does not
    // normalise the query.
    let items = [("Hello", 1)];
    let nt = Trie::build_normalized(&items, &Normalizer::new()).unwrap();
    let out = s.search_text(&nt, &cm, "HELO", &cfg).unwrap();
    let hit = out.hits[0];
    assert!(
        s.highlight_text(&nt, &cm, "HELO", &hit, "Hello", whole)
            .is_ok()
    );
    assert!(
        s.highlight_text(&nt, &cm, "HELO", &hit, "HELLO", whole)
            .is_ok()
    );
    assert_eq!(
        s.highlight_text(&nt, &cm, "HELO", &hit, "Help", whole),
        Err(HighlightError::SourceMismatch)
    );
    assert!(matches!(
        s.highlight(&nt, &cm, b"HELO", &hit, "Hello", whole),
        Err(HighlightError::CostMismatch { .. })
    ));
    // Errors display.
    for e in [
        HighlightError::SourceMismatch,
        HighlightError::UnknownTerm { id: 3 },
        HighlightError::CostMismatch {
            expected: 1,
            found: None,
        },
    ] {
        assert!(!e.to_string().is_empty());
    }
}

/// The range invariants of `docs/design/highlighting.md` section 2.2.
fn check_ranges(h: &Highlight, source: &str) {
    let n_chars = source.chars().count();
    let n16 = source.encode_utf16().count();
    let all = h.ranges.iter().chain(core::iter::once(&h.aligned));
    for r in all {
        assert!(r.chars.start <= r.chars.end && r.chars.end <= n_chars);
        assert_eq!(
            text::utf8_range(source, r.chars.clone()),
            Some(r.utf8.clone())
        );
        assert_eq!(
            text::utf16_range(source, r.chars.clone()),
            Some(r.utf16.clone())
        );
        assert!(source.is_char_boundary(r.utf8.start) && source.is_char_boundary(r.utf8.end));
        assert!(r.utf16.end <= n16);
    }
    for r in &h.ranges {
        assert!(r.chars.start < r.chars.end, "empty range");
        assert!(
            r.chars.start >= h.aligned.chars.start && r.chars.end <= h.aligned.chars.end,
            "{h:?} {source:?}"
        );
    }
    for w in h.ranges.windows(2) {
        assert!(w[0].chars.end < w[1].chars.start, "ranges touch or overlap");
    }
}

/// Characters of the property test: accents, `ß`, a combining mark, case
/// pairs, Cyrillic and emoji (astral).
const ALPHABET: [char; 14] = [
    'a', 'b', 'c', 'e', 'é', 'É', 'ç', 'Ç', 'ß', 's', '\u{301}', 'ж', '😀', '🎉',
];

fn word(rng: &mut Rng, max: u64) -> String {
    (0..1 + rng.below(max))
        .map(|_| ALPHABET[rng.below(ALPHABET.len() as u64) as usize])
        .collect()
}

/// A few random edits of `w` (by characters).
fn edit(rng: &mut Rng, w: &str) -> String {
    let mut c: Vec<char> = w.chars().collect();
    for _ in 0..rng.below(3) {
        let x = ALPHABET[rng.below(ALPHABET.len() as u64) as usize];
        let n = c.len() as u64;
        match rng.below(4) {
            0 if n > 1 => {
                let i = rng.below(n - 1) as usize;
                c.swap(i, i + 1);
            }
            1 => c.insert(rng.below(n + 1) as usize, x),
            2 if n > 0 => {
                c.remove(rng.below(n) as usize);
            }
            3 if n > 0 => c[rng.below(n) as usize] = x,
            _ => {}
        }
    }
    c.into_iter().collect()
}

const MODES: [Normalizer; 4] = [
    Normalizer::new(),
    Normalizer::new().with_diacritic_folding(false),
    Normalizer::new().with_case_folding(false),
    Normalizer::new()
        .with_case_folding(false)
        .with_diacritic_folding(false),
];

/// Random non-ASCII dictionaries, all normaliser modes and layouts, both
/// modes: every hit highlights, its cost equals the oracle's, and the ranges
/// hold their invariants in all three units.
#[test]
fn every_hit_highlights_with_the_oracle_cost_and_valid_ranges() {
    let rounds = if cfg!(miri) { 4 } else { 1_500 };
    let mut rng = Rng::new(32);
    let (mut checked, mut astral, mut split) = (0, 0, 0);
    for round in 0..rounds {
        let words: Vec<String> = (0..1 + rng.below(10)).map(|_| word(&mut rng, 7)).collect();
        let items: Vec<(&str, u16)> = words.iter().map(|w| (w.as_str(), 1)).collect();
        let n = MODES[round % MODES.len()];
        let cm = CostModel::for_layout(Layout::ALL[round % Layout::ALL.len()]);
        let Ok(trie) = Trie::build_normalized(&items, &n) else {
            continue;
        };
        let mut s = Searcher::new();
        for _ in 0..4 {
            let base = rng.below(words.len() as u64) as usize;
            let q = edit(&mut rng, &words[base]);
            let nq = n.normalize(&q);
            let cfg = SearchConfig {
                k: 20,
                budget: 64,
                ..SearchConfig::default()
            };
            for mode in [HighlightMode::Whole, HighlightMode::Prefix] {
                let out = match mode {
                    HighlightMode::Whole => s.search_text(&trie, &cm, &q, &cfg),
                    _ => s.search_prefix_text(&trie, &cm, &q, &cfg),
                }
                .unwrap();
                for hit in &out.hits {
                    let source = items[trie.input_index(hit.id) as usize].0;
                    let h = s
                        .highlight_text(&trie, &cm, &q, hit, source, mode)
                        .unwrap_or_else(|e| panic!("{e}: q={q:?} source={source:?} {n:?}"));
                    let t = trie.term(hit.id).as_bytes();
                    let want = match mode {
                        HighlightMode::Whole => oracle_cost(&cm, nq.as_bytes(), t),
                        _ => oracle_prefix_cost(&cm, nq.as_bytes(), t),
                    };
                    assert_eq!(u32::from(h.cost), want);
                    assert_eq!(h.cost, hit.cost);
                    check_ranges(&h, source);
                    checked += 1;
                    astral += usize::from(h.ranges.iter().any(|r| r.utf16.len() > r.chars.len()));
                    split += usize::from(h.ranges.len() > 1);
                }
            }
        }
    }
    eprintln!("{checked} hits checked, {astral} with astral characters highlighted, {split} split");
    if !cfg!(miri) {
        assert!(checked > 30_000 && astral > 5_000 && split > 2_500);
    }
}
