use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    BadMagic,
    BadTocMagic,
    UnsupportedVersion(u16),
    UnsupportedFlags(u16),
    UnsupportedHeaderSize(u32),
    EmptyXorKey,
    InvalidPath,
    InvalidChunkSize,
    InvalidAlignment,
    InvalidRange,
    DuplicatePath(String),
    Corrupt(&'static str),
    TooLarge(&'static str),
    Lz4(String),
    NotFound(String),
    HashCollision(u64),
    ChecksumMismatch { path: String, chunk: u32 }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::BadMagic => write!(f, "not a RPAK archive"),
            Self::BadTocMagic => write!(f, "invalid TOC magic (wrong XOR key or corrupt archive)"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported RPAK version {v}"),
            Self::UnsupportedFlags(v) => write!(f, "unsupported flags: 0x{v:04x}"),
            Self::UnsupportedHeaderSize(v) => write!(f, "unsupported header size {v}"),
            Self::EmptyXorKey => write!(f, "XOR key must not be empty"),
            Self::InvalidPath => write!(f, "invalid archive path"),
            Self::InvalidChunkSize => write!(f, "chunk size must be in 1..=64 MiB"),
            Self::InvalidAlignment => {
                write!(f, "alignment must be a power of two in 1..=1 MiB")
            }
            Self::InvalidRange => write!(f, "requested range is outside the asset"),
            Self::DuplicatePath(p) => write!(f, "duplicate archive path: {p}"),
            Self::Corrupt(s) => write!(f, "corrupt archive: {s}"),
            Self::TooLarge(s) => write!(f, "value too large: {s}"),
            Self::Lz4(s) => write!(f, "LZ4 error: {s}"),
            Self::NotFound(p) => write!(f, "entry not found: {p}"),
            Self::HashCollision(hash) => write!(f, "ambiguous asset hash collision: {hash:016x}"),
            Self::ChecksumMismatch { path, chunk } => {
                write!(f, "checksum mismatch: {path}, chunk {chunk}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;