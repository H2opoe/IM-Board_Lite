//! Owned Windows process tree. Never reopen a process by its numeric PID.
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    GetProcessIdOfThread, OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

pub(crate) struct ProcessJob(OwnedHandle);

fn owned(handle: HANDLE) -> OwnedHandle {
    // Only called for successful APIs returning a newly owned, valid handle.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

impl ProcessJob {
    fn handle(&self) -> HANDLE {
        HANDLE(self.0.as_raw_handle())
    }

    pub(crate) fn new() -> io::Result<Self> {
        let job = Self(owned(unsafe { CreateJobObjectW(None, None) }?));
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job.handle(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            )?;
        }
        Ok(job)
    }

    pub(crate) fn spawn(mut command: Command) -> io::Result<(Child, Self)> {
        let job = Self::new()?;
        // Assignment must happen before the runtime can create descendants.
        command.creation_flags((CREATE_SUSPENDED | CREATE_NO_WINDOW).0);
        let mut child = command.spawn()?;
        let assigned =
            unsafe { AssignProcessToJobObject(job.handle(), HANDLE(child.as_raw_handle())) }
                .map_err(io::Error::other);
        if let Err(error) = assigned.and_then(|()| resume_child(child.id())) {
            // The Child handle is still owned, including assignment failures.
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok((child, job))
    }

    pub(crate) fn spawn_async(
        &self,
        mut command: tokio::process::Command,
    ) -> io::Result<tokio::process::Child> {
        command
            .kill_on_drop(true)
            .creation_flags((CREATE_SUSPENDED | CREATE_NO_WINDOW).0);
        let mut child = command.spawn()?;
        let handle = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("spawned child has no process handle"))?;
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("spawned child has no process identity"))?;
        let assigned = unsafe { AssignProcessToJobObject(self.handle(), HANDLE(handle)) }
            .map_err(io::Error::other);
        if let Err(error) = assigned.and_then(|()| resume_child(pid)) {
            let _ = child.start_kill();
            return Err(error);
        }
        Ok(child)
    }

    pub(crate) fn active_processes(&self) -> io::Result<u32> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(self.handle()),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as _,
                std::mem::size_of_val(&info) as u32,
                None,
            )?;
        }
        Ok(info.ActiveProcesses)
    }

    pub(crate) fn request_termination(&self) -> io::Result<()> {
        if self.active_processes()? != 0 {
            unsafe { TerminateJobObject(self.handle(), 1) }?;
        }
        Ok(())
    }

    pub(crate) fn terminate(&self) -> io::Result<()> {
        self.request_termination()?;
        self.wait_until_empty()
    }

    pub(crate) fn wait_until_empty(&self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if self.active_processes()? == 0 {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "runtime job still has active processes",
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

// Thread enumeration/resume adapts the process-wrap strategy; see shared/third-party/process-wrap.
fn resume_child(child_id: u32) -> io::Result<()> {
    // std::process::Child retains the process handle but not the initial thread
    // handle. The suspended process has not run user code; find its owned thread.
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }?);
    let snapshot_handle = HANDLE(snapshot.as_raw_handle());
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    unsafe { Thread32First(snapshot_handle, &mut entry) }?;
    let mut resumed = false;
    loop {
        if entry.th32OwnerProcessID == child_id {
            let thread = owned(unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    false,
                    entry.th32ThreadID,
                )
            }?);
            if unsafe { GetProcessIdOfThread(HANDLE(thread.as_raw_handle())) } != child_id {
                return Err(io::Error::other("runtime thread ownership changed"));
            }
            let previous = unsafe { ResumeThread(HANDLE(thread.as_raw_handle())) };
            if previous == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            if previous > 0 {
                resumed = true;
            }
        }
        match unsafe { Thread32Next(snapshot_handle, &mut entry) } {
            Ok(()) => {}
            Err(error)
                if error.code() == windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) =>
            {
                return if resumed {
                    Ok(())
                } else {
                    Err(io::Error::other("suspended runtime thread was not found"))
                };
            }
            Err(error) => return Err(io::Error::other(error)),
        }
    }
}

#[cfg(test)]
#[path = "process_job/tests.rs"]
mod tests;
