// VLResearch: the Research view's numbers, chart geometry and groupings.
import assert from "node:assert/strict";
import test from "node:test";
import {
  CONFIRMED_MESSAGE,
  ablationLine,
  adjustedText,
  askAboutFinding,
  askAboutIdea,
  askAboutNext,
  askAboutStep,
  briefPayload,
  budgetLines,
  draftFromBrief,
  emptyDraft,
  ideaPayloads,
  kickoffMessage,
  defaultOptimization,
  optimizationLabel,
  parseOptimizations,
  setupProblems,
  calibrationLine,
  forecastText,
  formatSigned,
  formatValue,
  groupIdeas,
  insideInterval,
  niceTicks,
  parseBrief,
  parseFindings,
  parseIdeas,
  parseMetrics,
  runningBestChart,
  toneOf,
  waterfall,
  waterfallDomain,
  withoutIdeas,
  worseningIdeas,
} from "../src/flavor/research.ts";

// Shaped like GET /api/vlresearch/projects/{id}/metrics.
const metricsPayload = {
  primary: { key: "val_ic", direction: "max", noiseFloor: 0.001, split: null },
  keys: ["val_ic"],
  best: { runId: "r_best", experimentId: "best", experiment: "Best", status: "done", createdAt: 5, endedAt: 5, score: 0.0335, probe: false },
  baseline: { experimentId: "base", experiment: "Base", runId: "r_base", score: 0.02, probeRuns: 0 },
  history: [
    { runId: "r_base", endedAt: 1, score: 0.02, bestSoFar: 0.02, newBest: true },
    { runId: "r_wide", endedAt: 2, score: 0.026, bestSoFar: 0.026, newBest: true },
    { runId: "r_tiny", endedAt: 3, score: 0.0265, bestSoFar: 0.026, newBest: false },
    { runId: "r_lags", endedAt: 4, score: 0.0285, bestSoFar: 0.0285, newBest: true },
    { runId: "r_best", endedAt: 5, score: 0.0335, bestSoFar: 0.0335, newBest: true },
  ],
  path: [
    { experimentId: "base", experiment: "Base", idea: null, runId: "r_base", score: 0.02, probeRuns: 0, delta: null },
    { experimentId: "wide", experiment: "Wide", idea: "Wider window", runId: "r_wide", score: 0.026, probeRuns: 0, delta: 0.005999999999999998,
      ablation: { status: "confirmed", forecast: -0.006, outcome: -0.0055 } },
    { experimentId: "tiny", experiment: "Tiny", idea: "Tiny tweak", runId: "r_tiny", score: 0.0265, probeRuns: 0, delta: 0.0005000000000000004 },
    { experimentId: "lags", experiment: "Lags", idea: "Lagged returns", runId: "r_lags", score: 0.0285, probeRuns: 0, delta: 0.0020000000000000018,
      ablation: { status: "open", forecast: -0.002, outcome: null } },
    { experimentId: "best", experiment: "Best", idea: "Sector neutral", runId: "r_best", score: 0.0335, probeRuns: 0, delta: 0.005000000000000001 },
  ],
  trials: { runs: 5, ideasTested: 4, ideasKept: 3 },
  runs: [],
};

test("numbers read the way orx metric prints them", () => {
  assert.equal(formatValue(0.0335), "0.0335");
  assert.equal(formatValue(0.005999999999999998), "0.006");
  assert.equal(formatValue(1234.5678), "1234.57");
  assert.equal(formatValue(0), "0");
  assert.equal(formatValue(0.00001234), "1.2340e-5");
  assert.equal(formatValue(null), "–");
  assert.equal(formatSigned(0.003), "+0.003");
  assert.equal(formatSigned(-0.0012), "−0.0012");
  assert.equal(formatSigned(0), "0");
});

test("a change is a gain, a loss or noise in the metric's own direction", () => {
  assert.equal(toneOf(0.004, "max", 0.001), "gain");
  assert.equal(toneOf(0.004, "min", 0.001), "loss");
  assert.equal(toneOf(-0.004, "min", 0.001), "gain");
  assert.equal(toneOf(0.0005, "max", 0.001), "noise");
  assert.equal(toneOf(0.0005, "max", null), "gain");
});

test("the metrics payload is read defensively", () => {
  const metrics = parseMetrics(metricsPayload);
  assert.equal(metrics.primary?.key, "val_ic");
  assert.equal(metrics.best?.score, 0.0335);
  assert.equal(metrics.baseline?.score, 0.02);
  assert.equal(metrics.history.length, 5);
  assert.equal(metrics.path[1].ablation?.status, "confirmed");
  assert.equal(metrics.path[2].ablation, undefined);
  assert.deepEqual(metrics.trials, { runs: 5, ideasTested: 4, ideasKept: 3 });

  const empty = parseMetrics({ primary: null, keys: ["loss", 3], history: [{ runId: "x" }] });
  assert.equal(empty.primary, null);
  assert.deepEqual(empty.keys, ["loss"]);
  assert.deepEqual(empty.history, []);
  assert.equal(parseMetrics("nonsense").path.length, 0);
});

test("the running best is a step line through every finished run", () => {
  const metrics = parseMetrics(metricsPayload);
  const box = { width: 400, height: 160, left: 40, right: 10, top: 10, bottom: 20 };
  const chart = runningBestChart(metrics.history, metrics.baseline?.score ?? null, box);
  assert.equal(chart.points.length, 5);
  assert.equal(chart.points[0].x, 40);
  assert.equal(chart.points[4].x, 390);
  // Higher scores sit higher on the chart.
  assert.ok(chart.points[4].y < chart.points[0].y);
  for (const point of chart.points) assert.ok(point.y >= 10 && point.y <= 140, `y ${point.y}`);
  assert.match(chart.bestPath, /^M40\.0,[\d.]+H127\.5V[\d.]+H215\.0V[\d.]+H302\.5V[\d.]+H390\.0V[\d.]+$/);
  assert.equal(chart.baselineY, chart.points[0].y);
  assert.ok(chart.ticks.length >= 2);

  const single = runningBestChart([{ runId: "r", endedAt: 1, score: 1, bestSoFar: 1, newBest: true }], null, box);
  assert.equal(single.points[0].x, 215);
  assert.match(single.bestPath, /H390\.0$/);
  assert.deepEqual(runningBestChart([], 1, box).points, []);
});

test("ticks are round numbers inside the range", () => {
  assert.deepEqual(niceTicks(0.018, 0.035), [0.02, 0.025, 0.03, 0.035]);
  assert.deepEqual(niceTicks(0, 10), [0, 5, 10]);
  assert.deepEqual(niceTicks(2, 2), [2]);
});

test("the waterfall splits the gain over the steps behind the best", () => {
  const metrics = parseMetrics(metricsPayload);
  const rows = waterfall(metrics.path, "max", 0.001);
  assert.deepEqual(rows.map((row) => row.kind), ["baseline", "step", "step", "step", "step", "best"]);
  assert.deepEqual(rows.map((row) => row.label), ["Baseline", "Wider window", "Tiny tweak", "Lagged returns", "Sector neutral", "Best"]);
  assert.equal(rows[1].detail, "Wide");
  assert.deepEqual(rows.map((row) => row.tone), [null, "gain", "noise", "gain", "gain", "gain"]);
  assert.ok(Math.abs(rows[1].share - 0.006 / 0.0135) < 1e-9);
  const shares = rows.filter((row) => row.kind === "step").reduce((sum, row) => sum + row.share, 0);
  assert.ok(Math.abs(shares - 1) < 1e-9);
  assert.equal(rows[1].ablation?.status, "confirmed");
  assert.ok(Math.abs(rows[5].delta - 0.0135) < 1e-12);
  assert.equal(rows[0].from, 0.02);
  const [low, high] = waterfallDomain(rows);
  assert.ok(low < 0.02 && high > 0.0335);

  assert.equal(ablationLine(rows[1].ablation), "Ablation: taking it out changed −0.0055, about what it gained");
  assert.equal(ablationLine(rows[3].ablation), "Ablation check queued");
  assert.equal(
    ablationLine({ status: "refuted", forecast: -0.002, outcome: -0.0002 }),
    "Ablation: taking it out changed −0.0002, so the steps interact",
  );
});

test("a loss metric's waterfall counts falls as gains, and unscored steps fold forward", () => {
  const rows = waterfall(
    [
      { experimentId: "a", experiment: "A", idea: null, runId: "r_a", score: 0.5, probeRuns: 3, delta: null },
      { experimentId: "b", experiment: "B", idea: "Bigger batch", runId: null, score: null, probeRuns: 0, delta: null },
      { experimentId: "c", experiment: "C", idea: "Cosine schedule", runId: "r_c", score: 0.42, probeRuns: 0, delta: -0.08 },
      { experimentId: "d", experiment: "D", idea: null, runId: "r_d", score: 0.44, probeRuns: 0, delta: 0.02 },
    ],
    "min",
    0.005,
  );
  assert.deepEqual(rows.map((row) => row.kind), ["baseline", "unscored", "step", "step", "best"]);
  assert.equal(rows[2].tone, "gain");
  assert.equal(rows[3].tone, "loss");
  assert.equal(rows[3].label, "D");
  assert.equal(rows[3].detail, null);
  assert.ok(Math.abs(rows[2].share - 0.08 / 0.06) < 1e-9);
  assert.ok(Math.abs(rows[3].share + 0.02 / 0.06) < 1e-9);
  assert.equal(rows[4].tone, "gain");
  assert.deepEqual(waterfall([], "max", null), []);
});

const ideasPayload = {
  next: { idea: { id: "idea_3", title: "Ablate \"Wider window\" from the best", kind: "science", class: "ablation", status: "open", statusLabel: "open", priority: 2, createdAt: 3 }, reason: "it checks a step behind the current best" },
  calibration: { scored: 4, bias: 0.001375, meanAbsError: 0.002125, withInterval: 4, insideInterval: 2, tested: 4, kept: 3 },
  byClass: [{ class: "incremental", tested: 2, kept: 1, meanGain: 0.00325, gainPerHour: 0.001625 }, { class: null, tested: 1, kept: 0, meanGain: null, gainPerHour: null }],
  ideas: [
    { id: "idea_1", title: "Wider window", kind: "science", class: "incremental", priority: 2, forecast: 0.002, forecastLow: 0, forecastHigh: 0.004, costHours: 3, status: "kept", statusLabel: "kept", outcome: 0.006, createdAt: 1, closedAt: 10 },
    { id: "idea_2", title: "Tiny tweak", kind: "science", class: "incremental", priority: 2, forecast: 0.002, forecastLow: 0, forecastHigh: 0.004, status: "refuted", outcome: 0.0005, createdAt: 2, closedAt: 11 },
    { id: "idea_3", title: "Ablate \"Wider window\" from the best", kind: "science", class: "ablation", priority: 2, forecast: -0.006, status: "open", statusLabel: "open", createdAt: 3 },
    { id: "idea_4", title: "Fix the label join", kind: "eval-blocker", priority: 1, status: "open", createdAt: 4 },
    { id: "idea_5", title: "Turnover penalty", kind: "science", class: "orthogonal", priority: 1, forecast: 0.003, status: "open", createdAt: 5,
      adjusted: { forecast: 0.0044, low: -0.0008, high: 0.0083 } },
    { id: "idea_6", title: "Longer history", kind: "science", class: "data", priority: 2, status: "running", createdAt: 6 },
    { id: "idea_7", title: "Dropped one", kind: "science", priority: 2, status: "dropped", createdAt: 7 },
    { title: "no id" },
  ],
};

test("ideas group into blockers, running, queued and tried, with titles only", () => {
  const ideas = parseIdeas(ideasPayload);
  assert.equal(ideas.ideas.length, 7);
  assert.equal(ideas.ideas[1].statusLabel, "refuted");
  assert.equal(ideas.next?.idea.title, "Ablate \"Wider window\" from the best");
  const groups = groupIdeas(ideas.ideas);
  assert.deepEqual(groups.blockers.map((idea) => idea.title), ["Fix the label join"]);
  assert.deepEqual(groups.running.map((idea) => idea.title), ["Longer history"]);
  assert.deepEqual(groups.open.map((idea) => idea.title), ["Turnover penalty", "Ablate \"Wider window\" from the best"]);
  assert.deepEqual(groups.tried.map((idea) => idea.title), ["Tiny tweak", "Wider window"]);
  assert.equal(forecastText(ideas.ideas[0]), "+0.002 (0 to +0.004)");
  assert.equal(forecastText(ideas.ideas[3]), "no forecast");
  assert.equal(insideInterval(ideas.ideas[0]), false);
  assert.equal(insideInterval(ideas.ideas[1]), true);
  assert.equal(insideInterval(ideas.ideas[4]), null);
  assert.equal(
    calibrationLine(ideas.calibration),
    "3 of 4 tested ideas kept (75%) · mean forecast error +0.001375 (outcomes ran above forecasts) · 50% inside their 80% intervals",
  );
  assert.equal(calibrationLine(parseIdeas({}).calibration), null);
  assert.equal(adjustedText(ideas.ideas[4]), "+0.0044 (\u22120.0008 to +0.0083)");
  assert.equal(adjustedText(ideas.ideas[0]), null);
  assert.equal(ideas.byClass[1].class, null);
});

test("budgets read against the brief's limits", () => {
  const { brief, usage, missing } = parseBrief({
    brief: { objective: "Raise IC", locked: [{ path: "eval", object: null }, { nope: 1 }], budgetRuns: 20, budgetGpuHours: 40, budgetDays: 14, confirmedAt: 5, confirmedBy: "user" },
    usage: { runs: 17, runHours: 41.5, days: 2.25 },
    missing: [
      "stop rule",
      "what must not change (--must-not-change, and --lock for evaluation files)",
      "a primary metric (orx metric primary <projectId> <key> --maximize|--minimize)",
      "at least one idea to start from (orx idea add)",
    ],
  });
  assert.deepEqual(brief?.locked, [{ path: "eval" }]);
  // The agent's command-line hints stay out of the dashboard.
  assert.deepEqual(missing, ["stop rule", "what must not change", "a primary metric", "at least one idea to start from"]);
  const lines = budgetLines(brief, usage);
  assert.deepEqual(lines.map((line) => [line.label, line.text, line.tone]), [
    ["Runs", "17 of 20", "warn"],
    ["Run hours", "42 of 40", "alert"],
    ["Days", "2.3 of 14", "normal"],
  ]);
  assert.equal(lines[1].fraction, 1);
  assert.deepEqual(budgetLines(brief, { runs: 0, runHours: 0, days: 0 }).map((line) => line.text), ["0 of 20", "0 of 40", "0 of 14"]);
  assert.deepEqual(budgetLines({ ...brief, budgetRuns: null, budgetGpuHours: 0, budgetDays: null }, null), []);
  assert.equal(parseBrief({ brief: null }).brief, null);
});

test("findings carry their latest verdict", () => {
  const rows = parseFindings({
    findings: [
      { finding: { id: "fnd_1", statement: "Volume helps in volatile months", kind: "model", status: "supported", locked: true }, supporting: 2, against: 1, latest: { verdict: "supported", confidence: 0.7, scope: "2016-2023" } },
      { finding: { id: "fnd_2", statement: "Open question", kind: "dataset", status: "open", locked: false }, supporting: 0, against: 0, latest: null },
      { finding: { id: "bad" } },
    ],
  });
  assert.equal(rows.length, 2);
  assert.equal(rows[0].latest?.confidence, 0.7);
  assert.equal(rows[0].finding.locked, true);
  assert.equal(rows[1].latest, null);
});

test("the setup form turns into the brief, metric and ideas the endpoints take", () => {
  const draft = {
    ...emptyDraft(),
    objective: "  Raise validation IC  ",
    metricKey: "val_ic",
    direction: "max",
    noiseFloor: "0.002",
    mustNotChange: "Evaluation code",
    locked: "eval/score.py\n\n data/splits.yaml ,eval/folds",
    scope: "features/\n",
    budgetRuns: "20",
    budgetGpuHours: "",
    budgetDays: "14",
    stopRule: "No gain in 5 runs",
    pointers: "Volume features first",
    ideas: [{ title: "Volume features", forecast: "0.003" }, { title: "  ", forecast: "" }, { title: "Lagged returns", forecast: "" }],
  };
  assert.deepEqual(setupProblems(draft), {});
  assert.deepEqual(briefPayload(draft), {
    objective: "Raise validation IC",
    mayChange: null,
    mustNotChange: "Evaluation code",
    locked: ["eval/score.py", "data/splits.yaml", "eval/folds"],
    scope: ["features/"],
    budgetRuns: 20,
    budgetGpuHours: null,
    budgetDays: 14,
    pointers: "Volume features first",
    sources: null,
    stopRule: "No gain in 5 runs",
    metric: { key: "val_ic", direction: "max", noiseFloor: 0.002, split: null },
  });
  assert.deepEqual(ideaPayloads(draft), [
    { title: "Volume features", forecast: 0.003 },
    { title: "Lagged returns", forecast: null },
  ]);

  const problems = setupProblems({ ...emptyDraft(), budgetRuns: "2.5", noiseFloor: "-1", ideas: [{ title: "x", forecast: "lots" }] });
  assert.deepEqual(Object.keys(problems).sort(), ["budget", "ideas", "metricKey", "noiseFloor", "objective"]);
  // A typographic minus is a minus.
  assert.deepEqual(setupProblems({ ...draft, ideas: [{ title: "x", forecast: "\u22120.001" }] }), {});
  assert.equal(briefPayload({ ...draft, metricKey: " " }).metric, null);
});

test("a seed forecast that moves the metric the wrong way is flagged", () => {
  const ideas = [
    { title: "Up", forecast: "0.003" },
    { title: "Down", forecast: "−0.002" },
    { title: "Unknown", forecast: "" },
    { title: "", forecast: "-1" },
  ];
  assert.deepEqual(worseningIdeas({ ...emptyDraft(), direction: "max", ideas }), ["Down"]);
  assert.deepEqual(worseningIdeas({ ...emptyDraft(), direction: "min", ideas }), ["Up"]);
});

test("ideas queued before a failed step leave the form, so a retry doesn't queue them twice", () => {
  const draft = { ...emptyDraft(), ideas: [{ title: "A", forecast: "" }, { title: "", forecast: "" }, { title: "B", forecast: "0.1" }] };
  assert.deepEqual(withoutIdeas(draft, new Set([0])).ideas, [{ title: "", forecast: "" }, { title: "B", forecast: "0.1" }]);
  // The form always keeps a row to type into.
  assert.deepEqual(withoutIdeas(draft, new Set([0, 1, 2])).ideas, [{ title: "", forecast: "" }]);
  assert.equal(withoutIdeas(draft, new Set([2])).objective, draft.objective);
});

test("editing starts from the saved brief", () => {
  const draft = draftFromBrief({
    brief: {
      id: "opt_1", objective: "Raise IC", mayChange: null, mustNotChange: "eval", locked: [{ path: "eval" }, { path: "splits.yaml" }],
      scope: ["features", "data/loaders.py"], budgetRuns: 20, budgetGpuHours: null, budgetDays: 14, pointers: "Volume",
      sources: null, stopRule: "5 flat runs", confirmedAt: null, confirmedBy: null, chatSessionId: null, updatedAt: 1,
    },
    primary: { key: "val_loss", direction: "min", noiseFloor: 0.01, split: "val" },
    usage: null,
    missing: [],
  });
  assert.equal(draft.metricKey, "val_loss");
  assert.equal(draft.direction, "min");
  assert.equal(draft.noiseFloor, "0.01");
  assert.equal(draft.split, "val");
  assert.equal(draft.locked, "eval\nsplits.yaml");
  assert.equal(draft.scope, "features\ndata/loaders.py");
  assert.equal(draft.budgetRuns, "20");
  assert.equal(draft.budgetGpuHours, "");
  assert.deepEqual(draft.ideas, [{ title: "", forecast: "" }]);
  assert.equal(draftFromBrief({ brief: null, primary: null, usage: null, missing: [] }).objective, "");
});

test("the started chat is told where the brief is and what comes next", () => {
  const kickoff = kickoffMessage("proj_1", "opt_7", 2);
  assert.match(kickoff, /runs optimization opt_7/);
  assert.match(kickoff, /`orx brief show proj_1`/);
  assert.match(kickoff, /2 seed ideas \(`orx idea list proj_1`\)/);
  assert.match(kickoff, /orx-grill/);
  assert.match(kickoff, /orx-improve/);
  assert.match(kickoffMessage("proj_1", "opt_7", 0), /no seed ideas yet/);
  assert.match(kickoffMessage("proj_1", "opt_7", 1), /1 seed idea \(/);
  assert.match(CONFIRMED_MESSAGE, /orx-improve/);
});

test("the brief payload carries pointers, the chat link and the metric", () => {
  const { brief, primary } = parseBrief({
    brief: { objective: "x", locked: [], pointers: "Volume", sources: "/data/opt", chatSessionId: "chat_1" },
    primary: { key: "val_ic", direction: "max", noiseFloor: null, split: null },
  });
  assert.equal(brief?.pointers, "Volume");
  assert.equal(brief?.sources, "/data/opt");
  assert.equal(brief?.chatSessionId, "chat_1");
  assert.equal(primary?.key, "val_ic");
});

test("side chats open with a question that names the item and its id", () => {
  const idea = {
    id: "idea_7", title: "Volume features", kind: "science", class: "incremental", priority: 2, hypothesis: null, killTest: null,
    forecast: 0.003, forecastLow: -0.001, forecastHigh: 0.007, costHours: 2, status: "open", statusLabel: "open",
    experimentId: null, outcome: null, createdAt: 1, startedAt: null, closedAt: null, adjusted: null,
  };
  assert.match(askAboutNext(idea), /“Volume features” \(`idea_7`\) the next one/);
  assert.match(askAboutIdea(idea), /forecast of \+0\.003 \(\u22120\.001 to \+0\.007\) based on/);
  const tried = { ...idea, status: "refuted", statusLabel: "refuted", experimentId: "exp_2", outcome: -0.0004, closedAt: 5 };
  assert.match(askAboutIdea(tried), /why is it refuted\? It measured \u22120\.0004 against a forecast of \+0\.003/);
  const dropped = { ...idea, status: "dropped", statusLabel: "dropped", closedAt: 5 };
  assert.match(askAboutIdea(dropped), /why is it dropped\?$/);
  const step = { label: "Volume features", detail: "vol-20d", kind: "step", from: 0.02, to: 0.023, delta: 0.003, tone: "gain", share: 0.6 };
  assert.equal(askAboutStep(step, "val_ic"), "Why did “Volume features” (experiment vol-20d) move val_ic by +0.003? Would the gain hold without the steps around it?");
  assert.match(askAboutStep({ ...step, detail: null }, "val_ic"), /^Why did “Volume features” move/);
  assert.match(askAboutFinding({ id: "fnd_1", statement: "Volume helps in volatile months", kind: "model", status: "supported", locked: false }), /“Volume helps in volatile months” \(`fnd_1`\)/);
});

test("a project's optimizations: the open chat's comes first, each named by its goal", () => {
  const entry = (id, objective, chat, key) => ({
    brief: { id, objective, chatSessionId: chat, scope: [], locked: [] },
    primary: key ? { key, direction: "max", noiseFloor: null, split: null } : null,
    usage: null,
    missing: [],
  });
  const { optimizations, keys } = parseOptimizations({
    optimizations: [entry("opt_b", null, "chat_b", "backtest_seconds"), entry("opt_a", "Raise the validation IC", "chat_a", "val_ic"), { brief: null }],
    keys: ["val_ic", 3],
  });
  assert.deepEqual(optimizations.map((payload) => payload.brief.id), ["opt_b", "opt_a"]);
  assert.deepEqual(keys, ["val_ic"]);
  assert.equal(defaultOptimization(optimizations, "chat_a"), "opt_a");
  assert.equal(defaultOptimization(optimizations, "chat_other"), "opt_b");
  assert.equal(defaultOptimization([], "chat_a"), null);
  assert.equal(optimizationLabel(optimizations[1]), "Raise the validation IC");
  assert.equal(optimizationLabel(optimizations[0]), "Improve backtest_seconds");
  assert.equal(optimizationLabel(parseOptimizations({ optimizations: [entry("opt_c", null, null, null)] }).optimizations[0]), "New optimization");
  assert.match(optimizationLabel(entry("opt_d", "x".repeat(80), null, null)), /…$/);
});
