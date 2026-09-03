use std::{
    fs::{self, File},
    io::Cursor,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use rasen_archive::{
    Archive, ArchiveLimits, ArchiveScratch, AssetId, Error, InputFile, PackOptions,
    pack_with_options,
};

fn packed(data: &[u8], chunk_size: usize) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &[InputFile {
            path: "asset.bin".into(),
            data: data.to_vec(),
        }],
        b"key",
        PackOptions {
            chunk_size,
            ..PackOptions::default()
        },
    )
    .unwrap();
    output.into_inner()
}

#[test]
fn range_boundaries_and_destination_apis() {
    let data: Vec<_> = (0..100).collect();
    let archive = Archive::open(Cursor::new(packed(&data, 16)), b"key").unwrap();

    assert_eq!(archive.read_range("asset.bin", 0, 0).unwrap(), b"");
    assert_eq!(archive.read_range("asset.bin", 100, 10).unwrap(), b"");
    assert!(matches!(
        archive.read_range("asset.bin", 101, 0),
        Err(Error::InvalidRange)
    ));
    assert!(matches!(
        archive.read_range("asset.bin", u64::MAX, 2),
        Err(Error::InvalidRange)
    ));
    assert_eq!(archive.read_range("asset.bin", 84, 16).unwrap(), data[84..]);
    assert_eq!(archive.read_range("asset.bin", 90, 20).unwrap(), data[90..]);
    assert_eq!(
        archive.read_range("asset.bin", 15, 2).unwrap(),
        data[15..17]
    );
    assert_eq!(archive.read_range("asset.bin", 7, 70).unwrap(), data[7..77]);
    assert!(matches!(
        archive.read_range_exact("asset.bin", 90, 20),
        Err(Error::InvalidRange)
    ));

    let mut full = vec![0; data.len()];
    archive.read_into("asset.bin", &mut full).unwrap();
    assert_eq!(full, data);
    full.fill(0);
    let mut scratch = ArchiveScratch::default();
    archive
        .read_into_with_scratch("asset.bin", &mut full, &mut scratch)
        .unwrap();
    assert_eq!(full, data);

    let mut chunk = [0; 16];
    archive
        .read_chunk_into_with_scratch("asset.bin", 0, &mut chunk, &mut scratch)
        .unwrap();
    assert_eq!(chunk, data[..16]);
    assert!(matches!(
        archive.read_chunk("asset.bin", 7),
        Err(Error::ChunkOutOfRange { chunk: 7, count: 7 })
    ));
    assert!(matches!(
        archive.read_into("asset.bin", &mut [0; 99]),
        Err(Error::BufferSizeMismatch {
            expected: 100,
            actual: 99
        })
    ));
    assert!(matches!(
        archive.read_chunk_into("asset.bin", 0, &mut [0; 15]),
        Err(Error::BufferSizeMismatch {
            expected: 16,
            actual: 15
        })
    ));

    let mut range = [0; 20];
    assert_eq!(
        archive
            .read_range_with_scratch("asset.bin", 90, &mut range, &mut scratch)
            .unwrap(),
        10
    );
    assert_eq!(&range[..10], &data[90..]);

    let id = AssetId::from_path("asset.bin").unwrap();
    assert!(archive.contains_id(id));
    assert_eq!(
        archive.entry("./asset.bin").unwrap().path(),
        Some("asset.bin")
    );
    assert_eq!(archive.entry_by_id(id).unwrap().original_size, 100);
    assert_eq!(archive.read_by_id(id).unwrap(), data);
    assert!(archive.try_contains("../invalid").is_err());
}

#[test]
fn range_reads_match_full_read_slices() {
    let data: Vec<_> = (0..33).collect();
    let archive = Archive::open(Cursor::new(packed(&data, 7)), b"key").unwrap();

    for offset in 0..=data.len() {
        for len in 0..=data.len() + 2 {
            let end = offset.saturating_add(len).min(data.len());
            assert_eq!(
                archive.read_range("asset.bin", offset as u64, len).unwrap(),
                data[offset..end]
            );
        }
    }
}

#[test]
fn slices_and_shared_slices_are_archive_sources() {
    let data = b"slice source";
    let bytes = packed(data, 16);
    assert_eq!(
        Archive::open(bytes.as_slice(), b"key")
            .unwrap()
            .read("asset.bin")
            .unwrap(),
        data
    );

    let shared: Arc<[u8]> = bytes.into();
    assert_eq!(
        Archive::open(shared, b"key")
            .unwrap()
            .read("asset.bin")
            .unwrap(),
        data
    );
}

#[test]
fn configured_limits_reject_before_toc_decode() {
    let bytes = packed(b"payload", 16);
    let limits = ArchiveLimits {
        max_toc_stored_bytes: 1,
        ..ArchiveLimits::permissive_v1()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::TooLarge("stored TOC limit"))
    ));

    let limits = ArchiveLimits {
        max_entries: 0,
        ..ArchiveLimits::permissive_v1()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::TooManyEntries)
    ));

    let limits = ArchiveLimits {
        max_toc_raw_bytes: 1,
        ..ArchiveLimits::permissive_v1()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::TooLarge("raw TOC limit"))
    ));

    let limits = ArchiveLimits {
        max_chunks: 0,
        ..ArchiveLimits::permissive_v1()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::TooManyChunks)
    ));

    let limits = ArchiveLimits {
        max_path_bytes: 1,
        ..ArchiveLimits::permissive_v1()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::TooLarge("path length limit"))
    ));

    let limits = ArchiveLimits {
        max_total_path_bytes: 0,
        ..ArchiveLimits::permissive_v1()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::MetadataLimitExceeded)
    ));

    let limits = ArchiveLimits {
        max_total_decompressed_bytes: 6,
        ..ArchiveLimits::permissive_v1()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::ArchiveTooLarge)
    ));

    let limits = ArchiveLimits {
        max_single_asset_bytes: 6,
        ..ArchiveLimits::tooling_default()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::AssetTooLarge)
    ));

    let limits = ArchiveLimits {
        max_metadata_bytes: 0,
        ..ArchiveLimits::tooling_default()
    };
    assert!(matches!(
        Archive::open_with_limits(Cursor::new(bytes.clone()), b"key", limits),
        Err(Error::MetadataLimitExceeded)
    ));

    let mut impossible_counts = bytes;
    impossible_counts[20..24].copy_from_slice(&100u32.to_le_bytes());
    assert!(matches!(
        Archive::open(Cursor::new(impossible_counts), b"key"),
        Err(Error::Corrupt("TOC counts exceed raw TOC size"))
    ));
}

#[test]
fn runtime_and_tooling_limits_are_separate() {
    let runtime = ArchiveLimits::runtime_default();
    let tooling = ArchiveLimits::tooling_default();

    assert_eq!(ArchiveLimits::default().max_entries, 100_000);
    assert_eq!(runtime.max_chunks, 500_000);
    assert_eq!(runtime.max_single_asset_bytes, 512 * 1024 * 1024);
    assert!(runtime.max_entries < tooling.max_entries);
    assert!(runtime.max_chunks < tooling.max_chunks);
    assert!(runtime.max_metadata_bytes < tooling.max_metadata_bytes);

    let bytes = packed(b"payload", 16);
    let mut too_many_entries = bytes.clone();
    too_many_entries[20..24].copy_from_slice(&(runtime.max_entries + 1).to_le_bytes());
    assert!(matches!(
        Archive::open(Cursor::new(too_many_entries), b"key"),
        Err(Error::TooManyEntries)
    ));

    let mut too_many_chunks = bytes;
    too_many_chunks[24..28].copy_from_slice(&(runtime.max_chunks + 1).to_le_bytes());
    assert!(matches!(
        Archive::open(Cursor::new(too_many_chunks), b"key"),
        Err(Error::TooManyChunks)
    ));
}

#[test]
fn chunk_processing_is_limited_per_operation() {
    let bytes = packed(&(0..40).collect::<Vec<_>>(), 16);
    let limits = ArchiveLimits {
        max_chunks_per_operation: 2,
        ..ArchiveLimits::tooling_default()
    };
    let archive = Archive::open_with_limits(Cursor::new(bytes), b"key", limits).unwrap();

    assert!(matches!(
        archive.read("asset.bin"),
        Err(Error::TooManyChunks)
    ));
    assert!(matches!(archive.verify(), Err(Error::TooManyChunks)));
    assert_eq!(archive.read_range("asset.bin", 0, 32).unwrap().len(), 32);
    assert!(matches!(
        archive.read_range("asset.bin", 0, 33),
        Err(Error::TooManyChunks)
    ));
    assert_eq!(archive.read_chunk("asset.bin", 2).unwrap().len(), 8);
}

#[test]
fn caller_limits_cannot_raise_hard_archive_caps() {
    let bytes = packed(b"payload", 16);
    let cases = [
        (
            36,
            ArchiveLimits::HARD_MAX_TOC_STORED_BYTES + 1,
            "stored TOC",
        ),
        (44, ArchiveLimits::HARD_MAX_TOC_RAW_BYTES + 1, "raw TOC"),
    ];
    for (offset, value, expected) in cases {
        let mut malformed = bytes.clone();
        malformed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(matches!(
            Archive::open_with_limits(
                Cursor::new(malformed),
                b"key",
                ArchiveLimits::permissive_v1()
            ),
            Err(Error::TooLarge(actual)) if actual == expected
        ));
    }

    let mut too_many_entries = bytes.clone();
    too_many_entries[20..24].copy_from_slice(&(ArchiveLimits::HARD_MAX_ENTRIES + 1).to_le_bytes());
    assert!(matches!(
        Archive::open_with_limits(
            Cursor::new(too_many_entries),
            b"key",
            ArchiveLimits::permissive_v1()
        ),
        Err(Error::TooManyEntries)
    ));

    let mut too_many_chunks = bytes;
    too_many_chunks[24..28].copy_from_slice(&(ArchiveLimits::HARD_MAX_CHUNKS + 1).to_le_bytes());
    assert!(matches!(
        Archive::open_with_limits(
            Cursor::new(too_many_chunks),
            b"key",
            ArchiveLimits::permissive_v1()
        ),
        Err(Error::TooManyChunks)
    ));
}

#[test]
fn file_archive_supports_parallel_reads() {
    let data: Vec<_> = (0..300_000).map(|value| (value % 251) as u8).collect();
    let path = temp_file("parallel");
    fs::write(&path, packed(&data, 4096)).unwrap();
    let archive = Arc::new(Archive::open(File::open(&path).unwrap(), b"key").unwrap());
    let expected = Arc::new(data);
    let mut threads = Vec::new();
    for thread_index in 0..8 {
        let archive = Arc::clone(&archive);
        let expected = Arc::clone(&expected);
        threads.push(thread::spawn(move || {
            let offset = thread_index * 30_000;
            let range = archive
                .read_range("asset.bin", offset as u64, 20_000)
                .unwrap();
            assert_eq!(range, expected[offset..offset + 20_000]);
            assert_eq!(
                archive
                    .read_chunk("asset.bin", thread_index.try_into().unwrap())
                    .unwrap()
                    .len(),
                4096
            );
        }));
    }
    for handle in threads {
        handle.join().unwrap();
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn concurrent_corruption_is_reported() {
    let path = temp_file("parallel-corrupt");
    let mut seed = 0x1234_5678_9abc_def0u64;
    let data: Vec<_> = (0..32_000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    let mut bytes = packed(&data, 4096);
    let first_chunk = usize::try_from(rasen_archive::HEADER_SIZE.next_multiple_of(16)).unwrap();
    bytes[first_chunk] ^= 1;
    fs::write(&path, bytes).unwrap();
    let archive = Arc::new(Archive::open(File::open(&path).unwrap(), b"key").unwrap());
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let archive = Arc::clone(&archive);
            thread::spawn(move || {
                assert!(matches!(
                    archive.read_chunk("asset.bin", 0),
                    Err(Error::ChecksumMismatch { .. })
                ));
            })
        })
        .collect();
    for handle in threads {
        handle.join().unwrap();
    }
    fs::remove_file(path).unwrap();
}

fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rasen-archive-{name}-{}-{}.rpak",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
