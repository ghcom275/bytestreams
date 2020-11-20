use crate::{Status, Write};
use std::{io, str};

/// A `Write` implementation which translates into an output `Write` producing
/// a valid UTF-8 sequence from an arbitrary byte sequence from an arbitrary
/// byte sequence. Attempts to write invalid encodings are reported as errors.
///
/// `write` is not guaranteed to perform a single operation, because short
/// writes could produce invalid UTF-8, so `write` will retry as needed.
pub struct Utf8Writer<Inner: Write> {
    /// The wrapped byte stream.
    inner: Inner,
}

impl<Inner: Write> Utf8Writer<Inner> {
    /// Construct a new instance of `Utf8Writer` wrapping `inner`.
    #[inline]
    pub fn new(inner: Inner) -> Self {
        Self { inner }
    }

    /// Flush and close the underlying stream and return the underlying
    /// stream object.
    pub fn close_into_inner(mut self) -> io::Result<Inner> {
        self.inner.flush(Status::End)?;
        Ok(self.inner)
    }
}

impl<Inner: Write> Write for Utf8Writer<Inner> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match str::from_utf8(buf) {
            Ok(s) => self.write_all_utf8(s).map(|_| buf.len()),
            Err(error) if error.valid_up_to() != 0 => self
                .write_all(&buf[..error.valid_up_to()])
                .map(|_| error.valid_up_to()),
            Err(error) => {
                self.inner.abandon();
                Err(io::Error::new(io::ErrorKind::Other, error))
            }
        }
    }

    #[inline]
    fn flush(&mut self, status: Status) -> io::Result<()> {
        self.inner.flush(status)
    }

    #[inline]
    fn abandon(&mut self) {
        self.inner.abandon()
    }

    #[inline]
    fn write_all_utf8(&mut self, s: &str) -> io::Result<()> {
        self.inner.write_all_utf8(s)
    }
}
