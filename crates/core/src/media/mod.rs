//! Image decode, encode, resize; EXIF read; RAW handling

pub mod exif_jpeg;
pub mod image_ops;
pub mod meta;
pub mod slices;
pub mod text;

pub use image_ops::{
    apply_orientation, decode, decode_oriented, dimensions_for_megapixels, downscale_to_max_edge,
    encode_jpeg_within, reencode_preserving_exif, resize, QUALITY_LADDER,
};
pub use meta::{
    best_date, is_video, normalise_datetime, read_meta, DateSet, ExifWriter, MediaMeta,
    Orientation, TagSource,
};
