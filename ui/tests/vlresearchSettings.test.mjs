// VLResearch: the Research section of the Settings page.
import assert from "node:assert/strict";
import test from "node:test";
import {
  SETTING,
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
} from "../src/flavor/settings.ts";

const payload = {
  settings: [
    { key: "VLR_AUTO_COMPACT", value: "70%", source: "saved", environment: null, saved: "70%", default: "80%" },
    { key: "VLR_AUTO_ABLATE", value: "on", source: "default", environment: null, saved: null, default: "on" },
    { key: "VLR_STALL_MINUTES", value: "0", source: "environment", environment: "0", saved: "45", default: "30" },
    { key: "VLR_SCOPE_GATE", value: "strict", source: "bogus", environment: null, saved: "strict", default: "scoped" },
    { key: 42, value: "on" },
  ],
  network: [
    { key: "VLR_EGRESS_ALLOW", value: "", set: false },
    { key: "VLR_ALLOW_GITHUB", value: "off", set: false },
    { key: "VLR_ALLOW_INSTALLERS", value: "on", set: true },
    { key: "VLR_GIT_PROTOCOLS", value: "file:ssh", set: true },
    { key: "VLR_AGENT_ENV_DEFAULTS", value: "on", set: false },
  ],
};

test("settings parse with where each value comes from", () => {
  const settings = parseSettings(payload);
  assert.equal(settings.settings.length, 4, "a row without a key is dropped");
  assert.deepEqual(settingOf(settings, SETTING.compact), payload.settings[0]);
  assert.equal(settingOf(settings, SETTING.gate)?.source, "default", "an unknown source reads as the default");
  assert.equal(settingOf(settings, SETTING.wake), undefined);
  assert.deepEqual(parseSettings(null), { settings: [], network: [] });
});

test("values read as the switches and numbers the page shows", () => {
  assert.equal(compactPercent("80%"), 80);
  assert.equal(compactPercent("off"), null);
  assert.equal(stallMinutes("30"), 30);
  assert.equal(stallMinutes("0"), 0);
  assert.equal(isOn("on"), true);
  assert.equal(isOn("off"), false);
});

test("typed numbers are checked before they are saved", () => {
  assert.deepEqual(percentSetting(" 75 "), { value: "75%" });
  assert.deepEqual(percentSetting("60%"), { value: "60%" });
  for (const bad of ["", "19", "96", "70.5", "abc"]) assert.ok("error" in percentSetting(bad), bad);
  assert.deepEqual(minutesSetting("45"), { value: "45" });
  for (const bad of ["", "0", "-3", "1441", "2.5"]) assert.ok("error" in minutesSetting(bad), bad);
});

test("switching a numbered setting back on restores a number", () => {
  const entry = (saved, value = saved ?? "off") => ({ key: "k", value, source: "saved", environment: null, saved, default: "80%" });
  assert.equal(switchedOn(entry("70%"), "80%"), "70%");
  assert.equal(switchedOn(entry("off"), "80%"), "80%");
  assert.equal(switchedOn({ ...entry("0"), default: "0" }, "30"), "30");
});

test("a setting the environment pins says so; one the page decides doesn't", () => {
  const settings = parseSettings(payload);
  const pinned = pinnedNote(settingOf(settings, SETTING.stall));
  assert.match(pinned, /"0" by VLR_STALL_MINUTES/);
  assert.equal(pinnedNote(settingOf(settings, SETTING.compact)), null);
});

test("the brief gate and the network policy are worded for people", () => {
  assert.match(gateDescription("scoped"), /Other runs go ahead/);
  assert.match(gateDescription("strict"), /Every run/);
  assert.match(gateDescription("off"), /Nothing is checked/);
  const rows = policyRows(parseSettings(payload).network);
  assert.deepEqual(
    rows.map((row) => row.value),
    ["This machine only", "Off", "Allowed", "file:ssh", "Turned off by orx"],
  );
  assert.equal(policyRows([{ key: "VLR_EGRESS_ALLOW", value: "api.example.com", set: true }])[0].value, "This machine and api.example.com");
});
