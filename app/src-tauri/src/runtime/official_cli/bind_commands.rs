use std::path::Path;

pub fn default_bind_command(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    match platform {
        "wecom" => build_shell_command(
            &[("WECOM_CLI_CONFIG_DIR", config_dir)],
            &[],
            &[CommandStep::new(cli_path, &["init"])],
        ),
        "feishu" => {
            let lark_config_dir = config_dir.join(".lark-cli");
            let required_scopes = [
                "search:message",
                "im:chat:read",
                "im:message:readonly",
                "im:message.p2p_msg:get_as_user",
                "im:message.group_msg:get_as_user",
                "contact:user.base:readonly",
                "contact:user.basic_profile:readonly",
            ]
            .join(" ");
            let profile_name = config_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("im-board-feishu");
            build_shell_command(
                &[("LARKSUITE_CLI_CONFIG_DIR", &lark_config_dir)],
                &[],
                &[
                    CommandStep::new(
                        cli_path,
                        &["config", "init", "--new", "--brand", "feishu", "--name", profile_name],
                    ),
                    CommandStep::new(
                        cli_path,
                        &["--profile", profile_name, "auth", "login", "--scope", &required_scopes],
                    ),
                    CommandStep::new(
                        cli_path,
                        &["--profile", profile_name, "auth", "status", "--verify"],
                    ),
                ],
            )
        }
        "dingtalk" => {
            let cache_dir = config_dir.join("cache");
            let keychain_dir = config_dir.join("keychain");
            let auth_identity = config_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("im-board-dingtalk");
            build_shell_command(
                &[
                    ("DWS_CONFIG_DIR", config_dir),
                    ("DWS_CACHE_DIR", &cache_dir),
                    ("DWS_KEYCHAIN_DIR", &keychain_dir),
                ],
                &[
                    ("DWS_AUTH_IDENTITY", auth_identity),
                    ("DWS_TENANT", auth_identity),
                    ("DINGTALK_DWS_AGENTCODE", auth_identity),
                ],
                &[
                    CommandStep::new(cli_path, &["auth", "login", "--force"]),
                    CommandStep::new(cli_path, &["auth", "status", "--format", "json"]),
                    CommandStep::new(
                        cli_path,
                        &[
                            "pat",
                            "chmod",
                            "chat.message:list",
                            "--agentCode",
                            auth_identity,
                            "--grant-type",
                            "permanent",
                        ],
                    ),
                    CommandStep::new(cli_path, &["contact", "user", "get-self", "--format", "json"]),
                ],
            )
        }
        _ => shell_single_quote(cli_path),
    }
}

pub fn platform_label(platform: &str) -> &str {
    match platform {
        "wechat" => "微信",
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => platform,
    }
}

fn shell_single_quote(path: &Path) -> String {
    shell_single_quote_text(&path.to_string_lossy())
}

fn shell_single_quote_text(value: &str) -> String {
    value.replace('\'', "'\\''")
}

struct CommandStep<'a> {
    cli_path: &'a Path,
    args: &'a [&'a str],
}

impl<'a> CommandStep<'a> {
    fn new(cli_path: &'a Path, args: &'a [&'a str]) -> Self {
        Self { cli_path, args }
    }
}

fn build_shell_command(
    path_envs: &[(&str, &Path)],
    text_envs: &[(&str, &str)],
    steps: &[CommandStep<'_>],
) -> String {
    if cfg!(windows) {
        build_powershell_command(path_envs, text_envs, steps)
    } else {
        build_posix_command(path_envs, text_envs, steps)
    }
}

fn build_posix_command(
    path_envs: &[(&str, &Path)],
    text_envs: &[(&str, &str)],
    steps: &[CommandStep<'_>],
) -> String {
    let path_env_prefix = path_envs
        .iter()
        .map(|(name, value)| format!("{name}='{}'", shell_single_quote(value)))
        .collect::<Vec<_>>();
    let text_env_prefix = text_envs
        .iter()
        .map(|(name, value)| format!("{name}='{}'", shell_single_quote_text(value)));
    let env_prefix = path_env_prefix
        .into_iter()
        .chain(text_env_prefix)
        .collect::<Vec<_>>()
        .join(" ");
    steps
        .iter()
        .map(|step| {
            let invocation = std::iter::once(format!("'{}'", shell_single_quote(step.cli_path)))
                .chain(
                    step.args
                        .iter()
                        .map(|arg| format!("'{}'", shell_single_quote_text(arg))),
                )
                .collect::<Vec<_>>()
                .join(" ");
            if env_prefix.is_empty() {
                invocation
            } else {
                format!("{env_prefix} {invocation}")
            }
        })
        .collect::<Vec<_>>()
        .join(" &&\n")
}

fn build_powershell_command(
    path_envs: &[(&str, &Path)],
    text_envs: &[(&str, &str)],
    steps: &[CommandStep<'_>],
) -> String {
    // Windows 仍可能运行 PowerShell 5.1，不能依赖 PowerShell 7 才支持的 `&&` 串联。
    let mut parts = path_envs
        .iter()
        .map(|(name, value)| format!("$env:{name}={}", powershell_quote_path(value)))
        .collect::<Vec<_>>();
    parts.extend(
        text_envs
            .iter()
            .map(|(name, value)| format!("$env:{name}={}", powershell_quote_text(value))),
    );
    for (index, step) in steps.iter().enumerate() {
        if index > 0 {
            parts.push("if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }".to_owned());
        }
        let invocation = std::iter::once(format!("& {}", powershell_quote_path(step.cli_path)))
            .chain(step.args.iter().map(|arg| powershell_quote_text(arg)))
            .collect::<Vec<_>>()
            .join(" ");
        parts.push(invocation);
    }
    parts.join("; ")
}

fn powershell_quote_path(path: &Path) -> String {
    powershell_quote_text(&path.to_string_lossy())
}

fn powershell_quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
