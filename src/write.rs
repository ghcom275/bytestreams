use crate::Status;
use std::{
    fmt::Arguments,
    io::{self, IoSlice},
};

/// A superset of [`std::io::Write`], but has extra parameters for declaring
/// status, and an extra `write_all_utf8` function.
pub trait Write {
    /// Like [`std::io::Write::write`].
    fn write(&mut self, buf: &[u8]) -> io::Result<usize>;

    /// Like [`std::io::Write::flush`], but has a status parameter describing
    /// the future of the stream:
    ///  - `Status::Ok(Readiness::Ready)`: do nothing
    ///  - `Status::Ok(Readiness::Lull)`: flush the underlying stream
    ///  - `Status::End`: flush the underlying stream and declare the end
    fn flush(&mut self, status: Status) -> io::Result<()>;

    /// Discard any buffered bytes and declare an intention to cease using
    /// this stream. Use after an unrecoverable error.
    fn abandon(&mut self);

    /// Like [`std::io::Write::write_vectored`].
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        default_write_vectored(self, bufs)
    }

    /// Like [`std::io::Write::is_write_Vectored`].
    #[cfg(feature = "nightly")]
    fn is_write_vectored(&self) -> bool;

    /// Like [`std::io::Write::write_all`].
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        default_write_all(self, buf)
    }

    /// Like `write_all`, but takes a `&str`.
    fn write_all_utf8(&mut self, buf: &str) -> io::Result<()> {
        // Default to just writing it as bytes, however implementors of this
        // trait can override this to take advantage of knowing the input is
        // valid UTF-8.
        self.write_all(buf.as_bytes())
    }

    /// Like [`std::io::Write::write_all_vectored`].
    #[cfg(feature = "nightly")]
    fn write_all_vectored(&mut self, bufs: &mut [IoSlice<'_>]) -> io::Result<()>;

    /// Like [`std::io::Write::write_fmt`].
    fn write_fmt(&mut self, fmt: Arguments<'_>) -> io::Result<()> {
        let s = fmt.to_string();
        self.write_all_utf8(&s)
    }
}

/// Default implementation of `Write::write_vectored`.
pub fn default_write_vectored<Inner: Write + ?Sized>(
    inner: &mut Inner,
    bufs: &[IoSlice<'_>],
) -> io::Result<usize> {
    let buf = bufs
        .iter()
        .find(|b| !b.is_empty())
        .map_or(&[][..], |b| &**b);
    inner.write(buf)
}

/// Default implementation of `Write::write_all`.
pub fn default_write_all<Inner: Write + ?Sized>(
    inner: &mut Inner,
    mut buf: &[u8],
) -> io::Result<()> {
    while !buf.is_empty() {
        match inner.write(buf) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole buffer",
                ));
            }
            Ok(n) => buf = &buf[n..],
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
