//! The colour and adjustment engine (ED-1, ED-2, ED-4).

pub mod color;
pub mod curves;
pub mod geometry;
pub mod grading;
pub mod grain;
pub mod hsl;
pub mod looks;
pub mod lut;
pub mod pipeline;
pub mod preview;
pub mod vignette;

pub use color::{
    linear_srgb_to_oklab, oklab_to_linear_srgb, rotate_hue_oklch, rotate_hue_oklch_sincos,
};
pub use curves::{CurvePoint, CurveTable, MonotoneSpline, ToneCurves, ToneCurvesTable};
pub use geometry::{
    aspect_ratio_from_preset, largest_inscribed_rect, preset_to_normalized_crop, Geometry,
    NormalizedCrop, MIN_CROP_DIMENSION,
};
pub use grading::{ColorGrading, ColorWheel, CompiledGradingTable};
pub use grain::{CompiledGrain, FilmGrain};
pub use hsl::{CompiledHslTable, HslAdjustments, HslBand};
pub use looks::{aces_narkowicz, apply_looks_linear, LookEffects, HALATION_TINT};
pub use lut::{Lut, MAX_LUT_SIZE, MIN_LUT_SIZE};
pub use pipeline::{
    apply_recipe, apply_recipe_with_orientation, decode_image, linear_to_srgb, linear_to_u16,
    linear_to_u8, srgb_to_linear, tone_weights, u16_to_linear, u8_to_linear, validate_lut,
    white_black_weights, AdjustmentRecipe, ImageBuffer, LinearBuffer, LutRef, SRGB_TO_LINEAR_U16,
    SRGB_TO_LINEAR_U8,
};
pub use preview::{
    downscale_image_buffer, encode_preview_frame, render_rgba_frame, PreviewSession, PreviewStage,
    RgbaFrame, DRAG_MAX_EDGE, PREVIEW_FRAME_HEADER_LEN, SETTLE_MAX_EDGE,
};
pub use vignette::{CompiledVignette, Vignette};
