use std::{
    ffi::c_void,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    ptr, slice, str,
    sync::Arc,
};

use rasen_archive::RandomAccessRead;

use crate::error::{FfiResult, failure};
use crate::types::{
    RasenBuffer, RasenDestroyFn, RasenReadAtFn, RasenReadFn, RasenStatus, RasenWriter,
};

pub(crate) enum FfiSource {
    Memory(Arc<[u8]>),
    File(File),
    Callback(CallbackSource),
}

pub(crate) struct CallbackSource {
    pub(crate) user_data: usize,
    pub(crate) len: u64,
    pub(crate) read_at: Option<RasenReadAtFn>,
    pub(crate) destroy: Option<RasenDestroyFn>,
}

// Callback owner guarantees thread safety when sharing an archive between threads.
unsafe impl Send for CallbackSource {}
unsafe impl Sync for CallbackSource {}

impl Drop for CallbackSource {
    fn drop(&mut self) {
        if let Some(destroy) = self.destroy {
            unsafe { destroy(self.user_data as *mut c_void) };
        }
    }
}

impl RandomAccessRead for FfiSource {
    fn len(&self) -> io::Result<u64> {
        match self {
            Self::Memory(bytes) => u64::try_from(bytes.as_ref().len())
                .map_err(|_| io::Error::other("source length overflow")),
            Self::File(file) => Ok(file.metadata()?.len()),
            Self::Callback(source) => Ok(source.len),
        }
    }

    fn read_exact_at(&self, offset: u64, dst: &mut [u8]) -> io::Result<()> {
        match self {
            Self::Memory(bytes) => {
                let start = usize::try_from(offset).map_err(|_| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "read offset too large")
                })?;
                let end = start.checked_add(dst.len()).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "read range overflow")
                })?;
                let source = bytes.get(start..end).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "failed to fill whole buffer")
                })?;
                dst.copy_from_slice(source);
                Ok(())
            }
            Self::File(file) => RandomAccessRead::read_exact_at(file, offset, dst),
            Self::Callback(source) => {
                let read_at = source
                    .read_at
                    .ok_or_else(|| io::Error::other("read_at callback is null"))?;
                let status = unsafe {
                    read_at(
                        source.user_data as *mut c_void,
                        offset,
                        dst.as_mut_ptr(),
                        dst.len(),
                    )
                };
                (status == 0)
                    .then_some(())
                    .ok_or_else(|| io::Error::other(format!("read_at callback failed: {status}")))
            }
        }
    }
}

pub(crate) struct CallbackReader {
    pub(crate) user_data: usize,
    pub(crate) read: Option<RasenReadFn>,
}

impl Read for CallbackReader {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        let read = self
            .read
            .ok_or_else(|| io::Error::other("read callback is null"))?;
        let mut amount = 0usize;
        let status = unsafe {
            read(
                self.user_data as *mut c_void,
                dst.as_mut_ptr(),
                dst.len(),
                &mut amount,
            )
        };
        if status != 0 {
            return Err(io::Error::other(format!("read callback failed: {status}")));
        }
        (amount <= dst.len())
            .then_some(amount)
            .ok_or_else(|| io::Error::other("read callback returned an oversized count"))
    }
}

pub(crate) struct CallbackWriter {
    pub(crate) callbacks: RasenWriter,
    pub(crate) position: u64,
}

impl Write for CallbackWriter {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        let write = self
            .callbacks
            .write
            .ok_or_else(|| io::Error::other("write callback is null"))?;
        let status = unsafe { write(self.callbacks.user_data, src.as_ptr(), src.len()) };
        if status != 0 {
            return Err(io::Error::other(format!("write callback failed: {status}")));
        }
        let len = u64::try_from(src.len()).map_err(|_| io::Error::other("write size overflow"))?;
        self.position = self
            .position
            .checked_add(len)
            .ok_or_else(|| io::Error::other("writer position overflow"))?;
        Ok(src.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for CallbackWriter {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let absolute = match position {
            SeekFrom::Start(position) => position,
            SeekFrom::Current(0) => self.position,
            SeekFrom::End(0) => {
                let len = self
                    .callbacks
                    .len
                    .ok_or_else(|| io::Error::other("len callback is null"))?;
                let mut value = 0;
                let status = unsafe { len(self.callbacks.user_data, &mut value) };
                if status != 0 {
                    return Err(io::Error::other(format!("len callback failed: {status}")));
                }
                value
            }
            _ => return Err(io::Error::other("unsupported callback seek")),
        };
        let seek = self
            .callbacks
            .seek
            .ok_or_else(|| io::Error::other("seek callback is null"))?;
        let status = unsafe { seek(self.callbacks.user_data, absolute) };
        if status != 0 {
            return Err(io::Error::other(format!("seek callback failed: {status}")));
        }
        self.position = absolute;
        Ok(absolute)
    }
}

pub(crate) unsafe fn bytes<'a>(data: *const u8, len: usize, name: &str) -> FfiResult<&'a [u8]> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() {
        return Err(failure(RasenStatus::NullPointer, format!("{name} is null")));
    }
    Ok(unsafe { slice::from_raw_parts(data, len) })
}

pub(crate) unsafe fn bytes_mut<'a>(
    data: *mut u8,
    len: usize,
    name: &str,
) -> FfiResult<&'a mut [u8]> {
    if len == 0 {
        return Ok(&mut []);
    }
    if data.is_null() {
        return Err(failure(RasenStatus::NullPointer, format!("{name} is null")));
    }
    Ok(unsafe { slice::from_raw_parts_mut(data, len) })
}

pub(crate) unsafe fn utf8<'a>(data: *const u8, len: usize, name: &str) -> FfiResult<&'a str> {
    str::from_utf8(unsafe { bytes(data, len, name)? }).map_err(|_| {
        failure(
            RasenStatus::InvalidUtf8,
            format!("{name} is not valid UTF-8"),
        )
    })
}

pub(crate) unsafe fn required_ref<'a, T>(value: *const T, name: &str) -> FfiResult<&'a T> {
    unsafe { value.as_ref() }
        .ok_or_else(|| failure(RasenStatus::NullPointer, format!("{name} is null")))
}

pub(crate) unsafe fn required_mut<'a, T>(value: *mut T, name: &str) -> FfiResult<&'a mut T> {
    unsafe { value.as_mut() }
        .ok_or_else(|| failure(RasenStatus::NullPointer, format!("{name} is null")))
}

pub(crate) unsafe fn raw_slice<'a, T>(
    data: *const T,
    len: usize,
    name: &str,
) -> FfiResult<&'a [T]> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() {
        return Err(failure(RasenStatus::NullPointer, format!("{name} is null")));
    }
    Ok(unsafe { slice::from_raw_parts(data, len) })
}

pub(crate) fn into_buffer(bytes: Vec<u8>) -> RasenBuffer {
    if bytes.is_empty() {
        return RasenBuffer::default();
    }
    let mut bytes = bytes.into_boxed_slice();
    let buffer = RasenBuffer {
        data: bytes.as_mut_ptr(),
        len: bytes.len(),
    };
    Box::leak(bytes);
    buffer
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_buffer_free(buffer: *mut RasenBuffer) {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else {
        return;
    };
    if !buffer.data.is_null() {
        let raw = ptr::slice_from_raw_parts_mut(buffer.data, buffer.len);
        drop(unsafe { Box::from_raw(raw) });
    }
    *buffer = RasenBuffer::default();
}
