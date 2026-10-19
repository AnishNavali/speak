import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

test("settings open the test notepad without losing text; quick push-to-talk release flushes Start", async () => {
  const elements = new Map(), events = new Map(), calls = [];
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      value: "", checked: false, hidden: false, selectionStart: 8, selectionEnd: 8,
      dataset: {}, style: { setProperty() {} }, classList: { toggle() {} },
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

async function setupHarness(overrides = {}, handler = async () => {}) {
  const elements = new Map(), events = new Map(), calls = [];
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      value: "", dataset: {}, open: false, hidden: false, textContent: "", listeners: {},
      style: { setProperty() {} }, classList: { toggle() {} },
      addEventListener(name, callback) { this.listeners[name] = callback; },
      setAttribute() {}, removeAttribute(name) { delete this[name]; }, append() {}, focus() { this.focused = true; },
      showModal() { this.open = true; }, close() { this.open = false; },
    });
    return elements.get(id);
  }
  const initial = {
    view: "settings", phase: "idle", ready: false, input_ready: false, model_setup_needed: true,
    model_status: "Choose model files to begin", message: "Hold the shortcut to dictate", shortcut: "F5",
    settings: { model: "", projector: "", microphone: "", shortcut: "F5" }, ...overrides,
  };
  const invoke = async (command, args) => {
    calls.push([command, args]);
    if (command === "snapshot") return initial;
    if (command === "microphones") return [];
    if (command === "get_autostart") return false;
    return handler(command, args);
  };
  const sandbox = {
    window: { __TAURI__: { core: { invoke }, event: { listen: async (name, callback) => events.set(name, callback) } }, addEventListener() {} },
    document: { body: { dataset: {} }, getElementById: element, querySelectorAll: () => [], addEventListener() {} },
    localStorage: { getItem: () => "", setItem() {} }, console,
  };
  const source = readFileSync(new URL("../web/desktop.js", import.meta.url), "utf8");
  await vm.runInNewContext(`(async () => { ${source}\n })()`, sandbox);
  return { element, events, calls, initial };
}

test("setup prompts only for missing models; Later is session-only and existing files open Settings", async () => {
  const existing = await setupHarness({ model_setup_needed: false });
  assert.equal(existing.element("setup-dialog").open, false);
  const fresh = await setupHarness();
  assert.equal(fresh.element("setup-dialog").open, true);
  assert.equal(fresh.calls.some(([command]) => command === "download_model"), false);
  fresh.element("setup-later").onclick();
  fresh.events.get("speak-state")({ payload: fresh.initial });
  assert.equal(fresh.element("setup-dialog").open, false);
  const reopened = await setupHarness();
  assert.equal(reopened.element("setup-dialog").open, true);
  await reopened.element("setup-existing").onclick();
  assert.equal(reopened.element("setup-dialog").open, false);
  assert.equal(reopened.element("settings-panel").open, true);
  assert.equal(reopened.element("model").focused, true);
  assert.ok(reopened.calls.some(([command]) => command === "open_settings"));
  assert.equal(reopened.calls.some(([command]) => command === "download_model"), false);
});

test("setup resumes failed downloads, reports progress, and retries startup without downloading again", async () => {
  let downloads = 0, starts = 0, finishDownload;
  const saved = { model: "saved.gguf", projector: "audio.gguf", shortcut: "F5" };
  const ui = await setupHarness({}, async (command, args) => {
    if (command === "download_model") {
      assert.equal(args.preset, "q4");
      if (++downloads === 1) throw new Error("Download stalled");
      return new Promise((resolve) => { finishDownload = () => resolve(saved); });
    }
    if (command === "start_model" && ++starts === 1) throw new Error("Not enough memory");
  });
  await ui.element("setup-download").onclick();
  assert.match(ui.element("setup-status").textContent, /Download stalled/);
  assert.equal(ui.element("setup-download").textContent, "Retry / resume download");
  const retry = ui.element("setup-download").onclick();
  assert.equal(ui.element("setup-download").disabled, true);
  assert.equal(ui.element("setup-later").textContent, "Continue in background");
  await ui.element("setup-download").onclick();
  assert.equal(downloads, 2);
  ui.events.get("speak-download")({ payload: { file: "gemma.gguf", received: 500, total: 1000 } });
  assert.equal(ui.element("setup-progress").value, 50);
  assert.match(ui.element("setup-status").textContent, /Voice model: 50.0%/);
  ui.events.get("speak-download")({ payload: { file: "mmproj-audio.gguf", received: 10, total: 100 } });
  assert.match(ui.element("setup-status").textContent, /Audio support: 10.0%/);
  ui.events.get("speak-state")({ payload: { ...ui.initial, settings: saved, model_setup_needed: false } });
  assert.equal(ui.element("setup-dialog").open, true);
  finishDownload();
  await retry;
  assert.equal(ui.element("model").value, "saved.gguf");
  assert.match(ui.element("setup-status").textContent, /Not enough memory/);
  assert.equal(ui.element("setup-download").textContent, "Retry startup");
  await ui.element("setup-download").onclick();
  assert.equal(downloads, 2);
  assert.equal(starts, 2);
  assert.equal(ui.element("setup-title").textContent, "You're ready to speak.");
  assert.match(ui.element("setup-status").textContent, /F5/);
  assert.equal(ui.element("setup-progress").hidden, true);
  await ui.element("setup-download").onclick();
  assert.equal(ui.element("setup-dialog").open, false);
  assert.ok(ui.calls.some(([command]) => command === "open_test_ui"));
});

test("closing setup during download keeps it running without reopening the prompt", async () => {
  let finishDownload, started = false;
  const ui = await setupHarness({}, async (command) => {
    if (command === "download_model") return new Promise((resolve) => {
      finishDownload = () => resolve({ model: "saved.gguf", projector: "audio.gguf" });
    });
    if (command === "start_model") started = true;
  });
  const download = ui.element("setup-download").onclick();
  ui.element("setup-later").onclick();
  ui.events.get("speak-state")({ payload: ui.initial });
  assert.equal(ui.element("setup-dialog").open, false);
  assert.equal(ui.element("download-model").disabled, true);
  finishDownload();
  await download;
  assert.equal(started, true);
  assert.equal(ui.element("setup-dialog").open, false);
  assert.equal(ui.element("download-model").disabled, false);
  assert.match(ui.element("download-status").textContent, /F5/);
});
