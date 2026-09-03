use std::{
    cmp::Ordering,
    collections::HashSet,
    io::{Read, Seek, SeekFrom, Write},
};

use lz4_flex::block::{compress_into, get_maximum_output_size};

use crate::{
    crypto::{TOC_AAD, chunk_aad, derive_key, encrypt, xor_in_place},
    error::{Error, Result},
    format::{
        Chunk, HEADER_FLAG_AEAD, HEADER_FLAG_PATHS_STRIPPED, HEADER_FLAG_TOC_XOR, HEADER_SIZE,
        Header, MAX_TOC_RAW_SIZE, MAX_TOC_STORED_SIZE, REQUIRED_HEADER_FLAGS, TocEntry, VERSION,
        encode_toc,
        io::{align_up, usize_to_u32, usize_to_u64},
        write_header,
    },
    hash::checksum,
    path::normalize_path,
};

use super::options::{PackOptions, Protection};

#[derive(Debug, Clone)]
pub struct InputFile {
    pub path: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PackSummary {
    pub archive_len: u64,
    pub entry_count: u32,
    pub chunk_count: u32,
}

pub struct Packer<'a, W> {
    writer: &'a mut W,
    key: &'a [u8],
    aead_key: Option<[u8; 32]>,
    options: PackOptions,
    entries: Vec<TocEntry>,
    chunks: Vec<Chunk>,
    seen_hashes: HashSet<u64>,
    raw: Vec<u8>,
    compressed: Vec<u8>,
    offset: u64,
    failed: bool,
}

impl<'a, W: Write + Seek> Packer<'a, W> {
    pub fn new(writer: &'a mut W, key: &'a [u8], options: PackOptions) -> Result<Self> {
        (!key.is_empty()).then_some(()).ok_or(Error::EmptyXorKey)?;
        let options = options.validate()?;
        let aead_key = (options.protection == Protection::Aead).then(|| derive_key(key));
        (writer.seek(SeekFrom::End(0))? == 0)
            .then_some(())
            .ok_or(Error::NonEmptyDestination)?;
        writer.seek(SeekFrom::Start(0))?;
        writer.write_all(&[0u8; HEADER_SIZE as usize])?;

        let compressed_len = get_maximum_output_size(options.chunk_size);
        let mut compressed = Vec::new();
        compressed
            .try_reserve_exact(compressed_len)
            .map_err(|_| Error::TooLarge("compression buffer"))?;
        compressed.resize(compressed_len, 0);

        let mut raw = Vec::new();
        raw.try_reserve_exact(options.chunk_size)
            .map_err(|_| Error::TooLarge("raw chunk buffer"))?;
        raw.resize(options.chunk_size, 0);

        Ok(Self {
            writer,
            key,
            aead_key,
            options,
            entries: Vec::new(),
            chunks: Vec::new(),
            seen_hashes: HashSet::new(),
            raw,
            compressed,
            offset: HEADER_SIZE,
            failed: false,
        })
    }

    pub fn add_reader<R: Read>(&mut self, path: &str, reader: &mut R) -> Result<()> {
        (!self.failed).then_some(()).ok_or(Error::IncompletePack)?;
        let result = self.add_reader_inner(path, reader);
        self.failed |= result.is_err();
        result
    }

    fn add_reader_inner<R: Read>(&mut self, path: &str, reader: &mut R) -> Result<()> {
        let normalized_path = normalize_path(path)?;
        let path_hash = checksum(normalized_path.as_bytes());
        match self.seen_hashes.insert(path_hash) {
            true => {}
            false => {
                let duplicate = self
                    .entries
                    .iter()
                    .any(|entry| entry.path_hash == path_hash && entry.path == normalized_path);
                (!duplicate)
                    .then_some(())
                    .ok_or_else(|| Error::DuplicatePath(normalized_path.clone()))?;
                (!self.options.mode.strips_paths())
                    .then_some(())
                    .ok_or(Error::HashCollision(path_hash))?;
            }
        }

        let first_chunk_index = self.chunks.len();
        let first_chunk = usize_to_u32(first_chunk_index, "chunk index")?;
        let mut original_total = 0u64;
        let mut stored_total = 0u64;

        loop {
            let raw_len = match read_chunk(reader, &mut self.raw)? {
                0 => break,
                raw_len => raw_len,
            };
            let compressed_len = compress_into(&self.raw[..raw_len], &mut self.compressed)
                .map_err(|error| Error::Lz4(error.to_string()))?;
            let compression = compressed_len.cmp(&raw_len);
            self.align()?;
            let chunk_offset = self.offset;
            let (plain_stored, is_compressed) = match compression {
                Ordering::Less => (&self.compressed[..compressed_len], true),
                Ordering::Equal | Ordering::Greater => (&self.raw[..raw_len], false),
            };
            let chunk_checksum = checksum(&self.raw[..raw_len]);
            let global_chunk = usize_to_u32(self.chunks.len(), "chunk index")?;
            let encrypted = match &self.aead_key {
                Some(key) => Some(encrypt(plain_stored, key, &chunk_aad(global_chunk))?),
                None => None,
            };
            let stored = encrypted.as_deref().unwrap_or(plain_stored);

            self.writer.write_all(stored)?;
            self.offset = self
                .offset
                .checked_add(usize_to_u64(stored.len(), "stored chunk size")?)
                .ok_or(Error::TooLarge("archive offset"))?;

            let original_size = usize_to_u64(raw_len, "original chunk size")?;
            let stored_size = usize_to_u64(stored.len(), "stored chunk size")?;
            original_total = original_total
                .checked_add(original_size)
                .ok_or(Error::TooLarge("entry original size"))?;
            stored_total = stored_total
                .checked_add(stored_size)
                .ok_or(Error::TooLarge("entry stored size"))?;
            self.chunks.push(Chunk {
                offset: chunk_offset,
                stored_size,
                original_size,
                checksum: chunk_checksum,
                compressed: is_compressed,
            });
            usize_to_u32(self.chunks.len(), "chunk count")?;
        }

        let chunk_count = usize_to_u32(
            self.chunks
                .len()
                .checked_sub(first_chunk_index)
                .ok_or(Error::TooLarge("entry chunk count"))?,
            "entry chunk count",
        )?;
        self.entries.push(TocEntry {
            path: normalized_path,
            path_hash,
            original_size: original_total,
            stored_size: stored_total,
            first_chunk,
            chunk_count,
        });
        usize_to_u32(self.entries.len(), "entry count")?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<PackSummary> {
        (!self.failed).then_some(()).ok_or(Error::IncompletePack)?;
        self.align()?;
        let toc_offset = self.offset;
        let _ = self.options.mode.strips_paths().then(|| {
            self.entries.iter_mut().for_each(|entry| entry.path.clear());
        });
        let toc_plain = encode_toc(&self.entries, &self.chunks)?;
        let toc_raw_size = usize_to_u64(toc_plain.len(), "raw TOC size")?;
        (toc_raw_size <= MAX_TOC_RAW_SIZE)
            .then_some(())
            .ok_or(Error::TooLarge("raw TOC"))?;
        let toc_hash = checksum(&toc_plain);

        let max_compressed = get_maximum_output_size(toc_plain.len());
        let mut toc_stored = Vec::new();
        toc_stored
            .try_reserve_exact(max_compressed)
            .map_err(|_| Error::TooLarge("stored TOC allocation"))?;
        toc_stored.resize(max_compressed, 0);
        let toc_len = compress_into(&toc_plain, &mut toc_stored)
            .map_err(|error| Error::Lz4(error.to_string()))?;
        toc_stored.truncate(toc_len);
        match &self.aead_key {
            Some(key) => toc_stored = encrypt(&toc_stored, key, TOC_AAD)?,
            None => xor_in_place(&mut toc_stored, self.key),
        }
        let toc_size = usize_to_u64(toc_stored.len(), "stored TOC size")?;
        (toc_size <= MAX_TOC_STORED_SIZE)
            .then_some(())
            .ok_or(Error::TooLarge("stored TOC"))?;
        self.writer.write_all(&toc_stored)?;
        self.offset = self
            .offset
            .checked_add(toc_size)
            .ok_or(Error::TooLarge("archive length"))?;
        let archive_len = self.offset;

        let header = Header {
            version: VERSION,
            flags: REQUIRED_HEADER_FLAGS
                | match self.options.protection {
                    Protection::Xor => HEADER_FLAG_TOC_XOR,
                    Protection::Aead => HEADER_FLAG_AEAD,
                }
                | (HEADER_FLAG_PATHS_STRIPPED * u16::from(self.options.mode.strips_paths())),
            header_size: HEADER_SIZE as u32,
            alignment: self.options.alignment,
            chunk_size: usize_to_u32(self.options.chunk_size, "chunk size")?,
            entry_count: usize_to_u32(self.entries.len(), "entry count")?,
            chunk_count: usize_to_u32(self.chunks.len(), "chunk count")?,
            toc_offset,
            toc_size,
            toc_raw_size,
            toc_hash,
        };
        self.writer.seek(SeekFrom::Start(0))?;
        write_header(self.writer, header)?;
        self.writer.seek(SeekFrom::Start(archive_len))?;

        Ok(PackSummary {
            archive_len,
            entry_count: header.entry_count,
            chunk_count: header.chunk_count,
        })
    }

    fn align(&mut self) -> Result<()> {
        let aligned = align_up(self.offset, u64::from(self.options.alignment))?;
        let mut remaining = aligned - self.offset;
        const ZEROES: [u8; 4096] = [0; 4096];
        while remaining != 0 {
            let zeroes_len = u64::try_from(ZEROES.len()).map_err(|_| Error::TooLarge("padding"))?;
            let amount = usize::try_from(remaining.min(zeroes_len))
                .map_err(|_| Error::TooLarge("padding"))?;
            self.writer.write_all(&ZEROES[..amount])?;
            remaining = remaining
                .checked_sub(u64::try_from(amount).map_err(|_| Error::TooLarge("padding"))?)
                .ok_or(Error::TooLarge("padding"))?;
        }
        self.offset = aligned;
        Ok(())
    }
}

pub fn pack<W: Write + Seek>(writer: &mut W, files: &[InputFile], key: &[u8]) -> Result<()> {
    pack_with_options(writer, files, key, PackOptions::default())
}

pub fn pack_with_options<W: Write + Seek>(
    writer: &mut W,
    files: &[InputFile],
    key: &[u8],
    options: PackOptions,
) -> Result<()> {
    let mut packer = Packer::new(writer, key, options)?;
    for file in files {
        packer.add_reader(&file.path, &mut file.data.as_slice())?;
    }
    packer.finish()?;
    Ok(())
}

fn read_chunk<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(filled)
}
