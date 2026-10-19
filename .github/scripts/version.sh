#!/usr/bin/env bash
# The release version. On a tag it is the tag; on a workflow_dispatch rehearsal there
# is no tag, so fall back to the workspace version with a -dev suffix, which keeps the
# artifact names honest about not being a release.
set -euo pipefail
WORKSPACE_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [[ ! "$WORKSPACE_VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "::error::Expected a stable X.Y.Z workspace version, got '$WORKSPACE_VERSION'" >&2
  exit 1
fi
if [[ "${GITHUB_EVENT_NAME:-}" != workflow_dispatch && "${GITHUB_REF:-}" == refs/tags/* ]]; then
  V="${GITHUB_REF_NAME}"
  if [[ "$V" != "v$WORKSPACE_VERSION" ]]; then
    echo "::error::Tag '$V' must match workspace version 'v$WORKSPACE_VERSION'" >&2
    exit 1
  fi
else
  V="v$WORKSPACE_VERSION-dev"
fi
echo "v=$V" >> "$GITHUB_OUTPUT"
echo "version: $V"
