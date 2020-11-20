use crate::{Read, ReadOutcome};
use std::io::{self, IoSliceMut};

/// Adapts an `&[u8]` to implement `Read`.
pub struct SliceReader<'slice> {
    slice: &'slice [u8],
    ended: bool,
}

impl<'slice> SliceReader<'slice> {
    /// Construct a new `SliceReader` which wraps `slice`.
    pub fn new(slice: &'slice [u8]) -> Self {
        Self {
            slice,
            ended: false,
        }
    }
}

impl<'slice> Read for SliceReader<'slice> {
    #[inline]
    fn read_outcome(&mut self, buf: &mut [u8]) -> io::Result<ReadOutcome> {
        if self.ended {
            return Ok(ReadOutcome::end(0));
        }

        let size = io::Read::read(&mut self.slice, buf)?;
        Ok(ReadOutcome::ready_or_not(
            size,
            buf.is_empty() || !self.slice.is_empty(),
        ))
    }

    #[inline]
    fn read_vectored_outcome(&mut self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<ReadOutcome> {
        if self.ended {
            return Ok(ReadOutcome::end(0));
        }

        let size = io::Read::read_vectored(&mut self.slice, bufs)?;
        Ok(ReadOutcome::ready_or_not(
            size,
            bufs.iter().all(|b| b.is_empty()) || !self.slice.is_empty(),
        ))
    }

    #[cfg(feature = "nightly")]
    #[inline]
    fn is_read_vectored(&self) -> bool {
        io::is_read_vectored(&self.inner)
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        if self.ended {
            return Ok(0);
        }

        io::Read::read_to_end(&mut self.slice, buf)
    }

    #[inline]
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        if self.ended {
            return Ok(0);
        }

        io::Read::read_to_string(&mut self.slice, buf)
    }

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if self.ended {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "failed to fill whole buffer",
            ));
        }

        io::Read::read_exact(&mut self.slice, buf)
    }
}

impl<'slice> io::Read for SliceReader<'slice> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Read::read(self, buf)
    }

    #[inline]
    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        Read::read_vectored(self, bufs)
    }

    #[cfg(feature = "nightly")]
    #[inline]
    fn is_read_vectored(&self) -> bool {
        Read::is_read_vectored(self)
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        Read::read_to_end(self, buf)
    }

    #[inline]
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        Read::read_to_string(self, buf)
    }

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        Read::read_exact(self, buf)
    }
}
