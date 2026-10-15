//! Byte-exact reversible BMP masking. No filesystem or UI dependencies.
mod application;
mod bmp;
mod crypto;
mod domain;
mod envelope;

pub use application::{inspect, protect, restore, transform_blocks, Inspection, Protected};
pub use bmp::Bitmap;
pub use domain::{Error, Mode, Rect, Result, TILE};
