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
* Debug/production packing modes
* Production TOC path stripping (hash-only asset identifiers)
* Hash collision detection

For archive layout and format details, see [SPEC.md](./SPEC.md).

## Building

### Clone the repository

```bash
git clone https://github.com/OctoBanon-Main/rasen-archive
cd rasen-archive
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

### Debug vs production archives

Debug mode is the default and keeps normalized asset paths in the TOC:

```bash
cargo run --release -- pack ./assets ./content-debug.rpak --debug
```

Production mode strips path strings from the TOC and stores only their XXH3-64 hashes:

```bash
cargo run --release -- pack ./assets ./content.rpak --production
```

`--prod` and `--mode=production` are accepted aliases. Chunk size and alignment can still be combined with the mode switch:

```bash
cargo run --release -- pack ./assets ./content.rpak 256 4096 --production
```

Production archives can still be read with `archive.read("path/to/asset")`: the requested path is normalized and hashed at runtime. Because the original path strings are not present, production packing rejects hash collisions instead of relying on path strings to disambiguate them.

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

use rasen_archive::Archive;

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

Additional runtime details such as hash-based asset IDs and partial reads are documented in [SPEC.md](./SPEC.md).

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
    └── rasen-pack.rs         # Command-line interface

tests/
├── roundtrip.rs        # Public API round-trip tests
└── corruption.rs       # Corruption and wrong-key tests
```

## License

See the `LICENSE` file for licensing information.