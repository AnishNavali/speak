import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";
import { encodeWav, joinSamples } from "../web/audio.js";

test("dictation preserves the note on failure, retries without retranscribing, and undoes insertion", async () => {
  const elements = new Map();
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      value: "", hidden: true, disabled: false, selectionStart: 0, selectionEnd: 0,
      style: { setProperty() {} }, classList: { toggle() {} },
      addEventListener() {}, setAttribute() {}, focus() {},
      setSelectionRange(start, end) { this.selectionStart = start; this.selectionEnd = end; },
    });
    return elements.get(id);
  }
  const note = element("note");
  let transcriptions = 0, edits = 0;
  const sandbox = {
    encodeWav, joinSamples, Float32Array, Blob, console, setTimeout, clearTimeout,
    setInterval() { return 1; }, clearInterval() {},
    document: { getElementById: element, addEventListener() {} },
    window: { addEventListener() {} },
    localStorage: { getItem() { return "Before. After."; }, setItem() {} },
    fetch: async (path) => {
      if (path === "/api/health") return { ok: true, json: async () => ({ ready: true }) };
      if (path === "/api/transcribe") { transcriptions++; return { ok: true, json: async () => ({ text: "Friday, sorry, Monday." }) }; }
      edits++;
      return edits === 1 ? { ok: false, json: async () => ({ error: "Model temporarily unavailable" }) }
        : { ok: true, json: async () => ({ text: "Monday." }) };
    },
  };
  vm.createContext(sandbox);
  const source = readFileSync(new URL("../web/app.js", import.meta.url), "utf8").replace(/^import .*;\n/, "");
  vm.runInContext(source, sandbox);
  await vm.runInContext(`
    session = { chunks: [{ wav: new ArrayBuffer(44), text: null }], audio: [],
      worker: null, failure: null, before: note.value, start: 8, end: 8 };
    finish();
  `, sandbox);
  assert.equal(note.value, "Before. After.");
  assert.equal(element("recovery").hidden, false);
  assert.equal(element("raw").value, "Friday, sorry, Monday.");
  assert.equal(element("use-raw").disabled, false);
  await vm.runInContext("session.failure = null; finish();", sandbox);
  assert.equal(transcriptions, 1);
  assert.equal(edits, 2);
  assert.equal(note.value, "Before. Monday. After.");
  assert.equal(element("recovery").hidden, true);
  element("undo").onclick();
  assert.equal(note.value, "Before. After.");

  let attempts = 0;
  sandbox.fetch = async (path) => {
    if (path === "/api/transcribe") {
      attempts++;
      if (attempts === 2) return { ok: false, json: async () => ({ error: "Audio request failed" }) };
      return { ok: true, json: async () => ({ text: attempts === 1 ? "First phrase." : "Second phrase." }) };
    }
    return { ok: true, json: async () => ({ text: "First phrase. Second phrase." }) };
  };
  await vm.runInContext(`
    session = { chunks: [{ wav: new ArrayBuffer(44), text: null }, { wav: new ArrayBuffer(44), text: null }],
      audio: [], worker: null, failure: null, before: note.value, start: 0, end: 0 };
    finish();
  `, sandbox);
  assert.equal(note.value, "Before. After.");
  assert.equal(element("raw").value, "First phrase.");
  assert.equal(element("use-raw").disabled, true);
  await vm.runInContext("session.failure = null; finish();", sandbox);
  assert.equal(attempts, 3);
  assert.equal(note.value, "First phrase. Second phrase. Before. After.");
});

test("worklet flushes final samples before acknowledging Stop and averages channels", () => {
  let Processor;
  const messages = [];
  const sandbox = {
    Float32Array,
    AudioWorkletProcessor: class { constructor() { this.port = { postMessage: (message) => messages.push(message) }; } },
    registerProcessor: (_name, implementation) => { Processor = implementation; },
  };
  vm.runInNewContext(readFileSync(new URL("../web/recorder.js", import.meta.url), "utf8"), sandbox);
  const processor = new Processor();
  processor.process([[new Float32Array([1, .5]), new Float32Array([0, -.5])]]);
  processor.port.onmessage({ data: "stop" });
  assert.deepEqual(Array.from(messages[0]), [.5, 0]);
  assert.equal(messages[1], "stopped");
  assert.equal(processor.process([]), false);
});
