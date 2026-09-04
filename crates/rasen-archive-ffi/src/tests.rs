use std::{
    ffi::c_void,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    ptr, slice,
};

use rasen_archive::PackOptions;

use crate::*;

fn input(path: &[u8], data: &[u8]) -> RasenInputFile {
    RasenInputFile {
        path: path.as_ptr(),
        path_len: path.len(),
        data: data.as_ptr(),
        data_len: data.len(),
    }
}

#[test]
fn ffi_buffered_roundtrip_covers_archive_api() {
    unsafe {
        let path = b"textures\\hero.bin";
        let normalized = b"textures/hero.bin";
        let data: Vec<u8> = (0..100).collect();
        let files = [input(path, &data)];
        let mut options = RasenPackOptions {
            chunk_size: 16,
            ..PackOptions::default().into()
        };
        options.alignment = 64;
        let mut packed = RasenBuffer::default();
        let mut summary = RasenPackSummary::default();
        assert_eq!(
            rasen_pack_memory(
                files.as_ptr(),
                files.len(),
                b"key".as_ptr(),
                3,
                &options,
                &mut packed,
                &mut summary,
            ),
            RasenStatus::Ok
        );
        assert_eq!(summary.entry_count, 1);
        assert_eq!(summary.chunk_count, 7);

        let mut archive = ptr::null_mut();
        assert_eq!(
            rasen_archive_open_memory(
                packed.data,
                packed.len,
                b"key".as_ptr(),
                3,
                ptr::null(),
                &mut archive,
            ),
            RasenStatus::Ok
        );
        rasen_buffer_free(&mut packed);

        let mut info = RasenArchiveInfo {
            entry_count: 0,
            chunk_size: 0,
            alignment: 0,
            paths_stripped: 0,
            protection: 0,
        };
        assert_eq!(rasen_archive_info(archive, &mut info), RasenStatus::Ok);
        assert_eq!(info.entry_count, 1);
        assert_eq!(info.chunk_size, 16);
        assert_eq!(info.alignment, 64);

        let mut entry = RasenEntry {
            path: ptr::null(),
            path_len: 0,
            path_hash: 0,
            original_size: 0,
            stored_size: 0,
            first_chunk: 0,
            chunk_count: 0,
        };
        assert_eq!(
            rasen_archive_entry_at(archive, 0, &mut entry),
            RasenStatus::Ok
        );
        assert_eq!(
            slice::from_raw_parts(entry.path, entry.path_len),
            normalized
        );
        assert_eq!(entry.original_size, 100);

        let mut contains = 0;
        assert_eq!(
            rasen_archive_contains(
                archive,
                normalized.as_ptr(),
                normalized.len(),
                &mut contains,
            ),
            RasenStatus::Ok
        );
        assert_eq!(contains, 1);

        let mut read = RasenBuffer::default();
        assert_eq!(
            rasen_archive_read(archive, normalized.as_ptr(), normalized.len(), &mut read,),
            RasenStatus::Ok
        );
        assert_eq!(slice::from_raw_parts(read.data, read.len), data);
        rasen_buffer_free(&mut read);

        let mut range = [0u8; 20];
        let mut range_len = 0;
        let mut scratch = ptr::null_mut();
        assert_eq!(rasen_scratch_new(&mut scratch), RasenStatus::Ok);
        assert_eq!(
            rasen_archive_read_range_with_scratch(
                archive,
                normalized.as_ptr(),
                normalized.len(),
                90,
                range.as_mut_ptr(),
                range.len(),
                scratch,
                &mut range_len,
            ),
            RasenStatus::Ok
        );
        assert_eq!(&range[..range_len], &data[90..]);

        assert_eq!(
            rasen_archive_read(archive, b"../bad".as_ptr(), 6, &mut read,),
            RasenStatus::InvalidPath
        );
        assert!(!rasen_last_error_message().is_null());

        rasen_scratch_free(scratch);
        rasen_archive_free(archive);
    }
}

struct StreamState {
    input: Cursor<Vec<u8>>,
    output: Cursor<Vec<u8>>,
}

unsafe extern "C" fn stream_read(
    user_data: *mut c_void,
    dst: *mut u8,
    len: usize,
    out_read: *mut usize,
) -> i32 {
    let state = unsafe { &mut *(user_data as *mut StreamState) };
    match state
        .input
        .read(unsafe { slice::from_raw_parts_mut(dst, len) })
    {
        Ok(amount) => {
            unsafe { *out_read = amount };
            0
        }
        Err(_) => -1,
    }
}

unsafe extern "C" fn stream_write(user_data: *mut c_void, src: *const u8, len: usize) -> i32 {
    let state = unsafe { &mut *(user_data as *mut StreamState) };
    state
        .output
        .write_all(unsafe { slice::from_raw_parts(src, len) })
        .map_or(-1, |()| 0)
}

unsafe extern "C" fn stream_seek(user_data: *mut c_void, position: u64) -> i32 {
    let state = unsafe { &mut *(user_data as *mut StreamState) };
    state
        .output
        .seek(SeekFrom::Start(position))
        .map_or(-1, |_| 0)
}

unsafe extern "C" fn stream_len(user_data: *mut c_void, out: *mut u64) -> i32 {
    let state = unsafe { &mut *(user_data as *mut StreamState) };
    unsafe { *out = state.output.get_ref().len() as u64 };
    0
}

#[test]
fn ffi_streaming_pack_uses_callbacks() {
    unsafe {
        let data = vec![7; 100_000];
        let mut state = StreamState {
            input: Cursor::new(data.clone()),
            output: Cursor::new(Vec::new()),
        };
        let user_data = (&mut state as *mut StreamState).cast();
        let inputs = [RasenStreamInput {
            path: b"asset.bin".as_ptr(),
            path_len: 9,
            user_data,
            read: Some(stream_read),
        }];
        let writer = RasenWriter {
            user_data,
            write: Some(stream_write),
            seek: Some(stream_seek),
            len: Some(stream_len),
        };
        let mut summary = RasenPackSummary::default();
        assert_eq!(
            rasen_pack_streams(
                inputs.as_ptr(),
                inputs.len(),
                b"key".as_ptr(),
                3,
                ptr::null(),
                &writer,
                &mut summary,
            ),
            RasenStatus::Ok
        );
        assert_eq!(summary.entry_count, 1);

        let packed = state.output.into_inner();
        let mut archive = ptr::null_mut();
        assert_eq!(
            rasen_archive_open_memory(
                packed.as_ptr(),
                packed.len(),
                b"key".as_ptr(),
                3,
                ptr::null(),
                &mut archive,
            ),
            RasenStatus::Ok
        );
        let mut read = RasenBuffer::default();
        assert_eq!(
            rasen_archive_read(archive, b"asset.bin".as_ptr(), 9, &mut read),
            RasenStatus::Ok
        );
        assert_eq!(slice::from_raw_parts(read.data, read.len), data);
        rasen_buffer_free(&mut read);
        rasen_archive_free(archive);
    }
}
