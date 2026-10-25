use std::fs;
use std::path::{Path, PathBuf};

pub(super) use super::extraction::extract_tgz_bytes;
use sha2::{Digest, Sha512};

const MAX_DOWNLOAD_BYTES: usize = 128 * 1024 * 1024;

pub(super) fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(120))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("无法创建CLI下载客户端：{e}"))
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载官方CLI资源失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("下载官方CLI资源失败：HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_DOWNLOAD_BYTES as u64)
    {
        return Err("官方CLI资源超过大小限制。".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("读取CLI资源失败：{e}"))?
    {
        if chunk.len() > MAX_DOWNLOAD_BYTES - bytes.len() {
            return Err("官方CLI资源超过大小限制。".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) async fn download_and_extract_tgz(
    client: &reqwest::Client,
    url: &str,
    integrity: &str,
    destination: &Path,
) -> Result<(), String> {
    let bytes = download(client, url).await?;
    verify_npm_integrity(&bytes, integrity)?;
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    extract_tgz_bytes(&bytes, destination, true)
}

fn verify_npm_integrity(bytes: &[u8], integrity: &str) -> Result<(), String> {
    use base64::Engine;
    let expected = integrity
        .strip_prefix("sha512-")
        .ok_or_else(|| "官方CLI包使用了不支持的完整性算法。".to_owned())?;
    let expected = base64::engine::general_purpose::STANDARD
        .decode(expected)
        .map_err(|_| "官方CLI包完整性信息无效。".to_owned())?;
    if Sha512::digest(bytes).as_slice() != expected.as_slice() {
        return Err("官方CLI包完整性校验失败，已拒绝安装。".into());
    }
    Ok(())
}

fn verify_sha256(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = format!("{:x}", sha2::Sha256::digest(bytes));
    if actual != expected {
        return Err("官方CLI可执行文件完整性校验失败，已拒绝安装。".into());
    }
    Ok(())
}

pub(super) async fn download_first_verified(
    urls: &[String],
    sha256: &str,
) -> Result<Vec<u8>, String> {
    let client = download_client()?;
    download_verified_with_client(&client, urls, sha256).await
}

async fn download_verified_with_client(
    client: &reqwest::Client,
    urls: &[String],
    sha256: &str,
) -> Result<Vec<u8>, String> {
    let mut failures = Vec::new();
    for url in urls {
        match download(client, url).await {
            Ok(bytes) => match verify_sha256(&bytes, sha256) {
                Ok(()) => return Ok(bytes),
                Err(e) => failures.push(e),
            },
            Err(e) => failures.push(e),
        }
    }
    Err(format!(
        "下载官方CLI可执行文件失败：{}",
        failures.join("；")
    ))
}

pub(super) fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    let boundary = fs::canonicalize(root).ok()?;
    let mut pending = vec![boundary.clone()];
    let mut visited = std::collections::HashSet::new();
    while let Some(directory) = pending.pop() {
        let Ok(directory) = fs::canonicalize(directory) else {
            continue;
        };
        if !directory.starts_with(&boundary) || !visited.insert(directory.clone()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(resolved) = fs::canonicalize(&path) else {
                continue;
            };
            if !resolved.starts_with(&boundary) {
                continue;
            }
            if resolved.is_file() && path.file_name().and_then(|value| value.to_str()) == Some(name)
            {
                return Some(path);
            }
            if resolved.is_dir() {
                pending.push(resolved);
            }
        }
    }
    None
}

pub(super) fn make_executable(_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(_path)
            .map_err(|err| err.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(_path, permissions).map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn http_fixture(
        body: &'static str,
        length: usize,
        delay_ms: u64,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request);
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}"
            );
        });
        (url, thread)
    }

    #[tokio::test]
    async fn corrupted_first_source_is_rejected_before_verified_fallback() {
        let (bad, bad_thread) = http_fixture("tampered", 8, 0);
        let (good, good_thread) = http_fixture("verified", 8, 0);
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let digest = format!("{:x}", sha2::Sha256::digest(b"verified"));
        assert_eq!(
            download_verified_with_client(&client, &[bad, good], &digest)
                .await
                .unwrap(),
            b"verified"
        );
        bad_thread.join().unwrap();
        good_thread.join().unwrap();
    }

    #[tokio::test]
    async fn oversized_and_stalled_downloads_fail_without_extraction() {
        let (url, thread) = http_fixture("", MAX_DOWNLOAD_BYTES + 1, 0);
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_millis(40))
            .build()
            .unwrap();
        assert!(download(&client, &url)
            .await
            .unwrap_err()
            .contains("大小限制"));
        thread.join().unwrap();
        let (url, thread) = http_fixture("late", 4, 150);
        assert!(download(&client, &url).await.is_err());
        thread.join().unwrap();
        assert!(verify_sha256(b"same", "").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn file_search_handles_cycles_and_does_not_follow_external_links() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("cli"), "external").unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("cycle")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
        assert!(find_file_named(root.path(), "cli").is_none());
        fs::create_dir(root.path().join("nested")).unwrap();
        fs::write(root.path().join("nested/cli"), "internal").unwrap();
        assert_eq!(
            find_file_named(root.path(), "cli"),
            Some(fs::canonicalize(root.path().join("nested/cli")).unwrap())
        );
    }

    #[test]
    fn npm_integrity_accepts_only_matching_sha512() {
        let payload = b"verified official cli archive";
        let digest = Sha512::digest(payload);
        let integrity = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(digest)
        );
        assert!(verify_npm_integrity(payload, &integrity).is_ok());
        assert!(verify_npm_integrity(b"tampered", &integrity).is_err());
        assert!(verify_npm_integrity(payload, "sha256-unsupported").is_err());
    }
}
