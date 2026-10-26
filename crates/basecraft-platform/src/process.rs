//! Owned Unix process groups. Keep the leader unreaped until group teardown,
//! preventing its process-group identity from being reused during ownership.
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdout, Command};

use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};

pub struct OwnedProcess {
    child: Child,
    pid: Pid,
    reaped: bool,
}
impl OwnedProcess {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command.process_group(0).spawn()?;
        let pid = Pid::from_child(&child);
        Ok(Self {
            child,
            pid,
            reaped: false,
        })
    }
    pub fn exited(&self) -> io::Result<bool> {
        Ok(waitid(
            WaitId::Pid(self.pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )?
        .is_some())
    }
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }
    /// Terminate remaining descendants and reap an exited leader, retaining its status.
    pub fn finish(&mut self) -> io::Result<std::process::ExitStatus> {
        self.terminate();
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }
    pub fn terminate(&mut self) {
        if !self.reaped {
            let _ = kill_process_group(self.pid, Signal::KILL);
        }
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.terminate();
        let _ = self.child.wait();
    }
}
