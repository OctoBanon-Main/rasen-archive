use std::io::Cursor;

use rasen_archive::{Archive, InputFile, PackOptions, VERSION, pack_with_options};

// Produced by the pre-refactor v1 writer with 1 KiB chunks and 16-byte alignment.
const GOLDEN_V1: &[u8] = &[
    82, 80, 65, 75, 1, 0, 15, 0, 60, 0, 0, 0, 16, 0, 0, 0, 0, 4, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 80,
    0, 0, 0, 0, 0, 0, 0, 94, 0, 0, 0, 0, 0, 0, 0, 134, 0, 0, 0, 0, 0, 0, 0, 58, 125, 114, 65, 193,
    23, 20, 38, 0, 0, 0, 0, 103, 111, 108, 100, 101, 110, 32, 118, 49, 10, 0, 0, 0, 0, 0, 0, 186,
    44, 46, 46, 66, 70, 32, 153, 126, 239, 89, 121, 181, 97, 108, 112, 104, 150, 38, 102, 101, 121,
    101, 28, 8, 31, 95, 9, 8, 93, 31, 28, 87, 7, 17, 15, 141, 133, 130, 166, 155, 230, 62, 81, 111,
    73, 97, 105, 120, 108, 101, 47, 107, 149, 122, 100, 120, 97, 109, 121, 108, 101, 45, 3, 0, 21,
    9, 23, 79, 25, 8, 24, 37, 59, 107, 98, 92, 101, 124, 76, 109, 176, 107, 38, 152, 125, 149, 2,
    9, 133, 97, 109, 112, 108,
];

#[test]
fn pre_refactor_v1_archive_still_opens() {
    assert_eq!(VERSION, 1);
    let archive = Archive::open(Cursor::new(GOLDEN_V1), b"example-key").unwrap();
    assert_eq!(archive.entries().len(), 2);
    assert_eq!(archive.read("dir/empty.bin").unwrap(), b"");
    assert_eq!(archive.read("hello.txt").unwrap(), b"golden v1\n");
}

#[test]
fn new_writer_matches_pre_refactor_v1_bytes() {
    let files = [
        InputFile {
            path: "dir/empty.bin".into(),
            data: Vec::new(),
        },
        InputFile {
            path: "hello.txt".into(),
            data: b"golden v1\n".to_vec(),
        },
    ];
    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &files,
        b"example-key",
        PackOptions {
            chunk_size: 1024,
            alignment: 16,
            ..PackOptions::default()
        },
    )
    .unwrap();
    assert_eq!(output.into_inner(), GOLDEN_V1);
}
