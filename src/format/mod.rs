mod header;
pub(crate) mod io;
mod model;
mod toc;

use crate::error::{Error, Result};

pub const MAGIC: [u8; 4] = *b"RPAK";
pub const TOC_MAGIC: [u8; 4] = *b"TOC2";
pub const VERSION: u16 = 1;
pub const HEADER_SIZE: u64 = 60;

pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
pub const DEFAULT_ALIGNMENT: u32 = 16;

pub(crate) const HEADER_FLAG_TOC_LZ4: u16 = 1 << 0;
pub(crate) const HEADER_FLAG_TOC_XOR: u16 = 1 << 1;
pub(crate) const HEADER_FLAG_CHUNKED: u16 = 1 << 2;
pub(crate) const HEADER_FLAG_XXH3: u16 = 1 << 3;
pub(crate) const HEADER_FLAG_PATHS_STRIPPED: u16 = 1 << 4;
pub(crate) const REQUIRED_HEADER_FLAGS: u16 =
    HEADER_FLAG_TOC_LZ4 | HEADER_FLAG_TOC_XOR | HEADER_FLAG_CHUNKED | HEADER_FLAG_XXH3;
pub(crate) const KNOWN_HEADER_FLAGS: u16 = REQUIRED_HEADER_FLAGS | HEADER_FLAG_PATHS_STRIPPED;

pub(crate) const CHUNK_FLAG_LZ4: u16 = 1 << 0;

pub(crate) const MAX_TOC_STORED_SIZE: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_TOC_RAW_SIZE: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_PATH_LEN: usize = u16::MAX as usize;
pub(crate) const MAX_CHUNK_SIZE: usize = 64 * 1024 * 1024;
pub(crate) const MAX_ALIGNMENT: u32 = 1024 * 1024;
pub(crate) const TOC_ENTRY_FIXED_SIZE: usize = 36;
pub(crate) const TOC_CHUNK_FIXED_SIZE: usize = 36;

pub(crate) use header::{Header, read_header, validate_header, write_header};
pub(crate) use model::{Chunk, TocEntry};
pub(crate) use toc::{decode_toc, encode_toc, validate_layout};

pub(crate) fn validate_chunk_size(chunk_size: usize) -> Result<()> {
    if chunk_size == 0 || chunk_size > MAX_CHUNK_SIZE {
        return Err(Error::InvalidChunkSize);
    }
    Ok(())
}

pub(crate) fn validate_alignment(alignment: u32) -> Result<()> {
    if alignment == 0 || alignment > MAX_ALIGNMENT || !alignment.is_power_of_two() {
        return Err(Error::InvalidAlignment);
    }
    Ok(())
}
