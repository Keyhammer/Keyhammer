# The C ABI

`bindings/c` (crate `keyhammer-c`, built as `cdylib` and `staticlib`) exports a
small C interface over the core. It is meant as the one place where other
languages (.NET, Go, Java, C++) attach: they wrap these functions instead of
each binding the Rust API. The generated header is
`bindings/c/include/keyhammer.h`. Status: ABI version 1, unpublished (the
crate has `publish = false`); nothing below is a promise about releases yet,
but the rules apply to every change from now on.

## Shape

```c
kh_index *idx = NULL;
kh_entry e[] = { {(const uint8_t *)"hello", 5, 10}, ... };
if (kh_index_build(e, n, &idx) != KH_OK) fprintf(stderr, "%s\n", kh_last_error());

kh_config cfg; kh_config_default(&cfg); cfg.k = 5;
kh_results res;
if (kh_search(idx, (const uint8_t *)"helo", 4, &cfg, &res) == KH_OK) {
    for (size_t i = 0; i < res.len; i++) { /* res.hits[i].term, .term_len, .cost, .weight */ }
    kh_results_free(&res);
}
kh_index_free(&idx);
```

- **Handle-based.** `kh_index` is opaque. The caller never sees its layout.
- **No callbacks, no closures, no global state** other than the per-thread
  last-error message.
- **Embedded NUL bytes** in a term or query are ordinary bytes: they are
  accepted and returned unchanged. Terms are length-delimited, not C strings;
  a wrapper that turns a hit into a C string must use `term_len`.
- **UTF-8 only**, passed as `(pointer, length)`; never NUL-terminated, invalid
  UTF-8 is an error (`KH_ERR_INVALID_UTF8`). ASCII letters are lower-cased by
  this layer (the engine currently expects `a-z`), so hits return lower-cased
  terms. Bytes outside `a-z` are compared verbatim, as in the core.
- **Plain results.** `kh_results` is a struct with an array of `kh_hit`. A
  hit's `term` points into the index (no copy) and is valid until the index is
  freed; `kh_results_free` frees only the array.
- **Errors.** Every fallible function returns a `kh_status`; `KH_OK` is 0.
  `kh_last_error()` gives a message for the calling thread (valid until the
  next call into the library on that thread; empty after a success).
  `kh_status_string(int32_t)` gives a static description of a code.

## Validation and panics

Every entry point checks its arguments before touching them: null pointers
(`KH_ERR_NULL_POINTER`, except a null pointer with length 0, which is the empty
string), entry counts or term lengths above `isize::MAX` in `kh_index_build` (`KH_ERR_INVALID_LENGTH`; `kh_search` reports an oversize query as `KH_ERR_QUERY_TOO_LONG` instead), UTF-8, term and
query limits (65535 bytes per term, 128 bytes per query), `struct_size`,
`ranking` and `budget`. What cannot be checked in C is not checked: a dangling
or misaligned pointer, a length larger than the buffer, a freed handle. Those
are the caller's contract, stated in each function's `# Safety` section.

No panic crosses the boundary: each body runs in `catch_unwind` and a panic
becomes `KH_ERR_INTERNAL` (there is a test that forces one). This needs the
default `panic = "unwind"`. If a consumer builds the crate with
`panic = "abort"`, a panic aborts the process instead, which is also safe but
not recoverable. A caught panic still prints its message through the host's
panic hook (stderr by default). The repository's `wasm` profile is the only one that sets
`abort`, and it is not used for this crate. Out-of-memory aborts (the core
allocates infallibly), as in any Rust program; `kh_index_build` reserves its
own buffer with `try_reserve_exact` and reports `KH_ERR_INTERNAL` if that
fails.

## Ownership and "double free"

- `kh_index_free(kh_index **)` and `kh_results_free(kh_results *)` take a
  pointer to the caller's variable and reset it (to NULL / to the empty
  result). Freeing the same variable twice is therefore a defined no-op, and
  so is freeing NULL.
- What stays undefined behaviour: freeing through a *copy* of the handle (two
  variables holding the same pointer), using a hit's `term` after the index is
  freed, freeing an index while another thread searches it, or passing a
  `kh_results` the caller modified. C has no way to detect these; wrappers
  should hold the handle in exactly one place.
- Buffers passed in (`kh_entry` array, term bytes, query, config) are only read
  during the call; `kh_index_build` copies the terms.
- Copying a `kh_results` struct and freeing both copies is a double free, the
  same as copying the handle: only the variable passed to the free function is
  reset.
- The pointer from `kh_last_error()` is invalidated by the next call into the
  library on that thread, and that includes `kh_index_free` and
  `kh_results_free`; copy the message if it is needed later.
- `kh_search` overwrites `*out` without reading it, so an uninitialised
  struct is fine, but an earlier result still in it is leaked.

## Threads

The index is immutable after construction: any number of threads may call
`kh_search` on the same index at once (a test does this). Each search creates
its own scratch state, so there is no lock; the price is one small allocation
per search that a reusable per-thread context could avoid (not measured; a
context type can be added later without breaking this ABI). The last-error
message is thread-local.

## ABI stability rules

Version: `KH_ABI_VERSION` (header constant) and `kh_abi_version()` (loaded
library). A wrapper must compare them at start-up and refuse to run on a
mismatch. The number is a single integer:

1. **Never changed, once released:** the name, argument types and semantics of
   an exported function; the field order, types and offsets of an existing
   struct; the value and meaning of an existing enum constant or `#define`.
   Doing any of these bumps `KH_ABI_VERSION`.
2. **Allowed without a bump (additive):** new functions; new `kh_status`
   codes (clients must treat any unknown non-zero as failure); new constants;
   new fields **appended** to the end of a struct that carries a size (see 3);
   new accepted values of `ranking`.
3. **Extensible structs.** An appended field must increase `sizeof` on every
   target: `kh_config` therefore ends with an explicit `reserved` field (must be
   0, else `KH_ERR_INVALID_ARGUMENT`) so that there is no implicit tail
   padding, and a test asserts the size and the offset. Otherwise a `uint32_t`
   appended into padding would leave `sizeof` unchanged and `struct_size`
   could not tell the versions apart.
    `kh_config` starts with `struct_size` (the caller's
   `sizeof`). A library reads only the fields inside `struct_size`; fields it
   knows that lie beyond it take defaults, and a `struct_size` smaller than
   version 1's is an error. So an old program works with a newer library, and a
   newer program's larger struct works with an older one (the extra tail is
   ignored). Output structs (`kh_results`, `kh_hit`) are filled by the library
   and are not extended in place: adding fields there is an ABI bump, or a new
   struct with a new function.
4. **Enums as integers.** `kh_status` is returned only, never accepted from the
   caller, because a value outside a Rust enum is undefined behaviour;
   `kh_status_string` therefore takes `int32_t`, and `ranking` is a `uint32_t`
   with constants. Keep it that way for any input.
5. **Fixed-width types** (`uint32_t`, `uint64_t`, `size_t`, `uint8_t`) only; no
   `long`, `bool` or `enum` in structs, so layouts do not depend on the
   compiler's choices. A change of a struct layout on a platform (there are
   32-bit `size_t` targets) is a bug.
6. **The header is generated** by cbindgen from `bindings/c/src/lib.rs`
   (config in `bindings/c/cbindgen.toml`), committed, and checked by CI
   (`c-abi` job): the job regenerates it and fails on any difference, so an
   ABI change cannot slip through unreviewed in a diff of Rust only.
7. **Symbols** are unmangled `kh_*` C functions only; the Rust API of the crate
   (there is none besides the exports) is not part of the ABI.

The cost values are the core's provisional fixed-point costs (16 = one
ordinary edit) and can change between core versions without an ABI change,
because they are data, not layout; the core's own documentation says they are
provisional.

## What is tested

- `bindings/c/src/tests.rs`: build/search/free, defaults, config fields,
  lower-casing and duplicate handling, and every error path (null pointers,
  oversize lengths, invalid UTF-8, empty/oversize terms, query too long,
  unknown ranking, budget too large, bad `struct_size`, a forced panic,
  double free of an index and of results, concurrent searches).
- `bindings/c/tests/smoke.c`: a C program that includes the generated header,
  links the library, builds an index, searches and checks results and some
  error codes. The header is also compiled as C++17 in CI (compiled, linked against the library and run, with `c++ -std=c++17`). CI compiles it with the system `cc` (clang on macOS, gcc on
  Linux) against the `cdylib`. It was also compiled and run once with MSVC
  `cl` on a Windows machine; Windows is not part of CI (no MSVC environment
  step is set up), so that is not continuously checked. The header was also
  compiled and linked as C++17 (`cl /std:c++17`) and run there once, including
  `kh_config_default` and `kh_abi_version`; that too is a one-off.
- Miri: `cargo miri test -p keyhammer-c` (in the weekly `miri` workflow) runs
  the Rust tests, which call the `extern "C"` functions as C would, with
  `-Zmiri-strict-provenance`. It passed locally. Miri cannot run the C
  program, so the C-side behaviour is covered only by the smoke test.
- `unsafe` is confined to this crate: the core keeps `forbid(unsafe_code)`,
  this crate sets `#![deny(unsafe_op_in_unsafe_fn)]` and every `unsafe` block
  has a `// SAFETY:` comment.

## Supply chain

`keyhammer-c` adds no third-party dependency (only the `keyhammer` path
dependency), so `cargo deny check` for the workspace is unaffected apart from
the new workspace package itself (AGPL-3.0-or-later, already allowed).
`cbindgen` (MPL-2.0) is installed by the CI job as a build tool with an exact
version and `--locked`; it is not linked into anything shipped, and it is not
in `Cargo.lock`.

## Windows

The exports are `#[unsafe(no_mangle)] pub extern "C"` functions, which rustc
exports from the `cdylib` automatically: no `.def` file and no `__declspec`
are needed. With MSVC link `keyhammer_c.dll.lib` (the import library of
`keyhammer_c.dll`, which must be found at run time). `keyhammer_c.lib` is the
static library; using it also needs the system libraries that the Rust
standard library links (at least `ws2_32`, `userenv`, `ntdll` and `bcrypt`;
`cargo rustc -p keyhammer-c -- --print native-static-libs` prints the exact
list for a given toolchain). The header is committed with LF line endings
(`.gitattributes`) so that the freshness check does not depend on the platform.

## Not done

- No Windows CI job for the C test.
- No reusable search context, no iteration over hits with a callback (by
  design), no serialisation of an index.
- No packaging (`pkg-config`, install rules, versioned soname): the crate is
  unpublished.
