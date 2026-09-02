# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- Initial project scaffold
- `.mutants.toml.example` with scoping guidance to keep `cargo mutants` runs tractable on real downstreams
- `test.yml` split: ubuntu-only `Lint` job covers fmt, clippy, doctest, and `cargo doc`; the cross-platform matrix (ubuntu/macos/windows) runs only `cargo build` + `cargo nextest run`
- Comments in `security.yml`, `.pre-commit-config.yaml`, and `deny.toml` documenting the `cargo audit --ignore` pattern keyed off `deny.toml`'s `[advisories] ignore` as source of truth
- CLAUDE.md / SETUP.md note on `prek install --overwrite` to avoid double-firing legacy pre-commit hooks
- MSRV-job comment on stubbing `include_bytes!` build artifacts via `touch` for downstreams that embed generated files
- Expanded `cargo vet import` list in SETUP.md to the seven well-known trusted orgs (bytecode-alliance, embark-studios, fermyon, google, isrg, mozilla, zcash), using the named-import form
- Per-PR workflow documentation in SETUP.md for handling Supply Chain Review failures on Dependabot PRs (never auto-regenerate exemptions in CI; certify vs. regenerate vs. reject)
- `cargo vet trust` section in SETUP.md covering transitive trust of publishers already certified by imported orgs (seanmonstar, BurntSushi, epage, kennykerr, Lokathor), including `--allow-multiple-publishers` guidance for flagship multi-maintainer packages
- `scripts/mutants.sh` wrapper for `cargo mutants --in-diff`, with project-specific pre-setup placeholder, defaulting to `HEAD~1..HEAD`
- Two-job mutation-testing setup in `security.yml`: a per-push/PR `mutation-testing-diff` job using `cargo mutants --in-diff` (typical runtime seconds-to-minutes), with `fetch-depth: 0` and PR/push base resolution; and a `mutation-testing` (full) job available via manual `workflow_dispatch` only — full-codebase runs don't scale to be scheduled
- `## Mutation testing` section in CLAUDE.md covering the local script + CI shape
- `/mutants.out/` and `/mutants.out.old/` added to `.gitignore`
- All Cargo cache steps in `test.yml`, `release.yml`, and `security.yml` switched from `actions/cache@v5` + manual key composition to `Swatinem/rust-cache@v2` + `shared-key`. Auto-derived key includes the rustc version (catches stale-binary panics across toolchain bumps), and pre-save cleanup keeps cache size in check. Release-job `shared-key` includes `matrix.target` so the two macOS cross-targets don't clobber each other.

[Unreleased]: https://github.com/OWNER/REPO/commits/main
