//! The binary, as CI would call it.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use cqa_grader::{AnswerKind, Attempt, Benchmark, Graph, System, Trace};
use oxigraph::io::RdfFormat;
use oxigraph::model::{NamedNodeRef, Triple, vocab::rdf};
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
/// benchmark targets it.
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
    let saved = tempdir().join("saved.json");
    let out = cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--baseline", "--graph", &fixture("wine.ttl")])
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

/// Run the baseline over `graphs`, named as wine's benchmark targets its
/// graph, and return the trace's path.
fn baseline_trace(dir: &Path, graphs: &[&Path]) -> PathBuf {
    let trace = dir.join("trace.json");
    over_wine("baseline", graphs)
        .args(["--output", trace.to_str().unwrap()])
        .assert()
        .success();
    assert!(trace.exists());
    trace
}

/// `subcommand` (`baseline`, or `grade` with `--baseline`) over `graphs`,
/// named as wine's benchmark targets its graph.
fn over_wine(subcommand: &str, graphs: &[&Path]) -> Command {
    let mut cmd = cli();
    cmd.arg(subcommand);
    for graph in graphs {
        cmd.arg("--graph").arg(graph);
    }
    cmd.args(wine_graph_args())
        .args(["--benchmark", &fixture("wine-benchmark.ttl")]);
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
    // the benchmark's pins into its trace would fail.
    let dir = tempdir();
    let trace = dir.join("trace.json");
    cli()
        .args(["baseline", "--graph", &fixture("wine.ttl")])
        .args(["--dataset", "a-fork", "--dataset-version", "9.9.9"])
        .args(["--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--output", trace.to_str().unwrap()])
        .assert()
        .success();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(trace).unwrap()).unwrap();
    assert_eq!(
        json["graph"],
        serde_json::json!({"dataset": "a-fork", "dataset_version": "9.9.9"})
    );
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
/// benchmark, and return how many questions passed.
fn passed_over(dir: &Path, graphs: &[&Path]) -> u64 {
    let trace = baseline_trace(dir, graphs);
    let (_, json) = grade_json(&trace, &[]);
    json["summary"]["passed"].as_u64().unwrap()
}

/// wine's graph as two N-Triples files (which are Turtle): every other
/// named individual's triples in the second, everything else in the first.
fn split_wine(dir: &Path) -> (PathBuf, PathBuf) {
    let store = Store::new().unwrap();
    store
        .load_from_slice(RdfFormat::Turtle, &common::fixture_text("wine.ttl"))
        .unwrap();
    let individual = NamedNodeRef::new("http://www.w3.org/2002/07/owl#NamedIndividual").unwrap();
    let individuals: BTreeSet<String> = store
        .quads_for_pattern(None, Some(rdf::TYPE), Some(individual.into()), None)
        .map(|q| q.unwrap().subject.to_string())
        .collect();
    let moved: BTreeSet<String> = individuals.into_iter().skip(1).step_by(2).collect();
    let (mut first, mut second) = (String::new(), String::new());
    for quad in store.iter() {
        let triple = Triple::from(quad.unwrap());
        // Blank-node labels are local to a file, so a triple split from
        // the blank node it points at would point at a different one.
        assert!(
            !triple.subject.is_blank_node() && !triple.object.is_blank_node(),
            "split_wine cannot split a graph with blank nodes: {triple}"
        );
        let half = if moved.contains(&triple.subject.to_string()) {
            &mut second
        } else {
            &mut first
        };
        writeln!(half, "{triple} .").unwrap();
    }
    let paths = (dir.join("first.ttl"), dir.join("second.ttl"));
    std::fs::write(&paths.0, first).unwrap();
    std::fs::write(&paths.1, second).unwrap();
    paths
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
