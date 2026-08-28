# RPAK Specification

This document describes the RPAK archive layout and runtime semantics.

For build instructions and usage examples, see [README.md](./README.md).

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

## Asset IDs

Asset paths are normalized and hashed using XXH3-64.

This allows the engine to store a compact `u64` asset identifier instead of performing a string lookup every time an asset is requested.

```rust
use rasen_archive::hash_path;

let asset_id = hash_path("textures/player.dds")?;
```

An asset can then be loaded directly by its hash:

```rust
let data = archive.read_by_hash(asset_id)?;
```

In debug archives, paths are stored in the TOC so hash collisions can be disambiguated. In production archives, paths are stripped; the packer therefore rejects any hash collision before writing the archive.

When packing through the library, select the mode through `PackOptions`:

```rust
use rasen_archive::{PackMode, PackOptions};

let options = PackOptions {
    mode: PackMode::Production,
    ..PackOptions::default()
};
```

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