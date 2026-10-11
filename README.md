<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/lockup-dark.svg">
    <img src="brand/lockup-light.svg" width="360" alt="cqa-grader">
  </picture>
</h1>

<p align="center">
  <a href="https://github.com/padamson/cqa-grader/actions?query=branch%3Amain"><img src="https://img.shields.io/github/check-runs/padamson/cqa-grader/main?label=CI&amp;logo=github" alt="CI"></a>
</p>

Grades what a graph retriever found.

An ontology's competency questions say what its knowledge graph must be
able to answer. A [cqa](https://github.com/padamson/cqa-schema)
benchmark gives each question an answer key: the records a correct
answer must reach (`expected_anchors`), the ones it must cite
(`expected_citations`), the kind of answer it is (`answer_kind`), and
the answer in prose (`ground_truth`). The graph's own repository
publishes the benchmark as a Turtle file. cqa-grader reads that file and
a *trace* of what some system retrieved and cited for each question, and
checks one against the other. Only the trace crosses over: nothing about
how the system produced it reaches the grader.

It has three uses. Someone developing an ontology the way Noy and
McGuinness's *Ontology Development 101* describes writes an example
instance graph and a benchmark for its competency questions, and the
baseline shows whether the example answers them all (the
[ontology-authoring-template](https://github.com/padamson/ontology-authoring-template)'s
wine ontology carries both). A graph's repository runs the baseline in
CI to show that every question can be answered from the published
graph. Someone building a retriever has their system write a trace and
grades it against the benchmark.

Two tiers were always the plan. The deterministic tier is here: anchors
reached, citations grounded and cited, and "none on record" said where
the key says nothing is. It needs no language model, so the same trace
always gets the same grade and it can gate CI. Judging a prose answer
against `ground_truth` needs a judge and is not built yet; the trace
carries an `answer` field for when it is.

## Installation

```bash
cargo install cqa-grader
```

## Usage

Grade a trace against a benchmark. Exit code 0 means every question
passed, 1 means at least one failed, 2 means the inputs were unusable.

```bash
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json --format json --report report.json
```

```text
wine-benchmark: grading cqa-grader baseline 0.1.0
  cq-02 lookup                  ok    recall 1.00  precision 0.02  retrieved 46
  cq-03 closed-world-negative   ok    recall 1.00  precision 0.18  retrieved 11
  cq-04 attribution             ok    recall 1.00  precision 0.06  retrieved 47
  cq-05 synthesis               ok    recall 1.00  precision 0.13  retrieved 47
  cq-06 comparison              ok    recall 1.00  precision 0.09  retrieved 46
  cq-07 comparison+attribution  ok    recall 1.00  precision 0.17  retrieved 23
6 questions: 6 passed, 0 failed
```

Run the naive baseline retriever over a graph, so a benchmark's CI has a
student to grade:

```bash
cqa-grader baseline --graph wine.ttl --benchmark wine-benchmark.ttl --output trace.json
```

A graph published as several files (one Turtle file per dataset, say) takes
one `--graph` per file. They load into one store, so there is no need to
concatenate them first.

Print the JSON Schema for the trace format:

```bash
cqa-grader trace-schema
```

### The trace

```json
{
  "trace_format": "0.1",
  "benchmark": "wine-benchmark",
  "system": { "name": "my-retriever", "version": "0.3.0" },
  "attempts": [
    {
      "question": "cq-03",
      "retrieved": [
        "https://w3id.org/wine-linkml/cabernet-sauvignon",
        "https://w3id.org/wine-linkml/seafood-dish"
      ],
      "cited": [],
      "asserts_absence": true
    }
  ]
}
```

`question` is the benchmark's `cqa:id` for the question. `retrieved` and
`cited` are IRIs into the target graph, exactly as the benchmark writes
its anchors. `asserts_absence` is how a system says "none on record",
which is what a closed-world negative is graded on. `answer` (optional)
is the prose answer, unread by this tier. The schema is in
[schemas/trace.schema.json](schemas/trace.schema.json).

### The checks

Every question gets three base checks, then one per answer kind it
names. All must pass.

| Check | Passes when |
|---|---|
| `attempted` | the trace holds an attempt at the question |
| `anchors-reached` | `retrieved` ⊇ `expected_anchors` |
| `citations-grounded` | `cited` ⊆ `retrieved` |
| `lookup`, `comparison`, `synthesis` | `anchors-reached` passes (no further check until cqa declares expected values) |
| `attribution` | `cited` ⊇ `expected_citations`, and `cited` is not empty |
| `closed-world-negative` | every `unconnected_anchors` record was retrieved, and the attempt `asserts_absence` |
| `citations-rest` | `cited` ⊇ `expected_citations`, for a question with citations that does not name `attribution` |

Recall and precision over `expected_anchors` are reported for every
attempt. Recall of one is what `anchors-reached` requires. Precision is
never required; it is where a real retriever separates itself from the
baseline.

A benchmark here is a test suite, not a leaderboard. Wine's has six
questions, so a pass says the system reached and cited what the key
asks for on each of them; it is too few to rank two systems that both
pass. Until a retrieval budget lands (see the roadmap), retrieving far
more than an answer needs costs nothing, which is how the baseline
passes with precision as low as 0.02.

### The baseline

The baseline links words in the question to records by `rdfs:label`,
expands each entry point by one hop of SPARQL lookup (both directions,
named individuals only), and cites everything it retrieved. It asserts
absence when the records the question named fall into more than one
group that nothing joins. It reaches every anchor in wine's benchmark
with precision between 0.02 and 0.18, which is the point: the benchmark
is answerable by lookup alone, and a retriever that cannot beat those
numbers has not added anything.

It never reads the key. A test blanks every anchor and checks the
attempts do not change.

## Library

```rust
use cqa_grader::{Benchmark, Trace, grade};

let benchmark = Benchmark::from_path("wine-benchmark.ttl")?;
let trace = Trace::from_path("trace.json")?;
let report = grade(&benchmark, &trace)?;
assert!(report.passed, "{}", report.to_text());
# Ok::<(), cqa_grader::Error>(())
```

## Development

See [CLAUDE.md](CLAUDE.md) for the commands.

### Prerequisites

- Rust toolchain (MSRV 1.88)
- [prek](https://github.com/j178/prek) for the pre-commit hooks: `cargo install prek && prek install`

### Test graphs

The test graphs in `tests/fixtures/` are committed, so `cargo nextest run`
works in a fresh clone. They are generated, not written by hand: to move
them forward you need [panschema](https://github.com/padamson/panschema)
and sibling checkouts of
[ontology-authoring-template](https://github.com/padamson/ontology-authoring-template)
and [cqa-schema](https://github.com/padamson/cqa-schema) next to this
repository.

```bash
./scripts/regen-fixtures.sh     # rewrites tests/fixtures/*.ttl and PROVENANCE
cargo nextest run
```

## License

MIT or Apache-2.0, at your option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
