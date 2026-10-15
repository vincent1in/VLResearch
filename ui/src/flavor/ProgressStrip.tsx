// VLResearch: the progress strip docked above the composer. It answers "is the
// agent still doing anything?" without reading the transcript: the agent's own
// checklist, how long since its last update, and the runs this chat will be
// woken for, each with how long such runs usually take.
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, ChevronRight, Clock, FlaskConical, ListChecks, TriangleAlert, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { ChatMessage } from "../api";
import { workspaceKey } from "../queries/client";
import { tabOpenGestureHandlers, type TabOpenIntent } from "../tabPreview";
import { checklistCounts, currentItem, latestChecklist, type ChecklistItem } from "./checklist";
import {
  activityLine,
  hasLiveWork,
  parseSessionProgress,
  waitingLine,
  type SessionProgress,
  type Tone,
} from "./progress";

const LIVE_POLL_MS = 5_000;
const IDLE_POLL_MS = 30_000;

async function fetchSessionProgress(sessionId: string, signal: AbortSignal): Promise<SessionProgress> {
  const response = await fetch(`/api/vlresearch/sessions/${encodeURIComponent(sessionId)}/progress`, { signal });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return parseSessionProgress(await response.json());
}

function toneClass(tone: Tone): string {
  if (tone === "alert") return "text-accent-red";
  if (tone === "warn") return "text-accent-amber";
  return "";
}

function ItemIcon({ item }: { item: ChecklistItem }) {
  switch (item.status) {
    case "completed":
      return <Check size={12} className="shrink-0 text-accent-green" />;
    case "in_progress":
      return <ChevronRight size={12} className="shrink-0 text-accent-blue" />;
    case "cancelled":
      return <X size={12} className="shrink-0 text-muted" />;
    default:
      return (
        <span className="flex size-3 shrink-0 items-center justify-center">
          <span className="size-1.5 rounded-full bg-muted" />
        </span>
      );
  }
}

export function ProgressStrip({
  sessionId,
  messages,
  busy,
  toolRunning,
  onOpenRun,
}: {
  sessionId: string;
  /** The transcript on screen (active branch). */
  messages: readonly ChatMessage[];
  /** A turn is running and not waiting on the user. */
  busy: boolean;
  /** The turn's newest part is a tool call still in flight. */
  toolRunning: boolean;
  onOpenRun?: (runId: string, intent: TabOpenIntent) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const progress = useQuery({
    queryKey: workspaceKey("vlresearchProgress", sessionId),
    queryFn: ({ signal }) => fetchSessionProgress(sessionId, signal),
    refetchInterval: (query) => (hasLiveWork(busy, query.state.data) ? LIVE_POLL_MS : IDLE_POLL_MS),
    retry: false,
  });
  const waiting = progress.data?.waiting ?? [];
  const live = hasLiveWork(busy, progress.data);

  useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [live]);

  const checklist = useMemo(() => latestChecklist(messages), [messages]);
  const counts = checklist ? checklistCounts(checklist) : null;
  const current = checklist ? currentItem(checklist) : null;
  // A checklist is progress only while work is in flight; afterwards the
  // transcript keeps the record and the strip gets out of the way.
  const showChecklist = live && checklist !== null && counts !== null && counts.total > 0;
  const activity = busy
    ? activityLine({ lastActivityAt: progress.data?.lastActivityAt ?? null, toolRunning, now })
    : null;

  if (!showChecklist && !activity && waiting.length === 0) return null;

  const percent = counts && counts.total > 0 ? Math.round((100 * counts.done) / counts.total) : 0;

  return (
    <div className="vlr-progress flex flex-col gap-1 mb-1.5 py-1.5 px-2.5 text-sm text-subtext bg-background border border-border rounded-sm">
      {showChecklist && checklist && counts && (
        <div className="flex flex-col">
          <button
            type="button"
            className="flex w-full min-w-0 items-center gap-2 p-0 border-0 bg-transparent text-start text-subtext cursor-pointer"
            aria-expanded={expanded}
            title={expanded ? "Hide the checklist" : "Show the checklist"}
            onClick={() => setExpanded((open) => !open)}
          >
            <ListChecks size={13} className="shrink-0 text-muted" />
            <span className="shrink-0 text-text">
              {counts.done} of {counts.total} done
            </span>
            {current && (
              <span dir="auto" className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap">
                {current.text}
              </span>
            )}
            <span
              className="ms-auto h-1 w-16 shrink-0 overflow-hidden rounded-full bg-border"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={counts.total}
              aria-valuenow={counts.done}
            >
              <span className="block h-full bg-accent-green" style={{ width: `${percent}%` }} />
            </span>
            {expanded ? <ChevronDown size={13} className="shrink-0" /> : <ChevronRight size={13} className="shrink-0" />}
          </button>
          {expanded && (
            <ol className="mt-1 ms-5 flex flex-col gap-0.5">
              {checklist.map((item, index) => (
                <li key={index} className="flex items-center gap-1.5 min-w-0">
                  <ItemIcon item={item} />
                  <span
                    dir="auto"
                    className={`min-w-0 overflow-hidden text-ellipsis whitespace-nowrap ${
                      item.status === "cancelled" ? "line-through text-muted" : item.status === "in_progress" ? "text-text" : ""
                    }`}
                  >
                    {item.text}
                  </span>
                </li>
              ))}
            </ol>
          )}
        </div>
      )}
      {activity && (
        <div className={`flex items-center gap-2 min-w-0 ${toneClass(activity.tone)}`}>
          {activity.tone === "normal"
            ? <Clock size={13} className="shrink-0 text-muted" />
            : <TriangleAlert size={13} className="shrink-0" />}
          <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">{activity.text}</span>
        </div>
      )}
      {waiting.map((run) => {
        const line = waitingLine(run, now);
        const icon = line.tone === "normal"
          ? <FlaskConical size={13} className="shrink-0 text-muted" />
          : <TriangleAlert size={13} className="shrink-0" />;
        const text = (
          <span dir="auto" className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">
            {line.text}
          </span>
        );
        return (
          <div key={run.runId} className="flex flex-col min-w-0">
            {onOpenRun ? (
              <button
                type="button"
                className={`flex w-full min-w-0 items-center gap-2 p-0 border-0 bg-transparent text-start cursor-pointer [&:hover]:underline ${toneClass(line.tone) || "text-subtext"}`}
                title="Open the run's log"
                {...tabOpenGestureHandlers<HTMLButtonElement>((intent) => onOpenRun(run.runId, intent))}
              >
                {icon}
                {text}
              </button>
            ) : (
              <div className={`flex min-w-0 items-center gap-2 ${toneClass(line.tone)}`}>
                {icon}
                {text}
              </div>
            )}
            {run.alerts?.map((alert) => (
              <div key={`${alert.kind}-${alert.at}`} dir="auto" className="ms-5 text-accent-red">
                {alert.message}
              </div>
            ))}
          </div>
        );
      })}
    </div>
  );
}
