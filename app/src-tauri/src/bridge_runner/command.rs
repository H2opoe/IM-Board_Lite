pub(super) const OFFICIAL_CLI_AUTH_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(45);
pub(super) const OFFICIAL_CLI_COMMAND_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(180);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(super) fn register_pid(active_pids: Option<&Mutex<Vec<u32>>>, child_id: Option<u32>) {
    if let (Some(active_pids), Some(child_id)) = (active_pids, child_id) {
        if let Ok(mut pids) = active_pids.lock() {
            pids.push(child_id);
        }
    }
}

pub(super) fn unregister_pid(active_pids: Option<&Mutex<Vec<u32>>>, child_id: Option<u32>) {
    if let (Some(active_pids), Some(child_id)) = (active_pids, child_id) {
        if let Ok(mut pids) = active_pids.lock() {
            pids.retain(|pid| *pid != child_id);
        }
    }
}

fn is_node_cli_entry(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("js"))
}

pub(super) fn official_cli_command(path: &Path) -> Command {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let mut command = if is_node_cli_entry(path) {
        let mut command = Command::new("node");
        command.arg(path);
        command
    } else if cfg!(windows)
        && (extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"))
    {
        let mut command = Command::new("cmd.exe");
        command.args(["/d", "/c"]).arg(path);
        command
    } else {
        Command::new(path)
    };
    command.kill_on_drop(true);
    hide_windows_console(&mut command);
    command
}

pub(super) async fn wait_for_official_cli_output(
    child: tokio::process::Child,
    active_pids: Option<&Mutex<Vec<u32>>>,
    timeout: std::time::Duration,
) -> anyhow::Result<std::process::Output> {
    let child_id = child.id();
    register_pid(active_pids, child_id);
    let result = tokio::time::timeout(timeout, child.wait_with_output()).await;
    unregister_pid(active_pids, child_id);
    match result {
        Ok(output) => Ok(output?),
        Err(_) => anyhow::bail!(
            "官方CLI执行超过{}秒，已终止无响应进程，请重试。",
            timeout.as_secs()
        ),
    }
}

pub(super) fn apply_official_cli_env(command: &mut Command) {
    if let Some(path) = official_cli_path() {
        command.env("PATH", path);
    }
    command.env("PYTHONUTF8", "1");
    command.env("PYTHONIOENCODING", "utf-8");
    hide_windows_console(command);
}

fn hide_windows_console(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

fn official_cli_path() -> Option<std::ffi::OsString> {
    let mut entries = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&path));
    }
    entries.extend(node_path_candidates());
    dedupe_existing_paths(&mut entries);
    std::env::join_paths(entries).ok()
}

fn node_path_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if cfg!(windows) {
        for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA", "APPDATA"] {
            if let Some(root) = std::env::var_os(key) {
                let root = PathBuf::from(root);
                candidates.push(root.join("nodejs"));
                candidates.push(root.join("Programs").join("nodejs"));
                candidates.push(root.join("npm"));
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        collect_child_bin_dirs(
            &home.join(".nvm").join("versions").join("node"),
            &mut candidates,
        );
        collect_fnm_node_dirs(&home.join(".fnm").join("node-versions"), &mut candidates);
        collect_fnm_node_dirs(
            &home
                .join(".local")
                .join("share")
                .join("fnm")
                .join("node-versions"),
            &mut candidates,
        );
        candidates.push(home.join(".volta").join("bin"));
        candidates.push(home.join(".local").join("bin"));
    }
    candidates
}

fn collect_child_bin_dirs(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        output.push(entry.path().join("bin"));
    }
}

fn collect_fnm_node_dirs(root: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        output.push(entry.path().join("installation").join("bin"));
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
