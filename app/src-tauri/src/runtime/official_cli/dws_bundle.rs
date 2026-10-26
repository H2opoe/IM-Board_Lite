//! Security-patched macOS DWS payloads stay archived while the outer app is signed.
use sha2::{Digest, Sha256};
use std::path::Path;
#[cfg(any(debug_assertions, test))]
use std::path::PathBuf;

pub const VERSION: &str = "1.0.65-imboard.1";
fn target() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "darwin-arm64"
    } else {
        "darwin-x64"
    }
}
fn digests() -> (&'static str, &'static str) {
    match target() {
        "darwin-arm64" => (
            "b1548d349c2bae67e0f250de9d8ce540327a41a52bb2eee77b8390edd526a069",
            "b5f380564ca7090cea9518ffa0a985ed43bc21e1842053d9cabe15afef5b054c",
        ),
        _ => (
            "8fcb49324dce16ba3c4c4eb56086a9f8fa2cc1275955064c32ad1788da78a39c",
            "21b9ff57a750865cda94e6677bd83ab4e17e80378ee803a91f3142952ae91e7f",
        ),
    }
}
pub fn verified(path: &Path) -> bool {
    crate::runtime::file_integrity::verify_file(path, digests().1, 64 * 1024 * 1024)
}
fn archive(resource_dir: &Path) -> Result<Vec<u8>, String> {
    let suffix = format!("runtime/dws/{VERSION}/{}/package.tgz", target());
    let candidates = {
        let standard = vec![
            resource_dir.join(&suffix),
            resource_dir.join("_up_").join(&suffix),
            resource_dir.join("..").join(&suffix),
        ];
        #[cfg(debug_assertions)]
        {
            let mut with_debug_source = standard;
            with_debug_source.push(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join(&suffix),
            );
            with_debug_source
        }
        #[cfg(not(debug_assertions))]
        standard
    };
    let path = candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| "内置钉钉运行时缺失，请重新安装应用。".to_owned())?;
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| "无法读取内置钉钉运行时。".to_owned())?;
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取内置钉钉运行时失败。".to_owned())?;
    if bytes.len() > 32 * 1024 * 1024 || format!("{:x}", Sha256::digest(&bytes)) != digests().0 {
        return Err("内置钉钉运行时校验失败，已拒绝安装。".into());
    }
    Ok(bytes)
}
pub(super) fn stage(resource_dir: &Path, staging: &Path) -> Result<(), String> {
    let bytes = archive(resource_dir)?;
    let package = staging.join("node_modules/dingtalk-workspace-cli");
    std::fs::create_dir_all(&package).map_err(|e| e.to_string())?;
    super::extract_tgz_bytes(&bytes, &package, true)?;
    let metadata: serde_json::Value = serde_json::from_slice(
        &std::fs::read(package.join("package.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if metadata["name"] != "dingtalk-workspace-cli" || metadata["version"] != VERSION {
        return Err("内置钉钉运行时元数据不匹配。".into());
    }
    let binary = package.join(format!("vendor/dws-{}", target()));
    if !verified(&binary) {
        return Err("钉钉运行时解包后校验失败。".into());
    }
    super::make_executable(&binary)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn bundled_payload_installs_and_runs_version_without_account_access() {
        let dir = tempfile::tempdir().unwrap();
        let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        stage(&resources, dir.path()).unwrap();
        let binary = dir.path().join(format!(
            "node_modules/dingtalk-workspace-cli/vendor/dws-{}",
            target()
        ));
        assert!(verified(&binary));
        super::super::installer::check_binary(&binary, dir.path())
            .await
            .unwrap();
        std::fs::write(&binary, b"tampered or old official binary").unwrap();
        assert!(!verified(&binary));
    }
    #[test]
    fn tampered_resource_is_rejected_without_falling_back_to_debug_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(format!("runtime/dws/{VERSION}/{}/package.tgz", target()));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"tampered").unwrap();
        let staging = dir.path().join("stage");
        assert!(stage(dir.path(), &staging).is_err());
        assert!(!staging.exists());
    }
}
