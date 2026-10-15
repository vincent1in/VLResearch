#!/usr/bin/env bash
# Turns a saved `cargo test` log into GitHub Actions error annotations: each
# failing test with where it panicked and why, and any test binary that
# crashed. GitHub's API serves annotations even where the log itself can't be
# downloaded, so a failure on a runner nobody can reach still names its test.
#
#   cargo test --locked 2>&1 | tee test.log
#   flavor/scripts/test-failures.sh test.log
set -euo pipefail

log="${1:?usage: test-failures.sh <cargo test log>}"
# Git Bash on Windows: the runner hands over Windows paths.
if command -v cygpath >/dev/null 2>&1; then
  log="$(cygpath -u "$log")"
fi
if [ ! -f "$log" ]; then
  echo "::error::cargo test failed, and its log wasn't saved at $1"
  exit 0
fi

awk '
  # Workflow-command escaping: messages may hold any text; property values
  # (the title) may not hold ":" or ",".
  function message(text) { gsub(/%/, "%25", text); return text }
  function property(text) { text = message(text); gsub(/:/, "%3A", text); gsub(/,/, "%2C", text); return text }
  function flush() {
    if (name != "") { print "::error title=" property(name) "::" (detail == "" ? "failed" : detail); shown++ }
    name = ""; detail = ""; lines = 0
  }
  { sub(/\r$/, "") }
  # libtest prints each failure as "---- <name> stdout ----" then its output.
  /^---- .* stdout ----$/ { flush(); name = $2; next }
  /^failures:$/ || /^test result:/ { flush(); next }
  name != "" {
    if ($0 ~ /^stack backtrace:$/) lines = 6
    if ($0 == "" || $0 ~ /^note: run with `RUST_BACKTRACE/ || lines >= 6) next
    detail = detail (detail == "" ? "" : "%0A") message($0)
    lines++
    next
  }
  # A test binary that crashed or was killed prints no failure section.
  /^error: test failed/ || /process didn.t exit successfully/ { print "::error::" message($0); shown++ }
  { tail[NR % 8] = $0 }
  END {
    flush()
    if (!shown) {
      text = ""
      for (i = NR - 7; i <= NR; i++) if (i > 0 && tail[i % 8] != "") text = text (text == "" ? "" : "%0A") message(tail[i % 8])
      print "::error title=cargo test failed::" (text == "" ? "no output" : text)
    }
  }
' "$log" | head -n 20
