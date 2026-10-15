// VLResearch: the Research section of the Settings page. How the research
// loop behaves on this machine, for every project: the brief gate,
// replicates, ablation checks, autocompaction, wake-ups, stall alerts and the
// agent's checklist. Each is a variable orx also reads from its environment, which
// wins; the section says when it does. The network policy is shown here but
// changes only in that environment.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment, useEffect, useState, type ReactNode } from "react";
import { Input, LoadingRow, Spinner, Switch } from "../components/ui";
import { cn } from "../components/ui/cn";
import { workspaceKey } from "../queries/client";
import {
  COMPACT_DEFAULT,
  COMPACT_MAX,
  COMPACT_MIN,
  GATE_OPTIONS,
  SETTING,
  STALL_DEFAULT,
  STALL_MAX,
  compactPercent,
  gateDescription,
  isOn,
  minutesSetting,
  parseSettings,
  percentSetting,
  pinnedNote,
  policyRows,
  settingOf,
  stallMinutes,
  switchedOn,
  type ResearchSettings as Settings,
  type SettingEntry,
  type Typed,
} from "./settings";

const SETTINGS_URL = "/api/vlresearch/settings";

async function request(init?: RequestInit): Promise<Settings> {
  const response = await fetch(SETTINGS_URL, init);
  if (!response.ok) throw new Error((await response.text()) || `HTTP ${response.status}`);
  return parseSettings(await response.json());
}

/** A whole number with its unit, saved when the field loses focus or on Enter. */
function NumberField({
  shown,
  unit,
  label,
  min,
  max,
  disabled,
  parse,
  onSave,
  onInvalid,
}: {
  shown: string;
  unit: string;
  label: string;
  min: number;
  max: number;
  disabled: boolean;
  parse: (typed: string) => Typed;
  onSave: (value: string) => void;
  onInvalid: (problem: string | null) => void;
}) {
  const [typed, setTyped] = useState(shown);
  const [invalid, setInvalid] = useState(false);
  useEffect(() => setTyped(shown), [shown]);
  const commit = () => {
    const unchanged = typed.trim() === shown;
    const result: Typed = unchanged ? { value: shown } : parse(typed);
    setInvalid("error" in result);
    onInvalid("error" in result ? result.error : null);
    if (!unchanged && "value" in result) onSave(result.value);
  };
  return (
    <label className="flex items-center gap-1.5 text-sm text-subtext">
      <Input
        className="w-16 text-end"
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        aria-label={label}
        aria-invalid={invalid}
        value={typed}
        disabled={disabled}
        onChange={(event) => setTyped(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit();
        }}
      />
      {unit}
    </label>
  );
}

function Row({
  className,
  title,
  description,
  entry,
  children,
}: {
  className: string;
  title: string;
  description: ReactNode;
  entry: SettingEntry | undefined;
  children: ReactNode;
}) {
  const note = entry ? pinnedNote(entry) : null;
  return (
    <div className={className}>
      <div className="min-w-0">
        <div className="project-default-title text-base font-medium">{title}</div>
        <p>{description}</p>
        {note && <div className="mt-1 text-sm leading-relaxed text-accent-amber">{note}</div>}
      </div>
      <div className="flex flex-none items-center gap-2.5">{children}</div>
    </div>
  );
}

export function ResearchSettings({ cardClassName, rowClassName }: { cardClassName: string; rowClassName: string }) {
  const queryClient = useQueryClient();
  const queryKey = workspaceKey("vlresearchSettings");
  const query = useQuery({ queryKey, queryFn: ({ signal }) => request({ signal }), retry: false });
  const [saving, setSaving] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const settings = query.data;

  const save = (entry: SettingEntry, value: string) => {
    setSaving(true);
    setProblem(null);
    // Choosing the default drops the saved value, so a later default applies.
    const body = { [entry.key]: value === entry.default ? null : value };
    void request({ method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
      .then((next) => queryClient.setQueryData(queryKey, next))
      .catch((failure) => setProblem(failure instanceof Error ? failure.message : String(failure)))
      .finally(() => setSaving(false));
  };

  const heading = (
    <>
      <h2>Research</h2>
      <p className="mt-0 mb-0 text-sm leading-relaxed text-subtext">
        How the research loop runs on this machine, for every project. A variable set in the environment orx starts in
        wins over these.
      </p>
    </>
  );
  if (!settings) {
    return (
      <>
        {heading}
        {query.error ? <div className="error">{query.error.message}</div> : <LoadingRow><Spinner /> Loading</LoadingRow>}
      </>
    );
  }

  const entry = (key: string) => settingOf(settings, key);
  const locked = (current: SettingEntry | undefined) => saving || !current || current.source === "environment";
  const row = `${rowClassName} pb-3.5`;

  const compact = entry(SETTING.compact);
  const percent = compact ? compactPercent(compact.value) : null;
  const stall = entry(SETTING.stall);
  const minutes = stall ? stallMinutes(stall.value) : 0;
  const wake = entry(SETTING.wake);
  const replicate = entry(SETTING.replicate);
  const ablate = entry(SETTING.ablate);
  const gate = entry(SETTING.gate);
  const checklist = entry(SETTING.checklist);

  const toggle = (current: SettingEntry | undefined, title: string) =>
    current && (
      <Switch
        type="button"
        checked={isOn(current.value)}
        aria-label={title}
        disabled={locked(current)}
        onClick={() => save(current, isOn(current.value) ? "off" : "on")}
      />
    );

  return (
    <>
      {heading}
      <div className={`${cardClassName} mt-3`}>
        <Row className={row} title="Hold runs to their brief" entry={gate} description={gateDescription(gate?.value ?? "scoped")}>
          {gate && (
            <div className="inline-flex flex-none gap-0.5 p-0.5 border border-border rounded-md bg-surface" role="group" aria-label="Hold runs to their brief">
              {GATE_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={gate.value === option.value}
                  disabled={locked(gate)}
                  className={cn(
                    "py-[5px] px-2.5 rounded-sm border-0 text-sm cursor-pointer whitespace-nowrap transition-[background,color] duration-120 ease-standard disabled:cursor-default",
                    gate.value === option.value ? "text-background bg-primary" : "bg-transparent text-subtext [&:hover:not(:disabled)]:text-text [&:hover:not(:disabled)]:bg-highlight",
                  )}
                  onClick={() => gate.value !== option.value && save(gate, option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          )}
        </Row>
        <Row
          className={row}
          title="Rerun each new best with a new seed"
          entry={replicate}
          description={
            replicate && !isOn(replicate.value)
              ? "Off: a lucky run can stay the best. Turn it off only if your runs give the same score every time."
              : "When a kept idea makes a new best, its experiment is run again before anything is built on it. An experiment scores the mean of its runs, so a gain that was luck falls back."
          }
        >
          {toggle(replicate, "Rerun each new best with a new seed")}
        </Row>
        <Row
          className={row}
          title="Check each new best with ablations"
          entry={ablate}
          description="Queues leave-one-out checks for the biggest steps behind each new best, to test that their gains add up. Off by default: in the loop benchmark they took a tenth to a quarter of the budget without changing what got built."
        >
          {toggle(ablate, "Check each new best with ablations")}
        </Row>
        <Row
          className={row}
          title="Compact chats automatically"
          entry={compact}
          description={
            percent === null
              ? "Off: a chat is compacted only when you ask, or when its agent runs out of room mid-task."
              : `When a chat's context window is ${percent}% full, orx compacts it between turns, before the agent runs out of room mid-task.`
          }
        >
          {compact && percent !== null && (
            <NumberField
              shown={String(percent)}
              unit="%"
              label="Compact at this share of the context window"
              min={COMPACT_MIN}
              max={COMPACT_MAX}
              disabled={locked(compact)}
              parse={percentSetting}
              onSave={(value) => save(compact, value)}
              onInvalid={setProblem}
            />
          )}
          {compact && (
            <Switch
              type="button"
              checked={percent !== null}
              aria-label="Compact chats automatically"
              disabled={locked(compact)}
              onClick={() => save(compact, percent === null ? switchedOn(compact, COMPACT_DEFAULT) : "off")}
            />
          )}
        </Row>
        <Row
          className={row}
          title="Wake the chat when its runs finish"
          entry={wake}
          description={
            wake && !isOn(wake.value)
              ? "Off: a chat resumes after a run only if its agent asked to be woken."
              : "Each run a chat launches resumes that chat when it ends, so the agent can end its turn instead of waiting."
          }
        >
          {toggle(wake, "Wake the chat when its runs finish")}
        </Row>
        <Row
          className={row}
          title="Flag stalled runs"
          entry={stall}
          description={
            minutes === 0
              ? "Off: runs aren't watched for silence, NaNs or out-of-memory errors."
              : `A run that prints nothing for ${minutes} minutes, logs a NaN or runs out of memory is flagged, and the chat waiting on it is told.`
          }
        >
          {stall && minutes > 0 && (
            <NumberField
              shown={String(minutes)}
              unit="min"
              label="Minutes of silence before a run is flagged"
              min={1}
              max={STALL_MAX}
              disabled={locked(stall)}
              parse={minutesSetting}
              onSave={(value) => save(stall, value)}
              onInvalid={setProblem}
            />
          )}
          {stall && (
            <Switch
              type="button"
              checked={minutes > 0}
              aria-label="Flag stalled runs"
              disabled={locked(stall)}
              onClick={() => save(stall, minutes > 0 ? "0" : switchedOn(stall, STALL_DEFAULT))}
            />
          )}
        </Row>
        <Row
          className={rowClassName}
          title="Show the agent's checklist"
          entry={checklist}
          description="Gives Claude Code its to-do checklist on every model, so the chat shows the agent's progress. Applies from the agent's next turn."
        >
          {toggle(checklist, "Show the agent's checklist")}
        </Row>
        {problem && <div className="error mt-3">{problem}</div>}
      </div>
      <div className={cardClassName}>
        <h3>Network</h3>
        <dl className="m-0 grid grid-cols-[minmax(0,15rem)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm max-sm:grid-cols-1 max-sm:gap-y-0.5">
          {policyRows(settings.network).map((policy) => (
            <Fragment key={policy.key}>
              <dt className="text-subtext">{policy.label}</dt>
              <dd className="m-0 min-w-0 break-words text-text max-sm:mb-2">
                {policy.value} <code className="ms-1 text-xs text-muted">{policy.key}</code>
              </dd>
            </Fragment>
          ))}
        </dl>
        <p className="mt-3 mb-0 text-sm leading-relaxed text-subtext">
          These change only in the environment orx starts in, so that letting orx reach more of the internet takes a
          deliberate step. VLRESEARCH.md describes each variable.
        </p>
      </div>
    </>
  );
}
