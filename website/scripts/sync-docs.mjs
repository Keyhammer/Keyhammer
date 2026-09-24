// Generates website/docs/ (gitignored) from:
//   - the hand-written pages in website/content/
//   - named files of the repository (the single source of truth)
// Only the files named below are copied. The docs/ folder of the repository is
// never copied as a whole, so nothing else in it can reach the site.
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';
import { fileURLToPath } from 'node:url';

const websiteDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const repoDir = join(websiteDir, '..');
const outDir = join(websiteDir, 'docs');
const contentDir = join(websiteDir, 'content');
const BLOB = 'https://github.com/Keyhammer/Keyhammer/blob/main/';
const EDIT = 'https://github.com/Keyhammer/Keyhammer/edit/main/';

// repo-relative source -> generated page
const synced = [
  { src: 'docs/benchmarks/m0.md', slug: 'results', title: 'Results (M0 gate)', position: 3 },
  { src: 'docs/benchmarks/competitors.md', slug: 'competitors', title: 'Benchmark: Rust libraries', position: 4 },
  { src: 'docs/benchmarks/competitors-js.md', slug: 'competitors-js', title: 'Benchmark: JavaScript libraries', position: 5 },
  { src: 'docs/papers.md', slug: 'prior-art', title: 'Prior art', position: 6 },
  { src: 'CONTRIBUTING.md', slug: 'contributing', title: 'Contributing', position: 7 },
  { src: 'CHANGELOG.md', slug: 'changelog', title: 'Changelog', position: 8 },
];
const siteRoutes = new Map(synced.map((s) => [s.src, `/docs/${s.slug}`]));

// Another synced page becomes a site route; any other relative target becomes
// an absolute GitHub URL. Absolute URLs, anchors and mailto: are left alone.
function rewriteTarget(target, srcPath) {
  if (/^([a-z][a-z0-9+.-]*:|#|\/)/i.test(target)) return target;
  const [pathPart, hash] = target.split('#');
  const repoPath = posix.normalize(posix.join(posix.dirname(srcPath), pathPart));
  const suffix = hash ? `#${hash}` : '';
  const route = siteRoutes.get(repoPath);
  return route ? route + suffix : BLOB + repoPath + suffix;
}

// Limits of this regex-based rewriting: it also touches text inside code
// fences, does not handle images nested in links, and does not handle
// parentheses inside link targets. Relative links in the synced sources, if any, are rewritten.
function rewriteLinks(text, srcPath) {
  // Inline links and images: [text](target "title") and ![alt](target).
  const inline = text.replace(
    /(!?\[[^\]]*\]\()([^)\s]+)((?:\s+"[^"]*")?\))/g,
    (_m, open, target, close) => open + rewriteTarget(target, srcPath) + close,
  );
  // Reference definitions: [label]: target
  return inline.replace(
    /^(\s*\[[^\]]+\]:\s*)(\S+)/gm,
    (_m, open, target) => open + rewriteTarget(target, srcPath),
  );
}

function frontMatter(fields) {
  const lines = Object.entries(fields).map(
    ([k, v]) => `${k}: ${typeof v === 'string' ? JSON.stringify(v) : v}`,
  );
  return `---\n${lines.join('\n')}\n---\n\n`;
}

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

for (const name of readdirSync(contentDir).filter((f) => f.endsWith('.md'))) {
  writeFileSync(join(outDir, name), readFileSync(join(contentDir, name), 'utf8'));
}

for (const { src, slug, title, position } of synced) {
  const body = readFileSync(join(repoDir, ...src.split('/')), 'utf8').replace(/^﻿/, '');
  if (body.startsWith('---')) {
    throw new Error(`sync-docs: ${src} already starts with front matter; adapt the script before syncing it`);
  }
  const head = frontMatter({
    title,
    sidebar_position: position,
    slug: `/${slug}`,
    custom_edit_url: EDIT + src,
  });
  writeFileSync(join(outDir, `${slug}.md`), head + rewriteLinks(body, src));
}

console.log(`sync-docs: generated ${synced.length} pages from the repository into website/docs`);
