import Fuse from "fuse.js";
import { createRequire } from "module";
const require = createRequire(import.meta.url);
const { KeyhammerIndex } = require("../crates/node/keyhammer.node");

// ─── Config ────────────────────────────────────────────────────────────────

const SIZES = [1000, 5000, 10000];

const WORDS = [
  "abstract", "algorithm", "allocate", "analyze", "application", "architecture",
  "argument", "array", "assert", "async", "attribute", "authenticate", "authorize",
  "bandwidth", "benchmark", "binary", "boolean", "bootstrap", "buffer", "build",
  "cache", "callback", "certificate", "channel", "checkpoint", "class", "client",
  "closure", "cluster", "column", "command", "commit", "compile", "component",
  "compress", "compute", "concatenate", "concurrent", "config", "connection",
  "console", "constant", "constraint", "constructor", "container", "context",
  "controller", "convert", "coordinate", "coroutine", "credential", "cursor",
  "database", "deadlock", "debug", "decimal", "declare", "decrypt", "default",
  "delegate", "delete", "deploy", "derive", "deserialize", "destroy", "detect",
  "developer", "diagnostic", "dictionary", "directive", "directory", "dispatch",
  "display", "distribute", "document", "domain", "download", "driver", "duration",
  "dynamic", "element", "embed", "emit", "enable", "encode", "encrypt", "endpoint",
  "engine", "entity", "enumerate", "environment", "error", "evaluate", "event",
  "exception", "execute", "export", "expression", "extend", "extract", "factory",
  "fallback", "feature", "fetch", "field", "filter", "firmware", "float", "flush",
  "format", "fragment", "framework", "frequency", "function", "garbage", "gateway",
  "generate", "generic", "global", "gradient", "graph", "handle", "hardware",
  "hash", "header", "heap", "hostname", "http", "hybrid", "hyperlink",
  "identifier", "immutable", "implement", "import", "increment", "index",
  "infinite", "inherit", "initialize", "injection", "inline", "input", "insert",
  "instance", "integer", "integrate", "interface", "internal", "interpolate",
  "interrupt", "interval", "invoke", "isolate", "iterate", "javascript",
  "kernel", "keyboard", "keyword", "lambda", "latency", "launch", "layout",
  "library", "lifecycle", "linear", "listener", "literal", "localhost",
  "logging", "logic", "lookup", "machine", "manifest", "mapping", "marshal",
  "measure", "memory", "merge", "message", "metadata", "method", "middleware",
  "migrate", "module", "monitor", "mount", "multiply", "mutex", "namespace",
  "navigate", "network", "normalize", "notification", "nullable", "object",
  "observe", "offset", "operate", "optimize", "optional", "orchestrate",
  "output", "overflow", "override", "package", "parallel", "parameter",
  "parse", "partition", "password", "pattern", "payload", "performance",
  "permission", "persist", "pipeline", "platform", "plugin", "pointer",
  "policy", "polymorphism", "populate", "portable", "position", "predicate",
  "primary", "primitive", "priority", "private", "procedure", "process",
  "profile", "program", "progress", "projection", "promise", "property",
  "protocol", "provider", "proxy", "public", "publish", "python", "query",
  "queue", "random", "reactive", "readonly", "rebalance", "receive",
  "record", "recover", "recursive", "redirect", "reduce", "reference",
  "reflect", "refresh", "register", "release", "remote", "render", "replace",
  "replicate", "repository", "request", "require", "reset", "resolve",
  "resource", "response", "restore", "restrict", "retrieve", "return",
  "reverse", "rollback", "router", "runtime", "sandbox", "scalar", "schedule",
  "schema", "scope", "script", "search", "security", "segment", "select",
  "semaphore", "sequence", "serial", "server", "service", "session", "signal",
  "simulate", "snapshot", "socket", "software", "source", "spawn", "specify",
  "stack", "standard", "static", "storage", "stream", "string", "struct",
  "subscribe", "suspend", "switch", "symbol", "synchronize", "syntax",
  "system", "target", "template", "terminal", "test", "thread", "threshold",
  "throttle", "timeout", "timestamp", "token", "topology", "trace",
  "transaction", "transfer", "transform", "transition", "transport", "traverse",
  "trigger", "truncate", "tunnel", "typescript", "underflow", "unicode",
  "unique", "unittest", "unlock", "update", "upgrade", "upload", "upstream",
  "validate", "variable", "vector", "verify", "version", "virtual", "volume",
  "warning", "webhook", "widget", "window", "worker", "wrapper", "yield",
];

// ─── Helpers ───────────────────────────────────────────────────────────────

function scaleWords(base, target) {
  const words = [];
  for (let i = 0; i < target; i++) {
    words.push(i < base.length ? base[i] : base[i % base.length] + i);
  }
  return words;
}

function iters(base, n) {
  if (n >= 10000) return Math.max(10, Math.floor(base / 20));
  if (n >= 5000) return Math.max(20, Math.floor(base / 10));
  return base;
}

function bench(name, fn, iterations) {
  for (let i = 0; i < Math.min(iterations, 10); i++) fn(); // warmup
  const start = performance.now();
  for (let i = 0; i < iterations; i++) fn();
  const elapsed = performance.now() - start;
  return (elapsed / iterations) * 1000; // µs per op
}

function fmt(us) {
  if (us >= 1000) return `${(us / 1000).toFixed(2)}ms`;
  return `${us.toFixed(2)}µs`;
}

function speedup(fuse, kh) {
  return (fuse / kh).toFixed(1);
}

// ─── Typos ─────────────────────────────────────────────────────────────────

const TYPOS = {
  substitution: { query: "javoscript", expect: "javascript" },
  transposition: { query: "javsacript", expect: "javascript" },
  deletion: { query: "javasript", expect: "javascript" },
  insertion: { query: "javasccript", expect: "javascript" },
  adjacent_key: { query: "javadcript", expect: "javascript" },
  case_mixed: { query: "JAVASCRIPT", expect: "javascript" },
  short: { query: "pythn", expect: "python" },
  double_error: { query: "javscritp", expect: "javascript" },
  prefix_typo: { query: "xavascript", expect: "javascript" },
  suffix_typo: { query: "javascrip", expect: "javascript" },
};

const BATCH_QUERIES = [
  "javasript", "typscript", "pythn", "ruts", "golanf",
  "algortihm", "benchmerk", "framewrok", "databaes", "javscript",
];

// ─── Run ───────────────────────────────────────────────────────────────────

console.log("\n  KEYHAMMER vs FUSE.JS — COMPLETE BENCHMARK");
console.log("  Same terms, same queries, same process.\n");

for (const size of SIZES) {
  const terms = scaleWords(WORDS, size);

  console.log(`${"═".repeat(70)}`);
  console.log(`  ${size.toLocaleString()} TERMS`);
  console.log(`${"═".repeat(70)}`);

  // ── Build ──────────────────────────────────────────────────────────

  const fuseBuild = bench("fuse build", () => {
    new Fuse(terms, { threshold: 0.4, distance: 100, includeScore: true });
  }, iters(100, size));

  const khBuild = bench("kh build", () => {
    KeyhammerIndex.build(terms, 2);
  }, iters(100, size));

  console.log(`\n  BUILD`);
  console.log(`    FuseJS:     ${fmt(fuseBuild)}`);
  console.log(`    keyhammer:  ${fmt(khBuild)}`);
  console.log(`    ratio:      ${(khBuild / fuseBuild).toFixed(1)}x`);

  // ── Setup ──────────────────────────────────────────────────────────

  const fuse = new Fuse(terms, {
    threshold: 0.4,
    distance: 100,
    includeScore: true,
    includeMatches: true,
  });
  const kh = KeyhammerIndex.build(terms, 2);

  // ── Single query by typo type ──────────────────────────────────────

  console.log(`\n  SINGLE QUERY (by typo type)`);
  console.log(`    ${"type".padEnd(16)} ${"FuseJS".padEnd(12)} ${"keyhammer".padEnd(12)} speedup  both_find`);
  console.log(`    ${"─".repeat(62)}`);

  for (const [type, { query, expect }] of Object.entries(TYPOS)) {
    const it = iters(500, size);

    const fuseTime = bench("f", () => fuse.search(query), it);
    const khTime = bench("k", () => kh.search(query, 5), it);

    const fuseFound = fuse.search(query).some(r => r.item.toLowerCase() === expect);
    const khFound = kh.search(query, 5).some(r => r.term.toLowerCase() === expect);
    const both = fuseFound && khFound ? "✓✓" : fuseFound ? "F" : khFound ? "K" : "✗✗";

    console.log(`    ${type.padEnd(16)} ${fmt(fuseTime).padEnd(12)} ${fmt(khTime).padEnd(12)} ${speedup(fuseTime, khTime).padStart(6)}x  ${both}`);
  }

  // ── Batch: 10 queries ──────────────────────────────────────────────

  console.log(`\n  BATCH (10 queries)`);

  const fuseBatch = bench("f", () => {
    for (const q of BATCH_QUERIES) fuse.search(q);
  }, iters(200, size));

  const khBatch = bench("k", () => {
    for (const q of BATCH_QUERIES) kh.search(q, 5);
  }, iters(200, size));

  console.log(`    FuseJS:     ${fmt(fuseBatch)}  (${fmt(fuseBatch / 10)}/query)`);
  console.log(`    keyhammer:  ${fmt(khBatch)}  (${fmt(khBatch / 10)}/query)`);
  console.log(`    speedup:    ${speedup(fuseBatch, khBatch)}x`);

  // ── Throughput: 100 queries ────────────────────────────────────────

  const queries100 = BATCH_QUERIES.flatMap(q => Array(10).fill(q));

  const fuseTp = bench("f", () => {
    for (const q of queries100) fuse.search(q);
  }, iters(50, size));

  const khTp = bench("k", () => {
    for (const q of queries100) kh.search(q, 5);
  }, iters(50, size));

  const fuseQps = Math.round(100 / (fuseTp / 1e6));
  const khQps = Math.round(100 / (khTp / 1e6));

  console.log(`\n  THROUGHPUT (100 queries)`);
  console.log(`    FuseJS:     ${fuseQps.toLocaleString()} q/s`);
  console.log(`    keyhammer:  ${khQps.toLocaleString()} q/s`);
  console.log(`    speedup:    ${speedup(khQps, fuseQps)}x`);

  // ── Miss (no results) ──────────────────────────────────────────────

  const fuseMiss = bench("f", () => fuse.search("zzzzzzzzz"), iters(500, size));
  const khMiss = bench("k", () => kh.search("zzzzzzzzz", 5), iters(500, size));

  console.log(`\n  MISS (no results)`);
  console.log(`    FuseJS:     ${fmt(fuseMiss)}`);
  console.log(`    keyhammer:  ${fmt(khMiss)}`);
  console.log(`    speedup:    ${speedup(fuseMiss, khMiss)}x`);

  // ── With highlighting ──────────────────────────────────────────────

  const fuseHL = new Fuse(terms, {
    threshold: 0.4, distance: 100, includeScore: true, includeMatches: true,
  });

  const fuseHLTime = bench("f", () => fuseHL.search("javasript"), iters(500, size));
  const khHLTime = bench("k", () => {
    const r = kh.search("javasript", 5);
    r.forEach(x => x.matchRanges); // access ranges
  }, iters(500, size));

  console.log(`\n  WITH HIGHLIGHTING`);
  console.log(`    FuseJS:     ${fmt(fuseHLTime)}`);
  console.log(`    keyhammer:  ${fmt(khHLTime)}`);
  console.log(`    speedup:    ${speedup(fuseHLTime, khHLTime)}x`);

  // ── Threshold filtering ────────────────────────────────────────────

  const fuseThresh = new Fuse(terms, { threshold: 0.2, includeScore: true });
  const fuseThTime = bench("f", () => fuseThresh.search("javasript"), iters(500, size));
  const khThTime = bench("k", () => kh.searchWithThreshold("javasript", 5, 0.8), iters(500, size));

  console.log(`\n  WITH THRESHOLD`);
  console.log(`    FuseJS (0.2):     ${fmt(fuseThTime)}`);
  console.log(`    keyhammer (0.8):  ${fmt(khThTime)}`);
  console.log(`    speedup:          ${speedup(fuseThTime, khThTime)}x`);

  // ── Dynamic add ────────────────────────────────────────────────────

  const khDyn = KeyhammerIndex.build(terms.slice(0, 100), 2);
  const addTime = bench("k", () => {
    khDyn.add("newterm");
  }, 1000);
  // fuse has no .add() that's equivalent — it rebuilds internally

  console.log(`\n  DYNAMIC ADD`);
  console.log(`    keyhammer add:  ${fmt(addTime)}/term`);
  console.log(`    FuseJS:         no equivalent (must rebuild)`);

  // ── Result quality ─────────────────────────────────────────────────

  console.log(`\n  RESULT QUALITY ("javasript")`);
  const fuseRes = fuse.search("javasript").slice(0, 5).map(r => r.item);
  const khRes = kh.search("javasript", 5).map(r => r.term);
  console.log(`    FuseJS:     [${fuseRes.join(", ")}]`);
  console.log(`    keyhammer:  [${khRes.join(", ")}]`);

  console.log();
}
