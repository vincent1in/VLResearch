# Working on VLResearch

VLResearch is OpenResearch plus a thin layer of changes. Keeping that layer thin is what lets every new OpenResearch release merge in cleanly. These notes cover where things live, how to add to them, and how to take an upstream release.

## Where things live

| Path | What it holds |
| --- | --- |
| `src/flavor/` | All VLResearch Rust code: `egress.rs` (network policy), `wake.rs` (run wake-ups), `harness.rs` (agent settings and events surfaced in chat), `stall.rs` (stall alerts), `compact.rs` (autocompaction), `activity.rs` (each chat's last-activity clock), `metrics.rs` (the metrics ledger), `ideas.rs` (the idea queue and its selector), `scope.rs` (optimizations: each one's brief, the chat that runs it and its runs; the launch gate and the loop's wait for a confirmed brief), `start.rs` (starting an optimization, saving its brief and ideas, and confirming it from the dashboard), `findings.rs` (the findings ledger), `cli.rs` (the commands' arguments and dispatch), `metric_cli.rs`, `idea_cli.rs`, `brief_cli.rs` and `finding_cli.rs` (`orx metric`, `orx idea`, `orx brief`, `orx finding`), `skills.rs` (agent skills), `settings.rs` (the research settings: what the Settings page saved, the environment that overrides it, and the network policy it shows), `api.rs` (dashboard endpoints under `/api/vlresearch/`), `bench.rs` (the improvement-loop benchmark, a test) |
| `ui/src/flavor/` | Dashboard code: the chat progress strip (`ProgressStrip.tsx`, `progress.ts`, `checklist.ts`) the Research view (`ResearchView.tsx`, `research.ts`), with its form for starting auto-research (`ResearchSetup.tsx`), and the Settings page's Research section (`ResearchSettings.tsx`, `settings.ts`). Their logic is in the `.ts` files, tested by `ui/tests/vlresearch*.test.mjs` |
| `agent-skills/orx-metrics/`, `orx-ideas/`, `orx-findings/`, `orx-grill/`, `orx-improve/` | Agent skills: reporting metrics (with a Python helper), working the idea queue, keeping findings, scoping a goal with the user, and running the improvement loop |
| `src/flavor/chat_host.rs` | Code that starts hidden chat turns. It is compiled as a child module of `src/local/chat/mod.rs` (a `#[path]` declaration there), the only way to reach the chat host's private turn machinery without making upstream's items public |
| `flavor/scripts/` | `sync-upstream.sh`, `egress_audit.py`, `secret-scan.sh`, `install-hooks.sh` |
| `flavor/bench/loop.md` | The improvement-loop benchmark's last report. Rerun it after changing the selector, the keep rule or how the best is scored: `cargo test --release --bin orx flavor::bench::report -- --ignored --nocapture` |
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
| `src/local/harness/plan_gate.rs` | The reading forms of `orx metric`, `orx idea`, `orx brief` and `orx finding` pass Claude's plan-mode gate |
| `src/local/agent_skills.rs` | `skills()` hands both skill lists to `src/flavor/skills.rs`, which adds VLResearch's skills |
| `src/net.rs` | Both HTTP client builders go through the egress gate |
| `src/local/git.rs` | `git_command()` sets `GIT_ALLOW_PROTOCOL` |
| `src/local/github.rs`, `src/local/model.rs` | GitHub off unless `VLR_ALLOW_GITHUB` |
| `src/updates.rs`, `src/commands/update.rs`, `src/commands/version.rs` | No release-feed checks, update warnings or installs from alphaXiv |
| `src/commands/up/harness_setup.rs` | No agent installs or updates unless `VLR_ALLOW_INSTALLERS` |
| `src/local/chat/mod.rs` | `prepare_env` adds the agents' no-telemetry and checklist settings; `emit` feeds the activity clock; `watch_runs` runs the stall check; a finished turn calls the autocompactor; declares the `vlresearch` child module |
| `src/commands/up.rs` | Merges `src/flavor/api.rs`'s routes into the dashboard server |
| `ui/src/components/ChatPanel.tsx` | The progress strip above the composer; the agent's checklist tool reads as progress |
| `ui/src/App.tsx` | The Experiments tab's Research toggle, beside Table and Tree; the open chat and a way to bring a chat to the front, for the Research view; its side chats, which bring the loop's chat to the front and open upstream's side chat off it; its Settings button, which opens the Research settings |
| `ui/src/components/SettingsPage.tsx` | The Research section in the settings stack, after Harnesses, and `research` among the sections the Settings entry covers |
| `ui/src/workspaceState.ts`, `src/workspace_state.rs` | `research` is a settings section a dashboard address can name |
| `src/compute.rs` | `submit` checks the scoping gate (given the launching chat, to find the run's optimization) before any compute is spent, and the locked paths and scope against the commit it staged; then registers the launching chat's wake-up and records the run's optimization and probe runs |
| `src/plane/local_plane.rs` | `launch` passes a probe request on to the gate, which runs in the `orx up` process for launches from chat |
| `src/commands/exp.rs`, `src/plane/local_plane.rs` | `orx exp wait` timeout under Claude Code; a wait that sees the result settles the wake-up |
| `src/invocation.rs` | The line printed after a launch mentions the automatic wake-up |
| `src/local/harness/claude.rs` | Claude Code `api_retry` events become retry rows |
| `src/store.rs` | `flavor_conn()` accessor |
| `src/local/opencode.rs` | Test expects the new wake-up wording |
| `agent-skills/orx-compute/SKILL.md` | Wake-up and `exp wait` guidance for agents |
| `.github/workflows/release-on-bump.yml`, `linked-issue.yml` | Run only in alphaXiv/OpenResearch |
| `.github/workflows/ci.yml` | The Windows test step keeps its output, and `flavor/scripts/test-failures.sh` names failing tests in annotations |
| `.gitattributes`, `.gitignore` | Keep our README; ignore credential files |

## Taking an OpenResearch release

```sh
flavor/scripts/sync-upstream.sh            # newest release, on branch sync/<tag>
git push origin sync/<tag>                 # then open a pull request into main
```

The script fetches upstream's tags and merges the release on a `sync/<tag>` branch. It takes upstream's dashboard bundle and other generated files, refuses a merge that lost a `VLResearch:` hook line from an upstream file, rebuilds the bundle if VLResearch changed the dashboard, and runs fmt, clippy, the egress audit and the tests.

If it stops on a conflict, resolve the listed files: keep upstream's new code and put back the lines marked `VLResearch:`. Then run `git commit --no-edit` and the script again; it picks up on the sync branch. Git remembers each resolution (`rerere`) in the clone that made it. When upstream changes an API VLResearch's code uses, fix `src/flavor/` or `ui/src/flavor/` in a commit of its own that says what upstream changed.

A scheduled task (the OpenResearch release watcher) does this every morning: when there is a new release, it runs the script, resolves what it can, pushes `sync/<tag>` and opens a pull request for you to merge. It never pushes to `main`.

To try a merge before upstream releases it, run the script on upstream's main and don't push the result: `flavor/scripts/sync-upstream.sh upstream/main`.

Upstream merges are never rebased, so `main` is never force-pushed.

## The egress audit

`flavor/scripts/egress_audit.py` counts code that can open a connection without going through `src/net.rs`: websocket clients, raw sockets, spawned network tools, piped installers, and dashboard fetches to absolute URLs. A count above `flavor/egress-baseline.txt` fails CI. When an upstream merge adds one, check where it connects. If it is acceptable, or already blocked by the gate, accept it with `python3 flavor/scripts/egress_audit.py --update`.

## Next up

1. A claim page for each finding, with its evidence and links (`GET /api/vlresearch/findings/{id}`).
