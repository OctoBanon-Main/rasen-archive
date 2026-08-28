use crate::{
    codec::checksum,
    error::{Error, Result},
    path::normalize_path,
};

use super::{
    CHUNK_FLAG_LZ4, Chunk, HEADER_SIZE, MAX_PATH_LEN, TOC_CHUNK_FIXED_SIZE, TOC_ENTRY_FIXED_SIZE,
    TOC_MAGIC, TocEntry,
    io::Cursor,
};

pub(crate) fn encode_toc(entries: &[TocEntry], chunks: &[Chunk]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&TOC_MAGIC);

    for entry in entries {
        let path = entry.path.as_bytes();
        if path.len() > MAX_PATH_LEN {
            return Err(Error::TooLarge("path"));
        }
        out.extend_from_slice(&entry.path_hash.to_le_bytes());
        out.extend_from_slice(&entry.original_size.to_le_bytes());
        out.extend_from_slice(&entry.stored_size.to_le_bytes());
        out.extend_from_slice(&entry.first_chunk.to_le_bytes());
        out.extend_from_slice(&entry.chunk_count.to_le_bytes());
        out.extend_from_slice(&(path.len() as u16).to_le_bytes());
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
) -> Result<(Vec<TocEntry>, Vec<Chunk>)> {
    let mut cursor = Cursor::new(data);
    if cursor.take(4)? != TOC_MAGIC {
        return Err(Error::BadTocMagic);
    }

    let entries_count = usize::try_from(entry_count).map_err(|_| Error::TooLarge("entry count"))?;
    let chunks_count = usize::try_from(chunk_count).map_err(|_| Error::TooLarge("chunk count"))?;

    let min_bytes = entries_count
        .checked_mul(TOC_ENTRY_FIXED_SIZE)
        .and_then(|v| v.checked_add(chunks_count.checked_mul(TOC_CHUNK_FIXED_SIZE)?))
        .and_then(|v| v.checked_add(4))
        .ok_or(Error::TooLarge("TOC record counts"))?;
    if min_bytes > data.len() {
        return Err(Error::Corrupt("TOC counts exceed TOC size"));
    }

    let mut entries = Vec::with_capacity(entries_count);
    for _ in 0..entries_count {
        let path_hash = cursor.u64()?;
        let original_size = cursor.u64()?;
        let stored_size = cursor.u64()?;
        let first_chunk = cursor.u32()?;
        let chunk_count = cursor.u32()?;
        let path_len = cursor.u16()? as usize;
        let reserved = cursor.u16()?;
        if reserved != 0 {
            return Err(Error::Corrupt("non-zero entry reserved field"));
        }

        let path_bytes = cursor.take(path_len)?;
        let path = if paths_stripped {
            if path_len != 0 {
                return Err(Error::Corrupt("production TOC contains an entry path"));
            }
            String::new()
        } else {
            if path_len == 0 {
                return Err(Error::Corrupt("debug TOC contains an empty entry path"));
            }
            let path = std::str::from_utf8(path_bytes)
                .map_err(|_| Error::Corrupt("entry path is not UTF-8"))?;
            let path = normalize_path(path)?;
            if checksum(path.as_bytes()) != path_hash {
                return Err(Error::Corrupt("entry path hash mismatch"));
            }
            path
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

    let mut chunks = Vec::with_capacity(chunks_count);
    for _ in 0..chunks_count {
        let offset = cursor.u64()?;
        let stored_size = cursor.u64()?;
        let original_size = cursor.u64()?;
        let checksum = cursor.u64()?;
        let flags = cursor.u16()?;
        let reserved = cursor.u16()?;
        if flags & !CHUNK_FLAG_LZ4 != 0 {
            return Err(Error::Corrupt("unknown chunk flags"));
        }
        if reserved != 0 {
            return Err(Error::Corrupt("non-zero chunk reserved field"));
        }

        chunks.push(Chunk {
            offset,
            stored_size,
            original_size,
            checksum,
            compressed: flags & CHUNK_FLAG_LZ4 != 0,
        });
    }

    if !cursor.is_empty() {
        return Err(Error::Corrupt("trailing bytes in TOC"));
    }

    Ok((entries, chunks))
}

pub(crate) fn validate_layout(
    entries: &[TocEntry],
    chunks: &[Chunk],
    toc_offset: u64,
    chunk_size: u32,
    alignment: u32,
) -> Result<()> {
    let mut used_chunks = vec![false; chunks.len()];
    let mut previous_end = HEADER_SIZE;

    for chunk in chunks {
        if chunk.original_size == 0 || chunk.original_size > u64::from(chunk_size) {
            return Err(Error::Corrupt("invalid chunk original size"));
        }
        if chunk.stored_size == 0 || chunk.stored_size > chunk.original_size {
            return Err(Error::Corrupt("invalid stored chunk size"));
        }
        if chunk.compressed && chunk.stored_size >= chunk.original_size {
            return Err(Error::Corrupt("non-beneficial compressed chunk"));
        }
        if !chunk.compressed && chunk.stored_size != chunk.original_size {
            return Err(Error::Corrupt("raw chunk size mismatch"));
        }
        if chunk.offset < HEADER_SIZE || chunk.offset % u64::from(alignment) != 0 {
            return Err(Error::Corrupt("invalid chunk alignment"));
        }
        if chunk.offset < previous_end {
            return Err(Error::Corrupt("overlapping or out-of-order chunks"));
        }

        let end = chunk
            .offset
            .checked_add(chunk.stored_size)
            .ok_or(Error::Corrupt("chunk range overflow"))?;
        if end > toc_offset {
            return Err(Error::Corrupt("chunk outside payload region"));
        }
        previous_end = end;
    }

    for entry in entries {
        let first = usize::try_from(entry.first_chunk).map_err(|_| Error::TooLarge("chunk index"))?;
        let count = usize::try_from(entry.chunk_count).map_err(|_| Error::TooLarge("chunk count"))?;
        let end = first
            .checked_add(count)
            .ok_or(Error::Corrupt("entry chunk range overflow"))?;
        let owned = chunks
            .get(first..end)
            .ok_or(Error::Corrupt("entry chunk range outside TOC"))?;

        if entry.original_size == 0 {
            if entry.chunk_count != 0 || entry.stored_size != 0 {
                return Err(Error::Corrupt("empty entry has chunks"));
            }
        } else if entry.chunk_count == 0 {
            return Err(Error::Corrupt("non-empty entry has no chunks"));
        }

        let mut raw_total = 0u64;
        let mut stored_total = 0u64;
        for (local, chunk) in owned.iter().enumerate() {
            let global = first + local;
            if used_chunks[global] {
                return Err(Error::Corrupt("chunk is referenced by multiple entries"));
            }
            used_chunks[global] = true;

            raw_total = raw_total
                .checked_add(chunk.original_size)
                .ok_or(Error::Corrupt("entry raw size overflow"))?;
            stored_total = stored_total
                .checked_add(chunk.stored_size)
                .ok_or(Error::Corrupt("entry stored size overflow"))?;

            if local + 1 != owned.len() && chunk.original_size != u64::from(chunk_size) {
                return Err(Error::Corrupt("non-final chunk has unexpected size"));
            }
        }

        if raw_total != entry.original_size {
            return Err(Error::Corrupt("entry original size mismatch"));
        }
        if stored_total != entry.stored_size {
            return Err(Error::Corrupt("entry stored size mismatch"));
        }
    }

    if used_chunks.iter().any(|used| !used) {
        return Err(Error::Corrupt("orphan chunk in TOC"));
    }
    Ok(())
}