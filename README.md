# VLResearch

A local-first workspace for research agents, tuned for quantitative research: adding datasets, updating models, and making processes faster, with experiments run on your own machines over SSH.

VLResearch is a distribution of [OpenResearch](https://github.com/alphaXiv/OpenResearch) by alphaXiv. It keeps OpenResearch's experiment tree, SSH and Slurm compute, and chat with your coding agent (Claude Code, Codex or OpenCode), and changes nine things:

- **Nothing leaves your machine except SSH and your LLM.** orx itself never calls an LLM; your coding agent does. VLResearch blocks what orx would otherwise send on its own: update checks, accounts and managed compute, literature APIs, GitHub, Overleaf and agent installers. It also turns off the agents' own telemetry and auto-updates.
- **Chats hear back from their runs.** Every run a chat launches wakes that chat when it finishes, instead of only when the agent remembered to ask.
- **Stuck runs get noticed.** If a run a chat is waiting on prints nothing for 30 minutes, logs a NaN or infinite metric, or hits an out-of-memory error while still running, orx tells the agent. The agent can then read the log and cancel the run instead of letting it burn compute.
- **Waiting is visible.** When Claude Code is retrying a failed API call, the chat shows a retry row instead of a silent hang. Claude Code also keeps a checklist of its steps (its TodoWrite tool), whichever model you pick.
- **Runs are scored.** Run code prints `ORX_METRIC {json}` lines and orx records them per run. Each project names one primary metric and its direction. `orx metric` then ranks runs on it, tracks the best run against the baseline, and splits the gain across the experiments between them. A noise floor keeps a lucky run from counting as an improvement.
- **Ideas are work items.** Each idea to try is queued with a forecast, an 80% interval, a GPU-hour cost and a cheap kill test. `orx idea next` picks the next one by expected gain per GPU-hour, with blockers first and a rule that keeps the search from only making small steps. Closing an idea measures what its experiment did against its parent, so forecasts can be checked.
- **Work is scoped before compute is spent.** The agent interviews you in rounds, each question with its recommended answer, until the objective, metric, limits, budget and stop rule are agreed, then reviews the data for leakage and proposes ideas. Once a project has this brief, orx holds every launch to it.
- **What was learned is kept.** Findings are claims with evidence: runs that support or refute them, links between claims, and verdicts that are added, never overwritten. Refuted claims stay, so every trial behind a best result is counted.
- **Long chats keep going.** When a chat's context window is 80% full, orx compacts it as soon as the turn that filled it ends, so a long research loop doesn't run out of room in the middle of a task. The metrics, ideas, brief and findings live outside the chat, so compacting loses none of them.

In the dashboard, a progress strip above each chat's composer shows the agent's checklist, how long since its last update, and the runs it waits on. The Experiments tab's Research view shows the running best against the baseline, what moved the score, the idea queue, the brief with its budget, and the findings. On a project without a brief, it starts auto-research from a form. Its Side chat button, and the ask buttons beside the next idea, each step that moved the score, each idea and each finding, open a side chat off the chat that runs the loop, so you can ask about the work without stopping it.

## Scoring runs

Print metrics from the run's code, one JSON object per line:

```text
ORX_METRIC {"key": "val_ic", "value": 0.031, "step": 1200}
ORX_METRIC {"step": 1200, "val_ic": 0.031, "train_loss": 0.42}
```

`orx skill metrics/assets/orx_metrics.py` prints a small Python helper that writes these lines. Then:

```sh
orx metric primary <projectId> val_ic --maximize --noise-floor 0.002   # once per project
orx metric runs <projectId>     # runs ranked on the primary metric
orx metric best <projectId>     # best run against the baseline, step by step
orx metric show <runId>         # everything one run reported
```

A run's score is the last value it reported. Choose a validation metric as primary, and keep test splits for final reports; when a key is reported under several splits, `orx metric primary` asks which one to score (`--split val`). Coding agents in VLResearch chats get these instructions as the `orx-metrics` skill.

## Tracking ideas

```sh
orx idea add <projectId> "Volume features, 20-day window" --class incremental \
  --forecast 0.003 --interval -0.001 0.007 --cost-hours 2 \
  --kill-test "No IC gain on the 2023 fold"
orx idea next <projectId>               # what to do next, and why
orx idea start <ideaId> <experimentId>  # the experiment that tests it
orx idea done <ideaId>                  # measured against the parent: kept or refuted
orx idea calibration <projectId>        # how well forecasts have held up, and which classes of idea pay
```

Forecasts are changes in the primary metric, in its units and sign. Once five ideas have been tested, orx also reads each new forecast in the light of past ones: shifted by the mean error, with its interval stretched until 80% of past outcomes would have landed inside. Agents follow the same steps through the `orx-ideas` skill.

The per-step gains in `orx metric best` assume the steps add up. To check that, when a kept idea makes a new best, orx queues up to two ablation ideas for the biggest earlier steps behind it: take that step's change back out of the best and see whether the metric loses what the step gained. A check is confirmed when the loss lands in its forecast interval, and refuted when it is far smaller or larger: then the steps interact, and the per-step split is off. `orx idea next` takes these checks, and replicates, before new ideas (`VLR_AUTO_ABLATE=0` stops the automatic ablations).

## Scoping and the improvement loop

Ask the agent to scope a goal, or start one and let it: the `orx-grill` skill interviews you, checks the data and the baseline on your compute host, reviews the code for leakage (point-in-time joins, overlapping splits, survivorship), and proposes ideas. When you agree, it records the brief:

```sh
orx brief show <projectId>       # the brief, what it still needs, budget used
orx brief set <projectId> --objective "..." --lock eval/score.py --budget-runs 20 --stop-rule "..."
orx brief confirm <projectId>    # only after you have agreed to it
```

Or start from the dashboard: in the Experiments tab, open Research and choose Set up research. The form asks what to improve and the metric that measures it, what runs may and must not change, the budget and stop rule, and your pointers and seed ideas. Save and start records them and opens a chat in which the agent scopes the rest with you, asking only about what's missing. Confirm the brief in the Research view when you agree; the chat then starts the loop.

Once a project has a brief, orx refuses a launch until the brief is confirmed, while a blocker idea is open, when the commit about to run changes a locked file or folder, or once the run or day budget is spent. The budget counts from the first confirmation; editing and reconfirming the brief doesn't reset it unless you ask (`--reset-budget`). Smoke tests, baseline checks and seed runs for the noise floor go through as probes (`VLR_PROBE=1 orx exp run <expId>`), which never count as results. Projects without a brief launch as before, with a reminder, but the improvement loop itself (`orx idea next`, `orx idea start`) waits for a confirmed brief, and confirming needs at least one idea to start from.

Findings record what the loop learns:

```sh
orx finding add <projectId> "Volume features add IC in high-volatility months" --kind model
orx finding evidence <findingId> --supports --run <runId> --note "2022 fold: +0.004 IC"
orx finding assess <findingId> --verdict supported --confidence 0.7 --scope "2016-2023"
orx finding lock <findingId>     # settled; agents can add evidence but not change it
```

The `orx-improve` skill then runs the loop: pick the next idea, check its forecast against past calibration, build it on a child of the best experiment, run it, measure it, and keep a new best only past the noise floor, after a replicate with new seeds.

## Long chats

When a chat's context window is 80% full (`VLR_AUTO_COMPACT`), orx compacts the chat as soon as the turn that filled it ends and nothing else is waiting to run. It uses the agent's own compaction where there is one (Claude Code, Codex), and otherwise a summary that the next turn starts from. It shows in the chat like a manual compaction, Stop cancels it, and messages you send meanwhile run after it.

orx has to know how full the window is. Claude Code and Codex report it. OpenCode reports only how many tokens are in use, so orx knows its window for a local model (the one you entered when connecting it) but not for a cloud model; OpenResearch turns OpenCode's own compaction off, so this is the only compaction those local-model chats get. Cursor and Antigravity report no usage, and aren't compacted automatically.

orx doesn't compact a chat while a plan, question or permission card is waiting for your answer, or a helper agent that another chat delegated to. After compacting a chat, it lets three turns run before compacting it again; after a compaction that failed or that you stopped, it waits 30 minutes.

## Install from source

You need a stable Rust toolchain and git. VLResearch has no release builds, and it never installs alphaXiv's releases over itself.

```sh
git clone https://github.com/vincent1in/VLResearch.git
cd VLResearch
cargo install --path . --locked
orx up
```

`orx up` opens the dashboard at `http://127.0.0.1:4791`. The command is still called `orx`, as in OpenResearch.

Install and sign in to your coding agent yourself before the first run; VLResearch won't download it for you. To update, pull, run `cargo install --path . --locked` again, and restart `orx up`.

## Network policy

| Leaves the machine | How |
| --- | --- |
| SSH to your compute hosts | Your `~/.ssh/config`, as in OpenResearch |
| Calls to your LLM provider | Made by your coding agent, signed in with its own credentials |
| Anything else orx sends on its own | Blocked, unless you list the host in `VLR_EGRESS_ALLOW` |

Two things still connect when you choose to use them: the command-line tools of third-party compute backends you configure (Modal, Hugging Face, Kubernetes), and tectonic, which downloads a TeX bundle if it is the only LaTeX engine installed.

orx also can't police what the agent runs in its own shell. To make the policy hold for the whole machine, allow only your LLM provider (port 443) and your SSH hosts (port 22) at the firewall.

## Settings

All optional. Set them in the environment `orx up` starts from.

| Variable | Default | Effect |
| --- | --- | --- |
| `VLR_EGRESS_ALLOW` | empty | Hosts orx may reach besides this machine, comma-separated: `export.arxiv.org`, `*.example.com`, or `*` for no limit |
| `VLR_GIT_PROTOCOLS` | `file` | Git transports orx may use; `file:ssh` adds an internal git server over SSH |
| `VLR_ALLOW_GITHUB` | off | Turns GitHub integration (repo creation, branch pushes) back on |
| `VLR_ALLOW_INSTALLERS` | off | Lets the dashboard install or update coding agents |
| `VLR_AUTO_WAKE` | on | `0` keeps OpenResearch's opt-in wake-ups |
| `VLR_AGENT_ENV_DEFAULTS` | on | `0` stops orx adding the no-telemetry settings to agent processes |
| `VLR_AGENT_CHECKLIST` | on | `0` leaves Claude Code's task tools as Claude Code configures them |
| `VLR_STALL_MINUTES` | `30` | Minutes a running job may print nothing before its chat is told; `0` turns stall alerts off |
| `VLR_SCOPE_GATE` | on | Holds launches to a project's brief once it has one; `strict` also refuses projects without a brief; `0` turns the gate off |
| `VLR_PROBE` | off | Set to `1` on a single launch to run it as a probe |
| `VLR_AUTO_ABLATE` | on | `0` stops orx from queueing ablation checks when a kept idea makes a new best |
| `VLR_AUTO_COMPACT` | `0.8` | How full a chat's context window gets before orx compacts it, as a fraction (`0.8`) or a percentage (`80`, `80%`); `0` turns autocompaction off |

## Keys and secrets

No key, token, password or hostname belongs in this repository, and nothing here needs one. Agents keep their logins in their own config folders, and SSH uses your `~/.ssh/config`.

- Run `flavor/scripts/install-hooks.sh` once per clone. It refuses commits that contain credentials; it needs [gitleaks](https://github.com/gitleaks/gitleaks).
- CI scans every file VLResearch adds or changes.
- On GitHub, turn on secret scanning and push protection in the repository's security settings.

## Staying current with OpenResearch

OpenResearch releases every few days. `flavor/scripts/sync-upstream.sh` merges the newest release on a review branch, rebuilds generated files, and runs the checks. VLResearch's changes to upstream files are short hooks marked `VLResearch:`, so conflicts stay small. [VLRESEARCH.md](VLRESEARCH.md) covers the layout and the procedure.

## License

MIT, like OpenResearch. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
