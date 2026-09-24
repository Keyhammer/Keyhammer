# Prior art and references

This page records which references the project cites were checked against the
original source, and what the published literature already contains about the
pruning bound we call the Trasel Signature Bound (TSB). It was compiled on
2026-09-23 from the sources linked below. Anything we could not open is marked
`not verified` and is not stated as fact.

## Reference check

| Reference | Verified? | Source opened | What it actually says (one line) |
|---|---|---|---|
| Hsu & Ottaviano 2013, "Space-Efficient Data Structures for Top-k Completion", WWW 2013 | yes | [PDF (author copy)](http://groups.di.unipi.it/~ottavian/files/topk_completion_www13.pdf), [ACM](https://dl.acm.org/doi/10.1145/2488388.2488440) | Exact-prefix top-k completion over compressed tries; each internal node stores the maximum score of its descendant leaves and a priority queue expands nodes best-first. No error tolerance. |
| Xiao, Qin, Wang, Ishikawa, Tsuda, Sadakane 2013, "Efficient Error-tolerant Query Autocompletion", PVLDB 6(6) | yes | [PDF (VLDB)](http://www.vldb.org/pvldb/vol6/p373-xiao.pdf), [DOI 10.14778/2536336.2536339](https://doi.org/10.14778/2536336.2536339) | Error-tolerant autocompletion under an edit-distance threshold: indexes deletion-marked variants of the data strings in a trie (IncNGTrie) so few active nodes are kept. It does not rank completions by a per-node maximum score. |
| Schulz & Mihov 2002, "Fast string correction with Levenshtein automata", IJDAR 5:67-85 | yes | [PDF (course copy of the journal article)](https://dmice.ohsu.edu/bedricks/courses/cs655/pdf/readings/2002_Schulz.pdf), DOI 10.1007/s10032-002-0082-8 | Builds a deterministic Levenshtein automaton of degree n in time linear in the word and uses it to steer search in a trie/automaton dictionary. Unit-cost operations (with variants adding transpositions, merges, splits); symbol-dependent costs are listed only as future work. |
| Myers 1999, "A Fast Bit-Vector Algorithm for Approximate String Matching Based on Dynamic Programming", JACM 46(3):395-415 | yes | [PDF](https://www.gersteinlab.org/courses/452/09-spring/pdf/Myers.pdf), [ACM](https://dl.acm.org/doi/10.1145/316542.316550) | Bit-parallel encoding of the unit-cost DP matrix for k-differences matching in O(nm/w) time, independent of k. |
| Hyyrö 2003, "A Bit-Vector Algorithm for Computing Levenshtein and Damerau Edit Distances", Nordic Journal of Computing 10(1) | partly | [Prague Stringology Conference 2002 page](http://www.stringology.org/event/2002/p6.html) (opened); ACM DL page for the 2003 journal version returned HTTP 403 | Content verified from the 2002 conference version: a bit-vector algorithm for Levenshtein and Damerau (adjacent transposition) distance, mainly for thresholded tests. The 2003 journal venue, volume and pages are **not verified** (seen only in search snippets). |
| Daciuk, Mihov, Watson, Watson 2000, "Incremental Construction of Minimal Acyclic Finite-State Automata", Computational Linguistics 26(1) | yes | [PDF (ACL Anthology J00-1002)](https://aclanthology.org/J00-1002.pdf) | Builds a minimal deterministic acyclic automaton (DAWG) in one pass, adding strings one by one and minimizing on the fly, with a sorted-input specialisation. |
| Wobbrock & Myers 2006, "Analyzing the Input Stream for Character-Level Errors in Unconstrained Text Entry Evaluations", ACM TOCHI 13(4):458-489 | yes (metadata); claim not supported | [PDF (author copy)](https://faculty.washington.edu/wobbrock/pubs/tochi-06.pdf), [ACM](https://dl.acm.org/doi/10.1145/1188816.1188819) | A methodology for classifying character-level errors (substitutions, insertions, omissions, corrected or not) from the full input stream of text-entry experiments. We found no statement that errors on the first character should cost more, nor any positional error weight. |
| Wang, Feng, Li 2010, "Trie-Join: Efficient Trie-based String Similarity Joins with Edit-Distance Constraints", PVLDB 3(1):1219-1230 | yes | [PDF (Tsinghua)](https://dbgroup.cs.tsinghua.edu.cn/ligl/papers/vldb2010-triejoin.pdf), Crossref record for DOI 10.14778/1920841.1920992 | Similarity join over a trie with subtrie pruning; adds length pruning (each node keeps the length range [shortest, longest] of strings in its subtrie), single-branch pruning and count pruning. Also opened the extended version: Feng, Wang, Li, VLDB Journal 21:437-461, 2012 ([PDF](https://dbgroup.cs.tsinghua.edu.cn/ligl/papers/vldbj2012-triejoin.pdf)). |
| Ji, Li, Li, Feng 2009, "Efficient Interactive Fuzzy Keyword Search", WWW 2009 | yes | [PDF (UCI)](https://ics.uci.edu/~chenli/pub/www2009-tastier-fuzzy.pdf) | Fuzzy type-ahead search: incrementally maintains the set of trie "active nodes" within an edit-distance threshold of the typed prefix; trie nodes carry keyword-ID ranges for intersecting answer lists. Unit edit distance. |
| Deng, Li, Feng, Li 2013, "Top-k String Similarity Search with Edit-Distance Constraints", ICDE 2013 | yes | [PDF (Tsinghua)](https://dbgroup.cs.tsinghua.edu.cn/ligl/papers/icde13-topk.pdf) | Top-k search by progressively raising the edit distance over a trie, computing only "pivotal entries" of the DP matrix and grouping them into ranges. Unit costs; no per-subtree length or letter summaries were found. |
| Loving, Hernandez, Benson 2014, "BitPAl: a bit-parallel, general integer-scoring sequence alignment algorithm", Bioinformatics 30(22):3166-3173 | yes | [PubMed record 25075119](https://pubmed.ncbi.nlm.nih.gov/25075119/) (abstract read via the NCBI E-utilities endpoint) | Bit-parallel global alignment for general integer weights (match, mismatch, indel), with algorithm classes specialised per weight set; 7-25x faster than a standard iterative DP. |
| Bibbens, Borevitz, McCauley 2026, "Space-Efficient Text Indexing with Mismatches using Function Inversion", arXiv:2604.01307 | yes | [arXiv abstract](https://arxiv.org/abs/2604.01307), [PDF](https://arxiv.org/pdf/2604.01307) | Theory paper: an O(n)-space index for finding all substrings of a text within Hamming distance k of a query, combining the CGL tree with Fiat-Naor function inversion. Hamming distance only; edit distance is not discussed. |

Corrections to how these references had been summarised in earlier project notes:

- **Xiao et al. 2013** was credited with ordering prefix-mode candidates by the
  maximum weight of the subtree. The paper does not do that; it is about
  error-tolerant autocompletion under a threshold. The per-node maximum score
  with best-first expansion comes from **Hsu & Ottaviano 2013**.
- **Wobbrock & Myers 2006** was cited for a higher cost on the first character.
  The paper is about error classification in text-entry experiments; we did
  not find that claim in it, nor any positional error weight. Until another source is found, the first-character
  multiplier is our own tuning choice, not a published result.
- **Hyyrö 2003**: the content matches the 2002 Prague Stringology Conference
  version; the 2003 journal details could not be opened.
- **Trie-Join 2010**: the author order Wang, Feng, Li is correct for the 2010
  PVLDB paper. The 2012 VLDB Journal extension lists Feng, Wang, Li. Some web
  indexes list the 2010 paper as Wang, Li, Feng; both the PDF and Crossref say
  Wang, Feng, Li.
- **arXiv:2604.01307** is about Hamming-distance text indexing (substrings of
  one long text), not dictionary search with typos. It explains the legacy
  engine's Hamming-scan and CGL-tree parts, but it is not a source for the
  new trie + weighted edit distance design.

## Prior art for the Trasel Signature Bound (TSB)

What the TSB is, in our words. Every trie node stores the shortest and
longest length of the terms below it, `[Lmin, Lmax]`, and a 64-bit mask of the
letter classes that appear on edges below it. The search walks the trie
best-first and keeps one DP row per node, where cell `i` holds the cost of
matching the node's prefix against the first `i` query characters. For each
cell the bound adds `max(T_comp, T_let)` to that cell's cost, and the node's
bound is the minimum over cells:

- `T_comp = c_indel_min * distance(|rest of query|, [Lmin - depth, Lmax - depth])`,
  the cheapest insert/delete needed to make the lengths fit;
- `T_let = c_min * popcount(classes(rest of query) & ~mask_below)`, one
  cheapest edit per letter class of the remaining query that does not occur
  anywhere below the node.

With keyboard-weighted costs, in our engine this cut the number of trie nodes
expanded by 33-35% with identical results on real typo data (the project's own
benchmark, documented in `docs/benchmarks/m0.md`).

The four columns ask: (a) does the work keep a length range per subtree/node,
(b) a letter set or mask per subtree/node, (c) does it add those to the
per-cell accumulated DP cost of a trie walk, (d) are edit costs weighted by
keyboard. `unknown (not read)` means we did not read enough of the work to say.

| Work | Length range per subtree | Letter mask per subtree | Combined with accumulated DP cost | Weighted by keyboard | Notes |
|---|---|---|---|---|---|
| Trie-Join, Wang, Feng, Li, PVLDB 2010 (and VLDB J. 2012) | **yes**: each node stores `[ls, ll]`, the shortest and longest string lengths in its subtrie | no | no: prunes a pair of nodes when their length ranges differ by more than the threshold; not added to a DP row | no (unit cost) | Closest prior art for the length half of the TSB. Used in a join with a fixed threshold, as a yes/no filter on active-node pairs. |
| Bed-tree, Zhang, Hadjieleftheriou, Ooi, Srivastava, SIGMOD 2010 ([ACM](https://dl.acm.org/doi/10.1145/1807167.1807266)) | partly: requires a "length bounding" string order, i.e. an upper bound on the length of any string in a node's interval | partly: the gram-counting order bounds hashed n-gram bucket counts per interval; the dictionary order uses a candidate letter set, but for one position only | partly: the dictionary-order bound runs the edit-distance DP over the interval's common prefix plus that one-position letter set | no (edit distance and normalised edit distance) | Node-level lower bounds on edit distance inside a B+-tree, supporting range, top-k and join queries. Closest prior art for "a per-node summary gives an admissible lower bound". Not a trie; the length and letter parts are separate string orders, not combined. |
| Boytsov 2011, "Indexing Methods for Approximate Dictionary Searching: Comparative Analysis", ACM Journal of Experimental Algorithmics, 2011 ([author preprint PDF](http://boytsov.info/pubs/jea2011.pdf); volume details not checked) | no (length-divided indexes are a separate method) | **per string, not per subtree**: a string's signature is the bit vector of alphabet characters it contains, optionally over a hashed, reduced alphabet | no: signatures are compared by frequency distance, which is a lower bound on edit distance, as a filter (signature hashing, vector tries) | no | Shows the letter-mask idea, including reduced alphabets whose collisions only loosen the bound, is folklore; the survey traces signature filtering back to Damerau 1964 and 1970s spell-checkers. We did not read every method in this 90-page survey. |
| Oflazer 1996, "Error-tolerant Finite-state Recognition...", Computational Linguistics 22(1) ([PDF](https://aclanthology.org/J96-1003.pdf)) | no | no | yes, in its basic form: the "cut-off edit distance" is the minimum over a window of the current DP column, used to abandon a branch of the automaton | no (unit cost) | This is the classical row-minimum lower bound that the TSB strengthens. |
| Hsu & Ottaviano 2013 | no | no | no (exact prefix only) | no | Per-node summary of a different quantity (maximum score) with best-first search. Same "store a max/min per subtree to guide a priority queue" pattern. |
| Ji et al. 2009 | no (per-node keyword-ID ranges, not lengths) | no | no: keeps active nodes with their edit distance; no extra bound | no | Incremental fuzzy type-ahead, threshold-based. |
| Xiao et al. 2013 | no | no | no | no | Neighbourhood generation (deletion variants) instead of pruning bounds. |
| Deng et al. 2013 | no | no | no; the related-work section cites index-node lower bounds (Bed-tree) | no | Top-k over a trie by raising the threshold; unit cost. |
| Schulz & Mihov 2002 | no | no | not applicable (automaton, not a DP row) | no; weighted costs named as future work | Background for trie search driven by an automaton. |
| Keyboard-weighted spelling correction in general (web results, patents, theses) | unknown (not read) | unknown (not read) | unknown (not read) | yes (by definition) | Search hits show keyboard-proximity weighted edit distance is common in spelling correction; we did not open a work that combines it with subtree summaries. |

## Conclusion

Each ingredient of the TSB is already published. Per-subtree length ranges
for pruning edit-distance search over a trie are in Trie-Join (2010).
Character-presence signatures, including hashed or reduced alphabets, as
lower bounds on edit distance go back decades and are surveyed by Boytsov
(2011). Per-node lower bounds that combine a DP over a known prefix with
information about the unknown rest were used for top-k search in the Bed-tree
(2010). The row-minimum bound over a DP column is classical (Oflazer 1996).
What we did not find in the works we read is this particular combination:
both summaries stored on every trie node, turned into cost terms (the
cheapest indel times the length gap; the cheapest edit times the number of
missing letter classes), joined with `max` and added cell by cell to the
accumulated cost of a best-first DP walk under keyboard-weighted costs. That
is not strong evidence of novelty. Our search was 13 targeted web queries plus one
bibliographic lookup per reference (listed below), plus reading about ten papers; it was not a systematic survey,
and the combination may exist in work we did not reach. So we will not say
"we invented". Honest wording: "we apply the per-subtree length ranges of
Trie-Join and the character-signature filters of approximate dictionary
search as an admissible, cost-weighted lower bound inside a best-first trie
search with a weighted edit distance, and measure its effect (33-35% fewer
nodes expanded, same results, per the project's own benchmark in
`docs/benchmarks/m0.md`)." The name "Trasel Signature Bound" should be
presented as a name for this engineering combination, not as a new
technique.

## Search log

Queries run (web search), 2026-09-23:

1. `trie subtree character set bitmask pruning approximate string search`
2. `length and letter signature lower bound trie edit distance`
3. `top-k approximate string search trie length filter`
4. `trie-based fuzzy search node signature pruning`
5. `Boytsov "Indexing methods for approximate dictionary searching" comparative analysis signature hashing frequency distance`
6. `frequency distance lower bound edit distance Kahveci Singh frequency vector MRS-index`
7. `bitmap filter edit distance character presence bit vector lower bound string similarity join`
8. `"Bed-tree" all-purpose index structure string similarity search edit distance lower bound node`
9. `A* search trie admissible heuristic edit distance spelling correction remaining characters lower bound subtree`
10. `trie node stores bitset of characters in descendants prune approximate dictionary lookup`
11. `weighted edit distance keyboard adjacency trie search spelling correction top-k best-first`
12. `Oflazer 1996 "error-tolerant finite-state recognition" cut-off edit distance morphological analysis spelling correction`
13. `approximate string search tree index node "character set" OR "alphabet signature" lower bound edit distance subtree pruning paper`

Plus one bibliographic query per reference in the first table (title and
author lookups) to find the source pages.

Not followed up, possible further reading: Kahveci & Singh, VLDB 2001 (MRS
index, frequency-vector lower bounds); Mihov & Schulz 2004 (forward-backward
dictionary search); the Trie-Join journal version's later sections; recent
learned or SIMD fuzzy-search engines.
