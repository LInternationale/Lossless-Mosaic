use crate::{Error, Rect, Result};
use serde::{Deserialize, Serialize};

/// A canonical, row-major bitmap of grid cells. Painting is assignment, never XOR.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridSelection {
    pub block_size: u32,
    pub bits: Vec<u8>,
}
impl GridSelection {
    pub fn empty(width: u32, height: u32, block_size: u32) -> Result<Self> {
        if width == 0 || height == 0 || width as u64 * height as u64 > 16_000_000 {
            return Err(Error::Region("网格图片尺寸无效"));
        }
        if ![1, 8, 16, 32, 64].contains(&block_size) {
            return Err(Error::Region(
                "蒙版精度须为 1 像素，或 8、16、32、64 像素网格",
            ));
        }
        let count = width.div_ceil(block_size) as usize * height.div_ceil(block_size) as usize;
        Ok(Self {
            block_size,
            bits: vec![0; count.div_ceil(8)],
        })
    }
    pub fn validate(&self, width: u32, height: u32) -> Result<()> {
        let expected = Self::empty(width, height, self.block_size)?;
        if self.bits.len() != expected.bits.len() {
            return Err(Error::Region("网格位图尺寸错误"));
        }
        let count =
            width.div_ceil(self.block_size) as usize * height.div_ceil(self.block_size) as usize;
        if !count.is_multiple_of(8) && self.bits.last().is_some_and(|v| *v >> (count % 8) != 0) {
            return Err(Error::Region("网格位图存在越界位"));
        }
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|v| *v == 0)
    }
    pub fn from_regions(
        width: u32,
        height: u32,
        block_size: u32,
        regions: &[Rect],
    ) -> Result<Self> {
        let mut out = Self::empty(width, height, block_size)?;
        let cols = width.div_ceil(block_size) as usize;
        for r in regions {
            r.validate(width, height)?;
            for y in r.y / block_size..=(r.y + r.height - 1) / block_size {
                for x in r.x / block_size..=(r.x + r.width - 1) / block_size {
                    let i = y as usize * cols + x as usize;
                    out.bits[i / 8] |= 1 << (i % 8);
                }
            }
        }
        Ok(out)
    }
    pub fn rects(&self, width: u32, height: u32) -> Result<Vec<Rect>> {
        self.validate(width, height)?;
        let cols = width.div_ceil(self.block_size);
        let count = cols * height.div_ceil(self.block_size);
        if self.block_size == 1 {
            let mut out: Vec<Rect> = vec![];
            let mut previous: std::collections::HashMap<(u32, u32), usize> =
                std::collections::HashMap::new();
            for y in 0..height {
                let mut current = std::collections::HashMap::new();
                let mut x = 0;
                while x < width {
                    let i = (y * width + x) as usize;
                    if self.bits[i / 8] & (1 << (i % 8)) == 0 {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    x += 1;
                    while x < width {
                        let i = (y * width + x) as usize;
                        if self.bits[i / 8] & (1 << (i % 8)) == 0 {
                            break;
                        }
                        x += 1;
                    }
                    let key = (start, x - start);
                    let index = if let Some(&index) = previous.get(&key) {
                        out[index].height += 1;
                        index
                    } else {
                        out.push(Rect {
                            x: start,
                            y,
                            width: x - start,
                            height: 1,
                        });
                        out.len() - 1
                    };
                    current.insert(key, index);
                }
                previous = current;
            }
            return Ok(out);
        }

        Ok((0..count)
            .filter(|i| self.bits[*i as usize / 8] & (1 << (*i % 8)) != 0)
            .map(|i| {
                let x = i % cols * self.block_size;
                let y = i / cols * self.block_size;
                Rect {
                    x,
                    y,
                    width: self.block_size.min(width - x),
                    height: self.block_size.min(height - y),
                }
            })
            .collect())
    }
}
