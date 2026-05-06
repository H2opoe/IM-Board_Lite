use std::path::Path;

use super::resolver::is_node_cli_entry;
use super::{
    WINDOWS_DINGTALK_AUTH_TOKEN_VALUE, WINDOWS_DINGTALK_PROFILE_TOKEN_FILE,
    WINDOWS_DINGTALK_REGISTRY_KEY,
};

pub fn default_bind_command(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    if cfg!(windows) {
        return default_bind_command_windows(platform, cli_path, config_dir);
    }
    match platform {
        "wechat" => {
            let cli_invocation = shell_cli_invocation(cli_path);
            format!(
                "mkdir -p '{}' && {} init --config '{}/config.json' --keys-file '{}/all_keys.json' --force",
                shell_single_quote(config_dir),
                cli_invocation,
                shell_single_quote(config_dir),
                shell_single_quote(config_dir)
            )
        }
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
                "LARKSUITE_CLI_CONFIG_DIR='{}' '{}' config init --new --brand feishu --name '{}' && LARKSUITE_CLI_CONFIG_DIR='{}' '{}' --profile '{}' auth login --scope '{}' && LARKSUITE_CLI_CONFIG_DIR='{}' '{}' --profile '{}' auth status --verify",
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

fn default_bind_command_windows(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    match platform {
        "wechat" => format!(
            "New-Item -ItemType Directory -Force -Path {} | Out-Null; {} init --config {} --keys-file {} --force",
            powershell_single_quote(config_dir),
            powershell_cli_invocation(cli_path),
            powershell_single_quote(&config_dir.join("config.json")),
            powershell_single_quote(&config_dir.join("all_keys.json"))
        ),
        "wecom" => format!(
            "$env:WECOM_CLI_CONFIG_DIR = {}; {} init",
            powershell_single_quote(config_dir),
            powershell_cli_invocation(cli_path)
        ),
        "feishu" => {
            let home_dir = config_dir.join("home");
            let lark_config_dir = config_dir.join(".lark-cli");
            let appdata_dir = home_dir.join("AppData").join("Roaming");
            let local_appdata_dir = home_dir.join("AppData").join("Local");
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
            let cli = powershell_cli_invocation(cli_path);
            format!(
                "$env:HOME = {}; $env:USERPROFILE = {}; $env:APPDATA = {}; $env:LOCALAPPDATA = {}; $env:LARKSUITE_CLI_CONFIG_DIR = {}; {} config init --new --brand feishu --name {}; if ($LASTEXITCODE -eq 0) {{ {} --profile {} auth login --scope {} }}; if ($LASTEXITCODE -eq 0) {{ {} --profile {} auth status --verify }}",
                powershell_single_quote(&home_dir),
                powershell_single_quote(&home_dir),
                powershell_single_quote(&appdata_dir),
                powershell_single_quote(&local_appdata_dir),
                powershell_single_quote(&lark_config_dir),
                cli,
                powershell_single_quote_text(profile_name),
                cli,
                powershell_single_quote_text(profile_name),
                powershell_single_quote_text(&required_scopes),
                cli,
                powershell_single_quote_text(profile_name)
            )
        }
        "dingtalk" => {
            let cache_dir = config_dir.join("cache");
            let keychain_dir = config_dir.join("keychain");
            let home_dir = config_dir.join("home");
            let appdata_dir = home_dir.join("AppData").join("Roaming");
            let local_appdata_dir = home_dir.join("AppData").join("Local");
            let token_path = keychain_dir.join(WINDOWS_DINGTALK_PROFILE_TOKEN_FILE);
            let auth_identity = config_dir
                .parent()
                .and_then(|path| path.file_name())
                .and_then(|value| value.to_str())
                .unwrap_or("im-board-dingtalk");
            let cli = powershell_cli_invocation(cli_path);
            format!(
                "$env:HOME = {}; $env:USERPROFILE = {}; $env:APPDATA = {}; $env:LOCALAPPDATA = {}; $env:DWS_CONFIG_DIR = {}; $env:DWS_CACHE_DIR = {}; $env:DWS_KEYCHAIN_DIR = {}; $env:DWS_AUTH_IDENTITY = {}; $env:DWS_TENANT = {}; $env:DINGTALK_DWS_AGENTCODE = {}\n$dwsRegistryKey = {}; $dwsTokenValue = {}; $dwsProfileTokenPath = {}; $dwsPreviousToken = $null\nif (Test-Path $dwsRegistryKey) {{ $dwsPrevious = Get-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue; if ($null -ne $dwsPrevious) {{ $dwsPreviousToken = $dwsPrevious.$dwsTokenValue }} }}\nfunction Set-DwsProfileToken {{ New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dwsProfileTokenPath) | Out-Null; if (Test-Path $dwsProfileTokenPath) {{ New-Item -Force -Path $dwsRegistryKey | Out-Null; Set-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -Value ([System.IO.File]::ReadAllText($dwsProfileTokenPath).Trim()) }} elseif (Test-Path $dwsRegistryKey) {{ Remove-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue }} }}\nfunction Save-DwsProfileToken {{ $dwsCurrent = Get-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue; if ($null -eq $dwsCurrent) {{ throw '钉钉授权没有完成：未写入当前账号的授权令牌。' }}; [System.IO.File]::WriteAllText($dwsProfileTokenPath, [string]$dwsCurrent.$dwsTokenValue, [System.Text.Encoding]::ASCII) }}\nfunction Restore-DwsPreviousToken {{ if ($null -eq $dwsPreviousToken) {{ if (Test-Path $dwsRegistryKey) {{ Remove-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -ErrorAction SilentlyContinue }} }} else {{ New-Item -Force -Path $dwsRegistryKey | Out-Null; Set-ItemProperty -Path $dwsRegistryKey -Name $dwsTokenValue -Value $dwsPreviousToken }} }}\ntry {{ Set-DwsProfileToken; {} auth login --force; if ($LASTEXITCODE -eq 0) {{ {} auth status --format json }}; if ($LASTEXITCODE -eq 0) {{ {} pat chmod chat.message:list --agentCode {} --grant-type permanent }}; if ($LASTEXITCODE -eq 0) {{ {} contact user get-self --format json }}; if ($LASTEXITCODE -eq 0) {{ Save-DwsProfileToken }} }} finally {{ Restore-DwsPreviousToken }}",
                powershell_single_quote(&home_dir),
                powershell_single_quote(&home_dir),
                powershell_single_quote(&appdata_dir),
                powershell_single_quote(&local_appdata_dir),
                powershell_single_quote(config_dir),
                powershell_single_quote(&cache_dir),
                powershell_single_quote(&keychain_dir),
                powershell_single_quote_text(auth_identity),
                powershell_single_quote_text(auth_identity),
                powershell_single_quote_text(auth_identity),
                powershell_single_quote_text(WINDOWS_DINGTALK_REGISTRY_KEY),
                powershell_single_quote_text(WINDOWS_DINGTALK_AUTH_TOKEN_VALUE),
                powershell_single_quote(&token_path),
                cli,
                cli,
                cli,
                powershell_single_quote_text(auth_identity),
                cli
            )
        }
        _ => powershell_single_quote(cli_path),
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

fn shell_cli_invocation(path: &Path) -> String {
    let quoted = shell_single_quote(path);
    if is_node_cli_entry(path) {
        format!("node {quoted}")
    } else {
        quoted
    }
}

fn shell_single_quote_text(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn powershell_single_quote(path: &Path) -> String {
    powershell_single_quote_text(&path.to_string_lossy())
}

fn powershell_single_quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn powershell_cli_invocation(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "js" {
        format!("node {}", powershell_single_quote(path))
    } else {
        format!("& {}", powershell_single_quote(path))
    }
}
