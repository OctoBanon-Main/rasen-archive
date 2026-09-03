#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use rasen_archive::{Archive, ArchiveLimits, AssetId};

fuzz_target!(|data: &[u8]| {
    let limits = ArchiveLimits {
        max_toc_stored_bytes: 1024 * 1024,
        max_toc_raw_bytes: 2 * 1024 * 1024,
        max_entries: 10_000,
        max_chunks: 100_000,
        max_total_path_bytes: 1024 * 1024,
        max_single_asset_bytes: 1024 * 1024,
        max_metadata_bytes: 16 * 1024 * 1024,
        ..ArchiveLimits::runtime_default()
    };
    let Ok(archive) = Archive::open_with_limits(Cursor::new(data), b"fuzz-key", limits) else {
        return;
    };
    let Some(entry) = archive.entries().get(
        data.iter().take(8).fold(0usize, |value, byte| {
            value.wrapping_mul(256) | usize::from(*byte)
        }) % archive.entries().len().max(1),
    ) else {
        return;
    };
    let id = AssetId::from_raw(entry.path_hash);
    let _ = archive.entry_by_id(id);
    let _ = archive.contains_id(id);
    if entry.original_size <= 1024 * 1024 {
        let _ = archive.read_by_id(id);
    }
    if let Some(path) = entry.path() {
        let offset = u64::from(data.first().copied().unwrap_or(0)).min(entry.original_size);
        let len = usize::from(data.get(1).copied().unwrap_or(0)).min(4096);
        let _ = archive.entry(path);
        let _ = archive.read_range(path, offset, len);
        if archive.chunk_size() <= 1024 * 1024 && entry.chunk_count != 0 {
            let chunk = u32::from(data.get(2).copied().unwrap_or(0)) % entry.chunk_count;
            let _ = archive.read_chunk(path, chunk);
        }
    }
});
