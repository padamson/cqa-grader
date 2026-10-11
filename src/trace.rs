//! The trace format: what a system did with each question.
//!
//! A trace is the only thing a graded system hands this crate. It names
//! the benchmark, names the system, and for each question lists the
//! records retrieval reached and the records the answer rests on, as IRIs
//! into the target graph. Nothing about how the system found them is
//! recorded, which is the point: a private retriever can be graded by a
//! public grader without any of its code or method crossing over.
//!
//! The JSON Schema for the format is in `schemas/trace.schema.json` and
//! printed by `cqa-grader trace-schema`.

use std::collections::BTreeSet;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// The trace format version this crate reads and writes.
pub const TRACE_FORMAT: &str = "0.1";

/// The system that produced a trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct System {
    /// A name for the system under test.
    pub name: String,
    /// Its version, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Anything else worth recording about this run (a configuration, a
    /// commit, a date). Free text; never read by the grader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// What a system did with one question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    /// The question's `cqa:id` in the benchmark (for wine, `cq-03`).
    pub question: String,
    /// Every record retrieval reached, as IRIs into the target graph.
    #[serde(default)]
    pub retrieved: BTreeSet<String>,
    /// The records the answer rests on. Must be among `retrieved`.
    #[serde(default)]
    pub cited: BTreeSet<String>,
    /// Whether the system reports that the graph holds no record joining
    /// the entities the question names. A closed-world negative is graded
    /// on this: reaching the anchors and then saying "none on record" is a
    /// different outcome from reaching them and saying nothing.
    #[serde(default)]
    pub asserts_absence: bool,
    /// The prose answer, if the system wrote one. The deterministic tier
    /// never reads it; a judge scoring it against `ground_truth` would.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

impl Attempt {
    /// An attempt that reached nothing and said nothing.
    pub fn empty(question: impl Into<String>) -> Self {
        Attempt {
            question: question.into(),
            retrieved: BTreeSet::new(),
            cited: BTreeSet::new(),
            asserts_absence: false,
            answer: None,
        }
    }
}

/// A system's attempts at every question in one benchmark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Trace {
    /// The trace format version. Must be [`TRACE_FORMAT`].
    pub trace_format: String,
    /// The `cqa:id` of the benchmark these attempts answer (for wine,
    /// `wine-benchmark`).
    pub benchmark: String,
    /// The system under test.
    pub system: System,
    /// One attempt per question. A question with no attempt fails.
    pub attempts: Vec<Attempt>,
}

impl Trace {
    /// A trace with no attempts yet.
    pub fn new(benchmark: impl Into<String>, system: System) -> Self {
        Trace {
            trace_format: TRACE_FORMAT.to_string(),
            benchmark: benchmark.into(),
            system,
            attempts: Vec::new(),
        }
    }

    /// Read a trace from a JSON file.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text)
    }

    /// Read a trace from JSON text, and [`check`](Self::check) it.
    ///
    /// The format version is read before the rest, so a trace in a newer
    /// format is reported as that rather than as a field this one lacks.
    pub fn from_json(json: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        if let Some(format) = value.get("trace_format").and_then(|v| v.as_str()) {
            check_format(format)?;
        }
        let trace: Trace = serde_json::from_value(value)?;
        trace.check()?;
        Ok(trace)
    }

    /// The format version is this crate's, and no question is attempted
    /// twice.
    pub fn check(&self) -> Result<()> {
        check_format(&self.trace_format)?;
        let mut seen = BTreeSet::new();
        match self
            .attempts
            .iter()
            .find(|a| !seen.insert(a.question.as_str()))
        {
            Some(dup) => Err(Error::Trace(format!(
                "{}: more than one attempt at this question",
                dup.question
            ))),
            None => Ok(()),
        }
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Write to a JSON file.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        write_json(path, self)
    }

    /// The attempt at a question, if the trace holds one.
    pub fn attempt(&self, question_id: &str) -> Option<&Attempt> {
        self.attempts.iter().find(|a| a.question == question_id)
    }

    /// The JSON Schema for this format, as pretty-printed JSON.
    pub fn json_schema() -> Result<String> {
        let schema = schemars::schema_for!(Trace);
        Ok(serde_json::to_string_pretty(&schema)?)
    }
}

fn check_format(format: &str) -> Result<()> {
    if format == TRACE_FORMAT {
        Ok(())
    } else {
        Err(Error::Trace(format!(
            "trace_format {format:?} is not {TRACE_FORMAT:?}"
        )))
    }
}

/// Pretty-printed JSON with a trailing newline.
pub(crate) fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_finds_by_question_id() {
        let mut trace = Trace::new(
            "b",
            System {
                name: "s".into(),
                version: None,
                notes: None,
            },
        );
        trace.attempts = vec![Attempt::empty("q1"), Attempt::empty("q2")];
        assert_eq!(trace.attempt("q2").map(|a| a.question.as_str()), Some("q2"));
    }

    #[test]
    fn a_newer_format_is_reported_as_one() {
        // Its new field would otherwise fail first, as an unknown field.
        let json = r#"{
            "trace_format": "9.0",
            "benchmark": "b",
            "system": {"name": "s"},
            "attempts": [],
            "budget": 10
        }"#;
        let err = Trace::from_json(json).unwrap_err();
        assert!(
            matches!(&err, Error::Trace(m) if m.contains("9.0")),
            "{err}"
        );
    }

    #[test]
    fn a_misspelled_field_is_rejected_not_defaulted() {
        // `asserts_absense` would otherwise parse as `asserts_absence: false`
        // and fail a closed-world negative with no hint why.
        let json = r#"{
            "trace_format": "0.1",
            "benchmark": "b",
            "system": {"name": "s"},
            "attempts": [{"question": "q", "asserts_absense": true}]
        }"#;
        let err = Trace::from_json(json).unwrap_err();
        assert!(
            matches!(&err, Error::Json(e) if e.to_string().contains("asserts_absense")),
            "{err}"
        );
    }

    #[test]
    fn round_trips_through_json() {
        let mut trace = Trace::new(
            "wine-benchmark",
            System {
                name: "test".into(),
                version: Some("0".into()),
                notes: None,
            },
        );
        let mut attempt = Attempt::empty("cq-03");
        attempt
            .retrieved
            .insert("https://w3id.org/wine-linkml/seafood-dish".into());
        attempt.asserts_absence = true;
        trace.attempts.push(attempt);
        let json = trace.to_json().unwrap();
        assert_eq!(Trace::from_json(&json).unwrap(), trace);
    }

    #[test]
    fn missing_optional_fields_default() {
        let json = r#"{
            "trace_format": "0.1",
            "benchmark": "b",
            "system": {"name": "s"},
            "attempts": [{"question": "q"}]
        }"#;
        let trace = Trace::from_json(json).unwrap();
        let attempt = &trace.attempts[0];
        assert!(attempt.retrieved.is_empty());
        assert!(attempt.cited.is_empty());
        assert!(!attempt.asserts_absence);
        assert!(attempt.answer.is_none());
    }

    #[test]
    fn checked_in_schema_matches_the_type() {
        let on_disk = include_str!("../schemas/trace.schema.json");
        let generated = Trace::json_schema().unwrap();
        assert_eq!(
            on_disk.trim_end(),
            generated.trim_end(),
            "schemas/trace.schema.json is stale; run `cqa-grader trace-schema > schemas/trace.schema.json`"
        );
    }
}
