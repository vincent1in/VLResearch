#!/usr/bin/env bash
# Merges an alphaXiv/OpenResearch release into VLResearch on a review branch.
#
#   flavor/scripts/sync-upstream.sh            # the newest upstream release tag
#   flavor/scripts/sync-upstream.sh v0.2.19    # a specific release
#   flavor/scripts/sync-upstream.sh --no-test  # skip the slow cargo test step
#
# Releases are merged, never rebased, so `main` is never force-pushed and
# anyone who cloned VLResearch can keep pulling. Each VLResearch change to an
# upstream file is a short hook marked "VLResearch:"; when a merge conflicts,
# keep upstream's new code and put the marked lines back.
set -euo pipefail

run_tests=1
tag=""
for arg in "$@"; do
  case "$arg" in
    --no-test) run_tests=0 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) tag="$arg" ;;
  esac
done

cd "$(git rev-parse --show-toplevel)"

if ! git remote get-url upstream >/dev/null 2>&1; then
  git remote add upstream https://github.com/alphaXiv/OpenResearch.git
fi
# Nothing from here is ever pushed to alphaXiv.
git remote set-url --push upstream DISABLED
# Replay conflict resolutions made in earlier syncs, and keep our README.
git config rerere.enabled true
git config rerere.autoupdate true
git config merge.ours.driver true

git fetch --quiet --tags upstream
if [ -z "$tag" ]; then
  tag="$(git tag --list 'v*' --sort=-v:refname | head -n 1)"
fi
git rev-parse --verify --quiet "$tag^{commit}" >/dev/null || {
  echo "sync: no upstream tag named $tag" >&2
  exit 1
}

if git merge-base --is-ancestor "$tag" HEAD; then
  echo "sync: this branch already contains $tag."
  exit 0
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "sync: commit or stash your changes first." >&2
  exit 1
fi

branch="sync/$tag"
git switch --quiet -c "$branch"
echo "sync: merging $tag into $branch"

if ! git merge --no-ff --no-edit -m "Merge OpenResearch $tag" "$tag"; then
  # Generated files are rebuilt below, so take upstream's copy of each.
  while IFS= read -r path; do
    case "$path" in
      ui/dist/*|ui/src/routeTree.gen.ts|Cargo.lock|ui/pnpm-lock.yaml)
        git checkout --theirs -- "$path"
        git add -- "$path"
        ;;
    esac
  done < <(git diff --name-only --diff-filter=U)
  left="$(git diff --name-only --diff-filter=U)"
  if [ -n "$left" ]; then
    echo
    echo "sync: resolve these by hand, keeping every line marked 'VLResearch:':"
    echo "$left" | sed 's/^/  /'
    echo "sync: then run 'git commit --no-edit' and this script again to run the checks."
    exit 1
  fi
  git commit --quiet --no-edit
fi

# If VLResearch changed the dashboard, the committed bundle must be rebuilt.
if [ -n "$(git diff --name-only "$tag" HEAD -- ui/src ui/index.html)" ]; then
  echo "sync: rebuilding ui/dist"
  (cd ui && pnpm install --frozen-lockfile && pnpm build)
  if [ -n "$(git status --porcelain -- ui/dist)" ]; then
    git add ui/dist
    git commit --quiet -m "Rebuild ui/dist after merging OpenResearch $tag"
  fi
fi

echo "sync: checking"
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
python3 flavor/scripts/egress_audit.py
if [ "$run_tests" -eq 1 ]; then
  cargo test --locked
fi

echo
echo "sync: $tag is merged on $branch and the checks pass. To publish it:"
echo "  git switch main && git merge --ff-only $branch && git push origin main"
