use crate::{
    error::Result,
    format::{DEFAULT_ALIGNMENT, DEFAULT_CHUNK_SIZE, validate_alignment, validate_chunk_size},
};

#[derive(Debug, Copy, Clone)]
pub struct PackOptions {
    pub chunk_size: usize,
    pub alignment: u32,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            chunk_size: DEFAULT_CHUNK_SIZE,
            alignment: DEFAULT_ALIGNMENT,
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
