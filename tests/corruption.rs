use std::io::Cursor;

use rasen_archive::{Archive, Error, InputFile, pack};

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

    bytes[64] ^= 0x55;
    let mut corrupted = Archive::open(Cursor::new(bytes), key).unwrap();
    assert!(matches!(
        corrupted.read("noise.bin"),
        Err(Error::ChecksumMismatch { .. })
    ));
}
