---
name: orx-grill
description: "Scope research work with the user before spending compute: interview in numbered rounds with a recommended answer to each question, check the data and baseline, review for leakage, propose ideas with kill tests, and record the brief with `orx brief`. Use at the start of a research goal, when a launch says the brief is missing or unconfirmed, or when the goal changes."
---

## When

- Before the first improvement experiment in a project, and whenever the
  goal changes.
- When the user starts auto-research from the dashboard's Research view.
  They have already filled in part of the brief, and maybe some ideas.
- When `orx exp run` refuses a launch because the brief is missing or not
  confirmed.
- Work in plan mode when it is on: the interview and the review only read,
  and the plan you submit at the end is the brief.

## 1. Interrogate

Start from what is already recorded: `orx brief show <projectId>` and
`orx idea list <projectId>`. Don't ask again for what the user filled in;
ask about what is missing, vague or at odds with the repository.

Ask in numbered rounds, one round at a time, three to six questions each.
Give your recommended answer to every question, worked out from the
repository and the data, so the user can accept it with a word or correct
it. Keep going until every field is pinned; don't fill a gap with a guess.

| Field | Pin down | Example |
| --- | --- | --- |
| Objective and metric | The goal, the one number that measures it, its split and direction | Validation information coefficient of the forecast; maximize |
| Noise floor | How far the metric moves between seeds of the baseline | Spread across three seeds of the baseline |
| What may change | Code and data runs may touch | Feature code, model code, loaders; new datasets under a named folder |
| What must not change | Whatever defines the score | Evaluation code, split dates, the out-of-sample window |
| Budget | Runs, GPU-hours, wall-clock days | 20 runs, 40 GPU-hours, two weeks |
| Starting pointers | Where the user wants to begin | "Look at the volume features first" |
| Sources | Datasets on the compute host, earlier experiments, papers | The options dataset under `/data/opt` |
| Stop rule | When to stop and report | No validation gain in 5 runs, or the budget is spent |

The user's pointers become the first ideas.

## 2. Preflight on the compute host

Use probe runs, which never count as results:
`VLR_PROBE=1 orx exp run <expId>`.

- The data paths exist, with the expected dates and coverage.
- The baseline reproduces. Run it with three seeds; the spread of the primary
  metric is the noise floor.
- The metric comes from the evaluation code you are about to lock, and the
  code prints `ORX_METRIC` lines (`orx-metrics`).
- A smoke run completes end to end.

## 3. Leakage review

One pass over the workspace with a blockers-only checklist; ignore style.

- Point-in-time joins: each feature uses only data known at its timestamp.
- No future information in features, labels, normalization or resampling.
- Training, validation and out-of-sample periods don't overlap, including
  through rolling windows; purge or embargo gaps where labels span time.
- The metric's name and direction match its computation.
- Survivorship: the universe keeps delisted and dropped entities.
- The baseline is scored on the same split and evaluation code as the
  candidates.

File each problem as a blocker:
`orx idea add <projectId> "<problem>" --kind eval-blocker --priority 1`.
Launches wait until blockers are closed.

## 4. Propose

Three to eight ideas, each with a hypothesis, a forecast change with an 80%
interval, a GPU-hour cost and a kill test under about one GPU-hour
(`orx-ideas`). Mostly incremental steps from the strongest pointer, plus at
least one orthogonal idea.

## 5. Confirm

Present the brief and the ideas together, and ask the user to approve or
edit them. In plan mode, that is the plan you submit. Only after they agree:

```sh
orx metric primary <projectId> <key> --maximize --noise-floor <spread>
orx brief set <projectId> --objective "..." --may-change "..." \
  --must-not-change "..." --lock <evaluation file> --lock <split file> \
  --budget-runs 20 --budget-gpu-hours 40 --budget-days 14 \
  --pointers "..." --sources "..." --stop-rule "..."
orx idea add <projectId> "<title>" ...    # one per idea
orx brief confirm <projectId>
```

`orx brief set` changes only the fields you pass. The user can confirm the
brief in the dashboard's Research view, and this chat is told when they do;
ask them to if they started there. If they agree in the chat instead,
confirm it yourself.

Never confirm a brief the user has not seen. Confirming needs at least one
open idea to start from, and the loop (`orx idea next`, `orx idea start`)
waits for the confirmation. Any later `orx brief set` reopens it until they
agree again; the budget keeps counting from the first confirmation unless the
user asks for a fresh one (`--reset-budget`). Literature is optional: with no
papers, the brief is built from the code, the data and the user's pointers.
