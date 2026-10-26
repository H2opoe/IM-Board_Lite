use crate::bridge_registry::BridgeRegistry;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
#[cfg(all(test, unix))]
use std::time::Duration;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::connectors::{self, ConnectorKind};
use crate::security::sanitize_log;
use crate::storage::models::ImProfile;

mod dingtalk_runner;
mod errors;
mod feishu_runner;
mod normalizers;
mod paths;
mod process;
pub(crate) mod read_result;
mod validation;
mod wecom_runner;
use dingtalk_runner::run_official_dingtalk_cli;
#[cfg(test)]
use errors::*;
use feishu_runner::run_official_feishu_cli;
#[cfg(test)]
use normalizers::*;
use paths::resolve_bridge_executable;
use process::{bridge_process_spec, bridge_spawn_error};
use wecom_runner::run_official_wecom_cli;

pub(super) const APP_DATA_DIR_NAME: &str = crate::edition::DATA_DIR_NAME;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRequest {
    pub platform: String,
    pub command: String,
    pub profile: Option<ImProfile>,
    pub args: HashMap<String, String>,
    #[serde(default)]
    pub stdin_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEnvelope {
    pub ok: bool,
    pub data: serde_json::Value,
    pub warnings: Vec<String>,
    pub error: Option<BridgeError>,
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

pub async fn run_bridge(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
) -> anyhow::Result<BridgeEnvelope> {
    run_bridge_tracked(request, resource_dir, cache_dir, None).await
}

pub async fn run_bridge_tracked(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_bridges: Option<&BridgeRegistry>,
) -> anyhow::Result<BridgeEnvelope> {
    let command = request.command.clone();
    let envelope = run_bridge_inner(request, resource_dir, cache_dir, active_bridges).await?;
    Ok(read_result::enforce(&command, envelope))
}

async fn run_bridge_inner(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_bridges: Option<&BridgeRegistry>,
) -> anyhow::Result<BridgeEnvelope> {
    validation::validate_bridge_request(&request).map_err(anyhow::Error::msg)?;
    let started_at = Instant::now();
    if let Some(adapter) = connectors::find(&request.platform) {
        match adapter.kind {
            ConnectorKind::Wecom => {
                return run_official_wecom_cli(
                    request,
                    resource_dir,
                    cache_dir,
                    active_bridges,
                    started_at,
                )
                .await;
            }
            ConnectorKind::Feishu => {
                return run_official_feishu_cli(
                    request,
                    resource_dir,
                    cache_dir,
                    active_bridges,
                    started_at,
                )
                .await;
            }
            ConnectorKind::Dingtalk => {
                return run_official_dingtalk_cli(
                    request,
                    resource_dir,
                    cache_dir,
                    active_bridges,
                    started_at,
                )
                .await;
            }
        }
    }
    let executable = resolve_bridge_executable(&resource_dir, &request);
    let process = bridge_process_spec(&request.platform, executable, &resource_dir);
    let mut command = Command::new(&process.executable);
    configure_process_supervision(&mut command);
    command.args(&process.prefix_args);

    command.arg(&request.command).arg("--format").arg("json");

    if let Some(profile) = &request.profile {
        command.arg("--profile-id").arg(&profile.id);
        if let Some(config_path) = profile
            .config_json
            .get("configPath")
            .and_then(|v| v.as_str())
        {
            command.arg("--config").arg(config_path);
        }
        if let Some(keys_path) = profile.config_json.get("keysPath").and_then(|v| v.as_str()) {
            command.arg("--keys-file").arg(keys_path);
        }
        command.env("IMD_PROFILE_CONFIG_JSON", profile.config_json.to_string());
        let profile_cache = cache_dir.join(&profile.id);
        let tmp_dir = profile_cache.join("tmp");
        std::fs::create_dir_all(&tmp_dir)?;
        command.env("TMPDIR", tmp_dir);
    }

    for (key, value) in request.args {
        command
            .arg(format!("--{}", key.replace('_', "-")))
            .arg(value);
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let input = request
        .stdin_secret
        .as_deref()
        .map(|secret| format!("{secret}\n").into_bytes());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let child = match command::spawn_bridge(command) {
        Ok(child) => child,
        Err(err) => {
            return Ok(bridge_spawn_error(
                &request.platform,
                &process,
                err,
                started_at.elapsed().as_millis(),
            ))
        }
    };
    let output = wait_for_bridge_output(
        child,
        active_bridges,
        OFFICIAL_CLI_COMMAND_TIMEOUT,
        input.as_deref(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("桥接执行超时，已请求终止进程，请重试。"))??;
    let stderr = sanitize_log(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Ok(mut envelope) = serde_json::from_str::<BridgeEnvelope>(&stdout) {
        envelope.meta = merge_meta(
            envelope.meta,
            serde_json::json!({ "duration_ms": started_at.elapsed().as_millis() }),
        );
        if !stderr.is_empty() {
            envelope.warnings.push(stderr);
        }
        return Ok(envelope);
    }

    if !output.status.success() {
        let stdout_detail = sanitize_log(stdout.trim());
        let warnings = [stderr.as_str(), stdout_detail.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        return Ok(BridgeEnvelope {
            ok: false,
            data: serde_json::Value::Null,
            warnings,
            error: Some(BridgeError {
                code: "BRIDGE_CRASHED".to_owned(),
                message: "Bridge进程执行失败".to_owned(),
                recoverable: true,
            }),
            meta: serde_json::json!({
                "platform": request.platform,
                "executable": process.executable.to_string_lossy(),
                "script": process.script_path.map(|path| path.to_string_lossy().to_string()),
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }

    let mut envelope: BridgeEnvelope = serde_json::from_str(&stdout)?;
    envelope.meta = merge_meta(
        envelope.meta,
        serde_json::json!({ "duration_ms": started_at.elapsed().as_millis() }),
    );
    if !stderr.is_empty() {
        envelope.warnings.push(stderr);
    }
    Ok(envelope)
}

mod command;
use command::{
    apply_official_cli_env, configure_process_supervision, official_cli_command,
    wait_for_bridge_output, wait_for_official_cli_output, OFFICIAL_CLI_AUTH_TIMEOUT,
    OFFICIAL_CLI_COMMAND_TIMEOUT,
};
fn merge_meta(mut left: serde_json::Value, right: serde_json::Value) -> serde_json::Value {
    if let (Some(left_map), Some(right_map)) = (left.as_object_mut(), right.as_object()) {
        for (key, value) in right_map {
            left_map.insert(key.clone(), value.clone());
        }
    }
    left
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod envelope_contract_tests;

#[cfg(test)]
mod dingtalk_error_tests;

#[cfg(test)]
mod wecom_error_tests;

mod wecom_errors;
