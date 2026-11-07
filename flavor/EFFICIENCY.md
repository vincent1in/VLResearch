# Efficiency ledger

This ledger is shared by every VLResearch install and kept in this repository. It lists each change that makes the same research cheaper, with its evidence, and it is the only route by which such a change becomes a VLResearch default: results pooled across installs must agree, and the maintainer merges. A change that fits one researcher (their instructions, hosts, data or conventions) never goes here. It goes in that researcher's usage profile, described in [USAGE.md](USAGE.md).

As of 7 November 2026, at VLResearch 9f37717 (OpenResearch v0.2.24); line numbers are at that commit. One install has run VLResearch so far, so no entry has pooled evidence yet. Every number below is an estimate, a result of VLResearch's own simulation, or a result reported by a cited source, and each says which.

## What goes here

Cost is counted in three units, in this order (the maintainer's decision of 3 November):

1. **Wall-clock time** to a verified result: the agent's turns, the runs and the waiting between them. When delegated chats run in parallel, agent-hours are recorded beside it.
2. **CPU-hours** on the machines that run the work.
3. **Tokens**, split into uncached input, cache reads, cache writes, output and reasoning, as OpenResearch's `TokenUsage` already records them (`src/store/telemetry.rs` L52).

The decision metric is the one the self-change gate uses (plan addition A10): **verified yield per hour**, the true gain the loop ends with on the shared planted panels (benchmark tier L1), per hour of wall-clock time, within the token and CPU budgets. A change that saves time must raise it. A change that saves only CPU-hours or tokens must lower that unit per unit of true gain, and must not lower verified yield per hour by more than the margin δ below.

A change is an efficiency candidate only if all three of these hold:

1. **Nobody asked for it as a preference.** A researcher's instruction or correction is that researcher's usage item, applied at once on their install. If the loop finds a general cause behind it, it opens a separate candidate here.
2. **It names nothing that only one researcher has**: no paths, hosts, repositories, accounts, identities, datasets, schedules or domain conventions.
3. **Its gain holds on the shared planted panels**, measured on the install that proposes it. A gain that shows only on one researcher's own studies is a usage adaptation for that researcher.

A change with a general mechanism and a personal setting is split. The mechanism comes here with a default; one researcher's setting of it goes in their profile. The stall alert is shared; whether it fires after 30 minutes or 90 is per researcher.

**Protected parts never change through this ledger**: the evaluator and its seal, the keep rule and its thresholds, the benchmark and its planted panels, the self-change gate, the action log and the budget caps. Entries that touch them are listed for the record, marked *protected*. Changing one starts a new benchmark version, and the maintainer decides.

## How an entry moves

| Status | Meaning |
| --- | --- |
| candidate | Proposed, with its expected saving and the evidence that will show it fired. |
| testing | At least one install is running the paired test. |
| provisional | Positive on fewer researchers' installs than the rule needs. On only where a researcher opts in through their profile. |
| default | Passed the consensus rule. The maintainer merged the change that turns it on for every install. |
| opt-in | Positive on average but not for every install. Ships off, as a setting a researcher can turn on. |
| rejected | Pooled effect below zero, or still no different from zero after six researchers. Kept here with its numbers, so no install tests it again without a new reason. |
| retired | Superseded, usually by an OpenResearch release that does the same thing. |

Entries built before this ledger existed carry *in VLResearch* (on or off by default) and *planned* (with their work package) until pooled evidence moves them.

## What an install contributes

Nothing leaves an install by default, and VLResearch's egress gate does not change. A researcher who wants to contribute exports an evidence file and opens a pull request against this repository, or sends the file to the maintainer. The file holds harness-only numbers:

- the entry ID, the VLResearch commit, the OpenResearch version, and the names of the coding agent and model;
- the panel set and its version, the number of paired campaigns and, for each campaign, the log ratio of candidate to incumbent for verified yield per hour, for CPU-hours per unit of true gain and for tokens per unit of true gain, and the false keeps on the planted nulls;
- a random install ID that is not tied to an account, and the date.

It never holds research data, code, prompts, transcripts, file paths, hostnames, or project or dataset names. Both sides of a comparison run on the same install, with the same agent, model, panels and seeds, interleaved in time. Comparing one machine's candidate with another machine's incumbent would count machine speed as a gain: in one autoresearch run, about 82% of an apparent 3.53% gain was machine speed [autoresearch #651].

## The consensus rule

For an entry with results from k researchers, let yᵢ be researcher i's mean paired log ratio and sᵢ its standard error. A researcher's several installs are pooled first, so k counts researchers, not machines.

1. Pool with a random-effects model: the between-researcher variance τ² by REML, the pooled mean μ̂, and its 95% interval by the Hartung–Knapp–Sidik–Jonkman method, with its variance factor floored at 1 so that a few installs that happen to agree cannot narrow the interval below their own standard errors. In simulations of 2 to 20 studies, the usual DerSimonian–Laird interval's error rate at a nominal 5% could exceed 30%, while HKSJ's at most doubled, and HKSJ's advantage was largest with few studies [HKSJ].
2. Compute the 80% prediction interval for a new researcher's install: μ̂ ± t₀.₉₀,ₖ₋₂ · √(τ̂² + SE(μ̂)²) [Higgins et al.].
3. Decide:
   - **default**: k ≥ 3; the 95% interval's lower bound above 0; the prediction interval's lower bound above log(1 − δ), with δ = 0.05; no canary rollback left unexplained on any install; no protected case regressed; and the maintainer merges.
   - **opt-in**: the 95% interval's lower bound above 0, but the prediction interval reaches below log(1 − δ).
   - **rejected**: the 95% interval's upper bound below 0, or k ≥ 6 with an interval that still includes 0.
   - **provisional**: k < 3.

**Why the prediction interval.** A pooled gain can be significant while a new install loses. Of 479 significant random-effects meta-analyses with heterogeneity in the Cochrane database (2009 to 2013), 72.4% had a 95% prediction interval that included no effect or the opposite effect [IntHout et al.]. The interval asks whether the next install is likely to gain, which is what a default promises.

**Why pooling pays.** Testing one candidate on two planted campaigns costs about the time, tokens and CPU-hours of 60 research hypotheses (estimate, paper section 10.5). On one install, a kept 10% saving repays its test after about 600 later hypotheses, and a 30% saving after about 200. When every install adopts a result that a few installs tested, those hypotheses accumulate across all of them.

**Why consensus at all.** Gains measured on the set a harness evolved against were mostly overfitting and extra inference: 54% and 14% of the gain on average (independent) [Harness-Delta Attribution]. A harness at 94.0% on one benchmark fell to 32.0% after five rounds of evolution on another (independent) [Continual evolution]. And hacks travel through shared records: clean coders shown five passing hacked examples submitted hacks 73.6% of the time (from a machine-written summary of the paper) [Transmissible reward hacking]. So one install's result changes only that install, through its researcher's profile, and only results that hold across researchers, on panels with known truth, change the defaults.

δ, k and the 80% level are starting values (open decision 19 of the plan); what installs share and how is open decision 20.

## How the ledger feeds VLResearch

- **Defaults.** An entry at *default* is on in the flavor's code, skills or settings, and the release notes name it.
- **The self-improvement loop.** Before proposing, the loop (A18 and A10) reads this ledger. It does not propose a rejected entry again without new evidence; it tests candidates and provisional entries with the largest expected saving first; and it files each diagnosis against an existing entry or opens a new one.
- **Researchers' profiles.** A profile can opt in to provisional and opt-in entries and opt out of defaults. On that researcher's install the profile wins, and the update log records the override.
- **Upstream.** An entry that changes OpenResearch's own files is marked as an upstream candidate. If OpenResearch takes it, VLResearch drops its hook and the entry is retired (open decision 21).

## After each OpenResearch merge

When the release watcher merges an upstream release on `sync/<tag>`, each entry whose **Where** names a file the release changed is checked before the pull request is merged: its hook is still on the path it needs (under both values of `ORX_RUN_ENGINE` from v0.2.25 on), and its planted fault in tier L1f is still found. An entry the release now covers is retired, naming the release. Of the 30 upstream files with VLResearch hooks, 24 changed in v0.2.25 and one was deleted, so E1 to E5 need this check at the next merge.

## Entries

### Summary

| ID | Change | Saves | Status | Pooled |
| --- | --- | --- | --- | --- |
| E1 | Wake-ups for every run a chat launches | time, tokens | in VLResearch, on | — |
| E2 | Stall alerts | CPU-hours, time | in VLResearch, on (30 min) | — |
| E3 | Autocompaction when a chat is idle | tokens, time | in VLResearch, on (80%) | — |
| E4 | Run and day budgets at launch | CPU-hours, time | in VLResearch, on with a brief; *protected* | — |
| E5 | Probe runs that never count as results | time, CPU-hours | in VLResearch | — |
| E6 | Ablation checks off during the search | CPU-hours, time | in VLResearch, off by default | — |
| E7 | A replicate on every new best | spends CPU-hours to avoid false keeps | in VLResearch, on; to become conditional (W4); *protected* | — |
| E8 | Claude Code's checklist tool | unmeasured | in VLResearch, on; to become a researcher's setting (W4) | — |
| E9 | Time, token and CPU-hour accounts, with budgets | measures everything else | planned, W1 | — |
| E10 | Probe, subsample and screen | time, CPU-hours | planned, W3 | — |
| E11 | Fewer model calls per hypothesis | tokens, time | planned, W4 | — |
| E12 | Shorter skill text | tokens | planned, W4 | — |
| E13 | A fresh session every five hypotheses | tokens, time | planned, arm C4 | — |
| E14 | The project model as an index with retrieval | tokens, time | planned, W8 | — |
| E15 | Waste detectors | all three | planned, W8 | — |
| E16 | Stop rule and stall cohort | time, CPU-hours | planned, W5 | — |
| E17 | Refusal reasons and hypothesis lineage | time, CPU-hours | planned, W5 | — |
| E18 | A research pack at brief time | time, CPU-hours | planned, W6 | — |
| E19 | Attribution once, at study end | CPU-hours, time | planned, W6 | — |
| E20 | Self-changes screened on the fewest planted panels | time, tokens, CPU-hours | planned, W9 | — |
| E21 | A cheaper model for routine calls | tokens | candidate | — |
| E22 | Single-shot edits for small changes | tokens, time | candidate | — |
| E23 | SoL-Pi's other three token mechanisms | tokens | candidate | — |
| E24 | Independent hypotheses in parallel chats | time | candidate, after v0.2.25 | — |
| E25 | OpenResearch's run engine | time, CPU-hours | upstream, v0.2.25, off by default | — |
| E26 | OpenResearch's sub-agents on `orx mcp` | time | upstream, v0.2.25 | — |
| E27 | OpenResearch's workspace chat search | tokens, time | upstream, v0.2.25 | — |
| E28 | Forecast-led selection | — | rejected | — |
| E29 | A noise floor at the spread of the seed runs | — | rejected; *protected* | — |
| E30 | An agent-written project summary at every session start | — | rejected | — |

Each entry below gives what it **saves**, its **status**, **where** it lives, the **evidence** so far, the **pooled** result, what to re-check **on update**, and whether it is an **upstream** candidate.

### In VLResearch

#### E1 Wake-ups for every run a chat launches
- **Saves:** time and tokens. The agent ends its turn instead of polling and hears back when the run ends.
- **Status:** in VLResearch, on. `VLR_AUTO_WAKE=0`, or the Settings page, returns to OpenResearch's opt-in wake-ups.
- **Where:** `src/flavor/wake.rs`; hooks in `src/compute.rs` L944, `src/plane/local_plane.rs` L410 and L483, `src/invocation.rs` L31, `src/local/opencode.rs` L1512 and `src/commands/exp.rs` L179 and L219. Under Claude Code, a long `orx exp wait` also returns before Claude Code's 2-minute Bash timeout.
- **Evidence:** none measured. Turning it off is one of the six planted faults of tier L1f.
- **Pooled:** none yet.
- **On update:** v0.2.25 launches through its new engine (`compute::submit`), which the hook at `compute.rs` L944 does not reach. Re-check with `ORX_RUN_ENGINE=1`. Retire if the engine's run subscriptions and notices do the same job.
- **Upstream:** candidate. OpenResearch wakes a chat only when the agent remembered `orx exp wake`.

#### E2 Stall alerts
- **Saves:** CPU-hours and time on runs that hang, print nothing or have already failed.
- **Status:** in VLResearch, on. A run quiet for 30 minutes, a non-finite metric or an out-of-memory error is reported to its chat; the watcher scans at most every 30 seconds. `VLR_STALL_MINUTES` sets the window, and 0 turns it off.
- **Where:** `src/flavor/stall.rs`; hook in `src/local/chat/mod.rs` L8698.
- **Evidence:** none measured. Turning it off is a planted fault of tier L1f.
- **Pooled:** none yet.
- **On update:** with the v0.2.25 engine on, supervision moves into the engine and, on SSH hosts, into a per-run `orx-host`. Re-check that alerts still fire on engine runs.
- **Upstream:** candidate.

#### E3 Autocompaction when a chat is idle
- **Saves:** tokens, and the time of a compaction that would otherwise interrupt a turn.
- **Status:** in VLResearch, on. A chat past 80% of its context window is compacted as soon as the turn that filled it ends and nothing else is waiting; at least three turns run between automatic compactions, and a chat whose compaction failed or was stopped is left alone for 30 minutes. It needs the window's size, which Claude Code and Codex report, OpenCode only for local models, and Cursor and Antigravity not at all. `VLR_AUTO_COMPACT` sets the threshold, and `off` or 0 turns it off.
- **Where:** `src/flavor/compact.rs`, run through `src/flavor/chat_host.rs`; hook in `src/local/chat/mod.rs` L6067.
- **Evidence:** none measured here. Online context compaction was one of four mechanisms that together cut SoL-Pi's tokens by 44.7% to 49.0% (self-reported; see E23).
- **Pooled:** none yet.
- **On update:** v0.2.25 keeps chat context across agent switches; re-check that the two do not compact or re-seed the same chat twice.
- **Upstream:** candidate.

#### E4 Run and day budgets at launch *(protected)*
- **Saves:** CPU-hours and time, by refusing a launch past a budget the brief sets.
- **Status:** in VLResearch, on whenever a confirmed brief sets the budgets. The brief's GPU-hour budget is shown but never checked; E9 replaces it with a checked CPU-hour budget and adds tokens.
- **Where:** `check_launch`, `src/flavor/scope.rs` L1041–1054; the scoping gate is called from `src/compute.rs` L808, and the locked-path check from L850.
- **Evidence:** from code.
- **Pooled:** not applicable; budget caps are protected.
- **On update:** v0.2.25's engine launches through `compute::submit`, which neither the gate nor the locked-path check hooks. Keep the engine off until both are on its path, with a test under `ORX_RUN_ENGINE=1`.
- **Upstream:** no.

#### E5 Probe runs that never count as results
- **Saves:** time and CPU-hours, by letting the agent try a pipeline cheaply before a full run. It is the base for E10.
- **Status:** in VLResearch. A run launched with `VLR_PROBE=1` passes the launch gate and never counts as a result.
- **Where:** `src/flavor/scope.rs` L15–16 and L37; `src/flavor/metrics.rs` L811–819. Probe runs are left out of the budget's usage (`scope.rs` L807–827); under E9 they count toward the time and CPU accounts, though never toward results.
- **Evidence:** from code; no saving measured.
- **Pooled:** none yet.
- **On update:** re-check that `VLR_PROBE` still reaches the gate on the engine path.
- **Upstream:** no.

#### E6 Ablation checks off during the search
- **Saves:** CPU-hours and time.
- **Status:** in VLResearch, off by default (`VLR_AUTO_ABLATE`). E19 moves attribution to the end of a study.
- **Where:** `src/flavor/ideas.rs` L822; `queue_ablations` L846.
- **Evidence:** VLResearch's simulation, 2,000 campaigns per scenario: with the checks on, they took 10.6 of a 40-hour budget and the true gain fell from 4.61 to 4.01 seed-noise units in the typical scenario (from code; `flavor/bench/loop.md`).
- **Pooled:** none yet.
- **On update:** none needed; the code is VLResearch's own.
- **Upstream:** no.

#### E7 A replicate on every new best *(protected)*
- **Saves:** nothing directly. It spends CPU-hours to keep false gains out.
- **Status:** in VLResearch, on (`VLR_AUTO_REPLICATE`). Under the plan, replicates run only for stochastic pipelines (W4; open decision 1).
- **Where:** `new_best_checks`, `src/flavor/ideas.rs` L749; the switch at L818.
- **Evidence:** VLResearch's simulation: replicates raised true gain by 0.20 to 0.29 in five of seven scenarios, at about a fifth of the simulated budget; without them, the typical scenario's true gain was 4.35 against 4.61 and its overclaim 1.74 against 1.10 (from code).
- **Pooled:** not applicable; the keep rule is protected.
- **On update:** none needed.
- **Upstream:** no.

#### E8 Claude Code's checklist tool
- **Saves:** unmeasured. It may cost tokens on every turn.
- **Status:** in VLResearch, on under Claude Code (`VLR_AGENT_CHECKLIST`, which sets `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`). The plan makes it opt-in (W4), which turns it into a researcher's setting, recorded in their usage profile, rather than a default.
- **Where:** `src/flavor/harness.rs` L25; hook in `src/local/chat/mod.rs` L8734.
- **Evidence:** none.
- **Pooled:** none yet.
- **On update:** re-check that Claude Code still reads the variable.
- **Upstream:** no.

### Planned

#### E9 Time, token and CPU-hour accounts, with budgets (A1, W1)
- **Saves:** nothing by itself. Every other entry is measured with it, and a checked CPU-hour budget stops overspending.
- **Status:** planned, W1 (about 2 days).
- **Where:** on each idea: wall-clock time from start to close, split into agent turns, runs and waiting; tokens summed from `chat_usage_samples` (`src/store.rs` L734) through `chat_usage_executions` and `chat_turns`, split as above; CPU-hours from the metrics helper's metric lines; and the steps or rows each run completed. `budget_tokens` on the brief; the GPU-hour budget becomes a CPU-hour budget; both are checked in `check_launch` beside the run and day budgets. Agent-hours are kept beside wall-clock time once delegated chats run in parallel.
- **Evidence:** steps and rows are recorded because machine speed was about 82% of one apparent 3.53% gain [autoresearch #651].
- **Pooled:** not applicable; it is the instrument.
- **On update:** re-check token sums against the store's tables, which v0.2.25 extends.
- **Upstream:** the CPU and step accounting could be offered upstream.

#### E10 Probe, subsample and screen (A19, W3)
- **Saves:** time first, then CPU-hours.
- **Status:** planned, W3. Done when, on L1, the screen keeps at least 95% of planted gains of the target size and at least halves the wall-clock time per hypothesis.
- **Where:** an `orx probe` ladder of stratified data fractions and cheaper tests run as probe runs (E5); the smallest rung whose contrasts keep their signs and rank as at full size (Spearman at least 0.8) is hashed into the brief; each hypothesis runs there first and is screened out only on an error, a leak alarm or a gain below a lenient floor; survivors run at full size, where `judge` decides. The kill-test field (`src/flavor/cli.rs` L251) becomes this screen.
- **Evidence:** estimate: if six hypotheses in ten fail a screen that takes a tenth of a full run, a hypothesis costs about half a full run. Hyperband's small budgets spent broadly gave over an order-of-magnitude speedup in hyperparameter search [Hyperband]; 100 curated examples estimated a model's score on MMLU's 14,000 [tinyBenchmarks]; AgenticEvolve matched a baseline's quality with 3 to 26 formal evaluations against 180 [AgenticEvolve]. Whether a subset represents the whole depends on the domain: 5 of 112 time-series datasets ranked models at a Spearman correlation of 0.95, while recommender-system subsets did little better than random ones [Tasks That Matter]. So the probe measures agreement on each study's own data.
- **Pooled:** none yet.
- **On update:** a new dataset version or evaluator re-probes; an OpenResearch release does not.
- **Upstream:** no.

#### E11 Fewer model calls per hypothesis (W4)
- **Saves:** tokens and time.
- **Status:** planned, W4 (2 to 3 days): the forecast layer goes (E28); ablations move to the end of a study (E19); replicates become conditional (E7); the checklist becomes opt-in (E8); and OpenResearch's tree-loop section in chats is overridden, so one loop and one stop rule run per chat.
- **Evidence:** estimate from one traced hypothesis: from about 14 model calls to about 8, which saves about 36% of its tokens and six rounds of model latency.
- **Pooled:** none yet.
- **On update:** re-check the override of OpenResearch's tree-loop section after each merge, since OpenResearch updates its bundled skills with most releases.
- **Upstream:** no.

#### E12 Shorter skill text (W4)
- **Saves:** tokens: about 2,200 fewer tokens of skill text, about 2% of a hypothesis's tokens (estimate).
- **Status:** planned, W4, with the rewrites of `orx-ideas` and `orx-improve`.
- **Evidence:** estimate.
- **Pooled:** none yet.
- **On update:** OpenResearch's skill tests check descriptions; re-run them.
- **Upstream:** no.

#### E13 A fresh session every five hypotheses (arm C4)
- **Saves:** tokens and time: a session seeded from the record replaces one that keeps growing.
- **Status:** planned; tested as arm C4 against C3.
- **Evidence:** estimate: 10% to 55% of tokens, and the compaction turns that stall a long session.
- **Pooled:** none yet.
- **On update:** re-check against v0.2.25's context kept across agent switches.
- **Upstream:** no.

#### E14 The project model as an index with retrieval (A17, W8)
- **Saves:** tokens and time from a fresh session's start to its first launch, and the runs spent on hypotheses that repeat refuted ones.
- **Status:** planned, W8. `orx recap` loads a short index within a fixed token budget, and the agent retrieves the rest for each decision.
- **Evidence:** late in an autoresearch loop, a judge given focused retrieval of similar earlier attempts chose correctly 83.5% of the time, against 80.9% with a summary and 70.8% with the whole history (self-reported) [REHEARSE]; repository context files raised inference cost by over 20% without raising success (independent) [AGENTS.md study].
- **Pooled:** none yet.
- **On update:** v0.2.25's workspace chat search (E27) can serve the retrieval.
- **Upstream:** no.

#### E15 Waste detectors (A18, W8)
- **Saves:** all three units, by pointing each change at a cost that can be seen.
- **Status:** planned, W8. After each block of hypotheses, queries over the store count: waiting that a registered wake-up would have removed; runs that kept using CPU after a stall, NaN or out-of-memory alert; compactions and retries per hypothesis; files re-read across turns; refusals that repeat a reason; hypotheses that repeat a refuted one; hypotheses that pass once and fail on their repeat; and screens that full-size runs contradict. Each count is filed against an entry here or opens a candidate.
- **Evidence:** structural signals come first because an LLM judge detected failures at an AUROC of 0.433, below the 0.716 of step count alone (self-reported) [Structural Baselines].
- **Pooled:** not applicable; it finds candidates.
- **On update:** re-check the queries against tables v0.2.25 changes.
- **Upstream:** no.

#### E16 Stop rule and stall cohort (A6, W5)
- **Saves:** time and CPU-hours on a search that has stopped finding gains.
- **Status:** planned, W5. A slow block brings one cohort of four proposals anchored on different kept steps; a second slow block halts the study until the researcher closes it.
- **Evidence:** none measured; arm C6 tests the cohort.
- **Pooled:** none yet.
- **On update:** none needed.
- **Upstream:** no.

#### E17 Refusal reasons and hypothesis lineage (A3, A4, W5)
- **Saves:** time and CPU-hours: refuted ideas do not come back under new names.
- **Status:** planned, W5. A refusal gives a reason from a fixed list; a hypothesis revised after a screen or an attempt is a new hypothesis linked to its parent.
- **Evidence:** a debug consultant that shared runtime constraints across branches cut repeated bugs from 46.0% to 7.8% of AIDE's nodes and raised its gold medals from 22 to 38 (self-reported) [Recovering Wasted Compute].
- **Pooled:** none yet.
- **On update:** none needed.
- **Upstream:** no.

#### E18 A research pack at brief time (A9, W6)
- **Saves:** time and CPU-hours: a hypothesis that duplicates a known result is closed before it runs.
- **Status:** planned, W6. Built once per study from the project's earlier findings, an index of relevant papers with their main results, and open questions, and shared by every proposer [After42]. It needs literature access under the egress rule (open decision 5).
- **Evidence:** none measured.
- **Pooled:** none yet.
- **On update:** none needed.
- **Upstream:** no.

#### E19 Attribution once, at study end (A8, W6)
- **Saves:** CPU-hours and time against ablations during the search (E6).
- **Status:** planned, W6. `orx attribute` runs leave-one-out runs per kept step, and pairwise runs where estimates disagree, at the end of a study.
- **Evidence:** see E6.
- **Pooled:** none yet.
- **On update:** none needed.
- **Upstream:** no.

#### E20 Self-changes screened on the fewest planted panels (A19 applied to A10, W9)
- **Saves:** time, tokens and CPU-hours of testing self-changes.
- **Status:** planned, W9. The probe's procedure picks the smallest set of planted panels that ranks earlier harness changes as the full set does; candidates are screened there before their full test, which runs between studies and in batches.
- **Evidence:** estimate: one candidate on two planted campaigns costs about the time, tokens and CPU-hours of 60 research hypotheses. Infinity Machine's only self-derived harness was rejected for costing 2.02 times the incumbent, against its cap of 1.5 (self-reported) [Infinity Machine].
- **Pooled:** none yet.
- **On update:** none needed.
- **Upstream:** no.

### Candidates for the harness loop

These trade tokens or time against the quality of proposals, so they go through the harness loop (A10) and its gate rather than being made by hand.

#### E21 A cheaper model for routine calls
- **Saves:** tokens.
- **Status:** candidate. Routine calls (formatting, log reading, status checks) go to a cheaper model; proposals and verdicts stay where they are.
- **Evidence:** none yet. Non-inferiority on verified yield per hour decides it.
- **Pooled:** none yet.
- **On update:** re-check the harness adapters' model settings.
- **Upstream:** no.

#### E22 Single-shot edits for small changes
- **Saves:** tokens and time.
- **Status:** candidate. A one-line change to a parameter or a feature is written in one call, without a planning round.
- **Evidence:** none yet.
- **Pooled:** none yet.
- **On update:** none needed.
- **Upstream:** no.

#### E23 SoL-Pi's other three token mechanisms
- **Saves:** tokens.
- **Status:** candidate: action fusion, observation packing and an evidence-preserving reducer. The fourth, online context compaction, overlaps E3.
- **Evidence:** the four together cut recorded tokens by 49.0% with GPT-5.6 Sol and 44.7% with Opus 5, keeping 93.7% and 94.3% of the baseline harness's score, and API cost fell by about a third; no seeds are reported (self-reported) [SoL-Pi]. A loss of about 6% of score is a cost to set in advance, not a tie.
- **Pooled:** none yet.
- **On update:** none needed.
- **Upstream:** no.

#### E24 Independent hypotheses in parallel chats
- **Saves:** wall-clock time. Agent-hours and tokens rise, so all three are recorded (E9).
- **Status:** candidate, after the v0.2.25 merge: hypotheses that do not build on each other run in delegated chats (E26).
- **Evidence:** none yet.
- **Pooled:** none yet.
- **On update:** depends on E26.
- **Upstream:** no.

### From OpenResearch

These arrive with merges. They are listed so the loop does not rebuild them and so VLResearch's own entries are re-checked against them.

#### E25 The run engine (OpenResearch v0.2.25, #862)
- **Saves:** time and CPU-hours lost to runs that disconnect or end on a timer. Run subscriptions and notices; a per-run `orx-host` on SSH hosts and rented boxes that keeps execution and receipts across disconnects; a run ends only on evidence.
- **Status:** upstream, v0.2.25, not merged; off by default (`ORX_RUN_ENGINE`), always on in canary builds.
- **Evidence:** none measured.
- **Pooled:** none yet.
- **On update:** keep it off until VLResearch's launch checks and wake-up are on its path (E1, E2, E4, E5).
- **Upstream:** it is upstream's.

#### E26 Sub-agents on `orx mcp` (OpenResearch v0.2.25, #885)
- **Saves:** time, by letting a chat delegate work and wait on it (`delegate`, `subscribe`, `wait`, `status`, `read`, `send`, `answer`); `orx agent spawn` is removed.
- **Status:** upstream, v0.2.25, not merged. Before it is used, the reviewer and the evaluator are sealed from it, since `read`, `status` and `subscribe` accept any chat id (A7).
- **Evidence:** none measured.
- **Pooled:** none yet.
- **On update:** enables E24.
- **Upstream:** it is upstream's.

#### E27 Workspace chat search (OpenResearch v0.2.25, #896)
- **Saves:** tokens and time, if it serves E14's retrieval. v0.2.25 also keeps chat context across agent switches.
- **Status:** upstream, v0.2.25, not merged.
- **Evidence:** none measured.
- **Pooled:** none yet.
- **On update:** re-check E3 and E13 against it.
- **Upstream:** it is upstream's.

### Rejected

#### E28 Forecast-led selection
- **What it was:** ranking ideas by expected gain per estimated compute-hour from the agent's own forecasts, with calibration, a forecast adjustment and a class scoreboard.
- **Status:** rejected; cut in W4 (E11). A point forecast still orders the queue and never vetoes an idea.
- **Evidence:** in VLResearch's simulation the forecast-led loop found no more true gain than keeping the higher score in forecast order (−0.05 ± 0.09 in the typical scenario; from code). The best LLM forecaster in one study explained about half the variance of experiment outcomes [Outcome forecasting].

#### E29 A noise floor at the spread of the seed runs *(protected)*
- **What it was:** keeping a change only if it beat the spread of the baseline's seed runs.
- **Status:** rejected; replaced by keeping any gain with replicates, and under the plan by a paired per-period test (A2).
- **Evidence:** in VLResearch's simulation the floor cut the typical scenario's true gain from 4.61 to 1.67 (with ablations on, as in the old guidance; from code).

#### E30 An agent-written project summary at every session start
- **What it was:** loading an agent-written summary of the project into every session.
- **Status:** rejected in favour of E14.
- **Evidence:** a summary served a judge worse than retrieval (80.9% against 83.5%) [REHEARSE]; context files raised inference cost by over 20% without raising success [AGENTS.md study]; and an agent-written record can carry a gamed result into every later session [Transmissible reward hacking].

## Adding or changing an entry

Open a pull request that edits this file, one entry per pull request. A new entry gets the next number and the fields above. A status change cites its evidence files and the pooled numbers. A rejected entry stays, with its numbers.

## Sources

- [autoresearch #651] karpathy/autoresearch, issue #651, 1 November 2026. https://github.com/karpathy/autoresearch/issues/651
- [HKSJ] J. IntHout, J. P. A. Ioannidis, G. F. Borm. The Hartung–Knapp–Sidik–Jonkman method for random effects meta-analysis is straightforward and considerably outperforms the standard DerSimonian–Laird method. *BMC Medical Research Methodology* 14, 25 (2014). https://doi.org/10.1186/1471-2288-14-25
- [Higgins et al.] J. P. T. Higgins, S. G. Thompson, D. J. Spiegelhalter. A re-evaluation of random-effects meta-analysis. *Journal of the Royal Statistical Society A* 172(1), 137–159 (2009). https://pmc.ncbi.nlm.nih.gov/articles/PMC2667312/
- [IntHout et al.] J. IntHout, J. P. A. Ioannidis, M. M. Rovers, J. J. Goeman. Plea for routinely presenting prediction intervals in meta-analysis. *BMJ Open* 6(7), e010247 (2016). https://bmjopen.bmj.com/content/6/7/e010247
- [Harness-Delta Attribution] W. Ding et al. Harness-Delta Attribution: Dissecting Gains From Harness Evolution. arXiv:2610.22466. https://arxiv.org/abs/2610.22466
- [Continual evolution] S. Zhao, Y. Zhang, J. Li. Can Coding Agents Sustain Continual Evolution without Forgetting? arXiv:2611.03219. https://arxiv.org/abs/2611.03219
- [Transmissible reward hacking] Y. Hu et al. Reward Hacking as a Transmissible Strategy in Multi-Agent Coding Systems. arXiv:2610.12822. https://arxiv.org/abs/2610.12822
- [Hyperband] L. Li et al. Hyperband: A Novel Bandit-Based Approach to Hyperparameter Optimization. arXiv:1603.06560. https://arxiv.org/abs/1603.06560
- [tinyBenchmarks] F. Maia Polo et al. tinyBenchmarks: evaluating LLMs with fewer examples. arXiv:2402.14992. https://arxiv.org/abs/2402.14992
- [AgenticEvolve] Peng et al. AgenticEvolve. arXiv:2610.31326. https://arxiv.org/abs/2610.31326
- [Tasks That Matter] R. Gusev, A. Zaytsev. Benchmarking on Tasks That Matter: Dataset Selection for Preserving Model Rankings. arXiv:2606.27997. https://arxiv.org/abs/2606.27997
- [REHEARSE] J. Ji, S. Ding. REHEARSE: Stepping Back from the Confidence Cliff in Self-Improving Autoresearch. arXiv:2607.27687. https://arxiv.org/abs/2607.27687
- [AGENTS.md study] T. Gloaguen et al. Evaluating AGENTS.md: Are Repository-Level Context Files Helpful for Coding Agents? arXiv:2602.11988. https://arxiv.org/abs/2602.11988
- [Structural Baselines] R. A. Naagarajan, S. Streif. Structural Baselines Matter for Agent Failure Detection and Attribution. arXiv:2610.16661. https://arxiv.org/abs/2610.16661
- [Recovering Wasted Compute] K. C. Au et al. Recovering Wasted Compute in Autoresearch Agents. arXiv:2608.10424. https://arxiv.org/abs/2608.10424
- [After42] F. Nie et al. Can AI Propose Interesting Research Questions? After42, 24 October 2026. https://after42.org/research/can-ai-propose-interesting-research-questions/
- [Infinity Machine] H. Santhanalakshmi Ganesan. Infinity Machine: a self-improving AI agent for research. arXiv:2610.35579. https://arxiv.org/abs/2610.35579
- [SoL-Pi] H. Liu et al. SoL-Pi: Recursively Scaling Auto-Research Loops for Efficient Agent Harness. arXiv:2609.20519. https://arxiv.org/abs/2609.20519
- [Outcome forecasting] A. Bansal et al. Can LLMs predict the outcomes of machine learning experiments? arXiv:2610.22599. https://arxiv.org/abs/2610.22599
- VLResearch's own simulation: `flavor/bench/loop.md`. The plan these entries come from: `docs/research/index.html`, section 11.
