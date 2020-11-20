/// What is known about a stream in the future.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Status {
    /// The stream remains open.
    Open(Readiness),

    /// The stream has ended. No more bytes will be transmitted.
    End,
}

impl Status {
    /// Return `Status::Open` with readiness state `Ready`.
    #[inline]
    pub fn ready() -> Self {
        Self::Open(Readiness::Ready)
    }

    /// Return either `Status::Open` with readiness state `Ready` or
    /// `Status::End`.
    #[inline]
    pub fn ready_or_not(ready: bool) -> Self {
        if ready {
            Self::Open(Readiness::Ready)
        } else {
            Self::End
        }
    }

    /// Shorthand for testing equality with `Status::End`.
    #[inline]
    pub fn is_end(&self) -> bool {
        *self == Self::End
    }
}

/// Whether a stream is ready or in a temporary lull. Most users can
/// ignore this.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Readiness {
    /// There may be more bytes waiting to be read.
    Ready,

    /// The input source has indicated that there are no more bytes waiting to
    /// be read at this time. More bytes may become available in the future.
    ///
    /// This is not to be confused with data which waiting to be read but which
    /// will take time to be delivered.
    Lull,
}
