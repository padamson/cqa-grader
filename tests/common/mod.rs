//! Shared by the integration tests.

use std::path::PathBuf;

/// A committed fixture's path. `scripts/regen-fixtures.sh` generates them
/// from sibling checkouts of ontology-authoring-template and cqa-schema
/// (see `panschema.toml`).
pub fn fixture_path(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    assert!(
        path.exists(),
        "{} is missing: it is committed, so restore it with `git checkout`",
        path.display()
    );
    path
}

/// A committed fixture's text.
pub fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name)).unwrap()
}
