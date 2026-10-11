#!/usr/bin/env bash
#
# Local preview with hot reload. Run from anywhere.
#
# Writes the architecture section, builds the site, then serves it with
# Trunk at http://127.0.0.1:8080 and opens a browser. Edit components and
# the page updates live.
#
# Prereqs (once): rustup target add wasm32-unknown-unknown; cargo install
# trunk; asbuilt at the release asbuilt.toml pins, with Node (for its
# pinned npx likec4) and Graphviz dot. Trunk downloads the pinned Tailwind
# v4 binary on its first build.
set -euo pipefail

cd "$(dirname "$0")"
# The architecture section, as the deploy writes it. A stale model stops
# the preview here with asbuilt's own message, before anything is removed.
# The tree is then emptied (the .gitkeep stays) so the preview starts from
# what a fresh checkout has: `asbuilt docs` replaces only what it wrote.
(cd ../.. && asbuilt check)
find public/architecture -mindepth 1 ! -name .gitkeep -delete
(cd ../.. && asbuilt docs -o crates/site/public/architecture)
trunk build
exec trunk serve --open
