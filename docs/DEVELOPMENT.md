# Speak development guide

Looking to install and use Speak? Start with the [user guide](../README.md).
All commands below run from the project root. This guide covers building,
implementation details, testing, and the original web prototype.

A Rust/Tauri push-to-talk desktop app, plus the original web notepad for model
experiments. One local Gemma 4 E2B model handles transcription and polishing.
The native app owns its inference process; installed users do not need Python,
Node.js, Homebrew, or a separate llama.cpp installation.

## GitHub distribution

Public release preparation and publisher configuration are documented in
[DISTRIBUTION.md](DISTRIBUTION.md). The app source is MIT licensed.
Four release targets are configured: Apple Silicon/Intel Mac, Windows x64, and
Linux x64. Windows/Linux installers contain separate CPU/Vulkan runtimes and
automatically acquire a pinned CUDA runtime for compatible NVIDIA devices.
Releases default to **unsigned beta installers**: Apple Developer membership and
Windows code-signing credentials are not required. See
[beta installation instructions](BETA_INSTALLATION.md) for OS warnings and setup.
The repository is [AnishNavali/speak](https://github.com/AnishNavali/speak).
Publishing installers requires free self-generated updater keys, a matching
version tag, and release acceptance checks. Passing build checks alone does not
publish a release.

## Native desktop app

### Build and open on this Mac

The development build is at `target/debug/bundle/macos/Speak.app`:

```sh
open target/debug/bundle/macos/Speak.app
```

To rebuild from the project root (quit Speak first):

```sh
python3 scripts/configure-macos-signing.py
python3 scripts/bundle-runtime.py
npx --yes @tauri-apps/cli@2.12.3 build --debug --bundles app
open target/debug/bundle/macos/Speak.app
```

The Python 3.12+ script is a **build-time packaging helper**, not an application runtime.
It downloads a pinned llama.cpp release (`b11429`), checks the archive's SHA-256,
and includes its executable, libraries, license, and shaders in the app resources.
Use `--backend cpu` on supported Windows/Linux x64 builds for the CPU runtime.
Without a prepared bundle, `cargo run -p speak` can use an installed `llama-server`
in development. Settings can also select an explicit compatible executable.

### Keep macOS Accessibility approval across rebuilds

Run `python3 scripts/configure-macos-signing.py` once to select an existing Apple
Development or Developer ID Application certificate. It pins that certificate in
the ignored, machine-local `src-tauri/tauri.macos.conf.json`. Tauri automatically
merges this file on subsequent macOS bundle builds, including the `npx` command
above. Other platforms and unsigned CI builds do not use your local identity.
The helper never creates/exports keys and refuses to silently select a different
certificate when the configured identity is missing. With multiple identities,
use `--identity "certificate name or SHA-1"` to choose explicitly.

Ad-hoc builds authorize a changing binary hash; certificate-signed builds use a
certificate-based code requirement and the stable `app.speak.local` identifier.
When moving from the old ad-hoc app to the signed app, approve the new identity
once: quit Speak, remove its old Accessibility entry, add the current Speak.app,
enable it, then reopen Speak and enable keyboard insertion. Microphone permission
may also need approval once for the new identity. Future builds must keep using
the same signing identity and bundle identifier. Do not delete the local config
or override `APPLE_SIGNING_IDENTITY` with a different certificate unintentionally.

Check a packaged app with:

```sh
codesign --verify --deep --strict --verbose=2 target/debug/bundle/macos/Speak.app
codesign -dr - target/debug/bundle/macos/Speak.app
SPEAK_SIGNING_TEST=1 python3 -m unittest discover -s tests -p 'test_macos_signing.py'
```

The designated requirement should contain the app identifier and certificate
conditions, not just `cdhash H"..."`. The app includes the audio-input entitlement
needed for microphone use with the hardened runtime. Local development signing
does not upload/notarize/publish the app or grant Accessibility automatically.
On macOS, startup checks existing Accessibility authorization without prompting;
when already granted, keyboard insertion initializes automatically. Test cross-app
typing with the packaged Speak.app, not the unsigned executable from `cargo run`
or a separate `tauri dev` process, which can have a different signing identity.
The opt-in signing test signs two different temporary binaries, verifies both
against the app's requirement, and checks that ad-hoc signatures fail it. The test
uses the configured local key, never runs those binaries, and deletes them afterward.

### First use

1. Choose existing Gemma GGUF and matching projector files, or select a download
   preset in Settings. Q4/BF16 is the default; development discovers those files in `models/`.
2. Start Gemma if it is not already loading. Speak uses a private local port and
   random API key, so another model server can use port 8080 independently.
3. Settings has an **Open test notepad** button; you can also open it from Speak's
   tray menu. Settings and the test notepad are separate views, and switching keeps
   the note. Hold **F5**, speak, then release. A single function key works without modifiers;
   choose another key or combination in Settings. On a Mac with media-key behavior,
   use **Fn + F5** or enable standard function keys in keyboard settings. With Speak's window
   focused with the test notepad open, text goes there. Settings itself does not
   receive dictation. In another application, text is inserted
   into the captured target field. The floating microphone is also press-and-hold;
   on macOS it is a non-activating NSPanel, so clicking it leaves the destination app active.
   Hold the **microphone icon** to record. Drag the **six-dot grip** or status label
   to reposition the floating control. Dragging is separate from recording; saving
   settings does not reset its position during the session.
   The floating UI uses a rounded glass surface with light/dark and reduced-motion/
   reduced-transparency styles. macOS 26+ uses native Liquid Glass; older macOS uses
   native popover vibrancy, Windows uses Acrylic where available, and Linux uses
   the translucent CSS fallback.
4. macOS: grant **Microphone** permission when recording and use **Enable keyboard
   insertion** to grant/check **Accessibility** permission before typing into other apps.
   Microphone setup can take a moment; wait for the Listening indicator before speaking.
5. Closing the settings/test window keeps the tray app running. Use **Quit Speak**
   from its tray menu to quit and stop its model process. Launch-at-login is opt-in.

The default runtime requires macOS 13.3+. Automatic acceleration prefers Metal
on Mac, CUDA on compatible NVIDIA Windows/Linux devices, then Vulkan and an
isolated CPU fallback. GPU setup needs a working graphics driver, not a developer
SDK. Settings shows the selected backend and offers CPU-only/Recheck controls.
The native capture path uses CoreAudio,
WASAPI, or ALSA through CPAL; it runs independently of a webview's visibility.

### Recording and recovery

- Only an active push-to-talk session opens the microphone. Release finishes capture,
  transcribes pending segments, then polishes the complete transcript once.
- Mono 16-bit WAV segments are bounded to 24 seconds, including unusually large
  audio callbacks. Low-energy pauses can close segments after four seconds. The
  pause threshold is adjustable for quiet/noisy microphones; this is not trained VAD.
- The audio callback never runs inference. A separate worker owns the microphone,
  and the asynchronous pipeline processes segments in order.
- Escape cancels while recording on macOS/Windows/X11. The indicator/tray Cancel
  action works without a global Escape binding; Escape in Speak's test window also
  cancels. Wayland uses its portal shortcut's release event, plus the Cancel controls.
- Capture ends after ten minutes. Missing audio callbacks for two seconds or a
  capture overrun stops the session with a recoverable error (including after sleep).
- On failure, pending audio and partial/raw/polished text stay in memory. Retry
  skips successfully transcribed segments. Save Audio exports all retained segments.
  A completed result can be inserted after a three-second focus-switch countdown.
  Closing the window preserves pending data; quitting the app discards pending audio.
- macOS tracks the focused Accessibility element; Windows tracks the foreground
  window/control; X11 tracks the active/input-focus windows. A changed target blocks
  insertion and retains the text. Moving the caret within the same field is not tracked.
- Some Chromium/Electron fields, including web-page editors and integrated terminals,
  return `AXErrorNoValue` instead of a focused element. macOS focus discovery reads
  `AXRole` to initialize lazy accessibility support and retries the field query once.
  If the field remains unavailable, recording proceeds with an application/window
  guard. Keep the same destination focused: this fallback cannot detect switching
  between fields inside that window. Known fields still require the same field at
  insertion; denied permission and other AX errors remain errors. If neither window
  nor field was available, text is retained for copying instead of auto-inserted.
- macOS/Windows/X11 check physical modifier release before inserting. Wayland does
  not expose equivalent global focus/key-state queries, so insertion there targets
  the **currently focused field**; keep that field focused until processing completes.
- Input uses Unicode text simulation, not manually sending an Enter/Submit key or
  replacing the system clipboard. Some applications may interpret newlines differently;
  verify target-app behavior. Target-app undo is the normal editor undo command.
- Notes/settings persist locally; raw audio is not written to disk unless exported.
  Runtime logs are in the application-data directory displayed in Settings and do
  not intentionally include prompts, transcripts, or recordings.

### Windows and Linux

`.github/workflows/native.yml` builds separate packages on macOS, Windows, and
Ubuntu 24.04 from the shared source. Build/install dependencies are platform-specific:

- Windows: MSVC C++ build tools and the Windows SDK; Tauri uses WebView2.
- Linux: WebKitGTK 4.1, GTK/AppIndicator, ALSA, xkbcommon, D-Bus, and Vulkan libraries.
  The workflow lists the Ubuntu build packages.
- Wayland shortcuts use the GlobalShortcuts portal, not the X11-only shortcut
  plugin. Input uses Enigo's **experimental libei/RemoteDesktop-portal backend**;
  the desktop must implement these portals. The permission dialog authorizes local
  input simulation, not network remote access. GNOME and KDE need separate live tests.
- Ordinary Windows applications can receive input; elevated/system applications
  have OS integrity restrictions. Secure/password fields and unusual editors may
  reject synthetic input on any platform; the result stays recoverable.

**Verification:** see [GitHub Actions](https://github.com/AnishNavali/speak/actions)
for current build/test results on all four targets. The macOS bundle has also been
run locally, including a generated-speech inference smoke check. CI success is not
evidence of real-device microphone, permissions, GPU, or insertion compatibility;
follow the acceptance checks in [DISTRIBUTION.md](DISTRIBUTION.md). Local Mac builds
can use the development signing identity described above. Public builds default
to unsigned beta distribution unless publisher signing is explicitly enabled.

### Native checks

```sh
cargo fmt --all -- --check
cargo clippy -p speak -- -D warnings
cargo test -p speak
node --check web/desktop.js
node --check web/indicator.js
node --test tests/desktop.test.mjs
```

On macOS, `cargo test` includes a real AppKit check that the microphone uses a
non-activating NSPanel which cannot become the key or main window. It does not open
the microphone or load a model. Physical click-to-dictate is still a hands-on check.

Exercise the actual Rust inference path without recording or injecting keys:

```sh
cargo run -p speak -- --smoke-test /absolute/path/to/recording.wav
```

This accepts mono 16-bit PCM WAV, up to 28 seconds, and starts/stops its own model
process using the configured model files (Q4/BF16 by default). It prints the raw and polished outputs
and elapsed inference time. The model's language, tone, correction, and emoji quality
remain model-dependent. Model downloads are resumable and SHA-256 checked; the
supported presets are Q8/BF16, Q4/BF16, and Q4/Q8-projector.

## Original web prototype

### Run

Requires Python 3.10+, a current browser with AudioWorklet support, and a llama.cpp
build supporting **Gemma 4 audio input**. Model loading alone does not verify audio
support; the first real recording will exercise that path.

If loading fails with `unknown model architecture: 'gemma4'`, upgrade llama.cpp
with `brew upgrade llama.cpp`. Homebrew build 8190 does not support this model;
the updated 0.6.0_1 package was verified with audio transcription on this Mac.

Download these files from
[ggml-org/gemma-4-E2B-it-GGUF](https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF/tree/main)
into `models/`:

- `gemma-4-E2B-it-Q8_0.gguf`
- `mmproj-gemma-4-E2B-it-BF16.gguf`

From the project directory, start Gemma in one terminal:

```sh
llama-server \
  -m models/gemma-4-E2B-it-Q8_0.gguf \
  --mmproj models/mmproj-gemma-4-E2B-it-BF16.gguf \
  -ngl 99 -c 8192 -np 1 \
  --reasoning off \
  --host 127.0.0.1 --port 8080
```

Start the app in another terminal:

```sh
python3 server.py
```

Open **http://127.0.0.1:8000**. The UI also runs before the model is available and
shows its status. Recording becomes usable when the model health check succeeds.
Allow microphone access, click **Start speaking**, then **Stop & polish**.
The shortcut is **Cmd/Ctrl + Shift + Space**, while the page is focused.

Optional server settings: `PORT=8001 python3 server.py` or
`LLAMA_URL=http://127.0.0.1:8081 python3 server.py`.

### Behavior

- Microphone audio is captured as mono PCM WAV, normally at 16 kHz. The browser
  sends complete, independently decodable WAV segments, not broken MediaRecorder slices.
- Segments end near low-energy pauses, or at about 24 seconds, below Gemma's
  documented 30-second input limit. This simple energy detector is **not a trained VAD**;
  constant noise or continuous speech can cause hard cuts mid-word. No quiet audio
  is filtered out. Silence hallucinations and segment-boundary accuracy need real
  model testing.
- Audio segments are transcribed in order while you speak. After Stop, the same
  model edits the entire assembled transcript once, so spoken corrections can cross
  segment boundaries. Thinking is disabled and temperature is set to zero.
- Original languages/scripts are requested, without translation or a language
  allowlist. Actual recognition, mixed-language accuracy, and polishing quality
  depend on Gemma and need evaluation.
- The audio transcription prompt asks Gemma to use audible intonation and pauses
  for questions, emphasis, and hesitation. That punctuation carries the delivery
  into the text-only polishing step; this is not a separate emotion classifier.
  Tone recognition is model-dependent and may miss subtle or sarcastic delivery.
- Explicit emoji cues such as “smiling emoji” and “heart emoji” become symbols during
  polishing. Existing emojis are preserved. Ordinary mentions of emojis remain words;
  emojis are not automatically added just because the speaker sounds happy or sad.
- Text is inserted at the selection saved at recording start. The note is read-only
  during dictation/processing to avoid overwriting edits. Last-insertion undo is
  available only if the note has not subsequently changed.
- Notes are saved to this browser's localStorage. Recordings and raw transcripts
  remain in memory, not on the server's disk. Refreshing/closing loses pending
  audio; the browser warns while a dictation is unfinished. On failure you can
  retry, save the full WAV, or insert raw text once **all** segments are transcribed.
- Sessions stop automatically at ten minutes to bound audio memory. The polishing
  endpoint accepts up to 8,000 characters; longer transcripts remain available for
  raw insertion/export. Model context/output limits can also reject a long edit;
  truncated output is not silently inserted.
- Only local app origins can call the app API. Bind both servers to loopback.
  No recordings/transcripts are included in application request logs.

### Checks

```sh
python3 -m unittest discover -s tests
node --test tests/*.test.mjs
node --check web/app.js
node --check web/recorder.js
```

These check WAV handling, API error/retry paths, prompt requests, truncation handling,
and local-origin restrictions against a fake llama.cpp server. They do not measure
Gemma's accuracy or speed. With both servers running, check actual Gemma editing:

```sh
SPEAK_LIVE_TEST=1 python3 -m unittest discover -s tests
```

This opt-in smoke check covers spoken date/number corrections, preserving apologies,
Hindi script, and treating dictated commands as text. The local API was also verified
with generated English speech: transcription preserved the words and polishing
resolved a Friday-to-Monday correction. This is a smoke check, not a multilingual
accuracy benchmark. Test microphone recordings, silence, mixed-language speech,
and segment boundaries next.
