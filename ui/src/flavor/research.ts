// VLResearch: what the Research view shows, worked out from the
// /api/vlresearch/projects/{id}/* payloads. Kept free of React and the message
// catalog so node's test runner can import it directly
// (ui/tests/vlresearchResearch.test.mjs). The view shows idea titles, never
// idea ids.

export type Direction = "max" | "min";

export interface PrimaryMetric {
  key: string;
  direction: Direction;
  noiseFloor: number | null;
  split: string | null;
}

export interface ScoredRun {
  runId: string;
  experimentId: string;
  experiment: string;
  status: string;
  createdAt: number;
  endedAt: number | null;
  score: number | null;
  probe: boolean;
}

export interface Reference {
  experimentId: string;
  experiment: string;
  runId: string | null;
  score: number;
  /** How many probe runs the score averages, when it is such a mean. */
  probeRuns: number;
}

export interface HistoryPoint {
  runId: string;
  endedAt: number;
  score: number;
  bestSoFar: number;
  newBest: boolean;
}

export interface Ablation {
  status: string;
  forecast: number | null;
  outcome: number | null;
}

export interface PathStep {
  experimentId: string;
  experiment: string;
  idea: string | null;
  runId: string | null;
  score: number | null;
  probeRuns: number;
  delta: number | null;
  ablation?: Ablation;
}

export interface Trials {
  runs: number;
  ideasTested: number;
  ideasKept: number;
}

export interface MetricsPayload {
  primary: PrimaryMetric | null;
  keys: string[];
  best: ScoredRun | null;
  baseline: Reference | null;
  history: HistoryPoint[];
  path: PathStep[];
  trials: Trials | null;
  runs: ScoredRun[];
}

export interface Idea {
  id: string;
  title: string;
  kind: string;
  class: string | null;
  priority: number;
  hypothesis: string | null;
  killTest: string | null;
  forecast: number | null;
  forecastLow: number | null;
  forecastHigh: number | null;
  costHours: number | null;
  status: string;
  /** The status as people read it: "resolved" blockers, "confirmed" ablations. */
  statusLabel: string;
  experimentId: string | null;
  outcome: number | null;
  createdAt: number;
  startedAt: number | null;
  closedAt: number | null;
  /** For an open idea, once enough ideas are tested: its forecast as past misses suggest reading it. */
  adjusted: { forecast: number; low: number; high: number } | null;
}

export interface Pick {
  idea: Idea;
  reason: string;
}

export interface Calibration {
  scored: number;
  bias: number | null;
  meanAbsError: number | null;
  withInterval: number;
  insideInterval: number;
  tested: number;
  kept: number;
}

export interface ClassScore {
  class: string | null;
  tested: number;
  kept: number;
  meanGain: number | null;
  gainPerHour: number | null;
}

export interface IdeasPayload {
  next: Pick | null;
  calibration: Calibration;
  byClass: ClassScore[];
  ideas: Idea[];
}

export interface Brief {
  objective: string | null;
  mayChange: string | null;
  mustNotChange: string | null;
  locked: { path: string }[];
  budgetRuns: number | null;
  budgetGpuHours: number | null;
  budgetDays: number | null;
  pointers: string | null;
  sources: string | null;
  stopRule: string | null;
  confirmedAt: number | null;
  confirmedBy: string | null;
  /** The chat the Research view started for this brief. */
  chatSessionId: string | null;
}

export interface Usage {
  runs: number;
  runHours: number;
  days: number;
}

export interface BriefPayload {
  brief: Brief | null;
  primary: PrimaryMetric | null;
  usage: Usage | null;
  missing: string[];
}

export interface Finding {
  id: string;
  statement: string;
  kind: string;
  status: string;
  locked: boolean;
}

export interface Assessment {
  verdict: string;
  confidence: number | null;
  scope: string | null;
}

export interface FindingRow {
  finding: Finding;
  supporting: number;
  against: number;
  latest: Assessment | null;
}

// --- reading payloads -----------------------------------------------------------

type Json = Record<string, unknown>;

const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null && !Array.isArray(value);
const text = (value: unknown): string | null => (typeof value === "string" ? value : null);
const number = (value: unknown): number | null => (typeof value === "number" && Number.isFinite(value) ? value : null);
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

function readPrimary(value: unknown): PrimaryMetric | null {
  if (!isObject(value) || typeof value.key !== "string") return null;
  return {
    key: value.key,
    direction: value.direction === "min" ? "min" : "max",
    noiseFloor: number(value.noiseFloor),
    split: text(value.split),
  };
}

function readRun(value: unknown): ScoredRun | null {
  if (!isObject(value) || typeof value.runId !== "string") return null;
  return {
    runId: value.runId,
    experimentId: text(value.experimentId) ?? "",
    experiment: text(value.experiment) ?? "",
    status: text(value.status) ?? "",
    createdAt: number(value.createdAt) ?? 0,
    endedAt: number(value.endedAt),
    score: number(value.score),
    probe: value.probe === true,
  };
}

export function parseMetrics(value: unknown): MetricsPayload {
  const body = isObject(value) ? value : {};
  const baseline = isObject(body.baseline) && number(body.baseline.score) !== null
    ? {
        experimentId: text(body.baseline.experimentId) ?? "",
        experiment: text(body.baseline.experiment) ?? "",
        runId: text(body.baseline.runId),
        score: number(body.baseline.score) ?? 0,
        probeRuns: number(body.baseline.probeRuns) ?? 0,
      }
    : null;
  const history = list(body.history).flatMap((point): HistoryPoint[] => {
    if (!isObject(point)) return [];
    const score = number(point.score);
    const bestSoFar = number(point.bestSoFar);
    if (score === null || bestSoFar === null) return [];
    return [{
      runId: text(point.runId) ?? "",
      endedAt: number(point.endedAt) ?? 0,
      score,
      bestSoFar,
      newBest: point.newBest === true,
    }];
  });
  const path = list(body.path).flatMap((step): PathStep[] => {
    if (!isObject(step)) return [];
    const ablation = isObject(step.ablation)
      ? { status: text(step.ablation.status) ?? "open", forecast: number(step.ablation.forecast), outcome: number(step.ablation.outcome) }
      : undefined;
    return [{
      experimentId: text(step.experimentId) ?? "",
      experiment: text(step.experiment) ?? "",
      idea: text(step.idea),
      runId: text(step.runId),
      score: number(step.score),
      probeRuns: number(step.probeRuns) ?? 0,
      delta: number(step.delta),
      ...(ablation ? { ablation } : {}),
    }];
  });
  const trials = isObject(body.trials)
    ? { runs: number(body.trials.runs) ?? 0, ideasTested: number(body.trials.ideasTested) ?? 0, ideasKept: number(body.trials.ideasKept) ?? 0 }
    : null;
  return {
    primary: readPrimary(body.primary),
    keys: list(body.keys).flatMap((key) => (typeof key === "string" ? [key] : [])),
    best: readRun(body.best),
    baseline,
    history,
    path,
    trials,
    runs: list(body.runs).flatMap((run) => { const parsed = readRun(run); return parsed ? [parsed] : []; }),
  };
}

function readIdea(value: unknown): Idea | null {
  if (!isObject(value) || typeof value.id !== "string" || typeof value.title !== "string") return null;
  const status = text(value.status) ?? "open";
  return {
    id: value.id,
    title: value.title,
    kind: text(value.kind) ?? "science",
    class: text(value.class),
    priority: number(value.priority) ?? 2,
    hypothesis: text(value.hypothesis),
    killTest: text(value.killTest),
    forecast: number(value.forecast),
    forecastLow: number(value.forecastLow),
    forecastHigh: number(value.forecastHigh),
    costHours: number(value.costHours),
    status,
    statusLabel: text(value.statusLabel) ?? status,
    experimentId: text(value.experimentId),
    outcome: number(value.outcome),
    createdAt: number(value.createdAt) ?? 0,
    startedAt: number(value.startedAt),
    closedAt: number(value.closedAt),
    adjusted: readAdjusted(value.adjusted),
  };
}

function readAdjusted(value: unknown): Idea["adjusted"] {
  if (!isObject(value)) return null;
  const forecast = number(value.forecast);
  const low = number(value.low);
  const high = number(value.high);
  return forecast === null || low === null || high === null ? null : { forecast, low, high };
}

export function parseIdeas(value: unknown): IdeasPayload {
  const body = isObject(value) ? value : {};
  const calibration = isObject(body.calibration) ? body.calibration : {};
  const nextIdea = isObject(body.next) ? readIdea(body.next.idea) : null;
  return {
    next: nextIdea && isObject(body.next) ? { idea: nextIdea, reason: text(body.next.reason) ?? "" } : null,
    calibration: {
      scored: number(calibration.scored) ?? 0,
      bias: number(calibration.bias),
      meanAbsError: number(calibration.meanAbsError),
      withInterval: number(calibration.withInterval) ?? 0,
      insideInterval: number(calibration.insideInterval) ?? 0,
      tested: number(calibration.tested) ?? 0,
      kept: number(calibration.kept) ?? 0,
    },
    byClass: list(body.byClass).flatMap((score): ClassScore[] => {
      if (!isObject(score)) return [];
      return [{
        class: text(score.class),
        tested: number(score.tested) ?? 0,
        kept: number(score.kept) ?? 0,
        meanGain: number(score.meanGain),
        gainPerHour: number(score.gainPerHour),
      }];
    }),
    ideas: list(body.ideas).flatMap((idea) => { const parsed = readIdea(idea); return parsed ? [parsed] : []; }),
  };
}

export function parseBrief(value: unknown): BriefPayload {
  const body = isObject(value) ? value : {};
  const brief = isObject(body.brief)
    ? {
        objective: text(body.brief.objective),
        mayChange: text(body.brief.mayChange),
        mustNotChange: text(body.brief.mustNotChange),
        locked: list(body.brief.locked).flatMap((entry) => (isObject(entry) && typeof entry.path === "string" ? [{ path: entry.path }] : [])),
        budgetRuns: number(body.brief.budgetRuns),
        budgetGpuHours: number(body.brief.budgetGpuHours),
        budgetDays: number(body.brief.budgetDays),
        pointers: text(body.brief.pointers),
        sources: text(body.brief.sources),
        stopRule: text(body.brief.stopRule),
        confirmedAt: number(body.brief.confirmedAt),
        confirmedBy: text(body.brief.confirmedBy),
        chatSessionId: text(body.brief.chatSessionId),
      }
    : null;
  const usage = isObject(body.usage)
    ? { runs: number(body.usage.runs) ?? 0, runHours: number(body.usage.runHours) ?? 0, days: number(body.usage.days) ?? 0 }
    : null;
  return {
    brief,
    primary: readPrimary(body.primary),
    usage,
    missing: list(body.missing).flatMap((item) => (typeof item === "string" ? [plainMissing(item)] : [])),
  };
}

/** What a brief still needs, as the dashboard says it: without the
 * command-line hint the agent gets, such as "(--stop-rule)". */
export function plainMissing(item: string): string {
  return item.replace(/\s*\((?:--|orx )[^()]*\)\s*$/, "");
}

export function parseFindings(value: unknown): FindingRow[] {
  const body = isObject(value) ? value : {};
  return list(body.findings).flatMap((row): FindingRow[] => {
    if (!isObject(row) || !isObject(row.finding) || typeof row.finding.statement !== "string") return [];
    const latest = isObject(row.latest) && typeof row.latest.verdict === "string"
      ? { verdict: row.latest.verdict, confidence: number(row.latest.confidence), scope: text(row.latest.scope) }
      : null;
    return [{
      finding: {
        id: text(row.finding.id) ?? "",
        statement: row.finding.statement,
        kind: text(row.finding.kind) ?? "",
        status: text(row.finding.status) ?? "open",
        locked: row.finding.locked === true,
      },
      supporting: number(row.supporting) ?? 0,
      against: number(row.against) ?? 0,
      latest,
    }];
  });
}

// --- numbers --------------------------------------------------------------------

/** The metric's value the way `orx metric` prints it: about five significant digits. */
export function formatValue(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "–";
  if (value === 0) return "0";
  const magnitude = Math.abs(value);
  if (magnitude < 1e-4 || magnitude >= 1e6) return value.toExponential(4);
  const digits = Math.min(9, Math.max(0, 5 - Math.floor(Math.log10(magnitude))));
  const fixed = value.toFixed(digits);
  return fixed.includes(".") ? fixed.replace(/0+$/, "").replace(/\.$/, "") : fixed;
}

/** A change, with its sign: "+0.003", "−0.0012" (true minus), "0". */
export function formatSigned(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "–";
  const body = formatValue(Math.abs(value));
  if (value > 0) return `+${body}`;
  if (value < 0) return `−${body}`;
  return body;
}

/** A change counted in the metric's improving direction. */
export function gainOf(change: number, direction: Direction): number {
  return direction === "max" ? change : -change;
}

export type Tone = "gain" | "loss" | "noise";

/** How a change reads against the noise floor. */
export function toneOf(change: number, direction: Direction, noiseFloor: number | null): Tone {
  const gain = gainOf(change, direction);
  const floor = noiseFloor ?? 0;
  if (Math.abs(gain) <= floor) return "noise";
  return gain > 0 ? "gain" : "loss";
}

export function percent(part: number, whole: number): number {
  return whole > 0 ? Math.round((100 * part) / whole) : 0;
}

// --- the running best ------------------------------------------------------------

export interface ChartPoint {
  x: number;
  y: number;
  score: number;
  newBest: boolean;
  runId: string;
}

export interface Chart {
  points: ChartPoint[];
  /** SVG path of the running best as a step line. */
  bestPath: string;
  /** Height of the baseline, when known and on the chart. */
  baselineY: number | null;
  ticks: { y: number; label: string }[];
}

export interface ChartBox {
  width: number;
  height: number;
  /** Room for the tick labels on the left. */
  left: number;
  top: number;
  bottom: number;
  right: number;
}

/** Round numbers inside [low, high]: at most four of them, as many as fit. */
export function niceTicks(low: number, high: number): number[] {
  if (!Number.isFinite(low) || !Number.isFinite(high) || high < low) return [];
  if (low === high) return [low];
  const power = 10 ** Math.floor(Math.log10((high - low) / 4));
  const span = (step: number) => [Math.ceil(low / step - 1e-9), Math.floor(high / step + 1e-9)];
  const step = [1, 2, 2.5, 5, 10, 20, 25, 50, 100]
    .map((factor) => factor * power)
    .find((candidate) => { const [first, last] = span(candidate); return last - first + 1 <= 4; }) ?? 100 * power;
  const [first, last] = span(step);
  const ticks: number[] = [];
  for (let index = first; index <= last; index += 1) ticks.push(Number((index * step).toPrecision(12)));
  return ticks;
}

/** Lays the running best out in `box`: one point per finished run in the
 * order runs ended, evenly spaced, with the best so far as a step line and the
 * baseline as a reference. */
export function runningBestChart(history: readonly HistoryPoint[], baseline: number | null, box: ChartBox): Chart {
  if (history.length === 0) return { points: [], bestPath: "", baselineY: null, ticks: [] };
  const values = history.flatMap((point) => [point.score, point.bestSoFar]);
  if (baseline !== null) values.push(baseline);
  let low = Math.min(...values);
  let high = Math.max(...values);
  if (low === high) {
    const pad = Math.abs(low) * 0.1 || 1;
    low -= pad;
    high += pad;
  } else {
    const pad = (high - low) * 0.08;
    low -= pad;
    high += pad;
  }
  const plotWidth = box.width - box.left - box.right;
  const plotHeight = box.height - box.top - box.bottom;
  const xOf = (index: number) => box.left + (history.length === 1 ? plotWidth / 2 : (index * plotWidth) / (history.length - 1));
  const yOf = (value: number) => box.top + ((high - value) / (high - low)) * plotHeight;
  const points = history.map((point, index) => ({
    x: xOf(index),
    y: yOf(point.score),
    score: point.score,
    newBest: point.newBest,
    runId: point.runId,
  }));
  let bestPath = "";
  history.forEach((point, index) => {
    const x = xOf(index);
    const y = yOf(point.bestSoFar);
    bestPath += index === 0 ? `M${x.toFixed(1)},${y.toFixed(1)}` : `H${x.toFixed(1)}V${y.toFixed(1)}`;
  });
  if (history.length === 1) bestPath += `H${(box.left + plotWidth).toFixed(1)}`;
  return {
    points,
    bestPath,
    baselineY: baseline === null ? null : yOf(baseline),
    ticks: niceTicks(low, high).map((tick) => ({ y: yOf(tick), label: formatValue(tick) })),
  };
}

// --- what moved the score --------------------------------------------------------

export interface WaterfallRow {
  /** The idea's title, else the experiment's name. */
  label: string;
  /** The experiment, when the label is an idea. */
  detail: string | null;
  kind: "baseline" | "step" | "unscored" | "best";
  /** Metric values the bar spans; equal for the baseline marker. */
  from: number | null;
  to: number | null;
  delta: number | null;
  tone: Tone | null;
  /** A share of the total gain, for steps; null when there is no gain to split. */
  share: number | null;
  ablation?: Ablation;
}

/** The lineage from the baseline to the best as rows of a waterfall: the
 * baseline, each step's change (unscored experiments stay as rows without a
 * bar, their change folded into the next scored step), and the total. */
export function waterfall(path: readonly PathStep[], direction: Direction, noiseFloor: number | null): WaterfallRow[] {
  const scored = path.filter((step) => step.score !== null);
  if (scored.length === 0) return [];
  const first = scored[0];
  const last = scored[scored.length - 1];
  const total = first.score !== null && last.score !== null ? last.score - first.score : null;
  const totalGain = total === null ? 0 : gainOf(total, direction);
  const rows: WaterfallRow[] = [];
  let previous: number | null = null;
  for (const step of path) {
    const label = step.idea ?? step.experiment;
    const detail = step.idea ? step.experiment : null;
    if (step.score === null) {
      rows.push({ label, detail, kind: "unscored", from: null, to: null, delta: null, tone: null, share: null, ...(step.ablation ? { ablation: step.ablation } : {}) });
      continue;
    }
    if (previous === null) {
      rows.push({ label: "Baseline", detail: step.experiment, kind: "baseline", from: step.score, to: step.score, delta: null, tone: null, share: null });
    } else {
      const delta = step.score - previous;
      rows.push({
        label,
        detail,
        kind: "step",
        from: previous,
        to: step.score,
        delta,
        tone: toneOf(delta, direction, noiseFloor),
        share: totalGain > 0 ? gainOf(delta, direction) / totalGain : null,
        ...(step.ablation ? { ablation: step.ablation } : {}),
      });
    }
    previous = step.score;
  }
  if (scored.length > 1 && first.score !== null && last.score !== null) {
    rows.push({
      label: "Best",
      detail: last.experiment,
      kind: "best",
      from: first.score,
      to: last.score,
      delta: total,
      tone: total === null ? null : toneOf(total, direction, noiseFloor),
      share: null,
    });
  }
  return rows;
}

/** The value range a waterfall's bars need, padded a little. */
export function waterfallDomain(rows: readonly WaterfallRow[]): [number, number] | null {
  const values = rows.flatMap((row) => [row.from, row.to]).filter((value): value is number => value !== null);
  if (values.length === 0) return null;
  let low = Math.min(...values);
  let high = Math.max(...values);
  if (low === high) {
    const pad = Math.abs(low) * 0.1 || 1;
    return [low - pad, high + pad];
  }
  const pad = (high - low) * 0.05;
  low -= pad;
  high += pad;
  return [low, high];
}

/** What an ablation check found, or that it is still to run. */
export function ablationLine(ablation: Ablation): string {
  if (ablation.status === "confirmed" && ablation.outcome !== null) return `Ablation: taking it out changed ${formatSigned(ablation.outcome)}, about what it gained`;
  if (ablation.status === "refuted" && ablation.outcome !== null) return `Ablation: taking it out changed ${formatSigned(ablation.outcome)}, so the steps interact`;
  if (ablation.status === "dropped") return "Ablation check dropped";
  if (ablation.status === "running") return "Ablation check running";
  return "Ablation check queued";
}

// --- ideas -------------------------------------------------------------------------

const BLOCKER_KINDS = new Set(["infra-blocker", "eval-blocker", "halt"]);

export function isBlocker(idea: Idea): boolean {
  return BLOCKER_KINDS.has(idea.kind);
}

export interface IdeaGroups {
  blockers: Idea[];
  running: Idea[];
  open: Idea[];
  /** Tested and closed, newest first. */
  tried: Idea[];
}

export function groupIdeas(ideas: readonly Idea[]): IdeaGroups {
  const blockers = ideas.filter((idea) => isBlocker(idea) && (idea.status === "open" || idea.status === "running"));
  const running = ideas.filter((idea) => !isBlocker(idea) && idea.status === "running");
  const open = ideas
    .filter((idea) => !isBlocker(idea) && idea.status === "open")
    .sort((a, b) => a.priority - b.priority || a.createdAt - b.createdAt);
  const tried = ideas
    .filter((idea) => !isBlocker(idea) && (idea.status === "kept" || idea.status === "refuted"))
    .sort((a, b) => (b.closedAt ?? 0) - (a.closedAt ?? 0));
  return { blockers, running, open, tried };
}

/** "+0.003 (−0.001 to +0.007)", or "no forecast". */
export function forecastText(idea: Idea): string {
  if (idea.forecast === null) return "no forecast";
  const interval = idea.forecastLow !== null && idea.forecastHigh !== null
    ? ` (${formatSigned(idea.forecastLow)} to ${formatSigned(idea.forecastHigh)})`
    : "";
  return `${formatSigned(idea.forecast)}${interval}`;
}

/** "+0.0044 (−0.0008 to +0.0083)": the forecast as past misses suggest reading it. */
export function adjustedText(idea: Idea): string | null {
  const adjusted = idea.adjusted;
  if (!adjusted) return null;
  return `${formatSigned(adjusted.forecast)} (${formatSigned(adjusted.low)} to ${formatSigned(adjusted.high)})`;
}

/** Whether a measured outcome landed inside its forecast's 80% interval. */
export function insideInterval(idea: Idea): boolean | null {
  if (idea.outcome === null || idea.forecastLow === null || idea.forecastHigh === null) return null;
  return idea.outcome >= idea.forecastLow && idea.outcome <= idea.forecastHigh;
}

export function calibrationLine(calibration: Calibration): string | null {
  if (calibration.tested === 0 && calibration.scored === 0) return null;
  const parts: string[] = [];
  if (calibration.tested > 0) parts.push(`${calibration.kept} of ${calibration.tested} tested ideas kept (${percent(calibration.kept, calibration.tested)}%)`);
  if (calibration.bias !== null && calibration.scored > 0) {
    const lean = calibration.bias > 0 ? "outcomes ran above forecasts" : calibration.bias < 0 ? "outcomes ran below forecasts" : "no lean";
    parts.push(`mean forecast error ${formatSigned(calibration.bias)} (${lean})`);
  }
  if (calibration.withInterval > 0) parts.push(`${percent(calibration.insideInterval, calibration.withInterval)}% inside their 80% intervals`);
  return parts.join(" · ");
}

// --- the brief ----------------------------------------------------------------------

export interface BudgetLine {
  label: string;
  used: number;
  limit: number;
  /** 0 to 1, capped. */
  fraction: number;
  text: string;
  tone: "normal" | "warn" | "alert";
}

/** One line per budget the brief sets: runs, GPU-hours, days. */
export function budgetLines(brief: Brief, usage: Usage | null): BudgetLine[] {
  const used = usage ?? { runs: 0, runHours: 0, days: 0 };
  const lines: BudgetLine[] = [];
  const add = (label: string, value: number, limit: number | null, unit: (value: number) => string) => {
    if (limit === null || limit <= 0) return;
    const fraction = Math.min(1, value / limit);
    lines.push({
      label,
      used: value,
      limit,
      fraction,
      text: `${unit(value)} of ${unit(limit)}`,
      tone: value >= limit ? "alert" : fraction >= 0.8 ? "warn" : "normal",
    });
  };
  const hours = (value: number) => (value < 0.05 ? "0" : value < 10 ? value.toFixed(1) : String(Math.round(value)));
  add("Runs", used.runs, brief.budgetRuns, (value) => String(Math.round(value)));
  add("Run hours", used.runHours, brief.budgetGpuHours, hours);
  add("Days", used.days, brief.budgetDays, hours);
  return lines;
}

// --- starting research from the dashboard -------------------------------------------

/** A seed idea typed into the setup form. */
export interface SetupIdea {
  title: string;
  /** The expected change in the metric, as typed; blank when unknown. */
  forecast: string;
}

/** The setup form as typed: strings until saved. */
export interface SetupDraft {
  objective: string;
  metricKey: string;
  direction: Direction;
  split: string;
  noiseFloor: string;
  mayChange: string;
  mustNotChange: string;
  /** One repository path per line. */
  locked: string;
  budgetRuns: string;
  budgetGpuHours: string;
  budgetDays: string;
  stopRule: string;
  pointers: string;
  sources: string;
  /** New seed ideas; ideas already queued stay as they are. */
  ideas: SetupIdea[];
}

export function emptyDraft(): SetupDraft {
  return {
    objective: "",
    metricKey: "",
    direction: "max",
    split: "",
    noiseFloor: "",
    mayChange: "",
    mustNotChange: "",
    locked: "",
    budgetRuns: "",
    budgetGpuHours: "",
    budgetDays: "",
    stopRule: "",
    pointers: "",
    sources: "",
    ideas: [{ title: "", forecast: "" }],
  };
}

const shown = (value: number | null) => (value === null ? "" : String(value));

/** The form filled in from a saved brief, to edit it. */
export function draftFromBrief(payload: BriefPayload): SetupDraft {
  const { brief, primary } = payload;
  return {
    ...emptyDraft(),
    objective: brief?.objective ?? "",
    metricKey: primary?.key ?? "",
    direction: primary?.direction ?? "max",
    split: primary?.split ?? "",
    noiseFloor: shown(primary?.noiseFloor ?? null),
    mayChange: brief?.mayChange ?? "",
    mustNotChange: brief?.mustNotChange ?? "",
    locked: (brief?.locked ?? []).map((entry) => entry.path).join("\n"),
    budgetRuns: shown(brief?.budgetRuns ?? null),
    budgetGpuHours: shown(brief?.budgetGpuHours ?? null),
    budgetDays: shown(brief?.budgetDays ?? null),
    stopRule: brief?.stopRule ?? "",
    pointers: brief?.pointers ?? "",
    sources: brief?.sources ?? "",
    ideas: [{ title: "", forecast: "" }],
  };
}

type Parsed = { ok: true; value: number | null } | { ok: false };

function parseNumber(text: string, check: (value: number) => boolean): Parsed {
  const trimmed = text.trim();
  if (trimmed === "") return { ok: true, value: null };
  const value = Number(trimmed.replace(/^\u2212/, "-"));
  return Number.isFinite(value) && check(value) ? { ok: true, value } : { ok: false };
}

const positive = (value: number) => value > 0;
const wholePositive = (value: number) => Number.isInteger(value) && value > 0;

export interface SetupProblems {
  objective?: string;
  metricKey?: string;
  noiseFloor?: string;
  budget?: string;
  ideas?: string;
}

/** What has to be fixed before the form can be saved. */
export function setupProblems(draft: SetupDraft): SetupProblems {
  const problems: SetupProblems = {};
  if (!draft.objective.trim()) problems.objective = "Say what to improve.";
  if (!draft.metricKey.trim()) problems.metricKey = "Name the metric that measures it, as the runs report it.";
  if (!parseNumber(draft.noiseFloor, (value) => value >= 0).ok) problems.noiseFloor = "The noise floor is a number of zero or more.";
  const budgets = [
    parseNumber(draft.budgetRuns, wholePositive),
    parseNumber(draft.budgetGpuHours, positive),
    parseNumber(draft.budgetDays, positive),
  ];
  if (budgets.some((parsed) => !parsed.ok)) problems.budget = "Budgets are positive numbers, and runs a whole number.";
  if (draft.ideas.some((idea) => idea.title.trim() && !parseNumber(idea.forecast, () => true).ok)) {
    problems.ideas = "A forecast is a number: the change you expect in the metric.";
  }
  return problems;
}

const value = (parsed: Parsed) => (parsed.ok ? parsed.value : null);
const textOrNull = (text: string) => (text.trim() ? text.trim() : null);

/** The body for PUT /api/vlresearch/projects/{id}/brief. */
export function briefPayload(draft: SetupDraft) {
  return {
    objective: textOrNull(draft.objective),
    mayChange: textOrNull(draft.mayChange),
    mustNotChange: textOrNull(draft.mustNotChange),
    locked: draft.locked.split(/[\n,]/).map((path) => path.trim()).filter(Boolean),
    budgetRuns: value(parseNumber(draft.budgetRuns, wholePositive)),
    budgetGpuHours: value(parseNumber(draft.budgetGpuHours, positive)),
    budgetDays: value(parseNumber(draft.budgetDays, positive)),
    pointers: textOrNull(draft.pointers),
    sources: textOrNull(draft.sources),
    stopRule: textOrNull(draft.stopRule),
    metric: draft.metricKey.trim()
      ? {
          key: draft.metricKey.trim(),
          direction: draft.direction,
          noiseFloor: value(parseNumber(draft.noiseFloor, (number) => number >= 0)),
          split: textOrNull(draft.split),
        }
      : null,
  };
}

/** The body for POST /api/vlresearch/projects/{id}/ideas, or null for a blank row. */
export function ideaPayload(idea: SetupIdea) {
  const title = idea.title.trim();
  return title ? { title, forecast: value(parseNumber(idea.forecast, () => true)) } : null;
}

/** The bodies for POST /api/vlresearch/projects/{id}/ideas, one per typed idea. */
export function ideaPayloads(draft: SetupDraft) {
  return draft.ideas.flatMap((idea) => {
    const body = ideaPayload(idea);
    return body ? [body] : [];
  });
}

/** Seed ideas whose forecast moves the metric the wrong way, which is
 * usually a sign slip: with lower-is-better metrics an improvement is negative. */
export function worseningIdeas(draft: SetupDraft): string[] {
  return draft.ideas.flatMap((idea) => {
    const body = ideaPayload(idea);
    if (!body || body.forecast === null) return [];
    const worse = draft.direction === "max" ? body.forecast < 0 : body.forecast > 0;
    return worse ? [body.title] : [];
  });
}

/** The draft without the idea rows at `saved`, so a retry doesn't queue them twice. */
export function withoutIdeas(draft: SetupDraft, saved: ReadonlySet<number>): SetupDraft {
  const ideas = draft.ideas.filter((_, index) => !saved.has(index));
  return { ...draft, ideas: ideas.length > 0 ? ideas : [{ title: "", forecast: "" }] };
}

/** The first message of the chat the Research view starts. */
export function kickoffMessage(projectId: string, seedIdeas: number): string {
  const ideas = seedIdeas === 0
    ? "no seed ideas yet, so propose them from my pointers"
    : `${seedIdeas} seed idea${seedIdeas === 1 ? "" : "s"} (\`orx idea list ${projectId}\`)`;
  return [
    `Let's run auto-research on this project. I set the goal in the Research view: \`orx brief show ${projectId}\` has it, with ${ideas}.`,
    "",
    "Scope it with me using the orx-grill skill, starting from what I filled in:",
    "- ask only about what's missing or unclear, each question with your recommended answer;",
    "- check the data and the baseline on the compute host with probe runs, and measure the noise floor from three seeds;",
    "- review the code for leakage, and file what you find as blockers;",
    "- propose more ideas, each with a forecast, a cost and a kill test.",
    "",
    "Then ask me to confirm the brief; I'll do it in the Research view. Once it's confirmed, run the improvement loop with the orx-improve skill.",
  ].join("\n");
}

/** Sent to that chat when the user confirms the brief in the Research view. */
export const CONFIRMED_MESSAGE =
  "I confirmed the brief in the Research view. Start the improvement loop with the orx-improve skill.";
