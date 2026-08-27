use std::{
    collections::HashSet,
    io::{Seek, SeekFrom, Write},
};

use lz4_flex::block::compress;
use xxhash_rust::xxh3::xxh3_64;

use crate::{
    error::{Error, Result},
    format::{
        HEADER_SIZE, MAX_TOC_RAW_SIZE, MAX_TOC_STORED_SIZE, REQUIRED_HEADER_FLAGS, VERSION,
        Header, write_header,
    },
    path::normalize_path,
    toc::encode_toc,
    types::{Chunk, Entry, InputFile, PackOptions},
    util::{align_writer, usize_to_u32, usize_to_u64, xor_in_place},
};

pub fn pack<W: Write + Seek>(writer: &mut W, files: &[InputFile], xor_key: &[u8]) -> Result<()> {
    pack_with_options(writer, files, xor_key, PackOptions::default())
}

pub fn pack_with_options<W: Write + Seek>(
    writer: &mut W,
    files: &[InputFile],
    xor_key: &[u8],
    options: PackOptions,
) -> Result<()> {
    if xor_key.is_empty() {
        return Err(Error::EmptyXorKey);
    }
    let options = options.validate()?;

    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(&[0u8; HEADER_SIZE as usize])?;

    let mut entries = Vec::with_capacity(files.len());
    let mut chunks = Vec::new();
    let mut seen = HashSet::<String>::with_capacity(files.len());

    for file in files {
        let path = normalize_path(&file.path)?;
        if !seen.insert(path.clone()) {
            return Err(Error::DuplicatePath(path));
        }

        let first_chunk_index = chunks.len();
        let first_chunk = usize_to_u32(first_chunk_index, "chunk index")?;
        let mut stored_total = 0u64;

        for raw in file.data.chunks(options.chunk_size) {
            align_writer(writer, options.alignment)?;
            let offset = writer.stream_position()?;
            let compressed = compress(raw);
            let (stored, is_compressed) = if compressed.len() < raw.len() {
                (compressed.as_slice(), true)
            } else {
                (raw, false)
            };

            writer.write_all(stored)?;

            let stored_size = usize_to_u64(stored.len(), "stored chunk size")?;
            stored_total = stored_total
                .checked_add(stored_size)
                .ok_or(Error::TooLarge("entry stored size"))?;

            chunks.push(Chunk {
                offset,
                stored_size,
                original_size: usize_to_u64(raw.len(), "original chunk size")?,
                checksum: xxh3_64(raw),
                compressed: is_compressed,
            });
        }

        let chunk_count = usize_to_u32(chunks.len() - first_chunk_index, "entry chunk count")?;
        let path_hash = xxh3_64(path.as_bytes());

        entries.push(Entry {
            path,
            path_hash,
            original_size: usize_to_u64(file.data.len(), "original entry size")?,
            stored_size: stored_total,
            first_chunk,
            chunk_count,
        });
    }

    align_writer(writer, options.alignment)?;
    let toc_offset = writer.stream_position()?;
    let toc_plain = encode_toc(&entries, &chunks)?;
    let toc_raw_size = usize_to_u64(toc_plain.len(), "raw TOC size")?;
    if toc_raw_size > MAX_TOC_RAW_SIZE {
        return Err(Error::TooLarge("raw TOC"));
    }
    let toc_hash = xxh3_64(&toc_plain);

    let mut toc_stored = compress(&toc_plain);
    xor_in_place(&mut toc_stored, xor_key);
    let toc_size = usize_to_u64(toc_stored.len(), "stored TOC size")?;
    if toc_size > MAX_TOC_STORED_SIZE {
        return Err(Error::TooLarge("stored TOC"));
    }
    writer.write_all(&toc_stored)?;

    let header = Header {
        version: VERSION,
        flags: REQUIRED_HEADER_FLAGS,
        header_size: HEADER_SIZE as u32,
        alignment: options.alignment,
        chunk_size: usize_to_u32(options.chunk_size, "chunk size")?,
        entry_count: usize_to_u32(entries.len(), "entry count")?,
        chunk_count: usize_to_u32(chunks.len(), "chunk count")?,
        toc_offset,
        toc_size,
        toc_raw_size,
        toc_hash,
    };

    writer.seek(SeekFrom::Start(0))?;
    write_header(writer, header)?;
    Ok(())
}
