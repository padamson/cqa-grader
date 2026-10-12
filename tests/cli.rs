//! The binary, as CI would call it.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use cqa_grader::{Benchmark, Graph, System, Trace};
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
fn baseline_then_grade_fails_only_closed_world_negatives() {
    let dir = tempdir();
    let trace = dir.join("trace.json");
    cli()
        .args(["baseline", "--graph", &fixture("wine.ttl")])
        .args(wine_graph_args())
        .args(["--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--output", trace.to_str().unwrap()])
        .assert()
        .success();
    assert!(trace.exists());

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
        assert_eq!(r["passed"], !negative, "{r}");
    }
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
    let out = cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--trace", snippet.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let passed: Vec<&str> = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["passed"] == true)
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
    let whole = grade_baseline_over(&dir, &[&common::fixture_path("wine.ttl")]);
    assert_eq!(
        grade_baseline_over(&dir, &[&first, &second]),
        whole,
        "both halves"
    );
    // Each half alone misses anchors, so matching the whole needed both.
    assert!(grade_baseline_over(&dir, &[&first]) < whole, "first half");
    assert!(grade_baseline_over(&dir, &[&second]) < whole, "second half");
}

#[test]
fn a_graph_that_fails_to_load_is_named() {
    let dir = tempdir();
    let broken = dir.join("broken.ttl");
    std::fs::write(&broken, "this is not turtle").unwrap();
    let out = cli()
        .args(["baseline", "--graph", &fixture("wine.ttl")])
        .args(["--graph", broken.to_str().unwrap()])
        .args(wine_graph_args())
        .args(["--benchmark", &fixture("wine-benchmark.ttl")])
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
fn grade_baseline_over(dir: &Path, graphs: &[&Path]) -> u64 {
    let trace = dir.join("trace.json");
    let mut baseline = cli();
    baseline.arg("baseline");
    for graph in graphs {
        baseline.arg("--graph").arg(graph);
    }
    baseline
        .args(wine_graph_args())
        .args(["--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--output", trace.to_str().unwrap()])
        .assert()
        .success();
    let out = cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--trace", trace.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert!(
        matches!(out.status.code(), Some(0 | 1)),
        "grade could not run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
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
