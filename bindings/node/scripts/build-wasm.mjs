// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//
// Builds the WebAssembly module with the flags the project uses for public
// builds (size profile, source paths remapped so that no build-machine path is
// embedded) and copies it next to index.js as keyhammer.wasm.
//   node scripts/build-wasm.mjs
// Needs the wasm32-unknown-unknown target: rustup target add wasm32-unknown-unknown
import { spawnSync } from 'node:child_process';
import { copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkg = join(here, '..');
const repo = join(pkg, '..', '..');
const home = process.env.HOME ?? process.env.USERPROFILE ?? '';
const cargoHome = process.env.CARGO_HOME ?? join(home, '.cargo');
const rustupHome = process.env.RUSTUP_HOME ?? join(home, '.rustup');
const remap = [
  [repo, '/src'],
  [cargoHome, '/cargo'],
  [rustupHome, '/rustup'],
]
  .map(([from, to]) => `--remap-path-prefix=${from}=${to}`)
  .join(' ');

const r = spawnSync(
  'cargo',
  ['build', '-p', 'keyhammer-wasm', '--target', 'wasm32-unknown-unknown', '--profile', 'wasm'],
  { cwd: repo, stdio: 'inherit', env: { ...process.env, RUSTFLAGS: remap } },
);
if (r.status !== 0) process.exit(r.status ?? 1);
copyFileSync(
  join(repo, 'target', 'wasm32-unknown-unknown', 'wasm', 'keyhammer_wasm.wasm'),
  join(pkg, 'keyhammer.wasm'),
);
console.log('wrote keyhammer.wasm');
