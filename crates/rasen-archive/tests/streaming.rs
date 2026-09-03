use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use rasen_archive::{Archive, Error, InputFile, PackOptions, Packer, pack, pack_with_options};

struct ShortReader {
    data: Vec<u8>,
    position: usize,
    max_read: usize,
}

impl Read for ShortReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let amount = output
            .len()
            .min(self.max_read)
            .min(self.data.len() - self.position);
        output[..amount].copy_from_slice(&self.data[self.position..self.position + amount]);
        self.position += amount;
        Ok(amount)
    }
}

struct GeneratedReader {
    remaining: usize,
    position: usize,
    largest_request: usize,
}

impl Read for GeneratedReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.largest_request = self.largest_request.max(output.len());
        let amount = output.len().min(self.remaining).min(17);
        for byte in &mut output[..amount] {
            *byte = (self.position % 251) as u8;
            self.position += 1;
        }
        self.remaining -= amount;
        Ok(amount)
    }
}

struct FailingReader {
    reads: usize,
}

impl Read for FailingReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.reads == 3 {
            return Err(io::Error::other("injected read failure"));
        }
        self.reads += 1;
        output[..4].fill(7);
        Ok(4)
    }
}

#[derive(Default)]
struct SeekCounter {
    inner: Cursor<Vec<u8>>,
    seeks: usize,
}

impl Write for SeekCounter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.inner.write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for SeekCounter {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.seeks += 1;
        self.inner.seek(position)
    }
}

#[test]
fn short_reads_fill_chunks_and_roundtrip_boundaries() {
    let data: Vec<_> = (0..65_537).map(|value| (value % 251) as u8).collect();
    let mut source = ShortReader {
        data: data.clone(),
        position: 0,
        max_read: 3,
    };
    let mut output = Cursor::new(Vec::new());
    let mut packer = Packer::new(&mut output, b"key", PackOptions::default()).unwrap();
    packer.add_reader("large.bin", &mut source).unwrap();
    packer.add_reader("empty.bin", &mut io::empty()).unwrap();
    packer.finish().unwrap();

    let archive = Archive::open(Cursor::new(output.into_inner()), b"key").unwrap();
    assert_eq!(archive.read("large.bin").unwrap(), data);
    assert_eq!(archive.read("empty.bin").unwrap(), b"");
}

#[test]
fn generated_source_stays_chunk_bounded() {
    let size = 2 * 1024 * 1024 + 1;
    let mut source = GeneratedReader {
        remaining: size,
        position: 0,
        largest_request: 0,
    };
    let options = PackOptions {
        chunk_size: 4096,
        ..PackOptions::default()
    };
    let mut output = Cursor::new(Vec::new());
    let mut packer = Packer::new(&mut output, b"key", options).unwrap();
    packer.add_reader("generated.bin", &mut source).unwrap();
    packer.finish().unwrap();
    assert!(source.largest_request <= options.chunk_size);

    let archive = Archive::open(Cursor::new(output.into_inner()), b"key").unwrap();
    let data = archive.read("generated.bin").unwrap();
    assert_eq!(data.len(), size);
    assert!(
        data.iter()
            .enumerate()
            .all(|(index, &byte)| byte == (index % 251) as u8)
    );
}

#[test]
fn reader_failure_is_returned() {
    let mut output = Cursor::new(Vec::new());
    let mut packer = Packer::new(
        &mut output,
        b"key",
        PackOptions {
            chunk_size: 8,
            ..PackOptions::default()
        },
    )
    .unwrap();
    let error = packer
        .add_reader("broken.bin", &mut FailingReader { reads: 0 })
        .unwrap_err();
    assert!(matches!(error, Error::Io(_)));
    assert!(matches!(packer.finish(), Err(Error::IncompletePack)));
}

#[test]
fn seek_count_is_constant_across_chunk_counts() {
    fn seek_count(size: usize) -> usize {
        let mut output = SeekCounter::default();
        let mut packer = Packer::new(
            &mut output,
            b"key",
            PackOptions {
                chunk_size: 1024,
                ..PackOptions::default()
            },
        )
        .unwrap();
        packer
            .add_reader("asset.bin", &mut io::repeat(1).take(size as u64))
            .unwrap();
        packer.finish().unwrap();
        output.seeks
    }

    assert_eq!(seek_count(1), seek_count(1024 * 100));
}

#[test]
fn generic_pack_rejects_non_empty_destination() {
    let mut output = Cursor::new(vec![1, 2, 3]);
    assert!(matches!(
        pack(&mut output, &[], b"key"),
        Err(Error::NonEmptyDestination)
    ));
}

#[test]
fn compatibility_wrappers_still_roundtrip() {
    let files = [InputFile {
        path: "asset.bin".into(),
        data: vec![42; 4097],
    }];
    let mut output = Cursor::new(Vec::new());
    pack_with_options(&mut output, &files, b"key", PackOptions::default()).unwrap();
    let archive = Archive::open(Cursor::new(output.into_inner()), b"key").unwrap();
    assert_eq!(archive.read("asset.bin").unwrap(), files[0].data);
}
