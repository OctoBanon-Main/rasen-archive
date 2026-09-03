use xxhash_rust::xxh3::xxh3_64;

pub(crate) fn checksum(data: &[u8]) -> u64 {
    xxh3_64(data)
}
