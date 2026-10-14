# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- Read a `cqa:Benchmark` from its published Turtle artifact (`Benchmark`).
- The trace format (`Trace`, `Attempt`, `System`), with a generated JSON
  Schema in `schemas/trace.schema.json`.
- `grade`: four base checks per question (`attempted`, `anchors-reached`,
  `citations-grounded`, `no-false-absence`) plus one per answer kind, with
  recall and precision reported per attempt. Text and JSON reports. An
  attempt that says "none on record" for a question whose answer is on
  record fails.
- The naive baseline retriever (`Baseline`): label linking, one-hop SPARQL
  lookup, cites everything. It never asserts absence, since which records
  a negative is about is in the key, not the question; it passes every
  question in wine's benchmark but the closed-world negative.
- CLI: `cqa-grader grade`, `cqa-grader baseline`, `cqa-grader trace-schema`.
- `baseline --graph` can repeat, for a graph published as several files.
  Every file loads into one store (`Baseline::from_paths`), and a file that
  fails to read or parse is named in the error.
- A trace names the graph it was run over (`graph`: `dataset` and
  `dataset_version`), and grading refuses one whose graph is not the
  benchmark's `target_dataset` at its `target_dataset_version`, with exit
  code 2. A pre-release grades as its release: `0.2.1-dev` matches a key
  pinned to `0.2.1`, and the other way round. `baseline` takes the two as
  `--dataset` and `--dataset-version` and records them as given, never
  copying the benchmark's.
- Each question ends `passed`, `unanswered`, `wrong`, or `unattempted`
  (`Outcome`), and the summary counts them; the outcome replaces the
  report's per-question `passed` flag and its `failed` count. Unanswered is a closed-world
  negative whose records were reached with no absence asserted and nothing
  else failed; any other failure is a wrong answer. `grade` holds the trace
  to a requirement (`Require`): `answered`, the default, needs every
  question passed; `no-wrong-answers` tolerates the unanswered and still
  fails a wrong answer or an unattempted question, so the baseline meets it
  over wine. The CLI takes it as `--require`, the report records it, and
  the text report names it in its header and the outcome in its verdict
  column.
- Unusable input is refused with exit code 2 rather than graded: a
  benchmark question missing its id, text or ground truth, two questions
  sharing an id, a closed-world negative with no unconnected anchors, a
  trace field the format does not define, or two attempts at one
  question.
- The brand in `brand/`: the mark, the CQAgrader wordmark, the board and
  paper palette, and their fonts, with `scripts/gen-brand.py` to
  regenerate them. The lockup heads the README.
- The roadmap as data in `crates/site/public/roadmap.json`: Now, Next and Later,
  each Now and Next item with acceptance criteria that name the test
  proving them, held to its types and to existing tests by the site
  crate's own tests and CI job.
- The landing page in `crates/site` (Leptos, Trunk, Tailwind v4, in the
  brand's board palette), with each roadmap item's criteria in a popover
  and `crates/site/preview.sh` to serve it locally.
- An asbuilt architecture model of the repo in `docs/architecture/`,
  checked in the pre-commit hook and CI, and rendered as the landing
  page's Architecture section.
- Test graphs generated from sibling checkouts through `panschema.toml`
  rather than vendored; CI clones the siblings and installs panschema
  from its main SHA.

[Unreleased]: https://github.com/padamson/cqa-grader/commits/main
