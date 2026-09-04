use std::{
    cell::RefCell,
    ffi::{CString, c_char},
    io,
    panic::{AssertUnwindSafe, catch_unwind},
};

use rasen_archive::Error;

use crate::types::{ABI_VERSION, RasenStatus};

pub(crate) struct FfiFailure {
    status: RasenStatus,
    message: String,
}

pub(crate) type FfiResult<T> = Result<T, FfiFailure>;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

pub(crate) fn failure(status: RasenStatus, message: impl Into<String>) -> FfiFailure {
    FfiFailure {
        status,
        message: message.into(),
    }
}

fn core_status(error: &Error) -> RasenStatus {
    match error {
        Error::Io(_) => RasenStatus::Io,
        Error::BadMagic => RasenStatus::BadMagic,
        Error::BadTocMagic => RasenStatus::BadTocMagic,
        Error::UnsupportedVersion(_) => RasenStatus::UnsupportedVersion,
        Error::UnsupportedFlags(_) => RasenStatus::UnsupportedFlags,
        Error::UnsupportedHeaderSize(_) => RasenStatus::UnsupportedHeaderSize,
        Error::EmptyXorKey => RasenStatus::EmptyKey,
        Error::Crypto(_) => RasenStatus::Crypto,
        Error::IncompletePack => RasenStatus::IncompletePack,
        Error::NonEmptyDestination => RasenStatus::NonEmptyDestination,
        Error::InvalidPath => RasenStatus::InvalidPath,
        Error::InvalidChunkSize => RasenStatus::InvalidChunkSize,
        Error::InvalidAlignment => RasenStatus::InvalidAlignment,
        Error::InvalidRange => RasenStatus::InvalidRange,
        Error::BufferSizeMismatch { .. } => RasenStatus::BufferSizeMismatch,
        Error::ChunkOutOfRange { .. } => RasenStatus::ChunkOutOfRange,
        Error::DuplicatePath(_) => RasenStatus::DuplicatePath,
        Error::AssetTooLarge => RasenStatus::AssetTooLarge,
        Error::ArchiveTooLarge => RasenStatus::ArchiveTooLarge,
        Error::TooManyEntries => RasenStatus::TooManyEntries,
        Error::TooManyChunks => RasenStatus::TooManyChunks,
        Error::MetadataLimitExceeded => RasenStatus::MetadataLimitExceeded,
        Error::Corrupt(_) => RasenStatus::Corrupt,
        Error::TooLarge(_) => RasenStatus::TooLarge,
        Error::Lz4(_) => RasenStatus::Lz4,
        Error::NotFound(_) => RasenStatus::NotFound,
        Error::HashCollision(_) => RasenStatus::HashCollision,
        Error::ChecksumMismatch { .. } => RasenStatus::ChecksumMismatch,
    }
}

impl From<Error> for FfiFailure {
    fn from(error: Error) -> Self {
        Self {
            status: core_status(&error),
            message: error.to_string(),
        }
    }
}

impl From<io::Error> for FfiFailure {
    fn from(error: io::Error) -> Self {
        Error::from(error).into()
    }
}

fn set_last_error(message: &str) {
    let sanitized = message.replace('\0', "\\0");
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(sanitized).unwrap_or_default();
    });
}

pub(crate) fn ffi_call(call: impl FnOnce() -> FfiResult<()>) -> RasenStatus {
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(Ok(())) => RasenStatus::Ok,
        Ok(Err(error)) => {
            set_last_error(&error.message);
            error.status
        }
        Err(_) => {
            set_last_error("panic caught at FFI boundary");
            RasenStatus::Panic
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rasen_abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn rasen_version_string() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

#[unsafe(no_mangle)]
pub extern "C" fn rasen_last_error_message() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}
