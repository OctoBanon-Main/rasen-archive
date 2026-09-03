use std::fmt;

use crate::{
    error::Result,
    format::{DEFAULT_ALIGNMENT, DEFAULT_CHUNK_SIZE, validate_alignment, validate_chunk_size},
};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum PackMode {
    #[default]
    Debug,
    Production,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum Protection {
    #[default]
    Xor,
    Aead,
}

impl fmt::Display for Protection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Xor => "xor",
            Self::Aead => "aead",
        })
    }
}

impl PackMode {
    pub(crate) fn strips_paths(self) -> bool {
        matches!(self, Self::Production)
    }
}

impl fmt::Display for PackMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Debug => "debug",
            Self::Production => "production",
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct PackOptions {
    pub chunk_size: usize,
    pub alignment: u32,
    pub mode: PackMode,
    pub protection: Protection,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            alignment: DEFAULT_ALIGNMENT,
            mode: PackMode::Debug,
            protection: Protection::Xor,
        }
    }
}

impl PackOptions {
    pub(crate) fn validate(self) -> Result<Self> {
        validate_chunk_size(self.chunk_size)?;
        validate_alignment(self.alignment)?;
        Ok(self)
    }
}
