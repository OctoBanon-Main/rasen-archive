use std::io::Cursor;

use rasen_archive::{Archive, Error, InputFile, PackOptions, Protection, pack_with_options};

#[test]
fn roundtrip_chunked_random_access_and_range() {
    let key = b"key-1234";
    let files = vec![
        InputFile {
            path: "textures/hero.txt".into(),
            data: vec![b'A'; 180_000],
        },
        InputFile {
            path: "audio/raw.bin".into(),
            data: (0..=255).cycle().take(90_000).collect(),
        },
        InputFile {
            path: "empty.bin".into(),
            data: Vec::new(),
        },
    ];

    let mut out = Cursor::new(Vec::new());
    pack_with_options(
        &mut out,
        &files,
        key,
        PackOptions {
            chunk_size: 32 * 1024,
            alignment: 64,
            ..PackOptions::default()
        },
    )
    .unwrap();

    out.set_position(0);
    let archive = Archive::open(out, key).unwrap();
    assert_eq!(archive.chunk_size(), 32 * 1024);
    assert_eq!(archive.alignment(), 64);
    assert_eq!(archive.read("textures/hero.txt").unwrap(), files[0].data);
    assert_eq!(archive.read("audio/raw.bin").unwrap(), files[1].data);
    assert_eq!(archive.read("empty.bin").unwrap(), Vec::<u8>::new());
    assert!(archive.contains("textures\\hero.txt"));

    let range = archive.read_range("audio/raw.bin", 31_900, 3_000).unwrap();
    assert_eq!(range, files[1].data[31_900..34_900]);

    let chunk = archive.read_chunk("textures/hero.txt", 2).unwrap();
    assert_eq!(chunk, files[0].data[65_536..98_304]);
}

#[test]
fn production_mode_strips_paths_and_keeps_hash_lookup() {
    use rasen_archive::{PackMode, hash_path};

    let key = b"prod-key";
    let files = vec![
        InputFile {
            path: "textures/player.dds".into(),
            data: vec![7; 8_192],
        },
        InputFile {
            path: "audio/theme.ogg".into(),
            data: vec![3; 4_096],
        },
    ];

    let mut out = Cursor::new(Vec::new());
    pack_with_options(
        &mut out,
        &files,
        key,
        PackOptions {
            mode: PackMode::Production,
            ..PackOptions::default()
        },
    )
    .unwrap();

    out.set_position(0);
    let archive = Archive::open(out, key).unwrap();
    assert!(archive.paths_stripped());
    assert!(archive.entries().iter().all(|entry| entry.path().is_none()));
    assert_eq!(archive.read("textures/player.dds").unwrap(), files[0].data);

    let asset_id = hash_path("audio/theme.ogg").unwrap();
    assert_eq!(archive.read_by_hash(asset_id).unwrap(), files[1].data);
}

#[test]
fn debug_mode_keeps_paths() {
    let key = b"debug-key";
    let files = vec![InputFile {
        path: "textures/player.dds".into(),
        data: b"debug asset".to_vec(),
    }];

    let mut out = Cursor::new(Vec::new());
    pack_with_options(&mut out, &files, key, PackOptions::default()).unwrap();
    out.set_position(0);

    let archive = Archive::open(out, key).unwrap();
    assert!(!archive.paths_stripped());
    assert_eq!(archive.entries()[0].path, "textures/player.dds");
}

#[test]
fn aead_authenticates_toc_and_chunks() {
    let key = b"correct horse battery staple";
    let mut seed = 0x1234_5678_9abc_def0u64;
    let noise = (0..4096)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    let files = vec![
        InputFile {
            path: "secure.bin".into(),
            data: vec![b'A'; 4096],
        },
        InputFile {
            path: "noise.bin".into(),
            data: noise,
        },
    ];
    let options = PackOptions {
        chunk_size: 1024,
        protection: Protection::Aead,
        ..PackOptions::default()
    };
    let mut output = Cursor::new(Vec::new());
    pack_with_options(&mut output, &files, key, options).unwrap();
    let bytes = output.into_inner();

    let archive = Archive::open(Cursor::new(bytes.clone()), key).unwrap();
    assert_eq!(archive.protection(), Protection::Aead);
    assert_eq!(archive.read("secure.bin").unwrap(), files[0].data);
    assert_eq!(archive.read("noise.bin").unwrap(), files[1].data);
    assert!(matches!(
        Archive::open(Cursor::new(bytes.clone()), b"wrong key"),
        Err(Error::Crypto("authentication failed"))
    ));

    let toc_offset = u64::from_le_bytes(bytes[28..36].try_into().unwrap()) as usize;
    let mut bad_toc = bytes.clone();
    bad_toc[toc_offset + 24] ^= 1;
    assert!(matches!(
        Archive::open(Cursor::new(bad_toc), key),
        Err(Error::Crypto("authentication failed"))
    ));

    let chunk_offset = usize::try_from(rasen_archive::HEADER_SIZE.next_multiple_of(16)).unwrap();
    let mut bad_chunk = bytes;
    bad_chunk[chunk_offset + 24] ^= 1;
    let archive = Archive::open(Cursor::new(bad_chunk), key).unwrap();
    assert!(matches!(
        archive.read("secure.bin"),
        Err(Error::Crypto("authentication failed"))
    ));
}
