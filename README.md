# Rasen Archive

**RPAK is an archive format and asset packaging library**

It provides fast random access to game assets using chunked LZ4 compression, hashed asset paths, checksums, and an obfuscated table of contents.

## Features

* Independent LZ4-compressed chunks
* Raw storage fallback for incompressible data
* Fast random access to individual assets
* Partial asset reads without decompressing the entire file
* XXH3-64 hashed asset paths
* XXH3-64 chunk checksums
* XXH3-64 TOC checksum
* Configurable chunk size
* Configurable payload alignment
* XOR-obfuscated table of contents
* Path normalization and validation
* Hash collision detection

## Archive Format

An RPAK archive consists of a fixed-size header, aligned asset chunks, and a table of contents stored at the end of the file.

```text
┌──────────────────────────────────────┐
│ RPAK Header                          │
├──────────────────────────────────────┤
│ Asset Chunk 0              RAW / LZ4 │
├──────────────────────────────────────┤
│ Padding                              │
├──────────────────────────────────────┤
│ Asset Chunk 1              RAW / LZ4 │
├──────────────────────────────────────┤
│ ...                                  │
├──────────────────────────────────────┤
│ XOR( LZ4( TOC ) )                    │
└──────────────────────────────────────┘
```

Large assets are split into independent chunks. Each chunk can be read and decompressed separately, allowing the engine to access only the required part of an asset.

If LZ4 compression does not reduce the size of a chunk, the chunk is stored uncompressed.

The table of contents is serialized, compressed using LZ4, and then XOR-obfuscated before being written to the archive.

> XOR is used only for basic TOC obfuscation and should not be considered cryptographic protection.

## Building

### Clone the repository

```bash
git clone https://github.com/OctoBanon-Main/rpak
cd rpak
```

### Build

```bash
cargo build --release
```

### Run tests

```bash
cargo test
```

## Usage

### Pack a directory

```bash
cargo run --release -- pack ./assets ./content.rpak
```

By default, RPAK uses 64 KiB chunks and 16-byte payload alignment.

A custom chunk size and alignment can also be specified:

```bash
cargo run --release -- pack ./assets ./content.rpak 256 4096
```

This example uses 256 KiB chunks and 4096-byte alignment.

### List archive contents

```bash
cargo run --release -- list ./content.rpak
```

### Extract an asset

```bash
cargo run --release -- extract \
    ./content.rpak \
    textures/player.dds \
    ./player.dds
```

## Runtime Usage

Open an archive and read an asset by path:

```rust
use std::{
    fs::File,
    io::BufReader,
};

use rpak::Archive;

const RPAK_KEY: &[u8] = b"example-key";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("content.rpak")?;

    let mut archive = Archive::open(
        BufReader::new(file),
        RPAK_KEY,
    )?;

    let data = archive.read("textures/player.dds")?;

    println!("Loaded {} bytes", data.len());

    Ok(())
}
```

## Asset IDs

Asset paths are normalized and hashed using XXH3-64.

This allows the engine to store a compact `u64` asset identifier instead of performing a string lookup every time an asset is requested.

```rust
use rpak::hash_path;

let asset_id = hash_path("textures/player.dds")?;
```

An asset can then be loaded directly by its hash:

```rust
let data = archive.read_by_hash(asset_id)?;
```

Paths are still stored in the TOC so hash collisions can be detected instead of silently resolving to the wrong asset.

## Streaming

RPAK supports reading individual chunks:

```rust
let chunk = archive.read_chunk(
    "audio/music.pcm",
    10,
)?;
```

It also supports reading arbitrary ranges from an asset:

```rust
let data = archive.read_range(
    "textures/world.vt",
    512 * 1024,
    128 * 1024,
)?;
```

Only chunks intersecting the requested range are read and decompressed.

This is useful for large resources such as:

* Audio streams
* Virtual textures
* Large world data
* Precomputed geometry
* Other streamable game assets

## Integrity

Every asset chunk contains an XXH3-64 checksum calculated from its original uncompressed data.

The table of contents also has its own XXH3-64 checksum.

Checksums are validated when data is loaded, allowing corrupted archive data to be detected before it is passed to the engine.

## Project Structure

```text
src/
├── lib.rs              # Public API and re-exports
├── archive/
│   └── mod.rs          # Archive reader and runtime access
├── pack/
│   ├── mod.rs          # Archive creation
│   └── options.rs      # Packing options
├── format/
│   ├── mod.rs          # Format constants and validation
│   ├── header.rs       # Header encoding and decoding
│   ├── toc.rs          # TOC encoding, decoding and layout validation
│   ├── model.rs        # Internal on-disk models
│   └── io.rs           # Binary I/O and alignment helpers
├── codec.rs            # LZ4, XXH3 and XOR helpers
├── path.rs             # Path normalization and hashing
├── error.rs            # Error types
└── bin/
    └── rpak.rs         # Command-line interface

tests/
├── roundtrip.rs        # Public API round-trip tests
└── corruption.rs       # Corruption and wrong-key tests
```

## License

See the `LICENSE` file for licensing information.