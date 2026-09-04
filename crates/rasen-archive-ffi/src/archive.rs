use std::{fs::File, path::Path, sync::Arc};

use rasen_archive::{
    Archive, ArchiveLimits, ArchiveScratch, AssetId, Error, Protection, hash_path, normalize_path,
};

use crate::error::{FfiResult, failure, ffi_call};
use crate::io::{CallbackSource, FfiSource, bytes, bytes_mut, into_buffer, required_mut, utf8};
use crate::types::{
    PROTECTION_AEAD, PROTECTION_XOR, RasenArchiveInfo, RasenArchiveLimits, RasenBuffer, RasenEntry,
    RasenSource, RasenStatus, ffi_entry, limits,
};

pub struct RasenArchive {
    inner: Archive<FfiSource>,
}

pub struct RasenScratch {
    inner: ArchiveScratch,
}

unsafe fn archive_ref<'a>(archive: *const RasenArchive) -> FfiResult<&'a RasenArchive> {
    unsafe { crate::io::required_ref(archive, "archive") }
}

unsafe fn scratch_mut<'a>(scratch: *mut RasenScratch) -> FfiResult<&'a mut RasenScratch> {
    unsafe { required_mut(scratch, "scratch") }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_hash_path(
    path: *const u8,
    path_len: usize,
    out_hash: *mut u64,
) -> RasenStatus {
    ffi_call(|| {
        let path = unsafe { utf8(path, path_len, "path")? };
        *unsafe { required_mut(out_hash, "out_hash")? } = hash_path(path)?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_normalize_path(
    path: *const u8,
    path_len: usize,
    out: *mut RasenBuffer,
) -> RasenStatus {
    ffi_call(|| {
        let out = unsafe { required_mut(out, "out")? };
        let normalized = normalize_path(unsafe { utf8(path, path_len, "path")? })?;
        *out = into_buffer(normalized.into_bytes());
        Ok(())
    })
}

fn open_archive(
    source: FfiSource,
    key: &[u8],
    limits: ArchiveLimits,
) -> FfiResult<*mut RasenArchive> {
    let inner = Archive::open_with_limits(source, key, limits)?;
    Ok(Box::into_raw(Box::new(RasenArchive { inner })))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_open_memory(
    data: *const u8,
    data_len: usize,
    key: *const u8,
    key_len: usize,
    limits_ptr: *const RasenArchiveLimits,
    out: *mut *mut RasenArchive,
) -> RasenStatus {
    ffi_call(|| {
        let out = unsafe { required_mut(out, "out")? };
        let source: Arc<[u8]> = unsafe { bytes(data, data_len, "data")? }.into();
        let key = unsafe { bytes(key, key_len, "key")? };
        *out = open_archive(FfiSource::Memory(source), key, limits(limits_ptr))?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_open_file(
    path: *const u8,
    path_len: usize,
    key: *const u8,
    key_len: usize,
    limits_ptr: *const RasenArchiveLimits,
    out: *mut *mut RasenArchive,
) -> RasenStatus {
    ffi_call(|| {
        let out = unsafe { required_mut(out, "out")? };
        let path = unsafe { utf8(path, path_len, "path")? };
        let key = unsafe { bytes(key, key_len, "key")? };
        let file = File::open(Path::new(path))?;
        *out = open_archive(FfiSource::File(file), key, limits(limits_ptr))?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_open_source(
    source: RasenSource,
    key: *const u8,
    key_len: usize,
    limits_ptr: *const RasenArchiveLimits,
    out: *mut *mut RasenArchive,
) -> RasenStatus {
    let source = CallbackSource {
        user_data: source.user_data as usize,
        len: source.len,
        read_at: source.read_at,
        destroy: source.destroy,
    };
    ffi_call(move || {
        let out = unsafe { required_mut(out, "out")? };
        let key = unsafe { bytes(key, key_len, "key")? };
        *out = open_archive(FfiSource::Callback(source), key, limits(limits_ptr))?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_free(archive: *mut RasenArchive) {
    if !archive.is_null() {
        drop(unsafe { Box::from_raw(archive) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_info(
    archive: *const RasenArchive,
    out: *mut RasenArchiveInfo,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        *unsafe { required_mut(out, "out")? } = RasenArchiveInfo {
            entry_count: archive.inner.entries().len(),
            chunk_size: archive.inner.chunk_size(),
            alignment: archive.inner.alignment(),
            paths_stripped: u8::from(archive.inner.paths_stripped()),
            protection: match archive.inner.protection() {
                Protection::Xor => PROTECTION_XOR,
                Protection::Aead => PROTECTION_AEAD,
            },
        };
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_entry_at(
    archive: *const RasenArchive,
    index: usize,
    out: *mut RasenEntry,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let entry = archive.inner.entries().get(index).ok_or_else(|| {
            failure(
                RasenStatus::OutOfRange,
                format!("entry index out of range: {index}"),
            )
        })?;
        *unsafe { required_mut(out, "out")? } = ffi_entry(entry);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_entry(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    out: *mut RasenEntry,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        *unsafe { required_mut(out, "out")? } = ffi_entry(archive.inner.entry(path)?);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_entry_by_id(
    archive: *const RasenArchive,
    asset_id: u64,
    out: *mut RasenEntry,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        *unsafe { required_mut(out, "out")? } =
            ffi_entry(archive.inner.entry_by_id(AssetId::from_raw(asset_id))?);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_verify(archive: *const RasenArchive) -> RasenStatus {
    ffi_call(|| {
        unsafe { archive_ref(archive)? }.inner.verify()?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_contains(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    out_contains: *mut u8,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        *unsafe { required_mut(out_contains, "out_contains")? } =
            u8::from(archive.inner.try_contains(path)?);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_contains_id(
    archive: *const RasenArchive,
    asset_id: u64,
    out_contains: *mut u8,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        *unsafe { required_mut(out_contains, "out_contains")? } =
            u8::from(archive.inner.contains_id(AssetId::from_raw(asset_id)));
        Ok(())
    })
}

unsafe fn read_buffer(
    archive: *const RasenArchive,
    out: *mut RasenBuffer,
    read: impl FnOnce(&Archive<FfiSource>) -> Result<Vec<u8>, Error>,
) -> FfiResult<()> {
    let archive = unsafe { archive_ref(archive)? };
    let out = unsafe { required_mut(out, "out")? };
    let value = read(&archive.inner)?;
    *out = into_buffer(value);
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    out: *mut RasenBuffer,
) -> RasenStatus {
    ffi_call(|| {
        let path = unsafe { utf8(path, path_len, "path")? };
        unsafe { read_buffer(archive, out, |archive| archive.read(path)) }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_by_id(
    archive: *const RasenArchive,
    asset_id: u64,
    out: *mut RasenBuffer,
) -> RasenStatus {
    ffi_call(|| unsafe {
        read_buffer(archive, out, |archive| {
            archive.read_by_id(AssetId::from_raw(asset_id))
        })
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_into(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    dst: *mut u8,
    dst_len: usize,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        archive
            .inner
            .read_into(path, unsafe { bytes_mut(dst, dst_len, "dst")? })?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_chunk(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    chunk_index: u32,
    out: *mut RasenBuffer,
) -> RasenStatus {
    ffi_call(|| {
        let path = unsafe { utf8(path, path_len, "path")? };
        unsafe {
            read_buffer(archive, out, |archive| {
                archive.read_chunk(path, chunk_index)
            })
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_chunk_into(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    chunk_index: u32,
    dst: *mut u8,
    dst_len: usize,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        archive.inner.read_chunk_into(path, chunk_index, unsafe {
            bytes_mut(dst, dst_len, "dst")?
        })?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_range(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    offset: u64,
    len: usize,
    out: *mut RasenBuffer,
) -> RasenStatus {
    ffi_call(|| {
        let path = unsafe { utf8(path, path_len, "path")? };
        unsafe {
            read_buffer(archive, out, |archive| {
                archive.read_range(path, offset, len)
            })
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_range_exact(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    offset: u64,
    len: usize,
    out: *mut RasenBuffer,
) -> RasenStatus {
    ffi_call(|| {
        let path = unsafe { utf8(path, path_len, "path")? };
        unsafe {
            read_buffer(archive, out, |archive| {
                archive.read_range_exact(path, offset, len)
            })
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_range_into(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    offset: u64,
    dst: *mut u8,
    dst_len: usize,
    out_read: *mut usize,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        *unsafe { required_mut(out_read, "out_read")? } =
            archive
                .inner
                .read_range_into(path, offset, unsafe { bytes_mut(dst, dst_len, "dst")? })?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_scratch_new(out: *mut *mut RasenScratch) -> RasenStatus {
    ffi_call(|| {
        *unsafe { required_mut(out, "out")? } = Box::into_raw(Box::new(RasenScratch {
            inner: ArchiveScratch::default(),
        }));
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_scratch_free(scratch: *mut RasenScratch) {
    if !scratch.is_null() {
        drop(unsafe { Box::from_raw(scratch) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_into_with_scratch(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    dst: *mut u8,
    dst_len: usize,
    scratch: *mut RasenScratch,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        archive.inner.read_into_with_scratch(
            path,
            unsafe { bytes_mut(dst, dst_len, "dst")? },
            &mut unsafe { scratch_mut(scratch)? }.inner,
        )?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_chunk_into_with_scratch(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    chunk_index: u32,
    dst: *mut u8,
    dst_len: usize,
    scratch: *mut RasenScratch,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        archive.inner.read_chunk_into_with_scratch(
            path,
            chunk_index,
            unsafe { bytes_mut(dst, dst_len, "dst")? },
            &mut unsafe { scratch_mut(scratch)? }.inner,
        )?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_archive_read_range_with_scratch(
    archive: *const RasenArchive,
    path: *const u8,
    path_len: usize,
    offset: u64,
    dst: *mut u8,
    dst_len: usize,
    scratch: *mut RasenScratch,
    out_read: *mut usize,
) -> RasenStatus {
    ffi_call(|| {
        let archive = unsafe { archive_ref(archive)? };
        let path = unsafe { utf8(path, path_len, "path")? };
        *unsafe { required_mut(out_read, "out_read")? } = archive.inner.read_range_with_scratch(
            path,
            offset,
            unsafe { bytes_mut(dst, dst_len, "dst")? },
            &mut unsafe { scratch_mut(scratch)? }.inner,
        )?;
        Ok(())
    })
}
