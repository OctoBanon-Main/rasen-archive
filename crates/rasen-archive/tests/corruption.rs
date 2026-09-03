use std::io::Cursor;

use lz4_flex::block::{compress, decompress};
use rasen_archive::{
    Archive, ArchiveLimits, Error, InputFile, PackMode, PackOptions, pack, pack_with_options,
};
use xxhash_rust::xxh3::xxh3_64;

#[test]
fn wrong_xor_key_fails() {
    let files = vec![InputFile {
        path: "a.txt".into(),
        data: b"hello hello hello".to_vec(),
    }];
    let mut out = Cursor::new(Vec::new());
    pack(&mut out, &files, b"right").unwrap();
    out.set_position(0);
    assert!(Archive::open(out, b"wrong").is_err());
}

#[test]
fn payload_corruption_is_detected() {
    let key = b"key";
    let mut seed = 0x1234_5678_9abc_def0u64;
    let mut data = Vec::with_capacity(4096);
    for _ in 0..4096 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        data.push(seed as u8);
    }

    let files = vec![InputFile {
        path: "noise.bin".into(),
        data,
    }];
    let mut out = Cursor::new(Vec::new());
    pack(&mut out, &files, key).unwrap();
    let mut bytes = out.into_inner();

    let archive = Archive::open(Cursor::new(bytes.clone()), key).unwrap();
    let first_chunk_offset = usize::try_from(
        rasen_archive::HEADER_SIZE.next_multiple_of(u64::from(archive.alignment())),
    )
    .unwrap();
    bytes[first_chunk_offset] ^= 0x55;
    let corrupted = Archive::open(Cursor::new(bytes), key).unwrap();
    assert!(matches!(
        corrupted.read("noise.bin"),
        Err(Error::ChecksumMismatch { .. })
    ));
}

#[test]
fn malformed_headers_are_rejected() {
    let bytes = archive_bytes(&[InputFile {
        path: "a.bin".into(),
        data: b"payload".to_vec(),
    }]);
    let cases: &[(usize, &[u8])] = &[
        (0, b"NOPE"),
        (4, &2u16.to_le_bytes()),
        (6, &0x8000u16.to_le_bytes()),
        (8, &59u32.to_le_bytes()),
        (12, &3u32.to_le_bytes()),
        (16, &0u32.to_le_bytes()),
    ];
    for &(offset, replacement) in cases {
        let mut malformed = bytes.clone();
        malformed[offset..offset + replacement.len()].copy_from_slice(replacement);
        assert!(Archive::open(Cursor::new(malformed), b"key").is_err());
    }

    let flags = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    for invalid_flags in [flags & !(1 << 1), flags | (1 << 5)] {
        let mut malformed = bytes.clone();
        malformed[6..8].copy_from_slice(&invalid_flags.to_le_bytes());
        assert!(matches!(
            Archive::open(Cursor::new(malformed), b"key"),
            Err(Error::UnsupportedFlags(_))
        ));
    }

    let mut outside = bytes;
    outside[28..36].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        Archive::open(Cursor::new(outside), b"key"),
        Err(Error::Corrupt("TOC range overflow"))
    ));
    assert!(Archive::open(Cursor::new(vec![0; 59]), b"key").is_err());
}

#[test]
fn malformed_toc_records_are_rejected() {
    let bytes = archive_bytes(&[InputFile {
        path: "a.bin".into(),
        data: vec![7; 4096],
    }]);

    let bad_magic = mutate_toc(bytes.clone(), |toc| toc[0] ^= 1);
    assert!(matches!(
        Archive::open(Cursor::new(bad_magic), b"key"),
        Err(Error::BadTocMagic)
    ));

    let bad_utf8 = mutate_toc(bytes.clone(), |toc| toc[40] = 0xff);
    assert!(matches!(
        Archive::open(Cursor::new(bad_utf8), b"key"),
        Err(Error::Corrupt("entry path is not UTF-8"))
    ));

    let entry_reserved = mutate_toc(bytes.clone(), |toc| toc[38] = 1);
    assert!(matches!(
        Archive::open(Cursor::new(entry_reserved), b"key"),
        Err(Error::Corrupt("non-zero entry reserved field"))
    ));

    let truncated_path = mutate_toc(bytes.clone(), |toc| {
        toc[36..38].copy_from_slice(&u16::MAX.to_le_bytes())
    });
    assert!(matches!(
        Archive::open(Cursor::new(truncated_path), b"key"),
        Err(Error::Corrupt("truncated TOC"))
    ));

    let chunk_start = 4 + 36 + "a.bin".len();
    let unknown_flags = mutate_toc(bytes.clone(), |toc| toc[chunk_start + 32] = 2);
    assert!(matches!(
        Archive::open(Cursor::new(unknown_flags), b"key"),
        Err(Error::Corrupt("unknown chunk flags"))
    ));

    let chunk_reserved = mutate_toc(bytes.clone(), |toc| toc[chunk_start + 34] = 1);
    assert!(matches!(
        Archive::open(Cursor::new(chunk_reserved), b"key"),
        Err(Error::Corrupt("non-zero chunk reserved field"))
    ));

    let zero_stored_size = mutate_toc(bytes.clone(), |toc| {
        toc[chunk_start + 8..chunk_start + 16].copy_from_slice(&0u64.to_le_bytes())
    });
    assert!(matches!(
        Archive::open(Cursor::new(zero_stored_size), b"key"),
        Err(Error::Corrupt("invalid stored chunk size"))
    ));

    let chunk_in_toc = mutate_toc(bytes, |toc| {
        toc[chunk_start..chunk_start + 8].copy_from_slice(&u64::MAX.to_le_bytes())
    });
    assert!(Archive::open(Cursor::new(chunk_in_toc), b"key").is_err());
}

#[test]
fn malformed_lz4_and_chunk_storage_rules_are_rejected() {
    let bytes = archive_bytes(&[InputFile {
        path: "a.bin".into(),
        data: vec![7; 4096],
    }]);
    let mut malformed_lz4 = bytes.clone();
    let toc_offset = read_u64(&malformed_lz4[28..36]) as usize;
    let toc_size = read_u64(&malformed_lz4[36..44]) as usize;
    malformed_lz4[toc_offset..toc_offset + toc_size].fill(0);
    assert!(Archive::open(Cursor::new(malformed_lz4), b"key").is_err());

    let chunk_start = 4 + 36 + "a.bin".len();
    let non_beneficial = mutate_toc(bytes.clone(), |toc| {
        let original = toc[chunk_start + 16..chunk_start + 24].to_vec();
        toc[chunk_start + 8..chunk_start + 16].copy_from_slice(&original);
    });
    assert!(matches!(
        Archive::open(Cursor::new(non_beneficial), b"key"),
        Err(Error::Corrupt("non-beneficial compressed chunk"))
    ));

    let raw_mismatch = mutate_toc(bytes, |toc| {
        toc[chunk_start + 32..chunk_start + 34].copy_from_slice(&0u16.to_le_bytes())
    });
    assert!(matches!(
        Archive::open(Cursor::new(raw_mismatch), b"key"),
        Err(Error::Corrupt("raw chunk size mismatch"))
    ));
}

#[test]
fn malformed_totals_and_overlapping_chunks_are_rejected() {
    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &[InputFile {
            path: "a.bin".into(),
            data: (0..100).collect(),
        }],
        b"key",
        PackOptions {
            chunk_size: 16,
            ..PackOptions::default()
        },
    )
    .unwrap();
    let bytes = output.into_inner();
    let bad_total = mutate_toc(bytes.clone(), |toc| {
        toc[12..20].copy_from_slice(&99u64.to_le_bytes())
    });
    assert!(matches!(
        Archive::open(Cursor::new(bad_total), b"key"),
        Err(Error::Corrupt("entry original size mismatch"))
    ));

    let chunk_start = 4 + 36 + "a.bin".len();
    let overlap = mutate_toc(bytes, |toc| {
        let first_offset = toc[chunk_start..chunk_start + 8].to_vec();
        toc[chunk_start + 36..chunk_start + 44].copy_from_slice(&first_offset);
    });
    assert!(matches!(
        Archive::open(Cursor::new(overlap), b"key"),
        Err(Error::Corrupt("overlapping or out-of-order chunks"))
    ));

    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &[InputFile {
            path: "a.bin".into(),
            data: (0..100).collect(),
        }],
        b"key",
        PackOptions {
            chunk_size: 16,
            ..PackOptions::default()
        },
    )
    .unwrap();
    let orphan = mutate_toc(output.into_inner(), |toc| {
        let chunk_start = 4 + 36 + "a.bin".len();
        let stored = toc[chunk_start + 8..chunk_start + 16].to_vec();
        let original = toc[chunk_start + 16..chunk_start + 24].to_vec();
        toc[12..20].copy_from_slice(&original);
        toc[20..28].copy_from_slice(&stored);
        toc[32..36].copy_from_slice(&1u32.to_le_bytes());
    });
    assert!(matches!(
        Archive::open(Cursor::new(orphan), b"key"),
        Err(Error::Corrupt("orphan chunk in TOC"))
    ));
}

#[test]
fn duplicate_debug_paths_and_production_hashes_are_rejected() {
    let files = [
        InputFile {
            path: "a.bin".into(),
            data: b"a".to_vec(),
        },
        InputFile {
            path: "b.bin".into(),
            data: b"b".to_vec(),
        },
    ];
    let debug = archive_bytes(&files);
    let duplicate = mutate_toc(debug, |toc| {
        let first_hash = toc[4..12].to_vec();
        let second = 4 + 36 + "a.bin".len();
        toc[second..second + 8].copy_from_slice(&first_hash);
        toc[second + 36..second + 41].copy_from_slice(b"a.bin");
    });
    assert!(matches!(
        Archive::open(Cursor::new(duplicate), b"key"),
        Err(Error::DuplicatePath(path)) if path == "a.bin"
    ));

    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &files,
        b"key",
        PackOptions {
            mode: PackMode::Production,
            ..PackOptions::default()
        },
    )
    .unwrap();
    let collision = mutate_toc(output.into_inner(), |toc| {
        let first_hash = toc[4..12].to_vec();
        toc[40..48].copy_from_slice(&first_hash);
    });
    assert!(matches!(
        Archive::open(Cursor::new(collision), b"key"),
        Err(Error::HashCollision(_))
    ));

    let duplicate_owner = mutate_toc(archive_bytes(&files), |toc| {
        let second_entry = 4 + 36 + "a.bin".len();
        toc[second_entry + 24..second_entry + 28].copy_from_slice(&0u32.to_le_bytes());
    });
    assert!(matches!(
        Archive::open(Cursor::new(duplicate_owner), b"key"),
        Err(Error::Corrupt("chunk is referenced by multiple entries"))
    ));
}

#[test]
fn oversized_identifier_bucket_is_rejected() {
    let files: Vec<_> = (0..=ArchiveLimits::HARD_MAX_IDENTIFIER_BUCKET_ENTRIES)
        .map(|index| InputFile {
            path: format!("{index}.bin"),
            data: Vec::new(),
        })
        .collect();
    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &files,
        b"key",
        PackOptions {
            mode: PackMode::Production,
            ..PackOptions::default()
        },
    )
    .unwrap();
    let collision = mutate_toc(output.into_inner(), |toc| {
        let hash = toc[4..12].to_vec();
        for index in 1..files.len() {
            let start = 4 + index * 36;
            toc[start..start + 8].copy_from_slice(&hash);
        }
    });

    assert!(matches!(
        Archive::open(Cursor::new(collision), b"key"),
        Err(Error::TooLarge("identifier collision bucket"))
    ));
}

#[test]
fn total_decompressed_archive_size_is_bounded() {
    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &[InputFile {
            path: "large.bin".into(),
            data: vec![1; 129],
        }],
        b"key",
        PackOptions {
            chunk_size: 1,
            ..PackOptions::default()
        },
    )
    .unwrap();
    let mut bytes = output.into_inner();
    let chunk_size = 64 * 1024 * 1024u32;
    bytes[16..20].copy_from_slice(&chunk_size.to_le_bytes());
    let oversized = mutate_toc(bytes, |toc| {
        let chunk_start = 4 + 36 + "large.bin".len();
        for index in 0..129 {
            let start = chunk_start + index * 36;
            toc[start + 16..start + 24].copy_from_slice(&u64::from(chunk_size).to_le_bytes());
            toc[start + 32..start + 34].copy_from_slice(&1u16.to_le_bytes());
        }
    });

    assert!(matches!(
        Archive::open(Cursor::new(oversized), b"key"),
        Err(Error::ArchiveTooLarge)
    ));
}

#[test]
fn single_asset_size_is_bounded_before_payload_allocation() {
    let oversized = mutate_toc(
        archive_bytes(&[InputFile {
            path: "large.bin".into(),
            data: vec![1],
        }]),
        |toc| {
            toc[12..20].copy_from_slice(
                &(ArchiveLimits::runtime_default().max_single_asset_bytes + 1).to_le_bytes(),
            );
        },
    );

    assert!(matches!(
        Archive::open(Cursor::new(oversized), b"key"),
        Err(Error::AssetTooLarge)
    ));
}

fn archive_bytes(files: &[InputFile]) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    pack(&mut output, files, b"key").unwrap();
    output.into_inner()
}

fn mutate_toc(mut archive: Vec<u8>, mutate: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let toc_offset = read_u64(&archive[28..36]) as usize;
    let toc_size = read_u64(&archive[36..44]) as usize;
    let raw_size = read_u64(&archive[44..52]) as usize;
    let mut stored = archive[toc_offset..toc_offset + toc_size].to_vec();
    xor(&mut stored, b"key");
    let mut raw = decompress(&stored, raw_size).unwrap();
    mutate(&mut raw);

    archive[52..60].copy_from_slice(&xxh3_64(&raw).to_le_bytes());
    let mut stored = compress(&raw);
    xor(&mut stored, b"key");
    archive[36..44].copy_from_slice(&(stored.len() as u64).to_le_bytes());
    archive.truncate(toc_offset);
    archive.extend_from_slice(&stored);
    archive
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().unwrap())
}

fn xor(bytes: &mut [u8], key: &[u8]) {
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte ^= key[index % key.len()];
    }
}
