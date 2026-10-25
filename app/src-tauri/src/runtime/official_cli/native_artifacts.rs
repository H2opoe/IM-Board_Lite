//! Audited native artifacts only. This is deliberately not an npm dependency resolver.
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    packages: Vec<Package>,
    feishu_assets: Vec<Asset>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Package {
    pub name: String,
    pub version: String,
    pub url: String,
    pub integrity: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Asset {
    pub name: String,
    pub url: String,
    pub sha256: String,
}
fn manifest() -> Result<Manifest, String> {
    let value: Manifest = serde_json::from_str(include_str!("native-artifacts.json"))
        .map_err(|e| format!("官方CLI固定资源清单无效：{e}"))?;
    if value.schema_version != 1 {
        return Err("官方CLI资源清单版本不支持。".into());
    }
    Ok(value)
}

pub(super) fn packages(package: &str, version: &str, target: &str) -> Result<Vec<Package>, String> {
    if !["darwin-arm64", "darwin-x64", "win32-x64"].contains(&target) {
        return Err(format!("官方CLI不支持目标平台：{target}"));
    }
    let spec = super::OFFICIAL_CLIS
        .iter()
        .find(|s| s.package == package && s.compatible_version == version)
        .ok_or_else(|| format!("未经验证的官方CLI版本：{package}@{version}"))?;
    let manifest = manifest()?;
    let mut names = vec![spec.package.to_owned()];
    if spec.platform == "wecom" {
        names.push(format!("@wecom/cli-{target}"));
    }
    names
        .into_iter()
        .map(|name| {
            manifest
                .packages
                .iter()
                .find(|p| p.name == name && p.version == version)
                .cloned()
                .ok_or_else(|| format!("官方CLI资源清单缺少{name}@{version}"))
        })
        .collect()
}

pub(super) fn feishu_asset(name: &str) -> Result<Asset, String> {
    manifest()?
        .feishu_assets
        .into_iter()
        .find(|a| a.name == name)
        .ok_or_else(|| format!("未经验证的飞书CLI资源：{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_supported_version_has_only_its_required_native_artifacts() {
        for target in ["darwin-arm64", "darwin-x64", "win32-x64"] {
            for spec in crate::runtime::official_cli::OFFICIAL_CLIS {
                if spec.platform == "dingtalk" {
                    assert_eq!(
                        spec.compatible_version,
                        crate::runtime::isolated_dws::VERSION
                    );
                    assert!(packages(spec.package, spec.compatible_version, target).is_err());
                    continue; // Windows DWS is a pinned bundled adaptation, never an npm payload.
                }
                let plan = packages(spec.package, spec.compatible_version, target).unwrap();
                assert_eq!(plan[0].name, spec.package);
                assert_eq!(plan.len(), if spec.platform == "wecom" { 2 } else { 1 });
                if spec.platform == "wecom" {
                    assert_eq!(plan[1].name, format!("@wecom/cli-{target}"));
                }
                for item in plan {
                    assert!(item.url.starts_with("https://registry.npmjs.org/"));
                    assert!(item.integrity.starts_with("sha512-"));
                }
            }
        }
        for range in ["latest", "*", "^0.1.9", "~0.1.9", "0.1.10", "0.1.9-beta.1"] {
            assert!(packages("@wecom/cli", range, "darwin-arm64").is_err());
        }
        assert!(packages("../../escape", "0.1.9", "darwin-arm64").is_err());
        assert!(packages("@wecom/cli", "0.1.9", "win32-arm64").is_err());
    }
    #[test]
    fn feishu_downloads_require_an_explicit_digest() {
        for target in [
            "darwin-arm64.tar.gz",
            "darwin-amd64.tar.gz",
            "windows-amd64.zip",
        ] {
            let item = feishu_asset(&format!("lark-cli-1.0.98-{target}")).unwrap();
            assert_eq!(item.sha256.len(), 64);
            assert!(item.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(feishu_asset("lark-cli-latest-darwin-arm64.tar.gz").is_err());
    }
}
