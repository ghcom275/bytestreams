use crate::{default_read_exact, default_read_to_end, default_read_to_string, Read, ReadOutcome};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::{
    io::{self, IoSliceMut},
    mem::MaybeUninit,
};

/// Adapts an `io::Read` to implement `Read`.
pub struct StdReader<Inner: io::Read> {
    inner: Inner,
    sticky_end: bool,
    line_by_line: bool,
    ended: bool,
}

#[cfg(not(windows))]
impl<Inner: io::Read + AsRawFd> StdReader<Inner> {
    /// Construct a new `StdReader` which wraps `inner`, which implements
    /// `AsRawFd`, and automatically sets the `line_by_line` setting if
    /// appropriate.
    pub fn new(inner: Inner) -> Self {
        let line_by_line = unsafe {
            let mut termios = MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(inner.as_raw_fd(), termios.as_mut_ptr()) == 0 {
                (termios.assume_init().c_lflag & libc::ICANON) == libc::ICANON
            } else {
                // `tcgetattr` fails when it's not reading from a terminal.
                false
            }
        };

        if line_by_line {
            StdReader::line_by_line(inner)
        } else {
            StdReader::generic(inner)
        }
    }
}

#[cfg(windows)]
impl<Inner: io::Read + AsRawHandle> StdReader<Inner> {
    /// Construct a new `StdReader` which wraps `inner`, which implements
    /// `AsRawHandle`.
    ///
    /// TODO: Does Windows have a concept of line-by-line console input?
    #[cfg(windows)]
    pub fn new(inner: Inner) -> Self {
        StdReader::generic(inner)
    }
}

impl<Inner: io::Read> StdReader<Inner> {
    /// Construct a new `StdReader` which wraps `inner` with generic settings.
    pub fn generic(inner: Inner) -> Self {
        Self {
            inner,
            sticky_end: true,
            line_by_line: false,
            ended: false,
        }
    }

    /// Construct a new `StdReader` which wraps `inner`. When a lull occurs,
    /// don't treat it as the end of the stream, but keep waiting to see if
    /// more data arrives.
    pub fn wait_for_lulls(inner: Inner) -> Self {
        Self {
            inner,
            sticky_end: false,
            line_by_line: false,
            ended: false,
        }
    }

    /// Construct a new `StdReader` which wraps an `inner` which reads its
    /// input line-by-line, such as stdin on a terminal.
    pub fn line_by_line(inner: Inner) -> Self {
        Self {
            inner,
            sticky_end: true,
            line_by_line: true,
            ended: false,
        }
    }
}

impl<Inner: io::Read> Read for StdReader<Inner> {
    #[inline]
    fn read_outcome(&mut self, buf: &mut [u8]) -> io::Result<ReadOutcome> {
        if self.ended {
            return Ok(ReadOutcome::end(0));
        }
        match self.inner.read(buf) {
            Ok(0) if !buf.is_empty() => {
                if self.sticky_end {
                    self.ended = true;
                    Ok(ReadOutcome::end(0))
                } else {
                    Ok(ReadOutcome::lull(0))
                }
            }
            Ok(size) => {
                if self.line_by_line && buf[size - 1] == b'\n' {
                    Ok(ReadOutcome::lull(size))
                } else {
                    Ok(ReadOutcome::ready(size))
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => Ok(ReadOutcome::ready(0)),
            Err(e) => Err(e),
        }
    }

    #[inline]
    fn read_vectored_outcome(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<ReadOutcome> {
        if self.ended {
            return Ok(ReadOutcome::end(0));
        }
        match self.inner.read_vectored(bufs) {
            Ok(0) if !bufs.iter().all(|b| b.is_empty()) => {
                if self.sticky_end {
                    self.ended = true;
                    Ok(ReadOutcome::end(0))
                } else {
                    Ok(ReadOutcome::lull(0))
                }
            }
            Ok(size) => {
                if self.line_by_line {
                    let mut i = size;
                    let mut saw_line = false;
                    for buf in bufs.iter() {
                        if i < buf.len() {
                            saw_line = buf[i - 1] == b'\n';
                            break;
                        }
                        i -= bufs.len();
                    }
                    if saw_line {
                        return Ok(ReadOutcome::lull(size));
                    }
                }

                Ok(ReadOutcome::ready(size))
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => Ok(ReadOutcome::ready(0)),
            Err(e) => Err(e),
        }
    }

    #[cfg(feature = "nightly")]
    #[inline]
    fn is_read_vectored(&self) -> bool {
        self.inner.is_read_vectored(self)
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        if self.ended {
            return Ok(0);
        }

        default_read_to_end(self, buf)
    }

    #[inline]
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        if self.ended {
            return Ok(0);
        }

        default_read_to_string(self, buf)
    }

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if self.ended {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "failed to fill whole buffer",
            ));
        }

        default_read_exact(self, buf)
    }
}

#[test]
fn test_std_reader() {
    let mut input = io::Cursor::new(b"hello world");
    let mut reader = StdReader::generic(&mut input);
    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    assert_eq!(s, "hello world");
}
