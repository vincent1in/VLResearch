# Working on VLResearch

VLResearch is OpenResearch plus a thin layer of changes. Keeping that layer thin is what lets every new OpenResearch release merge in cleanly. These notes cover where things live, how to add to them, and how to take an upstream release.

## Where things live

| Path | What it holds |
| --- | --- |
| `src/flavor/` | All VLResearch Rust code: `egress.rs` (network policy), `wake.rs` (run wake-ups), `harness.rs` (agent settings and events surfaced in chat), `stall.rs` (stall alerts), `activity.rs` (each chat's last-activity clock), `metrics.rs` (the metrics ledger), `cli.rs` (`orx metric`), `skills.rs` (agent skills), `api.rs` (dashboard endpoints under `/api/vlresearch/`) |
| `agent-skills/orx-metrics/` | The agent skill for reporting and comparing metrics, with its Python helper |
| `src/flavor/chat_host.rs` | Code that starts hidden chat turns. It is compiled as a child module of `src/local/chat/mod.rs` (a `#[path]` declaration there), the only way to reach the chat host's private turn machinery without making upstream's items public |
| `flavor/scripts/` | `sync-upstream.sh`, `egress_audit.py`, `secret-scan.sh`, `install-hooks.sh` |
| `flavor/egress-baseline.txt` | Accepted network call sites outside the egress gate |
| `.github/workflows/vlresearch.yml` | Secret scan and egress audit; upstream's `ci.yml` still runs fmt, clippy and tests |
| `README.md`, `NOTICE`, this file | VLResearch's own docs; `README.md` never takes upstream's edits |

## Rules for changes

- Put new code in `src/flavor/` (or `ui/src/flavor/` for the dashboard). Upstream files get only short hooks that call into it, each marked with a `VLResearch:` comment.
- New tables and SQL go in `src/flavor/` too, through `Store::flavor_conn()`.
- Write dashboard strings in English inline. Upstream's `ui/messages/*.json` change in almost every release, and edits there would conflict constantly.
- Keep generated files (`ui/dist/`, `ui/src/routeTree.gen.ts`, lockfiles) out of feature commits; rebuild them in a commit of their own.
- One commit per feature, so a merge conflict points at the feature it touches.
- Commit no keys, tokens, hostnames or dataset paths. Run `flavor/scripts/install-hooks.sh` once per clone.

## Hooks in upstream files

Find them all with `git grep -n 'VLResearch:'`.

| File | Hook |
| --- | --- |
| `src/main.rs` | Declares `mod flavor`; one flattened `Flavor` command variant adds every VLResearch command, plus its arms in `command_name` and `dispatch` |
| `src/local/harness/plan_gate.rs` | `orx metric`'s reading forms pass Claude's plan-mode gate |
| `src/local/agent_skills.rs` | Registers the `orx-metrics` skill in both skill sets |
| `src/net.rs` | Both HTTP client builders go through the egress gate |
| `src/local/git.rs` | `git_command()` sets `GIT_ALLOW_PROTOCOL` |
| `src/local/github.rs`, `src/local/model.rs` | GitHub off unless `VLR_ALLOW_GITHUB` |
| `src/updates.rs`, `src/commands/update.rs`, `src/commands/version.rs` | No release-feed checks, update warnings or installs from alphaXiv |
| `src/commands/up/harness_setup.rs` | No agent installs or updates unless `VLR_ALLOW_INSTALLERS` |
| `src/local/chat/mod.rs` | `prepare_env` adds the agents' no-telemetry and checklist settings; `emit` feeds the activity clock; `watch_runs` runs the stall check; declares the `vlresearch` child module |
| `src/commands/up.rs` | Merges `src/flavor/api.rs`'s routes into the dashboard server |
| `src/compute.rs` | `submit` registers the launching chat's wake-up |
| `src/commands/exp.rs`, `src/plane/local_plane.rs` | `orx exp wait` timeout under Claude Code; a wait that sees the result settles the wake-up |
| `src/invocation.rs` | The line printed after a launch mentions the automatic wake-up |
| `src/local/harness/claude.rs` | Claude Code `api_retry` events become retry rows |
| `src/store.rs` | `flavor_conn()` accessor |
| `src/local/opencode.rs` | Test expects the new wake-up wording |
| `agent-skills/orx-compute/SKILL.md` | Wake-up and `exp wait` guidance for agents |
| `.github/workflows/release-on-bump.yml`, `linked-issue.yml` | Run only in alphaXiv/OpenResearch |
| `.gitattributes`, `.gitignore` | Keep our README; ignore credential files |

## Taking an OpenResearch release

```sh
flavor/scripts/sync-upstream.sh            # newest release, on branch sync/<tag>
git switch main && git merge --ff-only sync/<tag> && git push origin main
```

The script fetches upstream's tags and merges the release on a `sync/<tag>` branch. It takes upstream's copy of generated files, rebuilds the dashboard bundle if VLResearch changed the dashboard, and runs fmt, clippy, the egress audit and the tests.

If it stops on a conflict, resolve the listed files: keep upstream's new code and put back the lines marked `VLResearch:`. Git remembers each resolution (`rerere`) and replays it in later syncs. Then run `git commit --no-edit` and the script again.

Upstream merges are never rebased, so `main` is never force-pushed.

## The egress audit

`flavor/scripts/egress_audit.py` counts code that can open a connection without going through `src/net.rs`: websocket clients, raw sockets, spawned network tools, piped installers, and dashboard fetches to absolute URLs. A count above `flavor/egress-baseline.txt` fails CI. When an upstream merge adds one, check where it connects. If it is acceptable, or already blocked by the gate, accept it with `python3 flavor/scripts/egress_audit.py --update`.

## Next up

1. Dashboard views on the endpoints that already exist: the chat progress strip (`GET /api/vlresearch/sessions/{id}/progress`) and the optimization surface, with the running best and the lineage waterfall (`GET /api/vlresearch/projects/{id}/metrics`).
2. An idea queue, a "grill me" scoping gate, a forecast-and-verify loop, and a findings ledger.
