//! Grade a system's retrieval trace against a cqa competency-question
//! benchmark.
//!
//! The [cqa](https://w3id.org/cqa/) vocabulary pairs a competency question
//! with the specification of its correct answer: the records a correct
//! answer must reach (`expected_anchors`), the records it must rest on
//! (`expected_citations`), the kinds of answering it calls for
//! (`answer_kind`), and the answer in prose (`ground_truth`). A benchmark
//! is published by the graph's own repository as a self-contained Turtle
//! file. This crate reads that file, reads a [`Trace`] of what some system
//! did with each question, and scores one against the other.
//!
//! The trace format is the crate's contract with the systems it grades.
//! A system emits a trace; nothing about how it produced the trace crosses
//! into the grader. See [`trace`] for the format and [`grade`] for the
//! checks.
//!
//! Only the deterministic tier lives here today: anchors reached,
//! citations grounded and rested on, absence asserted where, and only
//! where, the key claims one. Judging a prose `answer` against `ground_truth` needs a
//! judge and stays out of CI; the [`Attempt::answer`] slot is where such a
//! judge will read from.
//!
//! [`baseline`] is a deliberately naive retriever (lexical entity linking,
//! then bounded one-hop SPARQL lookups) so that a benchmark's own CI has a
//! student to grade. It exists to show that every anchor can be reached by
//! lookup, not to answer well. It never says "none on record", so it leaves
//! every closed-world negative unanswered, which a grade held to
//! [`Require::NoWrongAnswers`] tolerates.

pub mod baseline;
pub mod benchmark;
pub mod grade;
mod rdf;
pub mod trace;

pub use baseline::Baseline;
pub use benchmark::{AnswerKind, Benchmark, Question};
pub use grade::{Check, Outcome, QuestionResult, Report, Require, Summary, grade};
pub use trace::{Attempt, Graph, System, TRACE_FORMAT, Trace};

/// Errors this crate reports.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The RDF store could not be created, loaded, or queried.
    #[error("rdf: {0}")]
    Rdf(String),
    /// The benchmark file parsed as RDF but does not describe one benchmark.
    #[error("benchmark: {0}")]
    Benchmark(String),
    /// The trace does not fit the benchmark it is graded against.
    #[error("trace: {0}")]
    Trace(String),
    /// Reading or writing a file failed. The cause is the error's source,
    /// so it is not repeated in this message.
    #[error("io")]
    Io(#[from] std::io::Error),
    /// A trace or report could not be (de)serialized. The cause is the
    /// error's source, so it is not repeated in this message.
    #[error("json")]
    Json(#[from] serde_json::Error),
    /// One of several input files failed to read or load. The cause is
    /// the error's source, so it is not repeated in this message.
    #[error("{}", path.display())]
    File {
        /// The file that failed.
        path: std::path::PathBuf,
        /// Why it failed.
        source: Box<Error>,
    },
}

/// `Result` with this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn rdf(e: impl std::fmt::Display) -> Self {
        Error::Rdf(e.to_string())
    }
}
