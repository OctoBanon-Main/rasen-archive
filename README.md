# Rasen Archive

Rasen Archive is a synchronous Rust library and CLI for RPAK
game/runtime asset archives.

RPAK v1 stores independently readable RAW/LZ4 chunks, an LZ4-compressed
table of contents, XXH3-64 non-cryptographic corruption detection
checks, normalized path hashes, and selectable XOR obfuscation or
XChaCha20-Poly1305 authenticated encryption. See [SPEC.md](./SPEC.md)
for wire details.

## Properties

-   Streaming packing with memory bounded by configured chunk size plus
    retained archive metadata
-   Concurrent positioned reads from one shared file-backed archive
-   Full, chunk, clipped-range, exact-range, and destination-buffer
    reads
-   Configurable archive-open resource limits
-   Debug archives may include paths; production archives may omit paths
    and store hash-only entries
-   Optional AEAD protection for both TOC and chunks, with BLAKE3 key
    derivation
-   Deterministic CLI ordering by normalized UTF-8 virtual path
-   Failure-safe same-directory temporary output replacement
-   Compatibility with the current RPAK v1 on-disk format

## Build And Check

Workspace layout:

-   `crates/rasen-archive`: format, pack/read APIs, tests, and benchmarks
-   `crates/rasen-packer`: Clap-based CLI packer and archive inspector

``` bash
cargo build --workspace --release
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## CLI

Pack a directory with default 64 KiB chunks, 16-byte alignment, debug
paths, and XOR protection:

``` bash
cargo run -p rasen-packer --release -- pack ./assets ./content.rpak
```

Production mode strips paths. Packing options use named values:

``` bash
cargo run -p rasen-packer --release -- pack ./assets ./content.rpak --mode=production --protection=aead --chunk-kib=256 --alignment=4096 --key=project-key
```

List and extract:

``` bash
cargo run -p rasen-packer --release -- list ./content.rpak
cargo run -p rasen-packer --release -- extract ./content.rpak textures/player.dds ./player.dds
```

All commands accept `--key <value>` or `--key=<value>`. `RPAK_KEY` is
used when the option is absent. The `example-key` default remains for
compatibility.

The default key exists only for compatibility and testing. It must not
be considered a secret or used as protection for shipped content.

``` bash
cargo run -p rasen-packer --release -- list ./content.rpak --key project-key
```

The same key used for packing is needed for reading. XOR protects only
the TOC and remains obfuscation without confidentiality or authentication.
AEAD protects the TOC and every chunk with XChaCha20-Poly1305; BLAKE3
derives its 256-bit key from supplied key material. This is not a
password-hard KDF, so use high-entropy key material. A key embedded in a
client executable is not secret.

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
use rasen_archive::{PackOptions, Packer, Protection};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = BufWriter::new(File::create("content.rpak.tmp")?);
    let options = PackOptions {
        protection: Protection::Aead,
        ..PackOptions::default()
    };
    let mut packer = Packer::new(&mut output, b"project-key", options)?;
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

AEAD archives provide confidentiality and authentication when key
material remains secret. Archive parsing still treats all input as
untrusted.

Implementations must protect against:

-   memory exhaustion;
-   CPU exhaustion;
-   malformed archive input;
-   invalid integer conversions;
-   invalid offsets;
-   unexpected resource usage.

## Fuzzing And Benchmarks

``` bash
cargo fuzz run archive_open -- -max_total_time=60
cargo bench -p rasen-archive --bench scaling
RPAK_BENCH_LARGE=1 cargo bench -p rasen-archive --bench scaling
```

Fuzz targets must never panic when processing malformed archive input.

Large benchmark mode adds the optional one-million-entry open case.

## License

See [LICENSE](./LICENSE).
