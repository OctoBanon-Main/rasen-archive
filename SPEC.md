# RPAK Format Specification

## Overview

RPAK is a binary archive format designed for use by game engines and
game runtime environments.

The format provides:

-   deterministic asset storage;
-   chunk-based asset loading;
-   optional LZ4 compression;
-   integrity verification using non-cryptographic hashes;
-   fast random access through a table of contents.

RPAK does not provide cryptographic security. Checksums and obfuscation
features are not intended for authentication, confidentiality, or
protection against malicious modification.

------------------------------------------------------------------------

# File Structure

An RPAK archive consists of:

1.  Fixed-size header.
2.  Aligned asset chunks.
3.  Table of contents stored at the end of the file.

The current format version uses a fixed-size header. Future versions may
introduce incompatible changes and must use a new version identifier.

------------------------------------------------------------------------

# Header

The header contains:

-   format version;
-   feature flags;
-   chunk configuration;
-   chunk count;
-   TOC location;
-   TOC size information;
-   TOC checksum.

Readers must reject unsupported versions.

------------------------------------------------------------------------

# Asset Storage

Assets are divided into independent chunks.

Each chunk contains:

-   stored data size;
-   original decompressed size;
-   offset;
-   checksum information;
-   compression information.

Chunks are independently stored and decoded after asset metadata has
been resolved from the TOC.

------------------------------------------------------------------------

# Compression

RPAK supports LZ4 compression.

The compression pipeline is:

    raw asset data
            |
            v
    chunk split
            |
            v
    optional LZ4 compression
            |
            v
    stored chunk

Compressed chunks must declare their original size.

Readers must verify that decompressed output matches the expected size.

------------------------------------------------------------------------

# Table of Contents

The TOC stores:

-   asset identifiers;
-   asset paths (when available);
-   chunk references;
-   asset sizes;
-   metadata required for loading.

The TOC storage pipeline is:

    serialize TOC
            |
            v
    calculate XXH3 checksum
            |
            v
    LZ4 compression
            |
            v
    optional XOR obfuscation
            |
            v
    stored TOC

The XOR layer is only a lightweight obfuscation mechanism and is not
cryptographic protection.

------------------------------------------------------------------------

# Asset Identifiers

Asset identifiers are generated from normalized asset paths using
XXH3-64.

Path normalization rules must be identical between packer and reader
implementations.

Production archives may remove stored paths.

The official packer rejects hash collisions when creating production
archives.

The archive format itself does not provide collision resistance
guarantees.

------------------------------------------------------------------------

# Integrity Checking

Checksums are used to detect accidental corruption.

Validation occurs after decoding and before returning data to the
caller.

Checksums do not prevent:

-   malicious archive creation;
-   excessive memory usage;
-   excessive CPU usage;
-   denial of service attacks.

------------------------------------------------------------------------

# Alignment

Asset chunks are aligned according to the archive alignment rules.

Readers must validate alignment requirements before accessing chunk
data.

Invalid alignment must result in archive rejection.

------------------------------------------------------------------------

# Runtime Limits and Validation

Runtime implementations must enforce resource limits before allocating
memory or processing archive data.

Recommended runtime limits:

  Limit                      Purpose
  -------------------------- --------------------------------------
  max_single_asset_bytes     Prevent oversized asset allocations
  max_entries                Prevent excessive metadata usage
  max_chunks                 Prevent excessive chunk tables
  max_metadata_bytes         Bound metadata memory usage
  max_chunks_per_operation   Prevent excessive CPU usage per read

Tooling environments may use higher limits than runtime environments.

Large archives suitable for editors or build systems should not
automatically be considered safe for game runtime loading.

------------------------------------------------------------------------

# Error Handling

Invalid archives must fail gracefully.

Examples of invalid data:

-   unsupported version;
-   invalid offsets;
-   truncated data;
-   invalid chunk references;
-   checksum mismatch;
-   resource limits exceeded.

Archive parsing must never rely on unchecked input data.

Malformed archives must not cause:

-   crashes;
-   panics;
-   out-of-bounds access.

------------------------------------------------------------------------

# Security Considerations

RPAK is designed as an asset container, not as a security boundary.

Implementations must protect against:

-   memory exhaustion;
-   CPU exhaustion;
-   malformed input;
-   invalid integer conversions;
-   invalid offsets.

Cryptographic authentication and confidentiality are intentionally
outside the scope of the format.