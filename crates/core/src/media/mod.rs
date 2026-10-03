//! Image decode, encode, resize; EXIF read; RAW handling

pub mod edit;
pub mod exif_jpeg;
pub mod histogram;
pub mod image_ops;
pub mod jpeg;
pub mod meta;
pub mod raw;
pub mod segment;
pub mod slices;
pub mod text;

pub use edit::{
    apply_recipe, decode_image, validate_lut, AdjustmentRecipe, ImageBuffer, LinearBuffer, Lut,
    LutRef, PreviewSession, PreviewStage, RgbaFrame,
};
pub use histogram::{histogram, Histogram};
pub use image_ops::{
    apply_orientation, decode, decode_oriented, dimensions_for_megapixels, downscale_to_max_edge,
    encode_jpeg_within, reencode_preserving_exif, resize, QUALITY_LADDER,
};
pub use meta::{
    best_date, is_video, normalise_datetime, read_meta, DateSet, ExifWriter, MediaMeta,
    Orientation, TagSource,
};
pub use raw::{is_raw, raw_to_jpeg, DerivedJpeg, RawSource};
