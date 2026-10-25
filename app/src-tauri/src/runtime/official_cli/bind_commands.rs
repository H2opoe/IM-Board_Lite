use std::path::Path;

pub fn default_bind_command(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    default_bind_command_windows(platform, cli_path, config_dir)
}

pub fn command_shell_name() -> &'static str {
    "Windows PowerShell"
}

#[derive(Debug, Clone)]
struct OfficialCliProfileEnv {
    config_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    tmp_dir: std::path::PathBuf,
}

impl OfficialCliProfileEnv {
    fn new(config_dir: &Path) -> Self {
        let cache_dir = config_dir.join("cache");
        let tmp_dir = cache_dir.join("tmp");
        Self {
            config_dir: config_dir.to_path_buf(),
            cache_dir,
            tmp_dir,
        }
    }

    fn lark_config_dir(&self) -> std::path::PathBuf {
        self.config_dir.join(".lark-cli")
    }

    fn dws_keychain_dir(&self) -> std::path::PathBuf {
        self.config_dir.join("keychain")
    }
}

fn default_bind_command_windows(platform: &str, cli_path: &Path, config_dir: &Path) -> String {
    let env = OfficialCliProfileEnv::new(config_dir);
    match platform {
        "wecom" => format!(
            "$env:WECOM_CLI_CONFIG_DIR = {}; {} auth init",
            powershell_single_quote(config_dir),
            powershell_cli_invocation(cli_path)
        ),
        "feishu" => {
            // Windows 开发环境可能通过 SMB 访问源码，绑定命令只隔离官方 CLI 自身配置目录，
            // 不再伪造 HOME/USERPROFILE/APPDATA，避免把系统级运行环境改到项目共享目录里。
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
            let cli = powershell_cli_invocation(cli_path);
            format!(
                "$env:LARKSUITE_CLI_CONFIG_DIR = {}; {} config init --new --brand feishu --name {}; if ($LASTEXITCODE -eq 0) {{ {} --profile {} auth login --scope {} }}; if ($LASTEXITCODE -eq 0) {{ {} --profile {} auth status }}",
                powershell_single_quote(&env.lark_config_dir()),
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
            let identity = config_dir
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|p| p.to_str())
                .unwrap_or("im-board-dingtalk");
            let cli = "Invoke-IMBoardDws";
            let setup = format!("$env:DWS_CONFIG_DIR = {}; $env:DWS_CACHE_DIR = {}; $env:DWS_KEYCHAIN_DIR = {}; $env:TMP = {}; $env:TEMP = $env:TMP; $env:TMPDIR = $env:TMP; $env:DWS_AUTH_IDENTITY = {}; $env:DWS_TENANT = $env:DWS_AUTH_IDENTITY; $env:DINGTALK_DWS_AGENTCODE = $env:DWS_AUTH_IDENTITY;",powershell_single_quote(&env.config_dir),powershell_single_quote(&env.cache_dir),powershell_single_quote(&env.dws_keychain_dir()),powershell_single_quote(&env.tmp_dir),powershell_single_quote_text(identity));
            let body = format!(
                "Write-Host '[1/3] 正在登录钉钉，请完成浏览器扫码。'; {cli} auth login --force; if ($LASTEXITCODE -eq 0) {{ {cli} auth status --format json }}; if ($LASTEXITCODE -eq 0) {{ Write-Host '[2/3] 登录已成功，正在授权消息读取权限（最长180秒）；请留意浏览器授权页。'; Invoke-IMBoardDwsBounded -TimeoutSeconds 180 -NativeArguments @('pat', 'chmod', 'chat.message:list', '--agentCode', {}, '--grant-type', 'permanent', '--format', 'json') }}; if ($LASTEXITCODE -eq 0) {{ Write-Host '[3/3] 正在核验账号身份。'; {cli} contact user get-self --format json }}; if ($LASTEXITCODE -eq 0) {{ Write-Host '钉钉授权检查已完成。请返回 IM-Board 保存配置。' }}",
                powershell_single_quote_text(identity)
            );
            format!(
                "{} {}",
                setup,
                crate::runtime::dws_auth::session(config_dir, cli_path, &body, false)
            )
        }
        _ => powershell_single_quote(cli_path),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn command(platform: &str) -> String {
        default_bind_command(
            platform,
            Path::new(r"C:\Tools\cli.exe"),
            Path::new("/Users/me/AppData/Roaming/IMBoard/Profiles/profile-a/feishu"),
        )
    }

    #[test]
    fn wecom_bind_uses_only_wecom_config_dir() {
        let command = command("wecom");
        assert!(command.contains("WECOM_CLI_CONFIG_DIR"));
        assert!(!command.contains("USERPROFILE"));
        assert!(!command.contains("APPDATA"));
        assert!(!command.contains("LOCALAPPDATA"));
    }

    #[test]
    fn feishu_bind_uses_larksuite_config_profile_and_scopes() {
        let command = command("feishu");
        assert!(command.contains("LARKSUITE_CLI_CONFIG_DIR"));
        assert!(command.contains("--profile 'profile-a' auth login"));
        assert!(command.contains("im:message.group_msg:get_as_user"));
        assert!(!command.contains("HOME"));
        assert!(!command.contains("USERPROFILE"));
        assert!(!command.contains("APPDATA"));
        assert!(!command.contains("LOCALAPPDATA"));
    }

    #[test]
    fn dingtalk_bind_uses_account_namespace_without_global_token_swap() {
        let command = command("dingtalk");
        assert!(command.contains("DWS_CONFIG_DIR"));
        assert!(command.contains("DWS_CACHE_DIR"));
        assert!(command.contains("DWS_KEYCHAIN_DIR"));
        assert!(command.contains("$env:TMP = "));
        assert!(command.contains("$env:TEMP = "));
        assert!(command.contains("IM_BOARD_DWS_NAMESPACE"));
        assert!(command.contains("Invoke-IMBoardDws auth login --force"));
        assert!(command.contains("[1/3]"));
        assert!(command.contains("[2/3]"));
        assert!(command.contains("[3/3]"));
        assert!(command.contains("Invoke-IMBoardDwsBounded -TimeoutSeconds 180"));
        assert!(command.contains("'chat.message:list'"));
        assert!(command.contains("'--grant-type', 'permanent'"));
        assert!(!command.contains("Write-DwsSessionToken"));
        assert!(command.contains("Get-FileHash"));
        assert!(!command.contains("dwsPreviousToken"));
        assert!(!command.contains("USERPROFILE"));
        assert!(!command.contains("APPDATA"));
        assert!(!command.contains("LOCALAPPDATA"));
    }
}
