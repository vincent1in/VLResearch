// VLResearch: the Research view of the Experiments tab. A project runs one or
// more optimizations, each in its own chat; the view shows one at a time: its
// surface (the running best against the baseline, and which steps moved the
// score), its idea queue by title and its brief with its budget, then the
// project's findings ledger. The user starts optimizations here (the setup
// form) and confirms their briefs here; the agent and `orx` write the rest.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpen, Check, ChevronDown, ChevronRight, ClipboardList, FlaskConical, Lightbulb, Lock, MessageSquare, MessagesSquare, Play, Plus, Settings2, Target, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { sendChatMessage } from "../api";
import { Button, IconButton } from "../components/ui";
import { workspaceKey } from "../queries/client";
import { ResearchSetup, refreshResearch } from "./ResearchSetup";
import {
  CONFIRMED_MESSAGE,
  defaultOptimization,
  draftFromBrief,
  optimizationLabel,
  parseOptimizations,
  emptyDraft,
  isBlocker,
  ablationLine,
  adjustedText,
  askAboutFinding,
  askAboutIdea,
  askAboutNext,
  askAboutStep,
  budgetLines,
  calibrationLine,
  forecastText,
  formatSigned,
  formatValue,
  groupIdeas,
  insideInterval,
  parseBrief,
  parseFindings,
  parseIdeas,
  parseMetrics,
  percent,
  runningBestChart,
  waterfall,
  waterfallDomain,
  type BriefPayload,
  type FindingRow,
  type Idea,
  type IdeasPayload,
  type MetricsPayload,
  type Tone,
  type WaterfallRow,
} from "./research";

const POLL_MS = 30_000;
/** Rows a list shows before "Show all". */
const LIST_LIMIT = 6;

async function fetchJson<T>(url: string, parse: (value: unknown) => T, signal: AbortSignal): Promise<T> {
  const response = await fetch(url, { signal });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return parse(await response.json());
}

function useProjectData<T>(projectId: string, family: string, path: string, parse: (value: unknown) => T) {
  return useQuery({
    queryKey: workspaceKey(family, projectId),
    queryFn: ({ signal }) => fetchJson(`/api/vlresearch/projects/${encodeURIComponent(projectId)}/${path}`, parse, signal),
    refetchInterval: POLL_MS,
    retry: false,
  });
}

function useOptimizationData<T>(optimizationId: string | null, family: string, path: string, parse: (value: unknown) => T) {
  return useQuery({
    queryKey: workspaceKey(family, optimizationId),
    queryFn: ({ signal }) =>
      fetchJson(`/api/vlresearch/optimizations/${encodeURIComponent(optimizationId ?? "")}/${path}`, parse, signal),
    enabled: optimizationId !== null,
    refetchInterval: POLL_MS,
    retry: false,
  });
}


/** The element's width in pixels, following resizes. */
function useWidth<E extends HTMLElement>(): [RefObject<E | null>, number] {
  const ref = useRef<E | null>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    setWidth(element.clientWidth);
    const observer = new ResizeObserver((entries) => setWidth(entries[0]?.contentRect.width ?? 0));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width];
}

const TONE_BAR: Record<Tone, string> = {
  gain: "bg-accent-green",
  loss: "bg-accent-red",
  noise: "bg-muted",
};

const TONE_TEXT: Record<Tone, string> = {
  gain: "text-accent-green",
  loss: "text-accent-red",
  noise: "text-muted",
};

function Section({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <section className="mt-5 pt-4 border-t border-t-border-variant">
      <h2 className="m-0 mb-3 flex items-center gap-1.5 text-sm font-semibold text-text [&_svg]:text-muted">
        {icon}
        {title}
      </h2>
      {children}
    </section>
  );
}

function Note({ children }: { children: ReactNode }) {
  return <p className="m-0 text-sm text-subtext leading-relaxed">{children}</p>;
}

function Code({ children }: { children: ReactNode }) {
  return <code className="py-px px-1 rounded-sm bg-hover-subtle text-xs text-text">{children}</code>;
}

function Chip({ tone, children }: { tone: "green" | "red" | "amber" | "blue" | "muted"; children: ReactNode }) {
  const tones = {
    green: "bg-accent-green-subtle text-accent-green",
    red: "bg-accent-red-subtle text-accent-red",
    amber: "bg-accent-amber-subtle text-accent-amber",
    blue: "bg-accent-blue-subtle text-accent-blue",
    muted: "bg-hover-subtle text-subtext",
  };
  return <span className={`inline-flex shrink-0 items-center py-px px-1.5 rounded-sm text-xs font-medium ${tones[tone]}`}>{children}</span>;
}

/** Long lists show their first rows, and the rest on request. */
function Collapsible<T>({ items, render, noun }: { items: readonly T[]; render: (item: T, index: number) => ReactNode; noun: string }) {
  const [all, setAll] = useState(false);
  const shown = all ? items : items.slice(0, LIST_LIMIT);
  return (
    <>
      {shown.map(render)}
      {items.length > LIST_LIMIT && (
        <button
          type="button"
          className="mt-1 p-0 border-0 bg-transparent text-xs text-subtext cursor-pointer hover:text-text"
          onClick={() => setAll((open) => !open)}
        >
          {all ? "Show fewer" : `Show all ${items.length} ${noun}`}
        </button>
      )}
    </>
  );
}

/** Opens a side chat off the chat that runs the loop, with a question. */
type Ask = (question: string) => void;

function AskButton({ label, question, onAsk }: { label: string; question: string; onAsk: Ask }) {
  return (
    <IconButton size="small" className="h-6 w-6 text-muted" aria-label={label} title={`${label} in a side chat; the loop keeps running`} onClick={() => onAsk(question)}>
      <MessagesSquare size={13} />
    </IconButton>
  );
}

// --- the optimization surface ---------------------------------------------------------

function Headline({ metrics }: { metrics: MetricsPayload }) {
  const primary = metrics.primary;
  if (!primary) {
    return (
      <Note>
        No primary metric yet. The agent picks one with you while scoping the project, or set it with{" "}
        <Code>orx metric primary &lt;projectId&gt; &lt;key&gt; --maximize</Code>.
        {metrics.keys.length > 0 && <> Runs so far report {metrics.keys.join(", ")}.</>}
      </Note>
    );
  }
  const describe = [
    primary.direction === "max" ? "higher is better" : "lower is better",
    primary.split ? `${primary.split} split` : null,
    primary.noiseFloor !== null ? `noise floor ${formatValue(primary.noiseFloor)}` : null,
  ].filter(Boolean).join(" · ");
  const best = metrics.best;
  const baseline = metrics.baseline;
  const change = best?.score != null && baseline ? best.score - baseline.score : null;
  return (
    <div className="flex flex-col gap-1">
      <div className="text-sm text-subtext">
        <span className="font-medium text-text">{primary.key}</span> <span>({describe})</span>
      </div>
      {best?.score != null ? (
        <>
          <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
            <span className="text-2xl font-semibold text-text tabular-nums">{formatValue(best.score)}</span>
            {change !== null && baseline && baseline.experimentId !== best.experimentId && (
              <span className="text-sm text-subtext tabular-nums">
                <span className={change === 0 ? "" : (primary.direction === "max") === (change > 0) ? "text-accent-green" : "text-accent-red"}>
                  {formatSigned(change)}
                </span>{" "}
                over the baseline&apos;s {formatValue(baseline.score)}
                {baseline.probeRuns > 0
                  ? ` (mean of ${baseline.probeRuns} probe runs)`
                  : baseline.runs > 1 && ` (mean of ${baseline.runs} runs)`}
              </span>
            )}
          </div>
          <div className="text-sm text-subtext">
            Best: <span className="text-text">{best.experiment}</span>
            {baseline && best.experimentId === baseline.experimentId
              ? ", the baseline: nothing has beaten it yet"
              : best.runs > 1 && ` (mean of ${best.runs} runs)`}
            {metrics.trials && (
              <>
                {" · "}
                {metrics.trials.runs} finished run{metrics.trials.runs === 1 ? "" : "s"} · {metrics.trials.ideasTested} idea
                {metrics.trials.ideasTested === 1 ? "" : "s"} tested, {metrics.trials.ideasKept} kept
              </>
            )}
          </div>
        </>
      ) : (
        <Note>No finished run has reported {primary.key} yet.</Note>
      )}
    </div>
  );
}

const CHART_HEIGHT = 180;
const CHART_BOX = { left: 52, right: 12, top: 10, bottom: 24 };

function RunningBest({ metrics }: { metrics: MetricsPayload }) {
  const [ref, width] = useWidth<HTMLDivElement>();
  const names = useMemo(() => new Map(metrics.runs.map((run) => [run.runId, run.experiment])), [metrics.runs]);
  const chart = useMemo(
    () => runningBestChart(metrics.history, metrics.baseline?.score ?? null, { width: Math.max(width, 200), height: CHART_HEIGHT, ...CHART_BOX }),
    [metrics.history, metrics.baseline, width],
  );
  if (metrics.history.length === 0) return <Note>The chart fills in as runs finish.</Note>;
  const plotRight = Math.max(width, 200) - CHART_BOX.right;
  return (
    <div ref={ref} className="w-full">
      {width > 0 && (
        <svg width={width} height={CHART_HEIGHT} role="img" aria-label="The best score so far after each finished run" className="block overflow-visible">
          {chart.ticks.map((tick) => (
            <g key={tick.label}>
              <line x1={CHART_BOX.left} x2={plotRight} y1={tick.y} y2={tick.y} className="stroke-border-variant" strokeWidth={1} />
              <text x={CHART_BOX.left - 6} y={tick.y} dy="0.32em" textAnchor="end" className="fill-muted text-xs tabular-nums">
                {tick.label}
              </text>
            </g>
          ))}
          {chart.baselineY !== null && (
            <g>
              <line x1={CHART_BOX.left} x2={plotRight} y1={chart.baselineY} y2={chart.baselineY} className="stroke-muted" strokeWidth={1} strokeDasharray="4 3" />
              <text x={plotRight} y={chart.baselineY - 4} textAnchor="end" className="fill-muted text-xs">
                baseline
              </text>
            </g>
          )}
          <path d={chart.bestPath} fill="none" className="stroke-accent-green" strokeWidth={2} strokeLinejoin="round" />
          {chart.points.map((point) => (
            <circle key={point.runId} cx={point.x} cy={point.y} r={point.newBest ? 3.5 : 2.5} className={point.newBest ? "fill-accent-green" : "fill-muted"}>
              <title>{`${names.get(point.runId) ?? "Run"}: ${formatValue(point.score)}${point.newBest ? " (new best)" : ""}`}</title>
            </circle>
          ))}
          <text x={(CHART_BOX.left + plotRight) / 2} y={CHART_HEIGHT - 4} textAnchor="middle" className="fill-muted text-xs">
            finished runs, in the order they ended
          </text>
        </svg>
      )}
    </div>
  );
}

function bar(row: WaterfallRow, domain: [number, number]): { left: string; width: string } | null {
  if (row.from === null || row.to === null) return null;
  const [low, high] = domain;
  const start = (Math.min(row.from, row.to) - low) / (high - low);
  const end = (Math.max(row.from, row.to) - low) / (high - low);
  return { left: `${(100 * start).toFixed(2)}%`, width: `${(100 * (end - start)).toFixed(2)}%` };
}

function Waterfall({ metrics, onAsk }: { metrics: MetricsPayload; onAsk?: Ask }) {
  const primary = metrics.primary;
  const rows = useMemo(
    () => (primary ? waterfall(metrics.path, primary.direction, primary.noiseFloor) : []),
    [metrics.path, primary],
  );
  const domain = useMemo(() => waterfallDomain(rows), [rows]);
  if (!primary || !domain || rows.length < 2) {
    return <Note>Once a run beats the baseline, this splits the gain over the experiments between them.</Note>;
  }
  return (
    <div className="flex flex-col gap-2">
      <Note>
        Each experiment branches from its parent, so the gain from the baseline to the best splits into the change each
        step made along the way.
      </Note>
      {/* One grid for every row, so all bars share one scale. */}
      <div className="grid grid-cols-[minmax(0,_1.3fr)_minmax(4rem,_1fr)_auto] items-center gap-x-3">
        {rows.map((row, index) => {
          const span = bar(row, domain);
          const total = row.kind === "best";
          const cell = `py-1.5 ${total ? "mt-1 pt-2.5 border-t border-t-border-variant" : ""}`;
          return (
            <div key={`${row.kind}-${index}`} className="contents">
              <div className={`min-w-0 ${cell}`}>
                <div className="flex min-w-0 items-center gap-1">
                  <div dir="auto" className={`truncate text-sm text-text ${total || row.kind === "baseline" ? "font-medium" : ""}`} title={row.label}>
                    {row.label}
                  </div>
                  {onAsk && row.kind === "step" && <AskButton label="Ask about this step" question={askAboutStep(row, primary.key)} onAsk={onAsk} />}
                </div>
                {(row.detail || row.kind === "unscored") && (
                  <div dir="auto" className="truncate text-xs text-muted" title={row.detail ?? undefined}>
                    {row.kind === "unscored" ? "no score of its own" : row.detail}
                  </div>
                )}
              </div>
              <div className={`self-stretch flex items-center ${cell}`}>
                <div className="relative h-3.5 w-full rounded-sm bg-hover-faint">
                  {span && row.kind === "baseline" && (
                    <span className="absolute top-0 bottom-0 w-0.5 bg-subtext" style={{ left: span.left }} />
                  )}
                  {span && row.kind !== "baseline" && (
                    <span
                      className={`absolute top-0.5 bottom-0.5 min-w-0.5 rounded-xs ${total ? "bg-accent-blue" : row.tone ? TONE_BAR[row.tone] : "bg-muted"}`}
                      style={{ left: span.left, width: span.width }}
                    />
                  )}
                </div>
              </div>
              <div className={`text-end text-sm tabular-nums whitespace-nowrap ${cell}`}>
                {row.kind === "baseline" && <span className="text-text">{formatValue(row.from)}</span>}
                {row.kind === "step" && (
                  <span className={row.tone ? TONE_TEXT[row.tone] : ""}>
                    {formatSigned(row.delta)}
                    {row.share !== null && <span className="text-muted"> {Math.round(100 * row.share)}%</span>}
                  </span>
                )}
                {total && (
                  <span className="font-medium text-text">
                    {formatValue(row.to)} <span className="text-muted">({formatSigned(row.delta)})</span>
                  </span>
                )}
              </div>
              {row.ablation && (
                <div
                  className={`col-span-3 -mt-1 pb-1.5 text-xs ${row.ablation.status === "confirmed" ? "text-accent-green" : row.ablation.status === "refuted" ? "text-accent-amber" : "text-muted"}`}
                >
                  {ablationLine(row.ablation)}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// --- ideas --------------------------------------------------------------------------------

function statusChip(idea: Idea) {
  switch (idea.statusLabel) {
    case "kept":
    case "confirmed":
    case "resolved":
      return <Chip tone="green">{idea.statusLabel}</Chip>;
    case "refuted":
      return <Chip tone="red">refuted</Chip>;
    case "running":
      return <Chip tone="blue">running</Chip>;
    default:
      return <Chip tone="muted">{idea.statusLabel}</Chip>;
  }
}

function IdeaRow({ idea, tried, onAsk }: { idea: Idea; tried?: boolean; onAsk?: Ask }) {
  const inside = insideInterval(idea);
  return (
    <div className="flex flex-col gap-0.5 py-1.5 border-b border-b-border-variant last:border-b-0">
      <div className="flex min-w-0 items-center gap-2">
        <span dir="auto" className="min-w-0 flex-1 truncate text-sm text-text" title={idea.hypothesis ?? idea.title}>
          {idea.title}
        </span>
        {idea.class && <Chip tone="muted">{idea.class}</Chip>}
        {(tried || idea.status === "running") && statusChip(idea)}
        {onAsk && <AskButton label="Ask about this idea" question={askAboutIdea(idea)} onAsk={onAsk} />}
      </div>
      <div className="text-xs text-muted tabular-nums">
        {tried && idea.outcome !== null ? (
          <>
            measured {formatSigned(idea.outcome)}
            {idea.forecast !== null && ` against a forecast of ${forecastText(idea)}`}
            {inside !== null && (inside ? " · inside its interval" : " · outside its interval")}
          </>
        ) : (
          <>
            forecast {forecastText(idea)}
            {idea.costHours !== null && ` · ${formatValue(idea.costHours)} GPU-h`}
            {adjustedText(idea) && (
              <span title="Shifted by the mean error of past forecasts, with the interval stretched to cover 80% of past outcomes">
                {` · past misses suggest ${adjustedText(idea)}`}
              </span>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function IdeasPanel({ ideas, onAsk }: { ideas: IdeasPayload; onAsk?: Ask }) {
  const groups = useMemo(() => groupIdeas(ideas.ideas), [ideas.ideas]);
  const calibration = calibrationLine(ideas.calibration);
  if (ideas.ideas.length === 0) {
    return <Note>No ideas queued yet. The agent adds each one with a forecast, a cost and a kill test before testing it.</Note>;
  }
  return (
    <div className="flex flex-col gap-4">
      {ideas.next && (
        <div className="flex flex-col gap-1 py-2 px-3 rounded-md border border-border bg-surface">
          <div className="flex items-center justify-between gap-2">
            <div className="text-xs font-medium text-subtext">Up next</div>
            {onAsk && <AskButton label="Ask why it's next" question={askAboutNext(ideas.next.idea)} onAsk={onAsk} />}
          </div>
          <div dir="auto" className="text-sm font-medium text-text">{ideas.next.idea.title}</div>
          <div className="text-xs text-subtext">Why: {ideas.next.reason}.</div>
          {ideas.next.idea.killTest && <div className="text-xs text-subtext">Kill test first: {ideas.next.idea.killTest}</div>}
        </div>
      )}
      {groups.blockers.length > 0 && (
        <div className="flex flex-col">
          <div className="mb-1 flex items-center gap-1.5 text-xs font-medium text-accent-amber">
            <TriangleAlert size={12} /> Blocking new runs
          </div>
          {groups.blockers.map((idea) => (
            <div key={idea.id} dir="auto" className="py-1 text-sm text-text">
              {idea.title} <span className="text-xs text-muted">({idea.kind.replace("-", " ")})</span>
            </div>
          ))}
        </div>
      )}
      {(groups.running.length > 0 || groups.open.length > 0) && (
        <div className="flex flex-col">
          <div className="mb-1 text-xs font-medium text-subtext">
            Queued ({groups.running.length + groups.open.length})
          </div>
          <Collapsible items={[...groups.running, ...groups.open]} noun="ideas" render={(idea) => <IdeaRow key={idea.id} idea={idea} onAsk={onAsk} />} />
        </div>
      )}
      {groups.tried.length > 0 && (
        <div className="flex flex-col">
          <div className="mb-1 text-xs font-medium text-subtext">Tried ({groups.tried.length})</div>
          <Collapsible items={groups.tried} noun="ideas" render={(idea) => <IdeaRow key={idea.id} idea={idea} tried onAsk={onAsk} />} />
        </div>
      )}
      {ideas.byClass.length > 0 && (
        <div className="flex flex-col gap-1">
          <div className="text-xs font-medium text-subtext">By class</div>
          <div className="overflow-x-auto">
          <table className="w-full border-collapse text-sm tabular-nums [&_th]:py-1 [&_th]:pe-3 [&_th]:text-start [&_th]:text-xs [&_th]:font-medium [&_th]:text-muted [&_td]:py-1 [&_td]:pe-3 [&_td]:border-t [&_td]:border-t-border-variant">
            <thead>
              <tr>
                <th>Class</th>
                <th>Tested</th>
                <th>Kept</th>
                <th>Mean gain</th>
                <th>Per GPU-h</th>
              </tr>
            </thead>
            <tbody>
              {ideas.byClass.map((score) => (
                <tr key={score.class ?? "none"}>
                  <td className="text-text">{score.class ?? "unclassed"}</td>
                  <td>{score.tested}</td>
                  <td>{percent(score.kept, score.tested)}%</td>
                  <td>{formatSigned(score.meanGain)}</td>
                  <td>{formatSigned(score.gainPerHour)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
          <div className="text-xs text-muted">Gains count in the metric&apos;s improving direction.</div>
        </div>
      )}
      {calibration && <Note>{calibration}.</Note>}
    </div>
  );
}

// --- the brief and findings ---------------------------------------------------------------

const BUDGET_TONE = { normal: "bg-accent-blue", warn: "bg-accent-amber", alert: "bg-accent-red" } as const;

/** The user confirms the brief; the chat that runs the optimization is told. */
function useConfirmBrief(optimizationId: string) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/vlresearch/optimizations/${encodeURIComponent(optimizationId)}/brief/confirm`, { method: "POST" });
      if (!response.ok) throw new Error((await response.text()) || `HTTP ${response.status}`);
      const confirmed = parseBrief(await response.json());
      const chat = confirmed.brief?.chatSessionId;
      if (chat) await sendChatMessage(chat, CONFIRMED_MESSAGE);
      await refreshResearch(queryClient);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  };
  return { confirm, busy, error };
}

function ConfirmBar({ data, onEdit }: { data: BriefPayload; onEdit: () => void }) {
  const { confirm, busy, error } = useConfirmBrief(data.brief?.id ?? "");
  const ready = data.missing.length === 0;
  // No chat is working on the brief: starting one comes first, and the edit
  // form is where it starts.
  const unattended = !data.brief?.chatSessionId;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-2">
        {unattended && (
          <Button variant="primary" size="small" disabled={busy} title="Open a chat in which the agent checks the brief with you" onClick={onEdit}>
            <Play size={13} /> Start the agent
          </Button>
        )}
        <Button
          variant={unattended ? "default" : "primary"}
          size="small"
          disabled={!ready || busy}
          title={
            !ready
              ? `Still missing: ${data.missing.join("; ")}`
              : unattended
                ? "Agree to the brief; then ask an agent to run the improvement loop"
                : "Agree to the brief; the agent starts the improvement loop"
          }
          onClick={() => void confirm()}
        >
          <Check size={13} /> {busy ? "Confirming…" : "Confirm brief"}
        </Button>
        <Button variant="ghost" size="small" disabled={busy} onClick={onEdit}>
          Edit
        </Button>
      </div>
      {error && <span className="text-xs text-accent-red wrap-anywhere">{error}</span>}
    </div>
  );
}

function SettingsButton({ onOpen }: { onOpen?: () => void }) {
  if (!onOpen) return null;
  return (
    <Button variant="ghost" size="small" title="Autocompaction, stall alerts, wake-ups, the brief gate and more" onClick={onOpen}>
      <Settings2 size={13} /> Settings
    </Button>
  );
}

function StartCard({ onStart, onOpenSettings }: { onStart: () => void; onOpenSettings?: () => void }) {
  return (
    <div className="flex flex-col gap-2 mb-5 py-3.5 px-4 rounded-md border border-border bg-surface">
      <div className="flex items-center gap-2 text-sm font-semibold text-text">
        <FlaskConical size={15} className="text-accent-blue" />
        Start auto-research
      </div>
      <p className="m-0 text-sm text-subtext leading-relaxed">
        Set the goal, the metric that measures it, the limits and where to start. The agent checks the data and the
        baseline, asks about anything missing, and waits for you to confirm before it runs the improvement loop. Each
        optimization runs in its own chat, so you can run several at once, on the whole system or on parts of it.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="primary" size="small" onClick={onStart}>
          Set up research
        </Button>
        <SettingsButton onOpen={onOpenSettings} />
      </div>
    </div>
  );
}

function BriefPanel({ data, onEdit }: { data: BriefPayload; onEdit: () => void }) {
  const brief = data.brief;
  if (!brief) {
    return (
      <Note>
        No brief yet. The agent writes one with you when it scopes the work; the improvement loop waits until you
        confirm it.
      </Note>
    );
  }
  const budgets = budgetLines(brief, data.usage);
  const field = (label: string, value: string | null) =>
    value && (
      <div className="grid grid-cols-[7.5rem_minmax(0,_1fr)] gap-x-3 py-0.5 text-sm">
        <span className="text-muted">{label}</span>
        <span dir="auto" className="text-text wrap-anywhere">{value}</span>
      </div>
    );
  return (
    <div className="flex flex-col gap-3">
      <div>
        {brief.confirmedAt ? (
          <Chip tone="green">
            <Check size={11} className="me-1" />
            Confirmed {brief.confirmedBy === "user" ? "by you" : "by the agent"} on {new Date(brief.confirmedAt).toLocaleDateString()}
          </Chip>
        ) : (
          <Chip tone="amber">Waiting for your confirmation</Chip>
        )}
        {data.missing.length > 0 && <span className="ms-2 text-xs text-subtext">Still missing: {data.missing.join(", ")}</span>}
        <Button variant="ghost" size="small" className="ms-2" onClick={onEdit}>
          Edit
        </Button>
      </div>
      <div className="flex flex-col">
        {field("Objective", brief.objective)}
        {field("May change", brief.mayChange)}
        {field("Must not change", brief.mustNotChange)}
        {brief.locked.length > 0 && (
          <div className="grid grid-cols-[7.5rem_minmax(0,_1fr)] gap-x-3 py-0.5 text-sm">
            <span className="text-muted">Locked</span>
            <span className="flex flex-wrap gap-1">
              {brief.locked.map((entry) => (
                <Code key={entry.path}>
                  <Lock size={10} className="inline me-1 align-[-1px]" />
                  {entry.path}
                </Code>
              ))}
            </span>
          </div>
        )}
        {brief.scope.length > 0 && (
          <div className="grid grid-cols-[7.5rem_minmax(0,_1fr)] gap-x-3 py-0.5 text-sm">
            <span className="text-muted">Only changes</span>
            <span className="flex flex-wrap gap-1">
              {brief.scope.map((path) => (
                <Code key={path}>{path}</Code>
              ))}
            </span>
          </div>
        )}
        {field("Stop rule", brief.stopRule)}
      </div>
      {budgets.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {budgets.map((line) => (
            <div key={line.label} className="grid grid-cols-[7.5rem_minmax(0,_1fr)_auto] items-center gap-x-3 text-sm">
              <span className="text-muted">{line.label}</span>
              <span className="relative h-1.5 rounded-full bg-hover-subtle overflow-hidden">
                <span className={`absolute inset-y-0 start-0 rounded-full ${BUDGET_TONE[line.tone]}`} style={{ width: `${(100 * line.fraction).toFixed(1)}%` }} />
              </span>
              <span className={`tabular-nums ${line.tone === "alert" ? "text-accent-red" : line.tone === "warn" ? "text-accent-amber" : "text-text"}`}>
                {line.text}
              </span>
            </div>
          ))}
          {!brief.confirmedAt && <div className="text-xs text-muted">The budget starts counting at the first confirmation.</div>}
        </div>
      )}
    </div>
  );
}

function verdictChip(status: string) {
  switch (status) {
    case "supported":
      return <Chip tone="green">supported</Chip>;
    case "refuted":
      return <Chip tone="red">refuted</Chip>;
    case "inconclusive":
      return <Chip tone="amber">inconclusive</Chip>;
    case "testing":
      return <Chip tone="blue">testing</Chip>;
    default:
      return <Chip tone="muted">{status}</Chip>;
  }
}

function FindingsPanel({ rows, onAsk }: { rows: FindingRow[]; onAsk?: Ask }) {
  if (rows.length === 0) {
    return <Note>No findings yet. The agent records what each result says beyond its one idea, with the runs as evidence.</Note>;
  }
  return (
    <div className="flex flex-col">
      <Collapsible
        items={rows}
        noun="findings"
        render={(row) => (
          <div key={row.finding.id} className="flex flex-col gap-0.5 py-1.5 border-b border-b-border-variant last:border-b-0">
            <div className="flex min-w-0 items-start gap-2">
              <span dir="auto" className="min-w-0 flex-1 text-sm text-text">{row.finding.statement}</span>
              {row.finding.locked && <Lock size={12} className="mt-1 shrink-0 text-muted" aria-label="Settled by you" />}
              {verdictChip(row.finding.status)}
              {onAsk && <AskButton label="Ask about this finding" question={askAboutFinding(row.finding)} onAsk={onAsk} />}
            </div>
            <div className="text-xs text-muted tabular-nums">
              {row.finding.kind}
              {` · ${row.supporting} supporting, ${row.against} against`}
              {row.latest?.confidence != null && ` · ${Math.round(100 * row.latest.confidence)}% confident`}
              {row.latest?.scope && ` · holds for ${row.latest.scope}`}
            </div>
          </div>
        )}
      />
    </div>
  );
}

// --- the view ---------------------------------------------------------------------------------

function Loading({ error }: { error: unknown }) {
  return <Note>{error ? "Couldn't load this section." : "Loading…"}</Note>;
}

const SELECT =
  "h-8 min-w-0 max-w-full flex-1 rounded-md border border-border bg-background px-2 font-sans text-sm text-text outline-none focus:border-text";

/** Which optimization the view shows, and a way to start another. */
function OptimizationBar({
  list,
  selectedId,
  activeSessionId,
  onSelect,
  onNew,
  onOpenChat,
  onOpenSettings,
}: {
  list: readonly BriefPayload[];
  selectedId: string | null;
  activeSessionId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onOpenChat?: (sessionId: string) => void;
  onOpenSettings?: () => void;
}) {
  const selected = list.find((entry) => entry.brief?.id === selectedId);
  const chat = selected?.brief?.chatSessionId ?? null;
  const here = (entry: BriefPayload) => activeSessionId !== null && entry.brief?.chatSessionId === activeSessionId;
  return (
    <div className="mb-5 flex flex-wrap items-center gap-2">
      {list.length > 1 ? (
        <select className={SELECT} value={selectedId ?? ""} aria-label="Optimization" onChange={(event) => onSelect(event.target.value)}>
          {list.map((entry) => (
            <option key={entry.brief?.id} value={entry.brief?.id}>
              {optimizationLabel(entry)}
              {here(entry) ? " (this chat)" : ""}
            </option>
          ))}
        </select>
      ) : (
        selected && (
          <span dir="auto" className="min-w-0 flex-1 truncate text-sm font-medium text-text" title={selected.brief?.objective ?? undefined}>
            {optimizationLabel(selected)}
          </span>
        )
      )}
      {chat && onOpenChat && chat !== activeSessionId && (
        <Button variant="ghost" size="small" title="Open the chat that runs this optimization" onClick={() => onOpenChat(chat)}>
          <MessageSquare size={13} /> Open its chat
        </Button>
      )}
      <Button variant="ghost" size="small" title="Start another optimization in its own chat" onClick={onNew}>
        <Plus size={13} /> New optimization
      </Button>
      <SettingsButton onOpen={onOpenSettings} />
    </div>
  );
}

export function ResearchView({
  projectId,
  activeSessionId = null,
  onSideChat,
  onOpenChat,
  onOpenSettings,
}: {
  projectId: string;
  /** The chat open beside the view; its optimization is shown first. */
  activeSessionId?: string | null;
  /** Opens a side chat off a chat, with a question (empty to just open it). */
  onSideChat?: (parentSessionId: string, question: string) => void;
  /** Brings a chat to the front. */
  onOpenChat?: (sessionId: string) => void;
  /** Opens the Research section of the Settings page. */
  onOpenSettings?: () => void;
}) {
  const list = useProjectData(projectId, "vlresearchOptimizations", "optimizations", parseOptimizations);
  const findings = useProjectData(projectId, "vlresearchFindings", "findings", parseFindings);
  const optimizations = useMemo(() => list.data?.optimizations ?? [], [list.data]);
  const [picked, setPicked] = useState<string | null>(null);
  const selectedId =
    picked !== null && optimizations.some((entry) => entry.brief?.id === picked)
      ? picked
      : defaultOptimization(optimizations, activeSessionId);
  const selected = optimizations.find((entry) => entry.brief?.id === selectedId) ?? null;
  const metrics = useOptimizationData(selectedId, "vlresearchMetrics", "metrics", parseMetrics);
  const ideas = useOptimizationData(selectedId, "vlresearchIdeas", "ideas", parseIdeas);
  const [details, setDetails] = useState(true);
  const [setup, setSetup] = useState<"start" | "edit" | null>(null);
  const queued = useMemo(
    () => (ideas.data?.ideas ?? []).filter((idea) => !isBlocker(idea) && (idea.status === "open" || idea.status === "running")),
    [ideas.data],
  );
  if (setup) {
    const editing = setup === "edit" && selected !== null;
    return (
      <div className="vlr-research absolute inset-0 overflow-y-auto bg-background">
        <div className="w-full max-w-230 mx-auto pt-5 px-5 pb-10">
          <ResearchSetup
            projectId={projectId}
            optimizationId={editing ? selectedId : null}
            initial={editing && selected ? draftFromBrief(selected) : emptyDraft()}
            editing={editing}
            startable={!editing || !selected?.brief?.chatSessionId}
            keys={list.data?.keys ?? []}
            queued={editing ? queued : []}
            onClose={() => setSetup(null)}
            onSaved={setPicked}
          />
        </div>
      </div>
    );
  }
  const unconfirmed = selected?.brief && !selected.brief.confirmedAt ? selected : null;
  // Side chats branch off the chat that runs the loop.
  const loopChat = selected?.brief?.chatSessionId ?? null;
  const ask: Ask | undefined = loopChat && onSideChat ? (question) => onSideChat(loopChat, question) : undefined;
  return (
    <div className="vlr-research absolute inset-0 overflow-y-auto bg-background">
      <div className="w-full max-w-230 mx-auto pt-5 px-5 pb-10">
        {!list.data && <Loading error={list.error} />}
        {list.data && optimizations.length === 0 && <StartCard onStart={() => setSetup("start")} onOpenSettings={onOpenSettings} />}
        {optimizations.length > 0 && (
          <OptimizationBar
            list={optimizations}
            selectedId={selectedId}
            activeSessionId={activeSessionId}
            onSelect={setPicked}
            onNew={() => setSetup("start")}
            onOpenChat={onOpenChat}
            onOpenSettings={onOpenSettings}
          />
        )}
        {unconfirmed && (
          <div className="flex flex-col gap-2 mb-5 py-3 px-4 rounded-md border border-accent-amber bg-accent-amber-subtle">
            <div className="text-sm text-text">
              <span className="font-medium">The brief is waiting for your confirmation.</span>{" "}
              {unconfirmed.missing.length > 0
                ? `It still needs ${unconfirmed.missing.join("; ")}.`
                : unconfirmed.brief?.chatSessionId
                  ? "Review it below; confirming starts the improvement loop."
                  : "No agent is working on it yet: start one to check the data and the baseline with you before the loop runs."}
            </div>
            <ConfirmBar data={unconfirmed} onEdit={() => setSetup("edit")} />
          </div>
        )}
        {selected && (
          <>
            <div className="mb-3 flex items-center justify-between gap-2">
              <h2 className="m-0 flex items-center gap-1.5 text-sm font-semibold text-text [&_svg]:text-muted">
                <Target size={14} />
                Optimization
              </h2>
              {ask && (
                <Button variant="ghost" size="small" title="Ask about the optimization in a side chat; the loop keeps running" onClick={() => ask("")}>
                  <MessagesSquare size={13} /> Side chat
                </Button>
              )}
            </div>
            {metrics.data ? <Headline metrics={metrics.data} /> : <Loading error={metrics.error} />}
            {metrics.data?.primary && (
              <>
                <div className="mt-4">
                  <RunningBest metrics={metrics.data} />
                </div>
                <button
                  type="button"
                  className="mt-4 mb-2 flex items-center gap-1 p-0 border-0 bg-transparent text-sm font-medium text-text cursor-pointer"
                  aria-expanded={details}
                  onClick={() => setDetails((open) => !open)}
                >
                  {details ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                  What moved the score
                </button>
                {details && <Waterfall metrics={metrics.data} onAsk={ask} />}
              </>
            )}
            <Section icon={<Lightbulb size={14} />} title="Ideas">
              {ideas.data ? <IdeasPanel ideas={ideas.data} onAsk={ask} /> : <Loading error={ideas.error} />}
            </Section>
            <Section icon={<ClipboardList size={14} />} title="Brief">
              <BriefPanel data={selected} onEdit={() => setSetup("edit")} />
            </Section>
          </>
        )}
        <Section icon={<BookOpen size={14} />} title="Findings">
          {findings.data ? <FindingsPanel rows={findings.data} onAsk={ask} /> : <Loading error={findings.error} />}
        </Section>
      </div>
    </div>
  );
}
