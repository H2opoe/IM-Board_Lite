//! Archive extraction uses a private staging root and validates link destinations
//! before exposing any files to the installer.
use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn extract_tgz_bytes(
    bytes: &[u8],
    destination: &Path,
    strip_package_prefix: bool,
) -> Result<(), String> {
    if fs::symlink_metadata(destination).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("解压目标不能是符号链接。".to_owned());
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    if fs::read_dir(destination)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err("解压目标必须为空目录。".to_owned());
    }
    let staging = Staging(destination.join(format!(".unpack-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&staging.0).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(bytes)));
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let raw = entry.path().map_err(|e| e.to_string())?.into_owned();
        let relative = safe_archive_path(&raw, strip_package_prefix)?;
        let kind = entry.header().entry_type();
        if relative.as_os_str().is_empty() {
            if !kind.is_dir() {
                return Err("归档根条目必须为目录。".to_owned());
            }
            continue;
        }
        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "归档链接缺少目标。".to_owned())?;
            if kind.is_symlink() {
                validate_symlink_target(&relative, &target)?;
            } else {
                safe_archive_path(&target, strip_package_prefix)?;
            }
        } else if !kind.is_file() && !kind.is_dir() {
            return Err(format!("不支持的归档条目：{}", raw.display()));
        }
        // unpack_in validates canonical parent paths and hard-link targets.
        // Unlike Entry::unpack, it cannot write through an outside parent link.
        if !entry.unpack_in(&staging.0).map_err(|e| e.to_string())? {
            return Err(format!("归档路径被拒绝：{}", raw.display()));
        }
    }
    let source = if strip_package_prefix {
        staging.0.join("package")
    } else {
        staging.0.clone()
    };
    let entries = fs::read_dir(&source)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    // Preflight all final destinations before moving any entry.
    for entry in &entries {
        if fs::symlink_metadata(destination.join(entry.file_name())).is_ok() {
            return Err("归档文件与暂存目录冲突。".to_owned());
        }
    }
    for entry in entries {
        fs::rename(entry.path(), destination.join(entry.file_name())).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn extract_zip_bytes(bytes: &[u8], destination: &Path) -> Result<(), String> {
    if fs::symlink_metadata(destination).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("解压目标不能是符号链接。".to_owned());
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    if fs::read_dir(destination)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err("解压目标必须为空目录。".to_owned());
    }
    let staging = Staging(destination.join(format!(".unpack-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&staging.0).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let entry_name = file.name().map_err(|error| error.to_string())?;
        let path = safe_archive_path(Path::new(entry_name.as_ref()), false)?;
        if path.as_os_str().is_empty() {
            return Err("ZIP 条目路径为空。".to_owned());
        }
        if let Some(mode) = file.unix_mode() {
            let kind = mode & 0o170000;
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                return Err("ZIP 不支持链接或特殊文件。".to_owned());
            }
        }
        let output = staging.0.join(path);
        if file.is_dir() {
            fs::create_dir_all(output).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut target).map_err(|e| e.to_string())?;
    }
    let entries = fs::read_dir(&staging.0)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for entry in &entries {
        if fs::symlink_metadata(destination.join(entry.file_name())).is_ok() {
            return Err("归档文件与暂存目录冲突。".to_owned());
        }
    }
    for entry in entries {
        fs::rename(entry.path(), destination.join(entry.file_name())).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn safe_archive_path(path: &Path, strip_package_prefix: bool) -> Result<PathBuf, String> {
    let mut parts = path.components();
    if strip_package_prefix && !matches!(parts.next(), Some(Component::Normal(p)) if p == "package")
    {
        return Err(format!("npm 归档必须位于 package/：{}", path.display()));
    }
    let mut output = PathBuf::new();
    for part in parts {
        match part {
            Component::Normal(value) if !value.to_string_lossy().contains(['\\', ':']) => {
                output.push(value)
            }
            Component::CurDir => {}
            _ => return Err(format!("归档包含不安全路径：{}", path.display())),
        }
    }
    Ok(output)
}

fn validate_symlink_target(link: &Path, target: &Path) -> Result<(), String> {
    if target.as_os_str().is_empty() {
        return Err("归档链接目标为空。".to_owned());
    }
    let mut resolved = link.parent().unwrap_or(Path::new("")).to_path_buf();
    for part in target.components() {
        match part {
            Component::Normal(value) if !value.to_string_lossy().contains(['\\', ':']) => {
                resolved.push(value)
            }
            Component::CurDir => {}
            Component::ParentDir if resolved.pop() => {}
            _ => return Err(format!("归档链接越界：{}", target.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};

    fn archive(entries: &[(&str, Option<&str>, bool)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (name, target, hard) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o755);
            if let Some(target) = target {
                header.set_size(0);
                header.set_entry_type(if *hard {
                    tar::EntryType::Link
                } else {
                    tar::EntryType::Symlink
                });
                builder.append_link(&mut header, name, target).unwrap();
            } else {
                header.set_size(7);
                header.set_entry_type(tar::EntryType::Regular);
                header.set_cksum();
                builder
                    .append_data(&mut header, name, &b"fixture"[..])
                    .unwrap();
            }
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn rejects_external_links_and_never_writes_outside_root() {
        for (target, hard) in [
            ("../outside", false),
            ("/tmp/outside", false),
            ("../outside", true),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let dest = dir.path().join("install");
            let bytes = archive(&[
                ("package/escape", Some(target), hard),
                ("package/escape/marker", None, false),
            ]);
            assert!(extract_tgz_bytes(&bytes, &dest, true).is_err());
            assert!(!dir.path().join("outside").exists());
            assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
        }
    }

    #[test]
    fn validates_prefix_before_stripping_and_rejects_traversal() {
        for path in [
            "/package/file",
            "../package/file",
            "other/file",
            "package/../../escape",
            "package/C:escape",
            "package/a\\b",
        ] {
            assert!(safe_archive_path(Path::new(path), true).is_err(), "{path}");
        }
    }

    #[test]
    fn extracts_regular_files_and_preserves_hard_links() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("install");
        let bytes = archive(&[
            ("package/bin/cli", None, false),
            ("package/alias", Some("package/bin/cli"), true),
        ]);
        extract_tgz_bytes(&bytes, &dest, true).unwrap();
        assert_eq!(fs::read(dest.join("alias")).unwrap(), b"fixture");
    }

    #[cfg(unix)]
    #[test]
    fn internal_relative_symlink_survives_prefix_removal() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("install");
        let bytes = archive(&[
            ("package/bin/cli", None, false),
            ("package/links/cli", Some("../bin/cli"), false),
        ]);
        extract_tgz_bytes(&bytes, &dest, true).unwrap();
        assert_eq!(fs::read(dest.join("links/cli")).unwrap(), b"fixture");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_destination_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let dest = dir.path().join("install");
        std::os::unix::fs::symlink(&outside, &dest).unwrap();
        assert!(
            extract_tgz_bytes(&archive(&[("package/file", None, false)]), &dest, true).is_err()
        );
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }
}

#[cfg(test)]
mod zip_tests {
    use super::*;
    use std::io::Write;
    fn archive(names: &[&str]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in names {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"fixture").unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn zip_rejects_bad_paths_without_exposing_partial_files() {
        for bad in [
            "../escape",
            "/absolute",
            "C:/drive",
            "dir\\escape",
            "valid/../../escape",
            "./ok",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let bytes = archive(&["ok", bad]);
            assert!(extract_zip_bytes(&bytes, dir.path()).is_err(), "{bad}");
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }
    #[test]
    fn zip_extracts_regular_files_and_rejects_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = archive(&["bin/cli.exe"]);
        extract_zip_bytes(&bytes, dir.path()).unwrap();
        assert_eq!(
            fs::read(dir.path().join("bin/cli.exe")).unwrap(),
            b"fixture"
        );
        assert!(extract_zip_bytes(&bytes, dir.path()).is_err());
    }
}
