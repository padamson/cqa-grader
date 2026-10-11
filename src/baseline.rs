//! A deliberately naive retriever, so a benchmark's CI has a student.
//!
//! The baseline does three plain things and nothing clever:
//!
//! 1. **Link** the question's words to records by label. A record whose
//!    whole `rdfs:label` appears in the question is an entry point. So is
//!    a record that shares a word (four letters or more, not a stopword)
//!    with the question, unless some record's whole label matched that
//!    same word, in which case only the whole-label matches count. A class
//!    whose label appears in the question contributes every instance of
//!    that class.
//! 2. **Expand** each entry point by `hops` rounds of a one-hop SPARQL
//!    lookup, in both directions, over named individuals only. One round
//!    reaches every anchor in wine's benchmark; two reaches most of the
//!    graph.
//! 3. **Cite** everything it retrieved.
//!
//! It asserts absence when the question named two or more records and no
//! record joins them: no direct edge between any two, and no third record
//! adjacent to both. That is the shape of cqa's own absence claim, checked
//! against the graph rather than assumed.
//!
//! None of this is a retrieval strategy anyone should ship. It exists so
//! that `expected_anchors` can be shown reachable by lookup alone, and so
//! that a real retriever has a precision floor to clear.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use oxigraph::store::Store;

use crate::Result;
use crate::benchmark::Benchmark;
use crate::rdf::{load_turtle, load_turtle_files, select};
use crate::trace::{Attempt, System, Trace};

/// Words that never link a question to a record on their own.
const STOPWORDS: &[&str] = &[
    "about", "after", "also", "best", "better", "between", "change", "choice", "could", "does",
    "each", "from", "good", "have", "into", "kind", "kinds", "list", "many", "more", "most",
    "much", "name", "other", "over", "same", "should", "some", "specific", "such", "than", "that",
    "their", "them", "then", "there", "these", "they", "this", "those", "under", "well", "were",
    "what", "when", "where", "which", "while", "with", "would", "your",
];

/// The naive retriever, over one loaded graph (which may have come from
/// several files).
pub struct Baseline {
    store: Store,
    /// Named individuals, with their label's normalized tokens.
    individuals: Vec<Labelled>,
    /// Classes, with their label's normalized tokens and their instances.
    classes: Vec<Class>,
}

struct Labelled {
    iri: String,
    tokens: Vec<String>,
}

struct Class {
    tokens: Vec<String>,
    instances: BTreeSet<String>,
}

impl Baseline {
    /// Load a graph from one or more Turtle files, into one store. A graph
    /// published one file per dataset is read as the union of its files.
    /// The T-box triples each file repeats are a set and load once, except
    /// blank-node structure (restrictions, lists), which loads once per
    /// file; no blank node is ever a record, so retrieval is unchanged.
    pub fn from_paths<P: AsRef<Path>>(paths: impl IntoIterator<Item = P>) -> Result<Self> {
        Self::from_store(load_turtle_files(paths)?)
    }

    /// Load a graph from Turtle text. Records are whatever is declared an
    /// `owl:NamedIndividual` with an `rdfs:label`, which is how panschema
    /// emits instance data.
    pub fn from_turtle(turtle: &str) -> Result<Self> {
        Self::from_store(load_turtle(turtle)?)
    }

    fn from_store(store: Store) -> Result<Self> {
        let mut individuals = Vec::new();
        for row in select(
            &store,
            "PREFIX owl: <http://www.w3.org/2002/07/owl#>
             PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
             SELECT ?s ?label WHERE {
               ?s a owl:NamedIndividual ; rdfs:label ?label .
               FILTER(isIRI(?s))
             }",
        )? {
            individuals.push(Labelled {
                iri: row.require("s")?,
                tokens: tokens(&row.require("label")?),
            });
        }

        let mut classes: BTreeMap<String, Class> = BTreeMap::new();
        for row in select(
            &store,
            "PREFIX owl: <http://www.w3.org/2002/07/owl#>
             PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
             SELECT ?c ?label ?i WHERE {
               ?c a owl:Class ; rdfs:label ?label .
               ?i a ?c ; a owl:NamedIndividual .
               FILTER(isIRI(?c) && isIRI(?i))
             }",
        )? {
            let iri = row.require("c")?;
            let instance = row.require("i")?;
            classes
                .entry(iri)
                .or_insert_with(|| Class {
                    tokens: tokens(&split_camel(&row.get("label").unwrap_or_default())),
                    instances: BTreeSet::new(),
                })
                .instances
                .insert(instance);
        }

        Ok(Baseline {
            store,
            individuals,
            classes: classes.into_values().collect(),
        })
    }

    /// Run every question in a benchmark and collect the attempts into a trace.
    ///
    /// Only each question's `id` and `question` text are read. The key
    /// (anchors, citations, kinds) is never consulted.
    pub fn run(&self, benchmark: &Benchmark, hops: usize) -> Result<Trace> {
        let mut trace = Trace::new(
            benchmark.id.clone(),
            System {
                name: "cqa-grader baseline".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                notes: Some(format!(
                    "lexical entity linking, {hops}-hop lookup, cites everything retrieved"
                )),
            },
        );
        for q in &benchmark.questions {
            trace
                .attempts
                .push(self.retrieve(&q.id, &q.question, hops)?);
        }
        Ok(trace)
    }

    /// Attempt one question.
    pub fn retrieve(&self, question_id: &str, question: &str, hops: usize) -> Result<Attempt> {
        let (named, by_class) = self.link(question);

        let mut retrieved: BTreeSet<String> = named.union(&by_class).cloned().collect();
        let mut frontier = retrieved.clone();
        for _ in 0..hops {
            if frontier.is_empty() {
                break;
            }
            let next = self.one_hop(&frontier)?;
            frontier = next.difference(&retrieved).cloned().collect();
            retrieved.extend(frontier.iter().cloned());
        }

        Ok(Attempt {
            question: question_id.to_string(),
            cited: retrieved.clone(),
            asserts_absence: self.unjoined(&named)?,
            retrieved,
            answer: None,
        })
    }

    /// Entry points: records the question names, and instances of classes
    /// it names.
    fn link(&self, question: &str) -> (BTreeSet<String>, BTreeSet<String>) {
        let q_tokens = tokens(question);
        let mut named = BTreeSet::new();
        for token in q_tokens
            .iter()
            .filter(|t| t.len() >= 4 && !STOPWORDS.contains(&t.as_str()))
        {
            let candidates: Vec<&Labelled> = self
                .individuals
                .iter()
                .filter(|l| l.tokens.contains(token))
                .collect();
            let whole: Vec<&Labelled> = candidates
                .iter()
                .copied()
                .filter(|l| contains_phrase(&q_tokens, &l.tokens))
                .collect();
            let chosen = if whole.is_empty() { candidates } else { whole };
            named.extend(chosen.into_iter().map(|l| l.iri.clone()));
        }

        let by_class = self
            .classes
            .iter()
            .filter(|c| contains_phrase(&q_tokens, &c.tokens))
            .flat_map(|c| c.instances.iter().cloned())
            .collect();
        (named, by_class)
    }

    /// Named individuals one edge away from any of `nodes`, either direction.
    fn one_hop(&self, nodes: &BTreeSet<String>) -> Result<BTreeSet<String>> {
        let values: Vec<String> = nodes.iter().map(|n| format!("<{n}>")).collect();
        let rows = select(
            &self.store,
            &format!(
                "PREFIX owl: <http://www.w3.org/2002/07/owl#>
                 SELECT DISTINCT ?n WHERE {{
                   VALUES ?s {{ {} }}
                   {{ ?s ?p ?n }} UNION {{ ?n ?p ?s }}
                   ?n a owl:NamedIndividual .
                   FILTER(isIRI(?n))
                 }}",
                values.join(" ")
            ),
        )?;
        rows.into_iter().map(|r| r.require("n")).collect()
    }

    /// True when the named entry points fall into more than one group,
    /// where two entries are in one group if a direct edge or a third
    /// record adjacent to both joins them. "Seafood" may link both the
    /// dish and a recommendation that names it; those two are one group,
    /// and the question is whether the wine reaches that group.
    fn unjoined(&self, named: &BTreeSet<String>) -> Result<bool> {
        if named.len() < 2 {
            return Ok(false);
        }
        let entries: Vec<&String> = named.iter().collect();
        let mut neighbours: Vec<BTreeSet<String>> = Vec::with_capacity(entries.len());
        for n in &entries {
            neighbours.push(self.one_hop(&BTreeSet::from([(*n).clone()]))?);
        }
        // one_hop runs in both directions, so a direct edge shows in
        // either entry's neighbours.
        let n = entries.len();
        let joined: Vec<(usize, usize)> = (0..n)
            .flat_map(|i| (0..n).map(move |j| (i, j)))
            .filter(|&(i, j)| {
                neighbours[i].contains(entries[j].as_str())
                    || !neighbours[i].is_disjoint(&neighbours[j])
            })
            .collect();
        // Every joined pair takes the smaller of its two labels. A path is
        // at most n records long, so n rounds carry a label along any of
        // them; bounded loops, so no edit to them can spin.
        let mut label: Vec<usize> = (0..n).collect();
        for _ in 0..n {
            for &(i, j) in &joined {
                let low = label[i].min(label[j]);
                label[i] = low;
                label[j] = low;
            }
        }
        let groups: BTreeSet<usize> = label.into_iter().collect();
        Ok(groups.len() > 1)
    }
}

/// Lowercase alphanumeric words, plural `-s` stripped from longer words.
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| singular(&w.to_lowercase()))
        .collect()
}

fn singular(word: &str) -> String {
    if word.len() > 4 && word.ends_with('s') && !word.ends_with("ss") {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

/// `PairingRecommendation` → `Pairing Recommendation`.
fn split_camel(label: &str) -> String {
    let mut out = String::with_capacity(label.len() + 4);
    let mut prev_lower = false;
    for c in label.chars() {
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        out.push(c);
    }
    out
}

fn contains_phrase(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use crate::rdf::add_turtle;

    #[test]
    fn tokens_lowercase_split_and_singularize() {
        assert_eq!(
            tokens("What were good vintages for Napa Zinfandel?"),
            [
                "what",
                "were",
                "good",
                "vintage",
                "for",
                "napa",
                "zinfandel"
            ]
        );
        assert_eq!(tokens("Seafood dish"), ["seafood", "dish"]);
        assert_eq!(tokens("glass"), ["glass"]);
        // Four letters is not longer: `ribs` keeps its s.
        assert_eq!(tokens("ribs"), ["ribs"]);
    }

    const PREFIXES: &str = "@prefix owl: <http://www.w3.org/2002/07/owl#> .
         @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
         @prefix ex: <http://example.org/> .";

    const RECORDS: &str = "ex:alpha a owl:NamedIndividual ; rdfs:label \"Alpha\" .
         ex:bravo a owl:NamedIndividual ; rdfs:label \"Bravo\" .
         ex:xray a owl:NamedIndividual ; rdfs:label \"Xray\" .
         ex:red a owl:NamedIndividual ; rdfs:label \"Red\" .";

    fn graph(triples: &str) -> Baseline {
        Baseline::from_turtle(&format!("{PREFIXES} {RECORDS} {triples}")).unwrap()
    }

    const BOTH: &str = "Does Alpha pair with Bravo?";

    #[test]
    fn two_named_records_with_nothing_between_them_are_unjoined() {
        let a = graph("").retrieve("q", BOTH, 1).unwrap();
        assert!(a.asserts_absence, "{a:?}");
    }

    #[test]
    fn a_direct_edge_joins_two_named_records() {
        let a = graph("ex:alpha ex:p ex:bravo .")
            .retrieve("q", BOTH, 1)
            .unwrap();
        assert!(!a.asserts_absence, "{a:?}");
    }

    #[test]
    fn a_record_adjacent_to_both_joins_them() {
        let a = graph("ex:alpha ex:p ex:xray . ex:bravo ex:p ex:xray .")
            .retrieve("q", BOTH, 1)
            .unwrap();
        assert!(!a.asserts_absence, "{a:?}");
    }

    #[test]
    fn a_chain_of_named_records_is_one_group() {
        // Alpha and Bravo share nothing, but each touches Xray, which the
        // question also names.
        let a = graph("ex:alpha ex:p ex:xray . ex:xray ex:p ex:bravo .")
            .retrieve("q", "Does Alpha reach Bravo through Xray?", 0)
            .unwrap();
        assert!(!a.asserts_absence, "{a:?}");
    }

    #[test]
    fn a_split_graph_retrieves_like_the_whole() {
        // The records in one file, the class and every edge in the other,
        // so linking by class, expansion, and the absence check each have
        // to read across the split.
        let edges = "ex:Colour a owl:Class ; rdfs:label \"Colour\" .
             ex:red a ex:Colour .
             ex:alpha ex:near ex:xray .
             ex:xray ex:near ex:red .";
        let whole = graph(edges);
        let store = load_turtle(&format!("{PREFIXES} {RECORDS}")).unwrap();
        add_turtle(&store, &format!("{PREFIXES} {edges}")).unwrap();
        let split = Baseline::from_store(store).unwrap();
        for (question, hops) in [
            ("Tell me about Alpha.", 2),
            ("Which colour is it?", 1),
            (BOTH, 1),
        ] {
            let expected = whole.retrieve("q", question, hops).unwrap();
            assert_eq!(
                split.retrieve("q", question, hops).unwrap(),
                expected,
                "{question:?} at {hops} hops"
            );
        }
        // Not vacuous: the edges in the second file carry Alpha two hops.
        let a = split.retrieve("q", "Tell me about Alpha.", 2).unwrap();
        assert!(a.retrieved.contains("http://example.org/red"), "{a:?}");
    }

    #[test]
    fn a_missing_graph_file_is_named() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let missing = root.join("no-such-graph.ttl");
        let err = Baseline::from_paths([root.join("wine.ttl"), missing.clone()])
            .err()
            .expect("a missing file must not load");
        match err {
            Error::File { path, source } => {
                assert_eq!(path, missing);
                assert!(matches!(*source, Error::Io(_)), "{source:?}");
            }
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn one_named_record_asserts_nothing() {
        let a = graph("").retrieve("q", "Tell me about Alpha.", 1).unwrap();
        assert!(!a.asserts_absence, "{a:?}");
    }

    #[test]
    fn words_under_four_letters_never_link() {
        let a = graph("").retrieve("q", "Is red good?", 0).unwrap();
        assert_eq!(a.retrieved, BTreeSet::new());
    }

    #[test]
    fn a_blank_node_is_never_a_record() {
        let a = graph("ex:alpha ex:p [ a owl:NamedIndividual ; rdfs:label \"Alpha\" ] .")
            .retrieve("q", "Tell me about Alpha.", 1)
            .unwrap();
        assert_eq!(
            a.retrieved,
            BTreeSet::from(["http://example.org/alpha".to_string()])
        );
    }

    #[test]
    fn camel_case_splits_into_words() {
        assert_eq!(
            split_camel("PairingRecommendation"),
            "Pairing Recommendation"
        );
        assert_eq!(split_camel("Wine"), "Wine");
        assert_eq!(split_camel("VintageYear"), "Vintage Year");
    }

    #[test]
    fn phrase_containment_is_contiguous() {
        let hay = tokens("Does Cabernet Sauvignon go well with seafood?");
        assert!(contains_phrase(&hay, &tokens("Cabernet Sauvignon")));
        assert!(!contains_phrase(&hay, &tokens("Cabernet seafood")));
        assert!(!contains_phrase(&hay, &[]));
    }
}
