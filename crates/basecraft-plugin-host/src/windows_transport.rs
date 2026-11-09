//! Windows loopback framing; authentication is owned by `PluginProcess` startup.
//! Snapshot paths refer to host-owned read-only files held against modification.
use std::io::{self, Read};
use std::net::TcpStream;
use std::path::Path;
pub type BorrowedFd<'a> = &'a Path;
pub type OwnedFd = std::fs::File;
pub struct Receiver {
    stream: TcpStream,
    buf: Vec<u8>,
    max_frame: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{Shutdown, TcpListener};
    use std::time::Duration;

    fn receiver(bytes: &[u8], limit: usize) -> Receiver {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        sender.write_all(bytes).unwrap();
        sender.shutdown(Shutdown::Write).unwrap();
        Receiver::with_limits(stream, limit, 0)
    }

    #[test]
    fn framing_rejects_oversize_partial_and_response_handles() {
        let mut valid = receiver(b"one\ntwo\n", 3);
        assert_eq!(
            valid.line_checked(|| Ok(())).unwrap(),
            Some(b"one".to_vec())
        );
        assert_eq!(
            valid.line_checked(|| Ok(())).unwrap(),
            Some(b"two".to_vec())
        );
        assert!(valid.line_checked(|| Ok(())).unwrap().is_none());
        assert!(valid.take_fds(1).is_err());
        for bytes in [&b"long\n"[..], &b"long"[..], &b"ab"[..]] {
            assert!(receiver(bytes, 3).line_checked(|| Ok(())).is_err());
        }
    }

    #[test]
    fn cancellation_is_checked_before_delivering_buffered_frames() {
        let mut input = receiver(b"one\ntwo\n", 3);
        input.line_checked(|| Ok(())).unwrap();
        let error = input
            .line_checked(|| Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled")))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }
}
impl Receiver {
    pub fn with_limits(stream: TcpStream, max_frame: usize, _: usize) -> Self {
        Self {
            stream,
            buf: Vec::new(),
            max_frame,
        }
    }
    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }
    pub fn line_checked(
        &mut self,
        mut check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Option<Vec<u8>>> {
        loop {
            check()?;
            if let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
                if end > self.max_frame {
                    return Err(io::Error::other("plugin frame byte limit exceeded"));
                }
                let mut result: Vec<_> = self.buf.drain(..=end).collect();
                result.pop();
                return Ok(Some(result));
            }
            if self.buf.len() > self.max_frame {
                return Err(io::Error::other("plugin frame byte limit exceeded"));
            }
            let mut chunk = [0; 16384];
            let n = self.stream.read(&mut chunk)?;
            if n == 0 {
                if !self.buf.is_empty() {
                    return Err(io::Error::other("incomplete plugin frame at EOF"));
                }
                return Ok(None);
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
    // Matches the Unix Receiver interface; Windows never accepts response handles.
    #[allow(clippy::unused_self)]
    pub fn take_fds(&mut self, n: usize) -> io::Result<Vec<OwnedFd>> {
        if n != 0 {
            return Err(io::Error::other("unsolicited Windows response buffers"));
        }
        Ok(Vec::new())
    }
}
