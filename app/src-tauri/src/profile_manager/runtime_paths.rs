use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::storage::models::ImProfile;

pub fn validate_profile_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 160
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("账号标识无效。".into());
    }
    // Keep identifiers valid as a single directory component on both supported OSes.
    let upper = id.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("账号标识是系统保留名称。".into());
    }
    Ok(())
}

pub fn cleanup_plan<'a>(
    profile: &ImProfile,
    other_profiles: impl IntoIterator<Item = &'a ImProfile>,
    app_dir: &Path,
    cache_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    validate_profile_id(&profile.id)?;
    let roots = [app_dir.join("Profiles"), cache_dir.to_path_buf()];
    filter_unreferenced_paths(
        candidates(profile, &roots),
        other_profiles,
        app_dir,
        cache_dir,
    )
}

pub(super) fn filter_unreferenced_paths<'a>(
    paths: BTreeSet<PathBuf>,
    other_profiles: impl IntoIterator<Item = &'a ImProfile>,
    app_dir: &Path,
    cache_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let roots = [app_dir.join("Profiles"), cache_dir.to_path_buf()];
    for root in &roots {
        match fs::symlink_metadata(root) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err("账号缓存根目录无效，已停止清理。".into())
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string())
            }
            _ => {}
        }
    }
    let mut protected = Vec::new();
    for other in other_profiles {
        for path in references(other, &roots) {
            protected.push(resolved_reference(&path)?);
            protected.push(path);
        }
    }
    let mut unreferenced = Vec::new();
    for path in paths {
        let resolved = resolved_reference(&path)?;
        if !protected
            .iter()
            .any(|reference| paths_overlap(&path, reference) || paths_overlap(&resolved, reference))
        {
            unreferenced.push(path);
        }
    }
    Ok(unreferenced)
}

pub fn remove_owned_paths(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        match fs::symlink_metadata(path) {
            // Delete only the link itself, never the linked account/external directory.
            Ok(meta) if meta.file_type().is_symlink() => {
                fs::remove_file(path).map_err(|error| format!("删除账号缓存链接失败：{error}"))?;
            }
            Ok(meta) if meta.is_dir() => {
                fs::remove_dir_all(path).map_err(|error| format!("删除账号缓存失败：{error}"))?;
            }
            Ok(_) => return Err("账号缓存路径不是目录，已停止清理。".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

// Destructive targets stay restricted to validated direct children. References may
// be external aliases or point at an ancestor, so they require broader protection.
fn candidates(profile: &ImProfile, roots: &[PathBuf; 2]) -> BTreeSet<PathBuf> {
    references(profile, roots)
        .iter()
        .flat_map(|path| {
            roots
                .iter()
                .filter_map(|root| managed_profile_root(root, path))
        })
        .collect()
}

fn references(profile: &ImProfile, roots: &[PathBuf; 2]) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if validate_profile_id(&profile.id).is_ok() {
        result.extend(roots.iter().map(|root| root.join(&profile.id)));
    }
    for key in [
        "profileDir",
        "configDir",
        "configPath",
        "keysPath",
        "cacheDir",
        "tmpDir",
        "larkConfigDir",
        "dwsCacheDir",
        "dwsKeychainDir",
    ] {
        let Some(text) = profile
            .config_json
            .get(key)
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let path = if let Some(tail) = text.strip_prefix("~/") {
            dirs::home_dir()
                .map(|home| home.join(tail))
                .unwrap_or_else(|| PathBuf::from(text))
        } else {
            PathBuf::from(text)
        };
        if !text.is_empty() {
            result.push(path);
        }
    }
    result
}

// Resolve the existing prefix, preserving missing children used by unfinished
// bindings. Permission errors and alias loops stop deletion rather than dropping
// a reference. Canonical paths are used only for protection, never as targets.
fn resolved_reference(path: &Path) -> Result<PathBuf, String> {
    let mut prefix = path.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        match fs::canonicalize(&prefix) {
            Ok(mut resolved) => {
                for part in suffix.iter().rev() {
                    resolved.push(part);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(part) = prefix.components().next_back() else {
                    return Ok(path.to_path_buf());
                };
                suffix.push(part.as_os_str().to_os_string());
                if !prefix.pop() {
                    return Ok(path.to_path_buf());
                }
            }
            Err(error) => return Err(format!("无法确认账号目录引用，已停止清理：{error}")),
        }
    }
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    // Conservative on case-sensitive volumes too: retaining a directory is safer
    // than deleting data referenced with a different spelling on another volume.
    left.components().zip(right.components()).all(|(a, b)| {
        match (a.as_os_str().to_str(), b.as_os_str().to_str()) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => a == b,
        }
    })
}

fn managed_profile_root(root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(root).ok()?;
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let Component::Normal(first) = relative.components().next()? else {
        return None;
    };
    validate_profile_id(first.to_str()?).ok()?;
    Some(root.join(first))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(id: &str, config_json: serde_json::Value) -> ImProfile {
        ImProfile {
            id: id.into(),
            platform: "wecom".into(),
            label: "fixture".into(),
            enabled: true,
            config_json,
            status: "normal".into(),
            sort_order: 0,
            created_at: "fixture".into(),
            updated_at: "fixture".into(),
        }
    }
    #[test]
    fn identifiers_cannot_escape_or_alias_windows_device_paths() {
        for id in [
            "",
            ".",
            "..",
            "../outside",
            "a/b",
            "a\\b",
            "/tmp/x",
            "C:temp",
            "CON",
            "lpt1",
            "a.",
            " x",
            "x\0y",
        ] {
            assert!(validate_profile_id(id).is_err(), "{id:?}");
        }
        for id in ["wecom_123", "feishu-uuid", "p", "COM10"] {
            assert!(validate_profile_id(id).is_ok());
        }
    }
    #[test]
    fn parent_identifier_fails_before_any_file_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        fs::create_dir_all(app.join("Profiles")).unwrap();
        fs::create_dir_all(app.join("outside")).unwrap();
        fs::write(app.join("outside/keep"), "synthetic").unwrap();
        assert!(cleanup_plan(
            &profile("../outside", serde_json::json!({})),
            [],
            &app,
            &dir.path().join("cache")
        )
        .is_err());
        assert!(app.join("outside/keep").exists());
    }
    #[test]
    fn active_profile_and_draft_roots_are_preserved_even_when_selected_in_deleted_config() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        for id in ["one", "other", "draft"] {
            fs::create_dir_all(app.join("Profiles").join(id)).unwrap();
            fs::write(app.join("Profiles").join(id).join("keep"), "fixture").unwrap();
        }
        let deleted = profile(
            "one",
            serde_json::json!({"configDir": app.join("Profiles/other/config"), "tmpDir": app.join("Profiles/draft/tmp")}),
        );
        let others = [
            profile("other", serde_json::json!({})),
            profile("draft", serde_json::json!({})),
        ];
        let paths = cleanup_plan(&deleted, &others, &app, &cache).unwrap();
        remove_owned_paths(&paths).unwrap();
        assert!(!app.join("Profiles/one").exists());
        assert!(app.join("Profiles/other/keep").exists());
        assert!(app.join("Profiles/draft/keep").exists());
    }
    #[test]
    fn adopted_directory_is_deleted_only_after_its_last_reference_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let config = serde_json::json!({"configDir": app.join("Profiles/adopted/config")});
        let deleted = profile("one", config.clone());
        let other = profile("two", config);
        assert!(!cleanup_plan(&deleted, [&other], &app, &cache)
            .unwrap()
            .contains(&app.join("Profiles/adopted")));
        assert!(cleanup_plan(&deleted, [], &app, &cache)
            .unwrap()
            .contains(&app.join("Profiles/adopted")));
    }
    #[test]
    fn case_variant_reference_preserves_owned_directory() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let owned = app.join("Profiles/Shared");
        fs::create_dir_all(&owned).unwrap();
        let deleted = profile("Shared", serde_json::json!({}));
        let other = profile(
            "other",
            serde_json::json!({
                "configDir": app.join("profiles/shared/not-created/config")
            }),
        );
        assert!(!cleanup_plan(&deleted, [&other], &app, &cache)
            .unwrap()
            .contains(&owned));
    }

    #[cfg(unix)]
    #[test]
    fn external_symlink_reference_preserves_owned_directory() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let owned = app.join("Profiles/shared");
        fs::create_dir_all(&owned).unwrap();
        let alias = dir.path().join("alias");
        symlink(&owned, &alias).unwrap();
        let deleted = profile("shared", serde_json::json!({}));
        let other = profile(
            "other",
            serde_json::json!({
                "configDir": alias.join("not-created/config")
            }),
        );
        assert!(!cleanup_plan(&deleted, [&other], &app, &cache)
            .unwrap()
            .contains(&owned));
    }

    #[test]
    fn ancestor_references_protect_children_but_sibling_prefixes_do_not() {
        assert!(paths_overlap(
            Path::new("/fixture/Profiles"),
            Path::new("/fixture/Profiles/one")
        ));
        assert!(!paths_overlap(
            Path::new("/fixture/Profiles/one"),
            Path::new("/fixture/Profiles/one-other")
        ));
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let owned = app.join("Profiles/one");
        fs::create_dir_all(&owned).unwrap();
        fs::write(owned.join("keep"), "fixture").unwrap();
        let deleted = profile("one", serde_json::json!({}));
        let other = profile(
            "other",
            serde_json::json!({"configDir": app.join("Profiles")}),
        );
        remove_owned_paths(&cleanup_plan(&deleted, [&other], &app, &cache).unwrap()).unwrap();
        assert!(owned.join("keep").exists());
        remove_owned_paths(&cleanup_plan(&deleted, [], &app, &cache).unwrap()).unwrap();
        assert!(!owned.exists());
    }

    #[test]
    fn default_ids_with_different_case_are_protected() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let deleted = profile("Shared", serde_json::json!({}));
        let other = profile("shared", serde_json::json!({}));
        assert!(cleanup_plan(&deleted, [&other], &app, &cache)
            .unwrap()
            .is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unresolvable_alias_stops_cleanup() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let alias = dir.path().join("loop");
        symlink(&alias, &alias).unwrap();
        let deleted = profile("one", serde_json::json!({}));
        let other = profile("other", serde_json::json!({"configDir": alias}));
        assert!(cleanup_plan(
            &deleted,
            [&other],
            &dir.path().join("app"),
            &dir.path().join("cache")
        )
        .is_err());
    }

    #[test]
    fn external_and_parent_paths_never_become_cleanup_targets() {
        let root = Path::new("/fixture/Profiles");
        for value in [
            "/fixture/Profiles/../outside",
            "/fixture/Profiles/one/../../other",
            "/elsewhere/one",
            "/fixture/Profiles",
        ] {
            assert!(managed_profile_root(root, Path::new(value)).is_none());
        }
    }
    #[cfg(unix)]
    #[test]
    fn account_symlink_is_unlinked_and_managed_root_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let cache = dir.path().join("cache");
        let external = dir.path().join("external");
        fs::create_dir_all(app.join("Profiles")).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("keep"), "fixture").unwrap();
        symlink(&external, app.join("Profiles/one")).unwrap();
        let p = profile("one", serde_json::json!({}));
        remove_owned_paths(&cleanup_plan(&p, [], &app, &cache).unwrap()).unwrap();
        assert!(external.join("keep").exists());
        fs::remove_dir(app.join("Profiles")).unwrap();
        symlink(&external, app.join("Profiles")).unwrap();
        assert!(cleanup_plan(&p, [], &app, &cache).is_err());
        assert!(external.join("keep").exists());
    }
}
