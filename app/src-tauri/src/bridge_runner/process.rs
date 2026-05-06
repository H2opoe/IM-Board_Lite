use std::path::{Path, PathBuf};

use crate::security::sanitize_log;

use super::{BridgeEnvelope, BridgeError};

pub(super) struct BridgeProcessSpec {
    pub(super) executable: PathBuf,
    pub(super) prefix_args: Vec<String>,
    pub(super) script_path: Option<PathBuf>,
}

pub(super) fn bridge_process_spec(
    _platform: &str,
    executable: PathBuf,
    _resource_dir: &Path,
) -> BridgeProcessSpec {
    BridgeProcessSpec {
        executable,
        prefix_args: Vec::new(),
        script_path: None,
    }
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
