# Distribution and release operations

## Current status

The source includes four-target build workflows, isolated GPU/CPU runtimes,
automatic CUDA setup, verified resumable downloads, and a signed updater. This is
release infrastructure, not evidence that every GPU/desktop combination works.
The source repository is [AnishNavali/speak](https://github.com/AnishNavali/speak).
See [Actions](https://github.com/AnishNavali/speak/actions) for current checks and
[Releases](https://github.com/AnishNavali/speak/releases) for published installers.
Passing native checks creates workflow artifacts, not a public release.

Public-release prerequisites still owned by the publisher:

1. Configure the updater signing key pair and the public key repository variable.
2. Complete the real-device compatibility tests below before publishing the draft.
3. Push a matching version tag, then review and publish the generated draft release.

**Apple and Windows publisher signing are optional.** The initial release workflow
defaults to unsigned beta installers. Mac bundles and their native helpers use
ad-hoc signatures without a certificate, keychain import, or notarization; Windows
installers have no publisher signature. Linux packages need neither credential.
Updater signing remains mandatory and uses a free, self-generated key pair.

Leave the repository variable `SPEAK_OS_SIGNING` unset (or set it to `false`) for
this initial mode. Later, supply the Apple/Azure configuration below and set
`SPEAK_OS_SIGNING` to exactly `true`. That explicitly enables publisher signing;
missing credentials then fail the build instead of falling back to unsigned.
Keep the bundle identifier and updater key pair stable when making that switch.

Unsigned releases include [beta installation instructions](BETA_INSTALLATION.md)
in their draft notes. macOS/Windows installation warnings are expected. Ad-hoc Mac
updates can require Accessibility reapproval, as can changing to Developer ID
signing later. Models and settings use the same app-data locations in both modes.

## Targets and assets

| Rust target | User download | Updater target |
|---|---|---|
| `aarch64-apple-darwin` | Apple Silicon DMG | `darwin-aarch64` |
| `x86_64-apple-darwin` | Intel Mac DMG | `darwin-x86_64` |
| `x86_64-pc-windows-msvc` | Per-user NSIS setup EXE | `windows-x86_64` |
| `x86_64-unknown-linux-gnu` | AppImage, DEB, RPM | `linux-x86_64` |

User-facing installers have stable names so the README links keep working across
versions:

```text
Speak-macOS-AppleSilicon.dmg
Speak-macOS-Intel.dmg
Speak-Windows-x64-Setup.exe
Speak-Linux-x64.AppImage
Speak-Linux-x64.deb
Speak-Linux-x64.rpm
```

The README links to `releases/latest/download/<filename>`. These links become live
when the first release containing these assets is published; update the README's
first-release notice at that point. Upload installers as release assets, not Git
source files. No duplicate installer aliases are generated. macOS updater archives
keep versioned names; every updater URL still points to its exact version tag,
and signatures bind the app version. Do not use mutable `latest` URLs in `latest.json`.

Rust and sidecar targets are explicit; cross-building Intel Mac on an ARM runner
must use the Intel llama.cpp archive, not the runner's architecture. Linux's first
build baseline is Ubuntu 24.04. Audit glibc/WebKit/system-library requirements on
each advertised distro; AppImage does not eliminate these requirements.

App sources are MIT licensed. Keep `LICENSE`, `THIRD_PARTY_NOTICES.md`, original
backend license/EULA files, and generated dependency-license metadata in releases.
Model and NVIDIA software terms remain separate from the application license.

## Automatic acceleration

Packaged backends live in separate directories, never a merged DLL/SO directory:

```text
app resources/runtime/metal/        # Mac: GPU plus explicit CPU execution
app resources/runtime/cpu/          # Windows/Linux: driver-independent fallback
app resources/runtime/vulkan/       # Windows/Linux: NVIDIA/AMD/Intel where supported
app data/runtimes/b11429/cuda/       # Downloaded only for a CUDA-capable NVIDIA driver
```

`scripts/bundle-runtime.py --target <Rust target>` stages Metal on Mac and both
CPU/Vulkan on Windows/Linux. It verifies the archives and replaces only its own
marked backend directories. Start from a clean checkout for release packaging;
old development flat-runtime files are not part of the new layout.

The trusted manifest is compiled from `src-tauri/runtime-manifest.json`. All
backends use pinned llama.cpp **b11429**. CUDA uses the upstream matched binary and
redistributable archives: Windows CUDA 12.4, Linux CUDA 12.8. CUDA is downloaded
from the official ggml-org GitHub release, not from an unconfigured project URL.
The CUDA Toolkit, Vulkan SDK, GPU drivers, Rust, Python, and Node are not end-user
requirements. A compatible installed graphics driver is required for GPU use.

Startup:

1. Mac tries Metal. Windows/Linux checks NVIDIA's installed driver through a
   short-lived child process using `nvcuda.dll`/`libcuda.so.1`; it does not depend
   on `nvidia-smi` or a developer SDK. Driver API 12+ is only a candidate check,
   not a guarantee that every kernel/GPU is supported.
2. For a compatible NVIDIA candidate, download/verify/resume CUDA archives,
   extract into managed staging, preserve licenses, then atomically install the
   normalized cache. A download failure continues to Vulkan/CPU.
3. Enumerate local devices with the chosen runtime; remote RPC devices are ignored.
4. Load with automatic layer fitting and a memory reserve, then exercise **audio
   encoder + text decoder** using synthetic local audio. Try a CPU audio projector
   and reduced GPU offload if necessary, then move to the next backend/device.
5. Use explicit CPU execution with decoder and projector offload disabled if GPU
   attempts fail. The synthetic startup probe tests operation support, not ASR quality.

Settings displays the actual selected acceleration and fallback details. CPU-only
mode is available; Recheck acceleration resets session recovery and re-probes.
Selection is revalidated at startup rather than permanently trusting a cached
hardware result. A runtime crash stops capture, retains captured segments, and
attempts one CPU recovery/retry after the recording has stopped. Focus checks still
apply before recovered text is inserted. Repeated CPU failure remains recoverable
in Settings instead of becoming a restart loop.

GPU enumeration and startup have separate timeouts. CPU fallback cannot repair
insufficient system RAM or a corrupt/incompatible model. Integrated GPUs share RAM
and may be slower than a CPU; publish measured results rather than assuming GPU
presence implies a speedup. Validate with the actual Gemma GGUF/projector pair,
not just `--list-devices` or a text-only model.

## First-run downloads and offline use

The installer includes the app and baseline runtimes, not multi-GB model weights.
Users choose a verified model preset or import existing model/projector files.
Download/resume also verifies SHA-256; setup starts local inference after success.
Network is used for first-run models/CUDA and user-requested updates. Dictation is
loopback-only and works offline after setup. Interrupted downloads retain `.part`
files. Models and runtime caches live in app data, not the signed app bundle, and
are reused across updates.

Q4/BF16 is the smaller default preset; Q8/BF16 and Q4/Q8-projector remain available.
Run the quality checks on the chosen default before recommending it publicly.

## GitHub configuration

The repository's real `owner/name` comes from `github.repository` in release jobs.
Local builds without a repository/public key have updates explicitly disabled—no
placeholder URL is embedded.

### Updater

Generate a Tauri updater key pair locally using the pinned CLI, keep the private
key outside the repository, and back it up. OS signing certificates and updater
keys are different things. Configure:

- Repository variable `SPEAK_UPDATER_PUBLIC_KEY`: public key **contents**.
- Secret `TAURI_SIGNING_PRIVATE_KEY`: private updater key contents.
- Secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password if encrypted.

The app compiles `SPEAK_RELEASE_REPOSITORY` and `SPEAK_UPDATER_PUBLIC_KEY` from the
build environment. Only the public key enters the binary. Check for updates is
user-requested and reads `releases/latest/download/latest.json` over HTTPS.
The updater verifies signatures before installation, checks artifact repository
URLs, preserves model caches, and refuses to install while dictation is pending.
`createUpdaterArtifacts: true` uses raw Windows installers/Linux AppImage and the
Mac app archive; legacy ZIP-updater compatibility is not enabled.

### Optional Apple public signing (later)

Only needed when `SPEAK_OS_SIGNING=true`. Use a **Developer ID Application**
certificate and notarization credentials, not the local Apple Development identity.

Repository variable:
- `APPLE_SIGNING_IDENTITY`: full **Developer ID Application** certificate name.

Secrets:
- `APPLE_CERTIFICATE`: base64 P12 certificate/key supplied by the publisher.
- `APPLE_CERTIFICATE_PASSWORD`: export password.
- `KEYCHAIN_PASSWORD`: password for the ephemeral runner keychain.
- `APPLE_API_KEY`, `APPLE_API_ISSUER`: notarization key ID and issuer.
- `APPLE_API_KEY_P8`: notarization key contents.

The CI-only import helper never operates on a local developer keychain. It creates
a temporary keychain, imports supplied credentials, writes the P8 with owner-only
permissions, and removes credentials at job completion. Native helpers/libraries
are signed before the app is sealed. Tauri notarizes/staples; release jobs verify
codesign, stapler, and Gatekeeper assessment. Missing notarization is a failure.

Personal `src-tauri/tauri.macos.conf.json` stays ignored. Keep using the local
development signing setup for local tests; CI overrides it with public credentials.

### Optional Windows public signing (later)

Only needed when `SPEAK_OS_SIGNING=true`.

Configure an Azure Artifact Signing account/certificate profile and its GitHub
OIDC authorization. Release jobs use the GitHub environment named `release`, so
the federation subject stays stable across tags: `repo:` followed by your actual
`owner/repository` followed by `:environment:release`. Use the Azure token-exchange
audience `api://AzureADTokenExchange` and grant signing access to that identity.
Supply repository/environment variables:

- `AZURE_SIGNING_CLIENT_ID`, `AZURE_SIGNING_TENANT_ID`, `AZURE_SIGNING_SUBSCRIPTION_ID`
- `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE`

The workflow signs the compiled EXE and runtime libraries before bundling, then
signs the final NSIS installer and checks Authenticode validity. It regenerates
the Tauri update signature **after** final Authenticode signing so the `.sig`
matches the bytes users download. A newly signed application can still trigger
SmartScreen reputation warnings. Do not advertise signing as an instant bypass.

## Build and release flow

`.github/workflows/native.yml` runs unsigned checks/packages for PRs/main and has
no signing secrets. `.github/workflows/release.yml` runs on a version tag, builds
all four targets, and creates a **draft** only after every job succeeds. By default
the title identifies it as an **unsigned beta**, with installation instructions in
the release notes. It never publishes a partial update manifest or silently skips
publisher signing after `SPEAK_OS_SIGNING=true` has explicitly enabled it.

The updater reads GitHub's latest published, non-prerelease release. Publish the
draft as a normal release for that updater to discover it; marking it as a GitHub
prerelease excludes it from the current channel even if its title says “beta.”

Before tagging, align `src-tauri/Cargo.toml`, `tauri.conf.json`, and `Cargo.lock`.
The tag must be `v` plus that exact version. The release helper renames assets by
OS/architecture, signs final update payloads, generates license metadata/checksums,
and merges a complete four-platform `latest.json`. Every GitHub asset must be less
than 2 GiB. Keep model weights on their pinned model host.

Local packaging/check commands:

```sh
python3 scripts/bundle-runtime.py --target aarch64-apple-darwin
cargo fmt --all -- --check
cargo clippy --locked -p speak -- -D warnings
cargo test --locked -p speak
python3 -m unittest discover -s tests
node --test tests/*.test.mjs
```

`python3 scripts/release.py configure --public --target <target>` prepares an
unsigned beta with signed updates. It requires the actual repository and updater
keys, but no Apple/Azure credentials. Add `--os-signing` only when those credentials
are ready. Generated overrides live in `target/release-config.json`; they do not
modify the base configuration or the ignored local development signing identity.
Normal signed development builds keep using the existing local configuration.

## Acceptance before publishing the draft

Record results for clean Windows/Linux installations without developer tools;
NVIDIA CUDA and forced Vulkan fallback; AMD and Intel Vulkan; no usable driver;
low VRAM and partial offload; hybrid laptops; interrupted CUDA/model downloads;
and runtime crashes with retained dictation. Check complete audio transcription
and polishing, cold/warm latency, memory use, and Unicode/emoji insertion.

Separately validate Mac Apple Silicon/Intel, Windows permissions/insertion,
GNOME Wayland, KDE Wayland, and X11. The Wayland input backend remains experimental
until tested on the advertised desktops. Test upgrading an installed beta:
model caches/settings survive, updater signatures are verified, and any required
Mac Accessibility reapproval is documented. After enabling Developer ID signing,
also test authorization across updates with the same publisher/bundle identifier.
Ordinary hosted CI builds do not establish this hardware/desktop coverage.
Real-device testing remains a release prerequisite; publisher signing is optional.
