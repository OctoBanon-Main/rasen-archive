use lz4_flex::block::decompress_into;
use xxhash_rust::xxh3::xxh3_64;

use crate::error::{Error, Result};

pub(crate) fn checksum(data: &[u8]) -> u64 {
    xxh3_64(data)
}

pub(crate) fn decompress_block_exact(
    data: &[u8],
    original_len: usize,
    mismatch: &'static str,
) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(original_len)
        .map_err(|_| Error::TooLarge("decompression output allocation"))?;
    decoded.resize(original_len, 0);
    decompress_into_exact(data, &mut decoded, mismatch)?;
    Ok(decoded)
}

pub(crate) fn xor_in_place(data: &mut [u8], key: &[u8]) {
    for (i, byte) in data.iter_mut().enumerate() {
        *byte ^= key[i % key.len()];
    }
}

pub(crate) fn decompress_into_exact(
    data: &[u8],
    output: &mut [u8],
    mismatch: &'static str,
) -> Result<()> {
    let decoded = decompress_into(data, output).map_err(|e| Error::Lz4(e.to_string()))?;
    if decoded != output.len() {
        return Err(Error::Corrupt(mismatch));
    }
    Ok(())
}
