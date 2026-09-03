use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};

use crate::error::{Error, Result};

pub(crate) const AEAD_OVERHEAD: u64 = 24 + 16;
pub(crate) const TOC_AAD: &[u8] = b"RPAK-TOC-v1";

const KEY_CONTEXT: &str = "rasen-archive XChaCha20-Poly1305 key v1";

pub(crate) fn derive_key(key_material: &[u8]) -> [u8; 32] {
    blake3::derive_key(KEY_CONTEXT, key_material)
}

pub(crate) fn chunk_aad(index: u32) -> [u8; 12] {
    let mut aad = *b"RPAK-CHN\0\0\0\0";
    aad[8..].copy_from_slice(&index.to_le_bytes());
    aad
}

pub(crate) fn encrypt(plain: &[u8], key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    let mut nonce_bytes = [0u8; 24];
    getrandom::fill(&mut nonce_bytes).map_err(|_| Error::Crypto("nonce generation failed"))?;
    let nonce = XNonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, Payload { msg: plain, aad })
        .map_err(|_| Error::Crypto("encryption failed"))?;

    let mut stored = Vec::new();
    stored
        .try_reserve_exact(nonce_bytes.len() + ciphertext.len())
        .map_err(|_| Error::TooLarge("encrypted block allocation"))?;
    stored.extend_from_slice(&nonce_bytes);
    stored.extend_from_slice(&ciphertext);
    Ok(stored)
}

pub(crate) fn decrypt(stored: &[u8], key: &[u8; 32], aad: &[u8]) -> Result<Vec<u8>> {
    let (nonce_bytes, ciphertext) = stored
        .split_at_checked(24)
        .filter(|(_, ciphertext)| ciphertext.len() >= 16)
        .ok_or(Error::Crypto("encrypted block is truncated"))?;
    let nonce = XNonce::from(<[u8; 24]>::try_from(nonce_bytes).expect("nonce length checked"));
    XChaCha20Poly1305::new(&(*key).into())
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| Error::Crypto("authentication failed"))
}
