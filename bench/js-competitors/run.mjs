// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Competitive benchmark, part 2: the keyhammer WebAssembly build in Node against
// MiniSearch, Fuse.js, uFuzzy and fuzzysort, on the same dictionaries and typo
// pairs as the Rust benchmark (bench/competitors). Prints Markdown tables.
//
//   node --expose-gc --max-old-space-size=8192 run.mjs --help
//
// The data comes from bench/fetch-data.mjs, bench/prepare-m0-data.mjs and
// bench/fetch-typo-corpora.mjs (never committed). The WebAssembly module is
// built as described in bindings/wasm/README.md.
import { readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';
import MiniSearch from 'minisearch';
import Fuse from 'fuse.js';
import uFuzzy from '@leeoniya/ufuzzy';
import fuzzysort from 'fuzzysort';

const here = dirname(fileURLToPath(import.meta.url));
const TOP = 10;
const Z95 = 1.96;
/** Candidates taken from a library's own order by the "/rerank" variants. */
const RERANK_CAP = 100;

const HELP = `\
keyhammer JS competitors: the WebAssembly build against JavaScript fuzzy-search libraries

USAGE:
    node --expose-gc --max-old-space-size=8192 run.mjs [OPTIONS]

OPTIONS:
    --wasm FILE            keyhammer_wasm.wasm [default: ../../target/wasm32-unknown-unknown/wasm/keyhammer_wasm.wasm]
    --data DIR             directory with words-{10000,100000,full}.tsv and tests.tsv [default: ../data]
    --corpus-dir DIR       directory with gtc.tsv and wiki.tsv [default: the --data directory]
    --corpora LIST         comma-separated: birkbeck (tests.tsv), gtc, wiki [default: birkbeck,gtc,wiki]
    --sizes LIST           comma-separated dictionary sizes: 10000, 100000, full [default: 10000,100000,full]
    --engines LIST         comma-separated: keyhammer, keyhammer/hr, minisearch, fuse, ufuzzy, fuzzysort
                           [default: all]. fuse and fuzzysort accept a threshold, for example fuse:0.6
    --queries N            use at most the first N usable pairs of each corpus (smoke runs)
    --repetitions N        latency runs per configuration; the report takes the median [default: 3]
    --min-queries N        timed queries per run at least (the query set is repeated) [default: 1000]
    --max-run-seconds S    stop a latency run once it took S seconds and has at least 100 timed
                           queries (keeps the slow libraries feasible) [default: 20]
    --builds N             builds per engine for the build-time median [default: 3]
    --load-runs N          fresh-process runs for the WebAssembly load time [default: 15]
    --skip-latency         quality, memory and load time only
    --latency-only         latency only
    --builds-only          build time, index memory and WebAssembly load time only (no quality, no latency)
    -h, --help             print this help

The JavaScript libraries are ranked by their own scoring. The "/rerank" rows (quality only) take
the first ${RERANK_CAP} candidates of the library's own order and re-sort them by (OSA distance,
higher weight, term), the rule of the Rust report's symspell/rerank row; they are a separate
variant and are labelled as such.
`;

function parseArgs(argv) {
  const a = {
    wasm: resolve(here, '..', '..', 'target', 'wasm32-unknown-unknown', 'wasm', 'keyhammer_wasm.wasm'),
    data: resolve(here, '..', 'data'),
    corpusDir: null,
    corpora: ['birkbeck', 'gtc', 'wiki'],
    sizes: ['10000', '100000', 'full'],
    engines: ['keyhammer', 'keyhammer/hr', 'minisearch', 'fuse', 'ufuzzy', 'fuzzysort'],
    queries: Infinity,
    repetitions: 3,
    minQueries: 1000,
    maxRunSeconds: 20,
    builds: 3,
    loadRuns: 15,
    quality: true,
    latency: true,
    buildsOnly: false,
    loadChild: false,
    buildChild: null,
  };
  const list = (v) => v.split(',').map((s) => s.trim()).filter(Boolean);
  const num = (name, v, min = 1) => {
    const n = Number(v);
    if (!Number.isFinite(n) || n < min) throw new Error(`${name} needs a number >= ${min}`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const f = argv[i];
    const val = () => {
      if (i + 1 >= argv.length) throw new Error(`${f} needs a value`);
      return argv[++i];
    };
    switch (f) {
      case '-h': case '--help': console.log(HELP); process.exit(0); break;
      case '--wasm': a.wasm = resolve(val()); break;
      case '--data': a.data = resolve(val()); break;
      case '--corpus-dir': a.corpusDir = resolve(val()); break;
      case '--corpora': a.corpora = list(val()); break;
      case '--sizes': a.sizes = list(val()); break;
      case '--engines': a.engines = list(val()); break;
      case '--queries': a.queries = num(f, val()); break;
      case '--repetitions': a.repetitions = num(f, val()); break;
      case '--min-queries': a.minQueries = num(f, val()); break;
      case '--max-run-seconds': a.maxRunSeconds = num(f, val()); break;
      case '--builds': a.builds = num(f, val()); break;
      case '--load-runs': a.loadRuns = num(f, val()); break;
      case '--skip-latency': a.latency = false; break;
      case '--latency-only': a.quality = false; break;
      case '--builds-only': a.buildsOnly = true; a.latency = false; break;
      case '--load-child': a.loadChild = true; break;
      case '--build-child': a.buildChild = val(); break;
      default: throw new Error(`unknown option ${f} (see --help)`);
    }
  }
  a.corpusDir ??= a.data;
  for (const s of a.sizes) if (!['10000', '100000', 'full'].includes(s)) throw new Error(`unknown size ${s}`);
  for (const c of a.corpora) if (!['birkbeck', 'gtc', 'wiki'].includes(c)) throw new Error(`unknown corpus ${c}`);
  for (const e of a.engines) if (!(e.split(':')[0] in FACTORIES)) throw new Error(`unknown engine ${e}`);
  return a;
}

// --- data -----------------------------------------------------------------------

function readPairs(path) {
  if (!existsSync(path)) throw new Error(`cannot read ${path}; see the data steps under "How to reproduce" in docs/benchmarks/competitors-js.md`);
  const out = [];
  for (const l of readFileSync(path, 'utf8').split('\n')) {
    if (l.trim() === '') continue;
    const t = l.replace(/\r$/, '').split('\t');
    if (t.length < 2) throw new Error(`${path}: line without a tab: ${l}`);
    out.push([t[0], t[1]]);
  }
  return out;
}

function readDict(path) {
  return readPairs(path).map(([w, f]) => {
    if (!/^\d+$/.test(f.trim()) || Number(f) > 65535) throw new Error(`${path}: bad weight for ${w}`);
    return [w.toLowerCase(), Number(f)];
  });
}

function corpusPath(a, name) {
  return name === 'birkbeck' ? join(a.data, 'tests.tsv') : join(a.corpusDir, `${name}.tsv`);
}

/** Pairs whose correct word is in the dictionary and whose typo is not (both lower-cased). */
function usable(pairs, dict) {
  return pairs
    .map(([t, c]) => [t.toLowerCase(), c.toLowerCase()])
    .filter(([t, c]) => dict.has(c) && !dict.has(t) && t !== c);
}

// --- statistics -----------------------------------------------------------------

const mean = (v) => (v.length ? v.reduce((s, x) => s + x, 0) / v.length : 0);

/** Paired difference a - b: mean and standard error (sample sd / sqrt(n)). */
function paired(a, b) {
  const d = a.map((x, i) => x - b[i]);
  const m = mean(d);
  if (d.length < 2) return [m, 0];
  const v = d.reduce((s, x) => s + (x - m) ** 2, 0) / (d.length - 1);
  return [m, Math.sqrt(v / d.length)];
}

const percentile = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
const median = (v) => [...v].sort((x, y) => x - y)[Math.floor(v.length / 2)];
const fmt = (x, d = 1) => x.toFixed(d);
const sgn = (x, d = 4) => (x >= 0 ? '+' : '-') + Math.abs(x).toFixed(d);

// --- optimal string alignment distance (for the rerank variants) ------------------

function osa(a, b) {
  const m = a.length, n = b.length;
  let p2 = new Array(n + 1).fill(0);
  let p1 = Array.from({ length: n + 1 }, (_, j) => j);
  let c = new Array(n + 1).fill(0);
  for (let i = 1; i <= m; i++) {
    c[0] = i;
    for (let j = 1; j <= n; j++) {
      const cost = a.charCodeAt(i - 1) === b.charCodeAt(j - 1) ? 0 : 1;
      let v = Math.min(p1[j] + 1, c[j - 1] + 1, p1[j - 1] + cost);
      if (i > 1 && j > 1 && a.charCodeAt(i - 1) === b.charCodeAt(j - 2) && a.charCodeAt(i - 2) === b.charCodeAt(j - 1)) {
        v = Math.min(v, p2[j - 2] + 1);
      }
      c[j] = v;
    }
    [p2, p1, c] = [p1, c, p2];
  }
  return p1[n];
}

// --- keyhammer WebAssembly ----------------------------------------------------------

const ERR = 0xffffffff;
const enc = new TextEncoder();
const dec = new TextDecoder();

class KeyhammerWasm {
  constructor(instance) {
    this.kh = instance.exports;
    this.scratch = this.kh.kh_alloc(256); // query buffer, reused for every search
    this.errors = 0;
    this.truncated = 0;
  }
  get memoryBytes() {
    return this.kh.memory.buffer.byteLength;
  }
  /** words: array of [term, weight]. */
  build(words) {
    const data = enc.encode(words.map(([w, f]) => `${w}\t${f}`).join('\n'));
    const ptr = this.kh.kh_alloc(data.length);
    if (ptr === 0) throw new Error('kh_alloc failed');
    new Uint8Array(this.kh.memory.buffer, ptr, data.length).set(data);
    const n = this.kh.kh_build(ptr, data.length) >>> 0;
    this.kh.kh_free(ptr, data.length);
    if (n === 0) throw new Error('kh_build failed');
    return n;
  }
  search(q, k, budget) {
    const { kh } = this;
    const { written } = enc.encodeInto(q, new Uint8Array(kh.memory.buffer, this.scratch, 256));
    const n = kh.kh_search(this.scratch, written, k, budget, 0) >>> 0;
    if (n === ERR) {
      this.errors++;
      return [];
    }
    const text = dec.decode(new Uint8Array(kh.memory.buffer, kh.kh_results_ptr(), kh.kh_results_len()));
    const lines = text.split('\n');
    if (lines[0].endsWith('\t1')) this.truncated++;
    const out = [];
    for (let i = 1; i <= n; i++) out.push(lines[i].slice(0, lines[i].indexOf('\t')));
    return out;
  }
}

async function instantiate(bytes) {
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return new KeyhammerWasm(instance);
}

// --- engines ------------------------------------------------------------------------
// Each factory takes (words, ctx) with words = [[term, weight], ...] and returns
// { search(q) -> top-10 terms in the engine's own order, cand(q) -> up to RERANK_CAP
// candidates in the engine's own order (absent for keyhammer), extra() -> notes }.

const FACTORIES = {
  keyhammer: async (words, ctx, budget = 32) => {
    const kh = await instantiate(ctx.wasmBytes);
    const memBefore = kh.memoryBytes;
    kh.build(words);
    return {
      search: (q) => kh.search(q, TOP, budget),
      wasmBytes: kh.memoryBytes - memBefore,
      health: () => ({ errors: kh.errors, truncated: kh.truncated }),
    };
  },
  'keyhammer/hr': (words, ctx) => FACTORIES.keyhammer(words, ctx, 48),
  minisearch: async (words, ctx) => {
    const dict = ctx.terms;
    const ms = new MiniSearch({ fields: ['t'] });
    ms.addAll(dict.map((t, id) => ({ id, t })));
    const run = (q) => ms.search(q, { fuzzy: 2, prefix: false });
    return {
      search: (q) => run(q).slice(0, TOP).map((r) => dict[r.id]),
      cand: (q) => run(q).slice(0, RERANK_CAP).map((r) => dict[r.id]),
    };
  },
  fuse: async (words, ctx, threshold = 0.4) => {
    const dict = ctx.terms;
    const f = new Fuse(dict, { threshold: Number(threshold), ignoreLocation: true });
    return {
      search: (q) => f.search(q, { limit: TOP }).map((r) => r.item),
      cand: (q) => f.search(q, { limit: RERANK_CAP }).map((r) => r.item),
    };
  },
  ufuzzy: async (words, ctx) => {
    const dict = ctx.terms;
    const uf = new uFuzzy({ intraMode: 1 });
    // infoThresh is raised so that the library always ranks its matches: with the default (1000)
    // it returns unranked matches, in dictionary order, whenever there are more than 1000.
    const ranked = (q, n) => {
      const [idxs, info, order] = uf.search(dict, q, 0, 1e9);
      if (!idxs) return [];
      return order ? order.slice(0, n).map((o) => dict[info.idx[o]]) : idxs.slice(0, n).map((i) => dict[i]);
    };
    return {
      search: (q) => ranked(q, TOP),
      cand: (q) => ranked(q, RERANK_CAP),
    };
  },
  fuzzysort: async (words, ctx, threshold = 0) => {
    const dict = ctx.terms;
    const prepared = dict.map((w) => fuzzysort.prepare(w));
    const th = Number(threshold);
    return {
      search: (q) => fuzzysort.go(q, prepared, { limit: TOP, threshold: th }).map((r) => r.target),
      cand: (q) => fuzzysort.go(q, prepared, { limit: RERANK_CAP, threshold: th }).map((r) => r.target),
    };
  },
};

async function makeEngine(name, words, ctx) {
  const [base, param] = name.split(':');
  return FACTORIES[base](words, ctx, ...(param === undefined ? [] : [param]));
}

// --- memory and build time -----------------------------------------------------------

function heap() {
  globalThis.gc();
  globalThis.gc();
  const m = process.memoryUsage();
  return { heap: m.heapUsed, buffers: m.arrayBuffers };
}

/** In a fresh process (so that no earlier engine disturbs the heap): builds one engine `builds` times.
 * Memory comes from the first build, time is the median of the builds. Prints JSON. */
async function buildChild(a, name, size) {
  const words = readDict(join(a.data, `words-${size}.tsv`));
  const terms = words.map(([w]) => w);
  const ctx = { wasmBytes: readFileSync(a.wasm), terms };
  const before = heap();
  let t0 = performance.now();
  let eng = await makeEngine(name, words, ctx);
  const times = [performance.now() - t0];
  const after = heap();
  const row = { heap: after.heap - before.heap, buffers: after.buffers - before.buffers, wasm: eng.wasmBytes ?? 0 };
  for (let i = 1; i < a.builds; i++) {
    eng = null;
    globalThis.gc();
    t0 = performance.now();
    eng = await makeEngine(name, words, ctx);
    times.push(performance.now() - t0);
  }
  console.log(JSON.stringify({ ...row, times }));
}

function measureBuilds(a, size, count) {
  const rows = [];
  for (const name of a.engines) {
    const args = [
      '--expose-gc', '--max-old-space-size=8192', fileURLToPath(import.meta.url),
      '--build-child', name, '--sizes', size, '--data', a.data, '--wasm', a.wasm, '--builds', String(a.builds),
    ];
    const r = spawnSync(process.execPath, args, { encoding: 'utf8' });
    if (r.status !== 0) throw new Error(`build child failed for ${name}: ${r.stderr}`);
    const j = JSON.parse(r.stdout.trim().split('\n').pop());
    rows.push({ name, ...j, ms: median(j.times), range: `${fmt(Math.min(...j.times), 0)}-${fmt(Math.max(...j.times), 0)}` });
  }
  console.log(`\n### Build time and index memory, ${size === 'full' ? '274 137 (full)' : size} terms\n`);
  console.log(`| Engine | Build (ms), median of ${a.builds} | Build range (ms) | JS heap delta (bytes) | Heap bytes/term | Other buffers delta (bytes) | Wasm linear memory growth (bytes) | Wasm bytes/term |`);
  console.log('|---|---|---|---|---|---|---|---|');
  for (const r of rows) {
    console.log(
      `| ${r.name} | ${fmt(r.ms, 0)} | ${r.range} | ${r.heap} | ${fmt(r.heap / count)} | ${r.buffers} | ${r.wasm || '-'} | ${r.wasm ? fmt(r.wasm / count) : '-'} |`,
    );
  }
}

// --- quality ---------------------------------------------------------------------------

function verdict(d, se) {
  const z = se > 0 ? Math.abs(d) / se : 0;
  if (z < 1.5) return ['not distinguishable', z];
  if (z < Z95) return [d > 0 ? 'keyhammer ahead (weak: interval includes 0)' : 'keyhammer behind (weak: interval includes 0)', z];
  return [d > 0 ? 'keyhammer ahead' : 'keyhammer behind', z];
}

function rerank(cands, q, weights) {
  return cands
    .map((t) => ({ t, d: osa(q, t) }))
    .sort((x, y) => x.d - y.d || weights.get(y.t) - weights.get(x.t) || (x.t < y.t ? -1 : x.t > y.t ? 1 : 0))
    .slice(0, TOP)
    .map((x) => x.t);
}

/** Returns the rows of the quality table and the paired-difference rows for one (corpus, size). */
async function qualityFor(a, size, words, ctx, pairs, weights) {
  const views = []; // { name, search }
  const health = [];
  for (const name of a.engines) {
    const eng = await makeEngine(name, words, ctx);
    views.push({ name, search: eng.search });
    if (eng.health) health.push([name, eng.health]);
    if (eng.cand) {
      views.push({ name: `${name}/rerank`, search: (q) => rerank(eng.cand(q), q, weights) });
    }
  }
  const res = views.map((v) => {
    const rr = [];
    let r1 = 0, r10 = 0, none = 0;
    for (const [typo, right] of pairs) {
      const top = v.search(typo);
      const rank = top.indexOf(right);
      rr.push(rank < 0 ? 0 : 1 / (rank + 1));
      r1 += rank === 0;
      r10 += rank >= 0;
      none += top.length === 0;
    }
    const n = Math.max(1, pairs.length);
    return { name: v.name, rr, mrr: mean(rr), r1: r1 / n, r10: r10 / n, none };
  });
  const errs = health.map(([n, h]) => ({ name: n, ...h() }));
  return { res, errs };
}

async function runQuality(a, ctxFor, dicts) {
  const out = new Map(); // corpus -> [{size, usableCount, total, within2, res}]
  const problems = [];
  for (const corpus of a.corpora) {
    const all = readPairs(corpusPath(a, corpus));
    for (const size of a.sizes) {
      const { words, set, weights } = dicts.get(size);
      let pairs = usable(all, set);
      const usableCount = pairs.length;
      pairs = pairs.slice(0, a.queries);
      const within2 = pairs.filter(([t, c]) => osa(t, c) <= 2).length;
      const { res, errs } = await qualityFor(a, size, words, ctxFor(size), pairs, weights);
      if (!out.has(corpus)) out.set(corpus, []);
      out.get(corpus).push({ size, words: words.length, total: all.length, usableCount, n: pairs.length, within2, res });
      for (const e of errs) if (e.errors || e.truncated) problems.push(`${corpus} / ${size} / ${e.name}: ${e.errors} errors, ${e.truncated} truncated searches`);
    }
  }
  const label = { birkbeck: 'Birkbeck', gtc: 'GitHub Typo Corpus', wiki: 'Wikipedia misspellings' };
  console.log('\n## Quality\n');
  for (const [corpus, blocks] of out) {
    console.log(`### ${label[corpus]}\n`);
    for (const b of blocks) {
      console.log(
        `${b.size === 'full' ? '274 137 (full)' : b.size} terms: ${b.usableCount} of ${b.total} pairs usable` +
          (b.n < b.usableCount ? `, first ${b.n} used` : '') +
          `, ${b.within2} (${fmt(b.within2 / Math.max(1, b.n), 3)}) within OSA distance 2.\n`,
      );
      console.log('| Engine | MRR@10 | R@1 | R@10 | queries with no result |');
      console.log('|---|---|---|---|---|');
      for (const r of b.res) console.log(`| ${r.name} | ${fmt(r.mrr, 3)} | ${fmt(r.r1, 3)} | ${fmt(r.r10, 3)} | ${r.none} |`);
      console.log('');
    }
    console.log(
      'Paired MRR differences, keyhammer minus the competitor (SE = sd / sqrt(n); verdict: "not distinguishable" when abs(difference) < 1.5 SE, "weak" between 1.5 and 1.96 SE, otherwise the sign says who is ahead; the 95% interval is difference +- 1.96 SE). `keyhammer/hr` is compared with the JS libraries and `keyhammer` with everything else.\n',
    );
    console.log('| Terms | Reference | Competitor | Difference | SE | 95% interval | abs(diff)/SE | Queries that differ | Verdict |');
    console.log('|---|---|---|---|---|---|---|---|---|');
    for (const b of blocks) {
      const by = new Map(b.res.map((r) => [r.name, r]));
      for (const ref of ['keyhammer', 'keyhammer/hr']) {
        const kh = by.get(ref);
        if (!kh) continue;
        for (const r of b.res) {
          // keyhammer is compared with every other row; keyhammer/hr with the rows that are not keyhammer.
          if (r.name === ref || (ref === 'keyhammer/hr' && r.name === 'keyhammer')) continue;
          const [d, se] = paired(kh.rr, r.rr);
          const [v, z] = verdict(d, se);
          const differ = kh.rr.reduce((s, x, i) => s + (x !== r.rr[i]), 0);
          console.log(
            `| ${b.size === 'full' ? '274 137 (full)' : b.size} | ${ref} | ${r.name} | ${sgn(d)} | ${fmt(se, 4)} | [${sgn(d - Z95 * se)}, ${sgn(d + Z95 * se)}] | ${fmt(z)} | ${differ} | ${v} |`,
          );
        }
      }
    }
    console.log('');
  }
  if (problems.length) console.log('Engine problems:\n' + problems.map((p) => `- ${p}`).join('\n') + '\n');
  else console.log('keyhammer search errors and searches cut short by the node limit: 0 in every configuration.\n');
}

// --- latency -----------------------------------------------------------------------------

function timeOnce(search, queries, a) {
  // Warm-up: up to 200 queries, or fewer (at least 20) once it has taken 5 seconds, so that a
  // slow library does not spend minutes on it.
  const w0 = performance.now();
  for (let i = 0; i < Math.min(queries.length, 200); i++) {
    search(queries[i]);
    if (i >= 19 && performance.now() - w0 > 5000) break;
  }
  globalThis.gc();
  const reps = Math.max(1, Math.ceil(a.minQueries / queries.length));
  const lat = [];
  const wall = performance.now();
  let stop = false;
  for (let r = 0; r < reps && !stop; r++) {
    for (const q of queries) {
      const t = performance.now();
      const res = search(q);
      lat.push((performance.now() - t) * 1e3);
      if (res === undefined) throw new Error('search returned nothing');
      // A slow engine stops here, so it is timed on the first N queries of the set only, not on the
      // whole set the other engines run; its percentiles compare a different query subset.
      if (lat.length >= 100 && performance.now() - wall > a.maxRunSeconds * 1e3) {
        stop = true;
        break;
      }
    }
  }
  const total = (performance.now() - wall) / 1e3;
  const sorted = [...lat].sort((x, y) => x - y);
  return { p50: percentile(sorted, 0.5), p95: percentile(sorted, 0.95), p99: percentile(sorted, 0.99), qps: lat.length / total, n: lat.length };
}

async function runLatency(a, ctxFor, dicts) {
  const label = { birkbeck: 'Birkbeck', gtc: 'GitHub Typo Corpus', wiki: 'Wikipedia misspellings' };
  console.log('\n## Latency\n');
  console.log(
    'Per query, in microseconds, including the JavaScript work of calling the engine and returning the top-10 terms as strings. Median of ' +
      `${a.repetitions} runs (the engines interleave within each repetition); "timed" is the number of timed queries per run.\n`,
  );
  const perCorpus = new Map();
  for (const size of a.sizes) {
    const { words, set } = dicts.get(size);
    const engines = [];
    for (const name of a.engines) engines.push({ name, eng: await makeEngine(name, words, ctxFor(size)) });
    for (const corpus of a.corpora) {
      const queries = usable(readPairs(corpusPath(a, corpus)), set).slice(0, a.queries).map(([t]) => t);
      if (!queries.length) {
        console.log(`Latency ${size} / ${corpus}: no usable pairs, skipped\n`);
        continue;
      }
      const runs = engines.map(() => []);
      for (let r = 0; r < a.repetitions; r++) {
        engines.forEach((e, i) => runs[i].push(timeOnce(e.eng.search, queries, a)));
      }
      if (!perCorpus.has(corpus)) perCorpus.set(corpus, []);
      perCorpus.get(corpus).push({ size, queries: queries.length, rows: engines.map((e, i) => ({ name: e.name, runs: runs[i] })) });
    }
    engines.length = 0;
    globalThis.gc();
  }
  for (const [corpus, blocks] of perCorpus) {
    console.log(`### ${label[corpus]}\n`);
    console.log('| Terms | Engine | timed | p50 (us) | p95 (us) | p99 (us) | queries/s | p50 range | p95 range |');
    console.log('|---|---|---|---|---|---|---|---|---|');
    for (const b of blocks) {
      for (const r of b.rows) {
        const col = (f) => r.runs.map(f);
        const range = (v) => `${fmt(Math.min(...v))}-${fmt(Math.max(...v))}`;
        const p50 = col((x) => x.p50), p95 = col((x) => x.p95);
        console.log(
          `| ${b.size === 'full' ? '274 137 (full)' : b.size} | ${r.name} | ${median(col((x) => x.n))} | ${fmt(median(p50))} | ${fmt(median(p95))} | ${fmt(median(col((x) => x.p99)))} | ${fmt(median(col((x) => x.qps)), 0)} | ${range(p50)} | ${range(p95)} |`,
        );
      }
    }
    console.log('');
  }
}

// --- WebAssembly load time ---------------------------------------------------------------

/** In a fresh Node process: read the file, compile, instantiate. Prints JSON. */
async function loadChild(a) {
  const t0 = performance.now();
  const bytes = readFileSync(a.wasm);
  const t1 = performance.now();
  const module = await WebAssembly.compile(bytes);
  const t2 = performance.now();
  await WebAssembly.instantiate(module, {});
  const t3 = performance.now();
  console.log(JSON.stringify({ read: t1 - t0, compile: t2 - t1, instantiate: t3 - t2 }));
}

function runLoad(a) {
  const bytes = readFileSync(a.wasm);
  const runs = [];
  for (let i = 0; i < a.loadRuns; i++) {
    const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), '--load-child', '--wasm', a.wasm], { encoding: 'utf8' });
    if (r.status !== 0) throw new Error(`load child failed: ${r.stderr}`);
    runs.push(JSON.parse(r.stdout));
  }
  const col = (k) => runs.map((r) => r[k]);
  const tot = runs.map((r) => r.read + r.compile + r.instantiate);
  const range = (v) => `${fmt(Math.min(...v), 2)}-${fmt(Math.max(...v), 2)}`;
  console.log('\n## WebAssembly load time\n');
  console.log(
    `Module: ${bytes.length} bytes raw, ${gzipSync(bytes, { level: 9 }).length} bytes gzip (level 9). Each of ${a.loadRuns} runs is a fresh Node process: read the file from local disk, WebAssembly.compile, WebAssembly.instantiate. The network transfer of a browser is not measured.\n`,
  );
  console.log('| Step | median (ms) | range (ms) |');
  console.log('|---|---|---|');
  for (const [name, k] of [['read the file', 'read'], ['compile', 'compile'], ['instantiate', 'instantiate']]) {
    console.log(`| ${name} | ${fmt(median(col(k)), 2)} | ${range(col(k))} |`);
  }
  console.log(`| total | ${fmt(median(tot), 2)} | ${range(tot)} |`);
  console.log('');
}

// --- main ----------------------------------------------------------------------------------

async function main() {
  const a = parseArgs(process.argv.slice(2));
  if (a.loadChild) return loadChild(a);
  if (a.buildChild) return buildChild(a, a.buildChild, a.sizes[0]);
  if (!a.loadChild && typeof globalThis.gc !== 'function') throw new Error('run with node --expose-gc (memory and latency need forced GC)');
  if (!existsSync(a.wasm)) throw new Error(`cannot read ${a.wasm}; build it as described in bindings/wasm/README.md`);
  const wasmBytes = readFileSync(a.wasm);

  console.log(`# keyhammer-js-competitors`);
  console.log(`node ${process.version}, ${process.platform} ${process.arch}, v8 ${process.versions.v8}`);
  console.log(`libraries: minisearch ${ver('minisearch')}, fuse.js ${ver('fuse.js')}, @leeoniya/ufuzzy ${ver('@leeoniya/ufuzzy')}, fuzzysort ${ver('fuzzysort')}`);

  const dicts = new Map();
  for (const size of a.sizes) {
    const words = readDict(join(a.data, `words-${size}.tsv`));
    dicts.set(size, {
      words,
      terms: words.map(([w]) => w),
      set: new Set(words.map(([w]) => w)),
      weights: new Map(words),
    });
    // JS heap of the term strings alone: the JS libraries keep references to these, and the
    // caller has them anyway, so the libraries' heap figures do not include them.
    const before = heap();
    let terms = readFileSync(join(a.data, `words-${size}.tsv`), 'utf8').split(String.fromCharCode(10)).filter(Boolean).map((l) => l.split(String.fromCharCode(9))[0]);
    const after = heap();
    console.log(`dictionary ${size}: ${words.length} terms; the term strings alone take about ${after.heap - before.heap} JS heap bytes`);
    terms = null;
  }
  const ctxFor = (size) => ({ wasmBytes, terms: dicts.get(size).terms });
  if (a.quality) {
    for (const size of a.sizes) measureBuilds(a, size, dicts.get(size).words.length);
    if (!a.buildsOnly) await runQuality(a, ctxFor, dicts);
    runLoad(a);
  }
  if (a.latency) await runLatency(a, ctxFor, dicts); // last, after everything else
}

function ver(pkg) {
  return JSON.parse(readFileSync(join(here, 'node_modules', pkg, 'package.json'), 'utf8')).version;
}

main().catch((e) => {
  console.error(`error: ${e.message}`);
  process.exit(1);
});
