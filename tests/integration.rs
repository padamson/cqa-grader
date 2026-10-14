//! The baseline against wine, end to end through the public API.

use cqa_grader::{AnswerKind, Baseline, Benchmark, Outcome, Require, grade};

mod common;

use common::fixture_text as fixture;

fn wine() -> (Benchmark, Baseline) {
    (
        common::wine_benchmark(),
        Baseline::from_turtle(&fixture("wine.ttl")).unwrap(),
    )
}

#[test]
fn the_baseline_leaves_only_closed_world_negatives_unanswered() {
    // It reaches every anchor and never asserts absence, so a negative
    // fails its own check and nothing else does, which is an abstention.
    let (benchmark, baseline) = wine();
    let trace = baseline
        .run(&benchmark, benchmark.target_graph(), 1)
        .unwrap();
    let report = grade(&benchmark, &trace, Require::Answered).unwrap();
    for r in &report.results {
        let failed: Vec<&str> = r
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.as_str())
            .collect();
        let negative = r.answer_kinds.contains(&AnswerKind::ClosedWorldNegative);
        let (expected_failed, expected_outcome): (&[&str], Outcome) = if negative {
            (&["closed-world-negative"], Outcome::Unanswered)
        } else {
            (&[], Outcome::Passed)
        };
        assert_eq!(failed, expected_failed, "{}", r.question);
        assert_eq!(r.outcome, expected_outcome, "{}", r.question);
    }
}

#[test]
fn the_baseline_never_reads_the_key() {
    // Same questions, everything but the id and the text blanked: the
    // trace must not change. Answer kinds are blanked too, or a baseline
    // that asserted absence because the key says closed-world-negative
    // would pass; so are the target pins, or one that copied the
    // benchmark's dataset into its trace would.
    let (benchmark, baseline) = wine();
    let honest = baseline
        .run(&benchmark, benchmark.target_graph(), 1)
        .unwrap();
    let mut blind = benchmark.clone();
    blind.target_schema.clear();
    blind.target_schema_version.clear();
    blind.target_dataset.clear();
    blind.target_dataset_version.clear();
    for q in &mut blind.questions {
        q.ground_truth.clear();
        q.answer_kinds.clear();
        q.expected_anchors.clear();
        q.expected_citations.clear();
        q.unconnected_anchors.clear();
        q.connecting_class = None;
    }
    let blinded = baseline.run(&blind, benchmark.target_graph(), 1).unwrap();
    assert_eq!(honest, blinded);
}

#[test]
fn the_baseline_never_asserts_absence() {
    let (benchmark, baseline) = wine();
    for hops in 0..=2 {
        let trace = baseline
            .run(&benchmark, benchmark.target_graph(), hops)
            .unwrap();
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
    let trace = baseline
        .run(&benchmark, benchmark.target_graph(), 0)
        .unwrap();
    let report = grade(&benchmark, &trace, Require::Answered).unwrap();
    let failed: Vec<&str> = report
        .results
        .iter()
        .filter(|r| r.outcome != Outcome::Passed)
        .map(|r| r.question.as_str())
        .collect();
    assert!(failed.contains(&"cq-04"), "{}", report.to_text());
}
