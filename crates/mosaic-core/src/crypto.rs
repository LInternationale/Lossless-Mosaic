use crate::domain::{Error, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{AeadInPlace, KeyInit},
    Tag, XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroizing;

pub(crate) fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0; N];
    OsRng
        .try_fill_bytes(&mut b)
        .map_err(|_| Error::Internal("系统随机数不可用".into()))?;
    Ok(b)
}
pub(crate) fn derive(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>> {
    if password.is_empty() || password.len() > 1024 {
        return Err(Error::Password);
    }
    let mut key = Zeroizing::new([0; 32]);
    let params = Params::new(65536, 3, 1, Some(32)).map_err(|e| Error::Internal(e.to_string()))?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(key)
}
pub(crate) fn seal(
    key: &[u8; 32],
    nonce: &[u8; 24],
    aad: &[u8],
    data: &mut [u8],
) -> Result<[u8; 16]> {
    let tag = XChaCha20Poly1305::new(key.into())
        .encrypt_in_place_detached(XNonce::from_slice(nonce), aad, data)
        .map_err(|_| Error::Authentication)?;
    Ok(tag.into())
}
pub(crate) fn open(
    key: &[u8; 32],
    nonce: &[u8; 24],
    aad: &[u8],
    data: &mut [u8],
    tag: &[u8; 16],
) -> Result<()> {
    XChaCha20Poly1305::new(key.into())
        .decrypt_in_place_detached(XNonce::from_slice(nonce), aad, data, Tag::from_slice(tag))
        .map_err(|_| Error::Authentication)
}

/// Metadata-free mode deliberately has no authentication and uses a deterministic stream.
pub(crate) fn stream(key: &[u8; 32], nonce: &[u8; 24], data: &mut [u8]) {
    use chacha20::cipher::{KeyIvInit, StreamCipher};
    chacha20::XChaCha20::new(key.into(), nonce.into()).apply_keystream(data);
}
