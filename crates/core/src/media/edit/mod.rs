//! The colour and adjustment engine (ED-1, ED-2, ED-4).

pub mod color;
pub mod lut;
pub mod pipeline;
pub mod preview;

pub use color::{
    linear_srgb_to_oklab, oklab_to_linear_srgb, rotate_hue_oklch, rotate_hue_oklch_sincos,
};
pub use lut::{Lut, MAX_LUT_SIZE, MIN_LUT_SIZE};
pub use pipeline::{
    apply_recipe, decode_image, linear_to_srgb, linear_to_u16, linear_to_u8, srgb_to_linear,
    tone_weights, u16_to_linear, u8_to_linear, validate_lut, white_black_weights, AdjustmentRecipe,
    ImageBuffer, LinearBuffer, LutRef, SRGB_TO_LINEAR_U16, SRGB_TO_LINEAR_U8,
};
pub use preview::{
    downscale_image_buffer, render_rgba_frame, PreviewSession, PreviewStage, RgbaFrame,
    DRAG_MAX_EDGE, SETTLE_MAX_EDGE,
};
