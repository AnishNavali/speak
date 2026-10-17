import { encodeWav, joinSamples } from "./audio.js";

const $ = (id) => document.getElementById(id);
const note = $("note");
let state = "idle";
let session = null;
let undo = null;
let stream, context, recorder, timer;
let stopAck;
let frames = [], frameCount = 0, silence = 0;
let modelReady = false;

try { note.value = localStorage.getItem("speak.note") || ""; } catch { $("saved").textContent = "Browser storage unavailable"; }

function saveNote() {
  $("word-count").textContent = `${note.value.trim() ? note.value.trim().split(/\s+/u).length : 0} words`;
  try {
    localStorage.setItem("speak.note", note.value);
    $("saved").textContent = "Saved on this device";
  } catch { $("saved").textContent = "Not saved — export your note"; }
}
note.addEventListener("input", saveNote);
saveNote();

function status(title, detail = "") { $("status").textContent = title; $("detail").textContent = detail; }
function error(message) { $("error").textContent = message; $("error").hidden = !message; }
function updateControls() {
  const recording = state === "recording";
  $("record").setAttribute("aria-pressed", String(recording));
  $("record-label").textContent = recording ? "Stop & polish" : state === "starting" ? "Opening microphone…" : state === "processing" ? "Processing…" : "Start speaking";
  $("record").disabled = !["idle", "recording"].includes(state);
  note.readOnly = state !== "idle";
  $("recovery").hidden = state !== "error";
  $("use-raw").disabled = !session?.chunks.length || session.chunks.some((chunk) => chunk.text === null) || !$("raw").value.trim();
  $("undo").disabled = state !== "idle" || !undo;
}

async function api(path, body, type) {
  const response = await fetch(path, { method: "POST", headers: { "Content-Type": type }, body });
  let result;
  try { result = await response.json(); } catch { throw new Error("The local app returned an unreadable response. Your recording is retained."); }
  if (!response.ok) throw new Error(result.error || "Request failed.");
  if (typeof result.text !== "string") throw new Error("The local app returned no text.");
  return result.text;
}

async function checkModel() {
  try {
    const response = await fetch("/api/health");
    if (!response.ok) throw new Error();
    const result = await response.json();
    modelReady = result.ready;
    $("model-status").textContent = modelReady ? "Gemma is ready" : "Gemma is offline / loading";
    $("model-status").title = result.message;
    $("model-status").classList.toggle("ready", modelReady);
  } catch {
    modelReady = false;
    $("model-status").textContent = "Local app is offline";
    $("model-status").classList.remove("ready");
  }
}
checkModel();
setInterval(checkModel, 10000);

function rawTranscript() {
  return session.chunks.filter((chunk) => chunk.text !== null).map((chunk) => chunk.text).filter(Boolean).join(" ");
}

function pump() {
  if (session.worker) return session.worker;
  session.worker = (async () => {
    try {
      for (const chunk of session.chunks) {
        if (chunk.text !== null) continue;
        chunk.text = await api("/api/transcribe", chunk.wav, "audio/wav");
        $("raw").value = rawTranscript();
        if (state === "recording") status("Listening", `${session.chunks.filter((c) => c.text !== null).length} segment(s) transcribed in the background.`);
      }
      session.failure = null;
    } catch (failure) {
      session.failure = failure;
      error(failure.message);
      if (state === "recording") status("Still recording", "Processing paused. Stop when ready, then retry.");
    } finally { session.worker = null; }
  })();
  return session.worker;
}

function enqueue() {
  if (!frameCount) return;
  const samples = joinSamples(frames);
  const wav = encodeWav(samples, context.sampleRate);
  session.chunks.push({ wav, text: null });
  frames = []; frameCount = 0; silence = 0;
  if (!session.failure) pump();
}

function receive(samples) {
  session.audio.push(samples);
  frames.push(samples);
  frameCount += samples.length;
  const rms = Math.sqrt(samples.reduce((sum, value) => sum + value * value, 0) / samples.length);
  $("meter").style.setProperty("--level", String(Math.min(1.65, .3 + rms * 18)));
  // ponytail: energy-based pause detection, not speech classification. Tune for microphones;
  // use a real VAD if noise regularly prevents pauses. No quiet audio is discarded.
  silence = rms < .006 ? silence + samples.length / context.sampleRate : 0;
  if (frameCount / context.sampleRate >= 24 || (frameCount / context.sampleRate >= 4 && silence >= .7)) enqueue();
}

async function releaseMicrophone() {
  clearInterval(timer);
  if (stream) {
    for (const track of stream.getTracks()) { track.onended = null; track.stop(); }
  }
  if (recorder) recorder.disconnect();
  if (context && context.state !== "closed") await context.close();
  stream = context = recorder = null;
  $("meter").style.setProperty("--level", ".4");
}

async function start() {
  if (state !== "idle") return;
  state = "starting"; updateControls(); error("");
  await checkModel();
  if (!modelReady) {
    state = "idle"; updateControls();
    error("Start llama-server and wait for Gemma to load before recording.");
    return;
  }
  try {
    if (!navigator.mediaDevices?.getUserMedia || !window.AudioWorkletNode) throw new Error("Use a current Safari, Chrome, or Firefox browser at the local app address.");
    stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true }, video: false });
    context = new AudioContext({ sampleRate: 16000 });
    await context.audioWorklet.addModule("/recorder.js");
    await context.resume();
    session = {
      chunks: [], audio: [], worker: null, failure: null, started: Date.now(),
      sampleRate: context.sampleRate, before: note.value,
      start: note.selectionStart, end: note.selectionEnd,
    };
    undo = null;
    $("raw").value = "";
    frames = []; frameCount = 0; silence = 0;
    recorder = new AudioWorkletNode(context, "recorder");
    recorder.port.onmessage = ({ data }) => {
      if (data === "stopped") { stopAck?.(); return; }
      receive(data);
    };
    context.createMediaStreamSource(stream).connect(recorder);
    recorder.connect(context.destination);
    state = "recording"; updateControls();
    status("Listening", "Speak naturally. Text will be polished when you stop.");
    $("timer").textContent = "00:00";
    timer = setInterval(() => {
      const seconds = Math.floor((Date.now() - session.started) / 1000);
      $("timer").textContent = `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
      // ponytail: bound in-memory recordings to ten minutes; use disk-backed sessions for longer dictation.
      if (seconds >= 600) stop();
    }, 500);
    stream.getAudioTracks()[0].onended = () => {
      error("The microphone disconnected. Finishing the audio already captured.");
      stop();
    };
  } catch (failure) {
    await releaseMicrophone();
    state = "idle"; updateControls();
    error(failure.name === "NotAllowedError" ? "Microphone access was denied. Allow it in your browser, then try again." : failure.message);
    status("Microphone unavailable", "Your note is unchanged.");
  }
}

function insert(text) {
  const left = session.before.slice(0, session.start);
  const right = session.before.slice(session.end);
  const prefix = left && !/\s$/u.test(left) ? " " : "";
  const suffix = right && !/^\s/u.test(right) ? " " : "";
  note.value = left + prefix + text + suffix + right;
  undo = { before: session.before, after: note.value };
  const cursor = left.length + prefix.length + text.length;
  state = "idle"; updateControls(); saveNote();
  note.focus(); note.setSelectionRange(cursor, cursor);
  error("");
  status("Added to your note", "You can edit, undo, or dictate another passage.");
  session.audio = [];
  session.chunks = [];
}

async function finish() {
  state = "processing"; updateControls(); error("");
  status("Transcribing", "Finishing your recording. Your note is safe.");
  await pump();
  if (session.failure) {
    state = "error"; updateControls();
    error(session.failure.message);
    status("Recording retained", "Retry processing or save the audio before leaving this page.");
    return;
  }
  const raw = rawTranscript();
  $("raw").value = raw;
  if (!raw.trim()) {
    state = "error"; updateControls();
    error("Gemma returned no speech. You can save the recording or retry.");
    status("No text returned", "Your note is unchanged.");
    return;
  }
  status("Polishing your words", "Adding punctuation and resolving spoken corrections.");
  try {
    const polished = await api("/api/polish", JSON.stringify({ text: raw }), "application/json");
    insert(polished);
  } catch (failure) {
    state = "error"; updateControls(); error(failure.message);
    status("Transcript retained", "Retry polishing or insert the raw transcript.");
  }
}

async function stop() {
  if (state !== "recording") return;
  state = "processing"; updateControls();
  status("Finishing recording", "Capturing the final words.");
  try {
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("Audio capture did not finish cleanly. Save the captured recording before retrying.")), 3000);
      stopAck = () => { clearTimeout(timeout); resolve(); };
      recorder.port.postMessage("stop");
    });
    enqueue();
    await releaseMicrophone();
    stopAck = null;
    await finish();
  } catch (failure) {
    enqueue();
    await releaseMicrophone();
    if (session.worker) await session.worker;
    stopAck = null;
    state = "error"; updateControls(); error(failure.message);
    status("Recording interrupted", "Captured audio is retained. Save it or retry processing.");
  }
}

function download(blob, filename) {
  const link = document.createElement("a");
  const url = URL.createObjectURL(blob);
  link.href = url; link.download = filename; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
$("record").onclick = () => state === "recording" ? stop() : start();
$("retry").onclick = () => {
  if (state !== "error") return;
  if (!rawTranscript().trim()) {
    for (const chunk of session.chunks) chunk.text = null;
  }
  session.failure = null;
  finish();
};
$("use-raw").onclick = () => { if (state === "error" && !$("use-raw").disabled) insert($("raw").value); };
$("save-audio").onclick = () => download(new Blob([encodeWav(joinSamples(session.audio), session.sampleRate)], { type: "audio/wav" }), "speak-recording.wav");
$("discard").onclick = () => {
  if (state !== "error" || !confirm("Discard this recording? Your existing note will be kept.")) return;
  session = null; state = "idle"; error(""); updateControls();
  status("Ready when you are", "Click the microphone to begin.");
};
$("undo").onclick = () => {
  if (!undo || state !== "idle") return;
  if (note.value !== undo.after) { error("Your note has changed since insertion. Use the editor’s undo or edit it manually to preserve your changes."); return; }
  note.value = undo.before; undo = null; saveNote(); updateControls();
  status("Insertion undone", "Your previous note has been restored.");
};
$("copy").onclick = async () => {
  try { await navigator.clipboard.writeText(note.value); status("Copied", "Your note is on the clipboard."); }
  catch { error("Clipboard access failed. Select the note and copy it manually."); }
};
$("download").onclick = () => download(new Blob([note.value], { type: "text/plain;charset=utf-8" }), "speak-note.txt");
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.code === "Space") {
    event.preventDefault(); if (state === "recording") stop(); else if (state === "idle") start();
  }
});
window.addEventListener("beforeunload", (event) => {
  if (state !== "idle") { event.preventDefault(); event.returnValue = ""; }
});
updateControls();
