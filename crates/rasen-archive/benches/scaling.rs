use std::{
    hint::black_box,
    io::Cursor,
    time::{Duration, Instant},
};

use rasen_archive::{Archive, InputFile, PackOptions, pack_with_options};

fn main() {
    println!("RPAK scaling benchmark ({})", std::env::consts::ARCH);
    for entries in [10_000, 100_000] {
        benchmark_open(entries);
    }
    if std::env::var_os("RPAK_BENCH_LARGE").is_some() {
        benchmark_open(1_000_000);
    }
    benchmark_reads();
}

fn benchmark_open(count: usize) {
    let files: Vec<_> = (0..count)
        .map(|index| InputFile {
            path: format!("assets/{index:07}.bin"),
            data: vec![(index % 251) as u8; 32],
        })
        .collect();
    let mut output = Cursor::new(Vec::new());
    let pack_start = Instant::now();
    pack_with_options(&mut output, &files, b"bench-key", PackOptions::default()).unwrap();
    let pack_time = pack_start.elapsed();
    let bytes = output.into_inner();
    let open_time = median(5, || {
        black_box(Archive::open(Cursor::new(bytes.as_slice()), b"bench-key").unwrap());
    });
    println!(
        "entries={count:>7} archive={:>9} bytes pack={pack_time:?} open_median={open_time:?}",
        bytes.len()
    );
}

fn benchmark_reads() {
    let data: Vec<_> = (0..64 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    let mut output = Cursor::new(Vec::new());
    pack_with_options(
        &mut output,
        &[InputFile {
            path: "large.bin".into(),
            data,
        }],
        b"bench-key",
        PackOptions::default(),
    )
    .unwrap();
    let archive = Archive::open(Cursor::new(output.into_inner()), b"bench-key").unwrap();
    let ranges = median(5, || {
        for index in 0..1000 {
            black_box(
                archive
                    .read_range("large.bin", ((index * 7919) % 60_000_000) as u64, 4096)
                    .unwrap(),
            );
        }
    });
    println!("1000 random 4 KiB ranges median={ranges:?}");
}

fn median(rounds: usize, mut operation: impl FnMut()) -> Duration {
    let mut times = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let start = Instant::now();
        operation();
        times.push(start.elapsed());
    }
    times.sort_unstable();
    times[times.len() / 2]
}
