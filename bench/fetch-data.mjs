// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
// Downloads benchmark datasets and verifies their SHA-256. Datasets are never committed.
import fs from "fs";
import crypto from "crypto";

const FILES = [
  ["https://www.dcs.bbk.ac.uk/~ROGER/missp.dat", "missp.dat", "ed7d8c91961a1201632351943571e77af2011cdf45d24304c4d7cad6cf77ea15"],
  ["https://norvig.com/big.txt", "big.txt", "fa066c7d40f0f201ac4144e652aa62430e58a6b3805ec70650f678da5804e87b"],
  ["https://norvig.com/spell-testset1.txt", "spell-testset1.txt", "a5a152f32d66001fd9762e6745357a744e415eae064bd8cb2f4a42471f850724"],
  ["https://norvig.com/spell-testset2.txt", "spell-testset2.txt", "72f64bbfa60ef1a41eabd408fb3f6daff6c54f1944cb919efbf1e1449296b0a3"],
];

fs.mkdirSync("data", { recursive: true });
for (const [url, name, sha] of FILES) {
  const path = `data/${name}`;
  if (!fs.existsSync(path)) {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`download failed: ${url} (${res.status})`);
    fs.writeFileSync(path, Buffer.from(await res.arrayBuffer()));
  }
  const got = crypto.createHash("sha256").update(fs.readFileSync(path)).digest("hex");
  if (got !== sha) throw new Error(`checksum mismatch for ${name}: ${got}`);
  console.log(`ok ${name}`);
}
