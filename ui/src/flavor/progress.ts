// VLResearch: what the chat's progress strip says. Kept free of React and the
// message catalog so node's test runner can import it directly
// (ui/tests/vlresearchProgress.test.mjs).

/** A run this chat will be woken for, from /api/vlresearch/sessions/{id}/progress. */
export interface WaitingRun {
  runId: string;
  experimentId: string;
  /** The experiment's title, else its slug; null when it has neither. */
  experiment: string | null;
  status: string;
  /** Unix millis. */
  startedAt: number;
  /** Median duration of comparable finished runs, once there are three. */
  typicalMs: number | null;
  /** Unix millis of the run log's last write; null before any output. */
  lastOutputAt: number | null;
  /** Problems orx noticed while the run is still going (stall alerts). */
  alerts?: RunAlert[];
}

export interface RunAlert {
  kind: string;
  message: string;
  /** Unix millis. */
  at: number;
}

export interface SessionProgress {
  /** Unix millis of the chat's latest streamed update since `orx up` started. */
  lastActivityAt: number | null;
  waiting: WaitingRun[];
}

export type Tone = "normal" | "warn" | "alert";

export interface StripLine {
  text: string;
  tone: Tone;
}

/** A busy chat with no update for this long gets an amber clock... */
export const QUIET_WARN_MS = 5 * 60_000;
/** ...and this long reads as possibly stuck. */
export const QUIET_ALERT_MS = 15 * 60_000;
/** A tool call may legitimately run for a while (Claude Code allows 10 minutes). */
export const TOOL_QUIET_WARN_MS = 15 * 60_000;
/** A running job with a silent log for this long is worth a look. */
export const OUTPUT_QUIET_WARN_MS = 10 * 60_000;
/** Runs past this multiple of their usual duration are flagged. */
export const OVERDUE_FACTOR = 2;

/** "45s", "12m", "1h 05m", "3d 4h": coarse on purpose, since the strip ticks. */
export function formatSpan(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${String(minutes % 60).padStart(2, "0")}m`;
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h`;
}

/** The "Working" row for a busy chat, or null when there is no clock to read. */
export function activityLine({
  lastActivityAt,
  toolRunning,
  now,
}: {
  lastActivityAt: number | null;
  toolRunning: boolean;
  now: number;
}): StripLine | null {
  if (lastActivityAt === null) return null;
  const quiet = Math.max(0, now - lastActivityAt);
  if (quiet < 10_000) return { text: "Working", tone: "normal" };
  const text = `Working · last update ${formatSpan(quiet)} ago`;
  if (toolRunning) {
    return quiet >= TOOL_QUIET_WARN_MS
      ? { text: `${text} · a tool call is still running`, tone: "warn" }
      : { text, tone: "normal" };
  }
  if (quiet >= QUIET_ALERT_MS) return { text: `${text} · may be stuck`, tone: "alert" };
  if (quiet >= QUIET_WARN_MS) return { text, tone: "warn" };
  return { text, tone: "normal" };
}

/** One row per run the chat is waiting on. */
export function waitingLine(run: WaitingRun, now: number): StripLine {
  const name = run.experiment?.trim() || "a run";
  const elapsed = Math.max(0, now - run.startedAt);
  if (run.status === "starting") {
    return { text: `Waiting on ${name} · starting, ${formatSpan(elapsed)} so far`, tone: "normal" };
  }
  const notes = [`running ${formatSpan(elapsed)}`];
  let tone: Tone = "normal";
  if (run.typicalMs !== null && run.typicalMs > 0) {
    notes.push(`usually about ${formatSpan(run.typicalMs)}`);
    if (elapsed > OVERDUE_FACTOR * run.typicalMs) tone = "warn";
  }
  const quietOutput = run.lastOutputAt === null ? null : Math.max(0, now - run.lastOutputAt);
  if (quietOutput !== null && quietOutput >= OUTPUT_QUIET_WARN_MS) {
    notes.push(`no output for ${formatSpan(quietOutput)}`);
    tone = "warn";
  }
  if (run.alerts && run.alerts.length > 0) tone = "alert";
  return { text: `Waiting on ${name} · ${notes.join(" · ")}`, tone };
}

/** Whether the strip has anything to show for this chat right now. */
export function hasLiveWork(busy: boolean, progress: SessionProgress | undefined): boolean {
  return busy || (progress?.waiting.length ?? 0) > 0;
}

/** Reads the endpoint's JSON defensively: a malformed field drops, not the strip. */
export function parseSessionProgress(value: unknown): SessionProgress {
  const record = typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
  const lastActivityAt = typeof record.lastActivityAt === "number" ? record.lastActivityAt : null;
  const waiting: WaitingRun[] = [];
  for (const item of Array.isArray(record.waiting) ? record.waiting : []) {
    if (typeof item !== "object" || item === null) continue;
    const run = item as Record<string, unknown>;
    if (typeof run.runId !== "string" || typeof run.startedAt !== "number") continue;
    const alerts: RunAlert[] = [];
    for (const alert of Array.isArray(run.alerts) ? run.alerts : []) {
      if (typeof alert !== "object" || alert === null) continue;
      const fields = alert as Record<string, unknown>;
      if (typeof fields.message !== "string") continue;
      alerts.push({
        kind: typeof fields.kind === "string" ? fields.kind : "notice",
        message: fields.message,
        at: typeof fields.at === "number" ? fields.at : 0,
      });
    }
    waiting.push({
      runId: run.runId,
      experimentId: typeof run.experimentId === "string" ? run.experimentId : "",
      experiment: typeof run.experiment === "string" ? run.experiment : null,
      status: typeof run.status === "string" ? run.status : "running",
      startedAt: run.startedAt,
      typicalMs: typeof run.typicalMs === "number" ? run.typicalMs : null,
      lastOutputAt: typeof run.lastOutputAt === "number" ? run.lastOutputAt : null,
      alerts,
    });
  }
  return { lastActivityAt, waiting };
}
