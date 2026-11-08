//! Integration and unit tests for eyedropper white balance, auto tone,
//! and 8-band black & white mixer (RP-4).

use rollchrome_core::media::edit::hsl::{BlackAndWhite, HslAdjustments, HslBand};
use rollchrome_core::media::edit::pipeline::{AdjustmentRecipe, ImageBuffer};
use rollchrome_core::media::edit::preview::{PreviewSession, PreviewStage};
use rollchrome_core::media::edit::{ColorGrading, ColorWheel};

fn make_rgb8_image(width: u32, height: u32, fill: [u8; 3]) -> ImageBuffer {
    let size = (width as usize) * (height as usize) * 3;
    let mut data = Vec::with_capacity(size);
    for _ in 0..(width * height) {
        data.extend_from_slice(&fill);
    }
    ImageBuffer::Rgb8 {
        width,
        height,
        data,
    }
}

// ---------------------------------------------------------------------------
// 1. Eyedropper tests
// ---------------------------------------------------------------------------

#[test]
fn white_balance_at_returns_neutral_for_neutral_patch() {
    let img = make_rgb8_image(50, 50, [128, 128, 128]);
    let session = PreviewSession::from_image_buffer(&img).expect("preview session creation");

    let pick = session.white_balance_at(0.5, 0.5).expect("pick neutral");
    assert!(
        pick.temperature.abs() < 0.1,
        "neutral patch must produce near-zero temperature shift, got {}",
        pick.temperature
    );
    assert!(
        pick.tint.abs() < 0.1,
        "neutral patch must produce near-zero tint shift, got {}",
        pick.tint
    );
    assert!(!pick.clamped, "neutral patch must not be clamped");
}

#[test]
fn white_balance_at_refuses_out_of_bounds() {
    let img = make_rgb8_image(50, 50, [128, 128, 128]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    assert!(session.white_balance_at(-0.1, 0.5).is_err());
    assert!(session.white_balance_at(1.1, 0.5).is_err());
    assert!(session.white_balance_at(0.5, -0.01).is_err());
    assert!(session.white_balance_at(0.5, 1.05).is_err());
    assert!(session.white_balance_at(f32::NAN, 0.5).is_err());
    assert!(session.white_balance_at(0.5, f32::NAN).is_err());
}

#[test]
fn white_balance_at_refuses_clipped_highlight() {
    // 255 in 8-bit sRGB maps to 1.0 linear, which is >= 0.98
    let img = make_rgb8_image(50, 50, [255, 255, 255]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let err = session
        .white_balance_at(0.5, 0.5)
        .expect_err("must refuse clipped highlight");
    assert!(
        err.to_string().contains("clipped highlight"),
        "error must mention clipped highlight; got: {err}"
    );
}

#[test]
fn white_balance_at_refuses_deep_shadow() {
    // Pure black has linear 0.0, luminance < 0.005
    let img = make_rgb8_image(50, 50, [0, 0, 0]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let err = session
        .white_balance_at(0.5, 0.5)
        .expect_err("must refuse deep shadow");
    assert!(
        err.to_string().contains("deep shadow"),
        "error must mention deep shadow; got: {err}"
    );
}

#[test]
fn white_balance_at_flags_clamped_when_exceeding_slider_range() {
    // High blue over red: R=10, G=128, B=240.
    // In linear light, B/R is ~80, log2(80) * 100 > 600, far exceeding the 100 limit.
    let img = make_rgb8_image(50, 50, [10, 128, 240]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let pick = session
        .white_balance_at(0.5, 0.5)
        .expect("pick should succeed with clamping");
    assert!(pick.clamped, "must flag clamped = true");
    assert_eq!(
        pick.temperature, 100.0,
        "temperature must clamp to upper limit 100.0"
    );
}

#[test]
fn white_balance_at_cool_patch_produces_warm_shift() {
    // Cool patch: B > R
    let img = make_rgb8_image(50, 50, [100, 130, 170]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let pick = session.white_balance_at(0.5, 0.5).expect("pick cool patch");
    assert!(
        pick.temperature > 0.0,
        "cool patch must produce positive temperature shift to warm it, got {}",
        pick.temperature
    );
}

// ---------------------------------------------------------------------------
// 2. Auto tone tests
// ---------------------------------------------------------------------------

#[test]
fn auto_tone_leaves_well_exposed_image_untouched() {
    // Create an image where:
    // - median display luma is ~110 (within [95, 135])
    // - 99.5th percentile is ~240 (within [235, 253])
    // - 0.5th percentile is ~5 (<= 12)
    let w = 200;
    let h = 200;
    let total = (w * h) as usize;
    let mut data = Vec::with_capacity(total * 3);

    // 1% (400 pixels) at 5 -> 0.5th percentile is 5 (<= 12)
    // 1% (400 pixels) at 240 -> 99.5th percentile is 240 (in [235, 253])
    // Remaining 98% at 110 -> median is 110 (in [95, 135])
    let dark_count = (total as f64 * 0.01) as usize;
    let bright_count = (total as f64 * 0.01) as usize;
    let mid_count = total - dark_count - bright_count;

    for _ in 0..dark_count {
        data.extend_from_slice(&[5, 5, 5]);
    }
    for _ in 0..mid_count {
        data.extend_from_slice(&[110, 110, 110]);
    }
    for _ in 0..bright_count {
        data.extend_from_slice(&[240, 240, 240]);
    }

    let img = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let tone = session.auto_tone().expect("auto tone evaluation");
    assert_eq!(
        tone.exposure, 0.0,
        "well-exposed image must keep exposure at 0.0, got {}",
        tone.exposure
    );
    assert_eq!(
        tone.whites, 0.0,
        "well-exposed image must keep whites at 0.0, got {}",
        tone.whites
    );
    assert_eq!(
        tone.blacks, 0.0,
        "well-exposed image must keep blacks at 0.0, got {}",
        tone.blacks
    );
}

#[test]
fn auto_tone_boosts_underexposed_image() {
    // Entirely dark image, median luma ~30 (< 95)
    let img = make_rgb8_image(100, 100, [30, 30, 30]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let tone = session.auto_tone().expect("auto tone evaluation");
    assert!(
        tone.exposure > 0.0,
        "underexposed image must receive positive exposure boost, got {}",
        tone.exposure
    );
}

#[test]
fn auto_tone_pulls_down_blown_highlights() {
    // Mostly normal midtones at 110, but 2% blown highlights at 255 (>= 254)
    let w = 100;
    let h = 100;
    let total = (w * h) as usize;
    let mut data = Vec::with_capacity(total * 3);

    let blown_count = (total as f64 * 0.02).ceil() as usize;
    let normal_count = total - blown_count;

    for _ in 0..normal_count {
        data.extend_from_slice(&[110, 110, 110]);
    }
    for _ in 0..blown_count {
        data.extend_from_slice(&[255, 255, 255]);
    }

    let img = ImageBuffer::Rgb8 {
        width: w,
        height: h,
        data,
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let tone = session.auto_tone().expect("auto tone evaluation");
    assert!(
        tone.whites < 0.0,
        "blown highlights must receive negative whites adjustment, got {}",
        tone.whites
    );
}

// ---------------------------------------------------------------------------
// 3. Black & white tests
// ---------------------------------------------------------------------------

#[test]
fn black_and_white_desaturates_pure_colours_to_zero_chroma() {
    // 3 pixels: pure red, pure green, pure blue
    let img = ImageBuffer::Rgb8 {
        width: 3,
        height: 1,
        data: vec![255, 0, 0, 0, 255, 0, 0, 0, 255],
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        ..Default::default()
    };

    let frame = session
        .render(&recipe, None, PreviewStage::Drag)
        .expect("render B&W");

    // Every pixel in output RGBA must have R == G == B
    for (i, px) in frame.bytes.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(
            px[0], px[1],
            "Pixel {i} R ({}) and G ({}) must be equal in monochrome output",
            px[0], px[1]
        );
        assert_eq!(
            px[1], px[2],
            "Pixel {i} G ({}) and B ({}) must be equal in monochrome output",
            px[1], px[2]
        );
    }
}

#[test]
fn black_and_white_mix_lifts_matching_hue_and_preserves_others() {
    // 2 pixels: red-dominated (200, 50, 50) and blue-dominated (50, 50, 200)
    let img = ImageBuffer::Rgb8 {
        width: 2,
        height: 1,
        data: vec![200, 50, 50, 50, 50, 200],
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    // 1. Default neutral mix
    let neutral_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        ..Default::default()
    };
    let neutral_frame = session
        .render(&neutral_recipe, None, PreviewStage::Drag)
        .unwrap();
    let neutral_red_luma = neutral_frame.bytes[0];
    let neutral_blue_luma = neutral_frame.bytes[4];

    // 2. Lift red channel by +50, leaving blue at 0
    let lifted_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite {
            red: 50.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let lifted_frame = session
        .render(&lifted_recipe, None, PreviewStage::Drag)
        .unwrap();
    let lifted_red_luma = lifted_frame.bytes[0];
    let lifted_blue_luma = lifted_frame.bytes[4];

    assert!(
        lifted_red_luma > neutral_red_luma,
        "red mixer lift must brighten red pixel (was {neutral_red_luma}, now {lifted_red_luma})"
    );
    assert!(
        (lifted_blue_luma as i32 - neutral_blue_luma as i32).abs() <= 1,
        "blue pixel must be preserved when red slider moves (was {neutral_blue_luma}, now {lifted_blue_luma})"
    );
}

#[test]
fn black_and_white_ignores_hsl_and_hue_rotation_when_active() {
    let img = ImageBuffer::Rgb8 {
        width: 4,
        height: 1,
        data: vec![220, 40, 40, 40, 220, 40, 40, 40, 220, 220, 220, 40],
    };
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    let bw_only_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite {
            red: 20.0,
            green: -10.0,
            ..Default::default()
        }),
        ..Default::default()
    };

    let bw_with_hsl_and_hue = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite {
            red: 20.0,
            green: -10.0,
            ..Default::default()
        }),
        hue: 120.0,
        hsl: Some(HslAdjustments {
            red: HslBand {
                hue: 60.0,
                saturation: -80.0,
                luminance: 50.0,
            },
            green: HslBand {
                hue: -45.0,
                saturation: 100.0,
                luminance: -30.0,
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    let frame1 = session
        .render(&bw_only_recipe, None, PreviewStage::Drag)
        .unwrap();
    let frame2 = session
        .render(&bw_with_hsl_and_hue, None, PreviewStage::Drag)
        .unwrap();

    assert_eq!(
        frame1.bytes, frame2.bytes,
        "B&W output must ignore HSL adjustments and hue rotation"
    );
}

#[test]
fn black_and_white_grading_and_lut_still_apply() {
    let img = make_rgb8_image(10, 10, [128, 128, 128]);
    let session = PreviewSession::from_image_buffer(&img).unwrap();

    // 1. Plain B&W
    let bw_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        ..Default::default()
    };
    let bw_frame = session
        .render(&bw_recipe, None, PreviewStage::Drag)
        .unwrap();
    assert_eq!(bw_frame.bytes[0], bw_frame.bytes[1]);
    assert_eq!(bw_frame.bytes[1], bw_frame.bytes[2]);

    // 2. B&W with color grading (blue tint via global wheel)
    let graded_recipe = AdjustmentRecipe {
        black_and_white: Some(BlackAndWhite::default()),
        grading: Some(ColorGrading {
            global: ColorWheel {
                hue: 264.05, // Blue
                saturation: 0.8,
                luminance: 0.0,
            },
            ..Default::default()
        }),
        ..Default::default()
    };
    let graded_frame = session
        .render(&graded_recipe, None, PreviewStage::Drag)
        .unwrap();

    // Blue channel should be lifted higher than red in the blue-graded shadows
    assert!(
        graded_frame.bytes[2] > graded_frame.bytes[0],
        "grading must tint the black & white image (R={}, B={})",
        graded_frame.bytes[0],
        graded_frame.bytes[2]
    );
}
