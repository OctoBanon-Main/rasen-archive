use std::{cmp::Ordering, collections::HashSet, io::Cursor};

use crate::{
    compression::{decompress_block_exact, decompress_into_exact},
    crypto::{TOC_AAD, chunk_aad, decrypt, derive_key, xor_in_place},
    error::{Error, Result},
    format::{
        Chunk, HEADER_FLAG_AEAD, HEADER_FLAG_PATHS_STRIPPED, HEADER_SIZE, MAX_PATH_LEN,
        MAX_TOC_RAW_SIZE, MAX_TOC_STORED_SIZE, TOC_CHUNK_FIXED_SIZE, TOC_ENTRY_FIXED_SIZE,
        TocEntry, decode_toc,
        io::{u64_to_usize, usize_to_u64},
        read_header, validate_header, validate_layout,
    },
    hash::checksum,
    pack::Protection,
    path::{AssetId, normalize_lookup_path},
};

use super::source::RandomAccessRead;

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub path_hash: u64,
    pub original_size: u64,
    pub stored_size: u64,
    pub first_chunk: u32,
    pub chunk_count: u32,
}

impl Entry {
    pub fn path(&self) -> Option<&str> {
        (!self.path.is_empty()).then_some(self.path.as_str())
    }
}

impl From<TocEntry> for Entry {
    fn from(entry: TocEntry) -> Self {
        Self {
            path: entry.path,
            path_hash: entry.path_hash,
            original_size: entry.original_size,
            stored_size: entry.stored_size,
            first_chunk: entry.first_chunk,
            chunk_count: entry.chunk_count,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ArchiveLimits {
    /// Maximum stored TOC bytes accepted from this caller.
    pub max_toc_stored_bytes: u64,
    /// Maximum decoded TOC bytes accepted from this caller.
    pub max_toc_raw_bytes: u64,
    /// Maximum number of archive entries accepted from this caller.
    pub max_entries: u32,
    /// Maximum number of chunks accepted from this caller.
    pub max_chunks: u32,
    /// Maximum chunks decoded by one read or verification operation.
    pub max_chunks_per_operation: u32,
    /// Maximum bytes in one encoded archive path.
    pub max_path_bytes: usize,
    /// Maximum combined bytes in all encoded archive paths.
    pub max_total_path_bytes: u64,
    /// Maximum combined original size of all archive assets.
    pub max_total_decompressed_bytes: u64,
    /// Maximum original size of one archive asset.
    pub max_single_asset_bytes: u64,
    /// Maximum estimated memory used by decoded archive metadata and indexes.
    pub max_metadata_bytes: u64,
}

impl ArchiveLimits {
    /// Hard maximum stored TOC size. Caller limits cannot raise it.
    pub const HARD_MAX_TOC_STORED_BYTES: u64 = MAX_TOC_STORED_SIZE;
    /// Hard maximum decoded TOC size. Caller limits cannot raise it.
    pub const HARD_MAX_TOC_RAW_BYTES: u64 = MAX_TOC_RAW_SIZE;
    /// Hard maximum archive entry count. Caller limits cannot raise it.
    pub const HARD_MAX_ENTRIES: u32 = 1_000_000;
    /// Hard maximum archive chunk count. Caller limits cannot raise it.
    pub const HARD_MAX_CHUNKS: u32 = 8_000_000;
    /// Hard maximum chunks decoded by one operation. Caller limits cannot raise it.
    pub const HARD_MAX_CHUNKS_PER_OPERATION: u32 = Self::HARD_MAX_CHUNKS;
    /// Hard maximum encoded path size. Caller limits cannot raise it.
    pub const HARD_MAX_PATH_BYTES: usize = MAX_PATH_LEN;
    /// Hard maximum combined encoded path size. Caller limits cannot raise it.
    pub const HARD_MAX_TOTAL_PATH_BYTES: u64 = 64 * 1024 * 1024;
    /// Hard maximum combined decompressed asset size. Caller limits cannot raise it.
    pub const HARD_MAX_TOTAL_DECOMPRESSED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
    /// Hard maximum size of one decompressed asset. Caller limits cannot raise it.
    pub const HARD_MAX_SINGLE_ASSET_BYTES: u64 = Self::HARD_MAX_TOTAL_DECOMPRESSED_BYTES;
    /// Hard maximum estimated metadata memory. Caller limits cannot raise it.
    pub const HARD_MAX_METADATA_BYTES: u64 = 2 * 1024 * 1024 * 1024;
    /// Hard maximum entries sharing one identifier.
    pub const HARD_MAX_IDENTIFIER_BUCKET_ENTRIES: usize = 64;

    pub const fn runtime_default() -> Self {
        Self {
            max_toc_stored_bytes: 64 * 1024 * 1024,
            max_toc_raw_bytes: 128 * 1024 * 1024,
            max_entries: 100_000,
            max_chunks: 500_000,
            max_chunks_per_operation: 65_536,
            max_path_bytes: Self::HARD_MAX_PATH_BYTES,
            max_total_path_bytes: 64 * 1024 * 1024,
            max_total_decompressed_bytes: 4 * 1024 * 1024 * 1024,
            max_single_asset_bytes: 512 * 1024 * 1024,
            max_metadata_bytes: 512 * 1024 * 1024,
        }
    }

    pub const fn tooling_default() -> Self {
        Self {
            max_toc_stored_bytes: Self::HARD_MAX_TOC_STORED_BYTES,
            max_toc_raw_bytes: Self::HARD_MAX_TOC_RAW_BYTES,
            max_entries: Self::HARD_MAX_ENTRIES,
            max_chunks: Self::HARD_MAX_CHUNKS,
            max_chunks_per_operation: Self::HARD_MAX_CHUNKS_PER_OPERATION,
            max_path_bytes: Self::HARD_MAX_PATH_BYTES,
            max_total_path_bytes: Self::HARD_MAX_TOTAL_PATH_BYTES,
            max_total_decompressed_bytes: Self::HARD_MAX_TOTAL_DECOMPRESSED_BYTES,
            max_single_asset_bytes: Self::HARD_MAX_SINGLE_ASSET_BYTES,
            max_metadata_bytes: Self::HARD_MAX_METADATA_BYTES,
        }
    }

    pub const fn permissive_v1() -> Self {
        Self::tooling_default()
    }

    fn clamped(self) -> Self {
        Self {
            max_toc_stored_bytes: self
                .max_toc_stored_bytes
                .min(Self::HARD_MAX_TOC_STORED_BYTES),
            max_toc_raw_bytes: self.max_toc_raw_bytes.min(Self::HARD_MAX_TOC_RAW_BYTES),
            max_entries: self.max_entries.min(Self::HARD_MAX_ENTRIES),
            max_chunks: self.max_chunks.min(Self::HARD_MAX_CHUNKS),
            max_chunks_per_operation: self
                .max_chunks_per_operation
                .min(Self::HARD_MAX_CHUNKS_PER_OPERATION),
            max_path_bytes: self.max_path_bytes.min(Self::HARD_MAX_PATH_BYTES),
            max_total_path_bytes: self
                .max_total_path_bytes
                .min(Self::HARD_MAX_TOTAL_PATH_BYTES),
            max_total_decompressed_bytes: self
                .max_total_decompressed_bytes
                .min(Self::HARD_MAX_TOTAL_DECOMPRESSED_BYTES),
            max_single_asset_bytes: self
                .max_single_asset_bytes
                .min(Self::HARD_MAX_SINGLE_ASSET_BYTES),
            max_metadata_bytes: self.max_metadata_bytes.min(Self::HARD_MAX_METADATA_BYTES),
        }
    }
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self::runtime_default()
    }
}

#[derive(Debug, Default)]
pub struct ArchiveScratch {
    stored: Vec<u8>,
    decoded: Vec<u8>,
}

pub struct Archive<R> {
    source: R,
    entries: Vec<Entry>,
    chunks: Vec<Chunk>,
    index: Vec<(AssetId, usize)>,
    chunk_size: u32,
    alignment: u32,
    paths_stripped: bool,
    protection: Protection,
    aead_key: Option<[u8; 32]>,
    max_chunks_per_operation: u32,
}

#[derive(Copy, Clone)]
enum ChecksumLabel<'a> {
    Path(&'a str),
    Hash(AssetId),
}

impl ChecksumLabel<'_> {
    fn to_error_string(self) -> String {
        match self {
            Self::Path(path) => path.to_owned(),
            Self::Hash(id) => format!("hash:{:016x}", id.get()),
        }
    }
}

impl<R: RandomAccessRead> Archive<R> {
    pub fn open(source: R, key: &[u8]) -> Result<Self> {
        Self::open_with_limits(source, key, ArchiveLimits::default())
    }

    pub fn open_with_limits(source: R, key: &[u8], limits: ArchiveLimits) -> Result<Self> {
        (!key.is_empty()).then_some(()).ok_or(Error::EmptyXorKey)?;

        let limits = limits.clamped();
        let mut header_bytes = [0u8; HEADER_SIZE as usize];
        source.read_exact_at(0, &mut header_bytes)?;
        let header = read_header(&mut Cursor::new(header_bytes))?;
        validate_header(header)?;
        validate_limits(header, limits)?;
        let protection = match header.flags & HEADER_FLAG_AEAD != 0 {
            true => Protection::Aead,
            false => Protection::Xor,
        };
        let aead_key = (protection == Protection::Aead).then(|| derive_key(key));

        let file_len = source.len()?;
        let toc_end = header
            .toc_offset
            .checked_add(header.toc_size)
            .ok_or(Error::Corrupt("TOC range overflow"))?;
        (header.toc_offset >= HEADER_SIZE && toc_end <= file_len)
            .then_some(())
            .ok_or(Error::Corrupt("TOC outside archive"))?;
        header
            .toc_offset
            .is_multiple_of(u64::from(header.alignment))
            .then_some(())
            .ok_or(Error::Corrupt("TOC is not aligned"))?;

        let toc_len = u64_to_usize(header.toc_size, "TOC size")?;
        let mut toc_stored = Vec::new();
        toc_stored
            .try_reserve_exact(toc_len)
            .map_err(|_| Error::TooLarge("stored TOC allocation"))?;
        toc_stored.resize(toc_len, 0);
        source.read_exact_at(header.toc_offset, &mut toc_stored)?;
        match &aead_key {
            Some(aead_key) => toc_stored = decrypt(&toc_stored, aead_key, TOC_AAD)?,
            None => xor_in_place(&mut toc_stored, key),
        }

        let toc_raw_len = u64_to_usize(header.toc_raw_size, "raw TOC size")?;
        let toc_plain =
            decompress_block_exact(&toc_stored, toc_raw_len, "decompressed TOC size mismatch")?;
        (checksum(&toc_plain) == header.toc_hash)
            .then_some(())
            .ok_or(Error::Corrupt("TOC checksum mismatch"))?;

        let paths_stripped = header.flags & HEADER_FLAG_PATHS_STRIPPED != 0;
        let (toc_entries, chunks) = decode_toc(
            &toc_plain,
            header.entry_count,
            header.chunk_count,
            paths_stripped,
            limits.max_path_bytes,
            limits.max_total_path_bytes,
            limits.max_single_asset_bytes,
        )?;
        validate_layout(
            &toc_entries,
            &chunks,
            header.toc_offset,
            header.chunk_size,
            header.alignment,
            protection == Protection::Aead,
            limits.max_total_decompressed_bytes,
        )?;

        let mut entries = Vec::new();
        entries
            .try_reserve_exact(toc_entries.len())
            .map_err(|_| Error::TooLarge("entry allocation"))?;
        entries.extend(toc_entries.into_iter().map(Entry::from));
        let mut index = Vec::new();
        index
            .try_reserve_exact(entries.len())
            .map_err(|_| Error::TooLarge("index allocation"))?;
        index.extend(
            entries
                .iter()
                .enumerate()
                .map(|(i, entry)| (AssetId::from_raw(entry.path_hash), i)),
        );
        index.sort_unstable_by_key(|&(id, _)| id);
        validate_index(&entries, &index, paths_stripped)?;

        Ok(Self {
            source,
            entries,
            chunks,
            index,
            chunk_size: header.chunk_size,
            alignment: header.alignment,
            paths_stripped,
            protection,
            aead_key,
            max_chunks_per_operation: limits.max_chunks_per_operation,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, path: &str) -> Result<&Entry> {
        let normalized = normalize_lookup_path(path)?;
        Ok(&self.entries[self.find_entry_index_normalized(&normalized)?])
    }

    pub fn entry_by_id(&self, asset_id: AssetId) -> Result<&Entry> {
        Ok(&self.entries[self.find_entry_index_by_id(asset_id)?])
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    pub fn alignment(&self) -> u32 {
        self.alignment
    }

    pub fn paths_stripped(&self) -> bool {
        self.paths_stripped
    }

    pub fn protection(&self) -> Protection {
        self.protection
    }

    pub fn verify(&self) -> Result<()> {
        self.ensure_chunk_operation_limit(
            u32::try_from(self.chunks.len()).map_err(|_| Error::TooManyChunks)?,
        )?;
        let mut scratch = ArchiveScratch::default();
        for entry in &self.entries {
            let label = match self.paths_stripped {
                true => ChecksumLabel::Hash(AssetId::from_raw(entry.path_hash)),
                false => ChecksumLabel::Path(&entry.path),
            };
            for local_chunk in 0..entry.chunk_count {
                let global_chunk = entry
                    .first_chunk
                    .checked_add(local_chunk)
                    .ok_or(Error::Corrupt("chunk index overflow"))?;
                let len = u64_to_usize(
                    self.chunk(global_chunk)?.original_size,
                    "original chunk size",
                )?;
                resize_buffer(&mut scratch.decoded, len, "decoded chunk allocation")?;
                self.read_chunk_by_index_into(
                    label,
                    local_chunk,
                    global_chunk,
                    &mut scratch.decoded,
                    &mut scratch.stored,
                )?;
            }
        }
        Ok(())
    }

    /// Returns `false` for both missing entries and malformed paths.
    ///
    /// Use [`Self::try_contains`] when malformed paths must be reported.
    pub fn contains(&self, path: &str) -> bool {
        self.try_contains(path).unwrap_or(false)
    }

    /// Distinguishes malformed paths from valid paths that are not present.
    pub fn try_contains(&self, path: &str) -> Result<bool> {
        let normalized = normalize_lookup_path(path)?;
        match self.find_entry_index_normalized(&normalized) {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn contains_hash(&self, path_hash: u64) -> bool {
        self.contains_id(AssetId::from_raw(path_hash))
    }

    pub fn contains_id(&self, asset_id: AssetId) -> bool {
        self.hash_range(asset_id).is_some()
    }

    pub fn read_by_hash(&self, path_hash: u64) -> Result<Vec<u8>> {
        self.read_by_id(AssetId::from_raw(path_hash))
    }

    pub fn read_by_id(&self, asset_id: AssetId) -> Result<Vec<u8>> {
        let idx = self.find_entry_index_by_id(asset_id)?;
        self.ensure_chunk_operation_limit(self.entries[idx].chunk_count)?;
        let size = u64_to_usize(self.entries[idx].original_size, "original entry size")?;
        let mut out = allocate(size, "entry allocation")?;
        let label = match self.paths_stripped {
            true => ChecksumLabel::Hash(asset_id),
            false => ChecksumLabel::Path(&self.entries[idx].path),
        };
        self.read_entry_into(idx, label, &mut out, &mut ArchiveScratch::default())?;
        Ok(out)
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        self.ensure_chunk_operation_limit(self.entries[idx].chunk_count)?;
        let size = u64_to_usize(self.entries[idx].original_size, "original entry size")?;
        let mut out = allocate(size, "entry allocation")?;
        self.read_entry_into(
            idx,
            ChecksumLabel::Path(&normalized),
            &mut out,
            &mut ArchiveScratch::default(),
        )?;
        Ok(out)
    }

    pub fn read_into(&self, path: &str, dst: &mut [u8]) -> Result<()> {
        self.read_into_with_scratch(path, dst, &mut ArchiveScratch::default())
    }

    pub fn read_into_with_scratch(
        &self,
        path: &str,
        dst: &mut [u8],
        scratch: &mut ArchiveScratch,
    ) -> Result<()> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        self.read_entry_into(idx, ChecksumLabel::Path(&normalized), dst, scratch)
    }

    pub fn read_chunk(&self, path: &str, chunk_index: u32) -> Result<Vec<u8>> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        let entry = &self.entries[idx];
        if chunk_index >= entry.chunk_count {
            return Err(Error::ChunkOutOfRange {
                chunk: chunk_index,
                count: entry.chunk_count,
            });
        }
        let global_chunk = entry
            .first_chunk
            .checked_add(chunk_index)
            .ok_or(Error::Corrupt("chunk index overflow"))?;
        self.ensure_chunk_operation_limit(1)?;
        let chunk = self.chunk(global_chunk)?;
        let mut out = allocate(
            u64_to_usize(chunk.original_size, "original chunk size")?,
            "chunk allocation",
        )?;
        self.read_chunk_by_index_into(
            ChecksumLabel::Path(&normalized),
            chunk_index,
            global_chunk,
            &mut out,
            &mut ArchiveScratch::default().stored,
        )?;
        Ok(out)
    }

    pub fn read_chunk_into(&self, path: &str, chunk_index: u32, dst: &mut [u8]) -> Result<()> {
        self.read_chunk_into_with_scratch(path, chunk_index, dst, &mut ArchiveScratch::default())
    }

    pub fn read_chunk_into_with_scratch(
        &self,
        path: &str,
        chunk_index: u32,
        dst: &mut [u8],
        scratch: &mut ArchiveScratch,
    ) -> Result<()> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        let entry = &self.entries[idx];
        if chunk_index >= entry.chunk_count {
            return Err(Error::ChunkOutOfRange {
                chunk: chunk_index,
                count: entry.chunk_count,
            });
        }
        let global_chunk = entry
            .first_chunk
            .checked_add(chunk_index)
            .ok_or(Error::Corrupt("chunk index overflow"))?;
        self.read_chunk_by_index_into(
            ChecksumLabel::Path(&normalized),
            chunk_index,
            global_chunk,
            dst,
            &mut scratch.stored,
        )
    }

    pub fn read_range(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        let actual_len = self.range_len(idx, offset, len, false)?;
        let mut out = allocate(actual_len, "range allocation")?;
        self.read_range_index_into(
            idx,
            ChecksumLabel::Path(&normalized),
            offset,
            &mut out,
            &mut ArchiveScratch::default(),
        )?;
        Ok(out)
    }

    pub fn read_range_exact(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        let actual_len = self.range_len(idx, offset, len, true)?;
        let mut out = allocate(actual_len, "range allocation")?;
        self.read_range_index_into(
            idx,
            ChecksumLabel::Path(&normalized),
            offset,
            &mut out,
            &mut ArchiveScratch::default(),
        )?;
        Ok(out)
    }

    pub fn read_range_into(&self, path: &str, offset: u64, dst: &mut [u8]) -> Result<usize> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        let actual_len = self.range_len(idx, offset, dst.len(), false)?;
        self.read_range_index_into(
            idx,
            ChecksumLabel::Path(&normalized),
            offset,
            &mut dst[..actual_len],
            &mut ArchiveScratch::default(),
        )?;
        Ok(actual_len)
    }

    pub fn read_range_with_scratch(
        &self,
        path: &str,
        offset: u64,
        dst: &mut [u8],
        scratch: &mut ArchiveScratch,
    ) -> Result<usize> {
        let normalized = normalize_lookup_path(path)?;
        let idx = self.find_entry_index_normalized(&normalized)?;
        let actual_len = self.range_len(idx, offset, dst.len(), false)?;
        self.read_range_index_into(
            idx,
            ChecksumLabel::Path(&normalized),
            offset,
            &mut dst[..actual_len],
            scratch,
        )?;
        Ok(actual_len)
    }

    fn hash_range(&self, asset_id: AssetId) -> Option<std::ops::Range<usize>> {
        let start = self.index.partition_point(|&(id, _)| id < asset_id);
        let end = self.index.partition_point(|&(id, _)| id <= asset_id);
        (start != end).then_some(start..end)
    }

    fn find_entry_index_normalized(&self, normalized: &str) -> Result<usize> {
        let id = AssetId::from_raw(checksum(normalized.as_bytes()));
        let range = self
            .hash_range(id)
            .ok_or_else(|| Error::NotFound(normalized.to_owned()))?;
        if self.paths_stripped {
            return Ok(self.index[range.start].1);
        }
        self.index[range]
            .iter()
            .find_map(|&(_, i)| (self.entries[i].path == normalized).then_some(i))
            .ok_or_else(|| Error::NotFound(normalized.to_owned()))
    }

    fn find_entry_index_by_id(&self, asset_id: AssetId) -> Result<usize> {
        let range = self
            .hash_range(asset_id)
            .ok_or_else(|| Error::NotFound(format!("hash:{:016x}", asset_id.get())))?;
        if range.len() != 1 {
            return Err(Error::HashCollision(asset_id.get()));
        }
        Ok(self.index[range.start].1)
    }

    fn read_entry_into(
        &self,
        idx: usize,
        label: ChecksumLabel<'_>,
        dst: &mut [u8],
        scratch: &mut ArchiveScratch,
    ) -> Result<()> {
        let entry = self
            .entries
            .get(idx)
            .ok_or(Error::Corrupt("entry index outside TOC"))?;
        self.ensure_chunk_operation_limit(entry.chunk_count)?;
        let expected = u64_to_usize(entry.original_size, "original entry size")?;
        if dst.len() != expected {
            return Err(Error::BufferSizeMismatch {
                expected,
                actual: dst.len(),
            });
        }
        let mut written = 0usize;
        for local_chunk in 0..entry.chunk_count {
            let global_chunk = entry
                .first_chunk
                .checked_add(local_chunk)
                .ok_or(Error::Corrupt("chunk index overflow"))?;
            let len = u64_to_usize(
                self.chunk(global_chunk)?.original_size,
                "original chunk size",
            )?;
            let end = written
                .checked_add(len)
                .ok_or(Error::Corrupt("entry decompressed size overflow"))?;
            let chunk_dst = dst
                .get_mut(written..end)
                .ok_or(Error::Corrupt("entry decompressed size mismatch"))?;
            self.read_chunk_by_index_into(
                label,
                local_chunk,
                global_chunk,
                chunk_dst,
                &mut scratch.stored,
            )?;
            written = end;
        }
        if written != dst.len() {
            return Err(Error::Corrupt("entry decompressed size mismatch"));
        }
        Ok(())
    }

    fn range_len(&self, idx: usize, offset: u64, len: usize, exact: bool) -> Result<usize> {
        let size = self.entries[idx].original_size;
        match (offset.cmp(&size), len, exact) {
            (Ordering::Greater, _, _) | (Ordering::Equal, 1.., true) => {
                return Err(Error::InvalidRange);
            }
            (_, 0, _) | (Ordering::Equal, _, false) => return Ok(0),
            (Ordering::Less, _, _) => {}
        }
        let requested_end = offset
            .checked_add(usize_to_u64(len, "range length")?)
            .ok_or(Error::InvalidRange)?;
        (!exact || requested_end <= size)
            .then_some(())
            .ok_or(Error::InvalidRange)?;
        let actual_len = u64_to_usize(requested_end.min(size) - offset, "range result size")?;
        if actual_len != 0 {
            let end = offset
                .checked_add(usize_to_u64(actual_len, "range length")?)
                .ok_or(Error::InvalidRange)?;
            let chunk_size = u64::from(self.chunk_size);
            let first = offset / chunk_size;
            let last = (end - 1) / chunk_size;
            let count = last
                .checked_sub(first)
                .and_then(|value| value.checked_add(1))
                .and_then(|value| u32::try_from(value).ok())
                .ok_or(Error::TooManyChunks)?;
            self.ensure_chunk_operation_limit(count)?;
        }
        Ok(actual_len)
    }

    fn ensure_chunk_operation_limit(&self, count: u32) -> Result<()> {
        (count <= self.max_chunks_per_operation)
            .then_some(())
            .ok_or(Error::TooManyChunks)
    }

    fn read_range_index_into(
        &self,
        idx: usize,
        label: ChecksumLabel<'_>,
        offset: u64,
        dst: &mut [u8],
        scratch: &mut ArchiveScratch,
    ) -> Result<()> {
        let Some(()) = (!dst.is_empty()).then_some(()) else {
            return Ok(());
        };
        let entry = &self.entries[idx];
        let end = offset
            .checked_add(usize_to_u64(dst.len(), "range length")?)
            .ok_or(Error::InvalidRange)?;
        let chunk_size = u64::from(self.chunk_size);
        let first_local =
            u32::try_from(offset / chunk_size).map_err(|_| Error::TooLarge("range chunk index"))?;
        let last_local = u32::try_from((end - 1) / chunk_size)
            .map_err(|_| Error::TooLarge("range chunk index"))?;
        let mut written = 0usize;

        for local_chunk in first_local..=last_local {
            (local_chunk < entry.chunk_count)
                .then_some(())
                .ok_or(Error::Corrupt("entry chunk range mismatch"))?;
            let global_chunk = entry
                .first_chunk
                .checked_add(local_chunk)
                .ok_or(Error::Corrupt("chunk index overflow"))?;
            let chunk = self.chunk(global_chunk)?;
            let chunk_len = u64_to_usize(chunk.original_size, "original chunk size")?;
            resize_buffer(&mut scratch.decoded, chunk_len, "decoded chunk allocation")?;
            self.read_chunk_by_index_into(
                label,
                local_chunk,
                global_chunk,
                &mut scratch.decoded,
                &mut scratch.stored,
            )?;
            let chunk_start = u64::from(local_chunk)
                .checked_mul(chunk_size)
                .ok_or(Error::Corrupt("range chunk offset overflow"))?;
            let chunk_end = chunk_start
                .checked_add(chunk.original_size)
                .ok_or(Error::Corrupt("range chunk end overflow"))?;
            let take_start =
                u64_to_usize(offset.max(chunk_start) - chunk_start, "range slice start")?;
            let take_end = u64_to_usize(end.min(chunk_end) - chunk_start, "range slice end")?;
            let amount = take_end
                .checked_sub(take_start)
                .ok_or(Error::Corrupt("range slice order"))?;
            let written_end = written
                .checked_add(amount)
                .ok_or(Error::Corrupt("range output size overflow"))?;
            let output = dst
                .get_mut(written..written_end)
                .ok_or(Error::Corrupt("range output size mismatch"))?;
            let decoded = scratch
                .decoded
                .get(take_start..take_end)
                .ok_or(Error::Corrupt("range chunk slice mismatch"))?;
            output.copy_from_slice(decoded);
            written = written_end;
        }
        (written == dst.len())
            .then_some(())
            .ok_or(Error::Corrupt("range decompressed size mismatch"))?;
        Ok(())
    }

    fn chunk(&self, global_chunk: u32) -> Result<Chunk> {
        self.chunks
            .get(usize::try_from(global_chunk).map_err(|_| Error::TooLarge("chunk index"))?)
            .copied()
            .ok_or(Error::Corrupt("chunk index outside TOC"))
    }

    fn read_chunk_by_index_into(
        &self,
        label: ChecksumLabel<'_>,
        local_chunk: u32,
        global_chunk: u32,
        dst: &mut [u8],
        stored: &mut Vec<u8>,
    ) -> Result<()> {
        self.ensure_chunk_operation_limit(1)?;
        let chunk = self.chunk(global_chunk)?;
        let original_len = u64_to_usize(chunk.original_size, "original chunk size")?;
        if dst.len() != original_len {
            return Err(Error::BufferSizeMismatch {
                expected: original_len,
                actual: dst.len(),
            });
        }
        match (self.aead_key.as_ref(), chunk.compressed) {
            (None, false) => self.source.read_exact_at(chunk.offset, dst)?,
            (aead_key, compressed) => {
                let stored_len = u64_to_usize(chunk.stored_size, "stored chunk size")?;
                resize_buffer(stored, stored_len, "stored chunk allocation")?;
                self.source
                    .read_exact_at(chunk.offset, &mut stored[..stored_len])?;
                let decrypted = match aead_key {
                    Some(key) => Some(decrypt(
                        &stored[..stored_len],
                        key,
                        &chunk_aad(global_chunk),
                    )?),
                    None => None,
                };
                let plain = decrypted.as_deref().unwrap_or(&stored[..stored_len]);
                match compressed {
                    true => decompress_into_exact(plain, dst, "decompressed chunk size mismatch")?,
                    false => {
                        (plain.len() == dst.len())
                            .then_some(())
                            .ok_or(Error::Corrupt("decrypted chunk size mismatch"))?;
                        dst.copy_from_slice(plain);
                    }
                }
            }
        }
        (checksum(dst) == chunk.checksum)
            .then_some(())
            .ok_or_else(|| Error::ChecksumMismatch {
                path: label.to_error_string(),
                chunk: local_chunk,
            })?;
        Ok(())
    }
}

fn validate_limits(header: crate::format::Header, limits: ArchiveLimits) -> Result<()> {
    (header.toc_size <= limits.max_toc_stored_bytes)
        .then_some(())
        .ok_or(Error::TooLarge("stored TOC limit"))?;
    (header.toc_raw_size <= limits.max_toc_raw_bytes)
        .then_some(())
        .ok_or(Error::TooLarge("raw TOC limit"))?;
    (header.entry_count <= limits.max_entries)
        .then_some(())
        .ok_or(Error::TooManyEntries)?;
    (header.chunk_count <= limits.max_chunks)
        .then_some(())
        .ok_or(Error::TooManyChunks)?;
    (header.flags & HEADER_FLAG_PATHS_STRIPPED != 0
        || u64::from(header.entry_count) <= limits.max_total_path_bytes)
        .then_some(())
        .ok_or(Error::MetadataLimitExceeded)?;
    let entry_record_size =
        u64::try_from(TOC_ENTRY_FIXED_SIZE).map_err(|_| Error::TooLarge("TOC entry size"))?;
    let chunk_record_size =
        u64::try_from(TOC_CHUNK_FIXED_SIZE).map_err(|_| Error::TooLarge("TOC chunk size"))?;
    let mut minimum = u64::from(header.entry_count)
        .checked_mul(entry_record_size)
        .and_then(|value| {
            u64::from(header.chunk_count)
                .checked_mul(chunk_record_size)
                .and_then(|chunks| value.checked_add(chunks))
        })
        .and_then(|value| value.checked_add(4))
        .ok_or(Error::TooLarge("TOC record counts"))?;
    minimum = match header.flags & HEADER_FLAG_PATHS_STRIPPED {
        0 => minimum
            .checked_add(u64::from(header.entry_count))
            .ok_or(Error::TooLarge("TOC record counts"))?,
        _ => minimum,
    };
    (minimum <= header.toc_raw_size)
        .then_some(())
        .ok_or(Error::Corrupt("TOC counts exceed raw TOC size"))?;
    (estimated_metadata_bytes(header)? <= limits.max_metadata_bytes)
        .then_some(())
        .ok_or(Error::MetadataLimitExceeded)?;
    Ok(())
}

fn estimated_metadata_bytes(header: crate::format::Header) -> Result<u64> {
    let entry_bytes = std::mem::size_of::<TocEntry>()
        .checked_add(std::mem::size_of::<Entry>())
        .and_then(|value| value.checked_add(std::mem::size_of::<(AssetId, usize)>()))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(Error::MetadataLimitExceeded)?;
    let chunk_bytes = std::mem::size_of::<Chunk>()
        .checked_add(std::mem::size_of::<bool>())
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(Error::MetadataLimitExceeded)?;

    header
        .toc_size
        .checked_add(header.toc_raw_size)
        .and_then(|value| value.checked_add(header.toc_raw_size))
        .and_then(|value| {
            u64::from(header.entry_count)
                .checked_mul(entry_bytes)
                .and_then(|entries| value.checked_add(entries))
        })
        .and_then(|value| {
            u64::from(header.chunk_count)
                .checked_mul(chunk_bytes)
                .and_then(|chunks| value.checked_add(chunks))
        })
        .ok_or(Error::MetadataLimitExceeded)
}

fn validate_index(
    entries: &[Entry],
    index: &[(AssetId, usize)],
    paths_stripped: bool,
) -> Result<()> {
    let mut start = 0;
    while start < index.len() {
        let mut end = start + 1;
        while end < index.len() && index[end].0 == index[start].0 {
            end += 1;
        }
        let bucket_len = end - start;
        (bucket_len <= ArchiveLimits::HARD_MAX_IDENTIFIER_BUCKET_ENTRIES)
            .then_some(())
            .ok_or(Error::TooLarge("identifier collision bucket"))?;
        if paths_stripped && end - start != 1 {
            return Err(Error::HashCollision(index[start].0.get()));
        }
        let mut paths = HashSet::new();
        paths
            .try_reserve(bucket_len)
            .map_err(|_| Error::TooLarge("identifier collision bucket allocation"))?;
        for &(_, entry_index) in &index[start..end] {
            let path = entries[entry_index].path.as_str();
            if !paths.insert(path) {
                return Err(Error::DuplicatePath(path.to_owned()));
            }
        }
        start = end;
    }
    Ok(())
}

fn allocate(len: usize, what: &'static str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| Error::TooLarge(what))?;
    out.resize(len, 0);
    Ok(out)
}

fn resize_buffer(buffer: &mut Vec<u8>, len: usize, what: &'static str) -> Result<()> {
    buffer
        .try_reserve_exact(len.saturating_sub(buffer.len()))
        .map_err(|_| Error::TooLarge(what))?;
    buffer.resize(len, 0);
    Ok(())
}
