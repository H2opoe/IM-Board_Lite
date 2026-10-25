use std::path::{Path, PathBuf};

pub const VERSION: &str = "1.0.65-imboard.1";
pub const SHA256: &str = "36587fa53af02944f0da725ec08ccfb95b4f3a19db247f8815ac238ebf426592";
pub const CAPABILITY: &str = "im-board-dws-isolation-v1";

pub fn verified(path: &Path) -> bool {
    crate::runtime::file_integrity::verify_file(path, SHA256, 64 * 1024 * 1024)
}

pub fn bundled(resource_dir: &Path) -> Result<PathBuf, String> {
    let suffix = format!("runtime/dws/{VERSION}/win-x64/dws.exe");
    let mut candidates = vec![
        resource_dir.join(&suffix),
        resource_dir.join("_up_").join(&suffix),
        resource_dir.join("..").join(&suffix),
    ];
    #[cfg(debug_assertions)]
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(&suffix),
    );
    candidates
        .into_iter()
        .find(|path| verified(path))
        .ok_or_else(|| "内置钉钉隔离运行时缺失或校验失败，请重新安装应用。".into())
}

pub fn resolve(resource_dir: &Path, configured: &Path) -> Result<PathBuf, String> {
    if verified(configured) {
        return Ok(configured.to_path_buf());
    }
    bundled(resource_dir)
}

pub fn stage(resource_dir: &Path, staging: &Path) -> Result<(), String> {
    let source = bundled(resource_dir)?;
    let package = staging.join("node_modules/dingtalk-workspace-cli");
    let vendor = package.join("vendor");
    std::fs::create_dir_all(&vendor).map_err(|e| e.to_string())?;
    let target = vendor.join("dws-windows-x64.exe");
    std::fs::copy(source, &target).map_err(|e| e.to_string())?;
    if !verified(&target) {
        return Err("钉钉隔离运行时复制后校验失败。".into());
    }
    std::fs::write(package.join("package.json"), serde_json::json!({"name":"dingtalk-workspace-cli","version":VERSION,"imBoardIsolation":CAPABILITY}).to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_old_or_tampered_binaries_and_stages_only_pinned_payload() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("dws.exe");
        std::fs::write(&bad, "old global-keychain executable").unwrap();
        assert!(!verified(&bad));
        let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let selected = resolve(&resources, &bad).unwrap();
        assert!(verified(&selected));
        stage(&resources, &dir.path().join("staging")).unwrap();
        assert!(verified(&dir.path().join(
            "staging/node_modules/dingtalk-workspace-cli/vendor/dws-windows-x64.exe"
        )));
    }
}
