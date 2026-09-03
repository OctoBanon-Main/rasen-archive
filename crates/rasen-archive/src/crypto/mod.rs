mod aead;
mod xor;

pub(crate) use aead::{AEAD_OVERHEAD, TOC_AAD, chunk_aad, decrypt, derive_key, encrypt};
pub(crate) use xor::xor_in_place;
