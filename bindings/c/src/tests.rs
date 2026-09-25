//! Rust-side tests of the C entry points, including every error path.
//! Unsafe blocks here call the `extern "C"` functions the way C would.

#![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]

use core::ffi::CStr;
use core::ptr;

use super::*;

fn entry(t: &str, w: u16) -> kh_entry {
    kh_entry {
        term: t.as_ptr(),
        len: t.len(),
        weight: w,
    }
}

fn last_error() -> String {
    // SAFETY: `kh_last_error` returns a valid NUL-terminated string.
    unsafe { CStr::from_ptr(kh_last_error()) }
        .to_str()
        .unwrap()
        .to_owned()
}

fn build(entries: &[kh_entry]) -> *mut kh_index {
    let mut idx = ptr::null_mut();
    // SAFETY: valid slice of valid entries and a valid out pointer.
    let s = unsafe { kh_index_build(entries.as_ptr(), entries.len(), &mut idx) };
    assert_eq!(s, kh_status::KH_OK);
    assert!(!idx.is_null());
    idx
}

fn terms(r: &kh_results) -> Vec<String> {
    (0..r.len)
        .map(|i| {
            // SAFETY: `hits` has `len` hits whose terms point into the index.
            let h = unsafe { *r.hits.add(i) };
            let b = unsafe { core::slice::from_raw_parts(h.term, h.term_len) };
            String::from_utf8(b.to_vec()).unwrap()
        })
        .collect()
}

fn search(idx: *const kh_index, q: &[u8], cfg: *const kh_config) -> (kh_status, kh_results) {
    let mut r = EMPTY_RESULTS;
    // SAFETY: `q` is a valid slice, `r` a valid out struct, `cfg` null or valid.
    let s = unsafe { kh_search(idx, q.as_ptr(), q.len(), cfg, &mut r) };
    (s, r)
}

#[test]
fn abi_version_and_status_strings() {
    assert_eq!(kh_abi_version(), KH_ABI_VERSION);
    let s = unsafe { CStr::from_ptr(kh_status_string(kh_status::KH_ERR_INVALID_UTF8 as i32)) };
    assert_eq!(s.to_str().unwrap(), "invalid UTF-8");
    let s = unsafe { CStr::from_ptr(kh_status_string(12345)) };
    assert_eq!(s.to_str().unwrap(), "unknown status");
    assert_eq!(KH_MAX_QUERY_LEN, keyhammer::search::MAX_QUERY_LEN);
}

#[test]
fn build_search_free() {
    let mut idx = build(&[entry("hello", 5), entry("help", 9), entry("world", 1)]);
    let mut n = 0usize;
    assert_eq!(unsafe { kh_index_len(idx, &mut n) }, kh_status::KH_OK);
    assert_eq!(n, 3);
    let (s, mut r) = search(idx, b"helo", ptr::null());
    assert_eq!(s, kh_status::KH_OK);
    assert!(r.len >= 2);
    assert_eq!(last_error(), "");
    let t = terms(&r);
    assert!(t.contains(&"hello".to_owned()) && t.contains(&"help".to_owned()));
    unsafe { kh_results_free(&mut r) };
    assert!(r.hits.is_null() && r.len == 0);
    // Freeing again is a no-op.
    unsafe { kh_results_free(&mut r) };
    unsafe { kh_index_free(&mut idx) };
    assert!(idx.is_null());
    // So is freeing the nulled handle again.
    unsafe { kh_index_free(&mut idx) };
    unsafe { kh_index_free(ptr::null_mut()) };
    unsafe { kh_results_free(ptr::null_mut()) };
}

#[test]
fn duplicates_after_folding_keep_the_highest_weight() {
    let mut idx = build(&[entry("Hello", 1), entry("hello", 7)]);
    let (s, mut r) = search(idx, b"HELLO", ptr::null());
    assert_eq!(s, kh_status::KH_OK);
    assert_eq!(terms(&r), ["hello"]);
    let h = unsafe { *r.hits };
    assert_eq!((h.cost, h.weight), (0, 7));
    unsafe { kh_results_free(&mut r) };
    unsafe { kh_index_free(&mut idx) };
}

#[test]
fn config_is_honoured() {
    let mut idx = build(&[entry("aaa", 1), entry("aab", 1), entry("aac", 1)]);
    let mut cfg = kh_config::defaults();
    cfg.k = 0;
    assert_eq!(unsafe { kh_config_default(&mut cfg) }, kh_status::KH_OK);
    assert_eq!(
        (cfg.k, cfg.budget, cfg.ranking),
        (10, 32, KH_RANKING_COARSE)
    );
    cfg.k = 1;
    cfg.ranking = KH_RANKING_EXACT;
    cfg.tsb = 1;
    let (s, mut r) = search(idx, b"aaa", &cfg);
    assert_eq!(s, kh_status::KH_OK);
    assert_eq!(r.len, 1);
    unsafe { kh_results_free(&mut r) };
    cfg.k = 0;
    let (s, r) = search(idx, b"aaa", &cfg);
    assert_eq!((s, r.len), (kh_status::KH_OK, 0));
    assert!(r.hits.is_null());
    cfg.k = 5;
    cfg.max_nodes = 1;
    let (s, mut r) = search(idx, b"zzz", &cfg);
    assert_eq!(s, kh_status::KH_OK);
    assert_eq!(r.truncated, 1);
    unsafe { kh_results_free(&mut r) };
    unsafe { kh_index_free(&mut idx) };
}

#[test]
fn build_errors() {
    let mut idx: *mut kh_index = ptr::null_mut();
    let e = [entry("ok", 1)];
    unsafe {
        assert_eq!(
            kh_index_build(e.as_ptr(), 1, ptr::null_mut()),
            kh_status::KH_ERR_NULL_POINTER
        );
        assert_eq!(
            kh_index_build(ptr::null(), 1, &mut idx),
            kh_status::KH_ERR_NULL_POINTER
        );
        assert!(idx.is_null());
        assert_eq!(
            kh_index_build(e.as_ptr(), 0, &mut idx),
            kh_status::KH_ERR_EMPTY_INDEX
        );
        assert_eq!(
            kh_index_build(e.as_ptr(), usize::MAX, &mut idx),
            kh_status::KH_ERR_INVALID_LENGTH
        );
        assert!(!last_error().is_empty());
    }
    // Empty term, null term with a length, oversize term length, too long term.
    let empty = [kh_entry {
        term: ptr::null(),
        len: 0,
        weight: 0,
    }];
    assert_eq!(
        unsafe { kh_index_build(empty.as_ptr(), 1, &mut idx) },
        kh_status::KH_ERR_EMPTY_TERM
    );
    let null_term = [kh_entry {
        term: ptr::null(),
        len: 3,
        weight: 0,
    }];
    assert_eq!(
        unsafe { kh_index_build(null_term.as_ptr(), 1, &mut idx) },
        kh_status::KH_ERR_NULL_POINTER
    );
    let huge = [kh_entry {
        term: b"x".as_ptr(),
        len: usize::MAX,
        weight: 0,
    }];
    assert_eq!(
        unsafe { kh_index_build(huge.as_ptr(), 1, &mut idx) },
        kh_status::KH_ERR_INVALID_LENGTH
    );
    let long = "a".repeat(65536);
    assert_eq!(
        unsafe { kh_index_build([entry(&long, 0)].as_ptr(), 1, &mut idx) },
        kh_status::KH_ERR_TERM_TOO_LONG
    );
    let bad_bytes = [0xffu8, 0xfe];
    let bad = [kh_entry {
        term: bad_bytes.as_ptr(),
        len: 2,
        weight: 0,
    }];
    assert_eq!(
        unsafe { kh_index_build(bad.as_ptr(), 1, &mut idx) },
        kh_status::KH_ERR_INVALID_UTF8
    );
    assert!(last_error().contains("entry 0"));
    assert!(idx.is_null());
}

#[test]
fn search_errors() {
    let mut idx = build(&[entry("hello", 1)]);
    let q = b"hello";
    let bad_utf8 = [0xc3u8, 0x28];
    let mut r = EMPTY_RESULTS;
    unsafe {
        assert_eq!(
            kh_search(idx, q.as_ptr(), 5, ptr::null(), ptr::null_mut()),
            kh_status::KH_ERR_NULL_POINTER
        );
        assert_eq!(
            kh_search(ptr::null(), q.as_ptr(), 5, ptr::null(), &mut r),
            kh_status::KH_ERR_NULL_POINTER
        );
        assert_eq!(
            kh_search(idx, ptr::null(), 5, ptr::null(), &mut r),
            kh_status::KH_ERR_NULL_POINTER
        );
        assert_eq!(
            kh_search(idx, q.as_ptr(), usize::MAX, ptr::null(), &mut r),
            kh_status::KH_ERR_QUERY_TOO_LONG
        );
        assert_eq!(
            kh_search(idx, bad_utf8.as_ptr(), 2, ptr::null(), &mut r),
            kh_status::KH_ERR_INVALID_UTF8
        );
        // A null query with length 0 is the empty query, which is valid.
        assert_eq!(
            kh_search(idx, ptr::null(), 0, ptr::null(), &mut r),
            kh_status::KH_OK
        );
        kh_results_free(&mut r);
    }
    let long = vec![b'a'; KH_MAX_QUERY_LEN + 1];
    assert_eq!(
        search(idx, &long, ptr::null()).0,
        kh_status::KH_ERR_QUERY_TOO_LONG
    );
    let mut cfg = kh_config::defaults();
    cfg.ranking = 7;
    assert_eq!(search(idx, q, &cfg).0, kh_status::KH_ERR_INVALID_ARGUMENT);
    cfg.ranking = 0;
    cfg.reserved = 1;
    assert_eq!(search(idx, q, &cfg).0, kh_status::KH_ERR_INVALID_ARGUMENT);
    assert!(last_error().contains("reserved"));
    cfg.reserved = 0;
    cfg.budget = 65;
    assert_eq!(search(idx, q, &cfg).0, kh_status::KH_ERR_INVALID_ARGUMENT);
    cfg.budget = 70_000;
    assert_eq!(search(idx, q, &cfg).0, kh_status::KH_ERR_INVALID_ARGUMENT);
    cfg.budget = 32;
    cfg.struct_size = 4;
    assert_eq!(search(idx, q, &cfg).0, kh_status::KH_ERR_INVALID_ARGUMENT);
    assert!(last_error().contains("struct_size"));
    // A larger struct_size (a newer header) is accepted.
    cfg.struct_size = 4096;
    let (s, mut r) = search(idx, q, &cfg);
    assert_eq!(s, kh_status::KH_OK);
    unsafe { kh_results_free(&mut r) };
    // A failed search leaves an empty, freeable result.
    let (s, mut r) = search(idx, &long, ptr::null());
    assert_ne!(s, kh_status::KH_OK);
    assert!(r.hits.is_null() && r.len == 0);
    unsafe { kh_results_free(&mut r) };
    unsafe { kh_index_free(&mut idx) };
}

#[test]
fn other_null_checks() {
    assert_eq!(
        unsafe { kh_config_default(ptr::null_mut()) },
        kh_status::KH_ERR_NULL_POINTER
    );
    let mut n = 0usize;
    assert_eq!(
        unsafe { kh_index_len(ptr::null(), &mut n) },
        kh_status::KH_ERR_NULL_POINTER
    );
}

#[test]
fn layout_has_no_implicit_padding() {
    assert_eq!(size_of::<kh_config>(), 32);
    assert_eq!(core::mem::offset_of!(kh_config, reserved) + 4, 32);
    // kh_hit: input_index is the last field and ends the struct exactly on
    // 64-bit targets (it fills what was tail padding in version 1).
    assert_eq!(
        core::mem::offset_of!(kh_hit, input_index),
        2 * size_of::<usize>() + 12
    );
}

#[test]
fn nul_bytes_in_terms_are_kept() {
    let mut idx = build(&[entry("a b", 1)]);
    let (s, mut r) = search(idx, b"a b", ptr::null());
    assert_eq!(s, kh_status::KH_OK);
    assert_eq!(terms(&r), ["a b"]);
    unsafe { kh_results_free(&mut r) };
    unsafe { kh_index_free(&mut idx) };
}

#[test]
fn a_panic_is_caught_and_reported() {
    // Silence the default hook's stderr output for the deliberate panic.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let s = guard(|| panic!("boom"));
    std::panic::set_hook(prev);
    assert_eq!(s, kh_status::KH_ERR_INTERNAL);
    assert!(last_error().contains("panic"));
}

#[test]
fn concurrent_searches_share_an_index() {
    struct SendPtr(*const kh_index);
    // SAFETY: the index is immutable and outlives the threads (joined below).
    unsafe impl Send for SendPtr {}
    let mut idx = build(&[entry("alpha", 1), entry("alpine", 2), entry("beta", 3)]);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let p = SendPtr(idx);
            std::thread::spawn(move || {
                let p = p;
                for _ in 0..50 {
                    let (s, mut r) = search(p.0, b"alpah", ptr::null());
                    assert_eq!(s, kh_status::KH_OK);
                    assert!(r.len >= 1);
                    unsafe { kh_results_free(&mut r) };
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    unsafe { kh_index_free(&mut idx) };
}

// ---- Unicode normalisation (issue #67) ----

/// A hit as `(input_index, cost, weight, term)`.
fn hits(r: &kh_results) -> Vec<(u32, u32, u32, String)> {
    let t = terms(r);
    (0..r.len)
        .map(|i| {
            // SAFETY: `hits` has `len` hits.
            let h = unsafe { *r.hits.add(i) };
            (h.input_index, h.cost, h.weight, t[i].clone())
        })
        .collect()
}

fn search_hits(
    idx: *const kh_index,
    q: &str,
    cfg: *const kh_config,
) -> Vec<(u32, u32, u32, String)> {
    let (s, mut r) = search(idx, q.as_bytes(), cfg);
    assert_eq!(s, kh_status::KH_OK, "{q:?}: {}", last_error());
    let h = hits(&r);
    unsafe { kh_results_free(&mut r) };
    h
}

#[test]
fn abi_is_version_2_with_input_index() {
    assert_eq!(KH_ABI_VERSION, 2);
    // kh_hit is two words and four u32 with no padding, on 32 and 64 bits.
    let words = 2 * size_of::<usize>() + 4 * size_of::<u32>();
    assert_eq!(size_of::<kh_hit>(), words);
    assert_eq!(KH_MAX_QUERY_BYTES, 4 * KH_MAX_QUERY_LEN);
}

#[test]
fn hits_return_the_original_text_and_input_index() {
    let items = [
        entry("São Paulo", 5),
        entry("coração", 3),
        entry("Ação", 1),
        entry("ação", 8),
        entry("Ç", 2),
        entry("Straße", 4),
    ];
    let mut idx = build(&items);
    let mut n = 0usize;
    unsafe { kh_index_len(idx, &mut n) };
    assert_eq!(n, 5, "Ação and ação merge");
    for (q, want) in [
        ("sao paulo", (0, "São Paulo")),
        ("SÃO PAULO", (0, "São Paulo")),
        ("São Paulo", (0, "São Paulo")),
        ("CORACAO", (1, "coração")),
        ("ACAO", (3, "ação")), // the entry with the higher weight is kept
        ("Ação", (3, "ação")),
        ("c", (4, "Ç")),
        ("STRASSE", (5, "Straße")),
        ("straße", (5, "Straße")),
    ] {
        let h = search_hits(idx, q, ptr::null());
        assert_eq!((h[0].0, h[0].3.as_str()), want, "{q}");
        assert_eq!(h[0].1, 0, "{q}: an exact match after folding costs 0");
    }
    // A typo is one edit over the folded text.
    let h = search_hits(idx, "sao paolo", ptr::null());
    assert_eq!((h[0].3.as_str(), h[0].1), ("São Paulo", 16));
    unsafe { kh_index_free(&mut idx) };
}

#[test]
fn normalisation_flags() {
    let items = [entry("Café", 1), entry("cafe", 1), entry("CAFE", 1)];
    let build_ex = |flags: u32| {
        let mut idx = ptr::null_mut();
        let s = unsafe { kh_index_build_ex(items.as_ptr(), items.len(), flags, &mut idx) };
        (s, idx)
    };
    // Default: all three are one term.
    let (s, mut idx) = build_ex(0);
    assert_eq!(s, kh_status::KH_OK);
    let mut n = 0usize;
    unsafe { kh_index_len(idx, &mut n) };
    assert_eq!(n, 1);
    unsafe { kh_index_free(&mut idx) };
    // Keep case: the three differ by case only.
    let (s, mut idx) = build_ex(KH_NORM_KEEP_CASE);
    assert_eq!(s, kh_status::KH_OK);
    unsafe { kh_index_len(idx, &mut n) };
    assert_eq!(n, 3); // "Cafe" (accent folded), "cafe" and "CAFE"
    let h = search_hits(idx, "CAFE", ptr::null());
    assert_eq!((h[0].3.as_str(), h[0].1), ("CAFE", 0));
    unsafe { kh_index_free(&mut idx) };
    // Keep diacritics: é differs from e.
    let (s, mut idx) = build_ex(KH_NORM_KEEP_DIACRITICS);
    assert_eq!(s, kh_status::KH_OK);
    unsafe { kh_index_len(idx, &mut n) };
    assert_eq!(n, 2); // "café" and "cafe"
    let h = search_hits(idx, "CAFÉ", ptr::null());
    assert_eq!((h[0].3.as_str(), h[0].1), ("Café", 0));
    unsafe { kh_index_free(&mut idx) };
    // Unknown flags are refused and no handle is returned.
    let (s, idx) = build_ex(4);
    assert_eq!(s, kh_status::KH_ERR_INVALID_ARGUMENT);
    assert!(idx.is_null());
    assert!(last_error().contains("flags"));
}

#[test]
fn query_limit_counts_code_points_after_normalisation() {
    let mut idx = build(&[entry("é", 1)]);
    // 128 two-byte code points: 256 bytes, accepted.
    let ok = "é".repeat(KH_MAX_QUERY_LEN);
    assert_eq!(search(idx, ok.as_bytes(), ptr::null()).0, kh_status::KH_OK);
    let over = "é".repeat(KH_MAX_QUERY_LEN + 1);
    assert_eq!(
        search(idx, over.as_bytes(), ptr::null()).0,
        kh_status::KH_ERR_QUERY_TOO_LONG
    );
    assert!(last_error().contains("code points"));
    // 65 sharp s fold to 130 letters.
    let sharp = "ß".repeat(65);
    assert_eq!(
        search(idx, sharp.as_bytes(), ptr::null()).0,
        kh_status::KH_ERR_QUERY_TOO_LONG
    );
    // 200 code points as given, 100 once the accents are dropped: accepted.
    let decomposed = "e\u{301}".repeat(100);
    assert_eq!(
        search(idx, decomposed.as_bytes(), ptr::null()).0,
        kh_status::KH_OK
    );
    // 128 four-byte code points is exactly KH_MAX_QUERY_BYTES: accepted.
    let wide = "😀".repeat(KH_MAX_QUERY_LEN);
    assert_eq!(wide.len(), KH_MAX_QUERY_BYTES);
    assert_eq!(
        search(idx, wide.as_bytes(), ptr::null()).0,
        kh_status::KH_OK
    );
    // Beyond the byte limit it is refused before the bytes are looked at.
    let junk = vec![0xffu8; KH_MAX_QUERY_BYTES + 1];
    assert_eq!(
        search(idx, &junk, ptr::null()).0,
        kh_status::KH_ERR_QUERY_TOO_LONG
    );
    // Within the limit, invalid UTF-8 is still an error.
    assert_eq!(
        search(idx, &junk[..8], ptr::null()).0,
        kh_status::KH_ERR_INVALID_UTF8
    );
    unsafe { kh_index_free(&mut idx) };
}

#[test]
fn invalid_utf8_and_empty_normalised_terms_are_errors() {
    let bad = [0xc3u8, 0x28];
    let mut idx: *mut kh_index = ptr::null_mut();
    let e = [kh_entry {
        term: bad.as_ptr(),
        len: 2,
        weight: 1,
    }];
    assert_eq!(
        unsafe { kh_index_build(e.as_ptr(), 1, &mut idx) },
        kh_status::KH_ERR_INVALID_UTF8
    );
    assert!(idx.is_null());
    // A lone combining acute accent folds to nothing.
    let e = [entry("ok", 1), entry("\u{301}\u{302}", 1)];
    assert_eq!(
        unsafe { kh_index_build(e.as_ptr(), 2, &mut idx) },
        kh_status::KH_ERR_EMPTY_TERM
    );
    assert!(idx.is_null());
    assert!(last_error().contains("entry 1: term normalises to nothing"));
    // With the diacritic folding off nothing folds to nothing.
    let s = unsafe { kh_index_build_ex(e.as_ptr(), 2, KH_NORM_KEEP_DIACRITICS, &mut idx) };
    assert_eq!(s, kh_status::KH_OK);
    unsafe { kh_index_free(&mut idx) };
}

const DICT: &str = include_str!("../../testdata/unicode_dict.tsv");
const CASES: &str = include_str!("../../testdata/unicode_cases.tsv");
const EXPECTED: &str = include_str!("../../testdata/unicode_expected.tsv");
#[cfg(not(miri))]
const EXPECTED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../testdata/unicode_expected.tsv"
);

fn dict() -> Vec<(&'static str, u16)> {
    DICT.lines()
        .map(|l| {
            let (t, w) = l.split_once('\t').unwrap();
            (t, w.parse().unwrap())
        })
        .collect()
}

fn cases() -> Vec<(&'static str, u16, Ranking)> {
    CASES
        .lines()
        .map(|l| {
            let mut f = l.split('\t');
            let q = f.next().unwrap();
            let budget = f.next().unwrap().parse().unwrap();
            let ranking = match f.next().unwrap() {
                "coarse" => Ranking::Coarse,
                "exact" => Ranking::Exact,
                other => panic!("bad ranking {other}"),
            };
            (q, budget, ranking)
        })
        .collect()
}

/// The shared golden file, computed by the core alone: one line per case,
/// `case TAB input_index:cost:weight,...`. Every binding's tests read this
/// file. Regenerate it with `UPDATE_GOLDEN=1 cargo test -p keyhammer-c golden`.
fn core_golden() -> String {
    let items = dict();
    let trie = Trie::build_normalized(&items, &Normalizer::new()).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let mut out = String::new();
    for (i, (q, budget, ranking)) in cases().into_iter().enumerate() {
        let cfg = SearchConfig {
            budget,
            ranking,
            ..SearchConfig::default()
        };
        let o = s.search_text(&trie, &cm, q, &cfg).unwrap();
        let hits: Vec<String> = o
            .hits
            .iter()
            .map(|h| format!("{}:{}:{}", trie.input_index(h.id), h.cost, h.weight))
            .collect();
        out.push_str(&format!("{i}\t{}\n", hits.join(",")));
    }
    out
}

#[test]
fn golden_file_is_the_cores_output() {
    let golden = core_golden();
    // Miri has no file system access; the file is compared as compiled in.
    #[cfg(not(miri))]
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(EXPECTED_PATH, &golden).unwrap();
        return;
    }
    assert_eq!(
        EXPECTED.replace("\r\n", "\n"),
        golden,
        "bindings/testdata/unicode_expected.tsv is stale: UPDATE_GOLDEN=1 cargo test -p keyhammer-c golden"
    );
    assert!(golden.lines().any(|l| l.contains(':')), "some hits");
}

#[test]
fn the_c_abi_matches_the_core_on_the_shared_dictionary() {
    let items = dict();
    let entries: Vec<kh_entry> = items.iter().map(|&(t, w)| entry(t, w)).collect();
    let mut idx = build(&entries);
    let golden = core_golden();
    for (i, ((q, budget, ranking), line)) in cases().into_iter().zip(golden.lines()).enumerate() {
        let mut cfg = kh_config::defaults();
        cfg.budget = u32::from(budget);
        cfg.ranking = match ranking {
            Ranking::Exact => KH_RANKING_EXACT,
            _ => KH_RANKING_COARSE,
        };
        let got: Vec<String> = search_hits(idx, q, &cfg)
            .iter()
            .map(|(ii, c, w, term)| {
                // The hit carries the caller's original text.
                assert_eq!(term, items[*ii as usize].0, "case {i} {q:?}");
                format!("{ii}:{c}:{w}")
            })
            .collect();
        let want = line.split_once('\t').unwrap().1;
        assert_eq!(got.join(","), want, "case {i}: {q:?}");
    }
    unsafe { kh_index_free(&mut idx) };
}
