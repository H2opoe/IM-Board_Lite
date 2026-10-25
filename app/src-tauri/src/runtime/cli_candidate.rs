use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Validate the local entry point before resolution. Runtime integrity is checked separately.
pub(crate) fn is_dependency_free_cli(path: &Path, require_exe: bool) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "js" | "cmd" | "bat") || (require_exe && extension != "exe") {
        return false;
    }
    // Check before opening so a directory or device is never treated as an entry point.
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !require_exe && metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    let Ok(file) = File::open(path) else {
        return false;
    };
    let Ok(opened_metadata) = file.metadata() else {
        return false;
    };
    if !opened_metadata.is_file() || opened_metadata.len() == 0 {
        return false;
    }
    dependency_free_header(file)
}

fn dependency_free_header(reader: impl Read) -> bool {
    let mut bytes = Vec::with_capacity(96);
    if reader.take(96).read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return false;
    }
    let head = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    !head.contains("/usr/bin/env node") && !head.contains("node ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn rejects_directories_empty_and_missing_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("directory.exe");
        std::fs::create_dir(&directory).unwrap();
        let empty = temp.path().join("empty.exe");
        binary(&empty, b"");
        for path in [directory, empty, temp.path().join("missing.exe")] {
            assert!(!is_dependency_free_cli(&path, false));
            assert!(!is_dependency_free_cli(&path, true));
        }
    }

    #[test]
    fn preserves_platform_entry_point_rules_and_rejects_node_wrappers() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("cli.EXE");
        binary(&executable, b"MZ\0synthetic native fixture");
        assert!(is_dependency_free_cli(&executable, true));
        let mac = temp.path().join("cli");
        binary(&mac, b"\xcf\xfa\xed\xfe synthetic native fixture");
        assert!(is_dependency_free_cli(&mac, false));
        assert!(!is_dependency_free_cli(&mac, true));
        for name in ["cli.js", "cli.cmd", "cli.bat", "node.exe"] {
            let path = temp.path().join(name);
            binary(&path, b"#!/usr/bin/env node\n");
            assert!(!is_dependency_free_cli(&path, false));
            assert!(!is_dependency_free_cli(&path, true));
        }
    }

    #[test]
    fn header_reads_are_bounded_and_errors_reject_the_candidate() {
        struct EndlessReader;
        impl Read for EndlessReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                assert!(buf.len() <= 96);
                buf.fill(b'M');
                Ok(buf.len())
            }
        }
        struct BrokenReader;
        impl Read for BrokenReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            }
        }
        assert!(dependency_free_header(EndlessReader));
        assert!(!dependency_free_header(BrokenReader));
    }

    #[cfg(unix)]
    #[test]
    fn unix_candidates_require_execute_permission_and_valid_symlink_targets() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cli");
        binary(&path, b"synthetic native fixture");
        let link = temp.path().join("cli-link");
        symlink(&path, &link).unwrap();
        assert!(is_dependency_free_cli(&link, false));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_dependency_free_cli(&path, false));
        assert!(!is_dependency_free_cli(&link, false));
        std::fs::remove_file(&path).unwrap();
        assert!(!is_dependency_free_cli(&link, false));
    }
}
