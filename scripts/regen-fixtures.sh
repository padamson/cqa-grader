#!/usr/bin/env bash
#
# Regenerate the test graphs in tests/fixtures/ from sibling checkouts of
# ontology-authoring-template (wine) and cqa-schema (cqa), through
# panschema.toml, and record what they were generated from in
# tests/fixtures/PROVENANCE.
#
# Usage:
#   ./scripts/regen-fixtures.sh
#   PANSCHEMA=../panschema/target/debug/panschema ./scripts/regen-fixtures.sh
#
# The fixtures are committed, so the test jobs need neither the siblings
# nor panschema. Moving them forward is a commit of this script's output;
# the Dogfood workflow runs it against every producer's main and fails
# when the committed graphs fall behind.

set -euo pipefail

panschema="${PANSCHEMA:-panschema}"
# A relative path is the caller's, so resolve it before changing directory.
case "$panschema" in
  */*) panschema="$(cd "$(dirname "$panschema")" && pwd)/$(basename "$panschema")" ;;
esac

cd "$(dirname "$0")/.."

# A tag where the sibling sits on one, otherwise tag-count-gSHA, with
# -dirty when its working tree has uncommitted changes.
describe() {
  local v
  v="$(git -C "$1" describe --tags --always --dirty --abbrev=12)"
  case "$v" in
    *-dirty) echo "warning: $1 has uncommitted changes; PROVENANCE will say so" >&2 ;;
  esac
  echo "$v"
}

wine="$(describe ../ontology-authoring-template)"
cqa="$(describe ../cqa-schema)"
version="$("$panschema" --version)"

"$panschema" generate --strict

cat > tests/fixtures/PROVENANCE <<EOF
# Written by scripts/regen-fixtures.sh; do not edit.
ontology-authoring-template $wine
cqa-schema $cqa
$version
EOF
