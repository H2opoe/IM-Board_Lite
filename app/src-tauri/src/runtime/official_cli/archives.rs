use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;

pub(super) async fn download_and_extract_tgz(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
) -> Result<(), String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("下载官方CLI包失败：{err}"))?;
    if !response.status().is_success() {
        return Err(format!("下载官方CLI包失败：HTTP {}", response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("读取官方CLI包失败：{err}"))?;
    fs::remove_dir_all(destination).ok();
    fs::create_dir_all(destination).map_err(|err| err.to_string())?;
    extract_tgz_bytes(&bytes, destination, true)
}

pub(super) async fn download_first_available(urls: &[String]) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::new();
    let mut last_error = String::new();
    for url in urls {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                return response
                    .bytes()
                    .await
                    .map(|bytes| bytes.to_vec())
                    .map_err(|err| format!("读取下载内容失败：{err}"));
            }
            Ok(response) => last_error = format!("{url}: HTTP {}", response.status()),
            Err(err) => last_error = format!("{url}: {err}"),
        }
    }
    Err(format!("下载官方CLI可执行文件失败：{last_error}"))
}

pub(super) fn extract_tgz_bytes(
    bytes: &[u8],
    destination: &Path,
    strip_package_prefix: bool,
) -> Result<(), String> {
    let decoder = GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|err| format!("读取 tgz 失败：{err}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|err| format!("读取 tgz 条目失败：{err}"))?;
        let raw_path = entry.path().map_err(|err| err.to_string())?.to_path_buf();
        let relative = safe_archive_path(&raw_path, strip_package_prefix)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        entry
            .unpack(&output)
            .map_err(|err| format!("解压官方CLI文件 {} 失败：{err}", output.display()))?;
    }
    Ok(())
}

pub(super) fn extract_zip_bytes(bytes: &[u8], destination: &Path) -> Result<(), String> {
    let reader = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|err| format!("读取 zip 失败：{err}"))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|err| err.to_string())?;
        let Some(path) = file.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        let output = destination.join(path);
        if file.is_dir() {
            fs::create_dir_all(&output).map_err(|err| err.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut output_file = fs::File::create(&output).map_err(|err| err.to_string())?;
        std::io::copy(&mut file, &mut output_file).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn safe_archive_path(path: &Path, strip_package_prefix: bool) -> Result<PathBuf, String> {
    let mut output = PathBuf::new();
    for (index, component) in path.components().enumerate() {
        if strip_package_prefix && index == 0 {
            continue;
        }
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            _ => return Err(format!("官方CLI包含不安全路径：{}", path.display())),
        }
    }
    Ok(output)
}

pub(super) fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

pub(super) fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|err| err.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).map_err(|err| err.to_string())?;
    }
    Ok(())
}
