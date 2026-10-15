---
name: orx-improve
description: "Run the forecast-and-verify improvement loop on this chat's scoped optimization: pick the next idea, check its forecast, build it on a child of the best experiment, run it, measure it against the forecast, and confirm each new best with a rerun. Use when asked to improve a model, dataset or process over several experiments, or to keep going on an optimization with a confirmed brief."
---

## Before the loop

The loop runs this chat's optimization, which needs a confirmed brief:
`orx brief show <projectId>`. Its best, budget and idea queue are its own:
the best is measured over the runs launched for it, so other chats'
optimizations in the project don't move it. Without
one, scope the work first with `orx-grill`; `orx idea next` and
`orx idea start` refuse until the user has confirmed it.

## One round

1. **Select.** `orx idea next <projectId>`. Work on what it picks. If you
   disagree, say why and change the idea's priority or blockers rather than
   skipping it silently.
2. **Predict.** Revisit the idea's forecast with what you now know. Once
   five ideas have been tested, `orx idea next` also prints the forecast as
   past misses suggest reading it: shifted by your mean error, with the
   interval stretched until about 80% of past outcomes would have landed
   inside. Take that reading unless you know something it can't. Edit the
   forecast (`orx idea edit <ideaId> --forecast ... --interval ...`) before
   the run, never after.
3. **Build.** Create the experiment as a child of the current best
   (`orx metric best <projectId>` prints its experiment id), changing only
   what the brief allows. Locked files are checked against the exact commit
   that runs.
4. **Run.** `orx idea start <ideaId> <expId>`, then launch. When the full
   idea is expensive, run its kill test first. The chat is woken when runs
   finish, and told if one stalls or diverges.
5. **Verify.** `orx idea done <ideaId> --evidence "<runs and numbers>"`.
   orx measures the change against the parent, compares it with the noise
   floor (0 unless the brief set one), and keeps or refutes the idea.
6. **Learn.** Record what the result says beyond this one idea as a finding,
   with the runs as evidence (`orx-findings`), whether the idea was kept or
   refuted.
7. **Retain.** When a kept idea makes a new best, `orx idea done` queues a
   replicate of it, and `orx idea next` takes it before new ideas. Run the
   same experiment again (`orx exp run <expId>`), which draws a new seed,
   then `orx idea start <replicateId> <expId>` and
   `orx idea done <replicateId>`. orx scores an experiment by the mean of its
   runs: if the gain was luck, the mean falls back, the best returns to what
   it was, and the idea it repeated is refuted. Build only on a best whose
   replicate held. When ablation checks are turned on (`VLR_AUTO_ABLATE`),
   orx queues those too (`orx-ideas`), and drops them if the replicate
   fails.

Report each round in two or three lines: the idea, forecast against
outcome, kept or refuted, and the best against the baseline.

## Guards

- Select on validation only. Touch the out-of-sample split only at
  milestones, and say so each time you do.
- Count every trial: close every idea you start, refuted ones included.
- Process-efficiency ideas optimize wall time or cost, and their output must
  match the baseline's: guard it (`orx metric guard <projectId> output_diff
  --max 0`, `orx-metrics`), and orx won't keep one that changes it.
- Dataset additions get a point-in-time and coverage check before any
  modeling.
- orx compacts the chat between turns when its context fills up. When
  earlier rounds survive only as a summary, re-read the state before the
  next step: `orx brief show`, `orx metric best`, `orx idea list` and the
  status of runs you launched. The ledgers are the record, not the summary.
- When a constraint besides the primary metric matters (turnover, latency,
  memory), guard it rather than trusting yourself to check it each round.
- Stop when the brief's stop rule fires or its budget runs out. Then report
  the best against the baseline, the ideas that built it (the path in
  `orx metric best`), what was refuted, and how calibrated the forecasts
  were.

## Improving the project's own tools

The loop can improve the scripts it runs on, too: a slow data loader, a
flaky download, a backtest that recomputes what it could cache. Run that as
an optimization of its own, in its own chat, scoped to the tool's paths
(`orx brief set ... --scope <path>`). Its primary metric is what the run
reports about the tool (its seconds, or its failures over a fixed set of
inputs), and a guard holds its output to the baseline's. The evaluation code
stays locked, as in every optimization.

When a procedure recurs across chats (staging data on a host, running the
backtest), offer the user to save it as a skill (`orx-customize`) so later
chats start from it.
