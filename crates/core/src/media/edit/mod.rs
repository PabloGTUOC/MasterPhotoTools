//! The colour and adjustment engine (ED-1, ED-2).

pub mod lut;
pub mod pipeline;

pub use lut::{Lut, MAX_LUT_SIZE, MIN_LUT_SIZE};
pub use pipeline::{
    apply_recipe, decode_image, linear_to_srgb, linear_to_u16, linear_to_u8, srgb_to_linear,
    tone_weights, u16_to_linear, u8_to_linear, AdjustmentRecipe, ImageBuffer, LinearBuffer, LutRef,
};
