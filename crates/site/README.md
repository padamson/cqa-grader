# cqa-grader-site

The landing page for `cqa-grader`, to be served at
[padamson.github.io/cqa-grader](https://padamson.github.io/cqa-grader/). A
Leptos CSR app (client-side, compiled to WebAssembly) built with
[Trunk](https://trunkrs.dev), styled with Tailwind CSS v4 in the brand's
board palette, and deployed to GitHub Pages as versioned snapshots
(`/cqa-grader/dev/` from main, `/cqa-grader/vX.Y.Z/` per release).

This crate has its own `[workspace]`, so the root crate's cargo commands
and supply chain are unaffected. Run cargo commands here with
`--manifest-path`; it carries its own `Cargo.lock`.

## Prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
```

Tailwind is handled by Trunk, which downloads the pinned v4 standalone
binary (see `Trunk.toml`), so there is no Node dependency.

## Preview locally

```bash
crates/site/preview.sh
```

`preview.sh` writes the architecture section with `asbuilt docs` (after
`asbuilt check`, so a stale model stops it), builds the site, and `trunk
serve`s it with hot reload at `http://127.0.0.1:8080`. It needs asbuilt
at the release `asbuilt.toml` pins, Node and Graphviz. Plain `trunk serve
--open` from this directory works too, without the architecture pages.

## Build

```bash
cd crates/site
SITE_VERSION=dev trunk build --release        # dist/, public_url = "/"
```

## Format, lint and test

```bash
cargo fmt --manifest-path crates/site/Cargo.toml -- --check
cargo clippy --manifest-path crates/site/Cargo.toml --target wasm32-unknown-unknown --all-targets -- -D warnings
cargo nextest run --manifest-path crates/site/Cargo.toml
```

The tests run on the host. They hold `public/roadmap.json` to its types
and every done item's criteria to tests that exist, and they check the
`versions.json` the deploy script writes against the version switcher
that reads it.

## Layout

```
index.html              Trunk entry: HTML shell, meta/OG tags, copied files
Trunk.toml              build config; pins Tailwind v4; public_url = "/"
input.css               Tailwind v4 entry, the board palette as @theme colors
build.rs                highlights snippets/ with syntect at build time; SITE_VERSION
snippets/               code samples, one file per language
public/roadmap.json     the roadmap; main's copy is the one every version fetches
public/receipts/        written by the dogfood test (ignored, .gitkeep tracked)
public/architecture/    written by `asbuilt docs` (ignored, .gitkeep tracked)
architecture.css        the architecture pages' palette, linked from the tree
deploy/update-manifest.sh   versions.json and the root redirect for the gh-pages tree
src/version.rs          SITE_VERSION, is_dev(), SITE_PREFIX, and the URL helpers
src/roadmap.rs          the roadmap's types and the tests that hold the file to them
src/app.rs              composes the page sections
src/components/         Hero, Install, HowItWorks, Example, Features, DogfoodBanner,
                        Roadmap (with each item's criteria as a popover),
                        VersionSwitcher, CodeBlock, CodeTabs, FeatureCard, Footer, icons
src/links.rs            every URL the page links to
```

## Notes

- The brand comes from `../../brand/`: `input.css` imports
  `wordmark.css`, `index.html` copies the fonts beside the stylesheet,
  and `icons::MARK` includes `mark.path`.
- The version switcher and the fetches of `versions.json` and main's
  roadmap use the deployed `/cqa-grader/` paths, so under `preview.sh`
  the switcher's links 404 and a release build falls back to its own
  roadmap. Everything else works at the root.
- `SITE_PREFIX` in `src/version.rs` is the one runtime copy of the
  `/cqa-grader` path; every other asset path is relative, which is what
  lets the same build serve at the root locally and under a snapshot
  path when deployed.
