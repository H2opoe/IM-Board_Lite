use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Instant;

use super::errors::{
    bridge_error_for, classify_feishu_cli_error, classify_feishu_cli_structured_error,
    parse_feishu_cli_json, sanitize_feishu_cli_output, validate_feishu_auth_status,
};
use super::normalizers::{
    feishu_time_arg, normalize_feishu_chats, normalize_feishu_message_sessions,
    normalize_feishu_messages,
};
use super::paths::{expand_home, resolve_official_cli_for_runtime};
use super::{
    apply_official_cli_env, official_cli_command, wait_for_official_cli_output, BridgeEnvelope,
    BridgeRequest, OFFICIAL_CLI_AUTH_TIMEOUT, OFFICIAL_CLI_COMMAND_TIMEOUT,
};

pub(super) async fn run_official_feishu_cli(
    request: BridgeRequest,
    resource_dir: PathBuf,
    cache_dir: PathBuf,
    active_pids: Option<&Mutex<Vec<u32>>>,
    started_at: Instant,
) -> anyhow::Result<BridgeEnvelope> {
    let Some(profile) = &request.profile else {
        return Ok(bridge_error_for(
            "feishu",
            "https://github.com/larksuite/cli",
            "FEISHU_PROFILE_NOT_FOUND",
            "缺少飞书账号配置。",
            true,
            started_at,
        ));
    };
    let Some(cli_path) = resolve_official_cli_for_runtime(
        &resource_dir,
        Some(profile),
        "feishu",
        "@larksuite/cli",
        "lark-cli",
    ) else {
        return Ok(bridge_error_for(
            "feishu",
            "https://github.com/larksuite/cli",
            "FEISHU_CLI_MISSING",
            "官方CLI尚未准备完成，请重新打开绑定窗口等待准备完成，或重新安装IM-Board。",
            true,
            started_at,
        ));
    };
    let profile_name = profile
        .config_json
        .get("profileName")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&profile.id);

    let lark_config_dir = feishu_lark_config_dir(&profile.config_json);
    if let Some(lark_config_dir) = &lark_config_dir {
        std::fs::create_dir_all(lark_config_dir)?;
    }
    if request.command != "auth-status" {
        if let Some(error) = run_feishu_auth_preflight(
            &cli_path,
            profile_name,
            lark_config_dir.as_deref(),
            active_pids,
        )
        .await?
        {
            return Ok(BridgeEnvelope {
                ok: false,
                data: serde_json::Value::Null,
                warnings: Vec::new(),
                error: Some(error),
                meta: serde_json::json!({
                    "platform": "feishu",
                    "source": "https://github.com/larksuite/cli",
                    "duration_ms": started_at.elapsed().as_millis()
                }),
            });
        }
    }

    let mut cli_args = vec!["--profile".to_owned(), profile_name.to_owned()];
    match request.command.as_str() {
        "list-chats" => {
            let page_size = request
                .args
                .get("limit")
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(100)
                .clamp(1, 100)
                .to_string();
            // 官方 CLI 新版群列表入口是 `im +chat-list`；旧的 `im chats` 只是子命令分组，
            // 不能接收 `--as`，会直接触发 unknown flag。
            cli_args.extend([
                "im".to_owned(),
                "+chat-list".to_owned(),
                "--as".to_owned(),
                "user".to_owned(),
                "--sort-type".to_owned(),
                "ByActiveTimeDesc".to_owned(),
                "--page-size".to_owned(),
                page_size,
                "--format".to_owned(),
                "json".to_owned(),
            ]);
        }
        "fetch-messages" => {
            let chat_id = request.args.get("chat").cloned().unwrap_or_default();
            if chat_id.trim().is_empty() {
                return Ok(bridge_error_for(
                    "feishu",
                    "https://github.com/larksuite/cli",
                    "MISSING_CHAT",
                    "fetch-messages缺少chat参数。",
                    true,
                    started_at,
                ));
            }
            cli_args.extend([
                "im".to_owned(),
                "+chat-messages-list".to_owned(),
                "--as".to_owned(),
                "user".to_owned(),
                "--chat-id".to_owned(),
                chat_id,
                "--page-size".to_owned(),
                "50".to_owned(),
                "--format".to_owned(),
                "json".to_owned(),
            ]);
            if let Some(start) = request
                .args
                .get("start_time")
                .and_then(|value| feishu_time_arg(value))
            {
                cli_args.extend(["--start".to_owned(), start]);
            }
            if let Some(end) = request
                .args
                .get("end_time")
                .and_then(|value| feishu_time_arg(value))
            {
                cli_args.extend(["--end".to_owned(), end]);
            }
        }
        "search-messages" => {
            cli_args.extend([
                "im".to_owned(),
                "+messages-search".to_owned(),
                "--as".to_owned(),
                "user".to_owned(),
                "--page-all".to_owned(),
                "--page-size".to_owned(),
                "50".to_owned(),
                "--format".to_owned(),
                "json".to_owned(),
            ]);
            if let Some(start) = request
                .args
                .get("start_time")
                .and_then(|value| feishu_time_arg(value))
            {
                cli_args.extend(["--start".to_owned(), start]);
            }
            if let Some(end) = request
                .args
                .get("end_time")
                .and_then(|value| feishu_time_arg(value))
            {
                cli_args.extend(["--end".to_owned(), end]);
            }
        }
        "auth-status" => {
            cli_args.extend(["auth".to_owned(), "status".to_owned()]);
        }
        other => {
            return Ok(bridge_error_for(
                "feishu",
                "https://github.com/larksuite/cli",
                "FEISHU_UNSUPPORTED_COMMAND",
                &format!("飞书官方CLI暂不支持应用命令：{other}"),
                true,
                started_at,
            ));
        }
    }

    let profile_cache = cache_dir.join(&profile.id);
    let tmp_dir = profile_cache.join("tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    let mut output = match run_feishu_cli_output(
        &cli_path,
        &cli_args,
        lark_config_dir.as_deref(),
        &tmp_dir,
        active_pids,
    )
    .await
    {
        Ok(output) => output,
        Err(err) => {
            return Ok(bridge_error_for(
                "feishu",
                "https://github.com/larksuite/cli",
                "FEISHU_CLI_MISSING",
                &format!("无法启动飞书官方CLI：{err}"),
                true,
                started_at,
            ));
        }
    };

    if request.command != "auth-status" && !output.status.success() {
        let stderr = sanitize_feishu_cli_output(&String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let sanitized_stdout = sanitize_feishu_cli_output(&stdout);
        if classify_feishu_cli_error(&sanitized_stdout, &stderr)
            .is_some_and(|error| error.code == "FEISHU_NOT_AUTHENTICATED")
            && run_feishu_auth_preflight(
                &cli_path,
                profile_name,
                lark_config_dir.as_deref(),
                active_pids,
            )
            .await?
            .is_none()
        {
            output = run_feishu_cli_output(
                &cli_path,
                &cli_args,
                lark_config_dir.as_deref(),
                &tmp_dir,
                active_pids,
            )
            .await?;
        }
    }

    let stderr = sanitize_feishu_cli_output(&String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let sanitized_stdout = sanitize_feishu_cli_output(&stdout);
        let detail = [sanitized_stdout.trim(), stderr.trim()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(error) = classify_feishu_cli_error(&sanitized_stdout, &stderr) {
            return Ok(BridgeEnvelope {
                ok: false,
                data: serde_json::Value::Null,
                warnings: Vec::new(),
                error: Some(error),
                meta: serde_json::json!({
                    "platform": "feishu",
                    "source": "https://github.com/larksuite/cli",
                    "duration_ms": started_at.elapsed().as_millis()
                }),
            });
        }
        let message = if detail.is_empty() {
            "飞书官方CLI执行失败。".to_owned()
        } else {
            format!("飞书官方CLI执行失败：{detail}")
        };
        return Ok(bridge_error_for(
            "feishu",
            "https://github.com/larksuite/cli",
            "FEISHU_CLI_FAILED",
            &message,
            true,
            started_at,
        ));
    }

    let sanitized_stdout = sanitize_feishu_cli_output(&stdout);
    let raw = parse_feishu_cli_json(&stdout)
        .unwrap_or_else(|| serde_json::json!({ "text": sanitized_stdout.clone() }));
    if request.command == "auth-status" {
        if let Some(error) = validate_feishu_auth_status(&raw) {
            return Ok(BridgeEnvelope {
                ok: false,
                data: raw.clone(),
                warnings: Vec::new(),
                error: Some(error),
                meta: serde_json::json!({
                    "platform": "feishu",
                    "source": "https://github.com/larksuite/cli",
                    "duration_ms": started_at.elapsed().as_millis()
                }),
            });
        }
    } else if let Some(error) = classify_feishu_cli_structured_error(&raw, &stderr) {
        return Ok(BridgeEnvelope {
            ok: false,
            data: raw.clone(),
            warnings: Vec::new(),
            error: Some(error),
            meta: serde_json::json!({
                "platform": "feishu",
                "source": "https://github.com/larksuite/cli",
                "duration_ms": started_at.elapsed().as_millis()
            }),
        });
    }
    let data = match request.command.as_str() {
        "list-chats" => normalize_feishu_chats(&raw),
        "fetch-messages" => normalize_feishu_messages(&raw, &request.args),
        "search-messages" => normalize_feishu_message_sessions(&raw),
        "auth-status" => serde_json::json!({ "authenticated": true, "raw": raw }),
        _ => serde_json::Value::Array(Vec::new()),
    };
    Ok(BridgeEnvelope {
        ok: true,
        data,
        warnings: vec![stderr]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect(),
        error: None,
        meta: serde_json::json!({
            "platform": "feishu",
            "source": "https://github.com/larksuite/cli",
            "duration_ms": started_at.elapsed().as_millis()
        }),
    })
}

fn feishu_lark_config_dir(config: &serde_json::Value) -> Option<PathBuf> {
    if let Some(config_dir) = config
        .get("larkConfigDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        return Some(expand_home(config_dir));
    }
    if let Some(home_dir) = config
        .get("homeDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        let legacy_dir = expand_home(home_dir).join(".lark-cli");
        if legacy_dir.join("config.json").exists() {
            return Some(legacy_dir);
        }
    }
    config
        .get("configDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| expand_home(value).join(".lark-cli"))
}

async fn run_feishu_cli_output(
    cli_path: &std::path::Path,
    cli_args: &[String],
    lark_config_dir: Option<&std::path::Path>,
    tmp_dir: &std::path::Path,
    active_pids: Option<&Mutex<Vec<u32>>>,
) -> anyhow::Result<std::process::Output> {
    let mut command = official_cli_command(cli_path);
    apply_official_cli_env(&mut command);
    apply_feishu_cli_env(&mut command);
    if let Some(lark_config_dir) = lark_config_dir {
        command.env("LARKSUITE_CLI_CONFIG_DIR", lark_config_dir);
    }
    command
        .args(cli_args)
        .env("TMPDIR", tmp_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command.spawn()?;
    wait_for_official_cli_output(child, active_pids, OFFICIAL_CLI_COMMAND_TIMEOUT).await
}

async fn run_feishu_auth_preflight(
    cli_path: &std::path::Path,
    profile_name: &str,
    lark_config_dir: Option<&std::path::Path>,
    active_pids: Option<&Mutex<Vec<u32>>>,
) -> anyhow::Result<Option<super::BridgeError>> {
    let mut command = official_cli_command(cli_path);
    apply_official_cli_env(&mut command);
    apply_feishu_cli_env(&mut command);
    if let Some(lark_config_dir) = lark_config_dir {
        command.env("LARKSUITE_CLI_CONFIG_DIR", lark_config_dir);
    }
    command
        .arg("--profile")
        .arg(profile_name)
        .arg("auth")
        .arg("status")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command.spawn()?;
    let output =
        wait_for_official_cli_output(child, active_pids, OFFICIAL_CLI_AUTH_TIMEOUT).await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = sanitize_feishu_cli_output(&String::from_utf8_lossy(&output.stderr));
    let sanitized_stdout = sanitize_feishu_cli_output(&stdout);
    if !output.status.success() {
        return Ok(
            classify_feishu_cli_error(&sanitized_stdout, &stderr).or_else(|| {
                Some(super::BridgeError {
                    code: "FEISHU_NOT_AUTHENTICATED".to_owned(),
                    message: super::errors::feishu_user_auth_incomplete_message(),
                    recoverable: true,
                })
            }),
        );
    }

    let raw = parse_feishu_cli_json(&stdout)
        .unwrap_or_else(|| serde_json::json!({ "text": sanitized_stdout.clone() }));
    Ok(validate_feishu_auth_status(&raw))
}

fn apply_feishu_cli_env(command: &mut tokio::process::Command) {
    // lark-cli 在部分本机网络环境会自动走到不稳定代理链路，消息检索表现为
    // network/transport EOF。显式禁用代理探测后仍保留系统直连网络，避免误判为授权失效。
    command.env("LARK_CLI_NO_PROXY", "1");
}
