//! The binary, as CI would call it.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use cqa_grader::{AnswerKind, Attempt, Benchmark, Graph, System, Trace};
use oxigraph::io::RdfFormat;
use oxigraph::model::{Triple, vocab::rdf};
use oxigraph::store::Store;
use predicates::prelude::*;

mod common;

fn fixture(name: &str) -> String {
    common::fixture_path(name).to_string_lossy().into_owned()
}

fn cli() -> Command {
    Command::cargo_bin("cqa-grader").unwrap()
}

fn wine_benchmark() -> &'static Benchmark {
    static WINE: LazyLock<Benchmark> = LazyLock::new(common::wine_benchmark);
    &WINE
}

/// `--dataset` and `--dataset-version` naming wine's graph as its
/// benchmark targets it, and as the graph's own dataset node states it.
fn wine_graph_args() -> [String; 4] {
    let target = wine_benchmark().target_graph();
    [
        "--dataset".into(),
        target.dataset,
        "--dataset-version".into(),
        target.dataset_version,
    ]
}

#[test]
fn baseline_then_grade_leaves_only_closed_world_negatives_unanswered() {
    let dir = tempdir();
    let trace = baseline_trace(&dir, &[&common::fixture_path("wine.ttl")]);
    let report = dir.join("report.json");
    cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--trace", trace.to_str().unwrap()])
        .args(["--report", report.to_str().unwrap()])
        .assert()
        .code(1);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    for r in json["results"].as_array().unwrap() {
        let negative = r["answer_kinds"]
            .as_array()
            .unwrap()
            .contains(&"closed-world-negative".into());
        let expected = if negative { "unanswered" } else { "passed" };
        assert_eq!(r["outcome"], expected, "{r}");
    }
}

#[test]
fn the_baseline_gives_no_wrong_answers() {
    // It leaves wine's negative unanswered, which this requirement
    // tolerates and the default one (the test above) does not.
    let dir = tempdir();
    let trace = baseline_trace(&dir, &[&common::fixture_path("wine.ttl")]);
    let (code, json) = grade_json(&trace, &["--require", "no-wrong-answers"]);
    assert_eq!(code, Some(0), "{json}");
    // The requirement had something to tolerate, so the pass is not vacuous.
    assert_eq!(json["summary"]["unanswered"], 1, "{json}");
    assert_eq!(json["summary"]["wrong"], 0, "{json}");
}

#[test]
fn grading_the_baseline_in_one_run_matches_baseline_then_grade() {
    // Under both requirements, so both exit codes are compared: the
    // baseline meets one and not the other.
    let dir = tempdir();
    let trace = baseline_trace(&dir, &[&common::fixture_path("wine.ttl")]);
    for require in ["answered", "no-wrong-answers"] {
        let mut one_run = over_wine("grade", &[&common::fixture_path("wine.ttl")]);
        one_run.args(["--baseline", "--require", require]);
        assert_eq!(
            report_json(&mut one_run),
            grade_json(&trace, &["--require", require]),
            "{require}"
        );
    }
}

#[test]
fn grading_from_the_yaml_matches_grading_from_the_turtle() {
    // The baseline's trace, so the reports hold both a pass and an
    // unanswered negative.
    let dir = tempdir();
    let trace = baseline_trace(&dir, &[&common::fixture_path("wine.ttl")]);
    let mut from_yaml = cli();
    from_yaml
        .arg("grade")
        .args(wine_yaml_args())
        .args(["--trace", trace.to_str().unwrap()]);
    assert_eq!(report_json(&mut from_yaml), grade_json(&trace, &[]));
}

#[test]
fn the_baseline_attempts_a_yaml_benchmark_as_it_does_the_turtle() {
    let dir = tempdir();
    let expected = baseline_trace(&dir, &[&common::fixture_path("wine.ttl")]);
    let mut from_yaml = cli();
    from_yaml
        .args(["baseline", "--graph", &fixture("wine.ttl")])
        .args(wine_yaml_args());
    let written = traced(&mut from_yaml, dir.join("from-yaml.json"));
    assert_eq!(
        Trace::from_path(&written).unwrap(),
        Trace::from_path(&expected).unwrap()
    );
}

#[test]
fn a_yaml_benchmark_without_its_schema_is_a_usage_error() {
    // Without the check it is parsed as Turtle, and the error is a Turtle
    // syntax error that never mentions the flag.
    cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.yaml")])
        .args(["--baseline", "--graph", &fixture("wine.ttl")])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--cqa-schema"));
}

#[test]
fn a_turtle_benchmark_with_a_schema_is_a_usage_error() {
    // Without the check the YAML loader parses the Turtle, and the error
    // is a YAML syntax error that never mentions the flag.
    cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--cqa-schema", &fixture("cqa.yaml")])
        .args(["--baseline", "--graph", &fixture("wine.ttl")])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--cqa-schema"));
}

#[test]
fn a_partial_read_refusal_is_printed_once() {
    let dir = tempdir();
    let yaml = common::fixture_text("wine-benchmark.yaml");
    assert!(yaml.contains("    expected_anchors:"), "{yaml}");
    let misspelled = dir.join("misspelled.yaml");
    std::fs::write(
        &misspelled,
        yaml.replacen("    expected_anchors:", "    expected_anchor:", 1),
    )
    .unwrap();
    let out = cli()
        .args(["grade", "--benchmark", misspelled.to_str().unwrap()])
        .args(["--cqa-schema", &fixture("cqa.yaml")])
        .args(["--baseline", "--graph", &fixture("wine.ttl")])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    // A refusal that was also the error's source printed twice.
    assert_eq!(stderr.matches("`expected_anchor`").count(), 1, "{stderr}");
}

/// `--benchmark` and `--cqa-schema` naming wine's benchmark by its LinkML
/// source.
fn wine_yaml_args() -> [String; 4] {
    [
        "--benchmark".into(),
        fixture("wine-benchmark.yaml"),
        "--cqa-schema".into(),
        fixture("cqa.yaml"),
    ]
}

#[test]
fn save_trace_writes_the_trace_baseline_would() {
    let dir = tempdir();
    let expected = baseline_trace(&dir, &[&common::fixture_path("wine.ttl")]);
    let written = dir.join("saved-trace.json");
    over_wine("grade", &[&common::fixture_path("wine.ttl")])
        .args(["--baseline", "--require", "no-wrong-answers"])
        .args(["--save-trace", written.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(
        Trace::from_path(&written).unwrap(),
        Trace::from_path(&expected).unwrap()
    );
}

#[test]
fn a_trace_is_graded_alone() {
    // Without the conflict, clap drops the requirements of an argument
    // that conflicts with one given, so `--dataset` beside `--trace` was
    // accepted and ignored.
    let dir = tempdir();
    let trace = baseline_trace(&dir, &[&common::fixture_path("wine.ttl")]);
    let graph = fixture("wine.ttl");
    let saved = dir.join("saved.json").to_string_lossy().into_owned();
    let extras: [&[&str]; 5] = [
        &["--baseline"],
        &["--graph", &graph],
        &["--dataset", "worked-example"],
        &["--hops", "2"],
        &["--save-trace", &saved],
    ];
    for extra in extras {
        cli()
            .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
            .args(["--trace", trace.to_str().unwrap()])
            .args(extra)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("cannot be used with"));
    }
}

#[test]
fn a_baseline_named_off_target_points_at_the_flags_and_keeps_its_trace() {
    // Grading refuses the trace (exit 2); with --baseline the user typed
    // no trace, so the error names the flags that named its graph, and the
    // saved trace is there to look at.
    let dir = tempdir();
    let unstated = wine_stating(&dir, None);
    let saved = dir.join("saved.json");
    let out = over_wine("grade", &[&unstated])
        .arg("--baseline")
        .args(["--dataset", &wine_benchmark().target_graph().dataset])
        .args(["--dataset-version", "9.9.9"])
        .args(["--save-trace", saved.to_str().unwrap()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("--dataset-version"), "{stderr}");
    assert!(saved.exists(), "{stderr}");
}

#[test]
fn a_graph_stating_another_version_is_refused_and_keeps_its_trace() {
    // As above, but the graph named itself, so the error points at its
    // dataset node, even where flags that agree with it were typed too.
    let dir = tempdir();
    let target = wine_benchmark().target_graph();
    let elsewhere = wine_stating(&dir, Some((&target.dataset, "9.9.9")));
    let agreeing = ["--dataset", &target.dataset, "--dataset-version", "9.9.9"];
    for (case, flags) in [("no flags", &[][..]), ("agreeing flags", &agreeing)] {
        let saved = dir.join(format!("{case}.json"));
        let out = over_wine("grade", &[&elsewhere])
            .arg("--baseline")
            .args(flags)
            .args(["--save-trace", saved.to_str().unwrap()])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{case}: {stderr}");
        assert!(stderr.contains("dcat:Dataset"), "{case}: {stderr}");
        assert!(!stderr.contains("--dataset"), "{case}: {stderr}");
        assert!(saved.exists(), "{case}: {stderr}");
    }
}

#[test]
fn a_flag_that_contradicts_the_graph_is_refused() {
    // The flags are a fallback, never an override: one that won would
    // relabel the graph past grading's check that it is the target.
    let dir = tempdir();
    let stated = wine_stating(&dir, Some(("held-here", "1.0.0")));
    for subcommand in ["baseline", "grade"] {
        for (dataset, version) in [("named-elsewhere", "1.0.0"), ("held-here", "2.0.0")] {
            let mut cmd = over_wine(subcommand, &[&stated]);
            if subcommand == "grade" {
                cmd.arg("--baseline");
            }
            let out = cmd
                .args(["--dataset", dataset, "--dataset-version", version])
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            let case = format!("{subcommand} {dataset} {version}");
            assert_eq!(out.status.code(), Some(2), "{case}: {stderr}");
            for named in ["held-here", "1.0.0", dataset, version] {
                assert!(stderr.contains(named), "{named} in {case}: {stderr}");
            }
        }
    }
}

#[test]
fn flags_that_agree_with_the_graph_change_nothing() {
    let dir = tempdir();
    let wine = common::fixture_path("wine.ttl");
    let without = baseline_trace(&dir, &[&wine]);
    let with = traced(
        over_wine("baseline", &[&wine]).args(wine_graph_args()),
        dir.join("with-flags.json"),
    );
    assert_eq!(
        Trace::from_path(&with).unwrap(),
        Trace::from_path(&without).unwrap()
    );
}

#[test]
fn a_flag_pinned_to_the_release_agrees_with_a_pre_release_graph() {
    // Grading takes 0.2.1-dev as 0.2.1, so the flags do too; the trace
    // records what the graph states.
    let dir = tempdir();
    let dev = wine_stating(&dir, Some(("worked-example", "0.2.1-dev")));
    let trace = traced(
        over_wine("baseline", &[&dev]).args([
            "--dataset",
            "worked-example",
            "--dataset-version",
            "0.2.1",
        ]),
        dir.join("trace.json"),
    );
    assert_eq!(
        Trace::from_path(&trace).unwrap().graph.dataset_version,
        "0.2.1-dev"
    );
}

#[test]
fn a_graph_that_states_no_dataset_needs_the_flags() {
    let dir = tempdir();
    let unstated = wine_stating(&dir, None);
    let out = over_wine("baseline", &[&unstated]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("--dataset-version"), "{stderr}");
    // Refused by the baseline, not by clap, which also exits 2.
    assert!(!stderr.contains("Usage:"), "{stderr}");
}

#[test]
fn graphs_naming_different_datasets_are_refused() {
    let dir = tempdir();
    let here = wine_stating(&dir.join("here"), Some(("held-here", "1.0.0")));
    let there = wine_stating(&dir.join("there"), Some(("held-there", "1.0.0")));
    let out = over_wine("baseline", &[&here, &there]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    for named in ["held-here", "held-there"] {
        assert!(stderr.contains(named), "{named}: {stderr}");
    }
}

#[test]
fn a_dataset_flag_alone_is_a_usage_error() {
    // Half a name would otherwise be dropped without a word, and the
    // graph's own statement used in its place.
    for (given, missing) in [
        ("--dataset", "--dataset-version <"),
        ("--dataset-version", "--dataset <"),
    ] {
        over_wine("baseline", &[&common::fixture_path("wine.ttl")])
            .args([given, "9.9.9"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(missing));
    }
}

#[test]
fn a_wrong_answer_fails_the_no_wrong_answers_requirement() {
    // Each spoils one question of an otherwise perfect trace in a way that
    // is an answer, not an abstention.
    type Spoil = fn(&mut Attempt);
    let spoils: [(&str, Spoil); 3] = [
        ("a false absence", |a| a.asserts_absence = true),
        ("an anchor not reached", |a| {
            // Dropped from `cited` too, or citations-grounded would also
            // fail and this case would not test the anchors check alone.
            let dropped = a.retrieved.pop_first().unwrap();
            a.cited.remove(&dropped);
        }),
        ("a citation never retrieved", |a| {
            a.cited.insert("https://w3id.org/wine-linkml/merlot".into());
        }),
    ];
    for (case, spoil) in spoils {
        let mut trace = perfect_trace();
        spoil(
            trace
                .attempts
                .iter_mut()
                .find(|a| a.question == "cq-04")
                .unwrap(),
        );
        let path = tempdir().join("trace.json");
        trace.write(&path).unwrap();
        let (code, json) = grade_json(&path, &["--require", "no-wrong-answers"]);
        assert_eq!(code, Some(1), "{case}: {json}");
        assert_eq!(json["summary"]["wrong"], 1, "{case}: {json}");
    }
}

/// Run the baseline over `graphs`, which name themselves, and return the
/// trace's path.
fn baseline_trace(dir: &Path, graphs: &[&Path]) -> PathBuf {
    traced(&mut over_wine("baseline", graphs), dir.join("trace.json"))
}

/// Run `cmd`, a `baseline` command, writing its trace to `trace`, and
/// return the path.
fn traced(cmd: &mut Command, trace: PathBuf) -> PathBuf {
    cmd.args(["--output", trace.to_str().unwrap()])
        .assert()
        .success();
    assert!(trace.exists());
    trace
}

/// `subcommand` (`baseline`, or `grade` with `--baseline`) over `graphs`
/// against wine's benchmark, with no flags naming the graph.
fn over_wine(subcommand: &str, graphs: &[&Path]) -> Command {
    let mut cmd = cli();
    cmd.arg(subcommand);
    for graph in graphs {
        cmd.arg("--graph").arg(graph);
    }
    cmd.args(["--benchmark", &fixture("wine-benchmark.ttl")]);
    cmd
}

/// Grade `trace` against wine's benchmark with `args` added, as
/// [`report_json`] does.
fn grade_json(trace: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    report_json(
        cli()
            .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
            .args(["--trace", trace.to_str().unwrap()])
            .args(args),
    )
}

/// Run a grading command with `--format json` and return the exit code and
/// the report. An exit code that is not a grade (2, unusable input) panics
/// with the binary's stderr.
fn report_json(cmd: &mut Command) -> (Option<i32>, serde_json::Value) {
    let out = cmd.args(["--format", "json"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        matches!(out.status.code(), Some(0 | 1)),
        "grade could not run: {stderr}"
    );
    let json = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {stderr}"));
    (out.status.code(), json)
}

/// A trace that retrieves and cites exactly the key for every question in
/// wine's benchmark, and asserts absence on its negative.
fn perfect_trace() -> Trace {
    let b = wine_benchmark();
    let system = System {
        name: "perfect".into(),
        version: None,
        notes: None,
    };
    let mut trace = Trace::new(b.id.clone(), b.target_graph(), system);
    trace.attempts = b
        .questions
        .iter()
        .map(|q| Attempt {
            question: q.id.clone(),
            retrieved: q.expected_anchors.clone(),
            cited: q.expected_citations.clone(),
            asserts_absence: q.answer_kinds.contains(&AnswerKind::ClosedWorldNegative),
            answer: None,
        })
        .collect();
    trace
}

#[test]
fn a_failing_trace_exits_one() {
    let trace = empty_trace(
        &tempdir(),
        "wine-benchmark",
        wine_benchmark().target_graph(),
    );
    cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--trace", trace.to_str().unwrap(), "--format", "json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"unattempted\": 6"));
}

#[test]
fn a_trace_for_another_benchmark_is_a_usage_error() {
    let trace = empty_trace(&tempdir(), "other", wine_benchmark().target_graph());
    cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--trace", trace.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "grading against \"wine-benchmark\"",
        ));
}

#[test]
fn a_trace_for_another_dataset_or_version_is_a_usage_error() {
    // Wine's anchors are written against one dataset at one version;
    // retrieval over any other says nothing about them.
    let target = wine_benchmark().target_graph();
    let elsewhere = Graph {
        dataset: "a-fork".into(),
        ..target.clone()
    };
    let older = Graph {
        dataset_version: "0.0.0-older".into(),
        ..target
    };
    for (graph, moved) in [(elsewhere, "a-fork"), (older, "0.0.0-older")] {
        let trace = empty_trace(&tempdir(), "wine-benchmark", graph);
        let out = cli()
            .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
            .args(["--trace", trace.to_str().unwrap()])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{moved}: {stderr}");
        assert!(stderr.contains(&format!("{moved:?}")), "{moved}: {stderr}");
    }
}

#[test]
fn the_documented_trace_is_read_as_written() {
    // The README's example, written by hand to the documented format
    // rather than by this crate's serializer. It attempts cq-03 alone and
    // passes it; the rest fail as unattempted.
    let snippet =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/site/snippets/step_trace.json");
    let (code, json) = grade_json(&snippet, &[]);
    assert_eq!(code, Some(1), "{json}");
    let passed: Vec<&str> = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["outcome"] == "passed")
        .map(|r| r["question"].as_str().unwrap())
        .collect();
    assert_eq!(passed, ["cq-03"]);
}

#[test]
fn the_baseline_records_the_graph_it_ran_over() {
    // Named apart from the benchmark's target, so a baseline that copied
    // the benchmark's pins into its trace would fail: once by the graph's
    // own dataset node, once by the flags over a graph that has none.
    let dir = tempdir();
    let stated = wine_stating(&dir.join("stated"), Some(("a-fork", "9.9.9")));
    let unstated = wine_stating(&dir.join("unstated"), None);
    let flags = ["--dataset", "a-fork", "--dataset-version", "9.9.9"];
    for (case, graph, args) in [("stated", stated, &[][..]), ("given", unstated, &flags)] {
        let trace = traced(
            over_wine("baseline", &[&graph]).args(args),
            dir.join(format!("{case}.json")),
        );
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(trace).unwrap()).unwrap();
        assert_eq!(
            json["graph"],
            serde_json::json!({"dataset": "a-fork", "dataset_version": "9.9.9"}),
            "{case}"
        );
    }
}

/// Write a trace for `benchmark` over `graph` with no attempts, and
/// return its path.
fn empty_trace(dir: &Path, benchmark: &str, graph: Graph) -> PathBuf {
    let system = System {
        name: "nothing".into(),
        version: None,
        notes: None,
    };
    let path = dir.join("trace.json");
    Trace::new(benchmark, graph, system).write(&path).unwrap();
    path
}

#[test]
fn baseline_reads_every_graph_given() {
    let dir = tempdir();
    let (first, second) = split_wine(&dir);
    let whole = passed_over(&dir, &[&common::fixture_path("wine.ttl")]);
    assert_eq!(passed_over(&dir, &[&first, &second]), whole, "both halves");
    // Each half alone misses anchors, so matching the whole needed both.
    assert!(passed_over(&dir, &[&first]) < whole, "first half");
    assert!(passed_over(&dir, &[&second]) < whole, "second half");
}

#[test]
fn a_graph_that_fails_to_load_is_named() {
    let dir = tempdir();
    let broken = dir.join("broken.ttl");
    std::fs::write(&broken, "this is not turtle").unwrap();
    let out = over_wine("baseline", &[&common::fixture_path("wine.ttl"), &broken])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("broken.ttl"), "{stderr}");
    // The file's error is its source, so a chain printer shows it once.
    assert_eq!(stderr.matches("rdf:").count(), 1, "{stderr}");
}

/// Run the baseline over `graphs`, grade its trace against wine's
/// benchmark, and return how many questions passed. The flags name the
/// graph, since a half of a split graph may carry no dataset node.
fn passed_over(dir: &Path, graphs: &[&Path]) -> u64 {
    let trace = traced(
        over_wine("baseline", graphs).args(wine_graph_args()),
        dir.join("trace.json"),
    );
    let (_, json) = grade_json(&trace, &[]);
    json["summary"]["passed"].as_u64().unwrap()
}

/// wine's graph as N-Triples (which are Turtle) in `dir`, its dataset node
/// replaced by one stating `stated` (a dataset and a version), or by none.
fn wine_stating(dir: &Path, stated: Option<(&str, &str)>) -> PathBuf {
    const DATASET: &str = "http://www.w3.org/ns/dcat#Dataset";
    let triples = wine_triples();
    let nodes: BTreeSet<String> = typed(&triples, DATASET)
        .filter(|t| t.subject.is_blank_node())
        .map(|t| t.subject.to_string())
        .collect();
    assert!(
        !nodes.is_empty(),
        "wine.ttl carries no dataset node to replace"
    );
    let mut text = ntriples(
        triples
            .iter()
            .filter(|t| !nodes.contains(&t.subject.to_string())),
    );
    if let Some((identifier, version)) = stated {
        writeln!(
            text,
            "_:stated a <{DATASET}> ; <http://purl.org/dc/terms/identifier> {identifier:?} ; \
             <http://www.w3.org/ns/dcat#version> {version:?} ."
        )
        .unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("wine.ttl");
    std::fs::write(&path, text).unwrap();
    path
}

/// wine's graph as two N-Triples files: every other named individual's
/// triples in the second, everything else in the first.
fn split_wine(dir: &Path) -> (PathBuf, PathBuf) {
    let triples = wine_triples();
    let individuals: BTreeSet<String> =
        typed(&triples, "http://www.w3.org/2002/07/owl#NamedIndividual")
            .map(|t| t.subject.to_string())
            .collect();
    let moved: BTreeSet<String> = individuals.into_iter().skip(1).step_by(2).collect();
    // Blank-node labels are local to a file, so a triple split from the
    // blank node it points at would point at a different one. Triples
    // split by subject, so a blank subject's triples (the graph's dataset
    // node) always land in one file.
    if let Some(t) = triples.iter().find(|t| t.object.is_blank_node()) {
        panic!("split_wine cannot split a graph that points at a blank node: {t}");
    }
    let (second, first): (Vec<&Triple>, Vec<&Triple>) = triples
        .iter()
        .partition(|t| moved.contains(&t.subject.to_string()));
    let paths = (dir.join("first.ttl"), dir.join("second.ttl"));
    std::fs::write(&paths.0, ntriples(first)).unwrap();
    std::fs::write(&paths.1, ntriples(second)).unwrap();
    paths
}

/// Every triple in wine's graph.
fn wine_triples() -> Vec<Triple> {
    let store = Store::new().unwrap();
    store
        .load_from_slice(RdfFormat::Turtle, &common::fixture_text("wine.ttl"))
        .unwrap();
    store.iter().map(|q| Triple::from(q.unwrap())).collect()
}

/// The triples typing a subject as `class`.
fn typed<'a>(triples: &'a [Triple], class: &str) -> impl Iterator<Item = &'a Triple> {
    let class = format!("<{class}>");
    triples
        .iter()
        .filter(move |t| t.predicate == rdf::TYPE && t.object.to_string() == class)
}

/// `triples` as N-Triples text.
fn ntriples<'a>(triples: impl IntoIterator<Item = &'a Triple>) -> String {
    triples.into_iter().map(|t| format!("{t} .\n")).collect()
}

#[test]
fn an_unreadable_trace_names_its_cause_once() {
    // The cause is the error's source, so a chain printer shows it once;
    // repeating it in the message would show it twice.
    let trace = tempdir().join("trace.json");
    std::fs::write(&trace, "{}").unwrap();
    let out = cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--trace", trace.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("trace.json"), "{stderr}");
    assert_eq!(stderr.matches("missing field").count(), 1, "{stderr}");
}

#[test]
fn trace_schema_prints_json_schema() {
    cli()
        .arg("trace-schema")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"trace_format\""));
}

/// A fresh directory for one test. The counter keeps two tests in one
/// process apart (as under `cargo test`, which cargo-mutants runs); the
/// clock's resolution on macOS is a microsecond, which they can share.
fn tempdir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "cqa-grader-test-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
