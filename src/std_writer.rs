use crate::{Readiness, Status, Write};
use std::{
    fmt::Arguments,
    io::{self, IoSlice},
};

/// Adapts a [`std::io::Write`] to implement [`Write`].
pub struct StdWriter<Inner: io::Write> {
    inner: Inner,
    ended: bool,
}

impl<Inner: io::Write> StdWriter<Inner> {
    /// Construct a new instance of `StdWriter` wrapping `inner`.
    pub fn new(inner: Inner) -> Self {
        Self {
            inner,
            ended: false,
        }
    }

    /// Gets a reference to the underlying writer.
    pub fn get_ref(&self) -> &Inner {
        &self.inner
    }

    /// Gets a mutable reference to the underlying writer.
    ///
    /// It is inadvisable to directly write to the underlying writer.
    pub fn get_mut(&mut self) -> &mut Inner {
        &mut self.inner
    }
}

impl<Inner: io::Write> Write for StdWriter<Inner> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.ended {
            return Err(stream_already_ended());
        }
        self.inner.write(buf)
    }

    #[inline]
    fn flush(&mut self, status: Status) -> io::Result<()> {
        if self.ended {
            return Err(stream_already_ended());
        }
        match status {
            Status::Open(Readiness::Ready) => Ok(()),
            Status::Open(Readiness::Lull) => self.inner.flush(),
            Status::End => {
                self.ended = true;
                self.inner.flush()
            }
        }
    }

    #[inline]
    fn abandon(&mut self) {
        self.ended = true;
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        if self.ended {
            return Err(stream_already_ended());
        }
        self.inner.write_vectored(bufs)
    }

    #[cfg(feature = "nightly")]
    #[inline]
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.ended {
            return Err(stream_already_ended());
        }
        self.inner.write_all(buf)
    }

    #[cfg(feature = "nightly")]
    #[inline]
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()> {
        if self.ended {
            return Err(stream_already_ended());
        }
        self.inner.write_all_vectored(bufs)
    }

    #[inline]
    fn write_fmt(&mut self, fmt: Arguments<'_>) -> io::Result<()> {
        if self.ended {
            return Err(stream_already_ended());
        }
        self.inner.write_fmt(fmt)
    }
}

fn stream_already_ended() -> io::Error {
    io::Error::new(io::ErrorKind::Other, "stream has already ended")
}
