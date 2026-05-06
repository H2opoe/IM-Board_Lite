use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tokio::process::Command;

use crate::storage::models::ImProfile;

use super::paths::{
    expand_home, is_dependency_free_cli, is_windows_wechat_python_launcher, official_cli_roots,
};
use super::{hide_windows_console, register_pid, unregister_pid, APP_DATA_DIR_NAME};

pub(super) struct WindowsWechatProfilePaths {
    pub(super) config_path: PathBuf,
    pub(super) keys_path: PathBuf,
    pub(super) cache_dir: PathBuf,
}

pub(super) async fn run_tracked_output(
    mut command: Command,
    active_pids: Option<&Mutex<Vec<u32>>>,
) -> anyhow::Result<std::process::Output> {
    let child = command.spawn()?;
    let child_id = child.id();
    register_pid(active_pids, child_id);
    let output = child.wait_with_output().await?;
    unregister_pid(active_pids, child_id);
    Ok(output)
}

pub(super) fn resolve_windows_wechat_cli(
    profile: Option<&ImProfile>,
    resource_dir: &Path,
) -> Option<PathBuf> {
    if let Some(path) = resolve_hot_updated_windows_wechat_cli(resource_dir) {
        return Some(path);
    }
    if let Some(configured) = profile
        .and_then(|profile| profile.config_json.get("cliPath"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        if let Some(path) = resolve_windows_command(configured) {
            return Some(path);
        }
    }
    if let Ok(configured) = std::env::var("IMD_WECHAT_QUERY_CLI") {
        if let Some(path) = resolve_windows_command(&configured) {
            return Some(path);
        }
    }
    None
}

fn resolve_hot_updated_windows_wechat_cli(resource_dir: &Path) -> Option<PathBuf> {
    official_cli_roots(resource_dir)
        .into_iter()
        .flat_map(|root| {
            let package_root = root.join("wechat").join("node_modules");
            [
                root.join("wechat").join("bin").join("wechat-cli.cmd"),
                package_root.join(".bin").join("wechat-cli.exe"),
                package_root.join(".bin").join("wechat-cli"),
            ]
        })
        .find(|path| {
            path.exists()
                && (is_dependency_free_cli(path) || is_windows_wechat_python_launcher(path))
        })
}

fn resolve_windows_command(command: &str) -> Option<PathBuf> {
    let expanded = expand_home(command);
    if expanded.is_absolute() || command.contains('\\') || command.contains('/') {
        return expanded.exists().then_some(expanded);
    }
    let path = std::env::var_os("PATH")?;
    let extensions: &[&str] = if Path::new(command).extension().is_some() {
        &[""]
    } else {
        &["", ".exe", ".cmd", ".bat"]
    };
    std::env::split_paths(&path).find_map(|entry| {
        extensions
            .iter()
            .map(|extension| entry.join(format!("{command}{extension}")))
            .find(|candidate| candidate.exists())
    })
}

pub(super) fn windows_command_for_path(path: &Path) -> Command {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "cmd" || extension == "bat" {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(path);
        hide_windows_console(&mut command);
        command
    } else {
        let mut command = Command::new(path);
        hide_windows_console(&mut command);
        command
    }
}

pub(super) fn windows_wechat_profile_paths(
    profile: &ImProfile,
    cache_root: &Path,
) -> WindowsWechatProfilePaths {
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(APP_DATA_DIR_NAME);
    let profile_dir = profile
        .config_json
        .get("profileDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| app_dir.join("Profiles").join(&profile.id));
    let config_path = profile
        .config_json
        .get("configPath")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| profile_dir.join("config.json"));
    let keys_path = profile
        .config_json
        .get("keysPath")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| profile_dir.join("all_keys.json"));
    let cache_dir = profile
        .config_json
        .get("cacheDir")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(expand_home)
        .unwrap_or_else(|| cache_root.join(&profile.id));
    WindowsWechatProfilePaths {
        config_path,
        keys_path,
        cache_dir,
    }
}

pub(super) fn wechat_command_key(command: &str) -> String {
    match command {
        "list-chats" | "sessions" => "listChats".to_owned(),
        "fetch-messages" | "history" | "fts-history" => "fetchMessages".to_owned(),
        other => other.to_owned(),
    }
}

pub(super) fn wechat_cli_command(config: &serde_json::Value, command: &str) -> String {
    if let Some(commands) = config
        .get("cliCommands")
        .and_then(|value| value.as_object())
    {
        for key in [
            wechat_command_key(command),
            command.to_owned(),
            command.replace('-', "_"),
        ] {
            if let Some(value) = commands
                .get(&key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                return value.trim().to_owned();
            }
        }
    }
    match wechat_command_key(command).as_str() {
        "listChats" => "sessions".to_owned(),
        "fetchMessages" => "history".to_owned(),
        _ => command.to_owned(),
    }
}

fn wechat_cli_arg_name(config: &serde_json::Value, command: &str, arg_name: &str) -> String {
    if let Some(command_args) = config
        .get("cliArgs")
        .and_then(|value| value.as_object())
        .and_then(|arg_maps| {
            [
                wechat_command_key(command),
                command.to_owned(),
                command.replace('-', "_"),
            ]
            .into_iter()
            .find_map(|key| arg_maps.get(&key).and_then(|value| value.as_object()))
        })
    {
        for key in [
            arg_name.to_owned(),
            arg_name.replace('_', "-"),
            camel_name(arg_name),
        ] {
            if let Some(value) = command_args
                .get(&key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                return value.trim().trim_start_matches('-').to_owned();
            }
        }
    }
    arg_name.replace('_', "-")
}

pub(super) fn wechat_cli_arg_placement(
    config: &serde_json::Value,
    command: &str,
    arg_name: &str,
) -> String {
    if let Some(command_args) = config
        .get("cliArgPlacement")
        .and_then(|value| value.as_object())
        .and_then(|placements| {
            [
                wechat_command_key(command),
                command.to_owned(),
                command.replace('-', "_"),
            ]
            .into_iter()
            .find_map(|key| placements.get(&key).and_then(|value| value.as_object()))
        })
    {
        for key in [arg_name.to_owned(), camel_name(arg_name)] {
            if let Some(value) = command_args
                .get(&key)
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
            {
                return value.trim().to_owned();
            }
        }
    }
    if wechat_command_key(command) == "fetchMessages" && arg_name == "chat" {
        "positional".to_owned()
    } else {
        "option".to_owned()
    }
}

pub(super) fn append_wechat_option(
    command: &mut Command,
    config: &serde_json::Value,
    cli_command: &str,
    arg_name: &str,
    value: &str,
) {
    command
        .arg(format!(
            "--{}",
            wechat_cli_arg_name(config, cli_command, arg_name)
        ))
        .arg(value);
}

fn camel_name(name: &str) -> String {
    let mut parts = name.split('_');
    let Some(first) = parts.next() else {
        return String::new();
    };
    let mut output = first.to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first_char) = chars.next() {
            output.extend(first_char.to_uppercase());
            output.push_str(chars.as_str());
        }
    }
    output
}
