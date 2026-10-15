use crate::GridSelection;
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
    pub source_bpp: u16,
    pub mode: Option<Mode>,
    pub rgba: Vec<u8>,
    pub suggested_blocks: Vec<Rect>,
    pub selection: Option<GridSelection>,
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
    let (mode, selection, source_bpp) = match extract(bytes, sidecar) {
        Ok((_, m)) => {
            let selection = if let Some(s) = m.selection {
                s.validate(b.width, b.height)?;
                s
            } else {
                GridSelection::from_regions(
                    b.width,
                    b.height,
                    1,
                    &m.records.iter().filter_map(|r| r.rect).collect::<Vec<_>>(),
                )?
            };
            (
                Some(m.mode),
                Some(selection),
                if m.mode == Mode::Alpha && !m.skeleton.is_empty() {
                    24
                } else {
                    b.bpp
                },
            )
        }
        Err(Error::MissingMetadata) => (None, None, b.bpp),
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
        source_bpp,
        mode,
        rgba: b.rgba(),
        suggested_blocks,
        selection,
    })
}
pub fn protect(bytes: &[u8], password: &str, mode: Mode, regions: &[Rect]) -> Result<Protected> {
    protect_internal(bytes, password, mode, regions, None)
}
/// Grid brush format v2 uses one authenticated payload and a compact cell bitmap.
pub fn protect_grid(
    bytes: &[u8],
    password: &str,
    mode: Mode,
    selection: &GridSelection,
) -> Result<Protected> {
    let source = Bitmap::parse(bytes.to_vec())?;
    selection.validate(source.width, source.height)?;
    if selection.is_empty() {
        return Ok(Protected {
            bitmap: bytes.to_vec(),
            sidecar: None,
        });
    }
    if mode == Mode::Entropy {
        return Ok(Protected {
            bitmap: transform_grid(bytes, password, selection)?,
            sidecar: None,
        });
    }
    protect_internal(bytes, password, mode, &[], Some(selection))
}
fn protect_internal(
    bytes: &[u8],
    password: &str,
    mode: Mode,
    regions: &[Rect],
    selection: Option<&GridSelection>,
) -> Result<Protected> {
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
    if selection.is_none() && (regions.is_empty() || regions.len() > MAX_RECORDS) {
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
    if let Some(selection) = selection {
        let rects = selection.rects(b.width, b.height)?;
        let nonce = crypto::random()?;
        let mut pixels = read_grid(&b, &rects, mode == Mode::Sidecar);
        let tag = crypto::seal(&key, &nonce, &grid_aad(mode), &mut pixels)?;
        write_grid(&mut b, &rects, &pixels, mode == Mode::Sidecar);
        records.push(Record {
            rect: None,
            nonce,
            tag,
        });
    }
    let mut normalized = source.clone();
    if mode == Mode::Alpha && source.bpp == 32 {
        normalized.put_alpha(&vec![255; source.width as usize * source.height as usize]);
    }
    let mut m = Manifest {
        version: if selection.is_some() { 2 } else { 1 },
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
        selection: selection.cloned(),
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
    if let Some(selection) = &m.selection {
        let rects = selection.rects(b.width, b.height)?;
        let rec = &m.records[0];
        if rec.rect.is_some() || selection.is_empty() {
            return Err(Error::Metadata);
        }
        let mut pixels = read_grid(&b, &rects, m.mode == Mode::Sidecar);
        crypto::open(&key, &rec.nonce, &grid_aad(m.mode), &mut pixels, &rec.tag)?;
        write_grid(&mut b, &rects, &pixels, m.mode == Mode::Sidecar);
    } else {
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
    let b = Bitmap::parse(bytes.to_vec())?;
    if regions.is_empty() || regions.len() > 65536 {
        return Err(Error::Region("请选择要处理的网格"));
    }
    let selection = GridSelection::from_regions(b.width, b.height, 32, regions)?;
    transform_grid(bytes, password, &selection)
}
pub fn transform_grid(bytes: &[u8], password: &str, selection: &GridSelection) -> Result<Vec<u8>> {
    if selection.block_size == 1 {
        return Err(Error::Region("零附加数据模式须使用 8/16/32/64 像素区块"));
    }
    let mut b = Bitmap::parse(bytes.to_vec())?;
    let rects = selection.rects(b.width, b.height)?;
    let context = format!(
        "lossless-mosaic/blocks/v1/{}/{}/{}",
        b.width, b.height, b.bpp
    );
    let hash = digest(context.as_bytes());
    let salt: [u8; 16] = hash[..16].try_into().unwrap();
    let key = crypto::derive(password, &salt)?;
    for r in rects {
        let hash = digest(&aad(Mode::Entropy, r));
        let nonce: [u8; 24] = hash[..24].try_into().unwrap();
        let mut pixels = b.read_channels(r, true);
        crypto::stream(&key, &nonce, &mut pixels);
        b.write_channels(r, &pixels, true);
    }
    Ok(b.bytes)
}
pub fn suggest_grid(bytes: &[u8], block_size: u32) -> Result<GridSelection> {
    Ok(crate::detect_blocks(bytes, block_size)?.selection)
}
fn grid_aad(mode: Mode) -> Vec<u8> {
    format!("lossless-mosaic/grid/v2/{mode:?}").into_bytes()
}
fn read_grid(b: &Bitmap, rects: &[Rect], alpha: bool) -> Vec<u8> {
    rects
        .iter()
        .flat_map(|r| b.read_channels(*r, alpha))
        .collect()
}
fn write_grid(b: &mut Bitmap, rects: &[Rect], data: &[u8], alpha: bool) {
    let n = if alpha && b.bpp == 32 { 4 } else { 3 };
    let mut offset = 0;
    for r in rects {
        let len = r.width as usize * r.height as usize * n;
        b.write_channels(*r, &data[offset..offset + len], alpha);
        offset += len;
    }
}
