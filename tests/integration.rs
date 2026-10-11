//! The baseline against wine, end to end through the public API.

use cqa_grader::{AnswerKind, Baseline, Benchmark, grade};

mod common;

use common::fixture_text as fixture;

fn wine() -> (Benchmark, Baseline) {
    (
        Benchmark::from_turtle(&fixture("wine-benchmark.ttl")).unwrap(),
        Baseline::from_turtle(&fixture("wine.ttl")).unwrap(),
    )
}

#[test]
fn the_baseline_passes_the_wine_benchmark() {
    let (benchmark, baseline) = wine();
    let trace = baseline.run(&benchmark, 1).unwrap();
    let report = grade(&benchmark, &trace).unwrap();
    assert!(report.passed, "{}", report.to_text());
    assert_eq!(report.summary.questions, 6);
}

#[test]
fn the_baseline_never_reads_the_key() {
    // Same questions, everything but the id and the text blanked: the
    // attempts must not change. Answer kinds are blanked too, or a
    // baseline that asserted absence because the key says
    // closed-world-negative would pass.
    let (benchmark, baseline) = wine();
    let honest = baseline.run(&benchmark, 1).unwrap();
    let mut blind = benchmark.clone();
    for q in &mut blind.questions {
        q.ground_truth.clear();
        q.answer_kinds.clear();
        q.expected_anchors.clear();
        q.expected_citations.clear();
        q.unconnected_anchors.clear();
        q.connecting_class = None;
    }
    let blinded = baseline.run(&blind, 1).unwrap();
    assert_eq!(honest.attempts, blinded.attempts);
}

#[test]
fn the_baseline_asserts_absence_only_where_nothing_joins() {
    let (benchmark, baseline) = wine();
    let trace = baseline.run(&benchmark, 1).unwrap();
    for q in &benchmark.questions {
        let attempt = trace.attempt(&q.id).unwrap();
        if q.answer_kinds.contains(&AnswerKind::ClosedWorldNegative) {
            assert!(attempt.asserts_absence, "{}: should assert absence", q.id);
        }
    }
    // The pairing question names a wine and a food that a recommendation joins.
    let joined = baseline
        .retrieve("x", "What is the best choice of wine for grilled meat?", 1)
        .unwrap();
    assert!(!joined.asserts_absence);
}

#[test]
fn entry_points_alone_are_not_enough() {
    // At zero hops the baseline retrieves only what the question names.
    // Wine's attribution question names a food and asks for the wine; the
    // recommendation that joins them is one edge away, so this must fail.
    // Recorded so a change in the fixture or the linker is noticed.
    let (benchmark, baseline) = wine();
    let trace = baseline.run(&benchmark, 0).unwrap();
    let report = grade(&benchmark, &trace).unwrap();
    let failed: Vec<&str> = report
        .results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| r.question.as_str())
        .collect();
    assert!(failed.contains(&"cq-04"), "{}", report.to_text());
}
