//! Byte-exact reversible BMP masking. No filesystem or UI dependencies.
mod application;
mod bmp;
mod crypto;
mod detection;
mod domain;
pub use detection::{assess_encryption, detect_blocks, BlockSuggestions, EncryptionPreview};
mod envelope;
mod selection;
pub use selection::GridSelection;

pub use application::{
    inspect, protect, protect_grid, restore, suggest_grid, transform_blocks, transform_grid,
    Inspection, Protected,
};
pub use bmp::Bitmap;
pub use domain::{Error, Mode, Rect, Result, TILE};
