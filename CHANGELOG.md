# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- Read a `cqa:Benchmark` from its published Turtle artifact (`Benchmark`).
  An answer kind reads from either IRI form under `cqa:AnswerKindEnum`:
  `AnswerKindEnum#lookup` (LinkML's gen-owl, and panschema after its 0.4.0
  release) or `AnswerKindEnum/lookup` (panschema's 0.4.0 release). An IRI
  outside that enum is an unknown answer kind.
- `Benchmark::from_yaml` reads a benchmark from its LinkML source, the
  YAML read against cqa's schema through `panschema-model`, and gives the
  same `Benchmark` as the Turtle for wine. A load that warns, leaves an
  anchor unexpanded, or skips a field or entry it could not place is
  refused rather than graded in part. `grade` and `baseline` read it with
  `--benchmark B.yaml --cqa-schema S`. Without `--cqa-schema` they read
  Turtle, and they refuse a `.yaml` or `.yml` benchmark given without it.
- Both readers take a benchmark written for cqa 0.2 as well as one written
  for 0.1. The target is the one entry of `target_datasets` (a
  `TargetDataset` with `dataset` and `version`) or the 0.1 pair
  `target_dataset` and `target_dataset_version`. A benchmark that lists
  more than one target dataset is refused, as is one that names its
  target both ways. The closed-world kind reads as `closed_world_negative`
  or as 0.1's `closed-world-negative`: from the YAML, whichever the schema
  it is read against permits, and from the Turtle, either. Reports spell
  it `closed_world_negative`, in text and in JSON.
- Both readers read an ordered question's order (`Question::order`): cqa
  0.2's `ordered_by`, and the records its expected values at that slot
  name, ranked by their values as decimal numbers compared exactly, with
  equal values tied. A key is refused when a value at the slot is not a
  number, when a record has two values there, when fewer than two records
  have one, or when a ranked record is not among the question's anchors.
  Expected values at other slots, and on a question with no order, are
  not read.
- A refused benchmark is `Error::Benchmark(Refusal)`, with a variant for
  each reason and, for a partial LinkML load, an `Unread` for each part
  the load read past.
- An ordered question's `order` check: the attempt's `sequence` gives
  every record the key ranks, ascending by its value at `ordered_by`, with
  tied records in any order and other records anywhere. An ordered
  question answered with no `sequence` ends `unanswered`, so the baseline
  still meets `--require no-wrong-answers`; one out of order, or missing a
  ranked record (an empty `sequence` misses them all), is `wrong`. Every
  record in a `sequence` must have been retrieved (`sequence-grounded`).
- The trace format (`Trace`, `Attempt`, `System`), with a generated JSON
  Schema in `schemas/trace.schema.json`.
- Trace format 0.2: an attempt's optional `sequence` lists the records
  the answer gives, in order, for a question the key orders. New traces,
  the baseline's included, are written as 0.2 with no sequence. A 0.1
  trace still reads, and one carrying a `sequence` is refused, as is a
  sequence that lists a record twice.
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
- `grade --baseline` grades the baseline in one run: it runs the baseline
  over `--graph` and grades its trace, so a benchmark's CI carries no
  trace file between two commands. `grade` takes `--trace` or
  `--baseline`, never both; `--save-trace` also writes the baseline's
  trace.
- `baseline --graph` can repeat, for a graph published as several files.
  Every file loads into one store (`Baseline::from_paths`), and a file that
  fails to read or parse is named in the error.
- A trace names the graph it was run over (`graph`: `dataset` and
  `dataset_version`), and grading refuses one whose graph is not the
  benchmark's `target_dataset` at its `target_dataset_version`, with exit
  code 2. A pre-release grades as its release: `0.2.1-dev` matches a key
  pinned to `0.2.1`, and the other way round. `baseline` reads the two from
  the graph's `dcat:Dataset` node (`Baseline::stated_graph`), as panschema
  writes it, never copying the benchmark's, so a benchmark's CI passes only
  `--graph`. Files that name more than one dataset are refused. For a graph
  with no node, `--dataset` and `--dataset-version` name it; beside a node
  they must agree with it, or the baseline exits 2.
- Each question ends `passed`, `unanswered`, `wrong`, or `unattempted`
  (`Outcome`), and the summary counts them; the outcome replaces the
  report's per-question `passed` flag and its `failed` count. Unanswered
  is a question whose every failed check failed by silence: a closed-world
  negative whose records were reached with no absence asserted, or an
  ordered question with no `sequence`. Any other failure is a wrong
  answer. `grade` holds the trace to a requirement (`Require`):
  `answered`, the default, needs every question passed; `no-wrong-answers`
  tolerates the unanswered and still fails a wrong answer or an
  unattempted question, so the baseline meets it over wine. The CLI takes
  it as `--require`, the report records it, and the text report names it
  in its header and the outcome in its verdict column.
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
- `--version` names the commit a binary was built from, as
  `0.1.0 (27f7f11)`. It prints the bare version when the build's checkout
  has the tag `v0.1.0` at HEAD, or when there is no checkout (a crates.io
  install). `cargo install --git <repo> --tag v0.1.0` still prints the
  commit, because cargo's checkout carries no tags.

[Unreleased]: https://github.com/padamson/cqa-grader/commits/main
