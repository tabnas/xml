#!/usr/bin/env bash
# Download the W3C XML Conformance Test Suite (xmlts) and extract it into
# test/xmlconf/ so both the Go and TypeScript conformance runners can
# exercise the parser against the authoritative corpus.
#
# UPSTREAM (pinned):
#   https://www.w3.org/XML/Test/xmlts20130923.tar.gz
#   sha256 9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f
#
# The URL names a dated, immutable snapshot (20130923) and the archive is
# additionally pinned by SHA-256: this script refuses to install a corpus
# whose bytes differ from the recorded digest. That is the tarball
# equivalent of pinning a git commit — every conformance number recorded
# in this repository refers to one exact corpus, and an upstream swap
# cannot silently change what "conformance" means here.
#
# The archive is owned by W3C and its contributors (Sun, OASIS, IBM,
# University of Edinburgh, Fuji Xerox, ...) and is NOT redistributed as
# part of this repository — test/xmlconf/ is gitignored and must never be
# committed. Running this script is an explicit opt-in to download it
# from the W3C site.
#
# The script is idempotent: if a corpus is already present it exits 0
# without touching the network. Pass --force (or delete test/xmlconf/) to
# re-download.
#
# Usage:
#   scripts/fetch-xml-suite.sh              # default location
#   scripts/fetch-xml-suite.sh --force      # re-download
#   scripts/fetch-xml-suite.sh /some/dir    # custom destination
#
# The tests run this automatically:
#   ts/  -> the `pretest` npm script
#   go/  -> TestMain in go/xmlconf_test.go
# and they FAIL when the corpus is absent rather than skipping.
set -euo pipefail

URL="https://www.w3.org/XML/Test/xmlts20130923.tar.gz"
SHA256="9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f"

# The corpus is pinned, so these are exact. The conformance runners
# re-check the catalogue census; this is a fast smoke check that the
# extraction produced a usable tree.
EXPECT_CATALOG="xmlconf.xml"
EXPECT_DIRS="xmltest sun oasis ibm japanese eduni"

FORCE=0
DEST=""
for arg in "$@"; do
  case "$arg" in
    --force) FORCE=1 ;;
    *) DEST="$arg" ;;
  esac
done

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${DEST:-$REPO_ROOT/test/xmlconf}"

corpus_ok() {
  [ -f "$DEST/$EXPECT_CATALOG" ] || return 1
  for d in $EXPECT_DIRS; do
    [ -d "$DEST/$d" ] || return 1
  done
  return 0
}

if [ "$FORCE" = "1" ]; then
  rm -rf "$DEST"
elif corpus_ok; then
  echo "W3C XML conformance suite already present at $DEST (use --force to re-download)."
  exit 0
else
  # Partial or legacy extraction: start clean rather than merge.
  rm -rf "$DEST"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Fetching $URL ..."
curl -fL --retry 3 --retry-delay 2 -o "$tmp/xmlts.tar.gz" "$URL"

echo "Verifying sha256 ..."
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/xmlts.tar.gz" | cut -d' ' -f1)"
else
  # macOS runners have shasum, not sha256sum.
  actual="$(shasum -a 256 "$tmp/xmlts.tar.gz" | cut -d' ' -f1)"
fi
if [ "$actual" != "$SHA256" ]; then
  echo "ERROR: checksum mismatch for $URL" >&2
  echo "  expected $SHA256" >&2
  echo "  actual   $actual" >&2
  echo "The pinned corpus changed upstream. Do NOT silently accept it:" >&2
  echo "review the difference, then update SHA256 in this script deliberately." >&2
  exit 1
fi

echo "Extracting to $DEST ..."
mkdir -p "$DEST"
# The archive contains a top-level `xmlconf/` directory, so strip one
# component to land its contents directly in $DEST.
tar -xzf "$tmp/xmlts.tar.gz" -C "$DEST" --strip-components=1

if ! corpus_ok; then
  echo "ERROR: extraction did not produce the expected layout under $DEST" >&2
  exit 1
fi

echo "Done. $DEST/$EXPECT_CATALOG present."
