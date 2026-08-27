use std::io::Cursor as IoCursor;

use crate::{Archive, Error, InputFile, PackOptions, pack, pack_with_options};

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

    let mut out = IoCursor::new(Vec::new());
    pack_with_options(
        &mut out,
        &files,
        key,
        PackOptions {
            chunk_size: 32 * 1024,
            alignment: 64,
        },
    )
    .unwrap();

    out.set_position(0);
    let mut archive = Archive::open(out, key).unwrap();
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
fn wrong_xor_key_fails() {
    let files = vec![InputFile {
        path: "a.txt".into(),
        data: b"hello hello hello".to_vec(),
    }];
    let mut out = IoCursor::new(Vec::new());
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
    let mut out = IoCursor::new(Vec::new());
    pack(&mut out, &files, key).unwrap();
    let mut bytes = out.into_inner();

    bytes[64] ^= 0x55;
    let mut corrupted = Archive::open(IoCursor::new(bytes), key).unwrap();
    assert!(matches!(
        corrupted.read("noise.bin"),
        Err(Error::ChecksumMismatch { .. })
    ));
}
