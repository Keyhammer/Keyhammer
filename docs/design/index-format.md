# Serialized index format (issue #26)

A built `Trie` can be written to bytes and loaded back without rebuilding it. This note fixes
the format before the code: the layout, what the loader validates and why, the policy for
corrupt or hostile input, and what is left for later. Status: format version 1, profile 1,
unpublished (the crate has `publish = false`); from the first release on, a change to anything
below needs a new version number (section 7).

## 1. Decisions in one list

1. `Trie::to_bytes()` writes an index; `Index::from_bytes(&[u8])` validates it once and
   returns `Index<'_>`, a **borrowed view**: it keeps sub-slices of the input and reads every
   value on demand with `u16/u32/u64::from_le_bytes` on checked sub-slices. No `unsafe`, no
   transmute, no pointer casts, no copy of the node arrays. `Index::to_trie()` (and the
   shortcut `Trie::from_bytes`) builds an owned `Trie` from a view.
2. **Search runs on the view.** `Index::search`, `search_prefix`, `search_text` and
   `search_prefix_text` take a `&mut Searcher` and run the same code as the `Searcher`
   methods, which is generic inside the crate over a crate-private node-access trait with two
   implementations (`Trie` and `Index`). The public `Searcher` methods keep their `&Trie`
   signatures exactly, so no caller changes and the `Trie` path compiles to the same
   monomorphic code as before (section 8).
3. Little-endian throughout; every section starts at a multiple of 8 bytes from the start of
   the file and is followed by zero padding up to the next multiple of 8. Alignment is a rule
   of the file (so that a future reader may map it), never something this crate relies on:
   the input slice may start at any address.
4. **Deterministic.** The bytes depend only on the trie's contents (terms, weights, input
   indices, normaliser), written in node order and term order; there is no hash map, no
   timestamp, no uninitialised padding. The same trie gives the same bytes on every platform.
5. **Canonical.** The loader accepts exactly the byte strings that `to_bytes` can produce for
   some trie, up to the values of three payload fields (section 5.4). In particular every
   per-node aggregate the search relies on (`len_min`, `len_max`, `below_mask`,
   `max_weight`) is **recomputed and compared**, not trusted: a `len_max` that is too small or
   a `below_mask` with a missing class would make the subtree bound inadmissible and silently
   drop results, and a checksum only protects against accidents, not against a file written
   wrong on purpose or by a buggy writer.
6. A CRC-32 (IEEE 802.3, the one of zlib and PNG) over the whole file, implemented in the
   crate (slicing-by-8 tables built by a `const fn`), catches accidental corruption before the
   structural checks run. Test vector: `"123456789"` gives `0xCBF43926`.
7. The text normaliser is part of the index: if the trie was built with
   `Trie::build_normalized`, the file records the normaliser's modes, the version of the
   normalisation algorithm and the Unicode version of its tables, and a build of the crate
   with a different algorithm or table version refuses to load it. Otherwise `search_text`
   could fold queries differently from how the terms were folded.
8. The core crate stays `no_std` + `alloc`, dependency-free, `forbid(unsafe_code)`.
   `from_bytes` allocates nothing; `to_bytes` allocates the output only.

## 2. Layout

```
offset  size  field
0       64    header
64      264   section table: 11 entries of 24 bytes
328     ...   sections 1 to 11, in this order, each 8-aligned, zero-padded to 8
```

### 2.1 Header (64 bytes)

| Offset | Type | Field | Value |
| --- | --- | --- | --- |
| 0 | `[u8; 8]` | magic | `4B 48 49 4E 44 45 58 00` (`"KHINDEX\0"`) |
| 8 | `u16` | format version | `1` |
| 10 | `u16` | profile id | `1` (the flat parallel-array layout below) |
| 12 | `u32` | flags | bit 0: a normaliser is recorded; other bits must be 0 |
| 16 | `u64` | file length | total length in bytes, a multiple of 8 |
| 24 | `u32` | CRC-32 | of the whole file with these 4 bytes read as zero |
| 28 | `u32` | section count | `11` |
| 32 | `u32` | node count `N` | at least 2 (the root and one term) |
| 36 | `u32` | term count `T` | `1 <= T <= N - 1` |
| 40 | `u32` | string pool length `P` | bytes of term text, `P >= T` |
| 44 | `u8` | normaliser modes | bit 0 folds case, bit 1 folds diacritics; other bits 0 |
| 45 | `u8` | reserved | 0 |
| 46 | `u16` | normaliser algorithm version | `1` in this crate |
| 48 | `[u8; 3]` | Unicode version of the normaliser tables | `15, 0, 0` in this crate |
| 51 | `u8` | reserved | 0 |
| 52 | `[u8; 12]` | reserved | 0 |

Bytes 44 to 51 are all zero when flag bit 0 is clear. A mode byte of 0 with the flag set is
valid: it is `build_normalized` with both foldings switched off.

### 2.2 Section table

Eleven entries of 24 bytes, one per section, in section order:

| Offset in entry | Type | Field |
| --- | --- | --- |
| 0 | `u32` | section id (1 to 11) |
| 4 | `u32` | element size in bytes (1, 2, 4 or 8) |
| 8 | `u64` | offset of the section from the start of the file |
| 16 | `u64` | length of the section in bytes, padding excluded |

The table is redundant with the counts on purpose: it lets a tool inspect a file without
knowing the profile, and it is checked against the layout the counts imply, entry by entry.
Offsets are therefore strictly increasing, 8-aligned and non-overlapping, and the file ends
at the last section's end rounded up to 8.

### 2.3 Sections

Nodes are numbered in breadth-first order, root 0, exactly as in `Trie` (the ids of
`Trie::children`, `Trie::term_id` and so on are the same in the file).

| Id | Name | Element | Count | Meaning |
| --- | --- | --- | --- | --- |
| 1 | labels | `u32` | `N` | code point on the edge into the node; 0 for the root |
| 2 | child offsets | `u32` | `N + 1` | children of `v` are `offsets[v] .. offsets[v + 1]` |
| 3 | term ids | `u32` | `N` | id of the term ending at the node, or `0xFFFFFFFF` |
| 4 | `len_min` | `u16` | `N` | shortest term at or below the node, in code points |
| 5 | `len_max` | `u16` | `N` | longest term at or below the node |
| 6 | `max_weight` | `u16` | `N` | largest weight at or below the node |
| 7 | `below_mask` | `u64` | `N` | `symbol_class` of every edge strictly below the node |
| 8 | weights | `u16` | `T` | weight of term `id` |
| 9 | input index | `u32` | `T` | `Trie::input_index(id)` |
| 10 | term offsets | `u32` | `T + 1` | term `id` is `pool[offsets[id] .. offsets[id + 1]]` |
| 11 | string pool | `u8` | `P` | the terms' UTF-8, concatenated in id order |

Labels are always 4 bytes, even for an ASCII dictionary where the in-memory trie stores one
byte per label. A narrower label section is the obvious first item of a compact profile
(milestone F2); it is not in version 1 because it would add a variant to every reader before
anyone has measured that the 3 bytes per node matter for a mapped file.

Because children are contiguous and the layout is breadth first, one `N + 1` array replaces
the in-memory pair (`child_start`, `child_count`).

## 3. Size limits

| Quantity | Limit | Why |
| --- | --- | --- |
| nodes `N` | `2 ..= 2^32 - 1` | child offsets are `u32` and go up to `N` |
| terms `T` | `1 ..= min(N - 1, 2^32 - 2)` | `0xFFFFFFFF` is the "no term" marker |
| pool `P` | `T ..= 2^32 - 1` bytes | term offsets are `u32`; terms are not empty |
| one term | at most 65535 bytes | as in `Trie::build`; so a depth fits `u16` |
| children of a node | at most 65535 | as the in-memory `child_count: u16` |
| file | whatever the counts give, and at most `usize::MAX` | computed in `u64` with checked arithmetic |

`to_bytes` returns `Err(FormatError::TooLarge)` for a trie beyond these limits (in practice a
pool above 4 GiB). No trie that `Trie::build` accepts has more nodes than that.

## 4. Checksum

CRC-32 with the reflected polynomial `0xEDB88320`, initial value and final XOR `0xFFFFFFFF`
(zlib's `crc32`). It covers the whole file, header included, with the four bytes of the CRC
field replaced by zeros while computing. Known vectors, all tested: `""` gives `0x00000000`,
`"a"` `0xE8B7BE43`, `"123456789"` `0xCBF43926`, `"The quick brown fox jumps over the lazy
dog"` `0x414FA339`. A CRC-32 detects every error confined to 32 consecutive bits, so every
change of a single byte, and every change to up to four consecutive bytes, of a valid file is
detected; that is what the mutation test relies on for its first half (section 6).

## 5. Validation

`from_bytes` runs these checks in this order and returns the first failure. Every read is a
checked sub-slice read; every size and offset is computed with checked arithmetic in `u64`,
so no input can make the loader panic, overflow or allocate.

### 5.1 Header, length and checksum

1. At least 64 bytes (`TooShort`), the magic (`BadMagic`).
2. The version: anything but 1 is `UnsupportedVersion`, newer or older. A reader never
   guesses at a newer file.
3. The profile: anything but 1 is `UnknownProfile`.
4. Unknown flag bits (`UnknownFlags`).
5. The declared length equals the input length (`LengthMismatch`): this is what rejects a
   truncated or extended file.
6. The CRC (`ChecksumMismatch`).
7. Reserved header bytes are zero (`NonZeroReserved`); the normaliser fields are zero without
   the flag; with it, the mode byte uses bits 0 and 1 only (`BadNormalizer`) and the algorithm
   and Unicode versions equal this crate's (`NormalizerMismatch`).
8. The section count is 11 (`SectionCount`), the counts are within section 3's limits
   (`BadCounts`), every table entry equals the one the counts imply (`BadSectionTable`), the
   length the counts imply is the file's (`BadCounts`), and the padding after each section is
   zero (`NonZeroReserved`).

### 5.2 Tree

9. Child offsets (`BadChildren`): `offsets[0] = 1`, `offsets[N] = N`, non-decreasing, at most
   65535 children per node, and `offsets[v] > v` for every node. Together these make the
   child ranges a partition of `1..N` in parent order, each child after its parent: every node
   but the root has exactly one parent, there is no cycle, and the order is breadth first.
   The depth of every node follows in one pass (the first node of each level starts the
   children of the next), without an allocation. A depth above 65535 is `TooDeep`.
10. Labels (`BadLabel`): the root's is 0, every label is a Unicode scalar value (not a
    surrogate, at most U+10FFFF), and the labels of the children of a node are strictly
    increasing. Strictly increasing makes siblings distinct (so each term has one path) and
    matches the order `Trie::build` produces (UTF-8 byte order and code point order agree).
11. Term ids (`BadTermId`): each is `0xFFFFFFFF` or below `T`; the root has none (terms are
    not empty); every leaf has one (`Trie::build` never makes a leaf without a term, and a
    leaf without one would make `len_min > len_max`). The number of nodes with a term is `T`
    (`TerminalCount`).
12. Aggregates, recomputed (`BadLenBounds`, `BadBelowMask`, `BadMaxWeight`): for every node,
    from its own term (length = depth, weight from section 8) and its children's *stored*
    values, exactly as `Trie::build` computes them. Checking each node against its children's
    stored values is enough: by induction from the leaves (children have larger ids), all
    stored values then equal the true subtree values. One pass, no allocation.

### 5.3 Terms

13. Term offsets (`BadTermOffsets`): `offsets[0] = 0`, `offsets[T] = P`, each term non-empty
    and at most 65535 bytes.
14. Each term is valid UTF-8 (`InvalidUtf8`).
15. Terms are strictly increasing in byte order (`TermsNotSorted`): ids are positions in the
    sorted, deduplicated list, as `Trie::build` assigns them.
16. Each term is in the trie at its id (`TermNotInTrie`): walking from the root by its code
    points (binary search among the sorted siblings) ends at a node whose term id is the term's
    id. With step 11 (`T` terminal nodes, all ids below `T`) this makes term ids unique and
    the map from ids to terminal nodes a bijection, and since every leaf is terminal, every
    node lies on the path of some term: the tree is exactly the trie of the term list.
17. With a normaliser, each term is already normalised (`TermNotNormalized`), checked without
    allocating by streaming the normaliser's output against the term.
18. Input indices are not `0xFFFFFFFF` (`BadInputIndex`).

The cost is linear in the file: one pass over the nodes, one over the terms, plus a binary
search per code point of each term (at most `log2` of the fan-out, which is small).

### 5.4 What is accepted: the canonical-form guarantee

After steps 1 to 18, the view is byte for byte what `to_bytes` writes for
`Trie::build(terms with weights)` (or `build_normalized` with the recorded normaliser), except
for the input indices: section 9 holds whatever the writer put there, as long as it is not
`0xFFFFFFFF`. So three things are **payload**: the weights, the input indices and the whole
normaliser record (flag bit 0 and bytes 44 to 50). They can be changed without breaking the
structure, and the loader accepts such a change (after the CRC is fixed) when it stays
consistent: a weight change that alters a `max_weight` on its path is rejected by step 12, a
normaliser change that leaves a term unnormalised by step 17. In particular:

- **The normaliser is the file's, not the caller's.** Clearing flag bit 0 and zeroing bytes 44
  to 50 turns a normalised index into a plain one, and the loader accepts it; the reverse is
  accepted too when the terms are already in normal form, and so is a changed mode byte.
  `search_text` on the view (and on `to_trie`) folds queries with the normaliser recorded in
  the file, so such a change changes results: on an index of `cafe` built with the default
  normaliser, `search_text("café")` at budget 0 has 1 hit; with the mode byte changed to case
  folding only, it has 0. The CRC is not authentication: anyone who can write the file can
  recompute it. A caller that needs a specific folding must compare `Index::normalizer()` with
  the value it expects (`index.normalizer() == Some(expected)`) before searching, or
  authenticate the file itself (a signature or an HMAC over the bytes).
- **Input indices are untrusted writer data.** Uniqueness is not checked (it would need memory
  proportional to the largest index), nor any upper bound (the loader does not know how many
  items the writer had). The core never reads them; a caller that does `items[input_index]`
  must bounds-check.

## 6. Policy for corrupt and hostile input, and how it is tested

- **Accidental corruption.** Every change of one byte, and every truncation or extension, of
  a valid file is rejected (CRC and length). Tested exhaustively on small indexes: every byte
  XOR every one of its 8 single-bit masks and XOR `0xFF`, and every length from 0 to the full
  length minus 1.
- **Consistent-looking but wrong files** (a buggy writer, or someone who recomputes the CRC).
  The same exhaustive mutations, with the CRC recomputed after each, must either be rejected
  by the structural checks or give an index that is canonical (section 5.4): the test then
  checks that the view's search results, in both modes and both rankings, equal those of a
  trie rebuilt from the view's own term list, weights and normaliser, that `to_trie` followed
  by `to_bytes` gives back the mutated bytes exactly, and that the accepted byte lay in a
  payload field (a weight, an input index or the normaliser record).
- **Arbitrary bytes.** A fuzz target (`index_from_bytes`) and its seeded stand-in in
  `tests/fuzz_like.rs` feed arbitrary and mutated files; `from_bytes` never panics, and when it
  accepts, searching the view never panics and matches `to_trie`.

So a search on an accepted view terminates (the tree is finite and acyclic), never panics (all
indices are in range by construction, and the view's reads are checked anyway) and returns the
results of the rebuilt trie. It cannot return a wrong result because of a wrong aggregate.

## 7. Compatibility rules

- The version is bumped for any change of the layout or of the meaning of a field. A reader
  refuses every version it does not implement, newer or older; this crate reads version 1
  only. There is no "minor, ignorable" section: an unknown section is a different version.
- The profile id names a layout within a version. Version 1 has profile 1 only; a compact
  profile (narrow labels, a smaller node record) would be profile 2. An unknown profile is
  an error.
- A change to the normaliser that changes its output for any input must bump the algorithm
  version in `text`; regenerating the tables from a newer Unicode version changes the
  recorded Unicode version. Either makes older normalised indexes fail to load with
  `NormalizerMismatch` (rebuild them from the source terms). Indexes without a normaliser are
  not affected.
- The cost model, layouts and search configuration are not part of the index: they are
  chosen at query time, as for an in-memory trie. The per-node `below_mask` depends on
  `cost::symbol_class`; a change of that function is a format change (a new version), since it
  would make every stored mask fail step 12.

## 8. API and the cost of search on the view

Public additions only: the module `keyhammer::index` with `Index`, `FormatError`
(`#[non_exhaustive]`), the constants `MAGIC`, `VERSION` and `PROFILE`, and `Trie::to_bytes` and
`Trie::from_bytes`. Nothing existing changes signature. The search code, and the match
highlighting of `docs/design/highlighting.md`, are generic over a crate-private trait; the
public `Searcher` methods instantiate them for `Trie` only, and `Index::search`,
`search_prefix`, `search_text`, `search_prefix_text`, `highlight` and `highlight_text` for the
view. The DP cell (`each_move` in `search.rs`) stays one function shared by the search rows and
the highlighting traceback, and does not depend on the node source. The owned path's node
counts, results and costs are unchanged (the existing regression tests pin them, and a digest
comparison against `main` in `docs/benchmarks/index-format.md` found them identical). The view pays a bounds check and a
little-endian decode per read, and four bytes per label; `docs/benchmarks/index-format.md`
measures the file size, the load (validation) time and the search cost on the view. In short,
on 274 137 words (one shared machine): the file is 21.0 MB, about 19% below the estimated heap
of the trie; validation takes about 32 ms against about 70 ms for a rebuild; search on the view
returns the same hits and work counters, and its latency was within about 3% of the owned
trie's.

The alternative, a public trait so that `Searcher::search` accepts either type, was rejected
for now: it changes the signatures of four public methods (a caller passing a `&&Trie` or an
`&Rc<Trie>` would stop compiling, since deref coercion does not apply to a generic argument)
and puts a trait into the public API before a second implementation outside the crate exists.

## 9. Not done

- The bindings (wasm, Node, Python, C) do not expose the format yet (follow-up). When they
  do, they must treat `input_index` after a load as untrusted (bounds-check it before indexing
  the caller's items) and let the caller check the recorded normaliser (section 5.4).
- No memory mapping helper: the caller reads the file into a buffer (or maps it) and passes
  the slice. Nothing here needs `std`.
- No compression and no compact profile; no incremental update of a stored index.
- Uniqueness and range of input indices are not validated (section 5.4).
