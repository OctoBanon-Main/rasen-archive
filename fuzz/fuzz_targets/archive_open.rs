#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use rasen_archive::{Archive, ArchiveLimits};

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
    let _ = Archive::open_with_limits(Cursor::new(data), b"fuzz-key", limits);
});
