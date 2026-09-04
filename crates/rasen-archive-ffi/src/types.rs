use std::{ffi::c_void, ptr, str};

use rasen_archive::{ArchiveLimits, Entry, PackMode, PackOptions, PackSummary, Protection};

use crate::error::{FfiResult, failure, ffi_call};
use crate::io::required_mut;

pub(crate) const ABI_VERSION: u32 = 1;
pub(crate) const PACK_MODE_DEBUG: u32 = 0;
pub(crate) const PACK_MODE_PRODUCTION: u32 = 1;
pub(crate) const PROTECTION_XOR: u32 = 0;
pub(crate) const PROTECTION_AEAD: u32 = 1;

#[repr(i32)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RasenStatus {
    Ok = 0,
    NullPointer = 1,
    InvalidUtf8 = 2,
    InvalidValue = 3,
    OutOfRange = 4,
    CallbackFailed = 5,
    Panic = 6,
    Io = 100,
    BadMagic = 101,
    BadTocMagic = 102,
    UnsupportedVersion = 103,
    UnsupportedFlags = 104,
    UnsupportedHeaderSize = 105,
    EmptyKey = 106,
    Crypto = 107,
    IncompletePack = 108,
    NonEmptyDestination = 109,
    InvalidPath = 110,
    InvalidChunkSize = 111,
    InvalidAlignment = 112,
    InvalidRange = 113,
    BufferSizeMismatch = 114,
    ChunkOutOfRange = 115,
    DuplicatePath = 116,
    AssetTooLarge = 117,
    ArchiveTooLarge = 118,
    TooManyEntries = 119,
    TooManyChunks = 120,
    MetadataLimitExceeded = 121,
    Corrupt = 122,
    TooLarge = 123,
    Lz4 = 124,
    NotFound = 125,
    HashCollision = 126,
    ChecksumMismatch = 127,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RasenBuffer {
    pub data: *mut u8,
    pub len: usize,
}

impl Default for RasenBuffer {
    fn default() -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RasenArchiveLimits {
    pub max_toc_stored_bytes: u64,
    pub max_toc_raw_bytes: u64,
    pub max_entries: u32,
    pub max_chunks: u32,
    pub max_chunks_per_operation: u32,
    pub max_path_bytes: usize,
    pub max_total_path_bytes: u64,
    pub max_total_decompressed_bytes: u64,
    pub max_single_asset_bytes: u64,
    pub max_metadata_bytes: u64,
}

impl From<ArchiveLimits> for RasenArchiveLimits {
    fn from(value: ArchiveLimits) -> Self {
        Self {
            max_toc_stored_bytes: value.max_toc_stored_bytes,
            max_toc_raw_bytes: value.max_toc_raw_bytes,
            max_entries: value.max_entries,
            max_chunks: value.max_chunks,
            max_chunks_per_operation: value.max_chunks_per_operation,
            max_path_bytes: value.max_path_bytes,
            max_total_path_bytes: value.max_total_path_bytes,
            max_total_decompressed_bytes: value.max_total_decompressed_bytes,
            max_single_asset_bytes: value.max_single_asset_bytes,
            max_metadata_bytes: value.max_metadata_bytes,
        }
    }
}

impl From<RasenArchiveLimits> for ArchiveLimits {
    fn from(value: RasenArchiveLimits) -> Self {
        Self {
            max_toc_stored_bytes: value.max_toc_stored_bytes,
            max_toc_raw_bytes: value.max_toc_raw_bytes,
            max_entries: value.max_entries,
            max_chunks: value.max_chunks,
            max_chunks_per_operation: value.max_chunks_per_operation,
            max_path_bytes: value.max_path_bytes,
            max_total_path_bytes: value.max_total_path_bytes,
            max_total_decompressed_bytes: value.max_total_decompressed_bytes,
            max_single_asset_bytes: value.max_single_asset_bytes,
            max_metadata_bytes: value.max_metadata_bytes,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RasenArchiveInfo {
    pub entry_count: usize,
    pub chunk_size: u32,
    pub alignment: u32,
    pub paths_stripped: u8,
    pub protection: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RasenEntry {
    pub path: *const u8,
    pub path_len: usize,
    pub path_hash: u64,
    pub original_size: u64,
    pub stored_size: u64,
    pub first_chunk: u32,
    pub chunk_count: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RasenPackOptions {
    pub chunk_size: usize,
    pub alignment: u32,
    pub mode: u32,
    pub protection: u32,
}

impl From<PackOptions> for RasenPackOptions {
    fn from(value: PackOptions) -> Self {
        Self {
            chunk_size: value.chunk_size,
            alignment: value.alignment,
            mode: match value.mode {
                PackMode::Debug => PACK_MODE_DEBUG,
                PackMode::Production => PACK_MODE_PRODUCTION,
            },
            protection: match value.protection {
                Protection::Xor => PROTECTION_XOR,
                Protection::Aead => PROTECTION_AEAD,
            },
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct RasenPackSummary {
    pub archive_len: u64,
    pub entry_count: u32,
    pub chunk_count: u32,
}

impl From<PackSummary> for RasenPackSummary {
    fn from(value: PackSummary) -> Self {
        Self {
            archive_len: value.archive_len,
            entry_count: value.entry_count,
            chunk_count: value.chunk_count,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct RasenInputFile {
    pub path: *const u8,
    pub path_len: usize,
    pub data: *const u8,
    pub data_len: usize,
}

pub type RasenReadAtFn = unsafe extern "C" fn(*mut c_void, u64, *mut u8, usize) -> i32;
pub type RasenDestroyFn = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RasenSource {
    pub user_data: *mut c_void,
    pub len: u64,
    pub read_at: Option<RasenReadAtFn>,
    pub destroy: Option<RasenDestroyFn>,
}

pub type RasenReadFn = unsafe extern "C" fn(*mut c_void, *mut u8, usize, *mut usize) -> i32;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RasenStreamInput {
    pub path: *const u8,
    pub path_len: usize,
    pub user_data: *mut c_void,
    pub read: Option<RasenReadFn>,
}

pub type RasenWriteFn = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32;
pub type RasenSeekFn = unsafe extern "C" fn(*mut c_void, u64) -> i32;
pub type RasenLenFn = unsafe extern "C" fn(*mut c_void, *mut u64) -> i32;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RasenWriter {
    pub user_data: *mut c_void,
    pub write: Option<RasenWriteFn>,
    pub seek: Option<RasenSeekFn>,
    pub len: Option<RasenLenFn>,
}

pub(crate) fn pack_options(options: *const RasenPackOptions) -> FfiResult<PackOptions> {
    if options.is_null() {
        return Ok(PackOptions::default());
    }
    let options = unsafe { *options };
    let mode = match options.mode {
        PACK_MODE_DEBUG => PackMode::Debug,
        PACK_MODE_PRODUCTION => PackMode::Production,
        value => {
            return Err(failure(
                RasenStatus::InvalidValue,
                format!("invalid pack mode: {value}"),
            ));
        }
    };
    let protection = match options.protection {
        PROTECTION_XOR => Protection::Xor,
        PROTECTION_AEAD => Protection::Aead,
        value => {
            return Err(failure(
                RasenStatus::InvalidValue,
                format!("invalid protection: {value}"),
            ));
        }
    };
    Ok(PackOptions {
        chunk_size: options.chunk_size,
        alignment: options.alignment,
        mode,
        protection,
    })
}

pub(crate) fn limits(limits: *const RasenArchiveLimits) -> ArchiveLimits {
    if limits.is_null() {
        ArchiveLimits::default()
    } else {
        unsafe { (*limits).into() }
    }
}

pub(crate) fn ffi_entry(entry: &Entry) -> RasenEntry {
    let path = entry.path();
    RasenEntry {
        path: path.map_or(ptr::null(), |path| path.as_ptr()),
        path_len: path.map_or(0, str::len),
        path_hash: entry.path_hash,
        original_size: entry.original_size,
        stored_size: entry.stored_size,
        first_chunk: entry.first_chunk,
        chunk_count: entry.chunk_count,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_limits_runtime(out: *mut RasenArchiveLimits) -> RasenStatus {
    ffi_call(|| {
        *unsafe { required_mut(out, "out")? } = ArchiveLimits::runtime_default().into();
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_limits_tooling(out: *mut RasenArchiveLimits) -> RasenStatus {
    ffi_call(|| {
        *unsafe { required_mut(out, "out")? } = ArchiveLimits::tooling_default().into();
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_pack_options_default(out: *mut RasenPackOptions) -> RasenStatus {
    ffi_call(|| {
        *unsafe { required_mut(out, "out")? } = PackOptions::default().into();
        Ok(())
    })
}
