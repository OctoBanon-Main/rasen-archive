use crate::{
    codec::checksum,
    error::{Error, Result},
    path::normalize_path,
};

use super::{
    CHUNK_FLAG_LZ4, Chunk, HEADER_SIZE, MAX_PATH_LEN, MAX_TOC_RAW_SIZE, TOC_CHUNK_FIXED_SIZE,
    TOC_ENTRY_FIXED_SIZE, TOC_MAGIC, TocEntry, io::Cursor,
};

pub(crate) fn encode_toc(entries: &[TocEntry], chunks: &[Chunk]) -> Result<Vec<u8>> {
    let mut size = 4usize;
    for entry in entries {
        (entry.path.len() <= MAX_PATH_LEN)
            .then_some(())
            .ok_or(Error::TooLarge("path"))?;
        size = size
            .checked_add(TOC_ENTRY_FIXED_SIZE)
            .and_then(|value| value.checked_add(entry.path.len()))
            .ok_or(Error::TooLarge("raw TOC"))?;
    }
    size = size
        .checked_add(
            chunks
                .len()
                .checked_mul(TOC_CHUNK_FIXED_SIZE)
                .ok_or(Error::TooLarge("raw TOC"))?,
        )
        .ok_or(Error::TooLarge("raw TOC"))?;
    (u64::try_from(size).map_err(|_| Error::TooLarge("raw TOC"))? <= MAX_TOC_RAW_SIZE)
        .then_some(())
        .ok_or(Error::TooLarge("raw TOC"))?;
    let mut out = Vec::new();
    out.try_reserve_exact(size)
        .map_err(|_| Error::TooLarge("raw TOC allocation"))?;
    out.extend_from_slice(&TOC_MAGIC);

    for entry in entries {
        let path = entry.path.as_bytes();
        out.extend_from_slice(&entry.path_hash.to_le_bytes());
        out.extend_from_slice(&entry.original_size.to_le_bytes());
        out.extend_from_slice(&entry.stored_size.to_le_bytes());
        out.extend_from_slice(&entry.first_chunk.to_le_bytes());
        out.extend_from_slice(&entry.chunk_count.to_le_bytes());
        out.extend_from_slice(
            &u16::try_from(path.len())
                .map_err(|_| Error::TooLarge("path"))?
                .to_le_bytes(),
        );
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(path);
    }

    for chunk in chunks {
        out.extend_from_slice(&chunk.offset.to_le_bytes());
        out.extend_from_slice(&chunk.stored_size.to_le_bytes());
        out.extend_from_slice(&chunk.original_size.to_le_bytes());
        out.extend_from_slice(&chunk.checksum.to_le_bytes());
        let flags = if chunk.compressed { CHUNK_FLAG_LZ4 } else { 0 };
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }

    Ok(out)
}

pub(crate) fn decode_toc(
    data: &[u8],
    entry_count: u32,
    chunk_count: u32,
    paths_stripped: bool,
    max_path_bytes: usize,
    max_total_path_bytes: u64,
    max_single_asset_bytes: u64,
) -> Result<(Vec<TocEntry>, Vec<Chunk>)> {
    let mut cursor = Cursor::new(data);
    (cursor.take(4)? == TOC_MAGIC)
        .then_some(())
        .ok_or(Error::BadTocMagic)?;

    let entries_count = usize::try_from(entry_count).map_err(|_| Error::TooLarge("entry count"))?;
    let chunks_count = usize::try_from(chunk_count).map_err(|_| Error::TooLarge("chunk count"))?;

    let min_bytes = entries_count
        .checked_mul(TOC_ENTRY_FIXED_SIZE)
        .and_then(|v| v.checked_add(chunks_count.checked_mul(TOC_CHUNK_FIXED_SIZE)?))
        .and_then(|v| v.checked_add(4))
        .ok_or(Error::TooLarge("TOC record counts"))?;
    (min_bytes <= data.len())
        .then_some(())
        .ok_or(Error::Corrupt("TOC counts exceed TOC size"))?;

    let mut entries = Vec::new();
    entries
        .try_reserve_exact(entries_count)
        .map_err(|_| Error::TooLarge("entry allocation"))?;
    let mut total_path_bytes = 0u64;
    for _ in 0..entries_count {
        let path_hash = cursor.u64()?;
        let original_size = cursor.u64()?;
        let stored_size = cursor.u64()?;
        let first_chunk = cursor.u32()?;
        let chunk_count = cursor.u32()?;
        let path_len = usize::from(cursor.u16()?);
        let reserved = cursor.u16()?;
        (original_size <= max_single_asset_bytes)
            .then_some(())
            .ok_or(Error::AssetTooLarge)?;
        (reserved == 0)
            .then_some(())
            .ok_or(Error::Corrupt("non-zero entry reserved field"))?;

        (path_len <= max_path_bytes)
            .then_some(())
            .ok_or(Error::TooLarge("path length limit"))?;
        total_path_bytes = total_path_bytes
            .checked_add(u64::try_from(path_len).map_err(|_| Error::TooLarge("path length"))?)
            .ok_or(Error::TooLarge("total path bytes"))?;
        (total_path_bytes <= max_total_path_bytes)
            .then_some(())
            .ok_or(Error::MetadataLimitExceeded)?;

        let path_bytes = cursor.take(path_len)?;
        let path = match (paths_stripped, path_len) {
            (true, 0) => String::new(),
            (true, _) => return Err(Error::Corrupt("production TOC contains an entry path")),
            (false, 0) => return Err(Error::Corrupt("debug TOC contains an empty entry path")),
            (false, _) => {
                let path = std::str::from_utf8(path_bytes)
                    .map_err(|_| Error::Corrupt("entry path is not UTF-8"))?;
                let path = normalize_path(path)?;
                (checksum(path.as_bytes()) == path_hash)
                    .then_some(())
                    .ok_or(Error::Corrupt("entry path hash mismatch"))?;
                path
            }
        };

        entries.push(TocEntry {
            path,
            path_hash,
            original_size,
            stored_size,
            first_chunk,
            chunk_count,
        });
    }

    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(chunks_count)
        .map_err(|_| Error::TooLarge("chunk allocation"))?;
    for _ in 0..chunks_count {
        let offset = cursor.u64()?;
        let stored_size = cursor.u64()?;
        let original_size = cursor.u64()?;
        let checksum = cursor.u64()?;
        let flags = cursor.u16()?;
        let reserved = cursor.u16()?;
        (flags & !CHUNK_FLAG_LZ4 == 0)
            .then_some(())
            .ok_or(Error::Corrupt("unknown chunk flags"))?;
        (reserved == 0)
            .then_some(())
            .ok_or(Error::Corrupt("non-zero chunk reserved field"))?;

        chunks.push(Chunk {
            offset,
            stored_size,
            original_size,
            checksum,
            compressed: flags & CHUNK_FLAG_LZ4 != 0,
        });
    }

    cursor
        .is_empty()
        .then_some(())
        .ok_or(Error::Corrupt("trailing bytes in TOC"))?;

    Ok((entries, chunks))
}

pub(crate) fn validate_layout(
    entries: &[TocEntry],
    chunks: &[Chunk],
    toc_offset: u64,
    chunk_size: u32,
    alignment: u32,
    max_total_decompressed_bytes: u64,
) -> Result<()> {
    let mut previous_end = HEADER_SIZE;
    let mut total_decompressed = 0u64;

    for chunk in chunks {
        (chunk.original_size != 0 && chunk.original_size <= u64::from(chunk_size))
            .then_some(())
            .ok_or(Error::Corrupt("invalid chunk original size"))?;
        (chunk.stored_size != 0 && chunk.stored_size <= chunk.original_size)
            .then_some(())
            .ok_or(Error::Corrupt("invalid stored chunk size"))?;
        (!chunk.compressed || chunk.stored_size < chunk.original_size)
            .then_some(())
            .ok_or(Error::Corrupt("non-beneficial compressed chunk"))?;
        (chunk.compressed || chunk.stored_size == chunk.original_size)
            .then_some(())
            .ok_or(Error::Corrupt("raw chunk size mismatch"))?;
        (chunk.offset >= HEADER_SIZE && chunk.offset.is_multiple_of(u64::from(alignment)))
            .then_some(())
            .ok_or(Error::Corrupt("invalid chunk alignment"))?;
        (chunk.offset >= previous_end)
            .then_some(())
            .ok_or(Error::Corrupt("overlapping or out-of-order chunks"))?;

        let end = chunk
            .offset
            .checked_add(chunk.stored_size)
            .ok_or(Error::Corrupt("chunk range overflow"))?;
        (end <= toc_offset)
            .then_some(())
            .ok_or(Error::Corrupt("chunk outside payload region"))?;
        total_decompressed = total_decompressed
            .checked_add(chunk.original_size)
            .ok_or(Error::TooLarge("total decompressed size"))?;
        (total_decompressed <= max_total_decompressed_bytes)
            .then_some(())
            .ok_or(Error::ArchiveTooLarge)?;
        previous_end = end;
    }

    let mut used_chunks = Vec::new();
    used_chunks
        .try_reserve_exact(chunks.len())
        .map_err(|_| Error::TooLarge("chunk ownership allocation"))?;
    used_chunks.resize(chunks.len(), false);

    for entry in entries {
        let first =
            usize::try_from(entry.first_chunk).map_err(|_| Error::TooLarge("chunk index"))?;
        let count =
            usize::try_from(entry.chunk_count).map_err(|_| Error::TooLarge("chunk count"))?;
        let end = first
            .checked_add(count)
            .ok_or(Error::Corrupt("entry chunk range overflow"))?;
        let owned = chunks
            .get(first..end)
            .ok_or(Error::Corrupt("entry chunk range outside TOC"))?;

        match (entry.original_size, entry.chunk_count, entry.stored_size) {
            (0, 0, 0) | (1.., 1.., _) => {}
            (0, _, _) => return Err(Error::Corrupt("empty entry has chunks")),
            (_, 0, _) => return Err(Error::Corrupt("non-empty entry has no chunks")),
        }

        let mut raw_total = 0u64;
        let mut stored_total = 0u64;
        for (local, chunk) in owned.iter().enumerate() {
            let global = first + local;
            (!used_chunks[global])
                .then_some(())
                .ok_or(Error::Corrupt("chunk is referenced by multiple entries"))?;
            used_chunks[global] = true;

            raw_total = raw_total
                .checked_add(chunk.original_size)
                .ok_or(Error::Corrupt("entry raw size overflow"))?;
            stored_total = stored_total
                .checked_add(chunk.stored_size)
                .ok_or(Error::Corrupt("entry stored size overflow"))?;

            (local + 1 == owned.len() || chunk.original_size == u64::from(chunk_size))
                .then_some(())
                .ok_or(Error::Corrupt("non-final chunk has unexpected size"))?;
        }

        (raw_total == entry.original_size)
            .then_some(())
            .ok_or(Error::Corrupt("entry original size mismatch"))?;
        (stored_total == entry.stored_size)
            .then_some(())
            .ok_or(Error::Corrupt("entry stored size mismatch"))?;
    }

    used_chunks
        .iter()
        .all(|used| *used)
        .then_some(())
        .ok_or(Error::Corrupt("orphan chunk in TOC"))?;
    Ok(())
}
