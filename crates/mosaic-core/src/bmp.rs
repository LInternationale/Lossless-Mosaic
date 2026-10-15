use crate::domain::{Error, Rect, Result, MAX_BYTES, TILE};

#[derive(Clone)]
pub struct Bitmap {
    pub(crate) bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bpp: u16,
    pub(crate) offset: usize,
    pub(crate) stride: usize,
    top_down: bool,
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
impl Bitmap {
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < 54 || bytes.len() > MAX_BYTES || &bytes[..2] != b"BM" {
            return Err(Error::Bitmap("需为 64 MiB 以内的 Windows BMP"));
        }
        let dib = u32_at(&bytes, 14) as usize;
        if ![40, 108, 124].contains(&dib) || 14 + dib > bytes.len() {
            return Err(Error::Bitmap("仅支持 40/108/124 字节 DIB 头"));
        }
        let width = u32_at(&bytes, 18) as i32;
        let signed_height = u32_at(&bytes, 22) as i32;
        let bpp = u16_at(&bytes, 28);
        if width <= 0
            || signed_height == 0
            || signed_height == i32::MIN
            || ![24, 32].contains(&bpp)
            || u16_at(&bytes, 26) != 1
            || u32_at(&bytes, 30) != 0
        {
            return Err(Error::Bitmap("仅支持未压缩 BI_RGB、24/32 位图片"));
        }
        let height = signed_height.unsigned_abs();
        let stride = (width as usize * bpp as usize).div_ceil(32) * 4;
        let offset = u32_at(&bytes, 10) as usize;
        let end = (height as usize)
            .checked_mul(stride)
            .and_then(|v| offset.checked_add(v))
            .ok_or(Error::Bitmap("尺寸溢出"))?;
        if offset < 14 + dib || end > bytes.len() || width as u64 * height as u64 > 16_000_000 {
            return Err(Error::Bitmap("像素数据越界或超过 1600 万像素"));
        }
        Ok(Self {
            bytes,
            width: width as u32,
            height,
            bpp,
            offset,
            stride,
            top_down: signed_height < 0,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn index(&self, x: u32, y: u32) -> usize {
        let row = if self.top_down {
            y
        } else {
            self.height - 1 - y
        };
        self.offset + row as usize * self.stride + x as usize * (self.bpp / 8) as usize
    }
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for y in 0..self.height {
            for x in 0..self.width {
                let i = self.index(x, y);
                out.extend_from_slice(&[self.bytes[i + 2], self.bytes[i + 1], self.bytes[i], 255]);
            }
        }
        out
    }
    pub(crate) fn read(&self, r: Rect) -> Vec<u8> {
        self.read_channels(r, false)
    }
    pub(crate) fn write(&mut self, r: Rect, data: &[u8]) {
        self.write_channels(r, data, false);
    }
    pub(crate) fn read_channels(&self, r: Rect, alpha: bool) -> Vec<u8> {
        let n = if alpha && self.bpp == 32 { 4 } else { 3 };
        let mut out = Vec::with_capacity(r.width as usize * r.height as usize * n);
        for y in r.y..r.y + r.height {
            for x in r.x..r.x + r.width {
                let i = self.index(x, y);
                out.extend_from_slice(&self.bytes[i..i + n]);
            }
        }
        out
    }
    pub(crate) fn write_channels(&mut self, r: Rect, data: &[u8], alpha: bool) {
        let n = if alpha && self.bpp == 32 { 4 } else { 3 };
        let mut k = 0;
        for y in r.y..r.y + r.height {
            for x in r.x..r.x + r.width {
                let i = self.index(x, y);
                self.bytes[i..i + n].copy_from_slice(&data[k..k + n]);
                k += n;
            }
        }
    }
    pub(crate) fn tiles(&self) -> Vec<Rect> {
        let mut out = vec![];
        for y in (0..self.height).step_by(TILE as usize) {
            for x in (0..self.width).step_by(TILE as usize) {
                out.push(Rect {
                    x,
                    y,
                    width: TILE.min(self.width - x),
                    height: TILE.min(self.height - y),
                });
            }
        }
        out
    }
    pub(crate) fn alpha(&self) -> Vec<u8> {
        let mut out = vec![];
        for y in 0..self.height {
            for x in 0..self.width {
                out.push(self.bytes[self.index(x, y) + 3]);
            }
        }
        out
    }
    pub(crate) fn put_alpha(&mut self, data: &[u8]) {
        for (k, v) in data.iter().enumerate() {
            let i = self.index(k as u32 % self.width, k as u32 / self.width) + 3;
            self.bytes[i] = *v;
        }
    }
    pub(crate) fn carrier(&self) -> Result<Self> {
        let len = 54 + self.width as usize * self.height as usize * 4;
        let mut bytes = vec![0; len];
        bytes[..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&(len as u32).to_le_bytes());
        bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&self.width.to_le_bytes());
        bytes[22..26].copy_from_slice(&(-(self.height as i32)).to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&32u16.to_le_bytes());
        let mut out = Self::parse(bytes)?;
        let all = Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        };
        out.write(all, &self.read(all));
        Ok(out)
    }
    // All original bytes except BGR, in physical order. Includes padding and DIB extensions.
    pub(crate) fn skeleton(&self) -> Vec<u8> {
        let mut out = self.bytes[..self.offset].to_vec();
        for row in 0..self.height as usize {
            let start = self.offset + row * self.stride + self.width as usize * 3;
            out.extend_from_slice(&self.bytes[start..self.offset + (row + 1) * self.stride]);
        }
        out.extend_from_slice(&self.bytes[self.offset + self.height as usize * self.stride..]);
        out
    }
    pub(crate) fn from_skeleton(s: &[u8], rgb: &Self, original_len: usize) -> Result<Self> {
        if s.len() < 54 || original_len > MAX_BYTES {
            return Err(Error::Metadata);
        }
        let offset = u32_at(s, 10) as usize;
        if offset > s.len() || offset > original_len {
            return Err(Error::Metadata);
        }
        let mut bytes = vec![0; original_len];
        bytes[..offset].copy_from_slice(&s[..offset]);
        let mut out = Self::parse(bytes)?;
        if out.bpp != 24 || out.width != rgb.width || out.height != rgb.height {
            return Err(Error::Metadata);
        }
        let pad = out.stride - out.width as usize * 3;
        let mut k = offset;
        for row in 0..out.height as usize {
            let i = out.offset + row * out.stride + out.width as usize * 3;
            if k + pad > s.len() {
                return Err(Error::Metadata);
            }
            out.bytes[i..i + pad].copy_from_slice(&s[k..k + pad]);
            k += pad;
        }
        let tail = out.offset + out.height as usize * out.stride;
        if out.bytes.len() - tail != s.len() - k {
            return Err(Error::Metadata);
        }
        out.bytes[tail..].copy_from_slice(&s[k..]);
        let all = Rect {
            x: 0,
            y: 0,
            width: out.width,
            height: out.height,
        };
        out.write(all, &rgb.read(all));
        Ok(out)
    }
}
