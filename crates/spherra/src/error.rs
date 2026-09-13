use std::{fmt, io, sync::Arc};
/// Errors distinguish unpublished failures from a publication whose final sync failed.
#[derive(Clone, Debug)]
pub enum Error {
    NotFound,
    AlreadyExists,
    IndexBusy,
    InvalidVector { position: u64 },
    InvalidTraining,
    InvalidOptions,
    EmptyCommit,
    RowLimit,
    SegmentLimit,
    Corrupt,
    IdentityMismatch,
    CertificateInvalid,
    Unsupported,
    DescriptorLimit { required: u64, available: u64 },
    Io(Arc<io::Error>),
    CommitOutcomeUnknown { generation: u64 },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "index I/O: {e}"),
            _ => write!(f, "{self:?}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(e) = self {
            Some(e.as_ref())
        } else {
            None
        }
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(Arc::new(e))
    }
}
impl From<crate::container::ContainerError> for Error {
    fn from(e: crate::container::ContainerError) -> Self {
        match e {
            crate::container::ContainerError::Version(_) => Self::Unsupported,
            _ => Self::Corrupt,
        }
    }
}
impl From<spherra_format::FormatError> for Error {
    fn from(_: spherra_format::FormatError) -> Self {
        Self::Corrupt
    }
}
impl From<spherra_codec::CertificateError> for Error {
    fn from(_: spherra_codec::CertificateError) -> Self {
        Self::CertificateInvalid
    }
}
pub(crate) fn lock_error(e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::WouldBlock => Error::IndexBusy,
        io::ErrorKind::NotFound => Error::NotFound,
        _ => e.into(),
    }
}
