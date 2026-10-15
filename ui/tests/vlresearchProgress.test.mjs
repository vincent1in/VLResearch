// VLResearch: the progress strip's checklist and status lines.
import assert from "node:assert/strict";
import test from "node:test";
import {
  checklistCounts,
  checklistFromInput,
  checklistLabel,
  currentItem,
  isChecklistTool,
  latestChecklist,
} from "../src/flavor/checklist.ts";
import {
  activityLine,
  formatSpan,
  hasLiveWork,
  parseSessionProgress,
  waitingLine,
} from "../src/flavor/progress.ts";

const MINUTE = 60_000;

const todo = (todos) => ({ type: "tool", tool: "TodoWrite", state: { status: "completed", input: { todos } } });
const message = (parts) => ({ id: crypto.randomUUID(), role: "assistant", parts, createdAt: 0 });

test("todo tools are recognized in every harness's spelling", () => {
  for (const name of ["TodoWrite", "todowrite", "todo_write"]) assert.ok(isChecklistTool(name), name);
  for (const name of [undefined, "", "Write", "TodoRead"]) assert.ok(!isChecklistTool(name), String(name));
});

test("a checklist reads the active form of the item in progress", () => {
  const items = checklistFromInput({
    todos: [
      { content: "Load the dataset", status: "completed" },
      { content: "Train the baseline", activeForm: "Training the baseline", status: "in_progress" },
      { content: "Compare with last week", status: "pending" },
      { content: "Try a bigger model", status: "cancelled" },
      { content: "", status: "pending" },
      { content: "Unknown status", status: "blocked" },
    ],
  });
  assert.deepEqual(items, [
    { text: "Load the dataset", status: "completed" },
    { text: "Training the baseline", status: "in_progress" },
    { text: "Compare with last week", status: "pending" },
    { text: "Try a bigger model", status: "cancelled" },
  ]);
  assert.deepEqual(checklistCounts(items), { done: 1, total: 3 });
  assert.deepEqual(currentItem(items), { text: "Training the baseline", status: "in_progress" });
  assert.equal(checklistFromInput({ todos: [] }), null);
  assert.equal(checklistFromInput("nope"), null);
});

test("the newest checklist on the transcript wins", () => {
  const messages = [
    message([todo([{ content: "Old plan", status: "pending" }])]),
    message([
      { type: "text", text: "Starting." },
      todo([{ content: "New plan", status: "completed" }, { content: "Next step", status: "pending" }]),
      { type: "tool", tool: "Bash", state: { status: "running", input: { command: "ls" } } },
    ]),
  ];
  const items = latestChecklist(messages);
  assert.equal(items?.[0].text, "New plan");
  assert.deepEqual(currentItem(items), { text: "Next step", status: "pending" });
  assert.equal(latestChecklist([message([{ type: "text", text: "hi" }])]), null);
});

test("the transcript row counts what is done", () => {
  assert.equal(
    checklistLabel({ todos: [{ content: "a", status: "completed" }, { content: "b", status: "pending" }] }),
    "Updated its checklist: 1 of 2 done",
  );
  assert.equal(checklistLabel({}), "Updated its checklist");
});

test("spans are short and coarse", () => {
  assert.equal(formatSpan(-5), "0s");
  assert.equal(formatSpan(45_000), "45s");
  assert.equal(formatSpan(12 * MINUTE + 30_000), "12m");
  assert.equal(formatSpan(65 * MINUTE), "1h 05m");
  assert.equal(formatSpan((3 * 24 + 4) * 60 * MINUTE), "3d 4h");
});

test("the working line escalates with silence, gently while a tool runs", () => {
  const now = 100 * MINUTE;
  assert.equal(activityLine({ lastActivityAt: null, toolRunning: false, now }), null);
  assert.deepEqual(activityLine({ lastActivityAt: now - 2_000, toolRunning: false, now }), { text: "Working", tone: "normal" });
  assert.deepEqual(activityLine({ lastActivityAt: now - 2 * MINUTE, toolRunning: false, now }), {
    text: "Working · last update 2m ago",
    tone: "normal",
  });
  assert.equal(activityLine({ lastActivityAt: now - 6 * MINUTE, toolRunning: false, now }).tone, "warn");
  assert.deepEqual(activityLine({ lastActivityAt: now - 20 * MINUTE, toolRunning: false, now }), {
    text: "Working · last update 20m ago · may be stuck",
    tone: "alert",
  });
  assert.equal(activityLine({ lastActivityAt: now - 10 * MINUTE, toolRunning: true, now }).tone, "normal");
  assert.equal(activityLine({ lastActivityAt: now - 20 * MINUTE, toolRunning: true, now }).tone, "warn");
});

test("waiting lines say how long a run usually takes and flag overdue or silent ones", () => {
  const now = 100 * MINUTE;
  const run = {
    runId: "r1",
    experimentId: "e1",
    experiment: "Volume features",
    status: "running",
    startedAt: now - 12 * MINUTE,
    typicalMs: 20 * MINUTE,
    lastOutputAt: now - MINUTE,
    alerts: [],
  };
  assert.deepEqual(waitingLine(run, now), {
    text: "Waiting on Volume features · running 12m · usually about 20m",
    tone: "normal",
  });
  assert.equal(waitingLine({ ...run, startedAt: now - 45 * MINUTE }, now).tone, "warn");
  assert.deepEqual(waitingLine({ ...run, typicalMs: null, lastOutputAt: now - 11 * MINUTE }, now), {
    text: "Waiting on Volume features · running 12m · no output for 11m",
    tone: "warn",
  });
  assert.equal(waitingLine({ ...run, alerts: [{ kind: "quiet", message: "x", at: 1 }] }, now).tone, "alert");
  assert.deepEqual(waitingLine({ ...run, status: "starting", experiment: null }, now), {
    text: "Waiting on a run · starting, 12m so far",
    tone: "normal",
  });
});

test("progress responses are read defensively", () => {
  assert.deepEqual(parseSessionProgress(null), { lastActivityAt: null, waiting: [] });
  const parsed = parseSessionProgress({
    lastActivityAt: 5,
    waiting: [
      { runId: "r1", startedAt: 1, experiment: "E", typicalMs: "soon", alerts: [{ message: "quiet" }, "junk"] },
      { runId: 7, startedAt: 1 },
      "junk",
    ],
  });
  assert.equal(parsed.lastActivityAt, 5);
  assert.equal(parsed.waiting.length, 1);
  assert.equal(parsed.waiting[0].typicalMs, null);
  assert.equal(parsed.waiting[0].status, "running");
  assert.deepEqual(parsed.waiting[0].alerts, [{ kind: "notice", message: "quiet", at: 0 }]);
  assert.ok(hasLiveWork(false, parsed));
  assert.ok(!hasLiveWork(false, { lastActivityAt: null, waiting: [] }));
  assert.ok(hasLiveWork(true, undefined));
});
