# Contributing

Thanks for your interest in Keyhammer.

## Developer Certificate of Origin

Every commit must be signed off (`git commit -s`), certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/):
you wrote the code or have the right to submit it under this project's license.

## License of contributions

Contributions are licensed under `AGPL-3.0-or-later`, the license of the project.
The maintainer may offer the project under additional (commercial) terms.

## Workflow

- Branch from `main`, open a pull request, keep commits in Conventional Commits style.
- `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test` must pass.
- The core crate forbids `unsafe` and must stay free of runtime dependencies.
