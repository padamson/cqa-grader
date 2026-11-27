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

use std::collections::btree_map::Entry;
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
#[serde(rename_all = "snake_case")]
pub enum AnswerKind {
    /// A value read from a single record.
    Lookup,
    /// The correct answer is that the graph holds no such record. cqa 0.1
    /// spelled it `closed-world-negative`, which is still read.
    #[serde(alias = "closed-world-negative")]
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
            AnswerKind::ClosedWorldNegative => "closed_world_negative",
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
    /// The order a correct answer gives its records in, if the key asks for one.
    pub order: Option<Order>,
}

/// An ordered question's order: cqa's `ordered_by`, with the records its
/// expected values at that slot name, ranked by those values as numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    /// The slot whose values order the answer, by the target schema's name for it.
    pub slot: String,
    /// The records in ascending order of their values, one tier per value,
    /// so records whose values are equal share a tier.
    pub tiers: Vec<BTreeSet<String>>,
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
    /// Which instance graph the anchors resolve in: cqa 0.1's
    /// `target_dataset`, or the `dataset` of the one entry in cqa 0.2's
    /// `target_datasets`.
    pub target_dataset: String,
    /// The version of that dataset: cqa 0.1's `target_dataset_version`, or
    /// the entry's `version`.
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
    #[error("{0}: closed_world_negative with no unconnected_anchors")]
    NegativeWithoutUnconnectedAnchors(String),
    /// A `target_datasets` value that names no `TargetDataset` record: in
    /// the YAML, a bare string where the entry belongs.
    #[error("{0}: not a TargetDataset; an entry of target_datasets gives its dataset and version")]
    NotATargetDataset(String),
    /// A benchmark whose `target_datasets` lists more than one dataset.
    /// cqa 0.2 allows it; this grader grades against one graph.
    #[error("{benchmark}: targets {count} datasets; this grader grades a benchmark against one")]
    SeveralTargetDatasets {
        /// The benchmark's id.
        benchmark: String,
        /// How many it lists.
        count: usize,
    },
    /// A benchmark that names no target in either shape.
    #[error(
        "{0}: no target; give `target_datasets` (cqa 0.2), or `target_dataset` and \
         `target_dataset_version` (cqa 0.1)"
    )]
    NoTarget(String),
    /// A benchmark that names its target both as cqa 0.2 does
    /// (`target_datasets`) and as cqa 0.1 did (`target_dataset`,
    /// `target_dataset_version`).
    #[error("{0}: names its target in both `target_datasets` and `target_dataset`; keep one")]
    TwoTargetShapes(String),
    /// Two questions share an id.
    #[error("{0}: more than one question has this id")]
    DuplicateQuestion(String),
    /// An `expected_values` value that names no `ExpectedValue` record: in
    /// the YAML, a bare string where the entry belongs.
    #[error(
        "{0}: not an ExpectedValue; an entry of expected_values gives its record, slot and value"
    )]
    NotAnExpectedValue(String),
    /// A value at an ordering slot that does not read as a number. Until the
    /// grader reads a value by its slot's range it compares an order's
    /// values as numbers, and text sorted as text would put 10 before 9.
    #[error(
        "{question}: ordered by `{slot}`, and its value `{value}` there is not a number; \
         the grader orders by number until it reads values by the slot's range"
    )]
    NotANumber {
        /// The question's id.
        question: String,
        /// The ordering slot.
        slot: String,
        /// The value as written.
        value: String,
    },
    /// One record given two different values at the ordering slot, so the
    /// key puts it in two places.
    #[error("{question}: <{record}> has two values at the slot the question is ordered by")]
    TwoValuesToOrderBy {
        /// The question's id.
        question: String,
        /// The record.
        record: String,
    },
    /// An ordered question that ranks a record outside its anchors. cqa
    /// asks that every record an expected value names be among them, and
    /// an answer need not retrieve any other.
    #[error("{question}: ordered over <{record}>, which is not among its expected_anchors")]
    RankedOutsideAnchors {
        /// The question's id.
        question: String,
        /// The record.
        record: String,
    },
    /// An ordered question with fewer than two records valued at its
    /// ordering slot, so there is no order to check.
    #[error("{question}: ordered by `{slot}`, and fewer than two records have a value there")]
    NothingToOrder {
        /// The question's id.
        question: String,
        /// The ordering slot.
        slot: String,
    },
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
        let of_type = |class: &'static str| {
            set.instances
                .iter()
                .filter(move |i| i.types.iter().any(|t| t == class))
        };
        // panschema mints target entries and expected values ids the author
        // never wrote, so what the load read past in one is reported under
        // the name every other refusal gives the entry. A bare string where
        // an entry belongs is no entry, and keeps the name it has.
        let mut minted: BTreeMap<&str, String> = BTreeMap::new();
        for (owners, slot, entries, name) in [
            (
                "Benchmark",
                "target_datasets",
                "TargetDataset",
                entry_name as fn(&str) -> String,
            ),
            (
                "CompetencyQuestionAnswer",
                "expected_values",
                "ExpectedValue",
                value_name,
            ),
        ] {
            for owner in of_type(owners) {
                let held = values(owner, slot).unwrap_or_default();
                for entry in of_type(entries).filter(|e| held.contains(&e.id)) {
                    minted.insert(&entry.id, name(&owner.id));
                }
            }
        }
        let named = |record: &String| minted.get(record.as_str()).unwrap_or(record).clone();
        let unread: Vec<Unread> = loaded
            .warnings
            .iter()
            .cloned()
            .map(Unread::Warning)
            .chain(set.expansion_gaps.iter().map(|g| Unread::Unexpanded {
                record: named(&g.record),
                slot: g.slot.clone(),
                base_slot: g.base_slot.clone(),
            }))
            .chain(set.duplicate_ids.iter().cloned().map(Unread::DuplicateId))
            .chain(
                set.undeclared_fields
                    .iter()
                    .map(|f| Unread::UndeclaredField {
                        record: named(&f.record),
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

        // cqa has one `tree_root`, so a dataset holds at most one Benchmark.
        let Some(benchmark) = of_type("Benchmark").next() else {
            return Err(Refusal::NoBenchmark.into());
        };

        let records: BTreeMap<&str, &Instance> = of_type("CompetencyQuestionAnswer")
            .map(|i| (i.id.as_str(), i))
            .collect();
        let expected: BTreeMap<&str, &Instance> = of_type("ExpectedValue")
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
            // The schema the benchmark is read against says how a kind is
            // spelled, so cqa 0.1's spelling is no kind under cqa 0.2, nor
            // 0.2's under 0.1.
            let permitted = |name: &str| {
                loaded
                    .schema
                    .enums
                    .get("AnswerKindEnum")
                    .is_some_and(|e| e.permissible_values.contains_key(name))
            };
            let answer_kinds = values(record, "answer_kind")?
                .into_iter()
                .map(|name| {
                    AnswerKind::from_name(&name)
                        .filter(|_| permitted(&name))
                        .ok_or_else(|| Refusal::UnknownAnswerKind {
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
                order: yaml_order(&loaded.schema, record, &id, &expected)?,
                id,
            });
        }

        let id = required(benchmark, "id")?;
        let entries = values(benchmark, "target_datasets")?
            .iter()
            .map(|target| {
                let Some(record) = of_type("TargetDataset").find(|i| i.id == *target) else {
                    return Err(Refusal::NotATargetDataset(target.clone()).into());
                };
                // panschema mints the entry an id the author never wrote,
                // so refusals name it as the Turtle reader does.
                let name = entry_name(&id);
                Ok(TargetEntry {
                    dataset: single_as(record, &name, "dataset")?,
                    version: single_as(record, &name, "version")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let target = one_target(
            &id,
            (
                single(benchmark, "target_dataset")?,
                single(benchmark, "target_dataset_version")?,
            ),
            entries,
        )?;

        Benchmark {
            id,
            name: single(benchmark, "name")?,
            target_schema: required(benchmark, "target_schema")?,
            target_schema_version: required(benchmark, "target_schema_version")?,
            target_dataset: target.dataset,
            target_dataset_version: target.dataset_version,
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
        let pin = |var, slot| -> Result<Option<String>> {
            if differs(var) {
                return Err(Refusal::MoreThanOne {
                    record: id.clone(),
                    slot,
                }
                .into());
            }
            Ok(row.get(var))
        };
        let required_pin = |var, slot| -> Result<String> {
            Ok(pin(var, slot)?.ok_or_else(|| Refusal::missing(&id, slot))?)
        };
        let target_schema = required_pin("ts", "target_schema")?;
        let target_schema_version = required_pin("tsv", "target_schema_version")?;
        let target = one_target(
            &id,
            (
                pin("td", "target_dataset")?,
                pin("tdv", "target_dataset_version")?,
            ),
            turtle_target_entries(store, &iri, &id)?,
        )?;

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
                    order: None,
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
        turtle_orders(store, &iri, &mut questions)?;

        Benchmark {
            id,
            iri,
            name: row.get("name"),
            target_schema,
            target_schema_version,
            target_dataset: target.dataset,
            target_dataset_version: target.dataset_version,
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
            let mut ranked = question.order.iter().flat_map(|o| o.tiers.iter().flatten());
            if let Some(outside) = ranked.find(|r| !question.expected_anchors.contains(*r)) {
                return Err(Refusal::RankedOutsideAnchors {
                    question: question.id.clone(),
                    record: outside.clone(),
                }
                .into());
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

/// One entry of cqa 0.2's `target_datasets` as a reader found it.
struct TargetEntry {
    dataset: Option<String>,
    version: Option<String>,
}

/// The graph a benchmark targets, from whichever shape names it: cqa 0.2's
/// `target_datasets` (`entries`), or cqa 0.1's `target_dataset` and
/// `target_dataset_version` (`pair`). Both readers end here.
fn one_target(
    benchmark: &str,
    pair: (Option<String>, Option<String>),
    mut entries: Vec<TargetEntry>,
) -> Result<Graph> {
    if entries.len() > 1 {
        return Err(Refusal::SeveralTargetDatasets {
            benchmark: benchmark.into(),
            count: entries.len(),
        }
        .into());
    }
    let (dataset, dataset_version) = match (entries.pop(), pair) {
        (Some(_), (Some(_), _) | (_, Some(_))) => {
            return Err(Refusal::TwoTargetShapes(benchmark.into()).into());
        }
        (Some(entry), _) => {
            let record = entry_name(benchmark);
            (
                entry
                    .dataset
                    .ok_or_else(|| Refusal::missing(&record, "dataset"))?,
                entry
                    .version
                    .ok_or_else(|| Refusal::missing(&record, "version"))?,
            )
        }
        (None, (None, None)) => return Err(Refusal::NoTarget(benchmark.into()).into()),
        (None, (dataset, version)) => (
            dataset.ok_or_else(|| Refusal::missing(benchmark, "target_dataset"))?,
            version.ok_or_else(|| Refusal::missing(benchmark, "target_dataset_version"))?,
        ),
    };
    Ok(Graph {
        dataset,
        dataset_version,
    })
}

/// How a refusal names a benchmark's target entry, which has no id of its
/// own in either form.
fn entry_name(benchmark: &str) -> String {
    format!("{benchmark}'s target dataset")
}

/// The entries of `cqa:target_datasets` on the benchmark at `iri`, one per
/// node.
fn turtle_target_entries(store: &Store, iri: &str, benchmark: &str) -> Result<Vec<TargetEntry>> {
    let name = entry_name(benchmark);
    turtle_entries(
        store,
        &format!("VALUES ?s {{ <{iri}> }}"),
        "target_datasets",
        [("cqa:dataset", "dataset"), ("pav:version", "version")],
    )?
    .iter()
    .map(|entry| match &entry.stray {
        Some(stray) => Err(Refusal::NotATargetDataset(stray.clone()).into()),
        None => Ok(TargetEntry {
            dataset: entry.part(&name, "dataset")?,
            version: entry.part(&name, "version")?,
        }),
    })
    .collect()
}

/// One entry of an inlined slot as the Turtle holds it. Nothing is refused
/// until a part is read, so a caller reads only the parts a check needs.
struct TurtleEntry<const N: usize> {
    /// The subject the entry is on.
    subject: String,
    /// What stands where an entry belongs, when this is none: a literal's
    /// text, or in angle brackets the IRI of a node with none of the
    /// parts, which is how panschema writes a bare string.
    stray: Option<String>,
    /// For each part: the slot a refusal names, how many values the node
    /// has there, and one of them, unless it is a blank node.
    parts: [(&'static str, String, Option<String>); N],
}

impl<const N: usize> TurtleEntry<N> {
    /// The entry's one value at the part `slot`, if it has one. A refusal
    /// names the entry `name`.
    fn part(&self, name: &str, slot: &'static str) -> Result<Option<String>> {
        let found = self.parts.iter().find(|(part, ..)| *part == slot);
        match found.map(|(_, count, value)| (count.as_str(), value)) {
            None | Some(("0", _)) => Ok(None),
            Some(("1", Some(value))) => Ok(Some(value.clone())),
            Some(("1", None)) => Err(Refusal::NotText {
                record: name.into(),
                slot,
                kind: "a blank node".into(),
            }
            .into()),
            Some(_) => Err(Refusal::MoreThanOne {
                record: name.into(),
                slot,
            }
            .into()),
        }
    }
}

/// The entries of `slot` on each subject the pattern `subjects` binds to
/// `?s`, with each of `parts`: a predicate, and the slot a refusal names.
/// An entry is a blank node, which a row leaves out, so each is a group.
/// It need not be typed, as a question need not be.
fn turtle_entries<const N: usize>(
    store: &Store,
    subjects: &str,
    slot: &str,
    parts: [(&str, &'static str); N],
) -> Result<Vec<TurtleEntry<N>>> {
    let mut projected = String::new();
    let mut optional = String::new();
    for (i, (predicate, _)) in parts.iter().enumerate() {
        projected.push_str(&format!(
            " (SAMPLE(?v{i}) AS ?value{i}) (COUNT(DISTINCT ?v{i}) AS ?count{i})"
        ));
        optional.push_str(&format!(" OPTIONAL {{ ?e {predicate} ?v{i} }}"));
    }
    let rows = select(
        store,
        &format!(
            "PREFIX cqa: <{CQA}> PREFIX pav: <http://purl.org/pav/>
             SELECT ?s ?e (isLiteral(?e) AS ?literal){projected}
             WHERE {{ {subjects} ?s cqa:{slot} ?e .{optional} }}
             GROUP BY ?s ?e"
        ),
    )?;
    let mut entries = Vec::new();
    for row in rows {
        let mut read = Vec::new();
        for (i, (_, part)) in parts.iter().enumerate() {
            read.push((
                *part,
                row.require(&format!("count{i}"))?,
                row.get(&format!("value{i}")),
            ));
        }
        let partless = read.iter().all(|(_, count, _)| count == "0");
        let stray = match row.get("e") {
            Some(text) if row.require("literal")? == "true" => Some(text),
            Some(iri) if partless => Some(format!("<{iri}>")),
            _ => None,
        };
        let mut read = read.into_iter();
        entries.push(TurtleEntry {
            subject: row.require("s")?,
            stray,
            parts: std::array::from_fn(|_| read.next().unwrap_or_default()),
        });
    }
    Ok(entries)
}

/// How a refusal names an entry of a question's `expected_values`, which
/// has no id of its own in either form.
fn value_name(question: &str) -> String {
    format!("{question}'s expected value")
}

/// A decimal number as a key writes it (`-0.5`, `10`, `1e3`), compared
/// exactly: a float would tie values that differ past its precision.
///
/// Its value is `0.d₁d₂…` × 10^`exponent`, with no leading or trailing
/// zero digit, so equal numbers are equal values. Zero has no digits.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Decimal {
    negative: bool,
    digits: String,
    exponent: i64,
}

impl Decimal {
    /// A sign, digits with at most one decimal point, and an optional
    /// exponent; `None` for anything else, `NaN` and `inf` included.
    fn parse(text: &str) -> Option<Self> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (mantissa, shift) = match unsigned.split_once(['e', 'E']) {
            Some((m, e)) => (m, e.parse::<i64>().ok()?),
            None => (unsigned, 0),
        };
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let all = format!("{int}{frac}");
        if all.is_empty() || !all.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let lead = all.bytes().take_while(|b| *b == b'0').count();
        let digits = all[lead..].trim_end_matches('0').to_owned();
        if digits.is_empty() {
            return Some(Decimal {
                negative: false,
                digits,
                exponent: 0,
            });
        }
        let exponent = (int.len() as i64 - lead as i64).checked_add(shift)?;
        Some(Decimal {
            negative,
            digits,
            exponent,
        })
    }

    /// -1, 0 or 1.
    fn sign(&self) -> i8 {
        match (self.digits.is_empty(), self.negative) {
            (true, _) => 0,
            (false, true) => -1,
            (false, false) => 1,
        }
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let magnitude = || {
            self.exponent
                .cmp(&other.exponent)
                .then_with(|| self.digits.cmp(&other.digits))
        };
        match self.sign().cmp(&other.sign()) {
            std::cmp::Ordering::Equal if self.negative => magnitude().reverse(),
            std::cmp::Ordering::Equal => magnitude(),
            unequal => unequal,
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The record and value of one expected value, if it is at `slot`, read
/// through `part`. The slot is read first and the rest only at `slot`, so
/// a part no check reads is never refused. A refusal names the entry
/// `name`.
fn at_slot(
    slot: &str,
    name: &str,
    part: impl Fn(&'static str) -> Result<Option<String>>,
) -> Result<Option<(String, String)>> {
    let need = |asked: &'static str| -> Result<String> {
        Ok(part(asked)?.ok_or_else(|| Refusal::missing(name, asked))?)
    };
    if need("slot")? != slot {
        return Ok(None);
    }
    Ok(Some((need("record")?, need("value")?)))
}

/// The order of the question `question`, ordered by `slot`, from each of
/// its expected values as [`at_slot`] read it. Both readers end here, so
/// they refuse the same keys.
fn order_of(
    question: &str,
    slot: String,
    read: Vec<Result<Option<(String, String)>>>,
) -> Result<Order> {
    let mut faults = Vec::new();
    let mut ranked = Vec::new();
    for entry in read {
        match entry {
            Ok(found) => ranked.extend(found),
            Err(fault) => faults.push(fault),
        }
    }
    // The readers meet the entries in different orders. Of several faults
    // this gives the first by its message, and the values are sorted, so
    // both readers refuse a key for the same reason.
    if let Some(fault) = faults.into_iter().min_by_key(Error::to_string) {
        return Err(fault);
    }
    ranked.sort();
    let mut values: BTreeMap<String, Decimal> = BTreeMap::new();
    for (record, value) in ranked {
        let Some(number) = Decimal::parse(&value) else {
            return Err(Refusal::NotANumber {
                question: question.into(),
                slot,
                value,
            }
            .into());
        };
        match values.entry(record) {
            Entry::Vacant(vacant) => {
                vacant.insert(number);
            }
            Entry::Occupied(held) if *held.get() != number => {
                return Err(Refusal::TwoValuesToOrderBy {
                    question: question.into(),
                    record: held.key().clone(),
                }
                .into());
            }
            Entry::Occupied(_) => {}
        }
    }
    if values.len() < 2 {
        return Err(Refusal::NothingToOrder {
            question: question.into(),
            slot,
        }
        .into());
    }
    let mut tiers: BTreeMap<Decimal, BTreeSet<String>> = BTreeMap::new();
    for (record, number) in values {
        tiers.entry(number).or_default().insert(record);
    }
    Ok(Order {
        slot,
        tiers: tiers.into_values().collect(),
    })
}

/// The order of the LinkML question `record`, whose id is `id`, if it is
/// ordered. Its expected values are read only then, since no check reads
/// them otherwise.
fn yaml_order(
    schema: &panschema_model::linkml::SchemaDefinition,
    record: &Instance,
    id: &str,
    expected: &BTreeMap<&str, &Instance>,
) -> Result<Option<Order>> {
    let Some(slot) = single(record, "ordered_by")? else {
        return Ok(None);
    };
    let name = value_name(id);
    let read = values(record, "expected_values")?
        .iter()
        .map(|value| {
            let Some(entry) = expected.get(value.as_str()) else {
                return Err(Refusal::NotAnExpectedValue(value.clone()).into());
            };
            at_slot(&slot, &name, |part| {
                let read = single_as(entry, &name, part)?;
                Ok(match part {
                    "record" => read.map(|r| expand_curie_or_verbatim(schema, &r)),
                    _ => read,
                })
            })
        })
        .collect();
    Ok(Some(order_of(id, slot, read)?))
}

/// Set the order of each ordered question of the benchmark at `iri`, each
/// question whole and in id order, as the YAML reader takes them.
fn turtle_orders(
    store: &Store,
    iri: &str,
    questions: &mut BTreeMap<String, Question>,
) -> Result<()> {
    let mut slots: BTreeMap<String, Vec<Option<String>>> = BTreeMap::new();
    for row in select(
        store,
        &format!(
            "PREFIX cqa: <{CQA}>
             SELECT ?q ?o WHERE {{
               <{iri}> cqa:competency_question_answers ?q .
               ?q cqa:ordered_by ?o .
             }}"
        ),
    )? {
        slots
            .entry(row.require("q")?)
            .or_default()
            .push(row.get("o"));
    }
    let mut entries: BTreeMap<String, Vec<TurtleEntry<3>>> = BTreeMap::new();
    for entry in turtle_entries(
        store,
        &format!("<{iri}> cqa:competency_question_answers ?s . ?s cqa:ordered_by ?o ."),
        "expected_values",
        [
            ("cqa:record", "record"),
            ("cqa:slot", "slot"),
            ("cqa:value", "value"),
        ],
    )? {
        entries
            .entry(entry.subject.clone())
            .or_default()
            .push(entry);
    }
    let mut ordered: Vec<&mut Question> = questions
        .values_mut()
        .filter(|q| slots.contains_key(&q.iri))
        .collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    for question in ordered {
        let slot = match slots.remove(&question.iri).unwrap_or_default().as_slice() {
            [Some(slot)] => slot.clone(),
            // A row leaves a blank node out.
            [None] => {
                return Err(Refusal::NotText {
                    record: question.id.clone(),
                    slot: "ordered_by",
                    kind: "a blank node".into(),
                }
                .into());
            }
            _ => {
                return Err(Refusal::MoreThanOne {
                    record: question.id.clone(),
                    slot: "ordered_by",
                }
                .into());
            }
        };
        let name = value_name(&question.id);
        let read = entries
            .remove(&question.iri)
            .unwrap_or_default()
            .iter()
            .map(|entry| match &entry.stray {
                Some(stray) => Err(Refusal::NotAnExpectedValue(stray.clone()).into()),
                None => at_slot(&slot, &name, |part| entry.part(&name, part)),
            })
            .collect();
        question.order = Some(order_of(&question.id, slot, read)?);
    }
    Ok(())
}

/// A record's values at `slot`, as text: a string literal, or the record a
/// reference names (an anchor, expanded to its IRI).
fn values(record: &Instance, slot: &'static str) -> Result<Vec<String>> {
    values_as(record, &record.id, slot)
}

/// `values`, with a refusal naming the record `name` rather than its id.
fn values_as(record: &Instance, name: &str, slot: &'static str) -> Result<Vec<String>> {
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
                record: name.into(),
                slot,
                value: scalar_to_display(scalar),
            }
            .into()),
            InstanceValue::Unexpected(kind) => Err(Refusal::NotText {
                record: name.into(),
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
    single_as(record, &record.id, slot)
}

/// `single`, with a refusal naming the record `name` rather than its id.
fn single_as(record: &Instance, name: &str, slot: &'static str) -> Result<Option<String>> {
    let mut found = values_as(record, name, slot)?;
    if found.len() > 1 {
        return Err(Refusal::MoreThanOne {
            record: name.into(),
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
             @prefix pav: <http://purl.org/pav/> .
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

    /// `benchmark_with("", TWO_QUESTIONS)` with its target written as cqa
    /// 0.2 writes it: `targets`, the entries of `cqa:target_datasets`.
    fn benchmark_targeting(targets: &str) -> String {
        let ttl = benchmark_with("", TWO_QUESTIONS);
        let pair = "cqa:target_dataset \"d\" ; cqa:target_dataset_version \"1\" ;";
        assert!(ttl.contains(pair), "{pair:?} is not in the benchmark");
        ttl.replacen(pair, &format!("cqa:target_datasets {targets} ;"), 1)
    }

    /// One `TargetDataset`, as panschema writes it: a blank node.
    const TARGET: &str = "[ a cqa:TargetDataset ; cqa:dataset \"d\" ; pav:version \"1\" ]";

    #[test]
    fn a_cqa_0_2_target_names_the_graph() {
        let b = Benchmark::from_turtle(&benchmark_targeting(TARGET)).unwrap();
        assert_eq!(b.target_graph(), crate::trace::tests::graph());
    }

    #[test]
    fn a_benchmark_targeting_two_datasets_is_refused() {
        let two =
            format!("{TARGET}, [ a cqa:TargetDataset ; cqa:dataset \"e\" ; pav:version \"1\" ]");
        assert_eq!(
            turtle_refused(&benchmark_targeting(&two)),
            Refusal::SeveralTargetDatasets {
                benchmark: "b".into(),
                count: 2,
            }
        );
    }

    #[test]
    fn a_target_dataset_without_its_name_or_version_is_refused_naming_it() {
        for (slot, dropped) in [
            ("dataset", "cqa:dataset \"d\" ;"),
            ("version", "; pav:version \"1\""),
        ] {
            let target = TARGET.replacen(dropped, "", 1);
            assert_eq!(
                turtle_refused(&benchmark_targeting(&target)),
                Refusal::missing("b's target dataset", slot),
                "{target}"
            );
        }
    }

    #[test]
    fn a_target_dataset_with_two_names_or_versions_is_refused() {
        for (slot, one, two) in [
            ("dataset", "cqa:dataset \"d\"", "cqa:dataset \"d\", \"e\""),
            ("version", "pav:version \"1\"", "pav:version \"1\", \"2\""),
        ] {
            let target = TARGET.replacen(one, two, 1);
            assert_eq!(
                turtle_refused(&benchmark_targeting(&target)),
                Refusal::MoreThanOne {
                    record: "b's target dataset".into(),
                    slot,
                },
                "{target}"
            );
        }
    }

    #[test]
    fn a_turtle_target_written_as_a_bare_literal_is_refused_naming_it() {
        assert_eq!(
            turtle_refused(&benchmark_targeting("\"d\"")),
            Refusal::NotATargetDataset("d".into())
        );
    }

    #[test]
    fn a_target_dataset_node_reads_without_its_type() {
        let untyped = "[ cqa:dataset \"d\" ; pav:version \"1\" ]";
        let b = Benchmark::from_turtle(&benchmark_targeting(untyped)).unwrap();
        assert_eq!(b.target_graph(), crate::trace::tests::graph());
    }

    #[test]
    fn a_benchmark_naming_its_target_both_ways_is_refused() {
        // Either of cqa 0.1's two slots beside `target_datasets` is enough.
        for old in [
            "cqa:target_dataset \"d\"",
            "cqa:target_dataset_version \"1\"",
        ] {
            let ttl = benchmark_targeting(TARGET).replacen(
                "cqa:target_datasets",
                &format!("{old} ; cqa:target_datasets"),
                1,
            );
            assert_eq!(
                turtle_refused(&ttl),
                Refusal::TwoTargetShapes("b".into()),
                "{old}"
            );
        }
    }

    #[test]
    fn a_benchmark_naming_no_target_is_refused() {
        let ttl =
            benchmark_targeting(TARGET).replacen(&format!("cqa:target_datasets {TARGET} ;"), "", 1);
        assert_eq!(turtle_refused(&ttl), Refusal::NoTarget("b".into()));
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
             ex:q2 cqa:answer_kind <{CQA}AnswerKindEnum#closed_world_negative> ."
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

    /// The cqa schema fixture `name`: `cqa.yaml`, the one wine's benchmark
    /// is written against, or `cqa-0.2/cqa.yaml`.
    fn cqa_schema(name: &str) -> panschema_model::linkml::SchemaDefinition {
        panschema_model::import_resolve::load_schema(&fixture_path(name), &YamlReader::new())
            .unwrap()
            .schema
    }

    /// The load of `data` as a cqa dataset, as `load_dataset` gives it but
    /// from text.
    fn loaded(data: &str) -> LoadedDataset {
        loaded_against("cqa.yaml", data)
    }

    /// The load of `data` read against the cqa schema fixture `schema`.
    fn loaded_against(schema: &str, data: &str) -> LoadedDataset {
        let schema = cqa_schema(schema);
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
        read_past(from_yaml_text(data))
    }

    /// What a load read past, where it was refused for reading in part.
    fn read_past(read: Result<Benchmark>) -> Vec<Unread> {
        match refusal(read) {
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

    /// The one-question benchmark in cqa 0.2's shape, its target written
    /// as `targets`.
    fn yaml_targeting(targets: &str) -> String {
        let pair = "target_dataset: d\ntarget_dataset_version: '1'\n";
        yaml_question_with(pair, &format!("target_datasets:\n{targets}"))
    }

    /// `data` read against cqa 0.2.
    fn from_yaml_0_2(data: &str) -> Result<Benchmark> {
        Benchmark::from_loaded(&loaded_against("cqa-0.2/cqa.yaml", data))
    }

    fn from_yaml_targeting(targets: &str) -> Result<Benchmark> {
        from_yaml_0_2(&yaml_targeting(targets))
    }

    const YAML_TARGET: &str = "  - dataset: d\n    version: '1'\n";

    #[test]
    fn a_cqa_0_2_yaml_target_names_the_graph() {
        let b = from_yaml_targeting(YAML_TARGET).unwrap();
        assert_eq!(b.target_graph(), crate::trace::tests::graph());
    }

    #[test]
    fn cqa_0_2s_closed_world_kind_reads_from_yaml() {
        let data = yaml_targeting(YAML_TARGET)
            .replace("[lookup]", "[closed_world_negative]")
            .replace("[r1]", "[r1]\n    unconnected_anchors: [r1]");
        let b = from_yaml_0_2(&data).unwrap();
        assert_eq!(
            b.question("q1").unwrap().answer_kinds,
            BTreeSet::from([AnswerKind::ClosedWorldNegative])
        );
    }

    #[test]
    fn a_cqa_0_2_yaml_benchmark_keeping_cqa_0_1s_pair_is_refused() {
        // cqa 0.2 declares no `target_dataset`, so the load reads past it
        // and the benchmark is refused before its target is chosen.
        let data = yaml_targeting(YAML_TARGET).replacen("id: b\n", "id: b\ntarget_dataset: d\n", 1);
        assert_eq!(
            refusal(from_yaml_0_2(&data)),
            Refusal::PartlyRead(vec![Unread::UndeclaredField {
                record: "b".into(),
                field: "target_dataset".into(),
                class: "Benchmark".into(),
            }])
        );
    }

    #[test]
    fn a_cqa_0_2_yaml_benchmark_targeting_two_datasets_is_refused() {
        let two = format!("{YAML_TARGET}  - dataset: e\n    version: '1'\n");
        assert_eq!(
            refusal(from_yaml_targeting(&two)),
            Refusal::SeveralTargetDatasets {
                benchmark: "b".into(),
                count: 2,
            }
        );
    }

    #[test]
    fn yaml_and_turtle_readers_agree_on_a_cqa_0_2_benchmark() {
        let ttl = format!(
            "@prefix cqa: <{CQA}> . @prefix pav: <http://purl.org/pav/> .
             cqa:b a cqa:Benchmark ; cqa:id \"b\" ;
               cqa:target_schema \"https://example.org/s/\" ; cqa:target_schema_version \"1\" ;
               cqa:target_datasets {TARGET} ;
               cqa:competency_question_answers <{CQA}b/q1> .
             <{CQA}b/q1> cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ;
               cqa:answer_kind <{CQA}AnswerKindEnum#lookup> ;
               cqa:expected_anchors <https://example.org/s/r1> ."
        );
        assert_eq!(
            from_yaml_targeting(YAML_TARGET).unwrap(),
            Benchmark::from_turtle(&ttl).unwrap()
        );
    }

    /// The test benchmarks' target schema, which bare records expand against.
    const S: &str = "https://example.org/s/";

    /// Two records ranked at `rank`.
    const TWO: [(&str, &str, &str); 2] = [("a", "rank", "1"), ("b", "rank", "2")];

    /// The one-question cqa 0.2 benchmark, ordered by `rank` over
    /// `values`, each a record and its value at `slot`.
    fn yaml_ordered(values: &[(&str, &str, &str)]) -> String {
        yaml_key(values, &["rank"])
    }

    /// `yaml_ordered`, ordered by each of `ordered_by`.
    fn yaml_key(values: &[(&str, &str, &str)], ordered_by: &[&str]) -> String {
        let anchors: Vec<&str> = values.iter().map(|(r, _, _)| *r).collect();
        let entries: String = values
            .iter()
            .map(|(r, slot, v)| {
                format!("      - record: {r}\n        slot: {slot}\n        value: '{v}'\n")
            })
            .collect();
        let ordered_by = match ordered_by {
            [] => String::new(),
            [one] => format!("    ordered_by: {one}\n"),
            many => format!("    ordered_by: [{}]\n", many.join(", ")),
        };
        yaml_targeting(YAML_TARGET).replace(
            "    expected_anchors: [r1]\n",
            &format!(
                "    expected_anchors: [{}]\n    expected_values:\n{entries}{ordered_by}",
                anchors.join(", ")
            ),
        )
    }

    /// The same benchmark as `yaml_ordered(values)`, as panschema writes
    /// it in Turtle.
    fn turtle_ordered(values: &[(&str, &str, &str)]) -> String {
        turtle_key(values, &["rank"])
    }

    /// `turtle_ordered`, ordered by each of `ordered_by`.
    fn turtle_key(values: &[(&str, &str, &str)], ordered_by: &[&str]) -> String {
        let anchors: Vec<String> = values.iter().map(|(r, _, _)| format!("<{S}{r}>")).collect();
        let entries: Vec<String> = values
            .iter()
            .map(|(r, slot, v)| {
                format!(
                    "[ a cqa:ExpectedValue ; cqa:record <{S}{r}> ; cqa:slot \"{slot}\" ; cqa:value \"{v}\" ]"
                )
            })
            .collect();
        let ordered_by: String = ordered_by
            .iter()
            .map(|slot| format!(" ; cqa:ordered_by \"{slot}\""))
            .collect();
        format!(
            "@prefix cqa: <{CQA}> . @prefix pav: <http://purl.org/pav/> .
             cqa:b a cqa:Benchmark ; cqa:id \"b\" ;
               cqa:target_schema \"{S}\" ; cqa:target_schema_version \"1\" ;
               cqa:target_datasets {TARGET} ;
               cqa:competency_question_answers <{CQA}b/q1> .
             <{CQA}b/q1> cqa:id \"q1\" ; cqa:question \"one?\" ; cqa:ground_truth \"One.\" ;
               cqa:answer_kind <{CQA}AnswerKindEnum#lookup> ;
               cqa:expected_anchors {} ;
               cqa:expected_values {}{ordered_by} .",
            anchors.join(", "),
            entries.join(", ")
        )
    }

    /// The order both readers give `values`, which they must agree on.
    fn order(values: &[(&str, &str, &str)]) -> Order {
        let yaml = from_yaml_0_2(&yaml_ordered(values)).unwrap();
        let turtle = Benchmark::from_turtle(&turtle_ordered(values)).unwrap();
        assert_eq!(yaml, turtle, "the readers disagree on {values:?}");
        yaml.questions[0].order.clone().unwrap()
    }

    /// `records` as one tier of IRIs into the test benchmark's target.
    fn tier(records: &[&str]) -> BTreeSet<String> {
        records.iter().map(|r| format!("{S}{r}")).collect()
    }

    /// Why both readers refuse `values`, which must be the same refusal.
    fn order_refused(values: &[(&str, &str, &str)]) -> Refusal {
        let yaml = refusal(from_yaml_0_2(&yaml_ordered(values)));
        assert_eq!(
            yaml,
            turtle_refused(&turtle_ordered(values)),
            "the readers refuse {values:?} differently"
        );
        yaml
    }

    #[test]
    fn an_order_ranks_its_records_by_their_values() {
        // Listed out of order, as cqa's own example lists them.
        let order = order(&[
            ("a", "rank", "0.9"),
            ("b", "rank", "0.7"),
            ("c", "rank", "0.85"),
        ]);
        assert_eq!(order.slot, "rank");
        assert_eq!(order.tiers, [tier(&["b"]), tier(&["c"]), tier(&["a"])]);
    }

    #[test]
    fn records_whose_values_are_equal_share_a_tier() {
        // `1` and `1.0` are one number, though not one string.
        let order = order(&[("a", "rank", "1"), ("b", "rank", "2"), ("c", "rank", "1.0")]);
        assert_eq!(order.tiers, [tier(&["a", "c"]), tier(&["b"])]);
    }

    #[test]
    fn an_order_ranks_by_number_not_by_text() {
        let order = order(&[("a", "rank", "10"), ("b", "rank", "9"), ("c", "rank", "-1")]);
        assert_eq!(order.tiers, [tier(&["c"]), tier(&["b"]), tier(&["a"])]);
    }

    #[test]
    fn values_at_another_slot_are_not_part_of_the_order() {
        // `verdict`'s text would be refused if the order read it.
        let order = order(&[
            ("a", "rank", "2"),
            ("b", "rank", "1"),
            ("c", "verdict", "average"),
        ]);
        assert_eq!(order.tiers, [tier(&["b"]), tier(&["a"])]);
    }

    #[test]
    fn values_that_differ_past_a_floats_precision_do_not_tie() {
        // Both parse to the same f64.
        let order = order(&[
            ("a", "rank", "9007199254740993"),
            ("b", "rank", "9007199254740992"),
        ]);
        assert_eq!(order.tiers, [tier(&["b"]), tier(&["a"])]);
    }

    #[test]
    fn decimals_compare_exactly() {
        let ascending = [
            "-1e400",
            "-10",
            "-9.5",
            "-0.1",
            "0",
            "1e-400",
            "0.1",
            "0.10000000000000001",
            ".5",
            "9",
            "9007199254740992",
            "9007199254740993",
            "1e400",
        ];
        let parsed: Vec<Decimal> = ascending
            .iter()
            .map(|t| Decimal::parse(t).unwrap_or_else(|| panic!("{t:?} is a number")))
            .collect();
        for (pair, text) in parsed.windows(2).zip(ascending.windows(2)) {
            assert!(pair[0] < pair[1], "{} < {}", text[0], text[1]);
        }
    }

    #[test]
    fn one_number_written_differently_is_one_value() {
        for same in [
            &["1", "1.0", "+1", "1.", "1e0", "0.1e1", "10E-1", "001"][..],
            &["0", "-0", "0.000", "0e5", ".0"][..],
        ] {
            let first = Decimal::parse(same[0]).unwrap();
            for text in same {
                assert_eq!(Decimal::parse(text).as_ref(), Some(&first), "{text}");
            }
        }
    }

    #[test]
    fn a_question_with_no_order_has_none() {
        let yaml = from_yaml_0_2(&yaml_key(&TWO, &[])).unwrap();
        let ttl = turtle_key(&TWO, &[]);
        assert_eq!(yaml.questions[0].order, None);
        assert_eq!(
            Benchmark::from_turtle(&ttl).unwrap().questions[0].order,
            None,
            "{ttl}"
        );
    }

    /// `data`, with a bare string `zz` where its first expected value
    /// belongs. panschema writes one into the Turtle as an IRI.
    fn with_a_bare_value(data: &str) -> String {
        let (yaml, ttl) = ("    expected_values:\n", "cqa:expected_values ");
        if data.contains(yaml) {
            data.replacen(yaml, &format!("{yaml}      - zz\n"), 1)
        } else {
            data.replacen(ttl, &format!("{ttl}cqa:zz, "), 1)
        }
    }

    #[test]
    fn the_expected_values_of_a_question_with_no_order_are_not_read() {
        // A bare string where an entry belongs is refused when it is read.
        let yaml = with_a_bare_value(&yaml_key(&TWO, &[]));
        let ttl = with_a_bare_value(&turtle_key(&TWO, &[]));
        assert!(from_yaml_0_2(&yaml).is_ok(), "{yaml}");
        assert!(Benchmark::from_turtle(&ttl).is_ok(), "{ttl}");
    }

    #[test]
    fn a_value_at_the_ordering_slot_that_is_not_a_number_is_refused_naming_it() {
        for value in [
            "first",
            "NaN",
            "inf",
            "",
            "1e",
            "0x10",
            " 1",
            "1,5",
            "--1",
            ".",
            "1e9999999999999999999",
        ] {
            assert_eq!(
                order_refused(&[("a", "rank", "1"), ("b", "rank", value)]),
                Refusal::NotANumber {
                    question: "q1".into(),
                    slot: "rank".into(),
                    value: value.into(),
                },
                "{value:?}"
            );
        }
    }

    #[test]
    fn an_order_with_fewer_than_two_records_at_its_slot_is_refused() {
        // Values at another slot do not count, and nor does one record
        // listed twice.
        for values in [
            &[("a", "rank", "1"), ("b", "verdict", "2")][..],
            &[("a", "rank", "1"), ("a", "rank", "1")][..],
        ] {
            assert_eq!(
                order_refused(values),
                Refusal::NothingToOrder {
                    question: "q1".into(),
                    slot: "rank".into(),
                },
                "{values:?}"
            );
        }
    }

    #[test]
    fn a_record_with_two_values_at_the_ordering_slot_is_refused() {
        assert_eq!(
            order_refused(&[("a", "rank", "1"), ("b", "rank", "2"), ("a", "rank", "3")]),
            Refusal::TwoValuesToOrderBy {
                question: "q1".into(),
                record: format!("{S}a"),
            }
        );
    }

    #[test]
    fn an_expected_value_missing_a_part_is_refused_naming_it() {
        for (slot, yaml_from, yaml_to, ttl_from, ttl_to) in [
            (
                "record",
                "record: b\n        ",
                "",
                format!("cqa:record <{S}b> ;"),
                String::new(),
            ),
            (
                "slot",
                "slot: rank\n        value: '2'",
                "value: '2'",
                "cqa:slot \"rank\" ; cqa:value \"2\"".into(),
                "cqa:value \"2\"".into(),
            ),
            (
                "value",
                "\n        value: '2'",
                "",
                "; cqa:value \"2\"".into(),
                String::new(),
            ),
        ] {
            let missing = Refusal::missing("q1's expected value", slot);
            let yaml = yaml_ordered(&TWO).replacen(yaml_from, yaml_to, 1);
            assert_eq!(refusal(from_yaml_0_2(&yaml)), missing, "{yaml}");
            let ttl = turtle_ordered(&TWO).replacen(&ttl_from, &ttl_to, 1);
            assert_eq!(turtle_refused(&ttl), missing, "{ttl}");
        }
    }

    #[test]
    fn an_expected_value_written_as_a_bare_string_is_refused() {
        // The Turtle holds the IRI panschema minted, not the string.
        assert_eq!(
            refusal(from_yaml_0_2(&with_a_bare_value(&yaml_ordered(&TWO)))),
            Refusal::NotAnExpectedValue("zz".into())
        );
        assert_eq!(
            turtle_refused(&with_a_bare_value(&turtle_ordered(&TWO))),
            Refusal::NotAnExpectedValue(format!("<{CQA}zz>"))
        );
    }

    #[test]
    fn a_turtle_expected_value_written_as_a_named_node_is_an_entry() {
        let blank = format!(
            "[ a cqa:ExpectedValue ; cqa:record <{S}a> ; cqa:slot \"rank\" ; cqa:value \"1\" ]"
        );
        let ttl = turtle_ordered(&TWO);
        assert!(ttl.contains(&blank), "{ttl}");
        let named = format!(
            "{}\ncqa:e1 cqa:record <{S}a> ; cqa:slot \"rank\" ; cqa:value \"1\" .",
            ttl.replacen(&blank, "cqa:e1", 1)
        );
        let read = Benchmark::from_turtle(&named).unwrap();
        assert_eq!(
            read.questions[0].order.as_ref().map(|o| &o.tiers[..]),
            Some(&[tier(&["a"]), tier(&["b"])][..]),
            "{named}"
        );
    }

    #[test]
    fn a_record_written_as_a_curie_expands_against_the_schemas_prefixes() {
        let data = yaml_ordered(&[("cqa:a", "rank", "1"), ("b", "rank", "2")]);
        let read = from_yaml_0_2(&data).unwrap();
        assert_eq!(
            read.questions[0].order.as_ref().map(|o| &o.tiers[..]),
            Some(&[BTreeSet::from([format!("{CQA}a")]), tier(&["b"])][..]),
            "{data}"
        );
    }

    #[test]
    fn a_turtle_literal_where_an_expected_value_belongs_is_refused() {
        let ttl = turtle_ordered(&TWO).replacen(
            "cqa:expected_values ",
            "cqa:expected_values \"zz\", ",
            1,
        );
        assert_eq!(
            turtle_refused(&ttl),
            Refusal::NotAnExpectedValue("zz".into())
        );
    }

    #[test]
    fn a_turtle_target_written_as_a_bare_iri_is_refused_naming_it() {
        // As panschema writes a bare string in `target_datasets`.
        assert_eq!(
            turtle_refused(&benchmark_targeting("cqa:d")),
            Refusal::NotATargetDataset(format!("<{CQA}d>"))
        );
    }

    #[test]
    fn a_part_of_a_value_at_another_slot_is_never_refused() {
        // Unquoted in the YAML, and two-valued in the Turtle: each a
        // refusal at the ordering slot.
        let values = [TWO[0], TWO[1], ("c", "vintage", "2018")];
        let yaml = yaml_ordered(&values).replacen("value: '2018'", "value: 2018", 1);
        let ttl = turtle_ordered(&values).replacen("cqa:value \"2018\"", "cqa:value 2018, 2019", 1);
        let expected = [tier(&["a"]), tier(&["b"])];
        let order = |read: Result<Benchmark>| read.unwrap().questions[0].order.clone().unwrap();
        assert_eq!(order(from_yaml_0_2(&yaml)).tiers, expected, "{yaml}");
        assert_eq!(order(Benchmark::from_turtle(&ttl)).tiers, expected, "{ttl}");
    }

    #[test]
    fn an_order_over_a_record_outside_the_anchors_is_refused() {
        let outside = Refusal::RankedOutsideAnchors {
            question: "q1".into(),
            record: format!("{S}b"),
        };
        let yaml =
            yaml_ordered(&TWO).replacen("expected_anchors: [a, b]", "expected_anchors: [a]", 1);
        let ttl = turtle_ordered(&TWO).replacen(&format!(", <{S}b> ;"), " ;", 1);
        assert_eq!(refusal(from_yaml_0_2(&yaml)), outside, "{yaml}");
        assert_eq!(turtle_refused(&ttl), outside, "{ttl}");
    }

    #[test]
    fn two_faults_in_one_question_are_refused_alike_by_both_readers() {
        // The YAML lists the entry with no record first; the store orders
        // the Turtle's as it likes.
        let yaml = yaml_ordered(&TWO)
            .replacen("record: a\n        ", "", 1)
            .replacen("value: '2'", "value: ['2', '3']", 1);
        let ttl = turtle_ordered(&TWO)
            .replacen(&format!("cqa:record <{S}a> ;"), "", 1)
            .replacen("cqa:value \"2\"", "cqa:value \"2\", \"3\"", 1);
        let two = Refusal::MoreThanOne {
            record: "q1's expected value".into(),
            slot: "value",
        };
        assert_eq!(refusal(from_yaml_0_2(&yaml)), two, "{yaml}");
        assert_eq!(turtle_refused(&ttl), two, "{ttl}");
    }

    /// The benchmark `first` with the question of `second` added as `q2`,
    /// in YAML or in Turtle.
    fn with_a_second_question(first: &str, second: &str) -> String {
        let iri = format!("<{CQA}b/q1>");
        let q2 = |question: &str| {
            question
                .replace("id: q1", "id: q2")
                .replace("cqa:id \"q1\"", "cqa:id \"q2\"")
                .replace("b/q1", "b/q2")
        };
        match second.rfind(&format!("{iri} cqa:id")) {
            Some(at) => {
                let listed = format!("cqa:competency_question_answers {iri}");
                assert!(first.contains(&listed), "{first}");
                let both = format!("{listed}, <{CQA}b/q2>");
                format!(
                    "{}\n{}",
                    first.replacen(&listed, &both, 1),
                    q2(&second[at..])
                )
            }
            None => {
                let at = second.find("  - id: q1").expect("a YAML question q1");
                format!("{first}{}", q2(&second[at..]))
            }
        }
    }

    #[test]
    fn of_two_faulty_questions_both_readers_refuse_the_first_by_id() {
        let not_a_number = [("a", "rank", "1"), ("b", "rank", "good")];
        let yaml_number = yaml_ordered(&not_a_number);
        let ttl_number = turtle_ordered(&not_a_number);
        let yaml_twice = yaml_ordered(&TWO).replacen("value: '2'", "value: ['2', '3']", 1);
        let ttl_twice =
            turtle_ordered(&TWO).replacen("cqa:value \"2\"", "cqa:value \"2\", \"3\"", 1);
        let number = Refusal::NotANumber {
            question: "q1".into(),
            slot: "rank".into(),
            value: "good".into(),
        };
        let twice = Refusal::MoreThanOne {
            record: "q1's expected value".into(),
            slot: "value",
        };
        for (yaml, ttl, first) in [
            (
                with_a_second_question(&yaml_number, &yaml_twice),
                with_a_second_question(&ttl_number, &ttl_twice),
                number,
            ),
            (
                with_a_second_question(&yaml_twice, &yaml_number),
                with_a_second_question(&ttl_twice, &ttl_number),
                twice,
            ),
        ] {
            assert_eq!(refusal(from_yaml_0_2(&yaml)), first, "{yaml}");
            assert_eq!(turtle_refused(&ttl), first, "{ttl}");
        }
    }

    #[test]
    fn a_turtle_blank_node_where_text_belongs_is_refused_naming_the_slot() {
        let blank = |record: &str, slot| Refusal::NotText {
            record: record.into(),
            slot,
            kind: "a blank node".into(),
        };
        let ordered_by =
            turtle_ordered(&TWO).replacen("cqa:ordered_by \"rank\"", "cqa:ordered_by []", 1);
        assert_eq!(turtle_refused(&ordered_by), blank("q1", "ordered_by"));
        let record =
            turtle_ordered(&TWO).replacen(&format!("cqa:record <{S}b>"), "cqa:record []", 1);
        assert_eq!(
            turtle_refused(&record),
            blank("q1's expected value", "record")
        );
    }

    #[test]
    fn a_record_that_does_not_expand_in_an_expected_value_is_named_as_the_entry() {
        let data = yaml_ordered(&TWO).replacen(
            "target_schema: https://example.org/s/",
            "target_schema: [https://example.org/s/]",
            1,
        );
        let unread = read_past(from_yaml_0_2(&data));
        let gap = Unread::Unexpanded {
            record: "q1's expected value".into(),
            slot: "record".into(),
            base_slot: "target_schema".into(),
        };
        assert_eq!(
            unread.iter().filter(|u| **u == gap).count(),
            2,
            "{unread:?}"
        );
    }

    #[test]
    fn a_record_named_where_an_entry_belongs_keeps_its_name() {
        // The benchmark's target is the string `q1`, and q1's first
        // expected value the string `b`: neither is an entry, so a field
        // read past on q1 or on b is still q1's or b's.
        let data = yaml_ordered(&TWO)
            .replacen("  - dataset: d\n    version: '1'\n", "  - q1\n", 1)
            .replacen(
                "    expected_values:\n",
                "    expected_values:\n      - b\n",
                1,
            )
            .replacen("id: b\n", "id: b\nnope: x\n", 1)
            .replacen(
                "    question: one?\n",
                "    question: one?\n    nope: x\n",
                1,
            );
        let unread = read_past(from_yaml_0_2(&data));
        for (record, class) in [("b", "Benchmark"), ("q1", "CompetencyQuestionAnswer")] {
            let past = Unread::UndeclaredField {
                record: record.into(),
                field: "nope".into(),
                class: class.into(),
            };
            assert!(unread.contains(&past), "{record}: {unread:?}");
        }
    }

    #[test]
    fn a_question_ordered_by_two_slots_is_refused() {
        let two = Refusal::MoreThanOne {
            record: "q1".into(),
            slot: "ordered_by",
        };
        let slots = ["rank", "position"];
        assert_eq!(refusal(from_yaml_0_2(&yaml_key(&TWO, &slots))), two);
        assert_eq!(turtle_refused(&turtle_key(&TWO, &slots)), two);
    }

    #[test]
    fn an_expected_value_with_two_values_at_a_part_is_refused_naming_it() {
        for (slot, yaml_from, yaml_to, ttl_from, ttl_to) in [
            (
                "record",
                "record: b\n",
                "record: [b, c]\n",
                format!("cqa:record <{S}b>"),
                format!("cqa:record <{S}b>, <{S}c>"),
            ),
            (
                "slot",
                "slot: rank\n        value: '2'",
                "slot: [rank, x]\n        value: '2'",
                "cqa:slot \"rank\" ; cqa:value \"2\"".into(),
                "cqa:slot \"rank\", \"x\" ; cqa:value \"2\"".into(),
            ),
            (
                "value",
                "value: '2'",
                "value: ['2', '3']",
                "cqa:value \"2\"".into(),
                "cqa:value \"2\", \"3\"".into(),
            ),
        ] {
            let two = Refusal::MoreThanOne {
                record: "q1's expected value".into(),
                slot,
            };
            let yaml = yaml_ordered(&TWO).replacen(yaml_from, yaml_to, 1);
            assert_eq!(refusal(from_yaml_0_2(&yaml)), two, "{yaml}");
            let ttl = turtle_ordered(&TWO).replacen(&ttl_from, &ttl_to, 1);
            assert_eq!(turtle_refused(&ttl), two, "{ttl}");
        }
    }

    #[test]
    fn a_misspelled_slot_in_an_expected_value_is_refused_naming_the_entry() {
        let data = yaml_ordered(&TWO).replacen("value: '2'", "vaule: '2'", 1);
        assert_eq!(
            read_past(from_yaml_0_2(&data)),
            [Unread::UndeclaredField {
                record: "q1's expected value".into(),
                field: "vaule".into(),
                class: "ExpectedValue".into(),
            }]
        );
    }

    #[test]
    fn a_target_written_as_a_bare_string_is_refused_naming_it() {
        assert_eq!(
            refusal(from_yaml_targeting("  - d\n")),
            Refusal::NotATargetDataset("d".into())
        );
    }

    #[test]
    fn a_yaml_target_dataset_with_two_versions_is_refused_as_the_turtle_is() {
        let target = YAML_TARGET.replace("version: '1'", "version: ['1', '2']");
        assert_eq!(
            refusal(from_yaml_targeting(&target)),
            Refusal::MoreThanOne {
                record: "b's target dataset".into(),
                slot: "version",
            }
        );
    }

    #[test]
    fn a_stray_field_in_a_yaml_target_dataset_is_refused_naming_the_entry() {
        let target = format!("{YAML_TARGET}    name: x\n");
        assert_eq!(
            refusal(from_yaml_targeting(&target)),
            Refusal::PartlyRead(vec![Unread::UndeclaredField {
                record: "b's target dataset".into(),
                field: "name".into(),
                class: "TargetDataset".into(),
            }])
        );
    }

    #[test]
    fn a_kind_spelled_for_the_other_cqa_version_is_refused() {
        // Each schema permits one spelling, so the other is no kind of
        // it, as panschema's own check of the benchmark says.
        let negative = |kind: &str| {
            yaml_question_with("[lookup]", &format!("[{kind}]"))
                .replace("[r1]", "[r1]\n    unconnected_anchors: [r1]")
        };
        let under_0_1 = refused(&negative("closed_world_negative"));
        let pair = "target_dataset: d\ntarget_dataset_version: '1'\n";
        let moved = negative("closed-world-negative")
            .replace(pair, &format!("target_datasets:\n{YAML_TARGET}"));
        let under_0_2 = refusal(from_yaml_0_2(&moved));
        for (refusal, kind) in [
            (under_0_1, "closed_world_negative"),
            (under_0_2, "closed-world-negative"),
        ] {
            assert_eq!(
                refusal,
                Refusal::UnknownAnswerKind {
                    question: "q1".into(),
                    kind: kind.into(),
                }
            );
        }
    }

    #[test]
    fn cqa_0_1s_closed_world_kind_reads_from_yaml() {
        let data = yaml_question_with("[lookup]", "[closed-world-negative]")
            .replace("[r1]", "[r1]\n    unconnected_anchors: [r1]");
        let b = from_yaml_text(&data).unwrap();
        assert_eq!(
            b.question("q1").unwrap().answer_kinds,
            BTreeSet::from([AnswerKind::ClosedWorldNegative])
        );
    }

    #[test]
    fn a_yaml_target_dataset_without_its_version_is_refused_naming_it() {
        let target = YAML_TARGET.replace("    version: '1'\n", "");
        let refusal = refusal(from_yaml_targeting(&target));
        assert_eq!(refusal, Refusal::missing("b's target dataset", "version"));
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
    fn cqa_0_1s_spelling_of_the_closed_world_kind_still_reads() {
        for sep in ['#', '/'] {
            let iri = format!("{CQA}AnswerKindEnum{sep}closed-world-negative");
            assert_eq!(
                AnswerKind::from_iri(&iri),
                Some(AnswerKind::ClosedWorldNegative),
                "{iri}"
            );
        }
    }

    #[test]
    fn a_report_spells_the_closed_world_kind_as_cqa_0_2_does() {
        assert_eq!(
            serde_json::to_string(&AnswerKind::ClosedWorldNegative).unwrap(),
            "\"closed_world_negative\""
        );
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
