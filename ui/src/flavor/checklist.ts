// VLResearch: the coding agent's own checklist, read from its todo tool calls.
//
// Claude Code's TodoWrite and OpenCode's todowrite both send the whole list on
// every call ({ todos: [{ content, status, activeForm? }] }), so the newest call
// is the current state. VLResearch turns TodoWrite on for every Claude model
// (see src/flavor/harness.rs).
import type { ChatMessage, ChatPart } from "../api";

export type ChecklistStatus = "pending" | "in_progress" | "completed" | "cancelled";

export interface ChecklistItem {
  text: string;
  status: ChecklistStatus;
}

const STATUSES: ReadonlySet<string> = new Set(["pending", "in_progress", "completed", "cancelled"]);

/** Whether a tool name is an agent's todo tool, in any harness's spelling. */
export function isChecklistTool(tool: string | undefined): boolean {
  return !!tool && tool.replace(/[^a-z]/gi, "").toLowerCase() === "todowrite";
}

/** The items a todo tool call carries, or null when the input has none. */
export function checklistFromInput(input: unknown): ChecklistItem[] | null {
  if (typeof input !== "object" || input === null) return null;
  const todos = (input as Record<string, unknown>).todos;
  if (!Array.isArray(todos)) return null;
  const items: ChecklistItem[] = [];
  for (const todo of todos) {
    if (typeof todo !== "object" || todo === null) continue;
    const record = todo as Record<string, unknown>;
    const status = typeof record.status === "string" ? record.status : "";
    if (!STATUSES.has(status)) continue;
    const content = typeof record.content === "string" ? record.content.trim() : "";
    const active = typeof record.activeForm === "string" ? record.activeForm.trim() : "";
    const text = status === "in_progress" && active ? active : content || active;
    if (text) items.push({ text, status: status as ChecklistStatus });
  }
  return items.length > 0 ? items : null;
}

function checklistFromPart(part: ChatPart): ChecklistItem[] | null {
  if (part.type !== "tool" || !isChecklistTool(part.tool)) return null;
  return checklistFromInput(part.state?.input);
}

/** The agent's newest checklist on this transcript path, or null. */
export function latestChecklist(messages: readonly ChatMessage[]): ChecklistItem[] | null {
  for (let m = messages.length - 1; m >= 0; m--) {
    const parts = messages[m].parts;
    for (let p = parts.length - 1; p >= 0; p--) {
      const items = checklistFromPart(parts[p]);
      if (items) return items;
    }
  }
  return null;
}

/** Completed items out of those still planned (dropped items don't count). */
export function checklistCounts(items: readonly ChecklistItem[]): { done: number; total: number } {
  const planned = items.filter((item) => item.status !== "cancelled");
  const done = planned.filter((item) => item.status === "completed").length;
  return { done, total: planned.length };
}

/** The item the agent is on: the one in progress, else the first not started. */
export function currentItem(items: readonly ChecklistItem[]): ChecklistItem | null {
  return (
    items.find((item) => item.status === "in_progress")
    ?? items.find((item) => item.status === "pending")
    ?? null
  );
}

/** Transcript row label for a todo tool call. */
export function checklistLabel(input: unknown): string {
  const items = checklistFromInput(input);
  if (!items) return "Updated its checklist";
  const { done, total } = checklistCounts(items);
  return `Updated its checklist: ${done} of ${total} done`;
}
