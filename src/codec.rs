use lz4_flex::block::{compress, decompress};
use xxhash_rust::xxh3::xxh3_64;

use crate::error::{Error, Result};

pub(crate) fn checksum(data: &[u8]) -> u64 {
    xxh3_64(data)
}

pub(crate) fn compress_block(data: &[u8]) -> Vec<u8> {
    compress(data)
}

pub(crate) fn decompress_block_exact(
    data: &[u8],
    original_len: usize,
    mismatch: &'static str,
) -> Result<Vec<u8>> {
    let decoded = decompress(data, original_len).map_err(|e| Error::Lz4(e.to_string()))?;
    if decoded.len() != original_len {
        return Err(Error::Corrupt(mismatch));
    }
    Ok(decoded)
}

pub(crate) fn xor_in_place(data: &mut [u8], key: &[u8]) {
    for (i, byte) in data.iter_mut().enumerate() {
        *byte ^= key[i % key.len()];
    }
}