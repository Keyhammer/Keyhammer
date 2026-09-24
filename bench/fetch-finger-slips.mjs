// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Builds a corpus of REAL finger slips from the 136M Keystrokes dataset (Dhakal,
// Feit, Kristensson and Oulasvirta, CHI 2018): downloads the archive, verifies
// its SHA-256, extracts the words a participant typed differently from the
// sentence shown, classifies each pair by edit operation with the project's
// QWERTY adjacency (crates/keyhammer/src/cost.rs) and writes TSV files. Node
// only, no dependencies. Data is never committed.
//
//   node fetch-finger-slips.mjs [--data DIR] [--out DIR] [--sample N]
//
// --data    directory holding words-full.tsv (from prepare-m0-data.mjs); default: ./data
// --out     directory for the download and the outputs; default: the --data directory
// --sample  size of the fixed-seed sample used by the Rust harness; default: 3000
//
// Source: https://userinterfaces.aalto.fi/136Mkeystrokes/ (data/Keystrokes.zip,
// 1 572 785 433 bytes, 168 595 participants). Licence (readme.txt in the archive):
// free for non-commercial use in research or projects, with attribution to the
// authors. That is not an open-source licence: the archive and the derived files
// must not be redistributed or committed; this script only rebuilds them locally.
//
// What a "slip" is here: a word of the shown sentence that the participant left
// different in the text they submitted (an UNCORRECTED error; slips the typist
// noticed and backspaced away are not in this corpus). Only participants who
// used a QWERTY layout, a full or laptop keyboard, and whose native language is
// English are kept, to limit spelling errors and layouts other than QWERTY.
//
// Pairing: a sentence is used only when the shown and the typed text have the same
// number of whitespace-separated tokens; tokens are compared one by one, after
// lower-casing and stripping punctuation at both ends. A pair is kept when both
// words are ^[a-z]+$, they differ, the correct word (at least 3 letters) is in
// words-full.tsv, the typo (at least 2 letters) is not, and the optimal-string-
// alignment (OSA) distance is at most 2 (the engine's budget cannot reach more).
//
// Output (all `\t`-separated, header line included):
//   slips.tsv         typo, correct, category, first, count, users; one row per unique pair
//   slips-sample.tsv  the same columns, --sample unique pairs shuffled with a fixed seed
//   category: sub_adjacent | sub_other | transposition | extra_letter | extra_doubled |
//             missing_letter | missing_doubled | two_edits
//   first:    1 when the edit involves the first letter of the word, else 0
//   count:    occurrences over all participants; users: distinct participants
import fs from "fs";
import path from "path";
import crypto from "crypto";
import zlib from "zlib";
import { pipeline } from "stream/promises";
import { Readable } from "stream";
import { fileURLToPath } from "url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const arg = (name, def) => {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : def;
};
const DATA = path.resolve(arg("--data", path.join(HERE, "data")));
const OUT = path.resolve(arg("--out", DATA));
const SAMPLE = Number(arg("--sample", "3000"));
const SEED = 20260924;

const SOURCE = {
  url: "https://userinterfaces.aalto.fi/136Mkeystrokes/data/Keystrokes.zip",
  file: "Keystrokes.zip",
  size: 1572785433,
  sha256: "5fb217e0e1273017a6789c5c7ebcc00eed624f2f6426c284da2077a88e270420",
};

// ---------------------------------------------------------------------------
// Download (streamed, the archive is 1.5 GB) and verify.

async function sha256File(p) {
  const h = crypto.createHash("sha256");
  await pipeline(fs.createReadStream(p), h);
  return h.digest("hex");
}

async function download() {
  const p = path.join(OUT, SOURCE.file);
  if (!fs.existsSync(p)) {
    const res = await fetch(SOURCE.url, { headers: { "user-agent": "keyhammer-bench (https://github.com/Keyhammer/Keyhammer)" } });
    if (!res.ok) throw new Error(`download failed: ${SOURCE.url} (${res.status})`);
    await pipeline(Readable.fromWeb(res.body), fs.createWriteStream(p));
  }
  const size = fs.statSync(p).size;
  const got = await sha256File(p);
  if (size !== SOURCE.size || got !== SOURCE.sha256) throw new Error(`checksum mismatch for ${SOURCE.file}: ${size} bytes, sha256 ${got}`);
  console.log(`ok ${SOURCE.file} (${size} bytes)`);
  return p;
}

// ---------------------------------------------------------------------------
// Minimal ZIP reader (central directory, ZIP64, stored or deflate entries).

function readZip(p) {
  const fd = fs.openSync(p, "r");
  const size = fs.fstatSync(fd).size;
  const tailLen = Math.min(size, 70000);
  const tail = Buffer.alloc(tailLen);
  fs.readSync(fd, tail, 0, tailLen, size - tailLen);
  let e = tail.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (e < 0) throw new Error("zip: no end of central directory");
  let count = tail.readUInt16LE(e + 10);
  let cdSize = tail.readUInt32LE(e + 12);
  let cdOff = tail.readUInt32LE(e + 16);
  if (count === 0xffff || cdSize === 0xffffffff || cdOff === 0xffffffff) {
    const loc = tail.lastIndexOf(Buffer.from([0x50, 0x4b, 0x06, 0x07]), e);
    if (loc < 0) throw new Error("zip: no ZIP64 locator");
    const z64 = Number(tail.readBigUInt64LE(loc + 8));
    const b = Buffer.alloc(56);
    fs.readSync(fd, b, 0, 56, z64);
    if (b.readUInt32LE(0) !== 0x06064b50) throw new Error("zip: bad ZIP64 end record");
    count = Number(b.readBigUInt64LE(32));
    cdSize = Number(b.readBigUInt64LE(40));
    cdOff = Number(b.readBigUInt64LE(48));
  }
  const cd = Buffer.alloc(cdSize);
  fs.readSync(fd, cd, 0, cdSize, cdOff);
  const entries = [];
  let o = 0;
  for (let i = 0; i < count; i++) {
    if (cd.readUInt32LE(o) !== 0x02014b50) throw new Error("zip: bad central directory entry");
    const method = cd.readUInt16LE(o + 10);
    let csize = cd.readUInt32LE(o + 20);
    let usize = cd.readUInt32LE(o + 24);
    const nl = cd.readUInt16LE(o + 28);
    const el = cd.readUInt16LE(o + 30);
    const cl = cd.readUInt16LE(o + 32);
    let off = cd.readUInt32LE(o + 42);
    const name = cd.toString("utf8", o + 46, o + 46 + nl);
    if (usize === 0xffffffff || csize === 0xffffffff || off === 0xffffffff) {
      let x = o + 46 + nl;
      const end = x + el;
      while (x + 4 <= end) {
        const id = cd.readUInt16LE(x);
        const len = cd.readUInt16LE(x + 2);
        if (id === 1) {
          let q = x + 4;
          if (usize === 0xffffffff) { usize = Number(cd.readBigUInt64LE(q)); q += 8; }
          if (csize === 0xffffffff) { csize = Number(cd.readBigUInt64LE(q)); q += 8; }
          if (off === 0xffffffff) { off = Number(cd.readBigUInt64LE(q)); q += 8; }
        }
        x += 4 + len;
      }
    }
    entries.push({ name, method, csize, usize, off });
    o += 46 + nl + el + cl;
  }
  const read = (en) => {
    const h = Buffer.alloc(30);
    fs.readSync(fd, h, 0, 30, en.off);
    if (h.readUInt32LE(0) !== 0x04034b50) throw new Error(`zip: bad local header for ${en.name}`);
    const start = en.off + 30 + h.readUInt16LE(26) + h.readUInt16LE(28);
    const raw = Buffer.alloc(en.csize);
    fs.readSync(fd, raw, 0, en.csize, start);
    if (en.method === 0) return raw;
    if (en.method === 8) return zlib.inflateRawSync(raw);
    throw new Error(`zip: unsupported method ${en.method} for ${en.name}`);
  };
  return { entries, read };
}

// ---------------------------------------------------------------------------
// Classification. The adjacency is the one in cost.rs (`CostModel::qwerty`).

const ROWS = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
const ADJ = new Set();
{
  const link = (a, b) => { ADJ.add(a + b); ADJ.add(b + a); };
  ROWS.forEach((row, r) => {
    for (let c = 0; c < row.length; c++) {
      if (row[c + 1]) link(row[c], row[c + 1]);
      const below = ROWS[r + 1];
      if (below) {
        if (below[c]) link(row[c], below[c]);
        if (c >= 1 && below[c - 1]) link(row[c], below[c - 1]);
      }
    }
  });
}

// OSA distance, capped: returns min(distance, cap + 1).
function osa(a, b, cap) {
  if (Math.abs(a.length - b.length) > cap) return cap + 1;
  const m = a.length, n = b.length;
  const d = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = 0; i <= m; i++) d[i][0] = i;
  for (let j = 0; j <= n; j++) d[0][j] = j;
  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
    }
  }
  return Math.min(d[m][n], cap + 1);
}

// Returns [category, touchesFirstLetter] for a pair with OSA distance 1 or 2.
// `t` is the typed word, `c` the intended word.
function classify(t, c) {
  const dist = osa(t, c, 2);
  if (dist === 2) return ["two_edits", t[0] !== c[0] ? 1 : 0];
  if (t.length === c.length) {
    let i = 0;
    while (t[i] === c[i]) i++;
    if (t[i + 1] === c[i] && t[i] === c[i + 1] && t.slice(i + 2) === c.slice(i + 2)) return ["transposition", i === 0 ? 1 : 0];
    return [ADJ.has(t[i] + c[i]) ? "sub_adjacent" : "sub_other", i === 0 ? 1 : 0];
  }
  if (t.length === c.length + 1) {
    // an extra letter was typed: doubled when removing it lies inside a run of equal letters
    let doubled = false;
    let first = 0;
    let found = false;
    for (let i = 0; i < t.length; i++) {
      if (t.slice(0, i) + t.slice(i + 1) === c) {
        if (!found) first = i === 0 ? 1 : 0;
        found = true;
        if (t[i] === t[i - 1] || t[i] === t[i + 1]) doubled = true;
      }
    }
    return [doubled ? "extra_doubled" : "extra_letter", first];
  }
  // a letter of the word was skipped; doubled when it is one of a run of equal letters
  let doubled = false;
  let first = 0;
  let found = false;
  for (let i = 0; i < c.length; i++) {
    if (c.slice(0, i) + c.slice(i + 1) === t) {
      if (!found) first = i === 0 ? 1 : 0;
      found = true;
      if (c[i] === c[i - 1] || c[i] === c[i + 1]) doubled = true;
    }
  }
  return [doubled ? "missing_doubled" : "missing_letter", first];
}

// ---------------------------------------------------------------------------

const dictPath = path.join(DATA, "words-full.tsv");
if (!fs.existsSync(dictPath)) throw new Error(`${dictPath} missing: run fetch-data.mjs and prepare-m0-data.mjs first`);
const dict = new Set(fs.readFileSync(dictPath, "utf8").split("\n").map((l) => l.split("\t")[0]).filter(Boolean));
fs.mkdirSync(OUT, { recursive: true });

const zipPath = await download();
const zip = readZip(zipPath);
const byName = new Map(zip.entries.map((e) => [e.name, e]));
const meta = byName.get("Keystrokes/files/metadata_participants.txt");
if (!meta) throw new Error("metadata_participants.txt not found in the archive");
const mlines = zip.read(meta).toString("utf8").split(/\r?\n/);
const head = mlines[0].split("\t");
const col = (n) => head.indexOf(n);
const [cId, cLayout, cLang, cKb] = ["PARTICIPANT_ID", "LAYOUT", "NATIVE_LANGUAGE", "KEYBOARD_TYPE"].map(col);
if ([cId, cLayout, cLang, cKb].some((x) => x < 0)) throw new Error("unexpected metadata columns");
const keep = new Set();
let participants = 0;
for (const l of mlines.slice(1)) {
  if (!l) continue;
  participants++;
  const f = l.split("\t");
  if (f[cLayout] === "qwerty" && (f[cKb] === "full" || f[cKb] === "laptop") && f[cLang] === "en") keep.add(f[cId]);
}
console.log(`participants: ${participants} in metadata, ${keep.size} kept (qwerty, full or laptop keyboard, native English)`);

const funnel = { files: 0, sentences: 0, sentencesEqualTokens: 0, tokenPairsDiffering: 0, caseOnly: 0, notAz: 0, tooFar: 0, correctNotInDict: 0, typoInDict: 0, tooShort: 0, kept: 0 };
const pairs = new Map(); // "typo\tcorrect" -> { count, users, lastUser }
const strip = (s) => s.toLowerCase().replace(/^[^a-z0-9']+|[^a-z0-9']+$/g, "");
const t0 = Date.now();
for (const en of zip.entries) {
  const m = /^Keystrokes\/files\/(\d+)_keystrokes\.txt$/.exec(en.name);
  if (!m || !keep.has(m[1])) continue;
  funnel.files++;
  const text = zip.read(en).toString("utf8");
  let prev = "";
  let pos = text.indexOf("\n") + 1; // skip the header
  while (pos > 0 && pos < text.length) {
    let end = text.indexOf("\n", pos);
    if (end < 0) end = text.length;
    // columns: PARTICIPANT_ID, TEST_SECTION_ID, SENTENCE, USER_INPUT, ...; one row per keystroke
    const t1 = text.indexOf("\t", pos);
    const t2 = text.indexOf("\t", t1 + 1);
    const section = text.slice(t1 + 1, t2);
    if (section !== prev) {
      prev = section;
      const t3 = text.indexOf("\t", t2 + 1);
      const t4 = text.indexOf("\t", t3 + 1);
      const shown = text.slice(t2 + 1, t3);
      const typed = text.slice(t3 + 1, t4);
      funnel.sentences++;
      const a = shown.split(/\s+/).filter(Boolean);
      const b = typed.split(/\s+/).filter(Boolean);
      if (a.length === b.length) {
        funnel.sentencesEqualTokens++;
        for (let i = 0; i < a.length; i++) {
          if (a[i] === b[i]) continue;
          const c = strip(a[i]);
          const t = strip(b[i]);
          if (c === t) {
            // differs only in case or punctuation
            funnel.caseOnly++;
            continue;
          }
          funnel.tokenPairsDiffering++;
          if (!/^[a-z]+$/.test(c) || !/^[a-z]+$/.test(t)) { funnel.notAz++; continue; }
          if (osa(t, c, 2) > 2) { funnel.tooFar++; continue; }
          if (c.length < 3 || t.length < 2) { funnel.tooShort++; continue; }
          if (!dict.has(c)) { funnel.correctNotInDict++; continue; }
          if (dict.has(t)) { funnel.typoInDict++; continue; }
          funnel.kept++;
          const key = `${t}\t${c}`;
          let e = pairs.get(key);
          if (!e) pairs.set(key, (e = { count: 0, users: 0, lastUser: "" }));
          e.count++;
          if (e.lastUser !== m[1]) { e.users++; e.lastUser = m[1]; }
        }
      }
    }
    pos = end + 1;
  }
  if (funnel.files % 20000 === 0) console.log(`  ${funnel.files}/${keep.size} files, ${((Date.now() - t0) / 1000).toFixed(0)} s`);
}
console.log("funnel:", JSON.stringify(funnel));

const rows = [...pairs.keys()].sort().map((key) => {
  const [t, c] = key.split("\t");
  const [cat, first] = classify(t, c);
  const e = pairs.get(key);
  return [t, c, cat, first, e.count, e.users];
});
const HEADER = "typo\tcorrect\tcategory\tfirst\tcount\tusers\n";
const write = (name, list) => {
  const text = HEADER + list.map((r) => r.join("\t")).join("\n") + "\n";
  fs.writeFileSync(path.join(OUT, name), text);
  console.log(`${name}: ${list.length} pairs (sha256 ${crypto.createHash("sha256").update(text).digest("hex")})`);
};
write("slips.tsv", rows);

// fixed-seed shuffle (same generator as the other bench scripts)
let s = SEED;
const rnd = () => (s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32;
const shuffled = [...rows];
for (let i = shuffled.length - 1; i > 0; i--) {
  const j = Math.floor(rnd() * (i + 1));
  [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
}
const sample = shuffled.slice(0, SAMPLE);
write("slips-sample.tsv", sample);

const CATS = ["sub_adjacent", "sub_other", "transposition", "extra_letter", "extra_doubled", "missing_letter", "missing_doubled", "two_edits"];
console.log("\ncategory\tunique pairs\toccurrences\tsample pairs");
for (const cat of CATS) {
  const all = rows.filter((r) => r[2] === cat);
  console.log(`${cat}\t${all.length}\t${all.reduce((n, r) => n + r[4], 0)}\t${sample.filter((r) => r[2] === cat).length}`);
}
console.log(`total\t${rows.length}\t${rows.reduce((n, r) => n + r[4], 0)}\t${sample.length}`);
console.log(`first letter involved: ${rows.filter((r) => r[3] === 1).length} unique pairs`);
