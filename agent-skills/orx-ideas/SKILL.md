---
name: orx-ideas
description: "Queue and track ideas with `orx idea`: forecasts with 80% intervals, GPU-hour costs, kill tests, blockers and measured outcomes. Use before proposing or starting an experiment, when deciding what to try next, after a run finishes, and when the user asks what has been tried or what worked."
---

## Every experiment tests one idea

Put the idea in the queue before creating its experiment:

```sh
orx idea add <projectId> "<title>" --class incremental \
  --hypothesis "<why it should help>" \
  --kill-test "<the cheapest test whose failure ends it>" \
  --forecast 0.003 --interval -0.001 0.007 --cost-hours 2
```

- `--forecast` is the change you expect in the project's primary metric, in
  its units and sign: negative for a loss that should fall. `--interval` is
  your 80% interval, so about one outcome in five should land outside it.
- `--class` says how it relates to what was tried: `incremental`,
  `orthogonal`, `ablation`, `data`, `process` or `reproduction`.
- The kill test is an experiment under about one GPU-hour whose negative
  result ends the idea. When the full idea is expensive, run the kill test
  first.
- Blockers go in the same queue: `--kind infra-blocker` (the pipeline is
  broken), `--kind eval-blocker` (the evaluation is in doubt), `--kind halt`
  (the user must decide). An idea that waits on one takes
  `--blocked-by <ideaId>`.

## Pick, run, close

1. `orx idea next <projectId>` picks what to do: open blockers first; then
   checks on the current best (replicates, then ablations); then the highest
   expected gain per GPU-hour, computed from each forecast and interval; and
   an orthogonal idea after three incremental ones in a row or three tested
   ideas that kept nothing.
2. Create the experiment, then record the link:
   `orx idea start <ideaId> <expId>`.
3. When its runs have finished:
   `orx idea done <ideaId> --evidence "<runs and numbers>"`. orx measures the
   experiment's change against its parent on the primary metric and marks
   the idea kept (beat the noise floor) or refuted.
4. A kill test that fired: `orx idea kill <ideaId> --evidence "..."`.
   Abandoned without a test: `orx idea drop <ideaId> --reason "..."`.

Close every idea you start. Refuted ideas are results too: they count the
trials behind any "best" number, which is how its luck gets discounted.

## Ablation checks

When a kept idea's experiment becomes the project's best, orx queues up to
two ablation ideas for the biggest earlier steps on its lineage path (the
steps `orx metric best` lists), titled `Ablate "<step>" from the best`. Each
forecasts that taking that step's change back out of the best loses what the
step gained.

- Build it as a child of the best experiment that reverts only that step's
  change, start it, run it with the best's seeds, and close it with
  `orx idea done` as usual.
- It is confirmed when taking the step out costs more than the noise floor,
  and refuted when it doesn't: then the steps interact, and the per-step
  gains in `orx metric best` don't add up the way they look.
- If a replicate shows the new best doesn't hold, drop its ablations.

## Learn from the record

`orx idea calibration <projectId>` compares forecasts with outcomes. The mean
error says whether your forecasts run high or low; the share of outcomes
inside the 80% intervals says whether the intervals are too narrow (well
under 80%) or too wide. Adjust new forecasts to match. Its table by class
shows which kinds of idea (data, incremental, orthogonal, process) have paid
off, as a hit rate and as gain per GPU-hour; lean the next proposals toward
the classes that pay.

`orx idea list <projectId> --all` shows everything tried, and
`orx metric best <projectId>` shows which ideas built the current best.
