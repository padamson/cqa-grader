//! Read a cqa benchmark from its published Turtle or its LinkML source.
//!
//! The artifact a consumer repository publishes (for wine,
//! `site/artifacts/wine-benchmark.ttl`) is self-contained: cqa's own
//! classes and properties plus one `cqa:Benchmark` and its
//! `cqa:CompetencyQuestionAnswer` records, with every anchor already
//! expanded to an absolute IRI into the target graph.
//!
//! The source is the benchmark's YAML read against cqa's schema. Short
//! anchors are written there, and `panschema-model` expands them as
//! panschema does when it writes the Turtle, so this crate still never has
//! to know how.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use oxigraph::store::Store;
use panschema_model::LoadedDataset;
use panschema_model::instances::{
    Instance, InstanceValue, ScalarValue, instance_iri_string, scalar_to_display,
};
use panschema_model::linkml_resolve::expand_curie_or_verbatim;
use panschema_model::yaml_reader::YamlReader;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Serialize};

use crate::rdf::{load_turtle, select};
use crate::trace::Graph;
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

    /// The kind a value IRI under `cqa:AnswerKindEnum` names, minted as
    /// `{enum}#{value}` (as LinkML's gen-owl does) or, by panschema's 0.4.0
    /// release, `{enum}/{value}`.
    fn from_iri(iri: &str) -> Option<Self> {
        let name = iri
            .strip_prefix(CQA)?
            .strip_prefix("AnswerKindEnum")?
            .strip_prefix(['#', '/'])?;
        Self::from_name(name)
    }

    /// The kind a permissible-value name names.
    fn from_name(name: &str) -> Option<Self> {
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

    /// Read the benchmark from its LinkML source: `data`, a cqa dataset
    /// holding exactly one `Benchmark`, read against `schema`, cqa's schema
    /// as one file with its imports merged.
    ///
    /// A load that warns, leaves an anchor unexpanded, or reads past a field
    /// or entry it could not place is refused, since grading against part
    /// of a key passes answers the whole key would fail.
    pub fn from_yaml(schema: impl AsRef<Path>, data: impl AsRef<Path>) -> Result<Self> {
        let loaded =
            panschema_model::load_dataset(schema.as_ref(), data.as_ref(), &YamlReader::new())
                .map_err(|failure| {
                    let mut message = failure.to_string();
                    for warning in &failure.warnings {
                        message.push_str("; ");
                        message.push_str(warning);
                    }
                    Error::Benchmark(message)
                })?;
        Self::from_loaded(&loaded)
    }

    fn from_loaded(loaded: &LoadedDataset) -> Result<Self> {
        let set = &loaded.instances;
        let unread: Vec<String> = loaded
            .warnings
            .iter()
            .cloned()
            .chain(set.expansion_gaps.iter().map(|g| {
                format!(
                    "{}: `{}` did not expand against `{}`",
                    g.record, g.slot, g.base_slot
                )
            }))
            .chain(
                set.duplicate_ids
                    .iter()
                    .map(|id| format!("{id}: more than one record has this id")),
            )
            .chain(
                set.undeclared_fields
                    .iter()
                    .map(|f| format!("{}: `{}` is not a slot of {}", f.record, f.field, f.class)),
            )
            .chain(
                set.unusable_collection_entries
                    .iter()
                    .map(|e| match &e.key {
                        Some(key) => format!("`{}`: entry `{key}` {}", e.slot, e.reason),
                        None => format!("`{}`: an entry {}", e.slot, e.reason),
                    }),
            )
            .chain(set.root_collision.iter().map(|id| {
                format!("{id}: the benchmark and a record share this id, so no Benchmark was read")
            }))
            .collect();
        if !unread.is_empty() {
            return Err(Error::Benchmark(unread.join("; ")));
        }

        let of_type = |class: &'static str| {
            set.instances
                .iter()
                .filter(move |i| i.types.iter().any(|t| t == class))
        };
        // cqa has one `tree_root`, so a dataset holds at most one Benchmark.
        let Some(benchmark) = of_type("Benchmark").next() else {
            return Err(Error::Benchmark("no Benchmark record found".into()));
        };

        let records: BTreeMap<&str, &Instance> = of_type("CompetencyQuestionAnswer")
            .map(|i| (i.id.as_str(), i))
            .collect();
        // Listing a question twice lists it once, as the Turtle's edges are a set.
        let listed: BTreeSet<String> = values(benchmark, "competency_question_answers")?
            .into_iter()
            .collect();
        let mut questions = Vec::new();
        for id in listed {
            let record = records
                .get(id.as_str())
                .ok_or_else(|| Error::Benchmark(format!("{id}: not a CompetencyQuestionAnswer")))?;
            // An anchor written as a CURIE expands as panschema expands it
            // when it writes the Turtle.
            let anchors = |name: &str| -> Result<BTreeSet<String>> {
                Ok(values(record, name)?
                    .iter()
                    .map(|v| expand_curie_or_verbatim(&loaded.schema, v))
                    .collect())
            };
            let answer_kinds = values(record, "answer_kind")?
                .into_iter()
                .map(|name| {
                    AnswerKind::from_name(&name).ok_or_else(|| {
                        Error::Benchmark(format!("{id}: unknown answer kind `{name}`"))
                    })
                })
                .collect::<Result<_>>()?;
            questions.push(Question {
                iri: instance_iri_string(&loaded.schema, record),
                question: required(record, "question")?,
                ground_truth: required(record, "ground_truth")?,
                answer_kinds,
                expected_anchors: anchors("expected_anchors")?,
                expected_citations: anchors("expected_citations")?,
                unconnected_anchors: anchors("unconnected_anchors")?,
                connecting_class: single(record, "connecting_class")?,
                id,
            });
        }

        Benchmark {
            id: required(benchmark, "id")?,
            name: single(benchmark, "name")?,
            target_schema: required(benchmark, "target_schema")?,
            target_schema_version: required(benchmark, "target_schema_version")?,
            target_dataset: required(benchmark, "target_dataset")?,
            target_dataset_version: required(benchmark, "target_dataset_version")?,
            iri: instance_iri_string(&loaded.schema, benchmark),
            questions,
        }
        .checked()
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

        Benchmark {
            id: row.require("id")?,
            iri,
            name: row.get("name"),
            target_schema: row.require("ts")?,
            target_schema_version: row.require("tsv")?,
            target_dataset: row.require("td")?,
            target_dataset_version: row.require("tdv")?,
            questions: questions.into_values().collect(),
        }
        .checked()
    }

    /// Fold the sub-property slots into the anchors, refuse a question no
    /// check could grade, and put the questions in `cqa:id` order. Both
    /// readers end here, so they refuse the same keys.
    fn checked(mut self) -> Result<Self> {
        if self.questions.is_empty() {
            return Err(Error::Benchmark(format!(
                "benchmark <{}> holds no competency question answers",
                self.iri
            )));
        }
        // cqa declares citations and unconnected anchors as sub-properties
        // of `expected_anchors`. The published data lists each explicitly,
        // but neither reader infers, so fold them in here.
        for question in &mut self.questions {
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

        // Only the Turtle can give two records one `cqa:id`; the model
        // refuses a repeated id before the YAML reader sees it.
        self.questions.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(pair) = self.questions.windows(2).find(|w| w[0].id == w[1].id) {
            return Err(Error::Benchmark(format!(
                "{}: more than one question has this id",
                pair[0].id
            )));
        }
        Ok(self)
    }

    /// The graph the anchors resolve in: the target dataset at its version.
    pub fn target_graph(&self) -> Graph {
        Graph {
            dataset: self.target_dataset.clone(),
            dataset_version: self.target_dataset_version.clone(),
        }
    }

    /// Look a question up by its `cqa:id`.
    pub fn question(&self, id: &str) -> Option<&Question> {
        self.questions.iter().find(|q| q.id == id)
    }
}

/// A record's values at `slot`, as text: a string literal, or the record a
/// reference names (an anchor, expanded to its IRI).
fn values(record: &Instance, slot: &str) -> Result<Vec<String>> {
    let Some(found) = record.slot_values.iter().find(|v| v.slot == slot) else {
        return Ok(Vec::new());
    };
    found
        .values
        .iter()
        .map(|value| match value {
            InstanceValue::Scalar(ScalarValue::String(s)) => Ok(s.clone()),
            InstanceValue::Reference { target, .. } => Ok(target.clone()),
            // panschema writes an unquoted number as a number, not as the
            // text it was meant to be, so `1.0` would read back as `1`.
            InstanceValue::Scalar(scalar) => Err(Error::Benchmark(format!(
                "{}: `{slot}` is {}, not text; quote it",
                record.id,
                scalar_to_display(scalar)
            ))),
            InstanceValue::Unexpected(kind) => Err(Error::Benchmark(format!(
                "{}: `{slot}` holds {kind}, not text",
                record.id
            ))),
        })
        .collect()
}

/// A record's one value at `slot`, which it must have.
fn required(record: &Instance, slot: &str) -> Result<String> {
    single(record, slot)?.ok_or_else(|| Error::Benchmark(format!("{}: no {slot}", record.id)))
}

/// A record's one value at `slot`, if it has one.
fn single(record: &Instance, slot: &str) -> Result<Option<String>> {
    let mut found = values(record, slot)?;
    if found.len() > 1 {
        return Err(Error::Benchmark(format!(
            "{}: more than one {slot}",
            record.id
        )));
    }
    Ok(found.pop())
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

    /// The committed benchmark fixture.
    pub(crate) fn wine_benchmark() -> String {
        fixture("wine-benchmark.ttl")
    }

    fn fixture_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn fixture(name: &str) -> String {
        let path = fixture_path(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}. It is committed, so restore it with `git checkout`",
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
        assert_eq!(b.target_schema_version, "0.2.1");
        assert_eq!(b.target_dataset, "worked-example");
        assert_eq!(b.target_dataset_version, "0.2.1");
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
    const GRADABLE: &str = "cqa:answer_kind <https://w3id.org/cqa/AnswerKindEnum#lookup> ; \
                            cqa:expected_anchors ex:r1";

    const TWO_QUESTIONS: &str = "
        ex:q1 cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ;
          cqa:answer_kind <https://w3id.org/cqa/AnswerKindEnum#lookup> ; cqa:expected_anchors ex:r1 .
        ex:q2 cqa:id \"q2\" ; cqa:question \"two?\" ; cqa:ground_truth \"Two.\" ;
          cqa:answer_kind <https://w3id.org/cqa/AnswerKindEnum#lookup> ; cqa:expected_anchors ex:r1 .";

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
             ex:q2 cqa:answer_kind <{CQA}AnswerKindEnum#closed-world-negative> ."
        );
        let err = Benchmark::from_turtle(&benchmark_with("", &questions)).unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m) if m.contains("q2") && m.contains("unconnected_anchors")),
            "{err}"
        );
    }

    #[test]
    fn an_answer_kind_outside_cqas_enum_is_an_error_naming_it() {
        let questions = format!(
            "{TWO_QUESTIONS}
             ex:q2 cqa:answer_kind <https://example.org/AnswerKindEnum#lookup> ."
        );
        let err = Benchmark::from_turtle(&benchmark_with("", &questions)).unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m)
                if m.contains("q2") && m.contains("<https://example.org/AnswerKindEnum#lookup>")),
            "{err}"
        );
    }

    #[test]
    fn yaml_and_turtle_readers_agree() {
        let yaml = Benchmark::from_yaml(
            fixture_path("cqa.yaml"),
            fixture_path("wine-benchmark.yaml"),
        )
        .unwrap();
        assert_eq!(yaml, Benchmark::from_turtle(&wine_benchmark()).unwrap());
    }

    /// The cqa schema the YAML fixtures are read against.
    fn cqa_schema() -> panschema_model::linkml::SchemaDefinition {
        panschema_model::import_resolve::load_schema(&fixture_path("cqa.yaml"), &YamlReader::new())
            .unwrap()
            .schema
    }

    /// The load of `data` as a cqa dataset, as `load_dataset` gives it but
    /// from text.
    fn loaded(data: &str) -> LoadedDataset {
        let schema = cqa_schema();
        let value: serde_norway::Value = serde_norway::from_str(data).unwrap();
        let instances = panschema_model::instances::InstanceSet::from_linkml_data(&schema, &value);
        LoadedDataset {
            schema,
            instances,
            warnings: Vec::new(),
        }
    }

    fn from_yaml_text(data: &str) -> Result<Benchmark> {
        Benchmark::from_loaded(&loaded(data))
    }

    /// Why `data` is refused.
    fn refused(data: &str) -> String {
        match from_yaml_text(data) {
            Err(Error::Benchmark(m)) => m,
            other => panic!("expected a benchmark error, got {other:?}"),
        }
    }

    /// A one-benchmark dataset: the four pins and `questions` as YAML list
    /// entries under `competency_question_answers`.
    fn yaml_benchmark_with(questions: &str) -> String {
        format!(
            "id: b
target_schema: https://example.org/s/
target_schema_version: '1'
target_dataset: d
target_dataset_version: '1'
competency_question_answers:
{questions}"
        )
    }

    const YAML_QUESTION: &str = "  - id: q1
    question: one?
    ground_truth: One.
    answer_kind: [lookup]
    expected_anchors: [r1]
";

    /// The one-question benchmark with `from` replaced by `to`.
    fn yaml_question_with(from: &str, to: &str) -> String {
        let data = yaml_benchmark_with(YAML_QUESTION);
        assert!(data.contains(from), "{from:?} is not in the benchmark");
        data.replace(from, to)
    }

    #[test]
    fn a_bare_anchor_expands_against_the_target_schema() {
        let b = from_yaml_text(&yaml_benchmark_with(YAML_QUESTION)).unwrap();
        assert_eq!(
            b.question("q1").unwrap().expected_anchors,
            BTreeSet::from(["https://example.org/s/r1".to_string()])
        );
    }

    #[test]
    fn a_curie_anchor_expands_against_the_schemas_prefixes() {
        let b = from_yaml_text(&yaml_question_with("[r1]", "[cqa:r1]")).unwrap();
        assert_eq!(
            b.question("q1").unwrap().expected_anchors,
            BTreeSet::from(["https://w3id.org/cqa/r1".to_string()])
        );
    }

    #[test]
    fn a_yaml_load_that_fails_names_the_file() {
        let missing = fixture_path("no-such-benchmark.yaml");
        let err = Benchmark::from_yaml(fixture_path("cqa.yaml"), &missing).unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m) if m.contains("no-such-benchmark.yaml")),
            "{err}"
        );
    }

    #[test]
    fn a_load_warning_is_refused() {
        let mut load = loaded(&yaml_benchmark_with(YAML_QUESTION));
        load.warnings.push("something was skipped".into());
        let err = Benchmark::from_loaded(&load).unwrap_err();
        assert!(
            matches!(&err, Error::Benchmark(m) if m.contains("something was skipped")),
            "{err}"
        );
    }

    #[test]
    fn a_misspelled_slot_is_refused_not_dropped() {
        let m = refused(&yaml_benchmark_with(&format!(
            "{YAML_QUESTION}    expected_citation: [r1]\n"
        )));
        assert!(m.contains("q1") && m.contains("`expected_citation`"), "{m}");
    }

    #[test]
    fn two_yaml_questions_sharing_an_id_are_refused() {
        let m = refused(&yaml_benchmark_with(&format!(
            "{YAML_QUESTION}{YAML_QUESTION}"
        )));
        assert!(
            m.contains("q1") && m.contains("more than one record"),
            "{m}"
        );
    }

    #[test]
    fn an_anchor_with_no_base_to_expand_against_is_refused() {
        let m = refused(&yaml_question_with(
            "target_schema: https://example.org/s/",
            "target_schema: [https://example.org/s/]",
        ));
        assert!(
            m.contains("q1") && m.contains("`expected_anchors`") && m.contains("did not expand"),
            "{m}"
        );
    }

    #[test]
    fn a_collection_entry_that_reads_as_no_question_is_refused() {
        let m = refused(&yaml_benchmark_with(&format!("{YAML_QUESTION}  - ~\n")));
        assert!(m.contains("`competency_question_answers`: an entry"), "{m}");
    }

    #[test]
    fn a_listed_id_that_names_no_question_is_refused() {
        let m = refused(&yaml_benchmark_with(&format!("{YAML_QUESTION}  - q2\n")));
        assert!(
            m.contains("q2") && m.contains("not a CompetencyQuestionAnswer"),
            "{m}"
        );
    }

    #[test]
    fn a_question_listed_twice_reads_once() {
        let b = from_yaml_text(&yaml_benchmark_with(&format!("{YAML_QUESTION}  - q1\n"))).unwrap();
        assert_eq!(b.questions.len(), 1);
    }

    #[test]
    fn a_benchmark_sharing_an_id_with_a_question_is_refused() {
        let m = refused(&yaml_question_with("id: b\n", "id: q1\n"));
        assert!(m.contains("q1") && m.contains("share this id"), "{m}");
    }

    #[test]
    fn a_benchmark_without_an_id_is_refused() {
        let m = refused(&yaml_question_with("id: b\n", ""));
        assert!(m.contains("no Benchmark record"), "{m}");
    }

    #[test]
    fn a_yaml_benchmark_with_no_questions_is_refused() {
        let m = refused(&yaml_benchmark_with("  []\n"));
        assert!(m.contains("holds no competency question answers"), "{m}");
    }

    #[test]
    fn an_answer_kind_name_outside_cqas_enum_is_refused_naming_it() {
        let m = refused(&yaml_question_with("[lookup]", "[look-up]"));
        assert!(m.contains("q1") && m.contains("`look-up`"), "{m}");
    }

    #[test]
    fn a_yaml_question_without_its_ground_truth_is_refused() {
        let m = refused(&yaml_question_with("    ground_truth: One.\n", ""));
        assert!(m.contains("q1") && m.contains("no ground_truth"), "{m}");
    }

    #[test]
    fn a_question_with_two_ground_truths_is_refused() {
        let m = refused(&yaml_question_with(
            "ground_truth: One.",
            "ground_truth: [One., Two.]",
        ));
        assert!(
            m.contains("q1") && m.contains("more than one ground_truth"),
            "{m}"
        );
    }

    #[test]
    fn a_number_where_text_belongs_is_refused() {
        let m = refused(&yaml_question_with("question: one?", "question: 1"));
        assert!(
            m.contains("q1") && m.contains("`question`") && m.contains("quote"),
            "{m}"
        );
    }

    #[test]
    fn a_mapping_where_text_belongs_is_refused() {
        let m = refused(&yaml_question_with(
            "question: one?",
            "question: {text: one?}",
        ));
        assert!(
            m.contains("q1") && m.contains("`question`") && m.contains("not text"),
            "{m}"
        );
    }

    #[test]
    fn a_yaml_benchmark_without_a_pin_is_refused() {
        let m = refused(&yaml_question_with("target_dataset: d\n", ""));
        assert!(m.contains("no target_dataset"), "{m}");
    }

    #[test]
    fn rejects_unparseable_turtle() {
        let err = Benchmark::from_turtle("this is not turtle").unwrap_err();
        assert!(matches!(err, Error::Rdf(_)), "{err}");
    }

    #[test]
    fn answer_kind_round_trips_through_its_name() {
        for sep in ['#', '/'] {
            for kind in [
                AnswerKind::Lookup,
                AnswerKind::ClosedWorldNegative,
                AnswerKind::Comparison,
                AnswerKind::Synthesis,
                AnswerKind::Attribution,
            ] {
                let iri = format!("{CQA}AnswerKindEnum{sep}{}", kind.as_str());
                assert_eq!(AnswerKind::from_iri(&iri), Some(kind), "{iri}");
            }
        }
    }

    #[test]
    fn a_kind_name_outside_cqas_enum_is_no_kind() {
        for iri in [
            "https://example.org/no-such-kind".to_string(),
            "https://example.org/AnswerKindEnum#lookup".to_string(),
            "https://example.org/AnswerKindEnum/lookup".to_string(),
            format!("{CQA}lookup"),
            format!("{CQA}OtherEnum#lookup"),
            format!("{CQA}AnswerKindEnumlookup"),
            format!("{CQA}AnswerKindEnum#foo/lookup"),
        ] {
            assert_eq!(AnswerKind::from_iri(&iri), None, "{iri}");
        }
    }
}
