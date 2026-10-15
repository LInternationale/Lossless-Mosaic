use crate::{
    crypto,
    domain::{Error, Mode, Rect, Result, MAX_BYTES, MAX_RECORDS},
};
use serde::{Deserialize, Serialize};

pub(crate) const MAGIC: &[u8; 8] = b"LMMASK01";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub rect: Option<Rect>,
    pub nonce: [u8; 24],
    pub tag: [u8; 16],
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub version: u32,
    pub mode: Mode,
    pub salt: [u8; 16],
    pub original_len: usize,
    pub original_hash: [u8; 32],
    pub protected_hash: [u8; 32],
    pub skeleton: Vec<u8>,
    pub records: Vec<Record>,
    pub auth_nonce: [u8; 24],
    pub auth_tag: [u8; 16],
}
impl Manifest {
    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|_| Error::Metadata)
    }
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() > MAX_BYTES / 4 {
            return Err(Error::Metadata);
        }
        let m: Self = serde_json::from_slice(data).map_err(|_| Error::Metadata)?;
        if m.mode == Mode::Entropy
            || m.version != 1
            || m.records.is_empty()
            || m.records.len() > MAX_RECORDS
            || m.original_len > MAX_BYTES
        {
            return Err(Error::Metadata);
        }
        Ok(m)
    }
    pub fn sign(&mut self, key: &[u8; 32]) -> Result<()> {
        self.auth_tag = [0; 16];
        self.auth_tag = crypto::seal(key, &self.auth_nonce, &self.encode()?, &mut [])?;
        Ok(())
    }
    pub fn verify(&self, key: &[u8; 32]) -> Result<()> {
        let mut m = self.clone();
        m.auth_tag = [0; 16];
        crypto::open(key, &m.auth_nonce, &m.encode()?, &mut [], &self.auth_tag)
    }
}
pub(crate) fn pack(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(MAGIC);
    out
}
pub(crate) fn unpack(bytes: &[u8]) -> Result<Option<(&[u8], &[u8])>> {
    if bytes.len() < 16 || &bytes[bytes.len() - 8..] != MAGIC {
        return Ok(None);
    }
    let len = u64::from_le_bytes(bytes[bytes.len() - 16..bytes.len() - 8].try_into().unwrap());
    if len > bytes.len() as u64 - 16 {
        return Err(Error::Metadata);
    }
    let start = bytes.len() - 16 - len as usize;
    Ok(Some((&bytes[..start], &bytes[start..bytes.len() - 16])))
}
