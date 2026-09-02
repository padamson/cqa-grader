# Project Name

<!-- Replace with a one-line description of what this project does -->

## Development

```bash
cargo build              # build
cargo nextest run        # run tests
cargo test --doc         # doc tests
cargo clippy             # lint
cargo fmt                # format
cargo audit              # security scan
cargo deny check         # license/dependency check
cargo vet                # supply chain review
```

## Pre-commit hooks

```bash
cargo install prek
prek install --overwrite   # --overwrite replaces any legacy pre-commit hook
```

Hooks mirror CI checks: fmt, clippy, check, nextest, doctest, audit, deny, vet.

## Mutation testing

```bash
./scripts/mutants.sh                 # diff HEAD~1..HEAD (default)
./scripts/mutants.sh main            # diff main..HEAD
./scripts/mutants.sh -- --jobs 4     # pass extra cargo-mutants args
```

`scripts/mutants.sh` wraps `cargo mutants --in-diff`, scoping mutation
testing to just the lines a commit touched. A full-codebase run grows
linearly with codebase size and routinely takes hours; `--in-diff`
keeps the loop fast enough to use while the test is still warm.

CI runs the per-diff variant on every push and PR (`mutation-testing-diff`
in `security.yml`). The full-codebase job (`mutation-testing`) is
manual-only via `workflow_dispatch` — use it for occasional audits or
big refactors, never on a schedule.

Scope the baseline with `.mutants.toml` (see `.mutants.toml.example`);
`--in-diff` narrows from there.

## Release process

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Commit: `git commit -m "Release vX.Y.Z"`
4. Tag: `git tag vX.Y.Z`
5. Push: `git push origin main --tags`

The tag triggers CI which builds, tests, creates a GitHub Release, and publishes to crates.io.

<!-- Add custom skills under .claude/skills/ as needed -->
