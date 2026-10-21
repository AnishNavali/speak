const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const button = document.getElementById("mic"), label = document.getElementById("status");
let held = false, pending;
button.onpointerdown = (event) => {
  if (held || button.disabled) return;
  event.preventDefault(); held = true; button.setPointerCapture(event.pointerId);
  pending = invoke("press", { test: false });
  pending.catch((error) => { held = false; label.textContent = "Unable to record"; label.title = String(error); });
};
async function release() {
  if (!held) return;
  held = false;
  try { await pending; await invoke("release"); } catch { /* Error shown by state. */ }
}
button.onpointerup = release; button.onpointercancel = release; button.onlostpointercapture = release;
const closeButton = document.getElementById("cancel");
closeButton.onclick = async () => {
  if (closeButton.disabled) return;
  closeButton.disabled = true;
  held = false;
  try { await pending; } catch { /* Close even if starting capture failed. */ }
  try { await invoke("close_indicator"); }
  catch (error) { label.textContent = "Unable to close"; label.title = String(error); }
  finally { closeButton.disabled = false; }
};
function render(state) {
  document.body.dataset.phase = state.phase;
  if (state.phase !== "recording") document.getElementById("waveform").style.setProperty("--wave-level", ".08");
  button.disabled = !state.ready || !["idle", "starting", "recording"].includes(state.phase);
  button.setAttribute("aria-pressed", String(state.phase === "recording"));
  const starting = /^(Loading|Checking|Setting up)/.test(state.model_status || "");
  const text = state.phase === "idle" ? state.ready ? "Ready · hold mic" : starting ? "Starting…" : "Model offline" : state.phase;
  if (label.textContent !== text) label.textContent = text;
  label.title = state.ready ? state.notice || state.message : state.model_status;
}
document.addEventListener("pointermove", (event) => {
  document.body.style.setProperty("--shine-x", `${event.clientX}px`);
  document.body.style.setProperty("--shine-y", `${event.clientY}px`);
});
await listen("speak-state", ({ payload }) => render(payload));
await listen("speak-meter", ({ payload }) => {
  if (document.body.dataset.phase === "recording") document.getElementById("waveform").style.setProperty("--wave-level", String(Math.min(1, Math.max(.08, payload.level * 12))));
});
render(await invoke("snapshot"));
