//! Read a cqa benchmark from its published Turtle artifact.
//!
//! The artifact a consumer repository publishes (for wine,
//! `site/artifacts/wine-benchmark.ttl`) is self-contained: cqa's own
//! classes and properties plus one `cqa:Benchmark` and its
//! `cqa:CompetencyQuestionAnswer` records, with every anchor already
//! expanded to an absolute IRI into the target graph. Reading that file
//! rather than the YAML source means this crate never has to know how
//! short anchors expand.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use oxigraph::store::Store;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Serialize};

use crate::rdf::{load_turtle, select};
use crate::{Error, Result};

const CQA: &str = "https://w3id.org/cqa/";

/// The kinds of answering a question calls for, one per check.
///
/// Mirrors `cqa:AnswerKindEnum`. Every kind a question names is a separate
/// check, and all of them must pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnswerKind {
    /// A value read from a single record.
    Lookup,
    /// The correct answer is that the graph holds no such record.
    ClosedWorldNegative,
    /// Two or more records set against each other on a shared slot.
    Comparison,
    /// A pattern across several records that no single record states.
    Synthesis,
    /// An answer that must carry the source it rests on.
    Attribution,
}

impl AnswerKind {
    /// The permissible-value name, as it appears in cqa data.
    pub fn as_str(self) -> &'static str {
        match self {
            AnswerKind::Lookup => "lookup",
            AnswerKind::ClosedWorldNegative => "closed-world-negative",
            AnswerKind::Comparison => "comparison",
            AnswerKind::Synthesis => "synthesis",
            AnswerKind::Attribution => "attribution",
        }
    }

    fn from_iri(iri: &str) -> Option<Self> {
        let name = iri.rsplit('/').next()?;
        let de: serde::de::value::StrDeserializer<serde::de::value::Error> =
            name.into_deserializer();
        AnswerKind::deserialize(de).ok()
    }
}

/// One competency question with the specification of its correct answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// The short identifier (`cqa:id`), which a trace refers to.
    pub id: String,
    /// The record's IRI in the benchmark graph.
    pub iri: String,
    /// The question, in the words it would be asked in.
    pub question: String,
    /// The correct answer in prose. Never read by the deterministic tier.
    pub ground_truth: String,
    /// The checks this question calls for.
    pub answer_kinds: BTreeSet<AnswerKind>,
    /// The records a correct answer must retrieve, as IRIs into the target graph.
    pub expected_anchors: BTreeSet<String>,
    /// The records a correct answer must rest on. Always among the anchors.
    pub expected_citations: BTreeSet<String>,
    /// The anchors that no single record joins, for a closed-world negative.
    pub unconnected_anchors: BTreeSet<String>,
    /// The class a joining record would belong to, if the key narrows the claim.
    pub connecting_class: Option<String>,
}

/// A named set of questions, evaluated against one knowledge graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Benchmark {
    /// The short identifier (`cqa:id`), which a trace names.
    pub id: String,
    /// The benchmark's IRI.
    pub iri: String,
    /// A short human-readable label, if the benchmark carries one.
    pub name: Option<String>,
    /// The schema the target graph's records conform to.
    pub target_schema: String,
    /// The version of that schema the anchors were written against.
    pub target_schema_version: String,
    /// Which instance graph the anchors resolve in.
    pub target_dataset: String,
    /// The version of that dataset.
    pub target_dataset_version: String,
    /// The questions, in `cqa:id` order.
    pub questions: Vec<Question>,
}

impl Benchmark {
    /// Read the benchmark from a Turtle file.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_turtle(&text)
    }

    /// Read the benchmark from Turtle text.
    ///
    /// The text must hold exactly one `cqa:Benchmark`.
    pub fn from_turtle(turtle: &str) -> Result<Self> {
        Self::from_store(&load_turtle(turtle)?)
    }

    fn from_store(store: &Store) -> Result<Self> {
        let benchmarks = select(
            store,
            &format!(
                "PREFIX cqa: <{CQA}>
                 SELECT ?b ?id ?name ?ts ?tsv ?td ?tdv WHERE {{
                   ?b a cqa:Benchmark ; cqa:id ?id ;
                      cqa:target_schema ?ts ; cqa:target_schema_version ?tsv ;
                      cqa:target_dataset ?td ; cqa:target_dataset_version ?tdv .
                   OPTIONAL {{ ?b cqa:name ?name }}
                   FILTER(isIRI(?b))
                 }}"
            ),
        )?;
        // A slot with two values (a name in two languages) gives one row
        // per combination, so count benchmarks, not rows.
        let found: BTreeSet<String> = benchmarks
            .iter()
            .map(|r| r.require("b"))
            .collect::<Result<_>>()?;
        let row = match (found.len(), benchmarks.first()) {
            (1, Some(row)) => row,
            (0, _) => {
                return Err(Error::Benchmark(
                    "no cqa:Benchmark with an IRI and its four target pins found".into(),
                ));
            }
            (n, _) => {
                return Err(Error::Benchmark(format!(
                    "expected one cqa:Benchmark, found {n}"
                )));
            }
        };
        let iri = row.require("b")?;
        for (var, slot) in [
            ("id", "id"),
            ("ts", "target_schema"),
            ("tsv", "target_schema_version"),
            ("td", "target_dataset"),
            ("tdv", "target_dataset_version"),
        ] {
            if benchmarks.iter().any(|r| r.get(var) != row.get(var)) {
                return Err(Error::Benchmark(format!(
                    "<{iri}> has more than one {slot}"
                )));
            }
        }

        let mut questions: BTreeMap<String, Question> = BTreeMap::new();
        for q in select(
            store,
            &format!(
                "PREFIX cqa: <{CQA}>
                 SELECT ?q ?id ?question ?gt WHERE {{
                   <{iri}> cqa:competency_question_answers ?q .
                   OPTIONAL {{ ?q cqa:id ?id }}
                   OPTIONAL {{ ?q cqa:question ?question }}
                   OPTIONAL {{ ?q cqa:ground_truth ?gt }}
                 }}"
            ),
        )? {
            let q_iri = q.require("q")?;
            let id = q
                .get("id")
                .ok_or_else(|| Error::Benchmark(format!("<{q_iri}>: no id")))?;
            let slot = |var: &str, name: &str| {
                q.get(var)
                    .ok_or_else(|| Error::Benchmark(format!("{id}: no {name}")))
            };
            let question = slot("question", "question")?;
            let ground_truth = slot("gt", "ground_truth")?;
            if questions.contains_key(&q_iri) {
                return Err(Error::Benchmark(format!(
                    "{id}: more than one id, question or ground_truth"
                )));
            }
            questions.insert(
                q_iri.clone(),
                Question {
                    id,
                    iri: q_iri,
                    question,
                    ground_truth,
                    answer_kinds: BTreeSet::new(),
                    expected_anchors: BTreeSet::new(),
                    expected_citations: BTreeSet::new(),
                    unconnected_anchors: BTreeSet::new(),
                    connecting_class: None,
                },
            );
        }
        if questions.is_empty() {
            return Err(Error::Benchmark(format!(
                "benchmark <{iri}> holds no competency question answers"
            )));
        }

        for (slot, pick) in [
            ("answer_kind", Slot::Kind),
            ("expected_anchors", Slot::Anchors),
            ("expected_citations", Slot::Citations),
            ("unconnected_anchors", Slot::Unconnected),
            ("connecting_class", Slot::ConnectingClass),
        ] {
            for row in select(
                store,
                &format!(
                    "PREFIX cqa: <{CQA}>
                     SELECT ?q ?v WHERE {{
                       <{iri}> cqa:competency_question_answers ?q .
                       ?q cqa:{slot} ?v .
                     }}"
                ),
            )? {
                let q_iri = row.require("q")?;
                let v = row.require("v")?;
                let Some(question) = questions.get_mut(&q_iri) else {
                    continue;
                };
                match pick {
                    Slot::Kind => {
                        let kind = AnswerKind::from_iri(&v).ok_or_else(|| {
                            Error::Benchmark(format!("{}: unknown answer kind <{v}>", question.id))
                        })?;
                        question.answer_kinds.insert(kind);
                    }
                    Slot::Anchors => {
                        question.expected_anchors.insert(v);
                    }
                    Slot::Citations => {
                        question.expected_citations.insert(v);
                    }
                    Slot::Unconnected => {
                        question.unconnected_anchors.insert(v);
                    }
                    Slot::ConnectingClass => question.connecting_class = Some(v),
                }
            }
        }

        // cqa declares citations and unconnected anchors as sub-properties
        // of `expected_anchors`. The published data lists each explicitly,
        // but the store does no inference, so fold them in here.
        for question in questions.values_mut() {
            let extra: Vec<String> = question
                .expected_citations
                .iter()
                .chain(question.unconnected_anchors.iter())
                .cloned()
                .collect();
            question.expected_anchors.extend(extra);
            if question.answer_kinds.is_empty() {
                return Err(Error::Benchmark(format!("{}: no answer_kind", question.id)));
            }
            if question.expected_anchors.is_empty() {
                return Err(Error::Benchmark(format!(
                    "{}: no expected_anchors",
                    question.id
                )));
            }
            if question
                .answer_kinds
                .contains(&AnswerKind::ClosedWorldNegative)
                && question.unconnected_anchors.is_empty()
            {
                return Err(Error::Benchmark(format!(
                    "{}: closed-world-negative with no unconnected_anchors",
                    question.id
                )));
            }
        }

        let mut questions: Vec<Question> = questions.into_values().collect();
        questions.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(pair) = questions.windows(2).find(|w| w[0].id == w[1].id) {
            return Err(Error::Benchmark(format!(
                "{}: more than one question has this id",
                pair[0].id
            )));
        }

        Ok(Benchmark {
            id: row.require("id")?,
            iri,
            name: row.get("name"),
            target_schema: row.require("ts")?,
            target_schema_version: row.require("tsv")?,
            target_dataset: row.require("td")?,
            target_dataset_version: row.require("tdv")?,
            questions,
        })
    }

    /// Look a question up by its `cqa:id`.
    pub fn question(&self, id: &str) -> Option<&Question> {
        self.questions.iter().find(|q| q.id == id)
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Kind,
    Anchors,
    Citations,
    Unconnected,
    ConnectingClass,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The generated benchmark fixture. Panics with the fix if it is missing.
    pub(crate) fn wine_benchmark() -> String {
        fixture("wine-benchmark.ttl")
    }

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}. The fixtures are generated from sibling checkouts of \
                 ontology-authoring-template and cqa-schema: run `panschema generate --strict` \
                 in the repo root first.",
                path.display()
            )
        })
    }

    #[test]
    fn reads_the_wine_benchmark() {
        let b = Benchmark::from_turtle(&wine_benchmark()).unwrap();
        assert_eq!(b.id, "wine-benchmark");
        assert_eq!(b.name.as_deref(), Some("Wine competency questions"));
        assert_eq!(b.target_schema, "https://w3id.org/wine-linkml/");
        assert_eq!(b.target_schema_version, "0.2.0");
        assert_eq!(b.target_dataset, "worked-example");
        assert_eq!(b.target_dataset_version, "0.2.0");
        let ids: Vec<&str> = b.questions.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, ["cq-02", "cq-03", "cq-04", "cq-05", "cq-06", "cq-07"]);
    }

    #[test]
    fn closed_world_negative_carries_its_claim() {
        let b = Benchmark::from_turtle(&wine_benchmark()).unwrap();
        let q = b.question("cq-03").unwrap();
        assert_eq!(
            q.answer_kinds,
            BTreeSet::from([AnswerKind::ClosedWorldNegative])
        );
        assert_eq!(q.unconnected_anchors.len(), 2);
        assert!(q.unconnected_anchors.is_subset(&q.expected_anchors));
        assert_eq!(
            q.connecting_class.as_deref(),
            Some("https://w3id.org/wine-linkml/PairingRecommendation")
        );
    }

    #[test]
    fn two_kinds_on_one_question() {
        let b = Benchmark::from_turtle(&wine_benchmark()).unwrap();
        let q = b.question("cq-07").unwrap();
        assert_eq!(
            q.answer_kinds,
            BTreeSet::from([AnswerKind::Attribution, AnswerKind::Comparison])
        );
        assert_eq!(q.expected_citations.len(), 2);
        assert!(q.expected_citations.is_subset(&q.expected_anchors));
        assert_eq!(q.expected_anchors.len(), 4);
    }

    #[test]
    fn rejects_a_file_without_a_benchmark() {
        let err = Benchmark::from_turtle("@prefix ex: <http://example.org/> . ex:a ex:b ex:c .")
            .unwrap_err();
        assert!(matches!(err, Error::Benchmark(_)), "{err}");
    }

    /// A one-benchmark file: the four pins, `extra` triples on the
    /// benchmark, and `questions` as Turtle statements under `ex:`.
    fn benchmark_with(extra: &str, questions: &str) -> String {
        format!(
            "@prefix cqa: <{CQA}> . @prefix ex: <http://example.org/> .
             ex:b a cqa:Benchmark ; cqa:id \"b\" ;
               cqa:target_schema \"s\" ; cqa:target_schema_version \"1\" ;
               cqa:target_dataset \"d\" ; cqa:target_dataset_version \"1\" ;
               cqa:competency_question_answers ex:q1, ex:q2 {extra} .
             {questions}"
        )
    }

    /// What makes a question gradable beyond its id, text and ground truth.
    const GRADABLE: &str = "cqa:answer_kind <https://w3id.org/cqa/AnswerKindEnum/lookup> ; \
                            cqa:expected_anchors ex:r1";

    const TWO_QUESTIONS: &str = "
        ex:q1 cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ;
          cqa:answer_kind <https://w3id.org/cqa/AnswerKindEnum/lookup> ; cqa:expected_anchors ex:r1 .
        ex:q2 cqa:id \"q2\" ; cqa:question \"two?\" ; cqa:ground_truth \"Two.\" ;
          cqa:answer_kind <https://w3id.org/cqa/AnswerKindEnum/lookup> ; cqa:expected_anchors ex:r1 .";

    #[test]
    fn a_benchmark_with_two_names_is_still_one_benchmark() {
        let b = Benchmark::from_turtle(&benchmark_with(
            "; cqa:name \"Wine\"@en, \"Vins\"@fr",
            TWO_QUESTIONS,
        ))
        .unwrap();
        assert_eq!(b.id, "b");
    }

    #[test]
    fn two_benchmarks_in_one_file_are_an_error() {
        let ttl = format!(
            "{}\nex:c a cqa:Benchmark ; cqa:id \"c\" ;
               cqa:target_schema \"s\" ; cqa:target_schema_version \"1\" ;
               cqa:target_dataset \"d\" ; cqa:target_dataset_version \"1\" .",
            benchmark_with("", TWO_QUESTIONS)
        );
        let err = Benchmark::from_turtle(&ttl).unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m) if m.contains("found 2")),
            "{err}"
        );
    }

    #[test]
    fn a_question_without_its_ground_truth_is_an_error_not_dropped() {
        let err = Benchmark::from_turtle(&benchmark_with(
            "",
            &format!(
                "ex:q1 cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ; {GRADABLE} .
                 ex:q2 cqa:id \"q2\" ; cqa:question \"two?\" ; {GRADABLE} ."
            ),
        ))
        .unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m) if m.contains("q2") && m.contains("ground_truth")),
            "{err}"
        );
    }

    #[test]
    fn two_questions_sharing_an_id_are_an_error() {
        let err = Benchmark::from_turtle(&benchmark_with(
            "",
            &format!(
                "ex:q1 cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ; {GRADABLE} .
                 ex:q2 cqa:id \"q1\" ; cqa:question \"two?\" ; cqa:ground_truth \"Two.\" ; {GRADABLE} ."
            ),
        ))
        .unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m) if m.contains("q1") && m.contains("more than one")),
            "{err}"
        );
    }

    #[test]
    fn a_closed_world_negative_without_unconnected_anchors_is_an_error() {
        // Without them the reached check passes on zero anchors, and the
        // question grades on `asserts_absence` alone.
        let questions = format!(
            "{TWO_QUESTIONS}
             ex:q2 cqa:answer_kind <{CQA}AnswerKindEnum/closed-world-negative> ."
        );
        let err = Benchmark::from_turtle(&benchmark_with("", &questions)).unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m) if m.contains("q2") && m.contains("unconnected_anchors")),
            "{err}"
        );
    }

    #[test]
    fn rejects_unparseable_turtle() {
        let err = Benchmark::from_turtle("this is not turtle").unwrap_err();
        assert!(matches!(err, Error::Rdf(_)), "{err}");
    }

    #[test]
    fn answer_kind_round_trips_through_its_name() {
        for kind in [
            AnswerKind::Lookup,
            AnswerKind::ClosedWorldNegative,
            AnswerKind::Comparison,
            AnswerKind::Synthesis,
            AnswerKind::Attribution,
        ] {
            let iri = format!("{CQA}AnswerKindEnum/{}", kind.as_str());
            assert_eq!(AnswerKind::from_iri(&iri), Some(kind));
        }
        assert_eq!(
            AnswerKind::from_iri("https://example.org/no-such-kind"),
            None
        );
    }
}
