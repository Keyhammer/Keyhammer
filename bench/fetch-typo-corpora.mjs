// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Downloads two extra typo corpora at pinned revisions, verifies their SHA-256
// and writes 1000-pair samples used by the competitive benchmark
// (bench/competitors). Node only, no dependencies. Datasets are never committed.
//
//   node fetch-typo-corpora.mjs [--data DIR] [--out DIR]
//
// --data  directory holding words-full.tsv (from prepare-m0-data.mjs); default: ./data
// --out   directory for the downloads and gtc.tsv / wiki.tsv; default: the --data directory
//
// Sources:
// - GitHub Typo Corpus (Hagiwara and Mita, 2020), Hugging Face mirror
//   chirunder/github_typo_corrections at commit 57d581ff (one Parquet file,
//   353055 text/correction edits). The mirror states no licence; the texts come
//   from public GitHub repositories and each follows its repository's licence.
// - Wikipedia "Lists of common misspellings/For machines", revision 1199637275
//   (2024-01-27), CC BY-SA 4.0.
//
// Output: gtc.tsv and wiki.tsv, `typo TAB correct`, 1000 unique pairs each,
// both words ^[a-z]+$, the correct word (at least 3 letters) in words-full.tsv,
// the typo (at least 2 letters) not in it; shuffled with a fixed seed.
import fs from "fs";
import path from "path";
import crypto from "crypto";
import { fileURLToPath } from "url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const arg = (name, def) => {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : def;
};
const DATA = path.resolve(arg("--data", path.join(HERE, "data")));
const OUT = path.resolve(arg("--out", DATA));
const SAMPLE = 1000;
const SEED = 20260924;

const SOURCES = {
  gtc: {
    url: "https://huggingface.co/datasets/chirunder/github_typo_corrections/resolve/57d581ff2bb13cec9c7d56d71f27d40319446c59/data/train-00000-of-00001-aeb59e07ad0a4436.parquet",
    file: "github_typo_corrections-57d581ff.parquet",
    sha256: "4ff2fccd363f519a11447e079513b4c51b6275a6a17b6b483f81e078c6f0a9fe",
  },
  wiki: {
    url: "https://en.wikipedia.org/w/index.php?title=Wikipedia:Lists_of_common_misspellings/For_machines&oldid=1199637275&action=raw",
    file: "wikipedia-misspellings-1199637275.txt",
    sha256: "3b6a9290e5aaad968da7ec769a5174adb45247688ef7ae097fd2dd80237a3050",
  },
};

async function download({ url, file, sha256 }) {
  const p = path.join(OUT, file);
  if (!fs.existsSync(p)) {
    const res = await fetch(url, { headers: { "user-agent": "keyhammer-bench (https://github.com/Keyhammer/Keyhammer)" } });
    if (!res.ok) throw new Error(`download failed: ${url} (${res.status})`);
    fs.writeFileSync(p, Buffer.from(await res.arrayBuffer()));
  }
  const buf = fs.readFileSync(p);
  const got = crypto.createHash("sha256").update(buf).digest("hex");
  if (got !== sha256) throw new Error(`checksum mismatch for ${file}: ${got}`);
  console.log(`ok ${file} (${buf.length} bytes)`);
  return buf;
}

// ---------------------------------------------------------------------------
// Minimal Parquet reader: enough for flat BYTE_ARRAY columns written by
// parquet-cpp (Thrift compact footer, v1 data pages, SNAPPY or no compression,
// PLAIN and dictionary encodings, optional or required columns).

class Thrift {
  constructor(buf, pos = 0) {
    this.buf = buf;
    this.pos = pos;
  }
  byte() {
    return this.buf[this.pos++];
  }
  varint() {
    let r = 0n;
    let shift = 0n;
    for (;;) {
      const b = this.byte();
      r |= BigInt(b & 0x7f) << shift;
      if (!(b & 0x80)) return r;
      shift += 7n;
    }
  }
  zigzag() {
    const v = this.varint();
    return Number((v >> 1n) ^ -(v & 1n));
  }
  value(type) {
    switch (type) {
      case 1:
        return true;
      case 2:
        return false;
      case 3:
        return (this.byte() << 24) >> 24;
      case 4:
      case 5:
      case 6:
        return this.zigzag();
      case 7:
        this.pos += 8;
        return null;
      case 8: {
        const n = Number(this.varint());
        const b = this.buf.subarray(this.pos, this.pos + n);
        this.pos += n;
        return b;
      }
      case 9:
      case 10: {
        const h = this.byte();
        let n = h >> 4;
        if (n === 15) n = Number(this.varint());
        const t = h & 15;
        const out = [];
        for (let i = 0; i < n; i++) out.push(t === 1 || t === 2 ? this.byte() === 1 : this.value(t));
        return out;
      }
      case 11: {
        const n = Number(this.varint());
        if (n === 0) return new Map();
        const kv = this.byte();
        const m = new Map();
        for (let i = 0; i < n; i++) m.set(this.value(kv >> 4), this.value(kv & 15));
        return m;
      }
      case 12:
        return this.struct();
      default:
        throw new Error(`thrift: unknown type ${type}`);
    }
  }
  struct() {
    const out = {};
    let id = 0;
    for (;;) {
      const h = this.byte();
      if (h === 0) return out;
      const delta = h >> 4;
      id = delta ? id + delta : this.zigzag();
      out[id] = this.value(h & 15);
    }
  }
}

function snappy(src) {
  let pos = 0;
  let len = 0;
  for (let shift = 0; ; shift += 7) {
    const b = src[pos++];
    len |= (b & 0x7f) << shift;
    if (!(b & 0x80)) break;
  }
  const out = Buffer.alloc(len);
  let o = 0;
  while (pos < src.length) {
    const tag = src[pos++];
    const kind = tag & 3;
    if (kind === 0) {
      let n = tag >> 2;
      if (n >= 60) {
        const bytes = n - 59;
        n = 0;
        for (let i = 0; i < bytes; i++) n |= src[pos++] << (8 * i);
      }
      n += 1;
      src.copy(out, o, pos, pos + n);
      pos += n;
      o += n;
      continue;
    }
    let n;
    let off;
    if (kind === 1) {
      n = ((tag >> 2) & 7) + 4;
      off = ((tag >> 5) << 8) | src[pos++];
    } else if (kind === 2) {
      n = (tag >> 2) + 1;
      off = src[pos] | (src[pos + 1] << 8);
      pos += 2;
    } else {
      n = (tag >> 2) + 1;
      off = src.readUInt32LE(pos);
      pos += 4;
    }
    for (let i = 0; i < n; i++, o++) out[o] = out[o - off];
  }
  if (o !== len) throw new Error("snappy: bad length");
  return out;
}

// RLE / bit-packed hybrid decoder; returns `count` values.
function hybrid(buf, pos, end, width, count) {
  const out = [];
  const bytes = Math.ceil(width / 8);
  while (out.length < count && pos < end) {
    let h = 0;
    for (let shift = 0; ; shift += 7) {
      const b = buf[pos++];
      h += (b & 0x7f) * 2 ** shift;
      if (!(b & 0x80)) break;
    }
    if (h & 1) {
      const n = (h >>> 1) * 8;
      let bit = 0;
      for (let i = 0; i < n; i++) {
        let v = 0;
        for (let k = 0; k < width; k++, bit++) {
          if (buf[pos + (bit >> 3)] & (1 << (bit & 7))) v |= 1 << k;
        }
        out.push(v);
      }
      pos += (n * width) / 8;
    } else {
      const n = h >>> 1;
      let v = 0;
      for (let i = 0; i < bytes; i++) v |= buf[pos++] << (8 * i);
      for (let i = 0; i < n; i++) out.push(v);
    }
  }
  return out.slice(0, count);
}

function plainByteArrays(buf, pos, n) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const len = buf.readUInt32LE(pos);
    pos += 4;
    out.push(buf.toString("utf8", pos, pos + len));
    pos += len;
  }
  return out;
}

// Returns { name: string[] (null for missing) } for every flat column.
function readParquet(file) {
  if (file.toString("latin1", 0, 4) !== "PAR1" || file.toString("latin1", file.length - 4) !== "PAR1") {
    throw new Error("not a Parquet file");
  }
  const metaLen = file.readUInt32LE(file.length - 8);
  const meta = new Thrift(file, file.length - 8 - metaLen).struct();
  const schema = meta[2].slice(1); // skip the root
  const optional = new Map(schema.map((s) => [s[4].toString(), s[3] === 1]));
  const cols = new Map([...optional.keys()].map((k) => [k, []]));
  for (const rg of meta[4]) {
    for (const chunk of rg[1]) {
      const cm = chunk[3];
      const name = cm[3].map((b) => b.toString()).join(".");
      const codec = cm[4];
      const total = cm[5];
      const out = cols.get(name);
      let pos = cm[11] !== undefined && cm[11] > 0 ? cm[11] : cm[9];
      let dict = null;
      let seen = 0;
      while (seen < total) {
        const t = new Thrift(file, pos);
        const ph = t.struct();
        const raw = file.subarray(t.pos, t.pos + ph[3]);
        pos = t.pos + ph[3];
        const page = codec === 0 ? raw : codec === 1 ? snappy(raw) : null;
        if (!page) throw new Error(`unsupported codec ${codec}`);
        if (ph[1] === 2) {
          dict = plainByteArrays(page, 0, ph[7][1]);
          continue;
        }
        if (ph[1] !== 0) throw new Error(`unsupported page type ${ph[1]}`);
        const n = ph[5][1];
        const enc = ph[5][2];
        let p = 0;
        let defs = null;
        if (optional.get(name)) {
          const len = page.readUInt32LE(0);
          defs = hybrid(page, 4, 4 + len, 1, n);
          p = 4 + len;
        }
        const present = defs ? defs.filter((d) => d === 1).length : n;
        let vals;
        if (enc === 0) vals = plainByteArrays(page, p, present);
        else if (enc === 2 || enc === 8) {
          const width = page[p];
          vals = hybrid(page, p + 1, page.length, width, present).map((i) => dict[i]);
        } else throw new Error(`unsupported encoding ${enc}`);
        let k = 0;
        for (let i = 0; i < n; i++) out.push(!defs || defs[i] ? vals[k++] : null);
        seen += n;
      }
    }
  }
  return Object.fromEntries(cols);
}

// ---------------------------------------------------------------------------

let s = SEED;
const rnd = () => (s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32;
const shuffle = (a) => {
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rnd() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
};

const dictPath = path.join(DATA, "words-full.tsv");
if (!fs.existsSync(dictPath)) throw new Error(`${dictPath} missing: run fetch-data.mjs and prepare-m0-data.mjs first`);
const dict = new Set(fs.readFileSync(dictPath, "utf8").split("\n").map((l) => l.split("\t")[0]).filter(Boolean));
const AZ = /^[a-z]+$/;
const ok = (t, c) => AZ.test(t) && AZ.test(c) && t !== c && dict.has(c) && !dict.has(t) && c.length >= 3 && t.length >= 2;

// Expected SHA-256 of the samples (as recorded in docs/benchmarks/competitors.md).
// They also depend on words-full.tsv; a mismatch means the inputs differ.
const EXPECTED = {
  gtc: "091dfd81ef0f13b901bfec40dc1617b74867832b3b3efec35a3cb831378c04e2",
  wiki: "d3dc05e3a52b1a2c5c06bac4bbfcaafdbfffbe3a501bcd95d457819008b9c3f9",
};

function sample(pairs, name) {
  const keys = [...new Set(pairs.map(([t, c]) => `${t}\t${c}`))].sort();
  s = SEED;
  const picked = shuffle(keys).slice(0, SAMPLE);
  const text = picked.join("\n") + "\n";
  fs.writeFileSync(path.join(OUT, `${name}.tsv`), text);
  const sha = crypto.createHash("sha256").update(text).digest("hex");
  console.log(`${name}.tsv: ${picked.length} pairs from ${keys.length} unique usable pairs (sha256 ${sha})`);
  if (sha !== EXPECTED[name]) {
    throw new Error(`${name}.tsv does not match the published sample: sha256 ${sha}, expected ${EXPECTED[name]} (is words-full.tsv from prepare-m0-data.mjs?)`);
  }
}

fs.mkdirSync(OUT, { recursive: true });

// GitHub Typo Corpus: keep edits where both sides tokenise to the same number
// of tokens and exactly one token differs.
{
  const cols = readParquet(await download(SOURCES.gtc));
  const text = cols.text;
  const corr = cols.correction;
  console.log(`gtc: ${text.length} edits`);
  const TOK = /[A-Za-z]+|[^A-Za-z\s]+/g;
  const pairs = [];
  for (let i = 0; i < text.length; i++) {
    if (text[i] == null || corr[i] == null) continue;
    const a = text[i].match(TOK) || [];
    const b = corr[i].match(TOK) || [];
    if (a.length !== b.length) continue;
    let diff = null;
    let count = 0;
    for (let j = 0; j < a.length && count < 2; j++) {
      if (a[j] !== b[j]) {
        count++;
        diff = [a[j], b[j]];
      }
    }
    if (count === 1 && ok(diff[0], diff[1])) pairs.push(diff);
  }
  sample(pairs, "gtc");
}

// Wikipedia: `typo->correct` lines with a single correction.
{
  const pairs = [];
  for (const line of (await download(SOURCES.wiki)).toString("utf8").split("\n")) {
    const l = line.trim();
    const i = l.indexOf("->");
    if (i < 0) continue;
    const t = l.slice(0, i).trim();
    const c = l.slice(i + 2).trim();
    if (c.includes(",")) continue;
    if (ok(t, c)) pairs.push([t, c]);
  }
  sample(pairs, "wiki");
}
