import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

test("settings open the test notepad without losing text; quick push-to-talk release flushes Start", async () => {
  const elements = new Map(), events = new Map(), calls = [];
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      value: "", checked: false, hidden: false, selectionStart: 8, selectionEnd: 8,
      style: { setProperty() {} }, classList: { toggle() {} },
      listeners: {}, addEventListener(name, callback) { this.listeners[name] = callback; },
      setAttribute() {}, setPointerCapture() {}, append() {},
      setSelectionRange(start, end) { this.selectionStart = start; this.selectionEnd = end; },
    });
    return elements.get(id);
  }
  let finishPress;
  const initial = {
    view: "settings",
    session_id: 1,
    phase: "idle", ready: true, input_ready: false, model_status: "Ready", message: "Ready", notice: "",
    raw: "", result: "", shortcut: "Control+Shift+Space", data_dir: "/local",
    settings: { model: "model.gguf", projector: "audio.gguf", executable: "", microphone: "", shortcut: "Control+Shift+Space", pause_threshold: .006, floating: true },
  };
  const invoke = async (command) => {
    calls.push(command);
    if (command === "snapshot") return initial;
    if (command === "microphones") return [];
    if (command === "get_autostart") return false;
    if (command === "press") return new Promise((resolve) => { finishPress = resolve; });
  };
  const sandbox = {
    window: { __TAURI__: { core: { invoke }, event: { listen: async (name, handler) => events.set(name, handler) } }, addEventListener() {} },
    document: { body: { dataset: {} }, getElementById: element, querySelectorAll: () => [], addEventListener() {} },
    localStorage: { getItem: () => "Before. After.", setItem() {} },
    console,
  };
  const source = readFileSync(new URL("../web/desktop.js", import.meta.url), "utf8");
  await vm.runInNewContext(`(async () => { ${source}\n })()`, sandbox);
  assert.equal(element("test-view").hidden, true);
  assert.equal(element("settings-panel").hidden, false);
  assert.equal(element("hold").hidden, true);
  await element("open-test").onclick();
  assert.ok(calls.includes("open_test_ui"));
  events.get("speak-state")({ payload: { ...initial, view: "test" } });
  assert.equal(element("test-view").hidden, false);
  assert.equal(element("settings-panel").hidden, true);
  assert.equal(element("hold").hidden, false);
  const press = element("hold").listeners.pointerdown({ preventDefault() {}, pointerId: 1 });
  const release = element("hold").listeners.pointerup();
  assert.equal(calls.filter((call) => call === "release").length, 0);
  finishPress();
  await Promise.all([press, release]);
  assert.equal(calls.filter((call) => call === "release").length, 1);
  events.get("speak-test-result")({ payload: { id: 999, text: "Stale result" } });
  assert.equal(element("note").value, "Before. After.");
  events.get("speak-test-result")({ payload: { id: 1, text: "Monday." } });
  assert.equal(element("note").value, "Before. Monday. After.");
  await element("open-settings").onclick();
  assert.ok(calls.includes("open_settings"));
  events.get("speak-state")({ payload: initial });
  assert.equal(element("test-view").hidden, true);
  assert.equal(element("settings-panel").hidden, false);
  assert.equal(element("note").value, "Before. Monday. After.");
  events.get("speak-state")({ payload: { ...initial, phase: "error", raw: "retained transcript", message: "Request failed" } });
  assert.equal(element("raw").value, "retained transcript");
  assert.equal(element("recovery").hidden, false);
  assert.equal(element("note").readOnly, true);
});
