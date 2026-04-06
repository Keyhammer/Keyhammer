use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::time::Duration;
use keyhammer::FuzzyIndex;

fn fast_config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
}

/// 350+ real programming terms.
fn real_words() -> Vec<&'static str> {
    vec![
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
    ]
}

fn scale_words(base: &[&str], target: usize) -> Vec<String> {
    let mut words = Vec::with_capacity(target);
    for i in 0..target {
        let base_word = base[i % base.len()];
        if i < base.len() {
            words.push(base_word.to_string());
        } else {
            words.push(format!("{}{}", base_word, i));
        }
    }
    words
}

/// Different typo types.
fn make_typos(word: &str) -> Vec<(String, &'static str)> {
    let bytes = word.as_bytes();
    if bytes.len() < 4 { return vec![(word.to_string(), "none")]; }
    let mut out = Vec::new();

    // substitution at middle
    let mut sub = bytes.to_vec();
    sub[bytes.len() / 2] = if sub[bytes.len() / 2] == b'a' { b'e' } else { b'a' };
    out.push((String::from_utf8(sub).unwrap(), "substitution"));

    // transposition
    let mut trans = bytes.to_vec();
    let p = bytes.len() / 3;
    trans.swap(p, p + 1);
    out.push((String::from_utf8(trans).unwrap(), "transposition"));

    // deletion (omit middle char)
    let mut del = bytes.to_vec();
    del.remove(bytes.len() / 2);
    out.push((String::from_utf8(del).unwrap(), "deletion"));

    // insertion (duplicate middle char)
    let mut ins = bytes.to_vec();
    ins.insert(bytes.len() / 2, ins[bytes.len() / 2]);
    out.push((String::from_utf8(ins).unwrap(), "insertion"));

    // adjacent key (s→d, common fat-finger)
    let mut adj = bytes.to_vec();
    for b in adj.iter_mut() {
        if *b == b's' { *b = b'd'; break; }
        if *b == b'a' { *b = b's'; break; }
        if *b == b'e' { *b = b'r'; break; }
    }
    out.push((String::from_utf8(adj).unwrap(), "adjacent_key"));

    out
}

// ─── Benchmarks ─────────────────────────────────────────────────────────────

fn bench_build(c: &mut Criterion) {
    let base = real_words();
    let mut group = c.benchmark_group("build");
    for size in [100, 500, 1_000, 3_000] {
        let words = scale_words(&base, size);
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        group.bench_with_input(BenchmarkId::from_parameter(size), &refs, |b, terms| {
            b.iter(|| FuzzyIndex::build(black_box(terms), 2).unwrap());
        });
    }
    group.finish();
}

/// 10 different typo queries per iteration.
fn bench_search_typos(c: &mut Criterion) {
    let base = real_words();
    let mut group = c.benchmark_group("search_typo");
    for size in [100, 500, 1_000, 3_000] {
        let words = scale_words(&base, size);
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        let index = FuzzyIndex::build(&refs, 2).unwrap();

        let typos: Vec<String> = (0..10)
            .map(|i| {
                let w = base[i * 7 % base.len()];
                make_typos(w)[i % 5].0.clone()
            })
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                for typo in &typos {
                    black_box(index.search(typo, 5).unwrap());
                }
            });
        });
    }
    group.finish();
}

/// Single query latency — the number that matters for autocomplete.
fn bench_single_query_latency(c: &mut Criterion) {
    let base = real_words();
    let mut group = c.benchmark_group("single_query");
    for size in [100, 500, 1_000, 3_000] {
        let words = scale_words(&base, size);
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        let index = FuzzyIndex::build(&refs, 2).unwrap();
        let typo = "javasript"; // classic deletion typo

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(index.search(typo, 5).unwrap()));
        });
    }
    group.finish();
}

/// By typo type — which errors are cheapest/costliest to find?
fn bench_by_typo_type(c: &mut Criterion) {
    let base = real_words();
    let words = scale_words(&base, 1_000);
    let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
    let index = FuzzyIndex::build(&refs, 2).unwrap();

    let mut group = c.benchmark_group("typo_type_1k");
    for (typo, label) in make_typos("javascript") {
        group.bench_with_input(BenchmarkId::from_parameter(label), &typo, |b, q| {
            b.iter(|| black_box(index.search(q, 5).unwrap()));
        });
    }
    group.finish();
}

/// keyhammer vs naked brute force at multiple scales.
fn bench_vs_brute(c: &mut Criterion) {
    let base = real_words();
    let mut group = c.benchmark_group("vs_brute");

    for size in [100, 500, 1_000, 3_000] {
        let words = scale_words(&base, size);
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        let index = FuzzyIndex::build(&refs, 2).unwrap();

        let typo = "javasript";
        let typo_bytes = typo.as_bytes();

        group.bench_with_input(
            BenchmarkId::new("keyhammer", size), &size, |b, _| {
                b.iter(|| black_box(index.search(typo, 5).unwrap()));
            },
        );

        // naked brute force: hamming only, no scoring, no variants
        let terms_bytes: Vec<&[u8]> = refs.iter().map(|s| s.as_bytes()).collect();
        group.bench_with_input(
            BenchmarkId::new("brute_hamming_only", size), &size, |b, _| {
                b.iter(|| {
                    let mut results: Vec<(usize, usize)> = Vec::new();
                    for (i, term) in terms_bytes.iter().enumerate() {
                        let len = typo_bytes.len().min(term.len());
                        let mut d = 0usize;
                        for j in 0..len {
                            if typo_bytes[j] != term[j] {
                                d += 1;
                                if d > 2 { break; }
                            }
                        }
                        if d <= 2 {
                            results.push((i, d));
                        }
                    }
                    black_box(results)
                });
            },
        );
    }
    group.finish();
}

/// Throughput: queries per second at different scales.
fn bench_throughput(c: &mut Criterion) {
    let base = real_words();
    let mut group = c.benchmark_group("throughput_qps");

    for size in [100, 500, 1_000, 3_000] {
        let words = scale_words(&base, size);
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        let index = FuzzyIndex::build(&refs, 2).unwrap();

        // 100 different queries
        let queries: Vec<String> = (0..100)
            .map(|i| {
                let w = base[i % base.len()];
                make_typos(w)[i % 5].0.clone()
            })
            .collect();

        group.throughput(criterion::Throughput::Elements(100));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                for q in &queries {
                    black_box(index.search(q, 5).unwrap());
                }
            });
        });
    }
    group.finish();
}

/// Worst case: query that matches nothing.
fn bench_miss(c: &mut Criterion) {
    let base = real_words();
    let mut group = c.benchmark_group("miss");

    for size in [100, 1_000, 3_000] {
        let words = scale_words(&base, size);
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        let index = FuzzyIndex::build(&refs, 2).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(index.search("zzzzzzzzz", 5).unwrap()));
        });
    }
    group.finish();
}

/// Best case: exact match query.
fn bench_exact_hit(c: &mut Criterion) {
    let base = real_words();
    let mut group = c.benchmark_group("exact_hit");

    for size in [100, 1_000, 3_000] {
        let words = scale_words(&base, size);
        let refs: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
        let index = FuzzyIndex::build(&refs, 2).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(index.search("javascript", 5).unwrap()));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = fast_config();
    targets = bench_build, bench_search_typos, bench_single_query_latency,
              bench_by_typo_type, bench_vs_brute, bench_throughput,
              bench_miss, bench_exact_hit
}
criterion_main!(benches);
