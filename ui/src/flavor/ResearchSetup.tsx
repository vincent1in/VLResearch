// VLResearch: the form that starts an optimization from the Research view. It
// saves the brief (what to improve and how it is measured, the limits and
// scope, the budget, where to start) and the seed ideas, then opens the chat
// that runs it, which scopes the rest with the user and waits for their
// confirmation before the loop. It edits an optimization's brief the same way.
import { useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { Plus, X } from "lucide-react";
import { useMemo, useRef, useState, type ReactNode } from "react";
import { createChatSession, sendChatMessage, type Harness, type HarnessId } from "../api";
import { Button, Input } from "../components/ui";
import { workspaceKey } from "../queries/client";
import { getUiStateQuery } from "../queries/projects";
import { getHarnessesQuery } from "../queries/settings";
import { taskLocation } from "../workspaceState";
import {
  briefPayload,
  ideaPayload,
  parseBrief,
  kickoffMessage,
  setupProblems,
  withoutIdeas,
  worseningIdeas,
  type Idea,
  type SetupDraft,
  type SetupProblems,
} from "./research";

const TEXTAREA =
  "w-full min-h-16 rounded-md border border-border bg-background px-2.5 py-1.5 font-sans text-sm text-text outline-none placeholder:text-muted focus:border-text resize-y";
const SELECT =
  "h-8 w-full rounded-md border border-border bg-background px-2 font-sans text-sm text-text outline-none focus:border-text";

/** Refetches everything the Research view shows about optimizations. */
export function refreshResearch(queryClient: QueryClient) {
  return Promise.all(
    ["vlresearchOptimizations", "vlresearchIdeas", "vlresearchMetrics"].map((family) =>
      queryClient.invalidateQueries({ queryKey: workspaceKey(family) }),
    ),
  );
}

async function write(method: "PUT" | "POST", url: string, body?: unknown): Promise<unknown> {
  const response = await fetch(url, {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) throw new Error((await response.text()) || `HTTP ${response.status}`);
  return response.json();
}

function Step({ number, title, hint, children }: { number: number; title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="mt-5 pt-4 border-t border-t-border-variant first:mt-0 first:pt-0 first:border-t-0">
      <h3 className="m-0 flex items-baseline gap-2 text-sm font-semibold text-text">
        <span className="inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-hover-subtle text-xs text-subtext">{number}</span>
        {title}
      </h3>
      {hint && <p className="mt-1 mb-0 ms-7 text-xs text-muted leading-relaxed">{hint}</p>}
      <div className="mt-3 ms-7 flex flex-col gap-3">{children}</div>
    </section>
  );
}

function Field({ label, hint, error, children }: { label: string; hint?: string; error?: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-xs font-medium text-subtext">{label}</span>
      {children}
      {error ? <span className="text-xs text-accent-red">{error}</span> : hint && <span className="text-xs text-muted">{hint}</span>}
    </label>
  );
}

/** Agents that can take a chat now. */
function readyAgents(harnesses: readonly Harness[] | undefined): Harness[] {
  return (harnesses ?? []).filter((harness) => harness.installed && harness.agentReady);
}

export function ResearchSetup({
  projectId,
  optimizationId,
  initial,
  editing,
  startable,
  keys,
  queued,
  onClose,
  onSaved,
}: {
  projectId: string;
  /** The optimization being edited; null to start a new one. */
  optimizationId: string | null;
  initial: SetupDraft;
  /** The project already has a brief; saving changes it and reopens it. */
  editing: boolean;
  /** No chat is working on the brief yet, so the form can start one. */
  startable: boolean;
  /** Metric keys runs have reported, to choose from. */
  keys: readonly string[];
  /** Ideas already queued, which the form shows but doesn't change. */
  queued: readonly Idea[];
  onClose: () => void;
  /** Told which optimization was saved, so the view can show it. */
  onSaved: (optimizationId: string) => void;
}) {
  const [draft, setDraft] = useState(initial);
  // Problems show once the user has tried to save, and clear as they're fixed.
  const [tried, setTried] = useState(false);
  const problems: SetupProblems = tried ? setupProblems(draft) : {};
  const worsening = worseningIdeas(draft);
  const metricName = draft.metricKey.trim() || "the metric";
  const improvementSign = draft.direction === "max" ? "Higher is better, so an improvement is positive." : "Lower is better, so an improvement is negative.";
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // What an attempt that failed later on already made, reused by the retry:
  // the optimization it started, and its chat.
  const created = useRef<string | null>(optimizationId);
  const started = useRef<string | null>(null);
  const queryClient = useQueryClient();
  const router = useRouter();
  const harnesses = useQuery(getHarnessesQuery());
  const uiState = useQuery(getUiStateQuery());
  const agents = useMemo(() => readyAgents(harnesses.data), [harnesses.data]);
  const preferred = uiState.data?.preferredAgent ?? null;
  const [agent, setAgent] = useState<HarnessId | null>(null);
  const chosen = agent ?? (preferred && agents.some((harness) => harness.id === preferred.harness) ? preferred.harness : agents[0]?.id ?? null);

  const update = <K extends keyof SetupDraft>(key: K, value: SetupDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));
  const updateIdea = (index: number, change: Partial<SetupDraft["ideas"][number]>) =>
    setDraft((current) => ({
      ...current,
      ideas: current.ideas.map((idea, at) => (at === index ? { ...idea, ...change } : idea)),
    }));

  const refresh = () => refreshResearch(queryClient);

  const save = async (start: boolean) => {
    setTried(true);
    setError(null);
    if (Object.keys(setupProblems(draft)).length > 0) return;
    if (start && !chosen) {
      setError("No coding agent is ready to start the chat. Set one up in Settings, or save the brief for now.");
      return;
    }
    setBusy(true);
    // Rows already queued by this attempt leave the form even if a later step
    // fails, so retrying doesn't queue them twice.
    const saved = new Set<number>();
    try {
      let id = created.current;
      if (id === null) {
        const made = parseBrief(await write("POST", `/api/vlresearch/projects/${encodeURIComponent(projectId)}/optimizations`, briefPayload(draft)));
        id = made.brief?.id ?? null;
        if (!id) throw new Error("The new optimization came back without an id.");
        created.current = id;
      } else {
        await write("PUT", `/api/vlresearch/optimizations/${encodeURIComponent(id)}/brief`, briefPayload(draft));
      }
      const base = `/api/vlresearch/optimizations/${encodeURIComponent(id)}`;
      for (const [index, idea] of draft.ideas.entries()) {
        const body = ideaPayload(idea);
        if (!body) continue;
        await write("POST", `${base}/ideas`, body);
        saved.add(index);
      }
      if (start && chosen) {
        const sameAgent = preferred?.harness === chosen;
        started.current ??= (
          await createChatSession(projectId, chosen, {
            model: sameAgent ? preferred?.model ?? null : null,
            serviceTier: sameAgent ? preferred?.serviceTier ?? null : null,
            permissionMode: sameAgent ? preferred?.permissionMode ?? null : null,
            reasoningLevel: sameAgent ? preferred?.reasoningLevel ?? null : null,
          })
        ).id;
        const sessionId = started.current;
        await write("PUT", `${base}/brief/chat`, { sessionId });
        await sendChatMessage(sessionId, kickoffMessage(projectId, id, queued.length + saved.size));
        await refresh();
        onSaved(id);
        const pane = new URLSearchParams(window.location.search).get("pane");
        void router.navigate({ href: `${taskLocation(projectId, sessionId)}${pane ? `?${new URLSearchParams({ pane })}` : ""}` });
      } else {
        await refresh();
        onSaved(id);
      }
      onClose();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      await refresh();
    } finally {
      if (saved.size > 0) setDraft((current) => withoutIdeas(current, saved));
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="m-0 text-base font-semibold text-text">{editing ? "Edit the brief" : "Start an optimization"}</h2>
          <p className="mt-1 mb-0 text-sm text-subtext leading-relaxed">
            {!editing
              ? "Say what to improve and where to start. It runs in its own chat: the agent checks the data and the baseline, asks about anything missing, and waits for you to confirm before it spends compute."
              : startable
                ? "Saving reopens the brief until you confirm it again. Save and start also opens a chat in which the agent checks the data and the baseline and asks about anything missing."
                : "Saving changes reopens the brief: the loop waits until you confirm it again."}
          </p>
        </div>
        <Button variant="ghost" size="small" onClick={onClose} disabled={busy} aria-label="Close without saving">
          <X size={14} />
        </Button>
      </div>

      <fieldset disabled={busy} className="mt-5 mx-0 mb-0 p-0 border-0 min-w-0">
        <Step number={1} title="What to improve">
          <Field label="Goal" error={problems.objective}>
            <textarea
              className={TEXTAREA}
              rows={2}
              value={draft.objective}
              placeholder="Raise the validation IC of the daily return forecast"
              onChange={(event) => update("objective", event.target.value)}
            />
          </Field>
          <Field
            label="Metric"
            hint={keys.length > 0 ? `Runs report ${keys.slice(0, 6).join(", ")}${keys.length > 6 ? "…" : ""}.` : "As the run code reports it in ORX_METRIC lines; the validation one."}
            error={problems.metricKey}
          >
            <div className="flex flex-wrap items-center gap-2">
              <Input className="w-auto min-w-32 flex-1" list="vlr-metric-keys" value={draft.metricKey} placeholder="val_ic" onChange={(event) => update("metricKey", event.target.value)} />
              <div
                className="seg inline-flex items-center gap-0.5 rounded-md bg-hover-subtle p-0.5 [&_button]:py-0.5 [&_button]:px-2 [&_button]:text-sm [&_button]:font-medium [&_button]:text-text [&_button]:rounded-sm [&_button.active]:bg-background [&_button.active]:shadow-segment"
                role="group"
                aria-label="Which way is better"
              >
                <button type="button" className={draft.direction === "max" ? "active" : ""} aria-pressed={draft.direction === "max"} onClick={() => update("direction", "max")}>
                  Higher is better
                </button>
                <button type="button" className={draft.direction === "min" ? "active" : ""} aria-pressed={draft.direction === "min"} onClick={() => update("direction", "min")}>
                  Lower is better
                </button>
              </div>
            </div>
            <datalist id="vlr-metric-keys">
              {keys.map((key) => (
                <option key={key} value={key} />
              ))}
            </datalist>
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Split (optional)" hint="When runs report the metric under several splits.">
              <Input value={draft.split} placeholder="val" onChange={(event) => update("split", event.target.value)} />
            </Field>
            <Field label="Noise floor (optional)" hint="Left empty, the agent measures it from three seeds." error={problems.noiseFloor}>
              <Input inputMode="decimal" value={draft.noiseFloor} placeholder="0.002" onChange={(event) => update("noiseFloor", event.target.value)} />
            </Field>
          </div>
        </Step>

        <Step number={2} title="Limits" hint="What the agent may change, and what defines the score and must stay put.">
          <Field label="May change">
            <textarea className={TEXTAREA} rows={2} value={draft.mayChange} placeholder="Feature code, model code, data loaders" onChange={(event) => update("mayChange", event.target.value)} />
          </Field>
          <Field label="Must not change">
            <textarea className={TEXTAREA} rows={2} value={draft.mustNotChange} placeholder="Evaluation code, split dates, the out-of-sample window" onChange={(event) => update("mustNotChange", event.target.value)} />
          </Field>
          <Field label="Locked files and folders" hint="One per line, from the repository root. A run whose commit changes one is refused.">
            <textarea className={`${TEXTAREA} font-mono`} rows={2} value={draft.locked} placeholder={"eval/score.py\ndata/splits.yaml"} onChange={(event) => update("locked", event.target.value)} />
          </Field>
          <Field
            label="Only change (optional)"
            hint="To work on one part of the system: runs may change only files inside these paths, one per line. Leave empty for the whole repository."
          >
            <textarea className={`${TEXTAREA} font-mono`} rows={2} value={draft.scope} placeholder={"features/\ndata/loaders.py"} onChange={(event) => update("scope", event.target.value)} />
          </Field>
        </Step>

        <Step number={3} title="Budget and stop rule">
          <div className="grid grid-cols-3 gap-3">
            <Field label="Runs">
              <Input inputMode="numeric" value={draft.budgetRuns} placeholder="20" onChange={(event) => update("budgetRuns", event.target.value)} />
            </Field>
            <Field label="GPU-hours">
              <Input inputMode="decimal" value={draft.budgetGpuHours} placeholder="40" onChange={(event) => update("budgetGpuHours", event.target.value)} />
            </Field>
            <Field label="Days">
              <Input inputMode="decimal" value={draft.budgetDays} placeholder="14" onChange={(event) => update("budgetDays", event.target.value)} />
            </Field>
          </div>
          {problems.budget && <span className="-mt-2 text-xs text-accent-red">{problems.budget}</span>}
          <Field label="Stop when">
            <Input value={draft.stopRule} placeholder="No validation gain in 5 runs, or the budget is spent" onChange={(event) => update("stopRule", event.target.value)} />
          </Field>
        </Step>

        <Step number={4} title="Where to start" hint="Your pointers and seed ideas are the loop's first ideas; the agent adds its own while scoping.">
          <Field label="Pointers">
            <textarea className={TEXTAREA} rows={2} value={draft.pointers} placeholder="Look at the volume features first" onChange={(event) => update("pointers", event.target.value)} />
          </Field>
          <Field label="Sources">
            <textarea className={TEXTAREA} rows={2} value={draft.sources} placeholder="Datasets on the compute host, earlier experiments, papers" onChange={(event) => update("sources", event.target.value)} />
          </Field>
          <div className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-subtext">Seed ideas</span>
            {queued.length > 0 && (
              <ul className="m-0 mb-1 p-0 list-none flex flex-col gap-0.5">
                {queued.map((idea) => (
                  <li key={idea.id} dir="auto" className="text-sm text-subtext">
                    {idea.title} <span className="text-xs text-muted">(queued)</span>
                  </li>
                ))}
              </ul>
            )}
            {draft.ideas.map((idea, index) => (
              <div key={index} className="grid grid-cols-[minmax(0,_1fr)_6.5rem_auto] items-center gap-2">
                <Input value={idea.title} placeholder="Volume features, 20-day window" aria-label="Idea" onChange={(event) => updateIdea(index, { title: event.target.value })} />
                <Input
                  inputMode="decimal"
                  value={idea.forecast}
                  placeholder={draft.direction === "max" ? "+0.003" : "−0.003"}
                  aria-label="Forecast change"
                  title="The change you expect in the metric, in its units and sign"
                  onChange={(event) => updateIdea(index, { forecast: event.target.value })}
                />
                <Button
                  variant="ghost"
                  size="small"
                  aria-label="Remove this idea"
                  disabled={draft.ideas.length === 1 && !idea.title && !idea.forecast}
                  onClick={() => setDraft((current) => ({ ...current, ideas: current.ideas.length === 1 ? [{ title: "", forecast: "" }] : current.ideas.filter((_, at) => at !== index) }))}
                >
                  <X size={13} />
                </Button>
              </div>
            ))}
            {problems.ideas ? (
              <span className="text-xs text-accent-red">{problems.ideas}</span>
            ) : worsening.length > 0 ? (
              <span dir="auto" className="text-xs text-accent-amber">
                {`Forecast to make ${metricName} worse: ${worsening.join(", ")}. ${improvementSign}`}
              </span>
            ) : (
              <span className="text-xs text-muted">{`Forecast: the change you expect in ${metricName}, with its sign. ${improvementSign}`}</span>
            )}
            <div>
              <Button variant="ghost" size="small" onClick={() => setDraft((current) => ({ ...current, ideas: [...current.ideas, { title: "", forecast: "" }] }))}>
                <Plus size={13} /> Add an idea
              </Button>
            </div>
          </div>
        </Step>

        {startable && (
          <Step number={5} title="Agent" hint="It starts in a new chat, reads this brief, and asks only about what's missing.">
            {agents.length > 0 ? (
              <select
                className={SELECT}
                value={chosen ?? ""}
                aria-label="Coding agent"
                onChange={(event) => {
                  setAgent(event.target.value as HarnessId);
                  started.current = null;
                }}
              >
                {agents.map((harness) => (
                  <option key={harness.id} value={harness.id}>
                    {harness.name}
                  </option>
                ))}
              </select>
            ) : (
              <p className="m-0 text-sm text-subtext">
                {harnesses.isPending ? "Looking for coding agents…" : "No coding agent is ready. Set one up in Settings, or save the brief and start a chat later."}
              </p>
            )}
          </Step>
        )}
      </fieldset>

      {error && <p className="mt-4 mb-0 py-2 px-3 rounded-md bg-accent-red-subtle text-sm text-accent-red wrap-anywhere">{error}</p>}
      <div className="mt-5 flex flex-wrap items-center gap-2">
        {startable && (
          <Button variant="primary" disabled={busy || !chosen} onClick={() => void save(true)}>
            {busy ? "Starting…" : "Save and start"}
          </Button>
        )}
        <Button variant={startable ? "default" : "primary"} disabled={busy} onClick={() => void save(false)}>
          {editing ? (busy ? "Saving…" : "Save changes") : "Save brief only"}
        </Button>
        <Button variant="ghost" disabled={busy} onClick={onClose}>
          Cancel
        </Button>
      </div>
    </div>
  );
}
