# Rasen Archive

Rasen Archive is a synchronous Rust library and CLI for RPAK
game/runtime asset archives.

RPAK v1 stores independently readable RAW/LZ4 chunks, an LZ4-compressed
table of contents, XXH3-64 non-cryptographic corruption detection
checks, and normalized path hashes. See [SPEC.md](./SPEC.md) for wire
details.

## Properties

-   Streaming packing with memory bounded by configured chunk size plus
    retained archive metadata
-   Concurrent positioned reads from one shared file-backed archive
-   Full, chunk, clipped-range, exact-range, and destination-buffer
    reads
-   Configurable archive-open resource limits
-   Debug archives may include paths; production archives may omit paths
    and store hash-only entries
-   Deterministic CLI ordering by normalized UTF-8 virtual path
-   Failure-safe same-directory temporary output replacement
-   Compatibility with the current RPAK v1 on-disk format

## Build And Check

``` bash
cargo build --release
cargo test --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## CLI

Pack a directory with default 64 KiB chunks, 16-byte alignment, and
debug paths:

``` bash
cargo run --release -- pack ./assets ./content.rpak
```

Production mode strips paths. Optional positional values set chunk KiB
and alignment:

``` bash
cargo run --release -- pack ./assets ./content.rpak 256 4096 --production
```

List and extract:

``` bash
cargo run --release -- list ./content.rpak
cargo run --release -- extract ./content.rpak textures/player.dds ./player.dds
```

All commands accept `--key <value>` or `--key=<value>`. `RPAK_XOR_KEY`
is used when the option is absent. The compatibility default is
`example-key`.

The default key exists only for compatibility and testing. It must not
be considered a secret or used as protection for shipped content.

``` bash
cargo run --release -- list ./content.rpak --key project-key
```

The same key used for packing is needed to decode the TOC. XOR is
obfuscation, not encryption: it provides no confidentiality or
authentication, and a key embedded in a client executable is not secret.
XXH3 provides non-cryptographic corruption detection only; an attacker
can modify data and recompute checksums.

CLI discovery does not follow symlinks. Filesystem names must be valid
UTF-8. Existing destination and temporary output files are excluded when
output is inside the input tree.

## Runtime

`File` uses platform positioned I/O, so reads take `&self` and one
archive can be shared between threads without a seek-cursor lock.

``` rust
use std::{fs::File, sync::Arc};
use rasen_archive::{Archive, AssetId};

const KEY: &[u8] = b"project-key";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let archive = Arc::new(Archive::open(File::open("content.rpak")?, KEY)?);
    let full = archive.read("textures/player.dds")?;
    let range = archive.read_range("audio/music.pcm", 64 * 1024, 4096)?;
    let id = AssetId::from_path("textures/player.dds")?;
    let same = archive.read_by_id(id)?;
    assert_eq!(full, same);
    assert!(!range.is_empty());
    Ok(())
}
```

Use `Archive::open_with_limits` for application-specific hostile-input
limits. `Archive::open` uses `ArchiveLimits::runtime_default()`; tooling
can opt into `ArchiveLimits::tooling_default()`.
`ArchiveLimits::permissive_v1()` remains an alias for format-maximal
tooling limits.

Archives loaded from untrusted sources should use explicit limits
appropriate for the application.

`Cursor<Vec<u8>>`, `Cursor<&[u8]>`, and `BufReader<File>` remain
supported sources.

`read_into`, `read_chunk_into`, and `read_range_with_scratch` avoid
repeated output or scratch allocation. `read_range` clips at EOF for
compatibility. `read_range_exact` rejects a request that extends past
EOF.

## Streaming Packing

`Packer` opens and consumes one reader at a time. Short reads are
accumulated until a chunk is full or EOF is reached.

Packing is streaming, but archive metadata required for finalization is
retained until the archive is completed.

``` rust
use std::{fs::File, io::BufWriter};
use rasen_archive::{PackOptions, Packer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = BufWriter::new(File::create("content.rpak.tmp")?);
    let mut packer = Packer::new(&mut output, b"project-key", PackOptions::default())?;
    let mut source = File::open("assets/textures/player.dds")?;
    packer.add_reader("textures/player.dds", &mut source)?;
    let summary = packer.finish()?;
    println!("{} bytes", summary.archive_len);
    Ok(())
}
```

Generic packing destinations must be empty. This prevents a shorter
repack from silently leaving stale trailing bytes. Existing `InputFile`,
`pack`, and `pack_with_options` APIs remain as buffered compatibility
wrappers over `Packer`.

## Security

RPAK is an asset container format, not a security boundary.

Implementations must protect against:

-   memory exhaustion;
-   CPU exhaustion;
-   malformed archive input;
-   invalid integer conversions;
-   invalid offsets;
-   unexpected resource usage.

Cryptographic authentication and confidentiality are intentionally
outside the scope of the format.

See `SPEC.md` for wire-format rules and `SECURITY.md` for threat model
details.

## Fuzzing And Benchmarks

``` bash
cargo fuzz run archive_open -- -max_total_time=60
cargo bench --bench scaling
RPAK_BENCH_LARGE=1 cargo bench --bench scaling
```

Fuzz targets must never panic when processing malformed archive input.

Large benchmark mode adds the optional one-million-entry open case.

## License

See [LICENSE](./LICENSE).