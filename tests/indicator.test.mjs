import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

test("recording wave responds to volume, resets on stop, and preserves quick-release recording", async () => {
  const elements = new Map(), events = new Map(), calls = [];
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      style: { setProperty(key, value) { this[key] = value; } },
      setAttribute() {}, setPointerCapture() {},
    });
    return elements.get(id);
  }
  const idle = { phase: "idle", ready: true, model_status: "Ready", message: "Ready" };
  let finishPress;
  const invoke = async (command) => {
    calls.push(command);
    if (command === "snapshot") return idle;
    if (command === "press") return new Promise((resolve) => { finishPress = resolve; });
  };
  const body = { dataset: {}, style: { setProperty() {} } };
  const sandbox = {
    window: { __TAURI__: { core: { invoke }, event: { listen: async (name, handler) => events.set(name, handler) } } },
    document: { body, getElementById: element, addEventListener() {} },
  };
  const source = readFileSync(new URL("../web/indicator.js", import.meta.url), "utf8");
  await vm.runInNewContext(`(async () => { ${source}\n })()`, sandbox);
  const level = () => Number(element("waveform").style["--wave-level"]);
  assert.equal(level(), .08);
  events.get("speak-state")({ payload: { ...idle, phase: "recording" } });
  events.get("speak-meter")({ payload: { level: .03 } });
  assert.equal(level(), .36);
  events.get("speak-meter")({ payload: { level: .9 } });
  assert.equal(level(), 1);
  events.get("speak-meter")({ payload: { level: 0 } });
  assert.equal(level(), .08);
  events.get("speak-state")({ payload: { ...idle, phase: "processing" } });
  events.get("speak-meter")({ payload: { level: .9 } });
  assert.equal(level(), .08);
  assert.equal(element("mic").disabled, true);
  events.get("speak-state")({ payload: idle });
  element("mic").onpointerdown({ preventDefault() {}, pointerId: 1 });
  const release = element("mic").onpointerup();
  assert.equal(calls.includes("release"), false);
  finishPress();
  await release;
  assert.equal(calls.filter((call) => call === "release").length, 1);
});
