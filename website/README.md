# Keyhammer website

Docusaurus 3 site, published to https://keyhammer.github.io/Keyhammer/ by
`.github/workflows/pages.yml`.

```sh
npm ci
npm start        # dev server
npm run build    # production build into build/
```

- `content/` holds the hand-written pages. `npm run sync` (run automatically
  by `prestart` and `prebuild`) generates the gitignored `docs/` folder from
  `content/` plus four named repository files: `docs/benchmarks/m0.md`,
  `docs/papers.md`, `CONTRIBUTING.md` and `CHANGELOG.md`. Edit those files, not
  the generated copies. Relative links in them are rewritten to absolute GitHub
  URLs.
- The API reference (rustdoc) is generated in CI with
  `cargo doc -p keyhammer --no-deps` and copied into `static/api/`. It is absent
  from a plain local build; the links to it use the `pathname://` protocol.
- Search is not set up yet. Options for later: Algolia DocSearch (free for open
  source) or a local search plugin.
- Only English is configured. A pt-BR locale can be added later through the
  Docusaurus i18n settings.
