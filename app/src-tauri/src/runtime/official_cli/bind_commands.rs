use std::path::Path;

pub fn default_bind_command(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    match platform {
        "wecom" => format!(
            "WECOM_CLI_CONFIG_DIR='{}' '{}' init",
            shell_single_quote(config_dir),
            shell_single_quote(cli_path)
        ),
        "feishu" => {
            let lark_config_dir = config_dir.join(".lark-cli");
            let required_scopes = [
                "search:message",
                "im:chat:read",
                "im:message:readonly",
                "im:message.reactions:read",
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
            format!(
                "LARKSUITE_CLI_CONFIG_DIR='{}' '{}' config init --new --brand feishu --name '{}' && LARKSUITE_CLI_CONFIG_DIR='{}' '{}' --profile '{}' auth login --scope '{}' && LARKSUITE_CLI_CONFIG_DIR='{}' '{}' --profile '{}' auth status",
                shell_single_quote(&lark_config_dir),
                shell_single_quote(cli_path),
                shell_single_quote_text(profile_name),
                shell_single_quote(&lark_config_dir),
                shell_single_quote(cli_path),
                shell_single_quote_text(profile_name),
                shell_single_quote_text(&required_scopes),
                shell_single_quote(&lark_config_dir),
                shell_single_quote(cli_path),
                shell_single_quote_text(profile_name)
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
            format!(
                "DWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' auth login --force &&\nDWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' auth status --format json &&\nDWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' pat chmod chat.message:list --agentCode '{}' --grant-type permanent &&\nDWS_CONFIG_DIR='{}' DWS_CACHE_DIR='{}' DWS_KEYCHAIN_DIR='{}' DWS_AUTH_IDENTITY='{}' DWS_TENANT='{}' DINGTALK_DWS_AGENTCODE='{}' '{}' contact user get-self --format json",
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path),
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path),
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path),
                shell_single_quote_text(auth_identity),
                shell_single_quote(config_dir),
                shell_single_quote(&cache_dir),
                shell_single_quote(&keychain_dir),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote_text(auth_identity),
                shell_single_quote(cli_path)
            )
        }
        _ => shell_single_quote(cli_path),
    }
}

pub fn command_shell_name() -> &'static str {
    "macOS终端"
}

pub fn platform_label(platform: &str) -> &str {
    match platform {
        "wecom" => "企业微信",
        "feishu" => "飞书",
        "dingtalk" => "钉钉",
        _ => platform,
    }
}

pub fn platform_cli_label(platform: &str) -> String {
    format!("{}官方CLI", platform_label(platform))
}

fn shell_single_quote(path: &Path) -> String {
    shell_single_quote_text(&path.to_string_lossy())
}

fn shell_single_quote_text(value: &str) -> String {
    value.replace('\'', "'\\''")
}
