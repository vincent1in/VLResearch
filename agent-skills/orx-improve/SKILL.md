---
name: orx-improve
description: "Run the forecast-and-verify improvement loop on a scoped project: pick the next idea, check its forecast, build it on a child of the best experiment, run it, measure it against the forecast, and keep a new best only past the noise floor. Use when asked to improve a model, dataset or process over several experiments, or to keep going on a project with a confirmed brief."
---

## Before the loop

The project needs a confirmed brief: `orx brief show <projectId>`. Without
one, scope the work first with `orx-grill`.

## One round

1. **Select.** `orx idea next <projectId>`. Work on what it picks. If you
   disagree, say why and change the idea's priority or blockers rather than
   skipping it silently.
2. **Predict.** Revisit the idea's forecast with what you now know, and with
   `orx idea calibration <projectId>`: shift forecasts by the reported bias,
   and widen intervals if fewer than about 80% of outcomes land inside them.
   Edit the forecast (`orx idea edit <ideaId> --forecast ... --interval ...`)
   before the run, never after.
3. **Build.** Create the experiment as a child of the current best
   (`orx metric best <projectId>` prints its experiment id), changing only
   what the brief allows. Locked files are checked against the exact commit
   that runs.
4. **Run.** `orx idea start <ideaId> <expId>`, then launch. When the full
   idea is expensive, run its kill test first. The chat is woken when runs
   finish, and told if one stalls or diverges.
5. **Verify.** `orx idea done <ideaId> --evidence "<runs and numbers>"`.
   orx measures the change against the parent, compares it with the noise
   floor, and keeps or refutes the idea.
6. **Learn.** Record what the result says beyond this one idea as a finding,
   with the runs as evidence (`orx-findings`), whether the idea was kept or
   refuted.
7. **Retain.** A new best counts only past the noise floor
   (`orx metric best`). Before building on it, queue a replicate
   (`orx idea add <projectId> "Replicate <idea>" --kind replicate`) with new
   seeds, and keep it as the incumbent only if the gain holds. orx also
   queues ablation checks for the biggest earlier steps behind a new best
   (`orx-ideas`); `orx idea next` runs the replicate, then those, before new
   ideas. Drop the ablations if the replicate fails.

Report each round in two or three lines: the idea, forecast against
outcome, kept or refuted, and the best against the baseline.

## Guards

- Select on validation only. Touch the out-of-sample split only at
  milestones, and say so each time you do.
- Count every trial: close every idea you start, refuted ones included.
- Process-efficiency ideas optimize wall time or cost, and their output must
  match the baseline's. Check that before claiming the gain.
- Dataset additions get a point-in-time and coverage check before any
  modeling.
- orx compacts the chat between turns when its context fills up. When
  earlier rounds survive only as a summary, re-read the state before the
  next step: `orx brief show`, `orx metric best`, `orx idea list` and the
  status of runs you launched. The ledgers are the record, not the summary.
- Stop when the brief's stop rule fires or its budget runs out. Then report
  the best against the baseline, the ideas that built it (the path in
  `orx metric best`), what was refuted, and how calibrated the forecasts
  were.
