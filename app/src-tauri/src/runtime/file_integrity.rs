//! Bounded, streaming integrity checks for executable candidates.
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::Path;

pub(crate) fn verify_file(path: &Path, expected: &str, limit: u64) -> bool {
    // Reject obvious devices/directories before opening, then recheck the opened file.
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= limit) {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= limit)
    {
        return false;
    }
    matches!(digest_reader(&mut file, limit), Ok(Some(digest)) if digest == expected)
}

fn digest_reader(reader: &mut impl Read, limit: u64) -> io::Result<Option<String>> {
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        // Consume at most one byte beyond the budget, even if the file grows after stat.
        let length = limit
            .saturating_sub(total)
            .saturating_add(1)
            .min(buffer.len() as u64) as usize;
        let count = match reader.read(&mut buffer[..length]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(Some(format!("{:x}", hash.finalize())));
        }
        total += count as u64;
        if total > limit {
            return Ok(None);
        }
        hash.update(&buffer[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regular_file_is_verified_and_changed_content_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("candidate");
        std::fs::write(&path, b"fixture").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"fixture"));
        assert!(verify_file(&path, &expected, 7));
        assert!(!verify_file(&path, &expected, 6));
        std::fs::write(&path, b"changed").unwrap();
        assert!(!verify_file(&path, &expected, 7));
        assert!(!verify_file(dir.path(), &expected, 7));
        assert!(!verify_file(&dir.path().join("missing"), &expected, 7));
    }
    #[test]
    fn sparse_oversized_candidate_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        assert!(!verify_file(&path, "unused", 64 * 1024 * 1024));
    }
    #[test]
    fn a_growing_stream_stops_at_limit_plus_one() {
        struct Growing(usize);
        impl Read for Growing {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                output.fill(0);
                self.0 += output.len();
                Ok(output.len())
            }
        }
        let mut reader = Growing(0);
        assert_eq!(digest_reader(&mut reader, 17).unwrap(), None);
        assert_eq!(reader.0, 18);
    }
    #[test]
    fn read_errors_fail_closed_and_interrupted_reads_retry() {
        struct Interrupted(bool);
        impl Read for Interrupted {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                if !self.0 {
                    self.0 = true;
                    Err(io::ErrorKind::Interrupted.into())
                } else {
                    Ok(0)
                }
            }
        }
        assert_eq!(
            digest_reader(&mut Interrupted(false), 0).unwrap(),
            Some(format!("{:x}", Sha256::digest(b"")))
        );
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }
        assert!(digest_reader(&mut Broken, 17).is_err());
    }
}
