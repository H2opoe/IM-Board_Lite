use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::security::sanitize_log;

use super::{BridgeEnvelope, BridgeError};

pub(super) struct BridgeProcessSpec {
    pub(super) executable: PathBuf,
    pub(super) prefix_args: Vec<String>,
    pub(super) script_path: Option<PathBuf>,
}

pub(super) fn bridge_process_spec(
    platform: &str,
    executable: PathBuf,
    resource_dir: &Path,
) -> BridgeProcessSpec {
    if platform == "wechat" && is_python_bridge_script(&executable) {
        if let Some(python) = resolve_python3(resource_dir) {
            return BridgeProcessSpec {
                executable: python,
                prefix_args: vec![executable.to_string_lossy().to_string()],
                script_path: Some(executable),
            };
        }
        return BridgeProcessSpec {
            executable: executable.clone(),
            prefix_args: Vec::new(),
            script_path: Some(executable),
        };
    }
    BridgeProcessSpec {
        executable,
        prefix_args: Vec::new(),
        script_path: None,
    }
}

fn is_python_bridge_script(path: &Path) -> bool {
    if path.extension().is_some() {
        return false;
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes.into_iter().take(80).collect()).ok())
        .is_some_and(|head| head.starts_with("#!") && head.contains("python"))
}

fn resolve_python3(resource_dir: &Path) -> Option<PathBuf> {
    python3_path_candidates(resource_dir)
        .into_iter()
        .find(|path| is_usable_python3(path))
}

fn python3_path_candidates(resource_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.extend(bundled_python3_candidates(resource_dir));
    for key in ["PYTHON3", "PYTHON"] {
        if let Some(value) = std::env::var_os(key) {
            candidates.push(PathBuf::from(value));
        }
    }
    if let Some(home) = dirs::home_dir() {
        candidates.extend([
            home.join(".local").join("bin").join("python3"),
            home.join(".pyenv").join("shims").join("python3"),
            home.join(".asdf").join("shims").join("python3"),
        ]);
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/python3"),
        PathBuf::from("/usr/local/bin/python3"),
    ]);
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .map(|dir| dir.join("python3"))
                .filter(|path| !is_macos_system_python_shim(path))
                .collect::<Vec<_>>(),
        );
    }
    if !cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/usr/bin/python3"));
    }
    dedupe_existing_paths(&mut candidates);
    candidates
}

fn is_macos_system_python_shim(path: &Path) -> bool {
    cfg!(target_os = "macos") && path == Path::new("/usr/bin/python3")
}

fn bundled_python3_candidates(resource_dir: &Path) -> Vec<PathBuf> {
    let (arch, executable) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("darwin-arm64", ["python", "bin", "python3.10"]),
        ("macos", "x86_64") => ("darwin-x64", ["python", "bin", "python3.10"]),
        ("windows", "x86_64") => ("win-x64", ["python", "python.exe", ""]),
        _ => ("", ["", "", ""]),
    };
    if arch.is_empty() {
        return Vec::new();
    }
    let cwd = std::env::current_dir().ok();
    let roots = [
        Some(resource_dir.join("runtime")),
        Some(resource_dir.join("_up_").join("runtime")),
        Some(resource_dir.join("..").join("runtime")),
        cwd.as_ref().map(|path| path.join("runtime")),
        cwd.as_ref().map(|path| path.join("..").join("runtime")),
    ];
    roots
        .into_iter()
        .flatten()
        .map(|root| {
            let mut path = root
                .join("python")
                .join(PYTHON_STANDALONE_VERSION)
                .join(arch)
                .join(executable[0])
                .join(executable[1]);
            if !executable[2].is_empty() {
                path = path.join(executable[2]);
            }
            path
        })
        .collect()
}

const PYTHON_STANDALONE_VERSION: &str = "20260414";

fn is_usable_python3(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(output) = std::process::Command::new(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let version = [
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ]
    .join(" ");
    version.contains("Python 3.")
}

pub(super) fn bridge_spawn_error(
    platform: &str,
    process: &BridgeProcessSpec,
    err: std::io::Error,
    duration_ms: u128,
) -> BridgeEnvelope {
    let detail = sanitize_log(&err.to_string());
    let script = process
        .script_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let mut hints = Vec::new();
    hints.push(format!(
        "无法启动 bridge 可执行入口：{}",
        process.executable.to_string_lossy()
    ));
    if platform == "wechat" && script.is_some() && process.prefix_args.is_empty() {
        hints.push("应用内置 Python 运行时缺失或不可用，macOS 微信 bridge 无法运行。请重新安装或使用重新打包后的应用。".to_owned());
    }
    hints.push(detail.clone());
    BridgeEnvelope {
        ok: false,
        data: serde_json::Value::Null,
        warnings: hints,
        error: Some(BridgeError {
            code: "BRIDGE_SPAWN_FAILED".to_owned(),
            message: format!("Bridge进程启动失败：{detail}"),
            recoverable: true,
        }),
        meta: serde_json::json!({
            "platform": platform,
            "executable": process.executable.to_string_lossy(),
            "script": script,
            "duration_ms": duration_ms
        }),
    }
}

fn dedupe_existing_paths(entries: &mut Vec<PathBuf>) {
    let mut seen = Vec::<PathBuf>::new();
    entries.retain(|entry| {
        if !entry.exists() || seen.iter().any(|item| item == entry) {
            return false;
        }
        seen.push(entry.clone());
        true
    });
}
