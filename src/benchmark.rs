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

use crate::Result;
use crate::rdf::{load_turtle, select};
use crate::trace::Graph;

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

/// Why a benchmark was refused: it could not be read whole, or it holds a
/// question no check could grade.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Refusal {
    /// The LinkML source could not be loaded at all.
    #[error("{error}{}", warnings.iter().map(|w| format!("; {w}")).collect::<String>())]
    Load {
        /// Why it could not be loaded.
        error: String,
        /// The warnings the load gave before it failed.
        warnings: Vec<String>,
    },
    /// The LinkML source loaded, but in part.
    #[error("{}", .0.iter().map(Unread::to_string).collect::<Vec<_>>().join("; "))]
    PartlyRead(Vec<Unread>),
    /// No benchmark with an IRI and an id. A LinkML record mints its IRI
    /// from its id, so there only the id can be missing.
    #[error("no Benchmark with an IRI and an id found")]
    NoBenchmark,
    /// More than one benchmark in a file that must hold one.
    #[error("expected one Benchmark, found {0}")]
    ManyBenchmarks(usize),
    /// The benchmark lists no questions.
    #[error("benchmark <{0}> holds no competency question answers")]
    NoQuestions(String),
    /// The benchmark lists an id that names no question.
    #[error("{0}: not a CompetencyQuestionAnswer")]
    NotAQuestion(String),
    /// A record lacks a slot it must have.
    #[error("{record}: no {slot}")]
    Missing {
        /// The record's id, or its IRI in angle brackets where it has no id.
        record: String,
        /// The slot.
        slot: &'static str,
    },
    /// A record has more than one value where it may have one.
    #[error("{record}: more than one {slot}")]
    MoreThanOne {
        /// The record's id, or its IRI in angle brackets where the slot is
        /// its id.
        record: String,
        /// The slot.
        slot: &'static str,
    },
    /// A number or boolean where text belongs. panschema reads an unquoted
    /// `1.0` as a number and an unquoted `yes` as a boolean, so neither
    /// reads back as the text it was meant to be.
    #[error("{record}: `{slot}` is {value}, not text; quote it")]
    Unquoted {
        /// The record's id.
        record: String,
        /// The slot.
        slot: &'static str,
        /// The value as it was read.
        value: String,
    },
    /// Some other value that is not text where text belongs.
    #[error("{record}: `{slot}` holds {kind}, not text")]
    NotText {
        /// The record's id.
        record: String,
        /// The slot.
        slot: &'static str,
        /// What it holds instead.
        kind: String,
    },
    /// An answer kind outside `cqa:AnswerKindEnum`.
    #[error("{question}: unknown answer kind `{kind}`")]
    UnknownAnswerKind {
        /// The question's id.
        question: String,
        /// The kind as written: a name in the YAML, an IRI in the Turtle.
        kind: String,
    },
    /// A closed-world negative that names no anchors left unjoined, so it
    /// would grade on its assertion of absence alone.
    #[error("{0}: closed-world-negative with no unconnected_anchors")]
    NegativeWithoutUnconnectedAnchors(String),
    /// Two questions share an id.
    #[error("{0}: more than one question has this id")]
    DuplicateQuestion(String),
}

/// What a LinkML load read past, as `panschema-model` records it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Unread {
    /// A warning the load gave.
    #[error("{0}")]
    Warning(String),
    /// A value that did not expand against the slot it is relative to.
    #[error("{record}: `{slot}` did not expand against `{base_slot}`")]
    Unexpanded {
        /// The record's id.
        record: String,
        /// The slot holding the value.
        slot: String,
        /// The slot it expands against.
        base_slot: String,
    },
    /// An id more than one record has.
    #[error("{0}: more than one record has this id")]
    DuplicateId(String),
    /// A field that is not a slot of its record's class.
    #[error("{record}: `{field}` is not a slot of {class}")]
    UndeclaredField {
        /// The record's id.
        record: String,
        /// The field.
        field: String,
        /// The record's class.
        class: String,
    },
    /// A collection entry that could not be read as a record.
    #[error("`{slot}`: {} {reason}", key.as_ref().map_or("an entry".into(), |k| format!("entry `{k}`")))]
    UnusableEntry {
        /// The collection's slot.
        slot: String,
        /// The entry's key, in a collection keyed by id.
        key: Option<String>,
        /// Why it could not be read.
        reason: String,
    },
    /// An id the benchmark shares with a record, so no benchmark was read.
    #[error("{0}: the benchmark and a record share this id, so no Benchmark was read")]
    RootCollision(String),
}

impl Refusal {
    fn missing(record: impl Into<String>, slot: &'static str) -> Self {
        Refusal::Missing {
            record: record.into(),
            slot,
        }
    }
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
                .map_err(|failure| Refusal::Load {
                    error: failure.to_string(),
                    warnings: failure.warnings,
                })?;
        Self::from_loaded(&loaded)
    }

    fn from_loaded(loaded: &LoadedDataset) -> Result<Self> {
        let set = &loaded.instances;
        let unread: Vec<Unread> = loaded
            .warnings
            .iter()
            .cloned()
            .map(Unread::Warning)
            .chain(set.expansion_gaps.iter().map(|g| Unread::Unexpanded {
                record: g.record.clone(),
                slot: g.slot.clone(),
                base_slot: g.base_slot.clone(),
            }))
            .chain(set.duplicate_ids.iter().cloned().map(Unread::DuplicateId))
            .chain(
                set.undeclared_fields
                    .iter()
                    .map(|f| Unread::UndeclaredField {
                        record: f.record.clone(),
                        field: f.field.clone(),
                        class: f.class.clone(),
                    }),
            )
            .chain(
                set.unusable_collection_entries
                    .iter()
                    .map(|e| Unread::UnusableEntry {
                        slot: e.slot.clone(),
                        key: e.key.clone(),
                        reason: e.reason.clone(),
                    }),
            )
            .chain(
                set.root_collision
                    .iter()
                    .cloned()
                    .map(Unread::RootCollision),
            )
            .collect();
        if !unread.is_empty() {
            return Err(Refusal::PartlyRead(unread).into());
        }

        let of_type = |class: &'static str| {
            set.instances
                .iter()
                .filter(move |i| i.types.iter().any(|t| t == class))
        };
        // cqa has one `tree_root`, so a dataset holds at most one Benchmark.
        let Some(benchmark) = of_type("Benchmark").next() else {
            return Err(Refusal::NoBenchmark.into());
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
                .ok_or_else(|| Refusal::NotAQuestion(id.clone()))?;
            // An anchor written as a CURIE expands as panschema expands it
            // when it writes the Turtle.
            let anchors = |name: &'static str| -> Result<BTreeSet<String>> {
                Ok(values(record, name)?
                    .iter()
                    .map(|v| expand_curie_or_verbatim(&loaded.schema, v))
                    .collect())
            };
            let answer_kinds = values(record, "answer_kind")?
                .into_iter()
                .map(|name| {
                    AnswerKind::from_name(&name).ok_or_else(|| Refusal::UnknownAnswerKind {
                        question: id.clone(),
                        kind: name,
                    })
                })
                .collect::<std::result::Result<_, _>>()?;
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
                   ?b a cqa:Benchmark ; cqa:id ?id .
                   OPTIONAL {{ ?b cqa:target_schema ?ts }}
                   OPTIONAL {{ ?b cqa:target_schema_version ?tsv }}
                   OPTIONAL {{ ?b cqa:target_dataset ?td }}
                   OPTIONAL {{ ?b cqa:target_dataset_version ?tdv }}
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
            (0, _) => return Err(Refusal::NoBenchmark.into()),
            (n, _) => return Err(Refusal::ManyBenchmarks(n).into()),
        };
        let iri = row.require("b")?;
        let differs = |var| benchmarks.iter().any(|r| r.get(var) != row.get(var));
        if differs("id") {
            return Err(Refusal::MoreThanOne {
                record: format!("<{iri}>"),
                slot: "id",
            }
            .into());
        }
        let id = row.require("id")?;
        let pin = |var, slot| -> Result<String> {
            if differs(var) {
                return Err(Refusal::MoreThanOne {
                    record: id.clone(),
                    slot,
                }
                .into());
            }
            Ok(row.get(var).ok_or_else(|| Refusal::missing(&id, slot))?)
        };
        let target_schema = pin("ts", "target_schema")?;
        let target_schema_version = pin("tsv", "target_schema_version")?;
        let target_dataset = pin("td", "target_dataset")?;
        let target_dataset_version = pin("tdv", "target_dataset_version")?;

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
                .ok_or_else(|| Refusal::missing(format!("<{q_iri}>"), "id"))?;
            let slot = |var: &str, slot| q.get(var).ok_or_else(|| Refusal::missing(&id, slot));
            let question = slot("question", "question")?;
            let ground_truth = slot("gt", "ground_truth")?;
            // A question with two values at one of these slots gives one
            // row per combination, so a second row says which. Rows hold
            // lexical forms, so two values that differ only in language
            // or datatype say none.
            if let Some(first) = questions.get(&q_iri) {
                let slot = if first.id != id {
                    "id"
                } else if first.question != question {
                    "question"
                } else if first.ground_truth != ground_truth {
                    "ground_truth"
                } else {
                    "id, question or ground_truth"
                };
                return Err(Refusal::MoreThanOne {
                    record: first.id.clone(),
                    slot,
                }
                .into());
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
                        let kind =
                            AnswerKind::from_iri(&v).ok_or_else(|| Refusal::UnknownAnswerKind {
                                question: question.id.clone(),
                                kind: v.clone(),
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
            id,
            iri,
            name: row.get("name"),
            target_schema,
            target_schema_version,
            target_dataset,
            target_dataset_version,
            questions: questions.into_values().collect(),
        }
        .checked()
    }

    /// Fold the sub-property slots into the anchors, refuse a question no
    /// check could grade, and put the questions in `cqa:id` order. Both
    /// readers end here, so they refuse the same keys.
    fn checked(mut self) -> Result<Self> {
        if self.questions.is_empty() {
            return Err(Refusal::NoQuestions(self.iri).into());
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
                return Err(Refusal::missing(&question.id, "answer_kind").into());
            }
            if question.expected_anchors.is_empty() {
                return Err(Refusal::missing(&question.id, "expected_anchors").into());
            }
            if question
                .answer_kinds
                .contains(&AnswerKind::ClosedWorldNegative)
                && question.unconnected_anchors.is_empty()
            {
                return Err(Refusal::NegativeWithoutUnconnectedAnchors(question.id.clone()).into());
            }
        }

        // Only the Turtle can give two records one `cqa:id`; the model
        // refuses a repeated id before the YAML reader sees it.
        self.questions.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(pair) = self.questions.windows(2).find(|w| w[0].id == w[1].id) {
            return Err(Refusal::DuplicateQuestion(pair[0].id.clone()).into());
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
fn values(record: &Instance, slot: &'static str) -> Result<Vec<String>> {
    let Some(found) = record.slot_values.iter().find(|v| v.slot == slot) else {
        return Ok(Vec::new());
    };
    found
        .values
        .iter()
        .map(|value| match value {
            InstanceValue::Scalar(ScalarValue::String(s)) => Ok(s.clone()),
            InstanceValue::Reference { target, .. } => Ok(target.clone()),
            InstanceValue::Scalar(scalar) => Err(Refusal::Unquoted {
                record: record.id.clone(),
                slot,
                value: scalar_to_display(scalar),
            }
            .into()),
            InstanceValue::Unexpected(kind) => Err(Refusal::NotText {
                record: record.id.clone(),
                slot,
                kind: kind.to_string(),
            }
            .into()),
        })
        .collect()
}

/// A record's one value at `slot`, which it must have.
fn required(record: &Instance, slot: &'static str) -> Result<String> {
    single(record, slot)?.ok_or_else(|| Refusal::missing(&record.id, slot).into())
}

/// A record's one value at `slot`, if it has one.
fn single(record: &Instance, slot: &'static str) -> Result<Option<String>> {
    let mut found = values(record, slot)?;
    if found.len() > 1 {
        return Err(Refusal::MoreThanOne {
            record: record.id.clone(),
            slot,
        }
        .into());
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
    use crate::Error;

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

    /// Why a reader refused the benchmark.
    fn refusal(read: Result<Benchmark>) -> Refusal {
        match read {
            Err(Error::Benchmark(r)) => r,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn turtle_refused(ttl: &str) -> Refusal {
        refusal(Benchmark::from_turtle(ttl))
    }

    #[test]
    fn rejects_a_file_without_a_benchmark() {
        assert_eq!(
            turtle_refused("@prefix ex: <http://example.org/> . ex:a ex:b ex:c ."),
            Refusal::NoBenchmark
        );
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
        assert_eq!(turtle_refused(&ttl), Refusal::ManyBenchmarks(2));
    }

    #[test]
    fn a_question_without_its_ground_truth_is_an_error_not_dropped() {
        let refusal = turtle_refused(&benchmark_with(
            "",
            &format!(
                "ex:q1 cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ; {GRADABLE} .
                 ex:q2 cqa:id \"q2\" ; cqa:question \"two?\" ; {GRADABLE} ."
            ),
        ));
        assert_eq!(refusal, Refusal::missing("q2", "ground_truth"));
    }

    #[test]
    fn a_question_with_two_values_at_a_single_slot_is_refused_naming_it() {
        for (slot, second) in [
            ("id", "cqa:id \"q1b\""),
            ("question", "cqa:question \"uno?\""),
            ("ground_truth", "cqa:ground_truth \"Uno.\""),
        ] {
            let questions = format!("{TWO_QUESTIONS}\nex:q1 {second} .");
            let refusal = turtle_refused(&benchmark_with("", &questions));
            // Which of q1's two ids comes first is the store's order.
            assert!(
                matches!(&refusal, Refusal::MoreThanOne { record, slot: s }
                    if *s == slot && (slot == "id" || record == "q1")),
                "{slot}: {refusal:?}"
            );
        }
    }

    #[test]
    fn a_question_without_a_check_to_grade_is_an_error() {
        for (slot, dropped) in [
            (
                "answer_kind",
                "cqa:answer_kind <https://w3id.org/cqa/AnswerKindEnum#lookup> ;",
            ),
            ("expected_anchors", "; cqa:expected_anchors ex:r1"),
        ] {
            let q2 = TWO_QUESTIONS.rfind(dropped).unwrap();
            let questions = format!(
                "{}{}",
                &TWO_QUESTIONS[..q2],
                &TWO_QUESTIONS[q2 + dropped.len()..]
            );
            let refusal = turtle_refused(&benchmark_with("", &questions));
            assert_eq!(refusal, Refusal::missing("q2", slot), "{slot}");
        }
    }

    #[test]
    fn a_benchmark_with_two_values_at_a_pin_is_an_error() {
        let refusal = turtle_refused(&benchmark_with("; cqa:target_dataset \"e\"", TWO_QUESTIONS));
        assert_eq!(
            refusal,
            Refusal::MoreThanOne {
                record: "b".into(),
                slot: "target_dataset",
            }
        );
    }

    #[test]
    fn a_benchmark_without_a_pin_is_an_error_naming_it() {
        let ttl = benchmark_with("", TWO_QUESTIONS).replacen("cqa:target_dataset \"d\" ;", "", 1);
        assert_eq!(
            turtle_refused(&ttl),
            Refusal::missing("b", "target_dataset")
        );
    }

    #[test]
    fn a_question_without_an_id_is_an_error_naming_its_iri() {
        let questions = TWO_QUESTIONS.replacen("ex:q1 cqa:id \"q1\" ;", "ex:q1", 1);
        assert_eq!(
            turtle_refused(&benchmark_with("", &questions)),
            Refusal::missing("<http://example.org/q1>", "id")
        );
    }

    #[test]
    fn two_values_differing_only_in_language_name_no_one_slot() {
        let questions = format!("{TWO_QUESTIONS}\nex:q1 cqa:question \"one?\"@en .");
        let refusal = turtle_refused(&benchmark_with("", &questions));
        assert_eq!(
            refusal,
            Refusal::MoreThanOne {
                record: "q1".into(),
                slot: "id, question or ground_truth",
            }
        );
    }

    #[test]
    fn a_blank_node_benchmark_is_no_benchmark() {
        let ttl = benchmark_with("", TWO_QUESTIONS).replacen(
            "ex:b a cqa:Benchmark",
            "[] a cqa:Benchmark",
            1,
        );
        assert_eq!(turtle_refused(&ttl), Refusal::NoBenchmark);
    }

    #[test]
    fn two_questions_sharing_an_id_are_an_error() {
        let refusal = turtle_refused(&benchmark_with(
            "",
            &format!(
                "ex:q1 cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ; {GRADABLE} .
                 ex:q2 cqa:id \"q1\" ; cqa:question \"two?\" ; cqa:ground_truth \"Two.\" ; {GRADABLE} ."
            ),
        ));
        assert_eq!(refusal, Refusal::DuplicateQuestion("q1".into()));
    }

    #[test]
    fn a_closed_world_negative_without_unconnected_anchors_is_an_error() {
        // Without them the reached check passes on zero anchors, and the
        // question grades on `asserts_absence` alone.
        let questions = format!(
            "{TWO_QUESTIONS}
             ex:q2 cqa:answer_kind <{CQA}AnswerKindEnum#closed-world-negative> ."
        );
        assert_eq!(
            turtle_refused(&benchmark_with("", &questions)),
            Refusal::NegativeWithoutUnconnectedAnchors("q2".into())
        );
    }

    #[test]
    fn an_answer_kind_outside_cqas_enum_is_an_error_naming_it() {
        let questions = format!(
            "{TWO_QUESTIONS}
             ex:q2 cqa:answer_kind <https://example.org/AnswerKindEnum#lookup> ."
        );
        assert_eq!(
            turtle_refused(&benchmark_with("", &questions)),
            Refusal::UnknownAnswerKind {
                question: "q2".into(),
                kind: "https://example.org/AnswerKindEnum#lookup".into(),
            }
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
    fn refused(data: &str) -> Refusal {
        refusal(from_yaml_text(data))
    }

    /// What the load of `data` read past.
    fn unread(data: &str) -> Vec<Unread> {
        match refused(data) {
            Refusal::PartlyRead(unread) => unread,
            other => panic!("expected a partial read, got {other:?}"),
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
        match refusal(Benchmark::from_yaml(fixture_path("cqa.yaml"), &missing)) {
            Refusal::Load { error, .. } => {
                assert!(error.contains("no-such-benchmark.yaml"), "{error}")
            }
            other => panic!("expected a load refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_load_warning_is_refused() {
        let mut load = loaded(&yaml_benchmark_with(YAML_QUESTION));
        load.warnings.push("something was skipped".into());
        assert_eq!(
            refusal(Benchmark::from_loaded(&load)),
            Refusal::PartlyRead(vec![Unread::Warning("something was skipped".into())])
        );
    }

    #[test]
    fn a_misspelled_slot_is_refused_not_dropped() {
        let unread = unread(&yaml_benchmark_with(&format!(
            "{YAML_QUESTION}    expected_citation: [r1]\n"
        )));
        assert_eq!(
            unread,
            [Unread::UndeclaredField {
                record: "q1".into(),
                field: "expected_citation".into(),
                class: "CompetencyQuestionAnswer".into(),
            }]
        );
    }

    #[test]
    fn two_yaml_questions_sharing_an_id_are_refused() {
        let unread = unread(&yaml_benchmark_with(&format!(
            "{YAML_QUESTION}{YAML_QUESTION}"
        )));
        assert_eq!(unread, [Unread::DuplicateId("q1".into())]);
    }

    #[test]
    fn an_anchor_with_no_base_to_expand_against_is_refused() {
        let unread = unread(&yaml_question_with(
            "target_schema: https://example.org/s/",
            "target_schema: [https://example.org/s/]",
        ));
        assert!(
            matches!(&unread[..], [Unread::Unexpanded { record, slot, base_slot }]
                if record == "q1" && slot == "expected_anchors" && base_slot == "target_schema"),
            "{unread:?}"
        );
    }

    #[test]
    fn a_collection_entry_that_reads_as_no_question_is_refused() {
        let unread = unread(&yaml_benchmark_with(&format!("{YAML_QUESTION}  - ~\n")));
        assert!(
            matches!(&unread[..], [Unread::UnusableEntry { slot, key: None, .. }]
                if slot == "competency_question_answers"),
            "{unread:?}"
        );
    }

    #[test]
    fn a_listed_id_that_names_no_question_is_refused() {
        let refusal = refused(&yaml_benchmark_with(&format!("{YAML_QUESTION}  - q2\n")));
        assert_eq!(refusal, Refusal::NotAQuestion("q2".into()));
    }

    #[test]
    fn a_question_listed_twice_reads_once() {
        let b = from_yaml_text(&yaml_benchmark_with(&format!("{YAML_QUESTION}  - q1\n"))).unwrap();
        assert_eq!(b.questions.len(), 1);
    }

    #[test]
    fn a_benchmark_sharing_an_id_with_a_question_is_refused() {
        let unread = unread(&yaml_question_with("id: b\n", "id: q1\n"));
        assert!(
            unread.contains(&Unread::RootCollision("q1".into())),
            "{unread:?}"
        );
    }

    #[test]
    fn a_benchmark_without_an_id_is_refused() {
        let refusal = refused(&yaml_question_with("id: b\n", ""));
        assert_eq!(refusal, Refusal::NoBenchmark);
    }

    #[test]
    fn a_yaml_benchmark_with_no_questions_is_refused() {
        let refusal = refused(&yaml_benchmark_with("  []\n"));
        assert_eq!(
            refusal,
            Refusal::NoQuestions("https://w3id.org/cqa/b".into())
        );
    }

    #[test]
    fn an_answer_kind_name_outside_cqas_enum_is_refused_naming_it() {
        let refusal = refused(&yaml_question_with("[lookup]", "[look-up]"));
        assert_eq!(
            refusal,
            Refusal::UnknownAnswerKind {
                question: "q1".into(),
                kind: "look-up".into(),
            }
        );
    }

    #[test]
    fn a_yaml_question_without_its_ground_truth_is_refused() {
        let refusal = refused(&yaml_question_with("    ground_truth: One.\n", ""));
        assert_eq!(refusal, Refusal::missing("q1", "ground_truth"));
    }

    #[test]
    fn a_question_with_two_ground_truths_is_refused() {
        let refusal = refused(&yaml_question_with(
            "ground_truth: One.",
            "ground_truth: [One., Two.]",
        ));
        assert_eq!(
            refusal,
            Refusal::MoreThanOne {
                record: "q1".into(),
                slot: "ground_truth",
            }
        );
    }

    #[test]
    fn a_number_or_boolean_where_text_belongs_is_refused() {
        for value in ["1", "true"] {
            let refusal = refused(&yaml_question_with(
                "question: one?",
                &format!("question: {value}"),
            ));
            assert_eq!(
                refusal,
                Refusal::Unquoted {
                    record: "q1".into(),
                    slot: "question",
                    value: value.into(),
                },
                "{value}"
            );
        }
    }

    #[test]
    fn a_mapping_where_text_belongs_is_refused() {
        let refusal = refused(&yaml_question_with(
            "question: one?",
            "question: {text: one?}",
        ));
        assert!(
            matches!(&refusal, Refusal::NotText { record, slot: "question", .. } if record == "q1"),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_yaml_benchmark_without_a_pin_is_refused() {
        let refusal = refused(&yaml_question_with("target_dataset: d\n", ""));
        assert_eq!(refusal, Refusal::missing("b", "target_dataset"));
    }

    #[test]
    fn a_failed_load_gives_its_warnings_with_its_error() {
        let refusal = Refusal::Load {
            error: "no such file".into(),
            warnings: vec!["an import was skipped".into()],
        }
        .to_string();
        assert!(
            refusal.contains("no such file") && refusal.contains("an import was skipped"),
            "{refusal}"
        );
    }

    #[test]
    fn a_partial_read_names_every_part_it_read_past() {
        let refusal = Refusal::PartlyRead(vec![
            Unread::DuplicateId("q1".into()),
            Unread::UnusableEntry {
                slot: "competency_question_answers".into(),
                key: Some("q2".into()),
                reason: "is not a mapping".into(),
            },
        ])
        .to_string();
        assert!(
            refusal.contains("q1") && refusal.contains("`q2`"),
            "{refusal}"
        );
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
