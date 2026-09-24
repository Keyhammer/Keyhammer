## What and why

<!-- What does this change, and why? Link the issue if there is one. -->

## Checklist

- [ ] The title follows Conventional Commits (`feat(core): ...`); CI checks it.
- [ ] Every commit is signed off (`git commit -s`); CI checks it (DCO).
- [ ] `cargo fmt -p keyhammer -- --check`, `cargo clippy -p keyhammer --all-targets -- -D warnings` and `cargo test -p keyhammer` pass.
- [ ] Docs and `CHANGELOG.md` are updated if behaviour or public API changed.
- [ ] Breaking change? Say so above and use `!` in the title (`feat(core)!: ...`).
