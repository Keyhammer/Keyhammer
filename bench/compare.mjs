// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
// JS comparison harness reproducing the original spike on real typos (Birkbeck corpus):
// legacy binding vs Fuse, uFuzzy, fuzzysort, MiniSearch and a plain edit-distance baseline.
import { createRequire } from "module";
import fs from "fs";
const require = createRequire(import.meta.url);
const { KeyhammerIndex } = require("../crates/node/keyhammer.node");
import Fuse from "fuse.js";
import uFuzzy from "@leeoniya/ufuzzy";
import fuzzysort from "fuzzysort";
import MiniSearch from "minisearch";
import wordList from "word-list";

const NQ = +process.env.NQ || 300;
const SIZES = (process.env.SIZES || "10000,100000,full").split(",");
const ENGINES = (process.env.ENGINES || "keyhammer,fuse,ufuzzy,fuzzysort,minisearch").split(",");

// seeded RNG
let s = 12345;
const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32);
const shuffle = (a) => { for (let i = a.length - 1; i > 0; i--) { const j = Math.floor(rnd() * (i + 1)); [a[i], a[j]] = [a[j], a[i]]; } return a; };

function dl(a, b) { // Damerau-Levenshtein (OSA)
  const m = a.length, n = b.length, d = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = 0; i <= m; i++) d[i][0] = i; for (let j = 0; j <= n; j++) d[0][j] = j;
  for (let i = 1; i <= m; i++) for (let j = 1; j <= n; j++) {
    const c = a[i - 1] === b[j - 1] ? 0 : 1;
    d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + c);
    if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
  }
  return d[m][n];
}

const words = fs.readFileSync(wordList, "utf8").split("\n").filter((w) => /^[a-z]+$/.test(w));
const wset = new Set(words);

// Birkbeck: "$correct" then misspellings
const pairs = [];
let cur = null;
for (const line of fs.readFileSync("data/missp.dat", "utf8").split(/\r?\n/)) {
  if (line.startsWith("$")) cur = line.slice(1).toLowerCase();
  else if (cur && /^[a-z]{3,}$/i.test(line)) pairs.push([line.toLowerCase(), cur]);
}
const good = pairs.filter(([t, c]) => /^[a-z]{3,}$/.test(c) && wset.has(c) && !wset.has(t) && t !== c);
shuffle(good);
const tests = good.slice(0, NQ).map(([t, c]) => ({ typo: t, right: c, dist: dl(t, c) }));
const within2 = tests.filter((t) => t.dist <= 2).length;
console.log(`dictionary words: ${words.length}; usable typo pairs: ${good.length}; tests: ${tests.length}; DL<=2: ${within2} (${((100 * within2) / tests.length).toFixed(0)}%)`);

const targets = [...new Set(tests.map((t) => t.right))];
const others = shuffle(words.filter((w) => !targets.includes(w)));

function makeDict(size) {
  const n = size === "full" ? words.length : +size;
  return shuffle([...targets, ...others.slice(0, Math.max(0, n - targets.length))]);
}

const freq = new Map();
for (const w of fs.readFileSync("data/big.txt", "utf8").toLowerCase().match(/[a-z]+/g)) freq.set(w, (freq.get(w) || 0) + 1);

// bounded OSA distance (returns >k early)
function osa(a, b, k) {
  const m = a.length, n = b.length;
  if (Math.abs(m - n) > k) return k + 1;
  let p2 = new Array(n + 1).fill(0), p1 = Array.from({ length: n + 1 }, (_, j) => j), c = new Array(n + 1).fill(0);
  for (let i = 1; i <= m; i++) {
    c[0] = i; let mn = c[0];
    for (let j = 1; j <= n; j++) {
      const cost = a.charCodeAt(i - 1) === b.charCodeAt(j - 1) ? 0 : 1;
      let v = Math.min(p1[j] + 1, c[j - 1] + 1, p1[j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) v = Math.min(v, p2[j - 2] + 1);
      c[j] = v; if (v < mn) mn = v;
    }
    if (mn > k) return k + 1;
    [p2, p1, c] = [p1, c, p2];
  }
  return p1[n];
}
const dlEngine = (useFreq) => (dict) => (q) => {
  const cand = [];
  for (const w of dict) { const d = osa(q, w, 2); if (d <= 2) cand.push([w, d]); }
  cand.sort((x, y) => x[1] - y[1] || (useFreq ? (freq.get(y[0]) || 0) - (freq.get(x[0]) || 0) : Math.abs(x[0].length - q.length) - Math.abs(y[0].length - q.length) || (x[0] < y[0] ? -1 : 1)));
  return cand.slice(0, 10).map((c) => c[0]);
};

const engines = {
  dl: dlEngine(false),
  dlfreq: dlEngine(true),
  keyhammer(dict) {
    const idx = KeyhammerIndex.build(dict, 2);
    idx.search("warmup", 1);
    return (q) => idx.search(q, 10).map((r) => r.term);
  },
  fuse(dict) {
    const f = new Fuse(dict, { threshold: 0.4, ignoreLocation: true, includeScore: false });
    return (q) => f.search(q, { limit: 10 }).map((r) => r.item);
  },
  ufuzzy(dict) {
    const uf = new uFuzzy({ intraMode: 1 });
    return (q) => {
      const [idxs, info, order] = uf.search(dict, q, 0);
      if (!idxs) return [];
      const list = info && order ? order.map((o) => dict[info.idx[o]]) : idxs.map((i) => dict[i]);
      return list.slice(0, 10);
    };
  },
  fuzzysort(dict) {
    const prepared = dict.map((w) => fuzzysort.prepare(w));
    return (q) => fuzzysort.go(q, prepared, { limit: 10, threshold: 0 }).map((r) => r.target);
  },
  minisearch(dict) {
    const ms = new MiniSearch({ fields: ["t"], storeFields: ["t"] });
    ms.addAll(dict.map((t, id) => ({ id, t })));
    return (q) => ms.search(q, { fuzzy: 2, prefix: false }).slice(0, 10).map((r) => r.t);
  },
};

function pct(a, p) { const b = [...a].sort((x, y) => x - y); return b[Math.min(b.length - 1, Math.floor(p * b.length))]; }

const rows = [];
for (const size of SIZES) {
  const dict = makeDict(size);
  console.log(`\n=== dictionary size: ${dict.length} ===`);
  for (const name of ENGINES) {
    let search;
    const t0 = process.hrtime.bigint();
    try { search = engines[name](dict); } catch (e) { console.log(`${name}: build failed: ${e.message}`); continue; }
    const buildMs = Number(process.hrtime.bigint() - t0) / 1e6;
    let r1 = 0, r5 = 0, r10 = 0, mrr = 0, r1w = 0, nw = 0;
    const lat = [];
    for (const t of tests) {
      const q0 = process.hrtime.bigint();
      let res; try { res = search(t.typo); } catch { res = []; }
      lat.push(Number(process.hrtime.bigint() - q0) / 1e6);
      const rank = res.indexOf(t.right) + 1;
      if (rank === 1) r1++; if (rank >= 1 && rank <= 5) r5++; if (rank >= 1 && rank <= 10) r10++;
      if (rank >= 1) mrr += 1 / rank;
      if (t.dist <= 2) { nw++; if (rank === 1) r1w++; }
    }
    const n = tests.length;
    const row = { size: dict.length, engine: name, buildMs: +buildMs.toFixed(0), "R@1": +(r1 / n).toFixed(3), "R@1(d<=2)": +(r1w / nw).toFixed(3), "R@5": +(r5 / n).toFixed(3), "R@10": +(r10 / n).toFixed(3), MRR: +(mrr / n).toFixed(3), medMs: +pct(lat, 0.5).toFixed(3), p95Ms: +pct(lat, 0.95).toFixed(3) };
    rows.push(row);
    console.log(JSON.stringify(row));
  }
}
fs.writeFileSync("data/results.json", JSON.stringify(rows, null, 1));
