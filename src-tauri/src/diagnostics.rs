use crate::{config, inference::Inference, runtime::Runtime};
use anyhow::{bail, Result};
use std::{io::Cursor, path::Path, time::Instant};
use tokio_util::sync::CancellationToken;

/// Exercises the exact Rust/runtime protocol without recording or typing into another app.
pub async fn smoke_test(path: &Path) -> Result<()> {
    let audio = std::fs::read(path)?;
    let reader = hound::WavReader::new(Cursor::new(&audio))?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.bits_per_sample != 16
        || !(8000..=96000).contains(&spec.sample_rate)
        || reader.duration() == 0
        || reader.duration() as f64 / spec.sample_rate as f64 > 28.
    {
        bail!("Use a mono 16-bit PCM WAV with 0–28 seconds of audio at 8–96 kHz");
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("speak-check-{}", uuid::Uuid::new_v4()));
    let settings = config::load(&dir)?;
    let cancel = CancellationToken::new();
    let runtime = Runtime::launch(&settings, &dir, root, &cancel, &|message| {
        eprintln!("{message}")
    })
    .await?;
    eprintln!("Selected acceleration: {}", runtime.acceleration);
    for note in &runtime.notes {
        eprintln!("Fallback: {note}");
    }
    let result = async {
        let inference = Inference::new(runtime.url.clone(), runtime.key.clone())?;
        let cancel = CancellationToken::new();
        let start = Instant::now();
        let raw = inference.transcribe(&audio, &cancel).await?;
        println!(
            "Transcription ({:.2}s): {raw}",
            start.elapsed().as_secs_f32()
        );
        let start = Instant::now();
        let polished = inference.polish(&raw, &cancel).await?;
        println!(
            "Polishing ({:.2}s): {polished}",
            start.elapsed().as_secs_f32()
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;
    drop(runtime); // Test-owned process is always stopped, including on failure.
    if result.is_ok() {
        let _ = std::fs::remove_file(dir.join("llama.log"));
        let _ = std::fs::remove_dir(dir);
    } else {
        eprintln!("Runtime log retained in {}", dir.display());
    }
    result
}
