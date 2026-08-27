use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom},
};

use lz4_flex::block::decompress;
use xxhash_rust::xxh3::xxh3_64;

use crate::{
    error::{Error, Result},
    format::{HEADER_SIZE, read_header, validate_header},
    path::normalize_lookup_path,
    toc::{decode_toc, validate_layout},
    types::{Chunk, Entry},
    util::{u64_to_usize, usize_to_u64, xor_in_place},
};

pub struct Archive<R> {
    reader: R,
    entries: Vec<Entry>,
    chunks: Vec<Chunk>,
    index: HashMap<u64, Vec<usize>>,
    chunk_size: u32,
    alignment: u32,
}

impl<R: Read + Seek> Archive<R> {
    pub fn open(mut reader: R, xor_key: &[u8]) -> Result<Self> {
        if xor_key.is_empty() {
            return Err(Error::EmptyXorKey);
        }

        reader.seek(SeekFrom::Start(0))?;
        let header = read_header(&mut reader)?;
        validate_header(header)?;

        let file_len = reader.seek(SeekFrom::End(0))?;
        let toc_end = header
            .toc_offset
            .checked_add(header.toc_size)
            .ok_or(Error::Corrupt("TOC range overflow"))?;
        if header.toc_offset < HEADER_SIZE || toc_end > file_len {
            return Err(Error::Corrupt("TOC outside archive"));
        }
        if header.toc_offset % u64::from(header.alignment) != 0 {
            return Err(Error::Corrupt("TOC is not aligned"));
        }

        reader.seek(SeekFrom::Start(header.toc_offset))?;
        let toc_len = u64_to_usize(header.toc_size, "TOC size")?;
        let mut toc_stored = vec![0u8; toc_len];
        reader.read_exact(&mut toc_stored)?;
        xor_in_place(&mut toc_stored, xor_key);

        let toc_raw_len = u64_to_usize(header.toc_raw_size, "raw TOC size")?;
        let toc_plain =
            decompress(&toc_stored, toc_raw_len).map_err(|e| Error::Lz4(e.to_string()))?;
        if toc_plain.len() != toc_raw_len {
            return Err(Error::Corrupt("decompressed TOC size mismatch"));
        }
        if xxh3_64(&toc_plain) != header.toc_hash {
            return Err(Error::Corrupt("TOC checksum mismatch"));
        }

        let (mut entries, chunks) =
            decode_toc(&toc_plain, header.entry_count, header.chunk_count)?;
        validate_layout(
            &mut entries,
            &chunks,
            header.toc_offset,
            header.chunk_size,
            header.alignment,
        )?;

        let mut index = HashMap::<u64, Vec<usize>>::with_capacity(entries.len());
        let mut seen = HashSet::<String>::with_capacity(entries.len());
        for (i, entry) in entries.iter().enumerate() {
            if !seen.insert(entry.path.clone()) {
                return Err(Error::DuplicatePath(entry.path.clone()));
            }
            index.entry(entry.path_hash).or_default().push(i);
        }

        Ok(Self {
            reader,
            entries,
            chunks,
            index,
            chunk_size: header.chunk_size,
            alignment: header.alignment,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    pub fn alignment(&self) -> u32 {
        self.alignment
    }

    pub fn contains(&self, path: &str) -> bool {
        self.find_entry_index(path).is_ok()
    }

    pub fn contains_hash(&self, path_hash: u64) -> bool {
        self.index.contains_key(&path_hash)
    }

    pub fn read_by_hash(&mut self, path_hash: u64) -> Result<Vec<u8>> {
        let idx = self.find_entry_index_by_hash(path_hash)?;
        let path = self.entries[idx].path.clone();
        self.read_entry_index(idx, &path)
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>> {
        let idx = self.find_entry_index(path)?;
        let normalized = self.entries[idx].path.clone();
        self.read_entry_index(idx, &normalized)
    }

    pub fn read_chunk(&mut self, path: &str, chunk_index: u32) -> Result<Vec<u8>> {
        let idx = self.find_entry_index(path)?;
        let entry = self.entries[idx].clone();
        if chunk_index >= entry.chunk_count {
            return Err(Error::InvalidRange);
        }
        let global_chunk = entry
            .first_chunk
            .checked_add(chunk_index)
            .ok_or(Error::Corrupt("chunk index overflow"))?;
        self.read_chunk_by_index(&entry.path, chunk_index, global_chunk)
    }

    pub fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let idx = self.find_entry_index(path)?;
        let entry = self.entries[idx].clone();

        if offset > entry.original_size {
            return Err(Error::InvalidRange);
        }
        if len == 0 || offset == entry.original_size {
            return Ok(Vec::new());
        }

        let requested_end = offset
            .checked_add(usize_to_u64(len, "range length")?)
            .ok_or(Error::InvalidRange)?;
        let end = requested_end.min(entry.original_size);
        let chunk_size = u64::from(self.chunk_size);
        let first_local = u32::try_from(offset / chunk_size)
            .map_err(|_| Error::TooLarge("range chunk index"))?;
        let last_local = u32::try_from((end - 1) / chunk_size)
            .map_err(|_| Error::TooLarge("range chunk index"))?;

        let actual_len = u64_to_usize(end - offset, "range result size")?;
        let mut out = Vec::new();
        out.try_reserve_exact(actual_len)
            .map_err(|_| Error::TooLarge("range allocation"))?;

        for local_chunk in first_local..=last_local {
            if local_chunk >= entry.chunk_count {
                return Err(Error::Corrupt("entry chunk range mismatch"));
            }
            let global_chunk = entry
                .first_chunk
                .checked_add(local_chunk)
                .ok_or(Error::Corrupt("chunk index overflow"))?;
            let bytes = self.read_chunk_by_index(&entry.path, local_chunk, global_chunk)?;

            let chunk_start = u64::from(local_chunk) * chunk_size;
            let chunk_end = chunk_start + usize_to_u64(bytes.len(), "chunk length")?;
            let take_start = offset.max(chunk_start) - chunk_start;
            let take_end = end.min(chunk_end) - chunk_start;
            let a = u64_to_usize(take_start, "range slice start")?;
            let b = u64_to_usize(take_end, "range slice end")?;
            out.extend_from_slice(&bytes[a..b]);
        }

        Ok(out)
    }

    fn find_entry_index(&self, path: &str) -> Result<usize> {
        let normalized = normalize_lookup_path(path)?;
        let hash = xxh3_64(normalized.as_bytes());
        let bucket = self
            .index
            .get(&hash)
            .ok_or_else(|| Error::NotFound(normalized.clone()))?;
        bucket
            .iter()
            .copied()
            .find(|&i| self.entries[i].path == normalized)
            .ok_or(Error::NotFound(normalized))
    }

    fn find_entry_index_by_hash(&self, path_hash: u64) -> Result<usize> {
        let bucket = self
            .index
            .get(&path_hash)
            .ok_or_else(|| Error::NotFound(format!("hash:{path_hash:016x}")))?;
        if bucket.len() != 1 {
            return Err(Error::HashCollision(path_hash));
        }
        Ok(bucket[0])
    }

    fn read_entry_index(&mut self, idx: usize, path: &str) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(idx)
            .cloned()
            .ok_or(Error::Corrupt("entry index outside TOC"))?;
        let capacity = u64_to_usize(entry.original_size, "original entry size")?;
        let mut out = Vec::new();
        out.try_reserve_exact(capacity)
            .map_err(|_| Error::TooLarge("entry allocation"))?;

        for local_chunk in 0..entry.chunk_count {
            let global_chunk = entry
                .first_chunk
                .checked_add(local_chunk)
                .ok_or(Error::Corrupt("chunk index overflow"))?;
            let bytes = self.read_chunk_by_index(path, local_chunk, global_chunk)?;
            out.extend_from_slice(&bytes);
        }

        if out.len() != capacity {
            return Err(Error::Corrupt("entry decompressed size mismatch"));
        }
        Ok(out)
    }

    fn read_chunk_by_index(
        &mut self,
        path: &str,
        local_chunk: u32,
        global_chunk: u32,
    ) -> Result<Vec<u8>> {
        let chunk = *self
            .chunks
            .get(usize::try_from(global_chunk).map_err(|_| Error::TooLarge("chunk index"))?)
            .ok_or(Error::Corrupt("chunk index outside TOC"))?;

        let stored_len = u64_to_usize(chunk.stored_size, "stored chunk size")?;
        self.reader.seek(SeekFrom::Start(chunk.offset))?;
        let mut stored = vec![0u8; stored_len];
        self.reader.read_exact(&mut stored)?;

        let original_len = u64_to_usize(chunk.original_size, "original chunk size")?;
        let data = if chunk.compressed {
            let data = decompress(&stored, original_len).map_err(|e| Error::Lz4(e.to_string()))?;
            if data.len() != original_len {
                return Err(Error::Corrupt("decompressed chunk size mismatch"));
            }
            data
        } else {
            if chunk.stored_size != chunk.original_size {
                return Err(Error::Corrupt("raw chunk size mismatch"));
            }
            stored
        };

        if xxh3_64(&data) != chunk.checksum {
            return Err(Error::ChecksumMismatch {
                path: path.to_owned(),
                chunk: local_chunk,
            });
        }
        Ok(data)
    }
}
