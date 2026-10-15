---
name: orx-metrics
description: "Report run metrics as ORX_METRIC lines and compare runs on the project's primary metric with `orx metric`. Use when writing or editing code that reports losses or scores, before launching a run whose result will be compared, when choosing what to optimize, or before saying which experiment is best."
---

## Report metrics from run code

orx records every line of run output that carries `ORX_METRIC` followed by a
JSON object. Print one line per measurement, flushed, from the process that
owns the result (rank 0 in distributed training):

```text
ORX_METRIC {"key": "val_ic", "value": 0.031, "step": 1200}
ORX_METRIC {"step": 1200, "val_ic": 0.031, "train_loss": 0.42}
```

- `step` and `split` (`"train"`, `"val"`, `"test"`) are optional.
- A run's score on a key is the last value it reported. If the code restores
  its best checkpoint, report that checkpoint's validation score last.
- NaN and infinities are recorded as missing values, and orx tells the chat
  waiting on the run that a metric diverged.
- In Python, save `orx skill metrics/assets/orx_metrics.py` next to the
  training code and call `log_metric("val_ic", ic, step=step)` or
  `log_metrics({...}, step=step)`. Any language works: the line is the
  contract.

Add the reporting before the first launch of an experiment; a run that
printed no metrics can't be ranked afterwards.

## Set the primary metric

Once per project, before comparing runs, name the key that defines "best":

```sh
orx metric primary <projectId> val_ic --maximize --noise-floor 0.002
orx metric primary <projectId> val_loss --minimize
```

- Pick a validation metric. Never select on a test split; report test numbers
  only for the final candidate.
- The noise floor is the smallest gain that counts as real. Use the spread
  between reruns of the baseline with different seeds when it is known; a
  gain below it does not make a new best.
- `orx metric primary <projectId>` with no key shows the current setting and
  the keys runs have reported so far.

## Read results

- `orx metric show <runId>`: each key a run reported, with its last, min and
  max values.
- `orx metric runs <projectId>`: every run ranked on the primary metric
  (`--key <key>` ranks by another), with the best and the baseline marked.
- `orx metric best <projectId>`: the best finished run against the baseline,
  and each experiment on the lineage between them with its change. The
  changes add up to the total gain.

Each takes `--json`. Before claiming that one experiment beats another, check
both scores with `orx metric runs` and say whether the gap exceeds the noise
floor.
