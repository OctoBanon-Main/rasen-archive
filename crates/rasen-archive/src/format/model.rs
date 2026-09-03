#[derive(Debug, Clone)]
pub(crate) struct TocEntry {
    pub path: String,
    pub path_hash: u64,
    pub original_size: u64,
    pub stored_size: u64,
    pub first_chunk: u32,
    pub chunk_count: u32,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct Chunk {
    pub(crate) offset: u64,
    pub(crate) stored_size: u64,
    pub(crate) original_size: u64,
    pub(crate) checksum: u64,
    pub(crate) compressed: bool,
}
