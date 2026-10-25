//! Stable Unix ownership; no public destructive bare-PGID entry points.
pub(crate) use macos_owned_process::{check_supported, ProcessOwner};

#[cfg(test)]
pub(crate) fn exists(pid: u32) -> std::io::Result<bool> {
    let target = libc::pid_t::try_from(pid)
        .ok()
        .filter(|pid| *pid > 1)
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    if unsafe { libc::kill(-target, 0) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else {
        Err(error)
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "process_group_owner_tests.rs"]
mod tests;
