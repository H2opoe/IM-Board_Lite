#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub fn trigger_privacy_registration(pane: &str, app_path: &str, data_dir: &str) {
    match pane {
        "full_disk_access" => {
            for path in protected_read_probe_paths(data_dir) {
                probe_read_access_path(&path);
            }
        }
        "app_management" => {
            if app_path.trim().is_empty() {
                return;
            }
            probe_app_management_permission(Path::new(app_path));
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
pub fn prepare_wechat_sync_access(config_json: &serde_json::Value) {
    for path in wechat_sync_probe_paths(config_json) {
        probe_read_access_path(&path);
    }
}

#[cfg(target_os = "macos")]
pub fn prepare_sync_storage_access(app_dir: &Path, cache_dir: &Path) {
    // 可移动卷访问授权要落到主 App 身份上。同步前先触碰应用数据和缓存目录，
    // 避免后续 bridge 子进程首次访问外置卷时才触发系统弹窗。
    for path in [app_dir, cache_dir] {
        probe_read_access_path(path);
    }
}

#[cfg(target_os = "macos")]
fn wechat_sync_probe_paths(config_json: &serde_json::Value) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for key in ["dbDir", "dataDir", "wechatFilesPath"] {
        if let Some(path) = config_json.get(key).and_then(|value| value.as_str()) {
            push_non_empty_path(&mut paths, path);
        }
    }
    if let Some(bundle_id) = config_json.get("bundleId").and_then(|value| value.as_str()) {
        push_non_empty_path(&mut paths, &default_xwechat_files(bundle_id));
    }
    paths.extend(protected_read_probe_paths(""));
    dedupe_paths(&mut paths);
    paths
}

#[cfg(target_os = "macos")]
fn protected_read_probe_paths(data_dir: &str) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"));
    let mut paths = Vec::new();
    push_non_empty_path(&mut paths, data_dir);
    paths.extend([
        home.join("Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files"),
        home.join("Library/Messages"),
        home.join("Library/Mail"),
        home.join("Library/Safari"),
    ]);
    paths
}

#[cfg(target_os = "macos")]
fn default_xwechat_files(bundle_id: &str) -> String {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return String::new();
    };
    home.join("Library")
        .join("Containers")
        .join(bundle_id)
        .join("Data/Documents/xwechat_files")
        .to_string_lossy()
        .to_string()
}

#[cfg(target_os = "macos")]
fn push_non_empty_path(paths: &mut Vec<PathBuf>, path: &str) {
    let path = path.trim();
    if !path.is_empty() {
        paths.push(PathBuf::from(path));
    }
}

#[cfg(target_os = "macos")]
fn dedupe_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.to_string_lossy().to_string()));
}

#[cfg(target_os = "macos")]
fn probe_read_access_path(path: &Path) {
    if path.is_file() {
        let _ = std::fs::File::open(path);
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten().take(5) {
        let entry_path = entry.path();
        if entry_path.is_file() {
            let _ = std::fs::File::open(entry_path);
        } else if entry_path.is_dir() {
            let _ = std::fs::read_dir(entry_path);
        }
    }
}

#[cfg(target_os = "macos")]
fn probe_app_management_permission(app_path: &Path) {
    use std::io::Write;

    let marker_path = app_path
        .join("Contents")
        .join(".imboard_app_management_probe");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&marker_path)
    {
        let _ = file.write_all(b"imboard permission probe\n");
    }
    let _ = std::fs::remove_file(marker_path);
}

#[cfg(not(target_os = "macos"))]
pub fn trigger_privacy_registration(_pane: &str, _app_path: &str, _data_dir: &str) {}

#[cfg(not(target_os = "macos"))]
pub fn prepare_wechat_sync_access(_config_json: &serde_json::Value) {}

#[cfg(not(target_os = "macos"))]
pub fn prepare_sync_storage_access(_app_dir: &std::path::Path, _cache_dir: &std::path::Path) {}
