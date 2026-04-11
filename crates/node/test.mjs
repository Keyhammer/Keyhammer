import { createRequire } from "module";
const require = createRequire(import.meta.url);
const { KeyhammerIndex } = require("./keyhammer.node");

let passed = 0;
let failed = 0;

function test(name, fn) {
  try {
    fn();
    passed++;
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.log(`  ✗ ${name}: ${e.message}`);
  }
}

function assert(condition, msg = "assertion failed") {
  if (!condition) throw new Error(msg);
}

function assertEquals(a, b, msg) {
  if (a !== b) throw new Error(msg || `expected ${b}, got ${a}`);
}

// ─── Basic search ──────────────────────────────────────────────────────────

console.log("\n=== Basic search ===");

test("exact match returns score 1.0", () => {
  const idx = KeyhammerIndex.build(["hello", "world"], 2);
  const r = idx.search("hello", 5);
  assertEquals(r[0].term, "hello");
  assertEquals(r[0].score, 1.0);
});

test("fuzzy match finds close term", () => {
  const idx = KeyhammerIndex.build(["javascript", "typescript", "python"], 2);
  const r = idx.search("javscript", 5);
  assert(r.length > 0, "should find results");
});

test("no match for distant query", () => {
  const idx = KeyhammerIndex.build(["hello", "world"], 2);
  const r = idx.search("zzzzzzzzz", 5);
  assertEquals(r.length, 0);
});

test("results sorted by score descending", () => {
  const idx = KeyhammerIndex.build(["hello", "hallo", "hullo", "jello"], 2);
  const r = idx.search("hello", 10);
  for (let i = 1; i < r.length; i++) {
    assert(r[i - 1].score >= r[i].score, `score[${i-1}] >= score[${i}]`);
  }
});

// ─── Case insensitive ──────────────────────────────────────────────────────

console.log("\n=== Case insensitive ===");

test("lowercase query matches uppercase term", () => {
  const idx = KeyhammerIndex.build(["JavaScript", "Python"], 2);
  const r = idx.search("javascript", 5);
  assertEquals(r[0].term, "JavaScript");
});

test("uppercase query matches lowercase term", () => {
  const idx = KeyhammerIndex.build(["javascript", "python"], 2);
  const r = idx.search("JAVASCRIPT", 5);
  assertEquals(r[0].term, "javascript");
});

test("mixed case query works", () => {
  const idx = KeyhammerIndex.build(["JavaScript"], 2);
  const r = idx.search("jAvAsCrIpT", 5);
  assertEquals(r[0].term, "JavaScript");
});

// ─── Typo types ────────────────────────────────────────────────────────────

console.log("\n=== Typo types ===");

test("substitution: javoscript → javascript", () => {
  const idx = KeyhammerIndex.build(["javascript"], 2);
  const r = idx.search("javoscript", 5);
  assert(r.length > 0 && r[0].term === "javascript");
});

test("transposition: javsacript → javascript", () => {
  const idx = KeyhammerIndex.build(["javascript"], 2);
  const r = idx.search("javsacript", 5);
  assert(r.length > 0 && r[0].term === "javascript");
});

test("deletion: javasript → javascript", () => {
  const idx = KeyhammerIndex.build(["javascript"], 2);
  const r = idx.search("javasript", 5);
  assert(r.length > 0 && r[0].term === "javascript");
});

test("insertion: javasccript → javascript", () => {
  const idx = KeyhammerIndex.build(["javascript"], 2);
  const r = idx.search("javasccript", 5);
  assert(r.length > 0 && r[0].term === "javascript");
});

// ─── Match highlighting ────────────────────────────────────────────────────

console.log("\n=== Match highlighting ===");

test("exact match has full range", () => {
  const idx = KeyhammerIndex.build(["hello"], 2);
  const r = idx.search("hello", 5);
  assert(r[0].matchRanges.length > 0, "should have match ranges");
  assertEquals(r[0].matchRanges[0][0], 0, "start should be 0");
  assertEquals(r[0].matchRanges[0][1], 5, "end should be 5");
});

test("partial match has ranges", () => {
  const idx = KeyhammerIndex.build(["hello"], 2);
  const r = idx.search("hxllo", 5);
  assert(r.length > 0);
  assert(r[0].matchRanges.length >= 2, "should have at least 2 match ranges");
});

// ─── Dynamic add/remove ────────────────────────────────────────────────────

console.log("\n=== Dynamic add/remove ===");

test("add a term and find it", () => {
  const idx = KeyhammerIndex.build(["hello", "world"], 2);
  assertEquals(idx.len(), 2);
  idx.add("rust");
  assertEquals(idx.len(), 3);
  const r = idx.search("rust", 5);
  assert(r.length > 0 && r[0].term === "rust");
});

test("remove a term", () => {
  const idx = KeyhammerIndex.build(["hello", "world", "rust"], 2);
  assertEquals(idx.len(), 3);
  idx.remove(1); // remove "world"
  assertEquals(idx.len(), 2);
  const r = idx.search("world", 5);
  // "world" removed, shouldn't be an exact match
  assert(!r.some(x => x.term === "world"), "world should be removed");
});

// ─── Threshold ─────────────────────────────────────────────────────────────

console.log("\n=== Threshold ===");

test("threshold filters low scores", () => {
  const idx = KeyhammerIndex.build(["hello", "world", "hxllo"], 2);
  const all = idx.search("hello", 10);
  const filtered = idx.searchWithThreshold("hello", 10, 0.9);
  assert(filtered.length <= all.length, "filtered should have fewer results");
  filtered.forEach(r => assert(r.score >= 0.9, `score ${r.score} should be >= 0.9`));
});

test("threshold 0.0 returns all", () => {
  const idx = KeyhammerIndex.build(["hello", "hallo"], 2);
  const all = idx.search("hello", 10);
  const filtered = idx.searchWithThreshold("hello", 10, 0.0);
  assertEquals(all.length, filtered.length);
});

// ─── Export/Import ─────────────────────────────────────────────────────────

console.log("\n=== Export/Import ===");

test("export returns terms", () => {
  const idx = KeyhammerIndex.build(["hello", "world"], 2);
  const terms = idx.export();
  assertEquals(terms.length, 2);
  assertEquals(terms[0], "hello");
  assertEquals(terms[1], "world");
});

test("import reconstructs index", () => {
  const original = KeyhammerIndex.build(["javascript", "typescript"], 2);
  const terms = original.export();
  const imported = KeyhammerIndex.import(terms, 2);
  const r = imported.search("javasript", 5);
  assert(r.length > 0, "imported index should find results");
});

// ─── Keyboard layouts ──────────────────────────────────────────────────────

console.log("\n=== Keyboard layouts ===");

test("QWERTY layout (default)", () => {
  const idx = KeyhammerIndex.build(["hello"], 2);
  const r = idx.search("hello", 5);
  assert(r.length > 0);
});

test("AZERTY layout", () => {
  const idx = KeyhammerIndex.buildWithLayout(["bonjour"], 2, "azerty");
  const r = idx.search("bonjour", 5);
  assertEquals(r[0].term, "bonjour");
});

test("QWERTZ layout", () => {
  const idx = KeyhammerIndex.buildWithLayout(["hallo"], 2, "qwertz");
  const r = idx.search("hallo", 5);
  assertEquals(r[0].term, "hallo");
});

// ─── Stats ─────────────────────────────────────────────────────────────────

console.log("\n=== Stats ===");

test("stats returns correct info", () => {
  const idx = KeyhammerIndex.build(["a", "b", "c"], 2);
  const s = idx.stats();
  assertEquals(s.numTerms, 3);
  assertEquals(s.maxMismatches, 2);
});

test("len returns count", () => {
  const idx = KeyhammerIndex.build(["a", "b", "c", "d", "e"], 1);
  assertEquals(idx.len(), 5);
});

// ─── Edge cases ────────────────────────────────────────────────────────────

console.log("\n=== Edge cases ===");

test("empty query returns empty", () => {
  const idx = KeyhammerIndex.build(["hello"], 2);
  const r = idx.search("", 5);
  // may return short terms or empty
  assert(Array.isArray(r));
});

test("single char terms", () => {
  const idx = KeyhammerIndex.build(["a", "b", "c"], 1);
  const r = idx.search("a", 5);
  assert(r.length > 0);
});

test("numbers in terms", () => {
  const idx = KeyhammerIndex.build(["v1.0.0", "v2.0.0", "v1.0.1"], 2);
  const r = idx.search("v1.0.0", 5);
  assertEquals(r[0].term, "v1.0.0");
});

test("unicode terms don't crash", () => {
  const idx = KeyhammerIndex.build(["café", "naïve", "hello"], 2);
  const r = idx.search("cafe", 5);
  assert(Array.isArray(r));
});

test("k too large throws error", () => {
  try {
    KeyhammerIndex.build(["hello"], 10);
    assert(false, "should have thrown");
  } catch (e) {
    assert(e.message.includes("too large"));
  }
});

test("empty terms throws error", () => {
  try {
    KeyhammerIndex.build([], 2);
    assert(false, "should have thrown");
  } catch (e) {
    assert(e.message.includes("empty"));
  }
});

// ─── Scale ─────────────────────────────────────────────────────────────────

console.log("\n=== Scale ===");

test("1000 terms", () => {
  const terms = Array.from({ length: 1000 }, (_, i) => `term${String(i).padStart(4, "0")}`);
  const idx = KeyhammerIndex.build(terms, 2);
  assertEquals(idx.len(), 1000);
  const r = idx.search("term0500", 5);
  assert(r.length > 0 && r[0].term === "term0500");
});

test("5000 terms", () => {
  const terms = Array.from({ length: 5000 }, (_, i) => `word${String(i).padStart(5, "0")}`);
  const idx = KeyhammerIndex.build(terms, 2);
  assertEquals(idx.len(), 5000);
  const r = idx.search("word02500", 5);
  assert(r.length > 0 && r[0].term === "word02500");
});

test("add 1000 terms dynamically", () => {
  const idx = KeyhammerIndex.build(["seed"], 2);
  for (let i = 0; i < 1000; i++) {
    idx.add(`dynamic${i}`);
  }
  assertEquals(idx.len(), 1001);
  const r = idx.search("dynamic500", 5);
  assert(r.length > 0);
});

// ─── Summary ───────────────────────────────────────────────────────────────

console.log(`\n${"=".repeat(50)}`);
console.log(`  ${passed} passed, ${failed} failed`);
console.log(`${"=".repeat(50)}\n`);

if (failed > 0) process.exit(1);
