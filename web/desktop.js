const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
let initialized = false, state, pendingPress, held = false, downloadingModel = false;
let insertionPoint = [0, 0];

function notice(message) { $("notice").textContent = message || ""; }
async function call(command, args = {}) {
  try { return await invoke(command, args); }
  catch (error) { notice(String(error)); throw error; }
}
function fill(settings) {
  for (const key of ["model", "projector", "executable", "microphone", "shortcut"]) $(key).value = settings[key];
  $("threshold").value = settings.pause_threshold;
  $("floating").checked = settings.floating ?? true;
  $("acceleration").value = settings.acceleration || "auto";
}
function render(next) {
  state = next;
  document.body.dataset.phase = next.phase;
  if (next.phase !== "recording") $("meter").style.setProperty("--wave-level", ".08");
  const testing = next.view === "test";
  $("test-view").hidden = !testing;
  $("settings-panel").hidden = testing;
  $("hold").hidden = !testing;
  $("open-test").setAttribute("aria-pressed", String(testing));
  $("open-settings").setAttribute("aria-pressed", String(!testing));
  $("page-title").textContent = testing ? "Try your voice." : "Set up your dictation.";
  $("model-status").textContent = next.model_status;
  $("model-status").classList.toggle("ready", next.ready);
  $("status").textContent = next.message;
  $("shortcut-label").textContent = next.shortcut;
  $("detail").textContent = next.input_ready ? "Ready for text fields in other applications" : testing ? "Test dictation here without keyboard-insertion permission" : "Enable keyboard insertion below to dictate into other apps";
  $("raw").value = next.raw;
  notice(next.notice || (next.phase === "error" ? next.message : ""));
  $("hold").disabled = !next.ready || !["idle", "starting", "recording"].includes(next.phase);
  $("hold").setAttribute("aria-pressed", String(next.phase === "recording"));
  $("hold").textContent = next.phase === "recording" ? "Release to finish" : "Hold to test";
  $("recovery").hidden = next.phase !== "error";
  $("insert").disabled = !next.result;
  $("cancel").disabled = ["idle", "updating"].includes(next.phase);
  $("acceleration-status").textContent = `Acceleration: ${next.acceleration || "Not running"}`;
  for (const id of ["save-settings", "start-model", "recheck-gpu", "check-update"]) $(id).disabled = next.phase === "updating";
  $("download-model").disabled = downloadingModel || next.phase === "updating";
  $("check-update").disabled = !next.updates_enabled || next.phase === "updating";
  if (!next.updates_enabled) $("update-status").textContent = "Updates are not configured in this development build.";
  $("note").readOnly = next.phase !== "idle";
  $("data-dir").textContent = `Local settings and runtime log: ${next.data_dir}`;
  if (!initialized) { fill(next.settings); initialized = true; }
}
function saveNote() {
  try { localStorage.setItem("speak.desktop.note", $("note").value); $("saved").textContent = "Saved on this device"; }
  catch { $("saved").textContent = "Storage unavailable — export your note"; }
}
try { $("note").value = localStorage.getItem("speak.desktop.note") || ""; } catch { /* Export is still available. */ }
$("note").addEventListener("input", saveNote);
$("open-test").onclick = () => call("open_test_ui").catch(() => {});
$("open-settings").onclick = () => call("open_settings").catch(() => {});

async function startHold(event) {
  if (held || !state?.ready || state.phase !== "idle") return;
  event.preventDefault();
  held = true;
  if (event.pointerId !== undefined) $("hold").setPointerCapture(event.pointerId);
  insertionPoint = [$("note").selectionStart, $("note").selectionEnd];
  pendingPress = call("press", { test: true });
  try { await pendingPress; } catch { held = false; }
}
async function stopHold() {
  if (!held) return;
  held = false;
  try { await pendingPress; await call("release"); } catch { /* Error already shown. */ }
}
$("hold").addEventListener("pointerdown", startHold);
$("hold").addEventListener("pointerup", stopHold);
$("hold").addEventListener("pointercancel", stopHold);
$("hold").addEventListener("lostpointercapture", stopHold);
$("hold").addEventListener("keydown", (event) => { if (["Space", "Enter"].includes(event.code) && !event.repeat) startHold(event); });
$("hold").addEventListener("keyup", (event) => { if (["Space", "Enter"].includes(event.code)) stopHold(); });
window.addEventListener("blur", stopHold);
document.addEventListener("keydown", (event) => { if (event.code === "Escape") invoke("cancel").catch(notice); });
$("cancel").onclick = () => call("cancel").catch(() => {});
$("retry").onclick = () => call("retry").catch(() => {});
$("discard").onclick = () => { if (confirm("Discard the pending recording?")) call("discard").catch(() => {}); };
$("insert").onclick = () => call("insert_result").catch(() => {});
$("enable-input").onclick = () => call("enable_input").catch(() => {});
$("start-model").onclick = () => call("start_model").catch(() => {});
$("recheck-gpu").onclick = () => call("recheck_acceleration").catch(() => {});
$("save-settings").onclick = async () => {
  const settings = Object.fromEntries(["model", "projector", "executable", "microphone", "shortcut"].map((key) => [key, $(key).value]));
  settings.pause_threshold = Number($("threshold").value);
  settings.floating = $("floating").checked;
  settings.acceleration = $("acceleration").value;
  try { await call("save_settings", { settings }); notice("Settings saved. Restart Gemma if you changed model files."); } catch { /* Already shown. */ }
};
for (const button of document.querySelectorAll("[data-pick]")) button.onclick = async () => {
  const key = button.dataset.pick;
  const path = await window.__TAURI__.dialog.open({ multiple: false, directory: false,
    ...(key !== "executable" ? { filters: [{ name: "GGUF model", extensions: ["gguf"] }] } : {}) });
  if (typeof path === "string") $(key).value = path;
};
$("autostart").onchange = async () => {
  try { await call("set_autostart", { enabled: $("autostart").checked }); }
  catch { $("autostart").checked = !$("autostart").checked; }
};
$("download-model").onclick = async () => {
  downloadingModel = true;
  $("download-model").disabled = true;
  $("download-status").textContent = "Downloading / verifying model files…";
  try {
    const settings = await call("download_model", { preset: $("preset").value });
    fill(settings); $("download-status").textContent = "Files verified. Starting local inference…";
    await call("start_model");
    $("download-status").textContent = "Setup complete. Gemma is ready.";
  } catch { $("download-status").textContent = "Setup incomplete. Downloaded files are retained; check the status above and retry."; }
  finally { downloadingModel = false; $("download-model").disabled = state?.phase === "updating"; }
};
$("check-update").onclick = async () => {
  $("check-update").disabled = true;
  $("update-status").textContent = "Checking release channel…";
  try {
    const update = await call("check_update");
    $("install-update").disabled = !update;
    $("update-status").textContent = update ? `Version ${update.version} is available.${update.notes ? " " + update.notes : ""}` : "You are up to date.";
  } catch (error) { $("update-status").textContent = String(error); }
  finally { $("check-update").disabled = false; }
};
$("install-update").onclick = async () => {
  if (!confirm("Install the verified update and restart Speak? Finish your dictation first.")) return;
  $("install-update").disabled = true;
  try { await call("install_update"); }
  catch (error) { $("update-status").textContent = String(error); $("install-update").disabled = false; }
};
async function copy(text) { try { await navigator.clipboard.writeText(text); notice("Copied to clipboard"); } catch { notice("Select the text and copy it with your keyboard."); } }
$("copy").onclick = () => copy($("note").value);
$("copy-raw").onclick = () => copy(state.raw);
$("save-audio").onclick = async () => {
  const path = await window.__TAURI__.dialog.save({ defaultPath: "speak-recording.wav", filters: [{ name: "WAV audio", extensions: ["wav"] }] });
  if (path) call("save_audio", { path }).catch(() => {});
};
$("export").onclick = async () => {
  const blob = new Blob([$("note").value], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob); const link = document.createElement("a");
  link.href = url; link.download = "speak-note.txt"; link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
};

await listen("speak-state", ({ payload }) => {
  if (payload.phase === "starting" && state?.phase === "idle") insertionPoint = [$("note").selectionStart, $("note").selectionEnd];
  render(payload);
});
await listen("speak-meter", ({ payload }) => {
  if (state?.phase === "recording") $("meter").style.setProperty("--wave-level", String(Math.min(1, Math.max(.08, payload.level * 12))));
  $("timer").textContent = `${String(Math.floor(payload.seconds / 60)).padStart(2, "0")}:${String(payload.seconds % 60).padStart(2, "0")}`;
});
await listen("speak-test-result", ({ payload }) => {
  if (state.session_id !== payload.id) return;
  const note = $("note"), [start, end] = insertionPoint;
  const left = note.value.slice(0, start), right = note.value.slice(end);
  const prefix = left && !/\s$/u.test(left) ? " " : "";
  const suffix = right && !/^\s/u.test(right) ? " " : "";
  note.value = left + prefix + payload.text + suffix + right;
  const cursor = left.length + prefix.length + payload.text.length;
  note.setSelectionRange(cursor, cursor); saveNote();
});
await listen("speak-download", ({ payload }) => {
  $("download-status").textContent = `${payload.file}: ${(payload.received / payload.total * 100).toFixed(1)}%`;
});
await listen("speak-update", ({ payload }) => {
  $("update-status").textContent = payload.total ? `Downloading update: ${(payload.received / payload.total * 100).toFixed(0)}%` : `Downloading update: ${(payload.received / 1024 / 1024).toFixed(1)} MB`;
});
try {
  render(await call("snapshot"));
  const devices = await call("microphones");
  for (const device of devices) { const option = document.createElement("option"); option.value = option.textContent = device; $("microphone").append(option); }
  $("microphone").value = state.settings.microphone;
  $("autostart").checked = await call("get_autostart");
} catch { /* Setup errors stay visible without a white-screen failure. */ }
