//! The SPARQL plumbing the benchmark reader and the baseline share.

use std::collections::BTreeMap;
use std::path::Path;

use oxigraph::io::RdfFormat;
use oxigraph::model::Term;
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;

use crate::{Error, Result};

/// An in-memory store holding the Turtle text.
pub(crate) fn load_turtle(turtle: &str) -> Result<Store> {
    let store = Store::new().map_err(Error::rdf)?;
    add_turtle(&store, turtle)?;
    Ok(store)
}

/// An in-memory store holding every Turtle file, as one graph. A file
/// that fails to read or parse is named in the error.
pub(crate) fn load_turtle_files<P: AsRef<Path>>(
    paths: impl IntoIterator<Item = P>,
) -> Result<Store> {
    let store = Store::new().map_err(Error::rdf)?;
    for path in paths {
        let path = path.as_ref();
        let load = || add_turtle(&store, &std::fs::read_to_string(path)?);
        load().map_err(|e| Error::File {
            path: path.to_owned(),
            source: Box::new(e),
        })?;
    }
    Ok(store)
}

/// Add Turtle text to a store. Blank-node labels stay local to the text,
/// as Turtle scopes them, so two files that both say `_:b0` do not merge.
pub(crate) fn add_turtle(store: &Store, turtle: &str) -> Result<()> {
    store
        .load_from_slice(RdfFormat::Turtle, turtle)
        .map_err(Error::rdf)
}

/// One SPARQL solution, with every IRI and literal flattened to a string.
/// Blank nodes are left out: nothing graded can name one.
pub(crate) struct Row(BTreeMap<String, String>);

impl Row {
    pub(crate) fn get(&self, var: &str) -> Option<String> {
        self.0.get(var).cloned()
    }

    pub(crate) fn require(&self, var: &str) -> Result<String> {
        self.get(var)
            .ok_or_else(|| Error::Rdf(format!("query solution lacks ?{var}")))
    }
}

/// Run a SELECT query and flatten its solutions.
pub(crate) fn select(store: &Store, query: &str) -> Result<Vec<Row>> {
    let results = SparqlEvaluator::new()
        .parse_query(query)
        .map_err(Error::rdf)?
        .on_store(store)
        .execute()
        .map_err(Error::rdf)?;
    let QueryResults::Solutions(solutions) = results else {
        return Err(Error::Rdf("expected a SELECT query".into()));
    };
    let mut rows = Vec::new();
    for solution in solutions {
        let solution = solution.map_err(Error::rdf)?;
        let row = solution
            .iter()
            .filter_map(|(var, term)| {
                let value = match term {
                    Term::NamedNode(n) => n.as_str().to_string(),
                    Term::Literal(l) => l.value().to_string(),
                    _ => return None,
                };
                Some((var.as_str().to_string(), value))
            })
            .collect();
        rows.push(Row(row));
    }
    Ok(rows)
}
