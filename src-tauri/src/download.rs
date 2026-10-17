use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, time::Duration};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

pub async fn verified(path: &Path, expected: &str) -> Result<bool> {
    let path = path.to_owned();
    let expected = expected.to_owned();
    tokio::task::spawn_blocking(move || -> Result<bool> { Ok(sha256_file(&path)? == expected) })
        .await?
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub async fn fetch(
    url: &str,
    size: u64,
    sha: &str,
    target: &Path,
    cancel: &CancellationToken,
    progress: impl Fn(u64, u64),
) -> Result<()> {
    if !(url.starts_with("https://") || cfg!(test) && url.starts_with("http://127.0.0.1:")) {
        bail!("Downloads require HTTPS");
    }
    if sha.len() != 64 || !sha.bytes().all(|v| v.is_ascii_hexdigit()) || size == 0 {
        bail!("Invalid download manifest");
    }
    if target.is_file() {
        if tokio::fs::metadata(target).await?.len() == size && verified(target, sha).await? {
            return Ok(());
        }
        bail!(
            "{} failed verification. Move it aside before retrying.",
            target.display()
        );
    }
    let partial = target.with_file_name(format!(
        "{}.part",
        target
            .file_name()
            .context("Missing download filename")?
            .to_string_lossy()
    ));
    let mut offset = tokio::fs::metadata(&partial)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    if offset > size {
        tokio::fs::remove_file(&partial).await?;
        offset = 0;
    }
    let transfer = async {
        if offset < size {
            let response = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(20))
                .build()?
                .get(url)
                .header("Range", format!("bytes={offset}-"))
                .send()
                .await?
                .error_for_status()?;
            let append = offset > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
            if append
                && !response
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| {
                        v.starts_with(&format!("bytes {offset}-"))
                            && v.ends_with(&format!("/{size}"))
                    })
            {
                bail!("Download server returned an invalid resume range");
            }
            if !append {
                offset = 0;
            }
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&partial)
                .await?;
            let mut stream = response.bytes_stream();
            let mut last = std::time::Instant::now();
            progress(offset, size);
            while let Some(bytes) = tokio::time::timeout(Duration::from_secs(60), stream.next())
                .await
                .context("Download stalled; retry to resume")?
            {
                let bytes = bytes?;
                offset += bytes.len() as u64;
                if offset > size {
                    bail!("Download exceeded its expected size");
                }
                file.write_all(&bytes).await?;
                if last.elapsed() > Duration::from_millis(250) {
                    progress(offset, size);
                    last = std::time::Instant::now();
                }
            }
            file.sync_all().await?;
        }
        if offset != size {
            bail!("Download interrupted; retry to resume");
        }
        if !verified(&partial, sha).await? {
            tokio::fs::remove_file(&partial).await?;
            bail!("Checksum mismatch. The invalid download was removed; retry.");
        }
        tokio::fs::rename(&partial, target).await?;
        progress(size, size);
        Ok(())
    };
    tokio::select! { result = transfer => result, _ = cancel.cancelled() => bail!("Download cancelled; partial file retained for resume") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[tokio::test]
    async fn resumes_verifies_and_rejects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("asset");
        std::fs::write(dir.path().join("asset.part"), b"abc").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/asset", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let n = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..n])
                .to_lowercase()
                .contains("range: bytes=3-"));
            stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef").unwrap();
        });
        let hash = format!("{:x}", Sha256::digest(b"abcdef"));
        fetch(
            &url,
            6,
            &hash,
            &target,
            &CancellationToken::new(),
            |_, _| {},
        )
        .await
        .unwrap();
        worker.join().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"abcdef");
        fetch(
            &url,
            6,
            &hash,
            &target,
            &CancellationToken::new(),
            |_, _| {},
        )
        .await
        .unwrap(); // Cached; no server needed.
        std::fs::write(&target, b"badbad").unwrap();
        assert!(fetch(
            &url,
            6,
            &hash,
            &target,
            &CancellationToken::new(),
            |_, _| {}
        )
        .await
        .is_err());
    }
}
