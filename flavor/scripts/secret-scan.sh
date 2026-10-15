#!/usr/bin/env bash
# Scans every file VLResearch added or changed relative to the OpenResearch
# release it is built on, so upstream's own files (already scanned by GitHub
# when alphaXiv pushed them) can't drown real findings in noise.
#
#   flavor/scripts/secret-scan.sh            # uses gitleaks on PATH, else Docker
#
# Needs upstream's release tags: `git fetch --tags upstream` locally, or
# `git fetch --tags https://github.com/alphaXiv/OpenResearch.git` in CI.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

base="$(git describe --tags --abbrev=0 --match 'v*' HEAD 2>/dev/null || true)"
if [ -z "$base" ]; then
  echo "secret-scan: no upstream release tag in history; fetch upstream tags first." >&2
  exit 2
fi

scan_dir="$(mktemp -d)"
trap 'rm -rf "$scan_dir"' EXIT

count=0
while IFS= read -r -d '' path; do
  [ -f "$path" ] || continue
  mkdir -p "$scan_dir/$(dirname "$path")"
  cp "$path" "$scan_dir/$path"
  count=$((count + 1))
done < <(git diff -z --name-only --diff-filter=AMR "$base" HEAD --)
# Uncommitted and untracked files count too when this runs on a working copy.
while IFS= read -r -d '' path; do
  [ -f "$path" ] || continue
  mkdir -p "$scan_dir/$(dirname "$path")"
  cp "$path" "$scan_dir/$path"
  count=$((count + 1))
done < <(git ls-files -z --modified --others --exclude-standard)

echo "secret-scan: checking $count file(s) changed since $base"
if [ "$count" -eq 0 ]; then
  exit 0
fi

if command -v gitleaks >/dev/null 2>&1; then
  gitleaks dir "$scan_dir" --config "$root/.gitleaks.toml" --redact --no-banner --verbose
else
  docker run --rm \
    -v "$scan_dir:/scan:ro" \
    -v "$root/.gitleaks.toml:/config.toml:ro" \
    zricethezav/gitleaks:latest \
    dir /scan --config /config.toml --redact --no-banner --verbose
fi
