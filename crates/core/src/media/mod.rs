//! Image decode, encode, resize; EXIF read; RAW handling

pub mod image_ops;
pub mod meta;
pub mod slices;

pub use meta::{read_meta, DateSet, ExifWriter, MediaMeta, Orientation, TagSource};
