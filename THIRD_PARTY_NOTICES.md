# Third-party components

Speak application source is MIT licensed; see LICENSE. This does not relicense
third-party software or downloaded model weights.

- **llama.cpp / ggml**: MIT, https://github.com/ggml-org/llama.cpp . The pinned
  version and archive digests are in `src-tauri/runtime-manifest.json`. Original
  license/notice files accompany each packaged or downloaded backend under
  `runtime/<backend>/licenses/` or the CUDA runtime cache's `licenses/` directory.
- **Gemma 4 E2B**: Google model license and model card:
  https://huggingface.co/google/gemma-4-E2B-it . The current model card declares
  Apache-2.0. GGUF conversions are from
  https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF . Models download separately
  from pinned revisions; app licensing does not replace model terms.
- **NVIDIA CUDA redistributables**: NVIDIA's applicable software licenses and
  redistribution conditions apply, not Speak's MIT license. Original license,
  EULA and notice files are retained from the paired pinned runtime archives.
  https://docs.nvidia.com/cuda/eula/ . GPU drivers are not distributed by Speak.
- **Rust/Tauri dependencies**: exact locked package versions and license metadata
  are generated with `cargo metadata` for every release and published as
  `dependency-licenses-<platform>.json`. See each upstream package's license for
  its full terms. Tauri and its plugins are generally MIT OR Apache-2.0.
- **WebView runtimes and operating-system frameworks** remain subject to their
  respective vendor terms. Windows installers bootstrap WebView2 if it is absent;
  the application does not include Microsoft's development SDK.

Publishers must retain the component license files when repackaging backends.
