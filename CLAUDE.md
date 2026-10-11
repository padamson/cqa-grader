# cqa-grader

Grades a system's retrieval trace against a cqa benchmark: reads the
benchmark's published Turtle, reads a JSON trace of what the system
retrieved and cited per question, dispatches on `answer_kind`, and
reports pass/fail per check. Ships a naive baseline retriever so a
benchmark's CI has something to grade.

## Layout

- `src/benchmark.rs` reads a `cqa:Benchmark` from Turtle (oxigraph, SPARQL).
- `src/trace.rs` is the trace format, the contract with graded systems.
  `schemas/trace.schema.json` is generated from it and a test keeps them equal.
- `src/grade.rs` scores a trace: three base checks plus one per answer kind.
- `src/baseline.rs` is the naive retriever: label linking, one-hop SPARQL
  lookup, cites everything, asserts absence when the named records fall
  into unjoined groups. It must never read the key; a test checks that.
- `src/main.rs` is the CLI: `grade`, `baseline`, `trace-schema`.
- `crates/site/` is the landing site (Leptos CSR, Trunk, Tailwind v4),
  outside the workspace (its own `[workspace]`; run cargo with
  `--manifest-path crates/site/Cargo.toml`, clippy with
  `--target wasm32-unknown-unknown`). `crates/site/preview.sh` serves it
  with hot reload. `public/roadmap.json` is the roadmap. Each Now and Next item
  lists acceptance criteria, each naming its proving test as
  `<path>::<function>`; a test fails if a done item names a test that
  does not exist. Move an item in the commit that does its work.
- `brand/` is the mark, wordmark, palette and fonts; `scripts/gen-brand.py`
  regenerates them.

## Test graphs are generated, not vendored

`panschema.toml` consumes wine (ontology-authoring-template) and cqa
(cqa-schema) as `path:` siblings and generates `tests/fixtures/*.ttl`
(gitignored). Both outputs are byte-identical to wine's own published
`site/artifacts/`. CI clones the siblings and installs panschema from its
main SHA. The sibling paths become `github:` pins when wine releases a
version containing the benchmark and cqa tags v0.1.0.

```bash
panschema generate --strict   # regenerate tests/fixtures/
```

## Development

```bash
cargo build              # build
cargo nextest run        # run tests (needs tests/fixtures/, see above)
cargo test --doc         # doc tests
cargo clippy             # lint
cargo fmt                # format
cargo audit              # security scan
cargo deny check         # license/dependency check
cargo vet                # supply chain review
cargo run -- trace-schema > schemas/trace.schema.json   # after changing src/trace.rs
```

## Sandbox

Claude Code's sandbox (sandbox-runtime, Seatbelt on macOS) is on
globally. The sibling checkouts are read, never written: `panschema
generate` reads `../ontology-authoring-template` and `../cqa-schema`,
and a panschema debug build is made in panschema's own session.
Anything that needs the Keychain or writes git hooks (`prek install`,
`gh auth`) runs outside the sandbox, by hand.

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
testing to just the lines a commit touched. CI runs the per-diff variant
on every push and PR; the full-codebase job is manual-only.

## Vocabulary

panschema *verifies* artifacts against their specs. This tool *grades* a
system's output against the key. *Validate* is reserved for the strict
V&V sense and is not used here.

## Release process

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Commit: `git commit -m "Release vX.Y.Z"`
4. Tag: `git tag vX.Y.Z`
5. Push: `git push origin main --tags`

The tag triggers CI which builds, tests, creates a GitHub Release, and publishes to crates.io.
