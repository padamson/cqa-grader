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
baseline shows whether every record an answer needs can be reached in the
example (the
[ontology-authoring-template](https://github.com/padamson/ontology-authoring-template)'s
wine ontology carries both). A graph's repository runs the baseline in
CI to show the same of its published graph. Someone building a retriever has their system write a trace and
grades it against the benchmark.

Two tiers were always the plan. The deterministic tier is here: anchors
reached, citations grounded and cited, and "none on record" said where,
and only where, the key says nothing is. It needs no language model, so the same trace
always gets the same grade and it can gate CI. Judging a prose answer
against `ground_truth` needs a judge and is not built yet; the trace
carries an `answer` field for when it is.

## Installation

```bash
cargo install cqa-grader
```

## Usage

Grade a trace against a benchmark. Exit code 0 means the trace met what
`--require` asks of it, 1 means it did not, 2 means the inputs were
unusable. The default, `--require answered`, asks that every question pass
its checks. `--require no-wrong-answers` tolerates a question left
unanswered (a closed-world negative whose records were reached, with no
"none on record" said) and still fails a wrong answer or an unattempted
question; it is what a benchmark's CI asks of the baseline.

```bash
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json --require no-wrong-answers
cqa-grader grade --benchmark wine-benchmark.ttl --trace trace.json --format json --report report.json
```

```text
wine-benchmark: grading cqa-grader baseline 0.1.0 over worked-example 0.2.1, requiring no wrong answers
  cq-02 lookup                  ok          recall 1.00  precision 0.02  retrieved 46
  cq-03 closed-world-negative   unanswered  recall 1.00  precision 0.18  retrieved 11
    closed-world-negative: unconnected anchors reached but the attempt does not assert absence
  cq-04 attribution             ok          recall 1.00  precision 0.06  retrieved 47
  cq-05 synthesis               ok          recall 1.00  precision 0.13  retrieved 47
  cq-06 comparison              ok          recall 1.00  precision 0.09  retrieved 46
  cq-07 comparison+attribution  ok          recall 1.00  precision 0.17  retrieved 23
6 questions: 5 passed, 1 unanswered, 0 wrong
```

Run the naive baseline retriever over a graph, so a benchmark's CI has a
student to grade:

```bash
cqa-grader baseline --graph wine.ttl --dataset worked-example --dataset-version 0.2.1 \
  --benchmark wine-benchmark.ttl --output trace.json
```

A graph published as several files (one Turtle file per dataset, say) takes
one `--graph` per file. They load into one store, so there is no need to
concatenate them first. `--dataset` and `--dataset-version` name what those
files hold, the way the benchmark's target names it, and the trace records
them as given. A published graph does not name its dataset, so the baseline
takes them on trust; grading checks them against the benchmark. In a CI
step they move with each release.

Print the JSON Schema for the trace format:

```bash
cqa-grader trace-schema
```

### The trace

```json
{
  "trace_format": "0.1",
  "benchmark": "wine-benchmark",
  "graph": { "dataset": "worked-example", "dataset_version": "0.2.1" },
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

`graph` names the dataset and version the system ran over, the way the
benchmark's `target_dataset` and `target_dataset_version` name its target.
Grading refuses a trace whose `graph` differs (exit code 2), since anchors
written against one version of a dataset say nothing about retrieval over
another. A pre-release counts as its release, so a graph built on `main` as
`0.2.1-dev` grades against a key pinned to `0.2.1`. `question` is the
benchmark's `cqa:id` for the question. `retrieved` and `cited` are IRIs into
the target graph, exactly as the benchmark writes its anchors.
`asserts_absence` is how a system says "none on record", which is what a
closed-world negative is graded on. `answer` (optional) is the prose answer,
unread by this tier. The schema is in
[schemas/trace.schema.json](schemas/trace.schema.json).

### The checks

Every question gets four base checks, then one per answer kind it
names. All must pass.

| Check | Passes when |
|---|---|
| `attempted` | the trace holds an attempt at the question |
| `anchors-reached` | `retrieved` ⊇ `expected_anchors` |
| `citations-grounded` | `cited` ⊆ `retrieved` |
| `no-false-absence` | the attempt `asserts_absence` only if the question is a `closed-world-negative` |
| `lookup`, `comparison`, `synthesis` | `anchors-reached` passes (no further check until cqa declares expected values) |
| `attribution` | `cited` ⊇ `expected_citations`, and `cited` is not empty |
| `closed-world-negative` | every `unconnected_anchors` record was retrieved, and the attempt `asserts_absence` |
| `citations-rest` | `cited` ⊇ `expected_citations`, for a question with citations that does not name `attribution` |

Each question ends in one of four outcomes, and the summary counts them.
`passed`: every check did. `unanswered`: a closed-world negative whose
unconnected anchors were all reached, with no absence asserted and nothing
else failed; the system had the records in hand and said nothing. `wrong`:
a check failed any other way. In this tier the answer is the evidence, so
a false "none on record", an anchor not reached, a citation retrieval
never reached, or an attribution without its citations is a wrong answer
whatever the prose says. `unattempted`: the trace holds no attempt. The
text report shows the outcome in its verdict column, and the JSON report
carries it per question, along with the requirement the grade was held to.

Recall and precision over `expected_anchors` are reported for every
attempt. Recall of one is what `anchors-reached` requires. Precision is
never required; it is where a real retriever separates itself from the
baseline.

A benchmark here is a test suite, not a leaderboard. Wine's has six
questions, so a pass says the system reached and cited what the key
asks for on each of them; it is too few to rank two systems that both
pass. Until a retrieval budget lands (see the roadmap), retrieving far
more than an answer needs costs nothing, which is how the baseline
passes questions with precision as low as 0.02.

### The baseline

The baseline links words in the question to records by `rdfs:label`,
expands each entry point by one hop of SPARQL lookup (both directions,
named individuals only), and cites everything it retrieved. It reaches
every anchor in wine's benchmark with precision between 0.02 and 0.18,
which is the point: every record an answer needs can be found by lookup
alone, and a retriever that cannot beat those numbers has not added
anything.

It never says "none on record", so it leaves every closed-world negative
unanswered (wine's cq-03, as above), which `--require no-wrong-answers`
tolerates. A negative's claim is about particular records,
and which records is written in the key, not the question. "Is Bordeaux a
red or white wine?" links Bordeaux and white, nothing joins them, and the
answer is still on record; "What is the best choice of wine for grilled
meat?" names one record and the class Wine, no wine references the
grilled meat, and that answer is on record too, in a pairing
recommendation. A rule that read the claim from the question's words
would say "none on record" for both. Whether a benchmark's negatives hold
is shown against the graph by panschema's `verify --strict`; the baseline
shows their anchors can be reached.

It never reads the key, or the benchmark's target. A test blanks every
anchor along with the benchmark's target schema, dataset, and their
versions, and checks the trace does not change. The trace names the graph
the baseline was given, never the one the benchmark targets.

## Library

```rust
use cqa_grader::{Benchmark, Require, Trace, grade};

let benchmark = Benchmark::from_path("wine-benchmark.ttl")?;
let trace = Trace::from_path("trace.json")?;
let report = grade(&benchmark, &trace, Require::Answered)?;
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
