//! The direct Child remains waitable until this owner disarms every group signal.
use std::io;
use std::process::{Child, ExitStatus};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Debug)]
enum OwnedChild {
    Async(tokio::process::Child),
    Blocking(Child),
}
impl OwnedChild {
    fn id(&self) -> u32 {
        match self {
            Self::Async(c) => c.id().expect("unreaped child"),
            Self::Blocking(c) => c.id(),
        }
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match self {
            Self::Async(c) => c.try_wait(),
            Self::Blocking(c) => c.try_wait(),
        }
    }
}
#[derive(Debug)]
struct State {
    child: Option<OwnedChild>,
    observed: Option<ExitStatus>,
    finished: Option<ExitStatus>,
    identity_lost: bool,
    signals_disarmed: bool,
}
#[derive(Debug)]
pub struct ProcessOwner {
    pid: u32,
    state: Mutex<State>,
    cleanup: Mutex<()>,
}

pub fn check_supported() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn imboard_group_api_available() -> i32;
        }
        let capability_error = unsafe { imboard_group_api_available() };
        if capability_error != 0 {
            return Err(io::Error::from_raw_os_error(capability_error));
        }
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if action.sa_sigaction == libc::SIG_IGN || action.sa_flags & libc::SA_NOCLDWAIT != 0 {
            return Err(io::Error::other(
                "SIGCHLD 自动回收策略不支持安全进程组所有权。",
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "此 Unix 平台尚未实现稳定进程组所有权。",
    ))
}
impl ProcessOwner {
    fn new(child: OwnedChild) -> Arc<Self> {
        Arc::new(Self {
            pid: child.id(),
            state: Mutex::new(State {
                child: Some(child),
                observed: None,
                finished: None,
                identity_lost: false,
                signals_disarmed: false,
            }),
            cleanup: Mutex::new(()),
        })
    }
    pub fn from_async(child: tokio::process::Child) -> Arc<Self> {
        Self::new(OwnedChild::Async(child))
    }
    pub fn from_blocking(child: Child) -> Arc<Self> {
        Self::new(OwnedChild::Blocking(child))
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    fn observe_locked(&self, state: &mut State) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = state.finished {
            return Ok(Some(status));
        }
        if state.identity_lost {
            return Err(identity_error());
        }
        check_supported()?;
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        loop {
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid,
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
                )
            };
            if result == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.raw_os_error() == Some(libc::ECHILD) {
                state.identity_lost = true;
                state.signals_disarmed = true;
            }
            return Err(error);
        }
        if unsafe { info.si_pid() } != 0 && state.observed.is_none() {
            use std::os::unix::process::ExitStatusExt;
            let status = unsafe { info.si_status() };
            let raw = match info.si_code {
                libc::CLD_EXITED => ((status & 0xff) << 8) | (status & 0xff00_0000u32 as i32),
                libc::CLD_KILLED => status,
                libc::CLD_DUMPED => status | 0x80,
                _ => return Err(io::Error::other("无法识别进程退出状态。")),
            };
            state.observed = Some(ExitStatus::from_raw(raw));
        }
        Ok(state.observed)
    }
    pub fn observe_exit(&self) -> io::Result<Option<ExitStatus>> {
        self.observe_locked(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub async fn wait_async(&self) -> io::Result<ExitStatus> {
        loop {
            if let Some(status) = self.observe_exit()? {
                return Ok(status);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    pub fn wait_blocking(&self) -> io::Result<ExitStatus> {
        loop {
            if let Some(status) = self.observe_exit()? {
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // The only destructive numeric syscall. Internal consumers never reap Child.
    fn signal(&self, signal: i32) -> io::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finished.is_some() {
            return Ok(());
        }
        if state.identity_lost || state.signals_disarmed {
            return Err(identity_error());
        }
        self.observe_locked(&mut state)?;
        let pid = libc::pid_t::try_from(self.pid)
            .ok()
            .filter(|pid| *pid > 1)
            .ok_or_else(identity_error)?;
        // Producers establish process_group(0) before exec. getpgid(zombie)
        // can return ESRCH on macOS even while WNOWAIT still pins its PID.
        // The held Child, rather than that lookup, prevents numeric reuse.
        if unsafe { libc::kill(-pid, signal) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    pub fn request_kill(&self) -> io::Result<()> {
        if self.confirm_exit()? {
            Ok(())
        } else {
            self.signal(libc::SIGKILL)
        }
    }
    /// No running process is modified. Only this owner's exited Child is reaped.
    pub fn confirm_exit(&self) -> io::Result<bool> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finished.is_some() {
            return Ok(true);
        }
        if self.observe_locked(&mut state)?.is_none() {
            return Ok(false);
        }
        if !only_owned_leader(self.pid)? {
            return Ok(false);
        }
        self.observe_locked(&mut state)?;
        state.signals_disarmed = true;
        match state
            .child
            .as_mut()
            .expect("owner retains child")
            .try_wait()
        {
            Ok(Some(status)) => {
                // Darwin clears extended exit bits on the first WNOWAIT read.
                // Keep the first observation while Child records final reap.
                state.finished = Some(state.observed.unwrap_or(status));
                state.child.take();
                Ok(true)
            }
            Ok(None) => Err(io::Error::other("进程退出观察与回收状态不一致。")),
            Err(error) => {
                if error.raw_os_error() == Some(libc::ECHILD) {
                    state.identity_lost = true;
                }
                Err(error)
            }
        }
    }
    pub fn terminate(&self) -> io::Result<()> {
        let _cleanup = self.cleanup.lock().unwrap_or_else(|e| e.into_inner());
        if self.confirm_exit()? {
            return Ok(());
        }
        for signal in [libc::SIGTERM, libc::SIGKILL] {
            if let Err(error) = self.signal(signal) {
                if error.raw_os_error() != Some(libc::EPERM) {
                    return Err(error);
                }
            }
            for _ in 0..20 {
                if self.confirm_exit()? {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "进程组未能在终止请求后退出。",
        ))
    }
    pub fn cleanup_in_background(owner: Arc<Self>) {
        if owner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .child
            .is_none()
        {
            return;
        }
        let retained = owner.clone();
        if std::thread::Builder::new()
            .name("unix-owner-cleanup".into())
            .spawn(move || {
                if owner.terminate().is_err() {
                    retain(owner);
                }
            })
            .is_err()
        {
            retain(retained);
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn unconfirmed_fixture() -> Arc<Self> {
        Arc::new(Self {
            pid: 0,
            state: Mutex::new(State {
                child: None,
                observed: None,
                finished: None,
                identity_lost: true,
                signals_disarmed: true,
            }),
            cleanup: Mutex::new(()),
        })
    }
}
impl Drop for ProcessOwner {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(|e| e.into_inner());
        if state.finished.is_none() {
            // Never let Tokio Drop/kill_on_drop/orphan reapers use an invalid ID.
            if let Some(child) = state.child.take() {
                std::mem::forget(child);
            }
        }
    }
}
fn retain(owner: Arc<ProcessOwner>) {
    static UNCONFIRMED: OnceLock<Mutex<Vec<Arc<ProcessOwner>>>> = OnceLock::new();
    let mut owners = UNCONFIRMED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    owners.retain(|old| !matches!(old.confirm_exit(), Ok(true)));
    if !owners.iter().any(|old| Arc::ptr_eq(old, &owner)) {
        owners.push(owner);
    }
}
fn identity_error() -> io::Error {
    io::Error::other("进程组所有权已失效，拒绝按旧编号清理。")
}
#[cfg(target_os = "macos")]
fn only_owned_leader(pid: u32) -> io::Result<bool> {
    unsafe extern "C" {
        fn imboard_group_only_leader(pid: i32, only_leader: *mut i32) -> i32;
    }
    let mut only = 0;
    let error = unsafe { imboard_group_only_leader(pid as i32, &mut only) };
    if error != 0 {
        Err(io::Error::from_raw_os_error(error))
    } else {
        Ok(only != 0)
    }
}
#[cfg(not(target_os = "macos"))]
fn only_owned_leader(_pid: u32) -> io::Result<bool> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(all(test, target_os = "macos"))]
#[path = "process_group_owner_tests.rs"]
mod tests;
