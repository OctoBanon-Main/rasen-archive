use std::io::{Read, Write};

use crate::{
    error::{Error, Result},
    types::PackOptions,
    util::{read_u16, read_u32, read_u64}
};

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
pub(crate) const REQUIRED_HEADER_FLAGS: u16 = HEADER_FLAG_TOC_LZ4
    | HEADER_FLAG_TOC_XOR
    | HEADER_FLAG_CHUNKED
    | HEADER_FLAG_XXH3;

pub(crate) const CHUNK_FLAG_LZ4: u16 = 1 << 0;

pub(crate) const MAX_TOC_STORED_SIZE: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_TOC_RAW_SIZE: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_PATH_LEN: usize = u16::MAX as usize;
pub(crate) const MAX_CHUNK_SIZE: usize = 64 * 1024 * 1024;
pub(crate) const MAX_ALIGNMENT: u32 = 1024 * 1024;
pub(crate) const TOC_ENTRY_FIXED_SIZE: usize = 36;
pub(crate) const TOC_CHUNK_FIXED_SIZE: usize = 36;

#[derive(Debug, Copy, Clone)]
pub(crate) struct Header {
    pub(crate) version: u16,
    pub(crate) flags: u16,
    pub(crate) header_size: u32,
    pub(crate) alignment: u32,
    pub(crate) chunk_size: u32,
    pub(crate) entry_count: u32,
    pub(crate) chunk_count: u32,
    pub(crate) toc_offset: u64,
    pub(crate) toc_size: u64,
    pub(crate) toc_raw_size: u64,
    pub(crate) toc_hash: u64,
}

pub(crate) fn write_header<W: Write>(w: &mut W, h: Header) -> Result<()> {
    w.write_all(&MAGIC)?;
    w.write_all(&h.version.to_le_bytes())?;
    w.write_all(&h.flags.to_le_bytes())?;
    w.write_all(&h.header_size.to_le_bytes())?;
    w.write_all(&h.alignment.to_le_bytes())?;
    w.write_all(&h.chunk_size.to_le_bytes())?;
    w.write_all(&h.entry_count.to_le_bytes())?;
    w.write_all(&h.chunk_count.to_le_bytes())?;
    w.write_all(&h.toc_offset.to_le_bytes())?;
    w.write_all(&h.toc_size.to_le_bytes())?;
    w.write_all(&h.toc_raw_size.to_le_bytes())?;
    w.write_all(&h.toc_hash.to_le_bytes())?;
    Ok(())
}

pub(crate) fn read_header<R: Read>(r: &mut R) -> Result<Header> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(Error::BadMagic);
    }

    Ok(Header {
        version: read_u16(r)?,
        flags: read_u16(r)?,
        header_size: read_u32(r)?,
        alignment: read_u32(r)?,
        chunk_size: read_u32(r)?,
        entry_count: read_u32(r)?,
        chunk_count: read_u32(r)?,
        toc_offset: read_u64(r)?,
        toc_size: read_u64(r)?,
        toc_raw_size: read_u64(r)?,
        toc_hash: read_u64(r)?,
    })
}

pub(crate) fn validate_header(h: Header) -> Result<()> {
    if h.version != VERSION {
        return Err(Error::UnsupportedVersion(h.version));
    }
    if h.header_size != HEADER_SIZE as u32 {
        return Err(Error::UnsupportedHeaderSize(h.header_size));
    }
    if h.flags != REQUIRED_HEADER_FLAGS {
        return Err(Error::UnsupportedFlags(h.flags));
    }
    PackOptions {
        chunk_size: usize::try_from(h.chunk_size).map_err(|_| Error::InvalidChunkSize)?,
        alignment: h.alignment,
    }
    .validate()?;
    if h.toc_size > MAX_TOC_STORED_SIZE {
        return Err(Error::TooLarge("stored TOC"));
    }
    if h.toc_raw_size > MAX_TOC_RAW_SIZE {
        return Err(Error::TooLarge("raw TOC"));
    }
    Ok(())
}
