# Contributing

Thanks for your interest in Keyhammer.

## Developer Certificate of Origin

Every commit must be signed off (`git commit -s`), certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/):
you wrote the code or have the right to submit it under this project's license.
The DCO CI check intentionally skips pull requests opened by bot accounts
(for example Dependabot), which cannot sign off.

## License of contributions

Contributions are licensed under `AGPL-3.0-or-later`, the license of the project.
The maintainer may offer the maintainer's own contributions to the project under
additional (commercial) terms. Offering third-party contributions under such terms
would require a separate agreement, for example a contributor license agreement (CLA).

## Workflow

- Branch from `main`, open a pull request, keep commits in Conventional Commits style.
- These are the CI checks for the core crate and must pass:
  `cargo fmt -p keyhammer -- --check`,
  `cargo clippy -p keyhammer --all-targets -- -D warnings` and
  `cargo test -p keyhammer`. `legacy/` is a frozen reference and is excluded
  from the strict lints.
- The core crate forbids `unsafe` and must stay free of runtime dependencies.
