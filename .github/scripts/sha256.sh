#!/usr/bin/env bash
# A .sha256 beside every asset, matching OpenCADStudio's release convention. Written
# with the bare filename so `sha256sum -c` works wherever the file is downloaded to.
set -euo pipefail
for f in "$@"; do
  [ -e "$f" ] || { echo "no such asset: $f" >&2; exit 1; }
  case "$f" in *.sha256) continue;; esac
  (
    cd "$(dirname "$f")"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum "$(basename "$f")" > "$(basename "$f").sha256"
    else
      # macOS ships shasum rather than GNU coreutils.
      shasum -a 256 "$(basename "$f")" > "$(basename "$f").sha256"
    fi
  )
  cat "$f.sha256"
done
