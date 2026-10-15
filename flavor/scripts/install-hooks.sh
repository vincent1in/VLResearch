#!/usr/bin/env bash
# Installs a git pre-commit hook that refuses commits containing secrets
# (API keys, tokens, private keys) using gitleaks. Run once per clone:
#
#   flavor/scripts/install-hooks.sh
#
# Install gitleaks first: https://github.com/gitleaks/gitleaks#installing
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
hooks="$(git rev-parse --git-path hooks)"
mkdir -p "$hooks"
hook="$hooks/pre-commit"

if [ -e "$hook" ] && ! grep -q 'VLResearch secret scan' "$hook"; then
  echo "install-hooks: $hook already exists and is not ours; add this to it by hand:" >&2
  echo "  gitleaks git --pre-commit --staged --redact --config \"$root/.gitleaks.toml\"" >&2
  exit 1
fi

cat > "$hook" <<'HOOK'
#!/usr/bin/env bash
# VLResearch secret scan: block commits that contain credentials.
root="$(git rev-parse --show-toplevel)"
if ! command -v gitleaks >/dev/null 2>&1; then
  echo "pre-commit: gitleaks is not installed, so staged changes were not scanned for secrets." >&2
  echo "pre-commit: install it (https://github.com/gitleaks/gitleaks#installing) to turn the check on." >&2
  exit 0
fi
if gitleaks git --help >/dev/null 2>&1; then
  exec gitleaks git --pre-commit --staged --redact --no-banner --config "$root/.gitleaks.toml"
else
  exec gitleaks protect --staged --redact --no-banner --config "$root/.gitleaks.toml"
fi
HOOK
chmod +x "$hook"
echo "install-hooks: pre-commit secret scan installed at $hook"
