#!/usr/bin/env bash
# Merges an alphaXiv/OpenResearch release into VLResearch on a review branch.
#
#   flavor/scripts/sync-upstream.sh               # the newest upstream release tag
#   flavor/scripts/sync-upstream.sh v0.2.19       # a specific release
#   flavor/scripts/sync-upstream.sh upstream/main # unreleased work, as a dry run
#   flavor/scripts/sync-upstream.sh --no-test     # skip the slow cargo test step
#   flavor/scripts/sync-upstream.sh --hooks-checked  # after confirming a lost hook
#
# Releases are merged, never rebased, so `main` is never force-pushed and
# anyone who cloned VLResearch can keep pulling. Each VLResearch change to an
# upstream file is a short hook marked "VLResearch:"; when a merge conflicts,
# keep upstream's new code and put the marked lines back, commit, and run the
# script again: it picks up on the sync branch and runs the checks.
set -euo pipefail

run_tests=1
hooks_checked=0
tag=""
for arg in "$@"; do
  case "$arg" in
    --no-test) run_tests=0 ;;
    --hooks-checked) hooks_checked=1 ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
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
  echo "sync: no upstream tag or ref named $tag" >&2
  exit 1
}

# Hooks in upstream files: every "VLResearch:" line outside VLResearch's own
# code and docs. A merge that loses one has dropped a hook.
hooks_in() {
  git grep -c 'VLResearch:' "$1" -- . ':!src/flavor' ':!ui/src/flavor' ':!flavor' \
    ':!VLRESEARCH.md' ':!README.md' 2>/dev/null | awk -F: '{ n += $NF } END { print n + 0 }'
}

branch="sync/${tag//\//-}"
if [ "$(git branch --show-current)" = "$branch" ] && git merge-base --is-ancestor "$tag" HEAD; then
  echo "sync: $tag is merged on $branch; checking it"
else
  if git merge-base --is-ancestor "$tag" HEAD; then
    echo "sync: this branch already contains $tag."
    exit 0
  fi
  if [ -n "$(git status --porcelain)" ]; then
    echo "sync: commit or stash your changes first." >&2
    exit 1
  fi
  git switch --quiet -c "$branch"
  echo "sync: merging $tag into $branch"
  if ! git merge --no-ff --no-edit -m "Merge OpenResearch $tag" "$tag"; then
    # The dashboard bundle is generated and rebuilt below, so take upstream's
    # as a whole; a renamed asset can conflict on both sides.
    if git diff --name-only --diff-filter=U | grep -q '^ui/dist/'; then
      git checkout "$tag" -- ui/dist
      comm -23 <(git ls-files ui/dist | sort -u) \
        <(git ls-tree -r --name-only "$tag" -- ui/dist | sort -u) | xargs -r git rm -q -f --
    fi
    while IFS= read -r path; do
      case "$path" in
        ui/src/routeTree.gen.ts|Cargo.lock|ui/pnpm-lock.yaml)
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
fi

merge="$(git rev-list --merges -n 1 HEAD)"
before="$(hooks_in "$merge^1")"
after="$(hooks_in HEAD)"
if [ "$after" -lt "$before" ] && [ "$hooks_checked" -eq 0 ]; then
  echo "sync: the merge has $after 'VLResearch:' hook lines in upstream files, down from $before." >&2
  echo "sync: compare 'git grep -n VLResearch: $merge^1' with the merge, put back any hook" >&2
  echo "sync: a resolution dropped, or rerun with --hooks-checked if upstream removed its place." >&2
  exit 1
fi

# If VLResearch changed the dashboard, the committed bundle must be rebuilt.
if [ -n "$(git diff --name-only "$tag" HEAD -- ui/src ui/index.html)" ]; then
  echo "sync: rebuilding ui/dist"
  # The message compiler downloads its plugins; behind a proxy, Node's fetch
  # needs to be told to use it.
  (cd ui && pnpm install --frozen-lockfile && NODE_USE_ENV_PROXY=1 pnpm build)
  if [ -n "$(git status --porcelain -- ui/dist)" ]; then
    git add -A ui/dist
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
echo "  git push origin $branch   # and open a pull request into main"
echo "  # or: git switch main && git merge --ff-only $branch && git push origin main"
