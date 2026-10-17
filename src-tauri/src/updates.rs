use crate::engine::Core;
use anyhow::{bail, Result};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

#[derive(Default)]
struct Pending(Mutex<Option<tauri_plugin_updater::Update>>);
#[derive(Serialize)]
pub struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

pub fn valid_repository(repository: &str) -> bool {
    let parts = repository.split('/').collect::<Vec<_>>();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && ![".", ".."].contains(part)
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
}

fn channel() -> Result<(&'static str, &'static str)> {
    let repo = option_env!("SPEAK_RELEASE_REPOSITORY").unwrap_or("");
    let key = option_env!("SPEAK_UPDATER_PUBLIC_KEY").unwrap_or("");
    if !valid_repository(repo) || key.trim().is_empty() {
        bail!("This build has no configured release channel. The publisher must set SPEAK_RELEASE_REPOSITORY and SPEAK_UPDATER_PUBLIC_KEY when building.");
    }
    Ok((repo, key))
}

pub fn configured() -> bool {
    channel().is_ok()
}

pub fn verify_artifact(file: &std::path::Path, signature: &std::path::Path) -> Result<()> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let (_, public_key) = channel()?;
    let key = String::from_utf8(STANDARD.decode(public_key.trim())?)?;
    let signed = String::from_utf8(STANDARD.decode(std::fs::read_to_string(signature)?.trim())?)?;
    let key = minisign_verify::PublicKey::decode(&key)?;
    let signature = minisign_verify::Signature::decode(&signed)?;
    key.verify(&std::fs::read(file)?, &signature, true)?;
    if !signature
        .trusted_comment()
        .split('\t')
        .any(|field| field == concat!("version:", env!("CARGO_PKG_VERSION")))
    {
        bail!("Updater signature does not bind this application version");
    }
    Ok(())
}

pub fn configure(app: &AppHandle) -> Result<()> {
    app.manage(Pending::default());
    if let Ok((_, key)) = channel() {
        app.plugin(tauri_plugin_updater::Builder::new().pubkey(key).build())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let result = async {
        let (repo, _) = channel()?;
        let endpoint =
            format!("https://github.com/{repo}/releases/latest/download/latest.json").parse()?;
        let update = app
            .updater_builder()
            .endpoints(vec![endpoint])?
            .timeout(std::time::Duration::from_secs(20))
            .build()?
            .check()
            .await?;
        if let Some(ref value) = update {
            if !value
                .download_url
                .as_str()
                .starts_with(&format!("https://github.com/{repo}/releases/download/"))
            {
                bail!("Update artifact URL is outside the configured release repository");
            }
        }
        let info = update.as_ref().map(|value| UpdateInfo {
            version: value.version.clone(),
            notes: value.body.clone(),
        });
        *app.state::<Pending>().0.lock().unwrap() = update;
        Ok::<_, anyhow::Error>(info)
    }
    .await;
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn install_update(app: AppHandle, core: State<'_, Arc<Core>>) -> Result<(), String> {
    let update = app
        .state::<Pending>()
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or("Check for an update first")?;
    let _runtime_operation = core.runtime_operation.lock().await;
    let _download_operation = core.download_lock.lock().await;
    core.begin_update().map_err(|error| error.to_string())?;
    let mut received = 0usize;
    let result = async {
        // Download verifies the updater signature before any installer is run.
        let bytes = update
            .download(
                |count, total| {
                    received += count;
                    let _ = app.emit(
                        "speak-update",
                        serde_json::json!({"received":received,"total":total}),
                    );
                },
                || {},
            )
            .await?;
        core.stop_runtime_for_update(); // Stop the model before Windows' installer exits this process.
        update.install(bytes)?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => {
            app.restart();
        }
        Err(error) => {
            core.update_failed(error.to_string());
            Err(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_channel_cannot_be_an_arbitrary_url() {
        assert!(valid_repository("owner/speak"));
        for value in [
            "",
            "https://github.com/a/b",
            "../repo",
            "owner/..",
            "a/b/c",
            "a/b?x",
            "a/b#x",
        ] {
            assert!(!valid_repository(value));
        }
    }
}
