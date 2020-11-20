use crate::{unicode::NORMALIZATION_BUFFER_SIZE, Readiness, Status};
use std::io::{self, IoSliceMut};

/// A superset of [`std::io::Read`], with `read_outcome` and
/// `read_vectored_outcome` which return more information and zero is not
/// special-cased.
pub trait Read {
    /// Like [`std::io::Read::read`], but returns a `ReadOutcome`.
    fn read_outcome(&mut self, buf: &mut [u8]) -> io::Result<ReadOutcome>;

    /// Like [`std::io::Read::read_vectored`], but returns a `ReadOutcome`.
    fn read_vectored_outcome(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<ReadOutcome> {
        default_read_vectored_outcome(self, bufs)
    }

    /// Like [`std::io::Read::read`].
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        default_read(self, buf)
    }

    /// Like [`std::io::Read::read_vectored`].
    fn read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        default_read_vectored(self, bufs)
    }

    /// Like [`std::io::Read::is_read_vectored`].
    #[cfg(feature = "nightly")]
    fn is_read_vectored(&self) -> bool;

    /// Like [`std::io::Read::read_to_end`] (but sometimes more efficient).
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        default_read_to_end(self, buf)
    }

    /// Like [`std::io::Read::read_to_string`] (but sometimes more efficient).
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        default_read_to_string(self, buf)
    }

    /// Like [`std::io::Read::read_exact`].
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        default_read_exact(self, buf)
    }
}

/// Information returned after a successful read.
#[derive(Clone, Debug)]
pub struct ReadOutcome {
    /// The number of bytes read.
    pub size: usize,

    /// What to expect from future reads from the stream.
    pub status: Status,
}

impl ReadOutcome {
    /// Data was read on a stream which remains open.
    #[inline]
    pub fn ready(size: usize) -> Self {
        Self {
            size,
            status: Status::ready(),
        }
    }

    /// Data was read on a stream which remains open.
    #[inline]
    pub fn ready_or_not(size: usize, ready: bool) -> Self {
        Self {
            size,
            status: Status::ready_or_not(ready),
        }
    }

    /// Data was read on a stream which is now closed.
    #[inline]
    pub fn end(size: usize) -> Self {
        Self {
            size,
            status: Status::End,
        }
    }

    /// Data was read on a stream which is now at a lull.
    #[inline]
    pub fn lull(size: usize) -> Self {
        Self {
            size,
            status: Status::Open(Readiness::Lull),
        }
    }
}

/// Default implementation of `Read::read`.
pub fn default_read<Inner: Read + ?Sized>(inner: &mut Inner, buf: &mut [u8]) -> io::Result<usize> {
    inner.read_outcome(buf).and_then(outcome_to_usize)
}

/// Default implementation of `Read::read_vectored`.
pub fn default_read_vectored<Inner: Read + ?Sized>(
    inner: &mut Inner,
    bufs: &mut [IoSliceMut<'_>],
) -> io::Result<usize> {
    inner.read_vectored_outcome(bufs).and_then(outcome_to_usize)
}

/// Default implementation of `Read::read_vectored_outcome`.
pub fn default_read_vectored_outcome<Inner: Read + ?Sized>(
    inner: &mut Inner,
    bufs: &mut [IoSliceMut<'_>],
) -> io::Result<ReadOutcome> {
    let buf = bufs
        .iter_mut()
        .find(|b| !b.is_empty())
        .map_or(&mut [][..], |b| &mut **b);
    inner.read_outcome(buf)
}

/// Default implementation of `Read::read_to_end`.
pub fn default_read_to_end<Inner: Read + ?Sized>(
    inner: &mut Inner,
    buf: &mut Vec<u8>,
) -> io::Result<usize> {
    let start_len = buf.len();
    let buffer_size = 1024;
    let mut read_len = buffer_size;
    loop {
        let read_pos = buf.len();

        // Allocate space in the buffer. This needlessly zeros out the
        // memory, however the current way to avoid it is to be part of the
        // standard library so that we can make assumptions about the
        // compiler not exploiting undefined behavior.
        // https://github.com/rust-lang/rust/issues/42788 for details.
        buf.resize(read_pos + read_len, 0);

        match inner.read_outcome(&mut buf[read_pos..]) {
            Ok(ReadOutcome { size, status }) => {
                buf.resize(read_pos + size, 0);
                match status {
                    Status::Open(_) => {
                        read_len -= size;
                        if read_len < NORMALIZATION_BUFFER_SIZE {
                            read_len += buffer_size;
                        }
                    }
                    Status::End => return Ok(buf.len() - start_len),
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                buf.resize(start_len, 0);
                return Err(e);
            }
        }

        if read_len == 0 {
            read_len = buffer_size;
        }
    }
}

/// Default implementation of `Read::read_to_string`.
pub fn default_read_to_string<Inner: Read + ?Sized>(
    inner: &mut Inner,
    buf: &mut String,
) -> io::Result<usize> {
    // Allocate a `Vec` and read into it. This needlessly allocates,
    // rather than reading directly into `buf`'s buffer, but similarly
    // avoids issues of undefined behavior for now.
    let mut vec = Vec::new();
    let size = inner.read_to_end(&mut vec)?;
    let new = String::from_utf8(vec).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    buf.push_str(&new);
    Ok(size)
}

/// Default implementation of `Read::read_exact`.
pub fn default_read_exact<Inner: Read + ?Sized>(
    inner: &mut Inner,
    mut buf: &mut [u8],
) -> io::Result<()> {
    while !buf.is_empty() {
        match inner.read_outcome(buf) {
            Ok(ReadOutcome { size, status }) => {
                let t = buf;
                buf = &mut t[size..];
                if status.is_end() {
                    break;
                }
            }
            Err(e) => return Err(e),
        }
    }

    if buf.is_empty() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "failed to fill whole buffer",
        ))
    }
}

fn outcome_to_usize(outcome: ReadOutcome) -> io::Result<usize> {
    match outcome {
        ReadOutcome {
            size: 0,
            status: Status::Open(_),
        } => Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "read zero bytes from stream",
        )),
        ReadOutcome { size, status: _ } => Ok(size),
    }
}
