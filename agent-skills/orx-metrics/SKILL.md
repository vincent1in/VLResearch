---
name: orx-metrics
description: "Report run metrics as ORX_METRIC lines and compare runs on this chat's optimization's primary metric with `orx metric`. Use when writing or editing code that reports losses or scores, before launching a run whose result will be compared, when choosing what to optimize, or before saying which experiment is best."
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
- An experiment's score is the mean of its finished runs. Have each run draw
  its own seed and print it, so that running an experiment again is a
  replicate.
- NaN and infinities are recorded as missing values, and orx tells the chat
  waiting on the run that a metric diverged.
- In Python, save `orx skill metrics/assets/orx_metrics.py` next to the
  training code and call `log_metric("val_ic", ic, step=step)` or
  `log_metrics({...}, step=step)`. Any language works: the line is the
  contract.

Add the reporting before the first launch of an experiment; a run that
printed no metrics can't be ranked afterwards.

## Set the primary metric

Once per optimization, before comparing runs, name the key that defines
"best". Each chat's optimization has its own; set in this chat, it is this
chat's:

```sh
orx metric primary <projectId> val_ic --maximize
orx metric primary <projectId> val_loss --minimize --noise-floor 0.001
```

- Pick a validation metric. Never select on a test split; report test numbers
  only for the final candidate. When runs report the key under several
  splits, orx asks you to choose one: `--split val`.
- The noise floor is the smallest gain worth keeping, 0 unless set; a gain
  no bigger does not make a new best. Set one only when smaller gains
  wouldn't matter in practice. Seed noise is handled by replicates: each new
  best is run again with a new seed, and the mean decides. Setting the same
  key again keeps its noise floor.
- `orx metric primary <projectId>` with no key shows the current setting and
  the keys runs have reported so far.

## Read results

- `orx metric show <runId>`: each key a run reported, with its last, min and
  max values.
- `orx metric runs <projectId>`: every run ranked on the primary metric, with
  the best, the baseline and probe runs marked. To rank by another key, give
  its direction: `--key val_loss --minimize`.
- `orx metric best <projectId>`: the best experiment, scored by the mean of
  its runs, against the baseline, and each experiment on the lineage between
  them (with its id) and its change. The changes add up to the total gain.
  Probe runs never count as results; a baseline measured only with probe
  runs is scored by their mean, and an attempt has to beat it.

Each takes `--json`. Before claiming that one experiment beats another, check
both scores with `orx metric runs`, say how many runs each averages, and
compare the gap with how far one run of the baseline moves between seeds.
