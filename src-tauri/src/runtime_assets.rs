use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Deserialize)]
pub struct Manifest {
    pub version: String,
    pub base_url: String,
    pub targets: BTreeMap<String, BTreeMap<String, Vec<Asset>>>,
}
pub fn manifest() -> Result<Manifest> {
    Ok(serde_json::from_str(include_str!(
        "../runtime-manifest.json"
    ))?)
}
pub fn executable_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct CudaDriver {
    pub available: bool,
    pub version: Option<i32>,
}

pub fn probe_cuda_driver() -> CudaDriver {
    #[cfg(any(windows, target_os = "linux"))]
    unsafe {
        let detected = (|| -> Option<i32> {
            #[cfg(windows)]
            let library: libloading::Library =
                libloading::os::windows::Library::load_with_flags("nvcuda.dll", 0x00000800)
                    .ok()?
                    .into(); // System32 only.
            #[cfg(target_os = "linux")]
            let library = libloading::Library::new("libcuda.so.1").ok()?;
            let init: libloading::Symbol<unsafe extern "system" fn(u32) -> i32> =
                library.get(b"cuInit\0").ok()?;
            let version: libloading::Symbol<unsafe extern "system" fn(*mut i32) -> i32> =
                library.get(b"cuDriverGetVersion\0").ok()?;
            let count: libloading::Symbol<unsafe extern "system" fn(*mut i32) -> i32> =
                library.get(b"cuDeviceGetCount\0").ok()?;
            let (mut driver, mut devices) = (0, 0);
            if init(0) != 0
                || version(&mut driver) != 0
                || count(&mut devices) != 0
                || devices == 0
                || driver < 12000
            {
                return None;
            }
            Some(driver)
        })();
        CudaDriver {
            available: detected.is_some(),
            version: detected,
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    CudaDriver::default()
}

pub async fn cuda_driver_available(cancel: &CancellationToken) -> bool {
    if cfg!(target_os = "macos") {
        return false;
    }
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--probe-cuda-driver")
        .env_remove("LD_LIBRARY_PATH")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    tokio::select! {
        result = tokio::time::timeout(std::time::Duration::from_secs(8), command.output()) => {
            result.ok().and_then(Result::ok).filter(|v| v.status.success())
                .and_then(|v| serde_json::from_slice::<CudaDriver>(&v.stdout).ok()).is_some_and(|v| v.available)
        }
        _ = cancel.cancelled() => false,
    }
}

fn safe_name(path: &str) -> bool {
    !path.starts_with(['/', '\\'])
        && !path.contains(':')
        && !path.replace('\\', "/").split('/').any(|v| v == "..")
}

fn checked_link(base: &Path, link: &Path) -> Result<()> {
    let mut result = PathBuf::new();
    for component in base.join(link).components() {
        match component {
            Component::Normal(v) => result.push(v),
            Component::CurDir => {}
            Component::ParentDir if result.pop() => {}
            _ => bail!("Runtime archive link escapes staging directory"),
        }
    }
    Ok(())
}

pub fn unpack(source: &Path, destination: &Path) -> Result<()> {
    let mut bytes = 0u64;
    if source.to_string_lossy().ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(fs::File::open(source)?)?;
        if archive.len() > 20_000 {
            bail!("Runtime archive contains too many entries");
        }
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if !safe_name(&entry.name()?)
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                bail!("Unsafe runtime ZIP path or symlink");
            }
            bytes += entry.size();
            if bytes > 12 * 1024 * 1024 * 1024 {
                bail!("Runtime archive exceeds extraction limit");
            }
            let target = destination.join(entry.enclosed_name().context("Invalid ZIP path")?);
            if entry.is_dir() {
                fs::create_dir_all(target)?;
            } else {
                fs::create_dir_all(target.parent().context("Missing ZIP parent")?)?;
                let mut output = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(target)?;
                std::io::copy(&mut entry, &mut output)?;
            }
        }
    } else {
        let reader = flate2::read::GzDecoder::new(fs::File::open(source)?);
        let mut archive = tar::Archive::new(reader);
        for (index, entry) in archive.entries()?.enumerate() {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            if index > 20_000 || !safe_name(&path.to_string_lossy()) {
                bail!("Unsafe runtime TAR path");
            }
            let kind = entry.header().entry_type();
            bytes += entry.size();
            if bytes > 12 * 1024 * 1024 * 1024 {
                bail!("Runtime archive exceeds extraction limit");
            }
            if kind.is_symlink() || kind.is_hard_link() {
                let link = entry.link_name()?.context("Missing runtime link target")?;
                checked_link(
                    if kind.is_symlink() {
                        path.parent().unwrap_or(Path::new(""))
                    } else {
                        Path::new("")
                    },
                    &link,
                )?;
            } else if !kind.is_file() && !kind.is_dir() {
                bail!("Unsupported runtime archive entry");
            }
            if !entry.unpack_in(destination)? {
                bail!("Runtime path escaped staging directory");
            }
        }
    }
    Ok(())
}

fn files(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            files(&entry.path(), output)?;
        } else if entry.path().is_file() {
            output.push(entry.path());
        }
    }
    Ok(())
}

pub fn copy_payload(source: &Path, output: &Path, part: usize) -> Result<()> {
    let root = source.canonicalize()?;
    let mut entries = Vec::new();
    files(source, &mut entries)?;
    for path in entries {
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(&root) {
            bail!("Runtime symlink escaped staging directory");
        }
        let name = path
            .file_name()
            .context("Missing runtime name")?
            .to_string_lossy();
        let lower = name.to_lowercase();
        let license = ["license", "copying", "notice", "eula"]
            .iter()
            .any(|word| lower.contains(word));
        let target = if license {
            output
                .join("licenses")
                .join(part.to_string())
                .join(path.strip_prefix(source)?)
        } else if name == executable_name()
            || lower.ends_with(".dll")
            || lower.ends_with(".dylib")
            || lower.contains(".so")
            || lower.ends_with(".metal")
            || lower.ends_with(".metallib")
        {
            output.join(name.as_ref())
        } else {
            continue;
        };
        fs::create_dir_all(target.parent().context("Missing runtime output parent")?)?;
        if target.exists() {
            if crate::download::sha256_file(&target)? != crate::download::sha256_file(&canonical)? {
                bail!("Conflicting runtime library: {name}");
            }
        } else {
            #[cfg(unix)]
            if path.is_symlink() && !license {
                let canonical_name = canonical
                    .file_name()
                    .context("Missing canonical library name")?;
                let library = output.join(canonical_name);
                if !library.exists() {
                    fs::copy(&canonical, &library)?;
                }
                if library != target {
                    std::os::unix::fs::symlink(canonical_name, target)?;
                }
                continue;
            }
            fs::copy(&canonical, target)?;
        }
    }
    Ok(())
}

pub async fn ensure_cuda(
    data_dir: &Path,
    cancel: &CancellationToken,
    progress: &(impl Fn(String) + Send + Sync),
) -> Result<PathBuf> {
    let manifest = manifest()?;
    let target = env!("SPEAK_TARGET");
    let assets = manifest
        .targets
        .get(target)
        .and_then(|v| v.get("cuda"))
        .context("CUDA is not packaged for this architecture")?
        .clone();
    let cache = data_dir.join("runtimes").join(&manifest.version);
    tokio::fs::create_dir_all(&cache).await?;
    let installed = cache.join("cuda");
    let marker = serde_json::to_vec(&assets)?;
    if tokio::fs::read(installed.join("ready.json"))
        .await
        .ok()
        .as_deref()
        == Some(marker.as_slice())
        && installed.join(executable_name()).is_file()
    {
        return Ok(installed.join(executable_name()));
    }
    for asset in &assets {
        if !safe_name(&asset.name) || asset.name.contains('/') {
            bail!("Invalid runtime manifest filename");
        }
        progress(format!("Setting up NVIDIA acceleration · {}", asset.name));
        crate::download::fetch(
            &format!("{}/{}", manifest.base_url, asset.name),
            asset.size,
            &asset.sha256,
            &cache.join(&asset.name),
            cancel,
            |received, total| {
                progress(format!(
                    "Setting up NVIDIA acceleration · {:.0}%",
                    received as f64 / total as f64 * 100.
                ));
            },
        )
        .await?;
    }
    let cache_path = cache.clone();
    let destination = installed.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let stage = cache_path.join(format!("extract-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&stage)?;
        let result = (|| {
            let normalized = stage.join("cuda");
            fs::create_dir_all(&normalized)?;
            for (index, asset) in assets.iter().enumerate() {
                let archive = stage.join(index.to_string());
                fs::create_dir_all(&archive)?;
                unpack(&cache_path.join(&asset.name), &archive)?;
                copy_payload(&archive, &normalized, index)?;
            }
            let executable = normalized.join(executable_name());
            if !executable.is_file() {
                bail!("Downloaded runtime contains no llama-server");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(executable, fs::Permissions::from_mode(0o755))?;
            }
            fs::write(normalized.join("ready.json"), marker)?;
            if destination.exists() {
                fs::remove_dir_all(&destination)?;
            }
            fs::rename(normalized, destination)?;
            Ok(())
        })();
        let _ = fs::remove_dir_all(stage);
        result
    })
    .await??;
    if cancel.is_cancelled() {
        bail!("CUDA setup cancelled");
    }
    Ok(installed.join(executable_name()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_and_archive_boundaries() {
        let manifest = manifest().unwrap();
        assert_eq!(manifest.targets.len(), 4);
        for backends in manifest.targets.values() {
            for assets in backends.values() {
                for asset in assets {
                    assert!(safe_name(&asset.name));
                    assert_eq!(asset.sha256.len(), 64);
                    assert!(asset.size > 0);
                }
            }
        }
        for path in ["../x", "/x", "C:\\x", "a/../../x", "a\\..\\x"] {
            assert!(!safe_name(path));
        }
        assert!(checked_link(Path::new("bin"), Path::new("../lib/x")).is_ok());
        assert!(checked_link(Path::new("bin"), Path::new("../../outside")).is_err());
    }

    #[test]
    fn extraction_normalizes_payload_and_rejects_tar_traversal() {
        use std::io::Write;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("runtime.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&source).unwrap());
        zip.start_file(
            format!("bin/{}", executable_name()),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"executable fixture").unwrap();
        zip.start_file("lib/x.dll", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"library fixture").unwrap();
        zip.start_file("LICENSE.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"license fixture").unwrap();
        zip.finish().unwrap();
        let extracted = temp.path().join("extracted");
        fs::create_dir(&extracted).unwrap();
        unpack(&source, &extracted).unwrap();
        let output = temp.path().join("normalized");
        copy_payload(&extracted, &output, 0).unwrap();
        assert_eq!(
            fs::read(output.join(executable_name())).unwrap(),
            b"executable fixture"
        );
        assert_eq!(fs::read(output.join("x.dll")).unwrap(), b"library fixture");
        assert_eq!(
            fs::read(output.join("licenses/0/LICENSE.txt")).unwrap(),
            b"license fixture"
        );

        let bad = temp.path().join("bad.tar.gz");
        let writer = flate2::write::GzEncoder::new(
            fs::File::create(&bad).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = tar::Builder::new(writer);
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes()[..9].copy_from_slice(b"../escape");
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, &b"bad"[..]).unwrap();
        archive.into_inner().unwrap().finish().unwrap();
        assert!(unpack(&bad, &extracted).is_err());
        assert!(!temp.path().join("escape").exists());
    }
}
