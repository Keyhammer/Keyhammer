// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! A global allocator that wraps `System` and counts the bytes requested by
//! live allocations (and their peak). Allocator overhead (headers, rounding
//! to size classes, fragmentation) is not counted.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn add(n: usize) {
    let now = LIVE.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

fn sub(n: usize) {
    LIVE.fetch_sub(n, Ordering::Relaxed);
}

// SAFETY: every call is forwarded unchanged to `System`; the counters only
// observe sizes and never touch the memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: same contract as the caller's.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            add(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: same contract as the caller's.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            add(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) };
        sub(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: same contract as the caller's.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                add(new_size - layout.size());
            } else {
                sub(layout.size() - new_size);
            }
        }
        p
    }
}

/// Bytes requested by live allocations right now.
pub fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

/// Resets the peak to the current live bytes.
pub fn reset_peak() {
    PEAK.store(live(), Ordering::Relaxed);
}

/// Highest live byte count since the last [`reset_peak`].
pub fn peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}
