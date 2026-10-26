//! Sending messages with attached file descriptors over a Unix socket.

use std::collections::VecDeque;
use std::io::{self, IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

/// Most descriptors accepted per received chunk.
const MAX_FDS: usize = 16;

/// Send all of `bytes`, attaching `fds` to the first chunk.
pub fn send(stream: &UnixStream, bytes: &[u8], fds: &[BorrowedFd<'_>]) -> io::Result<()> {
    let mut sent = 0;
    let mut first = true;
    while sent < bytes.len() {
        let n = if first && !fds.is_empty() {
            let mut space = vec![MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(fds.len()))];
            let mut control = SendAncillaryBuffer::new(&mut space);
            if !control.push(SendAncillaryMessage::ScmRights(fds)) {
                return Err(io::Error::other("too many descriptors"));
            }
            sendmsg(
                stream,
                &[IoSlice::new(&bytes[sent..])],
                &mut control,
                SendFlags::empty(),
            )?
        } else {
            sendmsg(
                stream,
                &[IoSlice::new(&bytes[sent..])],
                &mut SendAncillaryBuffer::default(),
                SendFlags::empty(),
            )?
        };
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "plugin socket write returned zero",
            ));
        }
        first = false;
        sent += n;
    }
    Ok(())
}

/// Reads newline-delimited messages and the descriptors that arrive with
/// them. Descriptors are queued in arrival order; a message says how many
/// it carries, and the reader takes that many from the queue.
pub struct Receiver {
    stream: UnixStream,
    buf: Vec<u8>,
    /// Scratch space for one `recvmsg`, reused across calls.
    chunk: Box<[u8]>,
    fds: VecDeque<OwnedFd>,
    max_frame: usize,
    max_fds: usize,
}

impl Receiver {
    pub fn new(stream: UnixStream) -> Self {
        Self::with_limits(stream, 8 * 1024 * 1024, MAX_FDS)
    }

    pub fn with_limits(stream: UnixStream, max_frame: usize, max_fds: usize) -> Self {
        Self {
            stream,
            buf: Vec::new(),
            chunk: vec![0u8; 64 * 1024].into_boxed_slice(),
            fds: VecDeque::new(),
            max_frame,
            max_fds: max_fds.min(MAX_FDS),
        }
    }

    pub fn stream(&self) -> &UnixStream {
        &self.stream
    }

    /// The next line, without its newline. `None` at end of stream.
    pub fn line(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.line_checked(|| Ok(()))
    }

    pub fn line_checked(
        &mut self,
        mut check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Option<Vec<u8>>> {
        loop {
            check()?;
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                if i > self.max_frame {
                    return Err(io::Error::other("plugin frame byte limit exceeded"));
                }
                let mut line: Vec<u8> = self.buf.drain(..=i).collect();
                line.pop();
                return Ok(Some(line));
            }
            if self.buf.len() > self.max_frame {
                return Err(io::Error::other("plugin frame byte limit exceeded"));
            }
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let got = recvmsg(
                self.stream.as_fd(),
                &mut [IoSliceMut::new(&mut self.chunk)],
                &mut control,
                RecvFlags::CMSG_CLOEXEC,
            )?;
            for msg in control.drain() {
                if let RecvAncillaryMessage::ScmRights(fds) = msg {
                    self.fds.extend(fds);
                }
            }
            if got.flags.contains(rustix::net::ReturnFlags::CTRUNC) || self.fds.len() > self.max_fds
            {
                return Err(io::Error::other("plugin descriptor limit exceeded"));
            }
            if got.bytes == 0 {
                if !self.buf.is_empty() || !self.fds.is_empty() {
                    return Err(io::Error::other("incomplete plugin frame at EOF"));
                }
                return Ok(None);
            }
            self.buf.extend_from_slice(&self.chunk[..got.bytes]);
        }
    }

    /// Take the next `n` received descriptors.
    pub fn take_fds(&mut self, n: usize) -> io::Result<Vec<OwnedFd>> {
        if n > self.max_fds || self.fds.len() < n {
            return Err(io::Error::other(format!(
                "expected {n} descriptors, have {}",
                self.fds.len()
            )));
        }
        Ok(self.fds.drain(..n).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, Write};

    #[test]
    fn message_with_descriptor() {
        let (a, b) = UnixStream::pair().unwrap();
        let mut file = tempfile_like();
        file.write_all(b"payload").unwrap();
        send(&a, b"{\"fds\":1}\nsecond\n", &[file.as_fd()]).unwrap();
        let mut r = Receiver::new(b);
        assert_eq!(r.line().unwrap().unwrap(), b"{\"fds\":1}");
        let fd = r.take_fds(1).unwrap().pop().unwrap();
        let mut got = std::fs::File::from(fd);
        got.rewind().unwrap();
        let mut s = String::new();
        got.read_to_string(&mut s).unwrap();
        assert_eq!(s, "payload");
        assert_eq!(r.line().unwrap().unwrap(), b"second");
        drop(a);
        assert_eq!(r.line().unwrap(), None);
    }

    fn tempfile_like() -> std::fs::File {
        let w = crate::shm::SharedWriter::new("t").unwrap();
        std::fs::File::from(OwnedFd::from(w.into_file()))
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_oversized_and_partial_frames_and_fd_backlog() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        sender.write_all(b"12345\n").unwrap();
        assert!(Receiver::with_limits(receiver, 4, 1).line().is_err());
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        sender.write_all(b"partial").unwrap();
        drop(sender);
        assert!(Receiver::new(receiver).line().is_err());
        let (sender, receiver) = UnixStream::pair().unwrap();
        let writer = crate::shm::SharedWriter::new("fd-budget").unwrap();
        let file = OwnedFd::from(writer.into_file());
        send(&sender, b"one", &[file.as_fd()]).unwrap();
        send(&sender, b"two\n", &[file.as_fd()]).unwrap();
        assert!(Receiver::with_limits(receiver, 100, 1).line().is_err());
    }

    #[test]
    fn bounded_receiver_preserves_multiple_frames_and_timeout_partial_reads() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_millis(10)))
            .unwrap();
        let mut reader = Receiver::with_limits(receiver, 4, 1);
        sender.write_all(b"12").unwrap();
        assert!(reader.line().is_err());
        sender.write_all(b"34\nok\n").unwrap();
        assert_eq!(reader.line().unwrap().unwrap(), b"1234");
        assert_eq!(reader.line().unwrap().unwrap(), b"ok");
    }
}
