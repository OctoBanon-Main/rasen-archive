use std::io::{Read, Write};

use crate::error::{Error, Result};

use super::{
    HEADER_SIZE, MAGIC, MAX_TOC_RAW_SIZE, MAX_TOC_STORED_SIZE, REQUIRED_HEADER_FLAGS, VERSION,
    io::{read_u16, read_u32, read_u64},
    validate_alignment, validate_chunk_size
};

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

pub(crate) fn validate_header(header: Header) -> Result<()> {
    if header.version != VERSION {
        return Err(Error::UnsupportedVersion(header.version));
    }
    if header.header_size != HEADER_SIZE as u32 {
        return Err(Error::UnsupportedHeaderSize(header.header_size));
    }
    if header.flags != REQUIRED_HEADER_FLAGS {
        return Err(Error::UnsupportedFlags(header.flags));
    }

    let chunk_size = usize::try_from(header.chunk_size).map_err(|_| Error::InvalidChunkSize)?;
    validate_chunk_size(chunk_size)?;
    validate_alignment(header.alignment)?;

    if header.toc_size > MAX_TOC_STORED_SIZE {
        return Err(Error::TooLarge("stored TOC"));
    }
    if header.toc_raw_size > MAX_TOC_RAW_SIZE {
        return Err(Error::TooLarge("raw TOC"));
    }
    Ok(())
}