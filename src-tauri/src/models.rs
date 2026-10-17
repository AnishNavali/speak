use crate::{config, engine::Core};
use anyhow::Result;
use std::{path::Path, sync::Arc};
use tauri::{Emitter, State};
use tokio_util::sync::CancellationToken;

struct File {
    name: &'static str,
    size: u64,
    sha: &'static str,
}
const Q4: File = File {
    name: "gemma-4-E2B-it-Q4_0.gguf",
    size: 2841481184,
    sha: "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52",
};
const Q8: File = File {
    name: "gemma-4-E2B-it-Q8_0.gguf",
    size: 4967497152,
    sha: "996d08777aadc6bfd3c7375ef70ba25a0f55240075860754fdb18d6d860aa63a",
};
const PROJECTOR: File = File {
    name: "mmproj-gemma-4-E2B-it-BF16.gguf",
    size: 986833664,
    sha: "711e1e8f43fa0664adbac493129be1e6c25b81af4b4cdea97c7d798b25c0a3a4",
};
const SMALL_PROJECTOR: File = File {
    name: "mmproj-gemma-4-E2B-it-Q8_0.gguf",
    size: 557368064,
    sha: "9406f99c16d68cda4f1f0552192dcc99021ea1fc6d2fd50b1dc3ccf30d04b292",
};

async fn download(core: &Core, dir: &Path, file: &File) -> Result<String> {
    let target = dir.join(file.name);
    let url = format!("https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF/resolve/b4243c156154b6dca9324415f8c7ccc098b4aed1/{}", file.name);
    crate::download::fetch(
        &url,
        file.size,
        file.sha,
        &target,
        &CancellationToken::new(),
        |received, total| {
            let _ = core.app.emit(
                "speak-download",
                serde_json::json!({"file":file.name,"received":received,"total":total}),
            );
        },
    )
    .await?;
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn download_model(
    core: State<'_, Arc<Core>>,
    preset: String,
) -> Result<config::Settings, String> {
    let _download = core.download_lock.lock().await;
    core.ensure_setup_idle()
        .map_err(|error| error.to_string())?;
    let files = match preset.as_str() {
        "q8" => (&Q8, &PROJECTOR),
        "q4" => (&Q4, &PROJECTOR),
        "q4-small" => (&Q4, &SMALL_PROJECTOR),
        _ => return Err("Unknown model preset".into()),
    };
    let dir = core.data_dir.join("models");
    let result = async {
        tokio::fs::create_dir_all(&dir).await?;
        let model = download(&core, &dir, files.0).await?;
        let projector = download(&core, &dir, files.1).await?;
        let mut settings = core.settings.lock().unwrap().clone();
        settings.model = model;
        settings.projector = projector;
        core.apply_download(settings.clone())?;
        Ok::<_, anyhow::Error>(settings)
    }
    .await;
    result.map_err(|error| format!("{error:#}"))
}
