use crate::{
    bmp::Bitmap,
    crypto,
    domain::{Error, Mode, Rect, Result, MAX_BYTES, MAX_RECORDS},
    envelope::{self, Manifest, Record, MAGIC},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub struct Protected {
    pub bitmap: Vec<u8>,
    pub sidecar: Option<Vec<u8>>,
}
#[derive(Serialize)]
pub struct Inspection {
    pub width: u32,
    pub height: u32,
    pub bpp: u16,
    pub mode: Option<Mode>,
    pub rgba: Vec<u8>,
    pub suggested_blocks: Vec<Rect>,
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn aad(mode: Mode, r: Rect) -> Vec<u8> {
    format!(
        "lossless-mosaic/v1/{mode:?}/{},{},{},{}",
        r.x, r.y, r.width, r.height
    )
    .into_bytes()
}
fn alpha_extract(b: &Bitmap) -> Result<Option<Manifest>> {
    if b.bpp != 32 {
        return Ok(None);
    }
    let a = b.alpha();
    if a.len() < 16 || &a[..8] != MAGIC {
        return Ok(None);
    }
    let len = u64::from_le_bytes(a[8..16].try_into().unwrap());
    if len > a.len() as u64 - 16 {
        return Err(Error::Metadata);
    }
    let m = Manifest::decode(&a[16..16 + len as usize])?;
    if m.mode != Mode::Alpha {
        return Err(Error::Metadata);
    }
    Ok(Some(m))
}
fn extract(bytes: &[u8], sidecar: Option<&[u8]>) -> Result<(Bitmap, Manifest)> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::Bitmap("文件过大"));
    }
    if let Some(s) = sidecar {
        let (_, m) = envelope::unpack(s)?.ok_or(Error::Metadata)?;
        let m = Manifest::decode(m)?;
        if m.mode != Mode::Sidecar {
            return Err(Error::Metadata);
        }
        return Ok((Bitmap::parse(bytes.to_vec())?, m));
    }
    let mut b = Bitmap::parse(bytes.to_vec())?;
    if let Some(m) = alpha_extract(&b)? {
        let zeros = vec![0; b.width as usize * b.height as usize];
        b.put_alpha(&zeros);
        return Ok((b, m));
    }
    Err(Error::MissingMetadata)
}
pub fn inspect(bytes: &[u8], sidecar: Option<&[u8]>) -> Result<Inspection> {
    let b = Bitmap::parse(bytes.to_vec())?;
    let mode = match extract(bytes, sidecar) {
        Ok((_, m)) => Some(m.mode),
        Err(Error::MissingMetadata) => None,
        Err(e) => return Err(e),
    };
    let suggested_blocks = if b.tiles().len() <= 65536 {
        b.tiles()
            .into_iter()
            .filter(|r| entropy(&b.read(*r)) > 7.6)
            .collect()
    } else {
        vec![]
    };
    Ok(Inspection {
        width: b.width,
        height: b.height,
        bpp: b.bpp,
        mode,
        rgba: b.rgba(),
        suggested_blocks,
    })
}
pub fn protect(bytes: &[u8], password: &str, mode: Mode, regions: &[Rect]) -> Result<Protected> {
    let source = Bitmap::parse(bytes.to_vec())?;
    if envelope::unpack(bytes)?.is_some() || alpha_extract(&source)?.is_some() {
        return Err(Error::AlreadyProtected);
    }
    if mode == Mode::Entropy {
        return Ok(Protected {
            bitmap: transform_blocks(bytes, password, regions)?,
            sidecar: None,
        });
    }
    if regions.is_empty() || regions.len() > MAX_RECORDS {
        return Err(Error::Region("请选择 1–256 个不重叠区域"));
    }
    for (i, r) in regions.iter().enumerate() {
        r.validate(source.width, source.height)?;
        if regions[..i].iter().any(|b| r.overlaps(*b)) {
            return Err(Error::Region("选区不可重叠"));
        }
    }

    let mut b = if mode == Mode::Alpha && source.bpp == 24 {
        source.carrier()?
    } else {
        source.clone()
    };
    if mode == Mode::Alpha {
        b.put_alpha(&vec![0; b.width as usize * b.height as usize]);
    }
    let selected = regions.to_vec();
    let salt = crypto::random()?;
    let key = crypto::derive(password, &salt)?;
    let mut records = vec![];
    for r in selected {
        let nonce = crypto::random()?;
        let mut pixels = b.read_channels(r, mode == Mode::Sidecar);
        let tag = crypto::seal(&key, &nonce, &aad(mode, r), &mut pixels)?;
        b.write_channels(r, &pixels, mode == Mode::Sidecar);
        records.push(Record {
            rect: Some(r),
            nonce,
            tag,
        });
    }
    let mut normalized = source.clone();
    if mode == Mode::Alpha && source.bpp == 32 {
        normalized.put_alpha(&vec![255; source.width as usize * source.height as usize]);
    }
    let mut m = Manifest {
        version: 1,
        mode,
        salt,
        original_len: bytes.len(),
        original_hash: digest(normalized.bytes()),
        protected_hash: digest(b.bytes()),
        skeleton: if mode == Mode::Alpha && source.bpp == 24 {
            source.skeleton()
        } else {
            vec![]
        },
        records,
        auth_nonce: crypto::random()?,
        auth_tag: [0; 16],
    };
    m.sign(&key)?;
    let meta = m.encode()?;
    let sidecar = match mode {
        Mode::Alpha => {
            let mut carrier = MAGIC.to_vec();
            carrier.extend_from_slice(&(meta.len() as u64).to_le_bytes());
            carrier.extend_from_slice(&meta);
            if carrier.len() > b.width as usize * b.height as usize {
                return Err(Error::AlphaCapacity);
            }
            b.put_alpha(&carrier);
            None
        }
        Mode::Sidecar => Some(envelope::pack(&meta)),
        Mode::Entropy => return Err(Error::Metadata),
    };
    if b.bytes.len() > MAX_BYTES {
        return Err(Error::Bitmap("输出超过 64 MiB"));
    }
    Ok(Protected {
        bitmap: b.bytes,
        sidecar,
    })
}
fn entropy(data: &[u8]) -> f64 {
    let mut counts = [0usize; 256];
    for b in data {
        counts[*b as usize] += 1;
    }
    counts
        .into_iter()
        .filter(|n| *n > 0)
        .map(|n| {
            let p = n as f64 / data.len() as f64;
            -p * p.log2()
        })
        .sum()
}
pub fn restore(bytes: &[u8], sidecar: Option<&[u8]>, password: &str) -> Result<Vec<u8>> {
    let (mut b, m) = extract(bytes, sidecar)?;
    let key = crypto::derive(password, &m.salt)?;
    m.verify(&key)?;
    if digest(b.bytes()) != m.protected_hash {
        return Err(Error::Authentication);
    }
    if m.mode == Mode::Entropy {
        return Err(Error::Metadata);
    }
    let mut seen = vec![];
    for rec in &m.records {
        let r = rec.rect.ok_or(Error::Metadata)?;
        r.validate(b.width, b.height)?;
        if seen.iter().any(|a: &Rect| a.overlaps(r)) {
            return Err(Error::Metadata);
        }
        seen.push(r);
        let mut pixels = b.read_channels(r, m.mode == Mode::Sidecar);
        crypto::open(&key, &rec.nonce, &aad(m.mode, r), &mut pixels, &rec.tag)?;
        b.write_channels(r, &pixels, m.mode == Mode::Sidecar);
    }
    let restored = if m.mode == Mode::Alpha && !m.skeleton.is_empty() {
        Bitmap::from_skeleton(&m.skeleton, &b, m.original_len)?.bytes
    } else {
        if m.mode == Mode::Alpha {
            b.put_alpha(&vec![255; b.width as usize * b.height as usize]);
        }
        b.bytes
    };
    if restored.len() != m.original_len || digest(&restored) != m.original_hash {
        return Err(Error::Authentication);
    }
    Ok(restored)
}

/// No metadata, no tag, no salt storage: the same password/geometry uses the same stream.
/// Applying to the same selected cells twice recovers every byte, including alpha.
pub fn transform_blocks(bytes: &[u8], password: &str, regions: &[Rect]) -> Result<Vec<u8>> {
    let mut b = Bitmap::parse(bytes.to_vec())?;
    if regions.is_empty() || regions.len() > 65536 {
        return Err(Error::Region("请选择要处理的网格"));
    }
    for r in regions {
        r.validate(b.width, b.height)?;
    }
    let context = format!(
        "lossless-mosaic/blocks/v1/{}/{}/{}",
        b.width, b.height, b.bpp
    );
    let hash = digest(context.as_bytes());
    let salt: [u8; 16] = hash[..16].try_into().unwrap();
    let key = crypto::derive(password, &salt)?;
    let columns = b.width.div_ceil(32) as usize;
    let rows = b.height.div_ceil(32) as usize;
    let mut selected = vec![false; columns * rows];
    for r in regions {
        for y in r.y / 32..=(r.y + r.height - 1) / 32 {
            for x in r.x / 32..=(r.x + r.width - 1) / 32 {
                selected[y as usize * columns + x as usize] = true;
            }
        }
    }
    for r in b
        .tiles()
        .into_iter()
        .filter(|r| selected[(r.y / 32) as usize * columns + (r.x / 32) as usize])
    {
        let hash = digest(&aad(Mode::Entropy, r));
        let nonce: [u8; 24] = hash[..24].try_into().unwrap();
        let mut pixels = b.read_channels(r, true);
        crypto::stream(&key, &nonce, &mut pixels);
        b.write_channels(r, &pixels, true);
    }
    Ok(b.bytes)
}
