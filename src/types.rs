use crate::{
    error::{Error, Result},
    format::{DEFAULT_ALIGNMENT, DEFAULT_CHUNK_SIZE, MAX_ALIGNMENT, MAX_CHUNK_SIZE},
};

#[derive(Debug, Clone)]
pub struct InputFile {
    pub path: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Copy, Clone)]
pub struct PackOptions {
    pub chunk_size: usize,
    pub alignment: u32,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            alignment: DEFAULT_ALIGNMENT,
        }
    }
}

impl PackOptions {
    pub(crate) fn validate(self) -> Result<Self> {
        if self.chunk_size == 0 || self.chunk_size > MAX_CHUNK_SIZE {
            return Err(Error::InvalidChunkSize);
        }
        if self.alignment == 0
            || self.alignment > MAX_ALIGNMENT
            || !self.alignment.is_power_of_two()
        {
            return Err(Error::InvalidAlignment);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
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
