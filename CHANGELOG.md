# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- Read a `cqa:Benchmark` from its published Turtle artifact (`Benchmark`).
- The trace format (`Trace`, `Attempt`, `System`), with a generated JSON
  Schema in `schemas/trace.schema.json`.
- `grade`: three base checks per question (`attempted`, `anchors-reached`,
  `citations-grounded`) plus one per answer kind, with recall and
  precision reported per attempt. Text and JSON reports.
- The naive baseline retriever (`Baseline`): label linking, one-hop SPARQL
  lookup, cites everything, asserts absence over unjoined groups. Passes
  wine's six-question benchmark.
- CLI: `cqa-grader grade`, `cqa-grader baseline`, `cqa-grader trace-schema`.
- Unusable input is refused with exit code 2 rather than graded: a
  benchmark question missing its id, text or ground truth, two questions
  sharing an id, a closed-world negative with no unconnected anchors, a
  trace field the format does not define, or two attempts at one
  question.
- Test graphs generated from sibling checkouts through `panschema.toml`
  rather than vendored; CI clones the siblings and installs panschema
  from its main SHA.

[Unreleased]: https://github.com/padamson/cqa-grader/commits/main
