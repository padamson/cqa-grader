//! The binary, as CI would call it.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
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

#[test]
fn baseline_then_grade_fails_only_closed_world_negatives() {
    let dir = tempdir();
    let trace = dir.join("trace.json");
    cli()
        .args(["baseline", "--graph", &fixture("wine.ttl")])
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
    let dir = tempdir();
    let trace = dir.join("trace.json");
    std::fs::write(
        &trace,
        r#"{"trace_format":"0.1","benchmark":"wine-benchmark","system":{"name":"nothing"},"attempts":[]}"#,
    )
    .unwrap();
    cli()
        .args(["grade", "--benchmark", &fixture("wine-benchmark.ttl")])
        .args(["--trace", trace.to_str().unwrap(), "--format", "json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"unattempted\": 6"));
}

#[test]
fn a_trace_for_another_benchmark_is_a_usage_error() {
    let dir = tempdir();
    let trace = dir.join("trace.json");
    std::fs::write(
        &trace,
        r#"{"trace_format":"0.1","benchmark":"other","system":{"name":"x"},"attempts":[]}"#,
    )
    .unwrap();
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
fn trace_schema_prints_json_schema() {
    cli()
        .arg("trace-schema")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"trace_format\""));
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cqa-grader-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
