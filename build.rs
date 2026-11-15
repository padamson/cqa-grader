//! Makes `--version` name the commit a binary was built from when it is
//! not the tagged release, so a build from `main` (`cargo install --git`)
//! can be told from the release it follows. No dirty-tree marker: nothing
//! re-runs this script on an uncommitted edit, so it would be wrong as
//! often as right.

#[path = "build/build_id.rs"]
mod build_id;

use std::path::Path;
use std::process::{Command, Stdio};

fn main() {
    let version = env!("CARGO_PKG_VERSION");
    // With no rerun-if-changed at all, cargo re-runs this script whenever
    // any file in the package changes.
    println!("cargo:rerun-if-changed=build.rs");

    // Only this crate's own checkout (`.git` is a file in a worktree).
    // Without the check, git walks up to any repository around the crate,
    // a home directory under git or a project that vendors it, and reports
    // that one's HEAD.
    let in_checkout = Path::new(".git").exists();
    let Some(sha) = in_checkout
        .then(|| git(&["rev-parse", "--short", "HEAD"]))
        .flatten()
    else {
        if in_checkout {
            println!(
                "cargo:warning=git could not read HEAD in this checkout; --version prints the bare version"
            );
        }
        println!(
            "cargo:rustc-env=CRATE_VERSION_WITH_BUILD={}",
            build_id::build_id(version, None, &[])
        );
        return;
    };

    let tags = git(&["tag", "--points-at", "HEAD"]).unwrap_or_default();
    let tags: Vec<&str> = tags.lines().collect();
    println!(
        "cargo:rustc-env=CRATE_VERSION_WITH_BUILD={}",
        build_id::build_id(version, Some(&sha), &tags)
    );

    // Re-run on a checkout (HEAD), a commit (a loose branch ref, or
    // packed-refs once gc has packed it), and a new tag. The two ref
    // directories are watched whole because the file a commit or a release
    // tag writes may not exist yet. `--git-path` resolves each in a
    // worktree too. A path that does not exist is left out, since cargo
    // re-runs on every build for one.
    let paths = git(&[
        "rev-parse",
        "--git-path",
        "HEAD",
        "--git-path",
        "packed-refs",
        "--git-path",
        "refs/heads",
        "--git-path",
        "refs/tags",
    ])
    .unwrap_or_default();
    for path in paths.lines().filter(|p| Path::new(p).exists()) {
        println!("cargo:rerun-if-changed={path}");
    }
}

/// Trimmed stdout of a git command that succeeded and printed something.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}
