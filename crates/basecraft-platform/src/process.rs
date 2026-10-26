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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finishing_exited_group_preserves_failure_status_and_is_repeatable() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 23"]);
        let mut process = OwnedProcess::spawn(&mut command).unwrap();
        let start = std::time::Instant::now();
        while !process.exited().unwrap() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(process.finish().unwrap().code(), Some(23));
        assert!(process.reaped);
        assert_eq!(process.finish().unwrap().code(), Some(23));
    }
}
