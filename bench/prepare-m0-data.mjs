// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
// Builds the dictionaries and typo test pairs used by the Rust M0 harness.
import fs from "fs";
import wordList from "word-list";

const NQ = 300;
let s = 12345;
const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32);
const shuffle = (a) => { for (let i = a.length - 1; i > 0; i--) { const j = Math.floor(rnd() * (i + 1)); [a[i], a[j]] = [a[j], a[i]]; } return a; };

const words = fs.readFileSync(wordList, "utf8").split("\n").filter((w) => /^[a-z]+$/.test(w));
const wset = new Set(words);

const freq = new Map();
for (const w of fs.readFileSync("data/big.txt", "utf8").toLowerCase().match(/[a-z]+/g)) freq.set(w, (freq.get(w) || 0) + 1);
const fmax = Math.max(...freq.values());
const weight = (w) => Math.round((65535 * Math.log(1 + (freq.get(w) || 0))) / Math.log(1 + fmax));

const pairs = [];
let cur = null;
for (const line of fs.readFileSync("data/missp.dat", "utf8").split(/\r?\n/)) {
  if (line.startsWith("$")) cur = line.slice(1).toLowerCase();
  else if (cur && /^[a-z]{3,}$/i.test(line)) pairs.push([line.toLowerCase(), cur]);
}
const good = shuffle(pairs.filter(([t, c]) => /^[a-z]{3,}$/.test(c) && wset.has(c) && !wset.has(t) && t !== c));
const tests = good.slice(0, NQ);
const targets = [...new Set(tests.map(([, c]) => c))];
const targetSet = new Set(targets);
const others = shuffle(words.filter((w) => !targetSet.has(w)));

fs.writeFileSync("data/tests.tsv", tests.map(([t, c]) => `${t}\t${c}`).join("\n") + "\n");
for (const size of ["10000", "100000", "full"]) {
  const n = size === "full" ? words.length : +size;
  const dict = shuffle([...targets, ...others.slice(0, Math.max(0, n - targets.length))]);
  fs.writeFileSync(`data/words-${size}.tsv`, dict.map((w) => `${w}\t${weight(w)}`).join("\n") + "\n");
  console.log(`words-${size}.tsv: ${dict.length} words`);
}
console.log(`tests.tsv: ${tests.length} pairs`);
