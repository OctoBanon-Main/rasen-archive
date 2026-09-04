use std::{
    fs::OpenOptions,
    io::{Cursor, Seek, Write},
    path::Path,
};

use rasen_archive::{Error, InputFile, PackOptions, PackSummary, Packer};

use crate::error::{FfiResult, ffi_call};
use crate::io::{
    CallbackReader, CallbackWriter, bytes, into_buffer, raw_slice, required_mut, required_ref, utf8,
};
use crate::types::{
    RasenBuffer, RasenInputFile, RasenPackOptions, RasenPackSummary, RasenStatus, RasenStreamInput,
    RasenWriter, pack_options,
};

unsafe fn buffered_inputs(files: *const RasenInputFile, count: usize) -> FfiResult<Vec<InputFile>> {
    let files = unsafe { raw_slice(files, count, "files")? };
    files
        .iter()
        .map(|file| {
            Ok(InputFile {
                path: unsafe { utf8(file.path, file.path_len, "file.path")? }.to_owned(),
                data: unsafe { bytes(file.data, file.data_len, "file.data")? }.to_vec(),
            })
        })
        .collect()
}

fn pack_buffered<W: Write + Seek>(
    writer: &mut W,
    files: &[InputFile],
    key: &[u8],
    options: PackOptions,
) -> Result<PackSummary, Error> {
    let mut packer = Packer::new(writer, key, options)?;
    for file in files {
        packer.add_reader(&file.path, &mut file.data.as_slice())?;
    }
    packer.finish()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_pack_memory(
    files: *const RasenInputFile,
    file_count: usize,
    key: *const u8,
    key_len: usize,
    options_ptr: *const RasenPackOptions,
    out_archive: *mut RasenBuffer,
    out_summary: *mut RasenPackSummary,
) -> RasenStatus {
    ffi_call(|| {
        let out_archive = unsafe { required_mut(out_archive, "out_archive")? };
        let files = unsafe { buffered_inputs(files, file_count)? };
        let key = unsafe { bytes(key, key_len, "key")? };
        let mut writer = Cursor::new(Vec::new());
        let summary = pack_buffered(&mut writer, &files, key, pack_options(options_ptr)?)?;
        *out_archive = into_buffer(writer.into_inner());
        if let Some(out_summary) = unsafe { out_summary.as_mut() } {
            *out_summary = summary.into();
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_pack_file(
    output_path: *const u8,
    output_path_len: usize,
    files: *const RasenInputFile,
    file_count: usize,
    key: *const u8,
    key_len: usize,
    options_ptr: *const RasenPackOptions,
    out_summary: *mut RasenPackSummary,
) -> RasenStatus {
    ffi_call(|| {
        let output_path = unsafe { utf8(output_path, output_path_len, "output_path")? };
        let files = unsafe { buffered_inputs(files, file_count)? };
        let key = unsafe { bytes(key, key_len, "key")? };
        let options = pack_options(options_ptr)?;
        let output_path = Path::new(output_path);
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output_path)?;
        let result = pack_buffered(&mut writer, &files, key, options);
        drop(writer);
        let summary = match result {
            Ok(summary) => summary,
            Err(error) => {
                let _ = std::fs::remove_file(output_path);
                return Err(error.into());
            }
        };
        if let Some(out_summary) = unsafe { out_summary.as_mut() } {
            *out_summary = summary.into();
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rasen_pack_streams(
    inputs: *const RasenStreamInput,
    input_count: usize,
    key: *const u8,
    key_len: usize,
    options_ptr: *const RasenPackOptions,
    writer: *const RasenWriter,
    out_summary: *mut RasenPackSummary,
) -> RasenStatus {
    ffi_call(|| {
        let inputs = unsafe { raw_slice(inputs, input_count, "inputs")? };
        let key = unsafe { bytes(key, key_len, "key")? };
        let callbacks = *unsafe { required_ref(writer, "writer")? };
        let mut writer = CallbackWriter {
            callbacks,
            position: 0,
        };
        let mut packer = Packer::new(&mut writer, key, pack_options(options_ptr)?)?;
        for input in inputs {
            let path = unsafe { utf8(input.path, input.path_len, "input.path")? };
            let mut reader = CallbackReader {
                user_data: input.user_data as usize,
                read: input.read,
            };
            packer.add_reader(path, &mut reader)?;
        }
        let summary = packer.finish()?;
        if let Some(out_summary) = unsafe { out_summary.as_mut() } {
            *out_summary = summary.into();
        }
        Ok(())
    })
}
