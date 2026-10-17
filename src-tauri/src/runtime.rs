use crate::{config::Settings, runtime_assets};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Client;
use serde_json::json;
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Runtime {
    child: Arc<Mutex<Child>>,
    pub url: String,
    pub key: String,
    pub acceleration: String,
    pub notes: Vec<String>,
}

#[derive(Default)]
struct OwnedProcesses {
    closing: bool,
    children: Vec<Weak<Mutex<Child>>>,
}
fn owned_processes() -> &'static Mutex<OwnedProcesses> {
    static PROCESSES: OnceLock<Mutex<OwnedProcesses>> = OnceLock::new();
    PROCESSES.get_or_init(|| Mutex::new(OwnedProcesses::default()))
}
pub fn stop_owned_processes() {
    let mut processes = owned_processes().lock().unwrap();
    processes.closing = true;
    for child in processes.children.iter().filter_map(Weak::upgrade) {
        let mut child = child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
    processes.children.clear();
}

#[derive(Clone, Debug)]
struct Candidate {
    executable: PathBuf,
    backend: String,
    device: Option<String>,
    layers: String,
    projector_gpu: bool,
}

fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.stdin(Stdio::null());
    if let Some(parent) = executable.parent().filter(|p| !p.as_os_str().is_empty()) {
        command.current_dir(parent);
        #[cfg(target_os = "linux")]
        if let Ok(paths) = std::env::join_paths([parent.to_owned(), parent.join("lib")]) {
            command.env("LD_LIBRARY_PATH", paths);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

pub fn parse_devices(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (id, name) = line.trim().split_once(':')?;
            if !["MTL", "Metal", "CUDA", "Vulkan"]
                .iter()
                .any(|prefix| id.starts_with(prefix))
                || !id.bytes().all(|v| v.is_ascii_alphanumeric())
            {
                return None;
            }
            Some((id.to_owned(), name.trim().to_owned()))
        })
        .collect()
}

async fn devices(executable: &Path, cancel: &CancellationToken) -> Result<Vec<(String, String)>> {
    let mut cmd = tokio::process::Command::from(command(executable));
    cmd.arg("--list-devices").kill_on_drop(true);
    let output = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(20), cmd.output()) => result.context("GPU enumeration timed out")??,
        _ = cancel.cancelled() => bail!("Model startup cancelled"),
    };
    if !output.status.success() {
        bail!(
            "GPU runtime could not enumerate devices ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(1200)
                .collect::<String>()
        );
    }
    let mut list = parse_devices(&format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));
    // Prefer recognizable discrete adapters on hybrid laptops; the full audio probe is authoritative.
    list.sort_by_key(|(_, name)| {
        let name = name.to_lowercase();
        !["geforce", "rtx", "radeon rx", "arc(tm) a", "arc a", "arc b"]
            .iter()
            .any(|v| name.contains(v))
    });
    list.dedup_by(|a, b| a.0 == b.0);
    Ok(list)
}

fn find_runtime(resources: &Path, backend: &str) -> Option<PathBuf> {
    let name = runtime_assets::executable_name();
    let mut roots = vec![resources.join("runtime")];
    if cfg!(debug_assertions) {
        roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime"));
    }
    roots
        .iter()
        .map(|root| root.join(backend).join(name))
        .find(|path| path.is_file())
        .or_else(|| {
            if cfg!(debug_assertions) {
                roots
                    .iter()
                    .map(|root| root.join(name))
                    .find(|path| path.is_file())
            } else {
                None
            }
        })
}

fn start_candidate(settings: &Settings, data_dir: &Path, candidate: &Candidate) -> Result<Runtime> {
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let key = Uuid::new_v4().to_string();
    fs::create_dir_all(data_dir)?;
    let log = fs::File::create(data_dir.join("llama.log"))?;
    let mut cmd = command(&candidate.executable);
    cmd.args([
        "-m",
        &settings.model,
        "--mmproj",
        &settings.projector,
        "--gpu-layers",
        &candidate.layers,
        "--device",
        candidate.device.as_deref().unwrap_or("none"),
        "--fit",
        "on",
        "--fit-target",
        "1024",
        "-c",
        "8192",
        "-np",
        "1",
        "--reasoning",
        "off",
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--api-key",
        &key,
        "--no-webui",
        "--log-verbosity",
        "1",
    ]);
    if !candidate.projector_gpu {
        cmd.arg("--no-mmproj-offload");
    }
    let mut processes = owned_processes().lock().unwrap();
    if processes.closing {
        bail!("Application is shutting down");
    }
    let child = cmd
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
        .context("Could not launch packaged llama-server")?;
    let child = Arc::new(Mutex::new(child));
    processes.children.retain(|child| child.strong_count() > 0);
    processes.children.push(Arc::downgrade(&child));
    Ok(Runtime {
        child,
        url: format!("http://127.0.0.1:{port}"),
        key,
        acceleration: candidate
            .device
            .as_ref()
            .map(|device| format!("{} · {device}", candidate.backend.to_uppercase()))
            .unwrap_or_else(|| "CPU".into()),
        notes: vec![],
    })
}

impl Runtime {
    pub async fn launch(
        settings: &Settings,
        data_dir: &Path,
        resources: &Path,
        cancel: &CancellationToken,
        progress: &(impl Fn(String) + Send + Sync),
    ) -> Result<Self> {
        crate::config::validate(settings)?;
        let mut notes = Vec::new();
        let mut gpu_paths = Vec::new();
        if settings.acceleration != "cpu" {
            if !settings.executable.is_empty() {
                gpu_paths.push(("custom", PathBuf::from(&settings.executable)));
            } else if cfg!(target_os = "macos") {
                if let Some(path) = find_runtime(resources, "metal") {
                    gpu_paths.push(("metal", path));
                }
            } else {
                progress("Checking NVIDIA driver…".into());
                if runtime_assets::cuda_driver_available(cancel).await {
                    match runtime_assets::ensure_cuda(data_dir, cancel, progress).await {
                        Ok(path) => gpu_paths.push(("cuda", path)),
                        Err(error) => notes.push(format!("CUDA setup: {error:#}")),
                    }
                }
                if let Some(path) = find_runtime(resources, "vulkan") {
                    gpu_paths.push(("vulkan", path));
                }
            }
        }
        for (backend, executable) in gpu_paths {
            if cancel.is_cancelled() {
                bail!("Model startup cancelled");
            }
            progress(format!("Checking {backend} devices…"));
            let list = match devices(&executable, cancel).await {
                Ok(list) => list,
                Err(error) => {
                    notes.push(format!("{backend}: {error:#}"));
                    continue;
                }
            };
            for (device, name) in list.into_iter().take(3) {
                for (layers, projector_gpu) in [("auto", true), ("auto", false), ("8", false)] {
                    progress(format!(
                        "Loading Gemma · {backend} · {name}{}",
                        if projector_gpu {
                            ""
                        } else {
                            " · CPU audio encoder"
                        }
                    ));
                    let candidate = Candidate {
                        executable: executable.clone(),
                        backend: backend.into(),
                        device: Some(device.clone()),
                        layers: layers.into(),
                        projector_gpu,
                    };
                    let attempt = async {
                        let mut runtime = start_candidate(settings, data_dir, &candidate)?;
                        runtime.ready(cancel).await?;
                        runtime.probe(cancel).await?;
                        Ok::<_, anyhow::Error>(runtime)
                    }
                    .await;
                    match attempt {
                        Ok(mut runtime) => {
                            runtime.acceleration = format!(
                                "{} · {}{}",
                                backend.to_uppercase(),
                                name,
                                if projector_gpu {
                                    ""
                                } else {
                                    " · CPU audio encoder"
                                }
                            );
                            runtime.notes = notes;
                            return Ok(runtime);
                        }
                        Err(error) => {
                            let log = data_dir.join(format!(
                                "llama-{backend}-{device}-{layers}-{}.log",
                                if projector_gpu {
                                    "gpu-audio"
                                } else {
                                    "cpu-audio"
                                }
                            ));
                            let _ = fs::copy(data_dir.join("llama.log"), &log);
                            notes.push(format!(
                                "{backend}/{device}/{layers}: {error:#} · {}",
                                log.display()
                            ));
                        }
                    }
                }
            }
        }
        if cancel.is_cancelled() {
            bail!("Model startup cancelled");
        }
        progress("Loading Gemma · CPU fallback…".into());
        let executable = find_runtime(resources, "cpu").or_else(|| if cfg!(target_os = "macos") { find_runtime(resources, "metal") } else { None })
            .or_else(|| (!settings.executable.is_empty()).then(|| PathBuf::from(&settings.executable)))
            .context("Packaged CPU runtime is missing. Reinstall Speak or select a compatible executable in advanced settings.")?;
        let candidate = Candidate {
            executable,
            backend: "cpu".into(),
            device: None,
            layers: "0".into(),
            projector_gpu: false,
        };
        let mut runtime = start_candidate(settings, data_dir, &candidate)?;
        runtime.ready(cancel).await?;
        runtime.probe(cancel).await?;
        runtime.notes = notes;
        Ok(runtime)
    }

    async fn ready(&mut self, cancel: &CancellationToken) -> Result<()> {
        let client = Client::builder().timeout(Duration::from_secs(2)).build()?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        loop {
            if self.exited()? {
                bail!("Model process exited during startup; see llama.log");
            }
            let check = async {
                if let Ok(response) = client
                    .get(format!("{}/health", self.url))
                    .bearer_auth(&self.key)
                    .send()
                    .await
                {
                    if response.status().is_success() {
                        return true;
                    }
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
                false
            };
            tokio::select! {
                ready = check => if ready { return Ok(()); },
                _ = tokio::time::sleep_until(deadline) => bail!("Model startup exceeded 120 seconds; see llama.log"),
                _ = cancel.cancelled() => bail!("Model startup cancelled"),
            }
        }
    }

    async fn probe(&self, cancel: &CancellationToken) -> Result<()> {
        // Synthetic audio never leaves loopback. Exercises encoder + decoder without capturing the user.
        let audio = crate::inference::wav(&vec![0.0001; 16000], 16000)?;
        let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
        for content in [
            json!([{"type":"text","text":"Transcribe."},{"type":"input_audio","input_audio":{"data":STANDARD.encode(&audio),"format":"wav"}}]),
            json!("Reply OK."),
        ] {
            let request = async {
                let response = client.post(format!("{}/v1/chat/completions", self.url)).bearer_auth(&self.key)
                    .json(&json!({"messages":[{"role":"user","content":content}],"max_tokens":1,"temperature":0,"stream":false,"chat_template_kwargs":{"enable_thinking":false}})).send().await?;
                if !response.status().is_success() {
                    let status = response.status();
                    let detail = response.text().await.unwrap_or_default();
                    bail!(
                        "Audio/text inference probe failed ({status}): {}",
                        detail.chars().take(1200).collect::<String>()
                    );
                }
                let value: serde_json::Value = response.json().await?;
                if value.pointer("/choices/0/message").is_none() {
                    bail!("Inference probe returned no completion");
                }
                Ok::<_, anyhow::Error>(())
            };
            tokio::select! { result = request => result?, _ = cancel.cancelled() => bail!("Model startup cancelled") }
        }
        Ok(())
    }

    pub fn exited(&mut self) -> Result<bool> {
        Ok(self.child.lock().unwrap().try_wait()?.is_some())
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_local_accelerators_are_selected() {
        let output = "Available devices:\nMTL0: Apple M4 (16384 MiB)\nVulkan1: AMD Radeon RX\nCUDA0: NVIDIA GeForce\nRPC0: remote host\nggml_vulkan: driver log\n";
        assert_eq!(
            parse_devices(output)
                .iter()
                .map(|v| v.0.as_str())
                .collect::<Vec<_>>(),
            ["MTL0", "Vulkan1", "CUDA0"]
        );
        assert!(parse_devices("CUDA0;rm: invalid").is_empty());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_gpu_reaches_isolated_cpu_and_probes_audio_then_text() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let cpu = root.join("runtime/cpu");
        fs::create_dir_all(&cpu).unwrap();
        let custom = root.join("custom");
        fs::create_dir_all(&custom).unwrap();
        let script = r#"#!/usr/bin/env python3
import http.server,json,pathlib,sys
args=sys.argv
def value(key): return args[args.index(key)+1]
if '--list-devices' in args:
 print('Vulkan0: Fake GPU (8192 MiB)'); raise SystemExit()
if value('--device')!='none': raise SystemExit(42)
assert value('--gpu-layers')=='0' and '--no-mmproj-offload' in args
probes=[]
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args): pass
 def do_GET(self):
  self.send_response(200);self.end_headers();self.wfile.write(b'{"status":"ok"}')
 def do_POST(self):
  assert self.headers.get('Authorization')=='Bearer '+value('--api-key')
  data=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  content=data['messages'][0]['content']
  probes.append('audio' if isinstance(content,list) else 'text')
  pathlib.Path(__file__).with_name('probes.json').write_text(json.dumps(probes))
  self.send_response(200);self.end_headers()
  self.wfile.write(b'{"choices":[{"message":{"content":"OK"},"finish_reason":"length"}]}')
http.server.HTTPServer(('127.0.0.1',int(value('--port'))),Handler).serve_forever()
"#;
        for path in [cpu.join("llama-server"), custom.join("llama-server")] {
            fs::write(&path, script).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let model = root.join("model.gguf");
        fs::write(&model, b"GGUFfixture").unwrap();
        let mut settings = Settings::default();
        settings.model = model.to_string_lossy().into_owned();
        settings.projector = settings.model.clone();
        settings.executable = custom.join("llama-server").to_string_lossy().into_owned();
        let runtime = Runtime::launch(
            &settings,
            &root.join("data"),
            root,
            &CancellationToken::new(),
            &|_| {},
        )
        .await
        .unwrap();
        assert_eq!(runtime.acceleration, "CPU");
        assert!(!runtime.notes.is_empty());
        assert_eq!(
            fs::read_to_string(cpu.join("probes.json")).unwrap(),
            "[\"audio\", \"text\"]"
        );
        let port = runtime.url.strip_prefix("http://").unwrap().to_owned();
        drop(runtime);
        assert!(std::net::TcpStream::connect(port).is_err());
    }
}
