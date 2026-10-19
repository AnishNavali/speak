use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model: String,
    pub projector: String,
    pub executable: String,
    pub microphone: String,
    pub shortcut: String,
    pub pause_threshold: f32,
    pub floating: bool,
    pub acceleration: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: String::new(),
            projector: String::new(),
            executable: String::new(),
            microphone: String::new(),
            shortcut: "F5".into(),
            pause_threshold: 0.006,
            floating: true,
            acceleration: "auto".into(),
        }
    }
}

impl Settings {
    pub fn needs_model_setup(&self) -> bool {
        !Path::new(&self.model).is_file() || !Path::new(&self.projector).is_file()
    }
}

pub fn load(dir: &Path) -> Result<Settings> {
    let path = dir.join("settings.json");
    if path.exists() {
        return serde_json::from_slice(&fs::read(path)?)
            .context("Could not read settings; existing file was left untouched");
    }
    let mut settings = Settings::default();
    // Reuse the prototype's downloads during development; installed apps use app data.
    let mut roots = vec![dir.join("models")];
    if cfg!(debug_assertions) {
        roots.insert(
            0,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../models"),
        );
    }
    for models in roots {
        let model = models.join("gemma-4-E2B-it-Q4_0.gguf");
        let projector = models.join("mmproj-gemma-4-E2B-it-BF16.gguf");
        if model.is_file() && projector.is_file() {
            settings.model = model.canonicalize()?.to_string_lossy().into_owned();
            settings.projector = projector.canonicalize()?.to_string_lossy().into_owned();
            break;
        }
    }
    Ok(settings)
}

pub fn validate(settings: &Settings) -> Result<()> {
    if !matches!(settings.acceleration.as_str(), "auto" | "cpu") {
        bail!("Acceleration must be auto or cpu");
    }
    if !(0.0001..=0.1).contains(&settings.pause_threshold) {
        bail!("Pause threshold must be between 0.0001 and 0.1");
    }
    if settings.shortcut.trim().is_empty() {
        bail!("Choose a push-to-talk shortcut");
    }
    for file in [&settings.model, &settings.projector] {
        let mut header = [0; 4];
        fs::File::open(file)
            .with_context(|| format!("Cannot open model file: {file}"))?
            .read_exact(&mut header)?;
        if &header != b"GGUF" {
            bail!("Not a GGUF model: {file}");
        }
    }
    Ok(())
}

pub fn save(dir: &Path, settings: &Settings) -> Result<()> {
    fs::create_dir_all(dir)?;
    let temp = dir.join("settings.json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(settings)?)?;
    fs::rename(temp, dir.join("settings.json"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_setup_requires_both_files_and_rechecks_saved_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        assert!(settings.needs_model_setup());
        settings.model = dir.path().join("model.gguf").to_string_lossy().into_owned();
        settings.projector = dir.path().join("audio.gguf").to_string_lossy().into_owned();
        fs::write(&settings.model, b"GGUF").unwrap();
        assert!(settings.needs_model_setup());
        fs::write(&settings.projector, b"GGUF").unwrap();
        assert!(!settings.needs_model_setup());
        save(dir.path(), &settings).unwrap();
        fs::remove_file(&settings.projector).unwrap();
        assert!(load(dir.path()).unwrap().needs_model_setup());
        settings.projector = dir.path().to_string_lossy().into_owned();
        assert!(settings.needs_model_setup());
    }

    #[test]
    fn default_push_to_talk_is_a_single_function_key() {
        let shortcut: tauri_plugin_global_shortcut::Shortcut =
            Settings::default().shortcut.parse().unwrap();
        assert!(shortcut.mods.is_empty());
        assert_eq!(shortcut.key, tauri_plugin_global_shortcut::Code::F5);
    }
}
