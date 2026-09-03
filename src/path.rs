use xxhash_rust::xxh3::xxh3_64;

use crate::{
    error::{Error, Result},
    format::MAX_PATH_LEN,
};

/// Maximum number of components inspected while normalizing one path.
const MAX_PATH_SEGMENTS: usize = 1024;

#[repr(transparent)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AssetId(u64);

impl AssetId {
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn from_path(path: &str) -> Result<Self> {
        hash_path(path).map(Self)
    }
}

impl From<u64> for AssetId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<AssetId> for u64 {
    fn from(value: AssetId) -> Self {
        value.0
    }
}

pub fn hash_path(path: &str) -> Result<u64> {
    let normalized = normalize_path(path)?;
    Ok(xxh3_64(normalized.as_bytes()))
}

pub(crate) fn normalize_lookup_path(path: &str) -> Result<String> {
    normalize_path(path)
}

pub fn normalize_path(path: &str) -> Result<String> {
    (path.len() <= MAX_PATH_LEN && !path.as_bytes().contains(&0))
        .then_some(())
        .ok_or(Error::InvalidPath)?;
    (!path.starts_with(['/', '\\']))
        .then_some(())
        .ok_or(Error::InvalidPath)?;

    let mut normalized = String::new();
    normalized
        .try_reserve(path.len())
        .map_err(|_| Error::TooLarge("normalized path allocation"))?;
    for (index, part) in path.split(['/', '\\']).enumerate() {
        (index < MAX_PATH_SEGMENTS)
            .then_some(())
            .ok_or(Error::InvalidPath)?;
        match part {
            "" | "." => {}
            ".." => return Err(Error::InvalidPath),
            _ => {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push_str(part);
            }
        }
    }
    (!normalized.is_empty() && normalized.len() <= MAX_PATH_LEN)
        .then_some(normalized)
        .ok_or(Error::InvalidPath)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_rules_and_length_apply_to_result() {
        for invalid in ["", ".", "././", "..", "a/../b", "/a", "\\a", "a\0b"] {
            assert!(matches!(normalize_path(invalid), Err(Error::InvalidPath)));
        }

        assert_eq!(normalize_path("a//b///c").unwrap(), "a/b/c");
        assert_eq!(normalize_path("./a/./b/.").unwrap(), "a/b");
        assert_eq!(normalize_path("a\\b\\c").unwrap(), "a/b/c");

        let maximum = "a".repeat(MAX_PATH_LEN);
        assert_eq!(normalize_path(&maximum).unwrap(), maximum);
        assert!(matches!(
            normalize_path(&"a".repeat(MAX_PATH_LEN + 1)),
            Err(Error::InvalidPath)
        ));

        let oversized_input = format!("{}asset", "./".repeat(MAX_PATH_LEN));
        assert!(matches!(
            normalize_path(&oversized_input),
            Err(Error::InvalidPath)
        ));

        let too_many_segments = format!("{}/asset", "./".repeat(MAX_PATH_SEGMENTS));
        assert!(matches!(
            normalize_path(&too_many_segments),
            Err(Error::InvalidPath)
        ));
        let too_many_separators = format!("asset{}", "/".repeat(MAX_PATH_SEGMENTS));
        assert!(matches!(
            normalize_path(&too_many_separators),
            Err(Error::InvalidPath)
        ));
    }

    #[test]
    fn normalization_is_idempotent() {
        for path in ["a", "a//b", "./a/b/.", "a\\b", "a/b/c"] {
            let normalized = normalize_path(path).unwrap();
            assert_eq!(normalize_path(&normalized).unwrap(), normalized);
        }
    }
}
