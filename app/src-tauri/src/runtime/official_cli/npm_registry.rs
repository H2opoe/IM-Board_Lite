use std::collections::BTreeMap;

use super::versioning::{compare_versions, parse_version};
use super::{emit_platform_cli_progress, PlatformCliProgressContext};

const NPM_REGISTRIES: &[&str] = &[
    "https://registry.npmmirror.com",
    "https://registry.npmjs.org",
];
#[derive(Debug, serde::Deserialize)]
pub(super) struct NpmPackageMetadata {
    #[serde(rename = "dist-tags")]
    pub(super) dist_tags: BTreeMap<String, String>,
    pub(super) versions: BTreeMap<String, NpmVersionMetadata>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct NpmVersionMetadata {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) dist: NpmDistMetadata,
    #[serde(default)]
    pub(super) dependencies: BTreeMap<String, String>,
    #[serde(rename = "optionalDependencies", default)]
    pub(super) optional_dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct NpmDistMetadata {
    pub(super) tarball: String,
}

pub async fn npm_latest_version(
    package: &str,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<String, String> {
    let metadata = npm_metadata(package, progress).await?;
    metadata
        .dist_tags
        .get("latest")
        .cloned()
        .or_else(|| metadata.versions.keys().next_back().cloned())
        .ok_or_else(|| "官方源未返回版本号。".to_owned())
}

pub(super) async fn npm_metadata(
    package: &str,
    progress: Option<&PlatformCliProgressContext<'_>>,
) -> Result<NpmPackageMetadata, String> {
    let client = reqwest::Client::new();
    let mut last_error = String::new();
    for (index, registry) in NPM_REGISTRIES.iter().enumerate() {
        if let Some(progress) = progress {
            emit_platform_cli_progress(
                progress,
                "checking_remote",
                format!("正在从镜像源核查 {} 最新版本…", package),
                2,
                5,
                Some(registry),
                Some(package),
                None,
            );
        }
        let url = format!(
            "{}/{}",
            registry.trim_end_matches('/'),
            npm_package_url(package)
        );
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                match response.json::<NpmPackageMetadata>().await {
                    Ok(metadata) => {
                        if let Some(version) = metadata.dist_tags.get("latest") {
                            if let Some(progress) = progress {
                                emit_platform_cli_progress(
                                    progress,
                                    "version_resolved",
                                    format!("已确认 {} 最新版本 v{}。", package, version),
                                    2,
                                    5,
                                    Some(registry),
                                    Some(package),
                                    Some(version),
                                );
                            }
                        }
                        return Ok(metadata);
                    }
                    Err(err) => {
                        last_error = format!("{registry}：解析版本信息失败：{err}");
                    }
                }
            }
            Ok(response) => {
                last_error = format!("{registry}：HTTP {}", response.status());
            }
            Err(err) => {
                last_error = format!("{registry}：{err}");
            }
        }
        if let Some(progress) = progress {
            let has_fallback = index + 1 < NPM_REGISTRIES.len();
            emit_platform_cli_progress(
                progress,
                "checking_remote_retry",
                if has_fallback {
                    format!("镜像源暂不可用，正在切换备用源核查 {}。", package)
                } else {
                    format!("{} 版本核查失败。", package)
                },
                2,
                5,
                Some(registry),
                Some(package),
                None,
            );
        }
    }
    Err(format!("核查官方CLI最新版本失败：{last_error}"))
}

fn npm_package_url(package: &str) -> String {
    package.replace('/', "%2F")
}

pub(super) fn resolve_npm_version(
    metadata: &NpmPackageMetadata,
    requirement: &str,
) -> Option<String> {
    if requirement == "latest" || requirement == "*" {
        return metadata.dist_tags.get("latest").cloned();
    }
    if metadata.versions.contains_key(requirement) {
        return Some(requirement.to_owned());
    }
    metadata
        .versions
        .keys()
        .filter(|version| version_satisfies(version, requirement))
        .max_by(|left, right| compare_versions(left, right))
        .cloned()
}

fn version_satisfies(version: &str, requirement: &str) -> bool {
    let requirement = requirement.trim();
    if requirement == "*" || requirement.is_empty() {
        return true;
    }
    let version_parts = parse_version(version);
    let base = parse_version(requirement.trim_start_matches(['^', '~', '=']));
    if base.is_empty() {
        return false;
    }
    if requirement.starts_with('^') {
        return version_parts >= base && version_parts.first() == base.first();
    }
    if requirement.starts_with('~') {
        return version_parts >= base
            && version_parts.first() == base.first()
            && version_parts.get(1) == base.get(1);
    }
    version == requirement
}
