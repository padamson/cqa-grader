//! The binary, as CI would call it.

use assert_cmd::Command;
use predicates::prelude::*;

mod common;

fn fixture(name: &str) -> String {
    common::fixture_path(name).to_string_lossy().into_owned()
}

fn cli() -> Command {
    Command::cargo_bin("cqa-grader").unwrap()
}

#[test]
fn baseline_then_grade_passes_wine() {
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
        .success()
        .stdout(predicate::str::contains("6 questions: 6 passed, 0 failed"));
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    assert_eq!(json["passed"], true);
    assert_eq!(json["summary"]["passed"], 6);
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
fn trace_schema_prints_json_schema() {
    cli()
        .arg("trace-schema")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"trace_format\""));
}

fn tempdir() -> std::path::PathBuf {
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
