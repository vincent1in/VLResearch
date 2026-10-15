// VLResearch: the Research section of the Settings page, for
// GET/PUT /api/vlresearch/settings. Kept free of React for the tests
// (ui/tests/vlresearchSettings.test.mjs).

export type SettingSource = "environment" | "saved" | "default";

/** One research setting: the value in effect and where it comes from. */
export type SettingEntry = {
  key: string;
  value: string;
  source: SettingSource;
  /** The value given in the environment, as given; it wins over the page. */
  environment: string | null;
  /** The value saved on the page. */
  saved: string | null;
  default: string;
};

/** One part of the network policy, which the page shows but can't change. */
export type PolicyEntry = { key: string; value: string; set: boolean };

export type ResearchSettings = { settings: SettingEntry[]; network: PolicyEntry[] };

/** The variable behind each setting. */
export const SETTING = {
  compact: "VLR_AUTO_COMPACT",
  replicate: "VLR_AUTO_REPLICATE",
  ablate: "VLR_AUTO_ABLATE",
  stall: "VLR_STALL_MINUTES",
  wake: "VLR_AUTO_WAKE",
  gate: "VLR_SCOPE_GATE",
  checklist: "VLR_AGENT_CHECKLIST",
} as const;

export const COMPACT_MIN = 20;
export const COMPACT_MAX = 95;
export const STALL_MAX = 24 * 60;
/** What a switched-on setting goes back to when it has no number of its own. */
export const COMPACT_DEFAULT = "80%";
export const STALL_DEFAULT = "30";

const record = (value: unknown): Record<string, unknown> =>
  value !== null && typeof value === "object" ? (value as Record<string, unknown>) : {};
const text = (value: unknown): string | null => (typeof value === "string" ? value : null);

export function parseSettings(value: unknown): ResearchSettings {
  const body = record(value);
  const list = (items: unknown) => (Array.isArray(items) ? items.map(record) : []);
  const settings = list(body.settings).flatMap((entry): SettingEntry[] => {
    const key = text(entry.key);
    const current = text(entry.value);
    if (!key || current === null) return [];
    const source = entry.source === "environment" || entry.source === "saved" ? entry.source : "default";
    return [{ key, value: current, source, environment: text(entry.environment), saved: text(entry.saved), default: text(entry.default) ?? current }];
  });
  const network = list(body.network).flatMap((entry): PolicyEntry[] => {
    const key = text(entry.key);
    return key ? [{ key, value: text(entry.value) ?? "", set: entry.set === true }] : [];
  });
  return { settings, network };
}

export function settingOf(settings: ResearchSettings, key: string): SettingEntry | undefined {
  return settings.settings.find((entry) => entry.key === key);
}

/** Whether a switch setting is on. */
export function isOn(value: string): boolean {
  return value !== "off";
}

/** 80 for "80%"; null when autocompaction is off. */
export function compactPercent(value: string): number | null {
  const percent = Number.parseFloat(value);
  return value === "off" || !Number.isFinite(percent) ? null : percent;
}

/** Minutes of quiet before a run is flagged; 0 when stall alerts are off. */
export function stallMinutes(value: string): number {
  const minutes = Number.parseInt(value, 10);
  return Number.isFinite(minutes) && minutes > 0 ? minutes : 0;
}

export type Typed = { value: string } | { error: string };

/** A percentage typed for autocompaction: the value to save, or why not. */
export function percentSetting(typed: string): Typed {
  const trimmed = typed.trim().replace(/%$/, "").trim();
  const percent = Number(trimmed);
  if (trimmed === "" || !Number.isInteger(percent) || percent < COMPACT_MIN || percent > COMPACT_MAX) {
    return { error: `Pick a whole percentage from ${COMPACT_MIN} to ${COMPACT_MAX}.` };
  }
  return { value: `${percent}%` };
}

/** Minutes typed for stall alerts: the value to save, or why not. */
export function minutesSetting(typed: string): Typed {
  const trimmed = typed.trim();
  const minutes = Number(trimmed);
  if (trimmed === "" || !Number.isInteger(minutes) || minutes < 1 || minutes > STALL_MAX) {
    return { error: `Pick whole minutes from 1 to ${STALL_MAX}.` };
  }
  return { value: String(minutes) };
}

/** What switching a numbered setting on restores: its saved number, else the default's. */
export function switchedOn(entry: SettingEntry, fallback: string): string {
  const off = (value: string | null) => value === null || value === "off" || value === "0";
  if (!off(entry.saved)) return entry.saved as string;
  if (!off(entry.default)) return entry.default;
  return fallback;
}

export const GATE_OPTIONS = [
  { value: "scoped", label: "Optimization runs" },
  { value: "strict", label: "Every run" },
  { value: "off", label: "Off" },
] as const;

/** What each brief gate setting does to launches. */
export function gateDescription(value: string): string {
  switch (value) {
    case "strict":
      return "Every run must belong to an optimization whose brief you confirmed, and stays within its budget, locked files and scope.";
    case "off":
      return "Nothing is checked: runs launch without a confirmed brief, and budgets, locked files and scopes are not enforced.";
    default:
      return "Runs of an optimization wait for you to confirm its brief, and stay within its budget, locked files and scope. Other runs go ahead.";
  }
}

/** The note under a setting the environment pins, or null when the page decides it. */
export function pinnedNote(entry: SettingEntry): string | null {
  if (entry.source !== "environment") return null;
  const given = entry.environment ?? entry.value;
  return `Set to "${given}" by ${entry.key} in the environment orx started in, which wins over this page.`;
}

export type PolicyRow = { key: string; label: string; value: string };

/** The network policy as the page words it, in the order the server lists it. */
export function policyRows(network: readonly PolicyEntry[]): PolicyRow[] {
  return network.flatMap((entry): PolicyRow[] => {
    const on = entry.value === "on";
    switch (entry.key) {
      case "VLR_EGRESS_ALLOW":
        return [{ key: entry.key, label: "Hosts orx may reach", value: entry.value === "" ? "This machine only" : entry.value === "*" ? "Any host" : `This machine and ${entry.value}` }];
      case "VLR_ALLOW_GITHUB":
        return [{ key: entry.key, label: "GitHub integration", value: on ? "On" : "Off" }];
      case "VLR_ALLOW_INSTALLERS":
        return [{ key: entry.key, label: "Installing coding agents", value: on ? "Allowed" : "Off: install agents yourself" }];
      case "VLR_GIT_PROTOCOLS":
        return [{ key: entry.key, label: "Git transports", value: entry.value === "file" ? "Local repositories only" : entry.value === "*" ? "Any" : entry.value }];
      case "VLR_AGENT_ENV_DEFAULTS":
        return [{ key: entry.key, label: "Agents' telemetry and auto-updates", value: on ? "Turned off by orx" : "Left to each agent" }];
      default:
        return [{ key: entry.key, label: entry.key, value: entry.value }];
    }
  });
}
