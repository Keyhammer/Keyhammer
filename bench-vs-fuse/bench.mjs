import Fuse from "fuse.js";
import { createRequire } from "module";
const require = createRequire(import.meta.url);
const { KeyhammerIndex } = require("../crates/node/keyhammer.node");

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

const TYPOS = [
  "javasript",   // deletion
  "javsacript",  // transposition
  "javasccript",  // insertion
  "javoscript",  // substitution
  "typscript",   // deletion
  "pythn",       // deletion
  "algortihm",   // transposition
  "benchmerk",   // substitution
  "framewrok",   // transposition
  "databaes",    // transposition
];

function scaleWords(base, target) {
  const words = [];
  for (let i = 0; i < target; i++) {
    if (i < base.length) words.push(base[i]);
    else words.push(base[i % base.length] + i);
  }
  return words;
}

function bench(name, fn, iterations = 1000) {
  // warmup
  const warmup = Math.min(iterations, 20);
  for (let i = 0; i < warmup; i++) fn();

  const start = performance.now();
  for (let i = 0; i < iterations; i++) fn();
  const elapsed = performance.now() - start;

  const perOp = (elapsed / iterations) * 1000; // microseconds
  return { name, totalMs: elapsed.toFixed(2), perOpUs: perOp.toFixed(2), iterations };
}

// scale iterations down for larger datasets
function iters(base, numTerms) {
  if (numTerms >= 50000) return Math.max(5, Math.floor(base / 100));
  if (numTerms >= 20000) return Math.max(10, Math.floor(base / 50));
  if (numTerms >= 10000) return Math.max(20, Math.floor(base / 20));
  if (numTerms >= 5000) return Math.max(50, Math.floor(base / 10));
  return base;
}

function runSuite(label, terms) {
  console.log(`\n${"=".repeat(60)}`);
  console.log(`  ${label} (${terms.length} terms)`);
  console.log(`${"=".repeat(60)}`);

  const n = terms.length;

  // --- Build ---
  const fuseBuild = bench("fuse.js build", () => {
    new Fuse(terms, { keys: [""], threshold: 0.4, distance: 100, includeScore: true });
  }, iters(100, n));

  const khBuild = bench("keyhammer build", () => {
    KeyhammerIndex.build(terms, 2);
  }, iters(100, n));

  console.log(`\n  Build:`);
  console.log(`    fuse.js:    ${fuseBuild.perOpUs} µs`);
  console.log(`    keyhammer:  ${khBuild.perOpUs} µs`);

  // --- Setup for search ---
  const fuse = new Fuse(terms, {
    keys: [""],
    threshold: 0.4,
    distance: 100,
    includeScore: true,
  });
  // fuse on plain strings needs a different setup
  const fuseSimple = new Fuse(terms, {
    threshold: 0.4,
    distance: 100,
    includeScore: true,
  });
  const kh = KeyhammerIndex.build(terms, 2);

  // --- Single query ---
  const fuseSingle = bench("fuse.js search 'javasript'", () => {
    fuseSimple.search("javasript");
  }, iters(1000, n));

  const khSingle = bench("keyhammer search 'javasript'", () => {
    kh.search("javasript", 5);
  }, iters(1000, n));

  console.log(`\n  Single query ("javasript"):`);
  console.log(`    fuse.js:    ${fuseSingle.perOpUs} µs`);
  console.log(`    keyhammer:  ${khSingle.perOpUs} µs`);
  console.log(`    speedup:    ${(parseFloat(fuseSingle.perOpUs) / parseFloat(khSingle.perOpUs)).toFixed(1)}x`);

  // --- 10 typo queries ---
  const fuse10 = bench("fuse.js 10 typos", () => {
    for (const t of TYPOS) fuseSimple.search(t);
  }, iters(500, n));

  const kh10 = bench("keyhammer 10 typos", () => {
    for (const t of TYPOS) kh.search(t, 5);
  }, iters(500, n));

  console.log(`\n  10 typo queries:`);
  console.log(`    fuse.js:    ${fuse10.perOpUs} µs  (${(parseFloat(fuse10.perOpUs) / 10).toFixed(2)} µs/query)`);
  console.log(`    keyhammer:  ${kh10.perOpUs} µs  (${(parseFloat(kh10.perOpUs) / 10).toFixed(2)} µs/query)`);
  console.log(`    speedup:    ${(parseFloat(fuse10.perOpUs) / parseFloat(kh10.perOpUs)).toFixed(1)}x`);

  // --- Throughput: 100 queries ---
  const queries100 = [];
  for (let i = 0; i < 100; i++) queries100.push(TYPOS[i % TYPOS.length]);

  const fuseThroughput = bench("fuse.js 100 queries", () => {
    for (const q of queries100) fuseSimple.search(q);
  }, iters(100, n));

  const khThroughput = bench("keyhammer 100 queries", () => {
    for (const q of queries100) kh.search(q, 5);
  }, iters(100, n));

  const fuseQps = Math.round(100 / (parseFloat(fuseThroughput.perOpUs) / 1e6));
  const khQps = Math.round(100 / (parseFloat(khThroughput.perOpUs) / 1e6));

  console.log(`\n  Throughput (100 queries):`);
  console.log(`    fuse.js:    ${fuseQps.toLocaleString()} queries/sec`);
  console.log(`    keyhammer:  ${khQps.toLocaleString()} queries/sec`);
  console.log(`    speedup:    ${(khQps / fuseQps).toFixed(1)}x`);

  // --- Miss query ---
  const fuseMiss = bench("fuse.js miss", () => {
    fuseSimple.search("zzzzzzzzz");
  }, iters(1000, n));

  const khMiss = bench("keyhammer miss", () => {
    kh.search("zzzzzzzzz", 5);
  }, iters(1000, n));

  console.log(`\n  Miss ("zzzzzzzzz"):`);
  console.log(`    fuse.js:    ${fuseMiss.perOpUs} µs`);
  console.log(`    keyhammer:  ${khMiss.perOpUs} µs`);
  console.log(`    speedup:    ${(parseFloat(fuseMiss.perOpUs) / parseFloat(khMiss.perOpUs)).toFixed(1)}x`);

  // --- Results quality check ---
  const fuseResults = fuseSimple.search("javasript").slice(0, 5).map(r => r.item);
  const khResults = kh.search("javasript", 5).map(r => r.term);
  console.log(`\n  Results for "javasript":`);
  console.log(`    fuse.js:    [${fuseResults.join(", ")}]`);
  console.log(`    keyhammer:  [${khResults.join(", ")}]`);
}

// ─── Run ────────────────────────────────────────────────────────────────────

console.log("\n  KEYHAMMER vs FUSE.JS BENCHMARK");
console.log("  Same terms, same queries, same process.\n");

for (const size of [10000]) {
  const terms = scaleWords(WORDS, size);
  runSuite(`${size} terms`, terms);
}
