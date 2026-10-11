//! Shared by the integration tests.

use std::path::PathBuf;

/// A generated fixture's path. The fixtures are not vendored:
/// `panschema generate --strict` in the repo root writes them from sibling
/// checkouts of ontology-authoring-template and cqa-schema (see
/// `panschema.toml`).
pub fn fixture_path(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    assert!(
        path.exists(),
        "{} is missing: run `panschema generate --strict` in the repo root first",
        path.display()
    );
    path
}
