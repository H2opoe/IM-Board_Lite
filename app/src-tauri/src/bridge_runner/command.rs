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

#[cfg(windows)]
pub(super) fn official_cli_command(path: &Path) -> Command {
    if is_node_cli_entry(path) {
        let mut command = Command::new("node");
        command.arg(path);
        hide_windows_console(&mut command);
        return command;
    }
    windows_command_for_path(path)
}

#[cfg(not(windows))]
pub(super) fn official_cli_command(path: &Path) -> Command {
    if is_node_cli_entry(path) {
        let mut command = Command::new("node");
        command.arg(path);
        return command;
    }
    Command::new(path)
}

#[cfg(windows)]
pub(super) fn hide_windows_console(command: &mut Command) {
    // Windows GUI版同步消息时会频繁启动官方CLI；隐藏子进程控制台，避免每次拉取会话历史都弹出终端窗口。
    command.creation_flags(CREATE_NO_WINDOW);
}

pub(super) fn apply_official_cli_env(command: &mut Command) {
    if let Some(path) = official_cli_path() {
        command.env("PATH", path);
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
        for key in [
            "ProgramFiles",
            "ProgramFiles(x86)",
            "LOCALAPPDATA",
            "APPDATA",
        ] {
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

