use xxhash_rust::xxh3::xxh3_64;

use crate::{
    error::{Error, Result},
    format::MAX_PATH_LEN
};

pub fn hash_path(path: &str) -> Result<u64> {
    let normalized = normalize_path(path)?;
    Ok(xxh3_64(normalized.as_bytes()))
}

pub(crate) fn normalize_lookup_path(path: &str) -> Result<String> {
    normalize_path(path)
}

pub(crate) fn normalize_path(path: &str) -> Result<String> {
    if path.is_empty() || path.len() > MAX_PATH_LEN || path.as_bytes().contains(&0) {
        return Err(Error::InvalidPath);
    }

    let path = path.replace('\\', "/");
    if path.starts_with('/') {
        return Err(Error::InvalidPath);
    }

    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(Error::InvalidPath),
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(Error::InvalidPath);
    }
    Ok(parts.join("/"))
}