//! Score a trace against a benchmark.
//!
//! Every question gets the same four base checks, then one check per
//! answer kind it names. All of them must pass for the question to pass.
//!
//! Base checks:
//!
//! - `attempted`: the trace holds an attempt at the question.
//! - `anchors-reached`: `retrieved` ⊇ `expected_anchors`.
//! - `citations-grounded`: `cited` ⊆ `retrieved`. A citation to a record
//!   retrieval never reached is a fabrication, whatever the key says.
//! - `no-false-absence`: the attempt `asserts_absence` only if the question
//!   is a `closed-world-negative`. "None on record" for a question whose
//!   answer is on record is a wrong answer, however much was retrieved.
//!
//! Per-kind checks:
//!
//! - `lookup`, `comparison`, `synthesis`: nothing beyond the anchors, until
//!   cqa declares expected values. The check is recorded so the dispatch is
//!   visible, and it passes when `anchors-reached` does.
//! - `attribution`: `cited` ⊇ `expected_citations`, and non-empty.
//! - `closed-world-negative`: the attempt `asserts_absence`, and every
//!   `unconnected_anchors` record was retrieved. A system that reaches both
//!   records and says nothing has not answered.
//!
//! When a question names `expected_citations` without naming
//! `attribution` (wine's synthesis question does), a `citations-rest`
//! check runs the same superset test.
//!
//! Every question ends in one of four [`Outcome`]s: passed, unanswered (a
//! closed-world negative whose records were reached, with no absence
//! asserted and nothing else failed), wrong, or unattempted. A grade is
//! held to a [`Require`]ment, which says which outcomes fail it. The
//! variants of both carry the definitions.
//!
//! Recall and precision over `expected_anchors` are reported for every
//! attempt as information. Recall of one is what `anchors-reached`
//! requires; precision is never required, but it is where a good retriever
//! separates itself from a naive one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::benchmark::{AnswerKind, Benchmark, Question};
use crate::trace::{Attempt, Graph, System, Trace, write_json};
use crate::{Error, Result};

/// One check on one question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// The check's name: a base check or an answer kind.
    pub name: String,
    /// Whether it passed.
    pub passed: bool,
    /// What was tested, in a sentence.
    pub detail: String,
    /// Records the check wanted and did not find, if that is what failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

impl Check {
    fn new(name: &str, passed: bool, detail: impl Into<String>) -> Self {
        Check {
            name: name.into(),
            passed,
            detail: detail.into(),
            missing: Vec::new(),
        }
    }

    /// Passes when `have` ⊇ `want`, and lists whatever of `want` is missing.
    fn superset(
        name: &str,
        have: &BTreeSet<String>,
        want: &BTreeSet<String>,
        what: &str,
        found: &str,
    ) -> Self {
        let missing: Vec<String> = want.difference(have).cloned().collect();
        let detail = if missing.is_empty() {
            format!("all {} {what} {found}", want.len())
        } else {
            format!("{} of {} {what} not {found}", missing.len(), want.len())
        };
        Check {
            passed: missing.is_empty(),
            missing,
            ..Check::new(name, false, detail)
        }
    }
}

/// How one question came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Every check passed.
    Passed,
    /// A closed-world negative whose unconnected anchors were all reached,
    /// with no absence asserted and nothing else failed. The system had
    /// the records in hand and said nothing.
    Unanswered,
    /// A check failed some other way. In this tier the answer is the
    /// evidence, so a false "none on record", an anchor not reached, a
    /// citation retrieval never reached, or an attribution without its
    /// citations is a wrong answer whatever the prose says.
    Wrong,
    /// The trace holds no attempt at the question.
    Unattempted,
}

impl Outcome {
    /// The verdict column of the text report: the outcome's name, in
    /// capitals when `require` rejects it, so a reader scanning for them
    /// finds what failed the grade and nothing the grade let through.
    fn label(self, require: Require) -> String {
        let name = match self {
            Outcome::Passed => "ok",
            Outcome::Unanswered => "unanswered",
            Outcome::Wrong => "wrong",
            Outcome::Unattempted => "unattempted",
        };
        if require.rejects(self) {
            name.to_uppercase()
        } else {
            name.to_owned()
        }
    }
}

/// What a trace must do for its grade to pass. The CLI takes it as
/// `--require`, spelled as the JSON report spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Require {
    /// Every question passed its checks, which is what answering one means
    /// here: a wrong answer fails this as an unanswered question does.
    #[default]
    Answered,
    /// No question answered wrongly and none left unattempted (tolerating
    /// those would pass an empty trace). A question left unanswered (a
    /// closed-world negative whose records were reached, with no absence
    /// asserted) is tolerated, so a system that abstains on negatives can
    /// meet it.
    NoWrongAnswers,
}

impl Require {
    /// Whether a question with this outcome fails the requirement.
    pub fn rejects(self, outcome: Outcome) -> bool {
        match self {
            Require::Answered => outcome != Outcome::Passed,
            Require::NoWrongAnswers => {
                matches!(outcome, Outcome::Wrong | Outcome::Unattempted)
            }
        }
    }

    /// The requirement in words, for the text report's header.
    pub fn describe(self) -> &'static str {
        match self {
            Require::Answered => "every question answered",
            Require::NoWrongAnswers => "no wrong answers",
        }
    }
}

/// The outcome for one question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionResult {
    /// The question's `cqa:id`.
    pub question: String,
    /// The question's IRI in the benchmark graph.
    pub iri: String,
    /// The kinds the question names.
    pub answer_kinds: Vec<AnswerKind>,
    /// How it came out.
    pub outcome: Outcome,
    /// How many records the attempt retrieved.
    pub retrieved: usize,
    /// Share of `expected_anchors` that were retrieved.
    pub recall: f64,
    /// Share of retrieved records that are `expected_anchors`.
    pub precision: f64,
    /// Every check run, in order.
    pub checks: Vec<Check>,
}

/// Counts over the whole benchmark. The four outcomes partition the
/// questions, so they sum to `questions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Questions in the benchmark.
    pub questions: usize,
    /// Questions whose every check passed.
    pub passed: usize,
    /// Closed-world negatives reached and left unanswered.
    pub unanswered: usize,
    /// Questions answered wrongly.
    pub wrong: usize,
    /// Questions the trace did not attempt.
    pub unattempted: usize,
}

/// A graded trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The benchmark's `cqa:id`.
    pub benchmark: String,
    /// The graph the graded attempts were made over, as the trace names
    /// it. Grading has checked it is the benchmark's target.
    pub graph: Graph,
    /// The system that was graded.
    pub system: System,
    /// What the trace was held to.
    pub require: Require,
    /// Whether the trace met it.
    pub passed: bool,
    /// The counts.
    pub summary: Summary,
    /// One result per question, in `cqa:id` order.
    pub results: Vec<QuestionResult>,
    /// Attempts at questions the benchmark does not hold. Not graded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_attempts: Vec<String>,
}

impl Report {
    /// Pretty-printed JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Write to a JSON file.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        write_json(path, self)
    }

    /// A plain-text rendering, one line per question plus any failures.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let version = self
            .system
            .version
            .as_deref()
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}: grading {}{} over {} {}, requiring {}\n",
            self.benchmark,
            self.system.name,
            version,
            self.graph.dataset,
            self.graph.dataset_version,
            self.require.describe()
        ));
        let width = self
            .results
            .iter()
            .map(|r| r.question.len() + 1 + kinds_label(&r.answer_kinds).len())
            .max()
            .unwrap_or(0);
        let verdict_width = self
            .results
            .iter()
            .map(|r| r.outcome.label(self.require).len())
            .max()
            .unwrap_or(0);
        for r in &self.results {
            let label = format!("{} {}", r.question, kinds_label(&r.answer_kinds));
            let verdict = r.outcome.label(self.require);
            out.push_str(&format!(
                "  {label:<width$}  {verdict:<verdict_width$}  recall {:.2}  precision {:.2}  retrieved {}\n",
                r.recall, r.precision, r.retrieved
            ));
            for c in r.checks.iter().filter(|c| !c.passed) {
                out.push_str(&format!("    {}: {}\n", c.name, c.detail));
                for m in &c.missing {
                    out.push_str(&format!("      missing <{m}>\n"));
                }
            }
        }
        for q in &self.unknown_attempts {
            out.push_str(&format!("  {q}: not in benchmark, not graded\n"));
        }
        let s = &self.summary;
        out.push_str(&format!(
            "{} questions: {} passed, {} unanswered, {} wrong",
            s.questions, s.passed, s.unanswered, s.wrong
        ));
        if s.unattempted > 0 {
            out.push_str(&format!(", {} unattempted", s.unattempted));
        }
        out.push('\n');
        out
    }
}

fn kinds_label(kinds: &[AnswerKind]) -> String {
    kinds
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join("+")
}

/// The release a version belongs to: `x.y.z-<anything>` is `x.y.z`, so a
/// graph built on the way to a release (`0.1.0-dev`) grades against a key
/// pinned to it. Any other string is its own release, so `0.1.0dev` and
/// `v0.1.0` stay distinct from `0.1.0`.
fn release(version: &str) -> &str {
    match version.split_once('-') {
        Some((core, pre))
            if !pre.is_empty()
                && core.split('.').count() == 3
                && core
                    .split('.')
                    .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) =>
        {
            core
        }
        _ => version,
    }
}

/// Grade a trace against a benchmark, holding it to `require`.
///
/// Fails with [`Error::Trace`] when the trace fails [`Trace::check`],
/// names a different benchmark, or was run over a dataset or version other
/// than the benchmark's target. Each is a usage error, not a failed grade.
pub fn grade(benchmark: &Benchmark, trace: &Trace, require: Require) -> Result<Report> {
    trace.check()?;
    if trace.benchmark != benchmark.id {
        return Err(Error::Trace(format!(
            "trace is for benchmark {:?}, grading against {:?}",
            trace.benchmark, benchmark.id
        )));
    }
    let (ran, target) = (&trace.graph, benchmark.target_graph());
    if ran.dataset != target.dataset
        || release(&ran.dataset_version) != release(&target.dataset_version)
    {
        return Err(Error::Trace(format!(
            "trace was run over dataset {:?} version {:?}, the benchmark targets {:?} version {:?}",
            ran.dataset, ran.dataset_version, target.dataset, target.dataset_version
        )));
    }

    let attempts: BTreeMap<&str, &Attempt> = trace
        .attempts
        .iter()
        .map(|a| (a.question.as_str(), a))
        .collect();
    let results: Vec<QuestionResult> = benchmark
        .questions
        .iter()
        .map(|q| grade_question(q, attempts.get(q.id.as_str()).copied()))
        .collect();
    let unknown_attempts: Vec<String> = attempts
        .keys()
        .filter(|id| benchmark.question(id).is_none())
        .map(|id| id.to_string())
        .collect();

    let count = |outcome: Outcome| results.iter().filter(|r| r.outcome == outcome).count();
    let summary = Summary {
        questions: results.len(),
        passed: count(Outcome::Passed),
        unanswered: count(Outcome::Unanswered),
        wrong: count(Outcome::Wrong),
        unattempted: count(Outcome::Unattempted),
    };
    Ok(Report {
        benchmark: benchmark.id.clone(),
        graph: trace.graph.clone(),
        system: trace.system.clone(),
        require,
        passed: results.iter().all(|r| !require.rejects(r.outcome)),
        summary,
        results,
        unknown_attempts,
    })
}

fn grade_question(question: &Question, attempt: Option<&Attempt>) -> QuestionResult {
    let answer_kinds: Vec<AnswerKind> = question.answer_kinds.iter().copied().collect();
    let Some(attempt) = attempt else {
        return QuestionResult {
            question: question.id.clone(),
            iri: question.iri.clone(),
            answer_kinds,
            outcome: Outcome::Unattempted,
            retrieved: 0,
            recall: 0.0,
            precision: 0.0,
            checks: vec![Check::new(
                "attempted",
                false,
                "no attempt at this question in the trace",
            )],
        };
    };

    let mut checks = vec![Check::new("attempted", true, "attempt present")];

    let anchors = Check::superset(
        "anchors-reached",
        &attempt.retrieved,
        &question.expected_anchors,
        "expected anchors",
        "reached",
    );
    let anchors_reached = anchors.passed;
    checks.push(anchors);

    checks.push(Check::superset(
        "citations-grounded",
        &attempt.retrieved,
        &attempt.cited,
        "citations",
        "retrieved",
    ));

    let negative = question
        .answer_kinds
        .contains(&AnswerKind::ClosedWorldNegative);
    let false_absence = attempt.asserts_absence && !negative;
    // Reached a negative's records and said nothing: the one failure the
    // outcome counts as an abstention rather than a wrong answer.
    let abstained = negative
        && !attempt.asserts_absence
        && question.unconnected_anchors.is_subset(&attempt.retrieved);
    checks.push(Check::new(
        "no-false-absence",
        !false_absence,
        if false_absence {
            "the attempt asserts absence, and the key says the answer is on record"
        } else {
            "no absence asserted against the key"
        },
    ));

    let names_attribution = question.answer_kinds.contains(&AnswerKind::Attribution);
    if !question.expected_citations.is_empty() && !names_attribution {
        checks.push(Check::superset(
            "citations-rest",
            &attempt.cited,
            &question.expected_citations,
            "expected citations",
            "cited",
        ));
    }

    for kind in &question.answer_kinds {
        checks.push(match kind {
            AnswerKind::Lookup | AnswerKind::Comparison | AnswerKind::Synthesis => {
                let detail = format!(
                    "{}: no check beyond the anchors until cqa declares expected values",
                    kind.as_str()
                );
                Check::new(kind.as_str(), anchors_reached, detail)
            }
            AnswerKind::Attribution => {
                let mut c = Check::superset(
                    kind.as_str(),
                    &attempt.cited,
                    &question.expected_citations,
                    "expected citations",
                    "cited",
                );
                if c.passed && attempt.cited.is_empty() {
                    c = Check::new(kind.as_str(), false, "an attribution must cite something");
                }
                c
            }
            AnswerKind::ClosedWorldNegative => {
                let reached = Check::superset(
                    kind.as_str(),
                    &attempt.retrieved,
                    &question.unconnected_anchors,
                    "unconnected anchors",
                    "reached",
                );
                if !reached.passed {
                    reached
                } else if attempt.asserts_absence {
                    Check::new(
                        kind.as_str(),
                        true,
                        "unconnected anchors reached and absence asserted",
                    )
                } else {
                    Check::new(
                        kind.as_str(),
                        false,
                        "unconnected anchors reached but the attempt does not assert absence",
                    )
                }
            }
        });
    }

    // An abstention is the negative's own check failing and nothing else;
    // a second failure makes the attempt a wrong answer however silent.
    let failed = checks.iter().filter(|c| !c.passed).count();
    let outcome = if failed == 0 {
        Outcome::Passed
    } else if abstained && failed == 1 {
        Outcome::Unanswered
    } else {
        Outcome::Wrong
    };

    let hits = attempt
        .retrieved
        .intersection(&question.expected_anchors)
        .count() as f64;
    let recall = hits / question.expected_anchors.len() as f64;
    let precision = if attempt.retrieved.is_empty() {
        0.0
    } else {
        hits / attempt.retrieved.len() as f64
    };

    QuestionResult {
        question: question.id.clone(),
        iri: question.iri.clone(),
        answer_kinds,
        outcome,
        retrieved: attempt.retrieved.len(),
        recall,
        precision,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wine() -> Benchmark {
        Benchmark::from_turtle(&crate::benchmark::tests::wine_benchmark()).unwrap()
    }

    fn system() -> System {
        System {
            name: "test".into(),
            version: None,
            notes: None,
        }
    }

    /// An attempt that retrieves and cites exactly the key.
    fn perfect(q: &Question) -> Attempt {
        Attempt {
            question: q.id.clone(),
            retrieved: q.expected_anchors.clone(),
            cited: q.expected_citations.clone(),
            asserts_absence: q.answer_kinds.contains(&AnswerKind::ClosedWorldNegative),
            answer: None,
        }
    }

    /// Wine's closed-world negative, its records reached and nothing said.
    fn silent_negative(b: &Benchmark) -> Attempt {
        Attempt {
            asserts_absence: false,
            ..perfect(b.question("cq-03").unwrap())
        }
    }

    /// A positive question answered "none on record".
    fn false_absence(b: &Benchmark) -> Attempt {
        Attempt {
            asserts_absence: true,
            ..perfect(b.question("cq-04").unwrap())
        }
    }

    /// The baseline's shape: every anchor reached, no absence asserted.
    fn abstaining(b: &Benchmark) -> Vec<Attempt> {
        b.questions
            .iter()
            .map(|q| Attempt {
                asserts_absence: false,
                ..perfect(q)
            })
            .collect()
    }

    fn trace_with(benchmark: &Benchmark, attempts: Vec<Attempt>) -> Trace {
        let mut t = Trace::new(benchmark.id.clone(), benchmark.target_graph(), system());
        t.attempts = attempts;
        t
    }

    fn check<'a>(r: &'a QuestionResult, name: &str) -> &'a Check {
        r.checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no check {name} in {:?}", r.checks))
    }

    /// Grade `attempts` against `benchmark` under the default requirement.
    fn graded(benchmark: &Benchmark, attempts: Vec<Attempt>) -> Report {
        grade(
            benchmark,
            &trace_with(benchmark, attempts),
            Require::Answered,
        )
        .unwrap()
    }

    fn result<'a>(report: &'a Report, question: &str) -> &'a QuestionResult {
        report
            .results
            .iter()
            .find(|r| r.question == question)
            .unwrap_or_else(|| panic!("no result for {question} in {:?}", report.results))
    }

    #[test]
    fn two_attempts_at_one_question_are_an_error() {
        // Grading only the first would hide the second from the report.
        let b = wine();
        let q = b.question("cq-03").unwrap();
        let t = trace_with(&b, vec![perfect(q), Attempt::empty("cq-03")]);
        let err = grade(&b, &t, Require::Answered).unwrap_err();
        assert!(
            matches!(&err, Error::Trace(m) if m.contains("cq-03")),
            "{err}"
        );
    }

    #[test]
    fn a_perfect_trace_passes_every_question() {
        let b = wine();
        let report = graded(&b, b.questions.iter().map(perfect).collect());
        assert!(report.passed, "{}", report.to_text());
        assert_eq!(
            report.summary,
            Summary {
                questions: 6,
                passed: 6,
                unanswered: 0,
                wrong: 0,
                unattempted: 0,
            }
        );
        assert!(
            report
                .results
                .iter()
                .all(|r| r.recall == 1.0 && r.precision == 1.0)
        );
    }

    #[test]
    fn an_empty_trace_fails_every_question_as_unattempted() {
        let b = wine();
        let report = graded(&b, Vec::new());
        assert!(!report.passed);
        assert_eq!(report.summary.unattempted, 6);
        assert_eq!(report.summary.passed, 0);
        for r in &report.results {
            assert_eq!(r.outcome, Outcome::Unattempted);
            assert_eq!(r.checks.len(), 1);
            assert_eq!(r.checks[0].name, "attempted");
            assert!(!r.checks[0].passed);
        }
    }

    #[test]
    fn a_missing_anchor_fails_anchors_reached_and_names_it() {
        let b = wine();
        let q = b.question("cq-04").unwrap();
        let mut a = perfect(q);
        let dropped = "https://w3id.org/wine-linkml/grilled-meat".to_string();
        assert!(a.retrieved.remove(&dropped));
        let report = graded(&b, vec![a]);
        let r = &report.results[2];
        assert_eq!(r.question, "cq-04");
        assert_eq!(r.outcome, Outcome::Wrong);
        let c = check(r, "anchors-reached");
        assert!(!c.passed);
        assert_eq!(c.missing, vec![dropped]);
        assert!((r.recall - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_citation_never_retrieved_is_ungrounded() {
        let b = wine();
        let q = b.question("cq-04").unwrap();
        let mut a = perfect(q);
        a.cited.insert("https://w3id.org/wine-linkml/merlot".into());
        let report = graded(&b, vec![a]);
        let c = check(&report.results[2], "citations-grounded");
        assert!(!c.passed);
        assert_eq!(c.missing, vec!["https://w3id.org/wine-linkml/merlot"]);
    }

    #[test]
    fn attribution_needs_the_expected_citations() {
        let b = wine();
        let q = b.question("cq-04").unwrap();
        let mut a = perfect(q);
        a.cited.clear();
        let report = graded(&b, vec![a]);
        let r = &report.results[2];
        assert!(check(r, "anchors-reached").passed);
        let c = check(r, "attribution");
        assert!(!c.passed);
        assert_eq!(
            c.missing,
            vec!["https://w3id.org/wine-linkml/pr-cabsauv-grilledmeat"]
        );
    }

    #[test]
    fn attribution_with_no_citations_fails_even_if_the_key_asks_for_none() {
        // Wine never asks for an empty citation set, so build a question that does.
        let b = wine();
        let mut q = b.question("cq-04").unwrap().clone();
        q.expected_citations.clear();
        let a = perfect(&q);
        assert!(a.cited.is_empty());
        let r = grade_question(&q, Some(&a));
        let c = check(&r, "attribution");
        assert!(!c.passed);
        assert!(c.detail.contains("must cite"));
    }

    #[test]
    fn synthesis_with_citations_runs_citations_rest() {
        let b = wine();
        let q = b.question("cq-05").unwrap();
        let mut a = perfect(q);
        a.cited.clear();
        let report = graded(&b, vec![a]);
        let r = &report.results[3];
        assert!(check(r, "synthesis").passed);
        let c = check(r, "citations-rest");
        assert!(!c.passed);
        assert_eq!(c.missing.len(), 3);
        assert_eq!(r.outcome, Outcome::Wrong);
    }

    #[test]
    fn closed_world_negative_needs_the_assertion() {
        let b = wine();
        let q = b.question("cq-03").unwrap();
        let mut a = perfect(q);
        a.asserts_absence = false;
        let report = graded(&b, vec![a]);
        let r = &report.results[1];
        assert!(check(r, "anchors-reached").passed);
        let c = check(r, "closed-world-negative");
        assert!(!c.passed);
        assert!(c.detail.contains("does not assert absence"));
    }

    #[test]
    fn absence_asserted_on_a_positive_question_fails() {
        let b = wine();
        let q = b.question("cq-04").unwrap();
        let mut a = perfect(q);
        a.asserts_absence = true;
        let r = grade_question(q, Some(&a));
        assert!(!check(&r, "no-false-absence").passed, "{r:?}");
        assert_eq!(r.outcome, Outcome::Wrong, "{r:?}");
    }

    #[test]
    fn closed_world_negative_asserted_without_reaching_the_anchors_fails() {
        let b = wine();
        let q = b.question("cq-03").unwrap();
        let mut a = Attempt::empty(&q.id);
        a.asserts_absence = true;
        let report = graded(&b, vec![a]);
        let c = check(&report.results[1], "closed-world-negative");
        assert!(!c.passed);
        assert_eq!(c.missing.len(), 2);
    }

    #[test]
    fn both_kinds_must_pass_on_a_two_kind_question() {
        let b = wine();
        let q = b.question("cq-07").unwrap();
        let mut a = perfect(q);
        a.cited
            .remove("https://w3id.org/wine-linkml/va-napa-zin-2017");
        let report = graded(&b, vec![a]);
        let r = &report.results[5];
        assert!(check(r, "comparison").passed);
        assert!(!check(r, "attribution").passed);
        assert_eq!(r.outcome, Outcome::Wrong);
    }

    #[test]
    fn precision_reports_noise_without_failing() {
        let b = wine();
        let q = b.question("cq-02").unwrap();
        let mut a = perfect(q);
        for i in 0..3 {
            a.retrieved
                .insert(format!("https://w3id.org/wine-linkml/noise-{i}"));
        }
        let report = graded(&b, vec![a]);
        let r = &report.results[0];
        assert_eq!(r.outcome, Outcome::Passed);
        assert_eq!(r.recall, 1.0);
        assert!((r.precision - 0.25).abs() < 1e-9);
        assert_eq!(r.retrieved, 4);
    }

    #[test]
    fn unknown_attempts_are_listed_not_graded() {
        let b = wine();
        let mut attempts: Vec<Attempt> = b.questions.iter().map(perfect).collect();
        attempts.push(Attempt::empty("cq-99"));
        let report = graded(&b, attempts);
        assert!(report.passed);
        assert_eq!(report.unknown_attempts, vec!["cq-99"]);
    }

    #[test]
    fn wrong_benchmark_or_format_is_an_error() {
        let b = wine();
        let mut t = trace_with(&b, Vec::new());
        t.benchmark = "other".into();
        assert!(matches!(
            grade(&b, &t, Require::Answered),
            Err(Error::Trace(_))
        ));
        let mut t = trace_with(&b, Vec::new());
        t.trace_format = "9.9".into();
        assert!(matches!(
            grade(&b, &t, Require::Answered),
            Err(Error::Trace(_))
        ));
    }

    #[test]
    fn a_trace_over_another_dataset_or_version_is_an_error() {
        // Either one moved is enough: the same dataset at another version
        // is a different set of records. The message names both graphs, so
        // the reader sees what the trace said as well as what was wanted.
        let b = wine();
        let target = b.target_graph();
        let moved_dataset = Graph {
            dataset: "other".into(),
            ..target.clone()
        };
        let moved_version = Graph {
            dataset_version: "0.0.1".into(),
            ..target.clone()
        };
        for (moved, value) in [(moved_dataset, "other"), (moved_version, "0.0.1")] {
            let mut t = trace_with(&b, Vec::new());
            t.graph = moved;
            match grade(&b, &t, Require::Answered) {
                Err(Error::Trace(m)) => assert!(
                    [value, &target.dataset, &target.dataset_version]
                        .iter()
                        .all(|v| m.contains(&format!("{v:?}"))),
                    "{value}: {m}"
                ),
                other => panic!("{value}: expected Error::Trace, got {other:?}"),
            }
        }
    }

    #[test]
    fn citations_rest_runs_where_citations_are_expected_without_attribution() {
        let b = wine();
        let attempts = b.questions.iter().map(perfect).collect();
        let report = graded(&b, attempts);
        for (q, r) in b.questions.iter().zip(&report.results) {
            let expected = !q.expected_citations.is_empty()
                && !q.answer_kinds.contains(&AnswerKind::Attribution);
            let ran = r.checks.iter().any(|c| c.name == "citations-rest");
            assert_eq!(ran, expected, "{}", q.id);
        }
    }

    #[test]
    fn the_label_column_is_as_wide_as_the_longest_label() {
        let b = wine();
        let attempts = b.questions.iter().map(perfect).collect();
        let report = graded(&b, attempts);
        let widest = report
            .results
            .iter()
            .map(|r| format!("{} {}", r.question, kinds_label(&r.answer_kinds)).len())
            .max()
            .unwrap();
        let text = report.to_text();
        let verdicts: Vec<usize> = text
            .lines()
            .filter(|l| l.starts_with("  cq-"))
            .map(|l| l.find("  ok  ").unwrap() + 2)
            .collect();
        assert_eq!(verdicts, vec![2 + widest + 2; b.questions.len()], "{text}");
    }

    #[test]
    fn only_failed_checks_are_listed() {
        let b = wine();
        let text = graded(&b, abstaining(&b)).to_text();
        assert!(text.contains("closed-world-negative: "), "{text}");
        assert!(!text.contains("attempted: "), "{text}");
    }

    #[test]
    fn a_full_trace_says_nothing_about_unattempted() {
        let b = wine();
        let attempts = b.questions.iter().map(perfect).collect();
        let text = graded(&b, attempts).to_text();
        assert!(
            text.ends_with("6 questions: 6 passed, 0 unanswered, 0 wrong\n"),
            "{text}"
        );
    }

    #[test]
    fn a_pre_release_grades_as_its_release() {
        // A graph on `main` says `0.2.1-dev` while the key pins `0.2.1`,
        // and the other way round; either is the same release.
        let b = wine();
        let pinned = b.target_dataset_version.clone();
        let mut t = trace_with(&b, Vec::new());
        t.graph.dataset_version = format!("{pinned}-dev");
        assert!(
            grade(&b, &t, Require::Answered).is_ok(),
            "trace {pinned}-dev, key {pinned}"
        );
        let mut dev = b.clone();
        dev.target_dataset_version = format!("{pinned}-dev");
        let t = trace_with(&b, Vec::new());
        assert!(
            grade(&dev, &t, Require::Answered).is_ok(),
            "trace {pinned}, key {pinned}-dev"
        );
    }

    #[test]
    fn only_a_dashed_suffix_on_three_numbers_is_a_pre_release() {
        for (version, expected) in [
            ("0.2.1-dev", "0.2.1"),
            ("0.2.1-alpha.1", "0.2.1"),
            ("10.20.30-rc-2", "10.20.30"),
            ("0.2.1", "0.2.1"),
            ("0.2.1dev", "0.2.1dev"),
            ("0.2.1-", "0.2.1-"),
            ("v0.2.1-dev", "v0.2.1-dev"),
            ("0.2-dev", "0.2-dev"),
            ("0.2.1.4-dev", "0.2.1.4-dev"),
            ("0..1-dev", "0..1-dev"),
            ("worked-example", "worked-example"),
        ] {
            assert_eq!(release(version), expected, "{version}");
        }
    }

    #[test]
    fn text_rendering_names_the_graph() {
        let b = wine();
        let report = graded(&b, Vec::new());
        let text = report.to_text();
        let header = text.lines().next().unwrap();
        assert!(
            header.contains(&b.target_dataset) && header.contains(&b.target_dataset_version),
            "{header}"
        );
    }

    #[test]
    fn text_rendering_names_failures() {
        let b = wine();
        let q = b.question("cq-03").unwrap();
        let mut a = perfect(q);
        a.asserts_absence = false;
        let report = graded(&b, vec![a]);
        let text = report.to_text();
        assert!(text.contains("cq-03 closed-world-negative"), "{text}");
        assert!(text.contains("UNATTEMPTED"), "{text}");
        assert!(
            text.contains("6 questions: 0 passed, 1 unanswered, 0 wrong, 5 unattempted"),
            "{text}"
        );
    }

    #[test]
    fn an_unanswered_negative_is_not_a_wrong_answer() {
        // Both fail the default requirement; the outcome tells them apart.
        let b = wine();
        let report = graded(&b, vec![silent_negative(&b), false_absence(&b)]);
        assert_eq!(result(&report, "cq-03").outcome, Outcome::Unanswered);
        assert_eq!(result(&report, "cq-04").outcome, Outcome::Wrong);
    }

    #[test]
    fn an_abstention_with_another_failure_is_wrong() {
        // Silence excuses only the negative's own check. `abstained &&
        // failed == 1` mutated to `>= 1` would call this unanswered.
        let b = wine();
        let mut a = silent_negative(&b);
        a.cited.insert("https://w3id.org/wine-linkml/merlot".into());
        let report = graded(&b, vec![a]);
        assert_eq!(result(&report, "cq-03").outcome, Outcome::Wrong);
    }

    #[test]
    fn a_silent_negative_whose_records_were_not_reached_is_wrong() {
        // Silence without the records is a retrieval failure, like a missed
        // anchor on a positive; only silence with them in hand abstains.
        let b = wine();
        let report = graded(&b, vec![Attempt::empty("cq-03")]);
        assert_eq!(result(&report, "cq-03").outcome, Outcome::Wrong);
    }

    #[test]
    fn the_summary_partitions_the_questions() {
        let b = wine();
        let fine = perfect(b.question("cq-02").unwrap());
        let report = graded(&b, vec![fine, silent_negative(&b), false_absence(&b)]);
        assert_eq!(
            report.summary,
            Summary {
                questions: 6,
                passed: 1,
                unanswered: 1,
                wrong: 1,
                unattempted: 3,
            }
        );
    }

    #[test]
    fn no_wrong_answers_tolerates_the_unanswered() {
        assert!(!Require::NoWrongAnswers.rejects(Outcome::Unanswered));
        assert!(Require::Answered.rejects(Outcome::Unanswered));
    }

    #[test]
    fn a_wrong_answer_fails_no_wrong_answers() {
        assert!(Require::NoWrongAnswers.rejects(Outcome::Wrong));
    }

    #[test]
    fn an_unattempted_question_fails_no_wrong_answers() {
        // Tolerating it would pass an empty trace.
        assert!(Require::NoWrongAnswers.rejects(Outcome::Unattempted));
    }

    #[test]
    fn a_passed_question_is_rejected_by_neither_requirement() {
        assert!(!Require::Answered.rejects(Outcome::Passed));
        assert!(!Require::NoWrongAnswers.rejects(Outcome::Passed));
    }

    #[test]
    fn passed_follows_the_requirement() {
        let b = wine();
        let t = trace_with(&b, abstaining(&b));
        let tolerant = grade(&b, &t, Require::NoWrongAnswers).unwrap();
        assert_eq!(tolerant.require, Require::NoWrongAnswers);
        assert!(tolerant.passed, "{}", tolerant.to_text());
        let strict = grade(&b, &t, Require::Answered).unwrap();
        assert!(!strict.passed, "{}", strict.to_text());
    }

    #[test]
    fn text_rendering_names_the_requirement() {
        let b = wine();
        let report = grade(&b, &trace_with(&b, Vec::new()), Require::NoWrongAnswers).unwrap();
        let text = report.to_text();
        let header = text.lines().next().unwrap();
        assert!(header.ends_with("requiring no wrong answers"), "{header}");
    }

    #[test]
    fn the_verdict_column_shows_the_outcome() {
        let b = wine();
        let text = graded(&b, vec![silent_negative(&b), false_absence(&b)]).to_text();
        assert!(line(&text, "cq-03").contains("  UNANSWERED  "), "{text}");
        assert!(line(&text, "cq-04").contains("  WRONG  "), "{text}");
        assert!(line(&text, "cq-02").contains("  UNATTEMPTED  "), "{text}");
    }

    #[test]
    fn an_outcome_the_requirement_tolerates_is_in_lowercase() {
        // Capitals mark what failed the grade, and under no-wrong-answers
        // an unanswered negative did not.
        let b = wine();
        let t = trace_with(&b, vec![silent_negative(&b)]);
        let text = grade(&b, &t, Require::NoWrongAnswers).unwrap().to_text();
        assert!(line(&text, "cq-03").contains("  unanswered  "), "{text}");
        assert!(line(&text, "cq-02").contains("  UNATTEMPTED  "), "{text}");
    }

    /// The report line for one question.
    fn line<'a>(text: &'a str, id: &str) -> &'a str {
        text.lines()
            .find(|l| l.trim_start().starts_with(id))
            .unwrap_or_else(|| panic!("no line for {id} in {text}"))
    }
}
