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
fn the_baseline_fails_only_closed_world_negatives() {
    // It reaches every anchor and never asserts absence, so a negative
    // fails its own check and nothing else does.
    let (benchmark, baseline) = wine();
    let trace = baseline.run(&benchmark, 1).unwrap();
    let report = grade(&benchmark, &trace).unwrap();
    for r in &report.results {
        let failed: Vec<&str> = r
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.as_str())
            .collect();
        let expected: &[&str] = if r.answer_kinds.contains(&AnswerKind::ClosedWorldNegative) {
            &["closed-world-negative"]
        } else {
            &[]
        };
        assert_eq!(failed, expected, "{}", r.question);
    }
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
fn the_baseline_never_asserts_absence() {
    let (benchmark, baseline) = wine();
    for hops in 0..=2 {
        let trace = baseline.run(&benchmark, hops).unwrap();
        let asserted: Vec<&str> = trace
            .attempts
            .iter()
            .filter(|a| a.asserts_absence)
            .map(|a| a.question.as_str())
            .collect();
        assert_eq!(asserted, Vec::<&str>::new(), "at {hops} hops");
    }
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
