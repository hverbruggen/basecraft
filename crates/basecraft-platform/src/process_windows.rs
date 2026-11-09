//! Owned Windows Job Object: assign the suspended child before it can spawn.
use std::io;
use std::os::windows::{
    io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::CommandExt,
};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
            WaitForSingleObject,
        },
    },
};

pub struct OwnedProcess {
    child: Child,
    job: OwnedHandle,
    status: Option<ExitStatus>,
}
fn resume_initial_thread(pid: u32) -> io::Result<()> {
    // SAFETY: owning wrappers close successful handles; all structs have correct sizes.
    unsafe {
        let raw = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = OwnedHandle::from_raw_handle(raw);
        let mut entry = THREADENTRY32 {
            dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>())
                .map_err(io::Error::other)?,
            ..std::mem::zeroed()
        };
        let mut found = Thread32First(snapshot.as_raw_handle(), &raw mut entry);
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let raw = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if raw.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = OwnedHandle::from_raw_handle(raw);
                if ResumeThread(thread.as_raw_handle()) == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            found = Thread32Next(snapshot.as_raw_handle(), &raw mut entry);
        }
        Err(io::Error::other("suspended plugin thread not found"))
    }
}
impl OwnedProcess {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        // SAFETY: all pointers remain valid during each call; job is retained
        // until all descendants are terminated. Failure never resumes the child.
        unsafe {
            let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = OwnedHandle::from_raw_handle(raw);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                u32::try_from(std::mem::size_of_val(&limits)).map_err(io::Error::other)?,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut child = command
                .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)
                .spawn()?;
            let result =
                if AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    resume_initial_thread(child.id())
                };
            if let Err(error) = result {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            Ok(Self {
                child,
                job,
                status: None,
            })
        }
    }
    pub fn exited(&self) -> io::Result<bool> {
        // SAFETY: Child keeps the process handle valid until drop.
        match unsafe { WaitForSingleObject(self.child.as_raw_handle() as HANDLE, 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }
    pub fn finish(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        // Preserve a completed leader's status before killing surviving descendants.
        let completed = self.child.try_wait()?;
        self.terminate();
        let status = match completed {
            Some(status) => status,
            None => self.child.wait()?,
        };
        self.status = Some(status);
        Ok(status)
    }
    pub fn terminate(&mut self) {
        // SAFETY: this is exclusively our retained job, never a PID lookup.
        unsafe {
            TerminateJobObject(self.job.as_raw_handle(), 1);
        }
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.terminate();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
        QueryInformationJobObject,
    };
    #[test]
    fn preserves_exit_code_and_finish_is_repeatable() {
        let mut process =
            OwnedProcess::spawn(Command::new("cmd.exe").args(["/C", "exit 7"])).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !process.exited().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(process.finish().unwrap().code(), Some(7));
        assert_eq!(process.finish().unwrap().code(), Some(7));
    }
    #[test]
    fn job_termination_reaps_descendants() {
        let mut process = OwnedProcess::spawn(Command::new("cmd.exe").args([
            "/C",
            "start /B ping -n 60 127.0.0.1 >NUL & ping -n 60 127.0.0.1 >NUL",
        ]))
        .unwrap();
        let active = |job: &OwnedHandle| {
            // SAFETY: the job is retained and the output buffer has the API's layout.
            unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                assert_ne!(
                    QueryInformationJobObject(
                        job.as_raw_handle(),
                        JobObjectBasicAccountingInformation,
                        (&raw mut info).cast(),
                        u32::try_from(std::mem::size_of_val(&info)).unwrap(),
                        std::ptr::null_mut()
                    ),
                    0
                );
                info.ActiveProcesses
            }
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while active(&process.job) < 2 {
            assert!(
                Instant::now() < deadline,
                "descendant did not join owned job"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        process.terminate();
        process.finish().unwrap();
        while active(&process.job) != 0 {
            assert!(
                Instant::now() < deadline,
                "descendant survived job termination"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn termination_stops_owned_process() {
        let mut process =
            OwnedProcess::spawn(Command::new("cmd.exe").args(["/C", "ping -n 60 127.0.0.1 >NUL"]))
                .unwrap();
        process.terminate();
        assert!(!process.finish().unwrap().success());
        assert!(process.exited().unwrap());
    }
}
