//! Owned Windows process tree. Only signal the retained Job; PID hints require handle membership verification.
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{
    DuplicateHandle, DUPLICATE_SAME_ACCESS, ERROR_MORE_DATA, ERROR_NO_MORE_FILES, HANDLE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessIdOfThread, OpenProcess, OpenThread, ResumeThread,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

pub(crate) struct ProcessJob {
    handle: OwnedHandle,
    exit: Mutex<ExitProof>,
}
#[derive(Default)]
struct ExitProof {
    leader: Option<OwnedHandle>,
    members: Vec<OwnedHandle>,
    requested: bool,
    total_at_stop: Option<u32>,
    incomplete: bool,
}

fn owned(handle: HANDLE) -> OwnedHandle {
    // Only called for successful APIs returning a newly owned, valid handle.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}

impl ProcessJob {
    fn handle(&self) -> HANDLE {
        HANDLE(self.handle.as_raw_handle())
    }

    pub(crate) fn new() -> io::Result<Self> {
        let job = Self {
            handle: owned(unsafe { CreateJobObjectW(None, None) }?),
            exit: Mutex::new(ExitProof::default()),
        };
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
        if let Err(error) = assigned
            .and_then(|()| job.retain_leader(HANDLE(child.as_raw_handle())))
            .and_then(|()| resume_child(child.id()))
        {
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
        if let Err(error) = assigned
            .and_then(|()| self.retain_leader(HANDLE(handle)))
            .and_then(|()| resume_child(pid))
        {
            let _ = child.start_kill();
            return Err(error);
        }
        Ok(child)
    }

    fn accounting(&self) -> io::Result<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION> {
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
        Ok(info)
    }

    pub(crate) fn active_processes(&self) -> io::Result<u32> {
        Ok(self.accounting()?.ActiveProcesses)
    }

    fn retain_leader(&self, process: HANDLE) -> io::Result<()> {
        let mut duplicate = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                process,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        }?;
        self.exit.lock().unwrap_or_else(|e| e.into_inner()).leader = Some(owned(duplicate));
        Ok(())
    }

    fn member_ids(&self) -> io::Result<Vec<u32>> {
        let mut capacity = 16usize;
        loop {
            // usize storage provides alignment for the variable-length Win32 structure.
            let mut buffer = vec![0usize; capacity + 2];
            let list = buffer
                .as_mut_ptr()
                .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            let result = unsafe {
                QueryInformationJobObject(
                    Some(self.handle()),
                    JobObjectBasicProcessIdList,
                    list.cast(),
                    (buffer.len() * std::mem::size_of::<usize>()) as u32,
                    None,
                )
            };
            match result {
                Ok(()) => {
                    let (assigned, count) = unsafe {
                        (
                            (*list).NumberOfAssignedProcesses as usize,
                            (*list).NumberOfProcessIdsInList as usize,
                        )
                    };
                    if assigned <= capacity && count == assigned {
                        let ids = unsafe {
                            std::slice::from_raw_parts((*list).ProcessIdList.as_ptr(), count)
                        };
                        return ids
                            .iter()
                            .map(|&pid| u32::try_from(pid).map_err(io::Error::other))
                            .collect();
                    }
                }
                Err(error)
                    if error.code() == windows::core::HRESULT::from_win32(ERROR_MORE_DATA.0) => {}
                Err(error) => return Err(io::Error::other(error)),
            }
            capacity *= 2;
            if capacity > 16384 {
                return Err(io::Error::other(
                    "runtime job membership exceeds supervision capacity",
                ));
            }
        }
    }

    fn capture_members(&self) -> io::Result<(Vec<OwnedHandle>, u32)> {
        for _ in 0..16 {
            let before = self.accounting()?;
            let ids = self.member_ids()?;
            let expected = ids.len();
            let mut handles = Vec::with_capacity(ids.len());
            for pid in ids {
                // The PID is only an enumeration hint. Verify membership on the
                // retained handle; never terminate or trust an unrelated reused PID.
                let Ok(handle) = (unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                        false,
                        pid,
                    )
                }) else {
                    break;
                };
                let handle = owned(handle);
                let mut member = windows::core::BOOL::default();
                unsafe {
                    IsProcessInJob(
                        HANDLE(handle.as_raw_handle()),
                        Some(self.handle()),
                        &mut member,
                    )
                }?;
                if !member.as_bool() {
                    break;
                }
                handles.push(handle);
            }
            let after = self.accounting()?;
            if before.TotalProcesses == after.TotalProcesses
                && handles.len() == expected
                && handles.len() == after.ActiveProcesses as usize
            {
                return Ok((handles, after.TotalProcesses));
            }
        }
        Err(io::Error::other(
            "runtime job membership could not be captured consistently",
        ))
    }

    pub(crate) fn request_termination(&self) -> io::Result<()> {
        let mut proof = self.exit.lock().unwrap_or_else(|e| e.into_inner());
        if !proof.requested {
            match self.capture_members() {
                Ok((members, total)) => {
                    proof.members = members;
                    proof.total_at_stop = Some(total);
                }
                Err(_) => proof.incomplete = true,
            }
            proof.requested = true;
            // TerminateJobObject is asynchronous. An empty job accounting list
            // is not proof that the retained process objects have been signaled.
            if let Err(error) = unsafe { TerminateJobObject(self.handle(), 1) } {
                proof.incomplete = true;
                return Err(io::Error::other(error));
            }
            if Some(self.accounting()?.TotalProcesses) != proof.total_at_stop {
                // A process joined between capture and termination. Preserve the
                // uncertain-stop state instead of falsely releasing account access.
                proof.incomplete = true;
            }
        }
        if proof.incomplete {
            return Err(io::Error::other("runtime job exit identity is unconfirmed"));
        }
        Ok(())
    }

    pub(crate) fn terminate(&self) -> io::Result<()> {
        self.request_termination()?;
        self.wait_until_empty()
    }

    pub(crate) fn confirm_exit(&self) -> io::Result<bool> {
        let proof = self.exit.lock().unwrap_or_else(|e| e.into_inner());
        if !proof.requested
            || proof.incomplete
            || Some(self.accounting()?.TotalProcesses) != proof.total_at_stop
        {
            return Err(io::Error::other("runtime job exit identity is unconfirmed"));
        }
        for handle in proof.leader.iter().chain(&proof.members) {
            match unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 0) } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => return Ok(false),
                _ => return Err(io::Error::last_os_error()),
            }
        }
        Ok(self.active_processes()? == 0)
    }

    pub(crate) fn wait_until_empty(&self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.confirm_exit()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "runtime job process handles have not exited",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
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
