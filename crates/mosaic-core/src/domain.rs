use serde::{Deserialize, Serialize};

pub const TILE: u32 = 32;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RECORDS: usize = 256;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("不支持或已损坏的 BMP：{0}")]
    Bitmap(&'static str),
    #[error("无效选区：{0}")]
    Region(&'static str),
    #[error("密码不能为空，且 UTF-8 长度不能超过 1024 字节")]
    Password,
    #[error("密码错误，或图片/还原信息已被修改")]
    Authentication,
    #[error("还原信息无效或不属于本文件")]
    Metadata,
    #[error("没有找到还原信息；附加文件模式请同时导入 .lmm 文件")]
    MissingMetadata,
    #[error("α 通道容量不足以容纳还原信息；请改用附加数据模式")]
    AlphaCapacity,
    #[error("该文件已打码，请先还原后重新选择区域")]
    AlreadyProtected,
    #[error("操作失败：{0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Alpha,
    Sidecar,
    Entropy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub fn validate(self, width: u32, height: u32) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.x.checked_add(self.width).is_none_or(|v| v > width)
            || self.y.checked_add(self.height).is_none_or(|v| v > height)
        {
            return Err(Error::Region("选区必须位于图片内且面积大于零"));
        }
        Ok(())
    }
    pub fn overlaps(self, b: Self) -> bool {
        self.x < b.x + b.width
            && b.x < self.x + self.width
            && self.y < b.y + b.height
            && b.y < self.y + self.height
    }
}
