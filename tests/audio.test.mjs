import assert from "node:assert/strict";
import { test } from "node:test";
import { encodeWav, joinSamples } from "../web/audio.js";

test("WAV is mono PCM with the correct length, rate, and clipped samples", () => {
  const samples = joinSamples([new Float32Array([-2, -.5]), new Float32Array([0, .5, 2])]);
  const buffer = encodeWav(samples, 16000);
  const bytes = new Uint8Array(buffer);
  const view = new DataView(buffer);
  assert.equal(new TextDecoder().decode(bytes.slice(0, 4)), "RIFF");
  assert.equal(new TextDecoder().decode(bytes.slice(8, 12)), "WAVE");
  assert.equal(view.getUint32(4, true), buffer.byteLength - 8);
  assert.equal(view.getUint16(20, true), 1);
  assert.equal(view.getUint16(22, true), 1);
  assert.equal(view.getUint32(24, true), 16000);
  assert.equal(view.getUint16(34, true), 16);
  assert.equal(view.getUint32(40, true), 10);
  assert.deepEqual(Array.from({ length: 5 }, (_, i) => view.getInt16(44 + i * 2, true)), [-32768, -16384, 0, 16384, 32767]);
});
