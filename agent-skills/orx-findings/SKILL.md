---
name: orx-findings
description: "Record what the project has learned as claims with evidence using `orx finding`: back or undercut a claim with runs, link related claims, and add verdicts with confidence and scope. Use after an idea closes, when a result changes what you believe about the data, model or pipeline, and before summarizing what is known."
---

## A finding is a claim with evidence

One sentence that could turn out wrong, about the data, a model or the
pipeline:

```sh
orx finding add <projectId> "Volume features add IC in high-volatility months" \
  --kind model --idea <ideaId>
```

- One claim per finding, specific enough to test again. Put its scope in the
  sentence when it matters: universe, period, regime.
- `--kind` is `dataset`, `model` or `process`. `--idea` links the idea whose
  test produced it.

## Attach evidence

```sh
orx finding evidence <findingId> --supports --run <runId> --note "2022 fold: +0.004 IC"
orx finding evidence <findingId> --refutes --run <runId>
```

A run brings a snapshot of its metrics. Record evidence against a claim as
carefully as evidence for it.

## Link and assess

```sh
orx finding relate <findingId> contradicts <otherId>   # or depends-on, refines, motivates
orx finding assess <findingId> --verdict supported --confidence 0.7 \
  --scope "2016-2023, top 1000 names" --gaps "one market" --next "rerun on Europe"
```

Assessments are append-only: when new evidence arrives, assess again rather
than rewording the claim. A locked finding was settled by the user; you can
still add evidence to it, but ask them before anything else changes.

## Use the ledger

- Before proposing ideas, read `orx finding list <projectId>`, and don't
  retest a refuted claim without saying what is different this time.
- When summarizing, keep supported, refuted and inconclusive claims apart,
  and say how many trials stand behind the best result (`orx metric best`
  reports them).
