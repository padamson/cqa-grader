//! The build's version identity, the one place the site's path prefix is
//! spelled, and the fetch of the files the deploy shares between versions.
//! `build.rs` injects `SITE_VERSION`; the deploy serves every snapshot
//! under `/cqa-grader/<dev|vX.Y.Z>/` (project Pages), and only what reads a
//! shared file (the version switcher's manifest, the roadmap's data) needs
//! to know that: every other asset path is relative.

/// The snapshot identifier: `"dev"` for the main-HEAD build, or the
/// release version (e.g. `"0.1.0"`).
pub const SITE_VERSION: &str = env!("SITE_VERSION");

/// The path the site is served under. `""` would be a custom domain at
/// the apex; project Pages is `/cqa-grader`.
pub const SITE_PREFIX: &str = "/cqa-grader";

/// Whether this is the unreleased main-HEAD (`dev`) build.
pub fn is_dev() -> bool {
    SITE_VERSION == "dev"
}

/// Where the deploy writes the manifest of published versions.
pub fn manifest_url() -> String {
    format!("{SITE_PREFIX}/versions.json")
}

/// A JSON file the deploy shares between versions, or `None` when it
/// cannot be fetched or read (no deploy yet, a local preview, a shape this
/// build does not know): every caller has a fallback of its own.
pub async fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Option<T> {
    let response = gloo_net::http::Request::get(url).send().await.ok()?;
    if !response.ok() {
        return None;
    }
    response.json::<T>().await.ok()
}

/// The root of a snapshot: `dev` or a version number.
pub fn snapshot_url(value: &str) -> String {
    match value {
        "dev" => format!("{SITE_PREFIX}/dev/"),
        v => format!("{SITE_PREFIX}/v{v}/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_and_snapshots_live_under_the_prefix() {
        assert_eq!(manifest_url(), "/cqa-grader/versions.json");
        assert_eq!(snapshot_url("dev"), "/cqa-grader/dev/");
        assert_eq!(snapshot_url("0.1.0"), "/cqa-grader/v0.1.0/");
    }
}
