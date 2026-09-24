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
fn ascii_is_lowercased_and_duplicates_keep_highest_weight() {
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
            kh_status::KH_ERR_INVALID_LENGTH
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
