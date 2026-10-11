#!/usr/bin/env bash
#
# Local preview with hot reload. Run from anywhere.
#
# Builds the site, then serves it with Trunk at http://127.0.0.1:8080 and
# opens a browser. Edit components and the page updates live.
#
# Prereqs (once): rustup target add wasm32-unknown-unknown; cargo install
# trunk. Trunk downloads the pinned Tailwind v4 binary on its first build.
set -euo pipefail

cd "$(dirname "$0")"
trunk build
exec trunk serve --open
